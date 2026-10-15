use repair_core::{guidance, model, process, repository, types::*, Engine, Store};
use serde_json::{json, Value};
use std::{io::{Read, Write}, net::{TcpListener, TcpStream}, path::PathBuf, sync::{mpsc, atomic::Ordering}, time::{Duration, Instant}};

struct Fixture { engine: Engine, run: Run, root: PathBuf }
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("gpufold-resilience-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let engine = Engine::new(Store::open(&root).unwrap());
        let run = engine.fixture().unwrap();
        Self { engine, run, root }
    }
}
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); } }
fn good() -> Value { serde_json::from_str(include_str!("../../../fixtures/fix-add.json")).unwrap() }
fn read_request(socket: &mut TcpStream) -> Value {
    socket.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0;4096];
        let n = socket.read(&mut chunk).unwrap();
        assert!(n > 0, "request closed early");
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(header_end) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let length: usize = headers.lines().find_map(|s| s.to_ascii_lowercase().strip_prefix("content-length:").map(|s| s.trim().parse().unwrap())).unwrap();
            if bytes.len() >= header_end + 4 + length { return serde_json::from_slice(&bytes[header_end+4..header_end+4+length]).unwrap(); }
        }
    }
}
fn server(replies: Vec<Value>) -> (Settings, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for reply in replies {
            let (mut socket,_) = listener.accept().unwrap();
            requests.push(read_request(&mut socket));
            let body = json!({"choices":[{"finish_reason":"stop","message":{"content":reply.to_string()}}]}).to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        }
        requests
    });
    let mut settings = Settings::default();
    settings.model_service = "other".into();
    settings.endpoint = format!("http://{address}/v1/chat/completions");
    settings.context_tokens = 8192;
    (settings,handle)
}

#[test]
fn malformed_json_keeps_retrying_beyond_three_attempts_with_feedback_and_no_edits() {
    let mut fixture = Fixture::new();
    let mut missing = good(); missing.as_object_mut().unwrap().remove("explanation");
    let (settings,server) = server(vec![missing.clone(),missing.clone(),missing.clone(),missing,good()]);
    fixture.engine.analyze(&mut fixture.run,&settings,&process::cancel_token()).unwrap();
    let requests = server.join().unwrap();
    assert_eq!(fixture.run.attempts,5);
    assert_eq!(fixture.run.state,State::AwaitPlan);
    assert!(requests[1]["messages"][1]["content"].as_str().unwrap().contains("missing field `explanation`"));
    assert!(requests[0]["response_format"]["json_schema"]["schema"]["required"].as_array().unwrap().contains(&json!("explanation")));
    assert_eq!(std::fs::read_to_string(fixture.run.root.join("work/src/lib.rs")).unwrap(),include_str!("../../../fixtures/fixture-add/src/lib.rs"));
    let log = fixture.engine.activity.lock().unwrap();
    assert!(log.contains("Attempt 5") && log.contains("needs revision") && log.contains("Ready for your review"));
}

#[test]
fn invalid_citations_are_retried_and_saved_tickets_can_take_alternate_turns() {
    let mut fixture = Fixture::new();
    let mut invalid = good(); invalid["citations"][0]["snippet"] = json!("invented code");
    let (settings,server) = server(vec![invalid,good()]);
    let error = fixture.engine.analyze_once(&mut fixture.run,&settings,&process::cancel_token()).unwrap_err();
    assert!(error.downcast_ref::<model::RetryableProposalError>().is_some());
    assert_eq!(fixture.engine.store.get(&fixture.run.id).unwrap().attempts,1);
    // Simulate another queue item taking its turn before this saved item is retried.
    let other = fixture.engine.fixture().unwrap();
    assert_eq!(other.state,State::Pinned);
    fixture.engine.analyze_once(&mut fixture.run,&settings,&process::cancel_token()).unwrap();
    assert_eq!(fixture.run.state,State::AwaitPlan);
    assert!(server.join().unwrap()[1]["messages"][1]["content"].as_str().unwrap().contains("Citation does not match"));
}

#[test]
fn a_no_fix_answer_stays_unresolved_and_is_eligible_for_another_queue_turn() {
    let mut fixture = Fixture::new();
    let no_fix = json!({"explanation":"Not enough evidence yet","citations":[],"planned_files":[],"edits":[],"reproduction":null,"no_fix":true});
    let (settings,server) = server(vec![no_fix,good()]);
    assert!(fixture.engine.analyze_once(&mut fixture.run,&settings,&process::cancel_token()).is_err());
    assert!(fixture.run.proposal.is_none());
    fixture.engine.analyze_once(&mut fixture.run,&settings,&process::cancel_token()).unwrap();
    assert_eq!(fixture.run.state,State::AwaitPlan);
    server.join().unwrap();
}

#[test]
fn stop_interrupts_automatic_retry_wait_and_preserves_the_attempt() {
    let mut fixture = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx,rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket,_) = listener.accept().unwrap();
        read_request(&mut socket);
        let body = json!({"choices":[{"finish_reason":"stop","message":{"content":"{}"}}]}).to_string();
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        tx.send(()).unwrap();
    });
    let cancel = process::cancel_token(); let stop = cancel.clone();
    let stopper = std::thread::spawn(move || { rx.recv_timeout(Duration::from_secs(10)).unwrap(); std::thread::sleep(Duration::from_millis(100)); stop.store(true,Ordering::SeqCst); Instant::now() });
    let mut settings = Settings::default(); settings.model_service="other".into(); settings.context_tokens=8192; settings.endpoint=format!("http://{address}/v1/chat/completions");
    assert!(fixture.engine.analyze(&mut fixture.run,&settings,&cancel).is_err());
    assert!(stopper.join().unwrap().elapsed()<Duration::from_secs(1));
    assert_eq!(fixture.engine.store.get(&fixture.run.id).unwrap().state,State::Cancelled);
    assert_eq!(fixture.run.attempts,1);
    server.join().unwrap();
}

#[test]
fn oversized_issue_and_source_are_fitted_automatically_without_a_tokenizer_file() {
    let mut fixture = Fixture::new();
    fixture.run.task = format!("The add function is wrong. {}", "extra issue context ".repeat(800));
    std::fs::write(fixture.run.root.join("pristine/src/lib.rs"),format!("pub fn add(a: u32, b: u32) -> u32 {{ a - b }}\n{}", "// detail café\n".repeat(5000))).unwrap();
    let mut settings = Settings::default(); settings.model_service="other".into();
    let (prompt,paths,tokens,exact) = model::preview(&fixture.run,&settings).unwrap();
    assert!(!exact && tokens <= settings.context_tokens);
    assert!(prompt.contains("pub fn add") && prompt.contains("(excerpt)"));
    assert!(paths.contains(&"src/lib.rs".into()));
    assert!(settings.tokenizer_path.is_empty());
    let (tiny,_) = repository::context(&fixture.run.root.join("pristine"),&fixture.run.task,256).unwrap();
    assert!(tiny.len()<=256);
}

#[test]
fn project_guidance_is_scoped_and_actually_included_in_model_context() {
    let fixture = Fixture::new(); let root = fixture.run.root.join("pristine");
    std::fs::write(root.join("AGENTS.md"),"Use descriptive Rust names.").unwrap();
    std::fs::write(root.join("src/agents.md"),"In src, preserve the public API.").unwrap();
    std::fs::create_dir_all(root.join("unrelated")).unwrap();
    std::fs::write(root.join("unrelated/AGENTS.md"),"UNRELATED_SCOPE").unwrap();
    std::fs::create_dir_all(root.join(".github/instructions")).unwrap();
    std::fs::write(root.join(".github/instructions/rust.instructions.md"),"---\napplyTo: '**/*.rs'\n---\nKeep Rust edits focused.").unwrap();
    std::fs::write(root.join(".github/instructions/python.instructions.md"),"---\napplyTo: '**/*.py'\n---\nPYTHON_ONLY").unwrap();
    std::fs::create_dir_all(root.join(".codex/skills/repair")).unwrap();
    std::fs::write(root.join(".codex/skills/repair/SKILL.md"),"Check arithmetic edge cases.").unwrap();
    let (text,paths) = guidance::context(&root,&["src/lib.rs".into()],12000).unwrap();
    assert!(paths.contains(&"AGENTS.md".into()) && paths.contains(&"src/agents.md".into()) && paths.contains(&".codex/skills/repair/SKILL.md".into()));
    assert!(text.contains("Keep Rust edits focused") && !text.contains("UNRELATED_SCOPE") && !text.contains("PYTHON_ONLY"));
    assert!(text.find("Use descriptive").unwrap() < text.find("preserve the public API").unwrap());
    let mut settings = Settings::default(); settings.model_service="other".into(); settings.context_tokens=32000;
    let (prompt,_,_,_) = model::preview(&fixture.run,&settings).unwrap();
    assert!(prompt.contains("Use descriptive Rust names") && prompt.contains("Check arithmetic edge cases"));
    let policies = repository::scan(&root).unwrap().policies;
    assert!(policies.contains_key("src/agents.md") && policies.contains_key(".codex/skills/repair/SKILL.md"));
}

#[test]
fn response_wrappers_are_tolerated_without_inventing_missing_fields() {
    let text = good().to_string();
    assert!(model::parse_proposal(&format!("```json\n{text}\n```" )).is_ok());
    assert!(model::parse_proposal(&format!("<think>private trace {{not JSON}}</think>\n{text}")).is_ok());
    assert!(model::parse_proposal("{\"no_fix\":true}").is_err());
    assert_eq!(model::proposal_summary("{\"explanation\":\"Checking arithmetic"),"Checking arithmetic");
    assert_eq!(model::proposal_summary("{\"explanation\":\"Uses \\\"quoted\\\" text\",\"edits\":[]}"),"Uses \"quoted\" text");
    assert!(model::proposal_summary("<think>hidden analysis").is_empty());
}

#[test]
fn live_logs_and_attempt_history_remain_bounded_and_old_saved_attempts_deserialize() {
    let fixture = Fixture::new();
    for _ in 0..100 { fixture.engine.report_activity(&"café ".repeat(1000)); }
    assert!(fixture.engine.activity.lock().unwrap().len()<=64000);
    let mut old = serde_json::to_value(&fixture.run).unwrap(); old["attempts"]=json!(255);
    let mut run:Run=serde_json::from_value(old).unwrap(); run.attempts+=1;
    assert_eq!(run.attempts,256);
}

#[test]
fn unresolved_attempts_explore_another_window_of_a_large_source_file() {
    let fixture = Fixture::new();
    let source = (0..200).map(|i| format!("pub fn item_{i}() -> usize {{ {i} }}\n")).collect::<String>();
    std::fs::write(fixture.run.root.join("pristine/src/lib.rs"),source).unwrap();
    let (first,_) = repository::context_window(&fixture.run.root.join("pristine"),"investigate a bug",1000,0).unwrap();
    let (later,_) = repository::context_window(&fixture.run.root.join("pristine"),"investigate a bug",1000,1).unwrap();
    assert!(first.contains("item_0()"));
    assert!(later.contains("item_40()"));
    assert_ne!(first,later);
}

#[test]
fn an_empty_issue_fix_is_retried_instead_of_being_reported_as_done() {
    let mut fixture = Fixture::new();
    let mut empty = good(); empty["edits"]=json!([]); empty["reproduction"]=Value::Null;
    let (settings,server) = server(vec![empty,good()]);
    assert!(fixture.engine.analyze_once(&mut fixture.run,&settings,&process::cancel_token()).is_err());
    fixture.engine.analyze_once(&mut fixture.run,&settings,&process::cancel_token()).unwrap();
    assert!(!fixture.run.proposal.as_ref().unwrap().edits.is_empty());
    server.join().unwrap();
}

#[test]
fn actual_engine_window_overrides_a_larger_requested_context() {
    let listener=TcpListener::bind("127.0.0.1:0").unwrap(); let address=listener.local_addr().unwrap();
    let server=std::thread::spawn(move || {
        let (mut socket,_)=listener.accept().unwrap(); let mut request=[0;4096]; socket.read(&mut request).unwrap();
        let body=json!({"data":[{"id":"local","meta":{"n_ctx":2048}}]}).to_string();
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let mut selected=Settings::default(); selected.endpoint=format!("http://{address}/v1/chat/completions");
    let fitted=model::fit_loaded_context(&selected).unwrap();
    assert_eq!(fitted.context_tokens+fitted.max_output_tokens,1536);
    assert!(fitted.context_tokens>=512 && fitted.max_output_tokens>=256);
    server.join().unwrap();
}
