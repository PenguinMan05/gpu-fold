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

#[test]
fn a_wrong_citation_line_is_anchored_to_unique_exact_source_without_another_model_call() {
    let mut fixture=Fixture::new(); let mut reply=good(); reply["citations"][0]["start_line"]=json!(13);
    let (settings,server)=server(vec![reply]);
    fixture.engine.analyze_once(&mut fixture.run,&settings,&process::cancel_token()).unwrap();
    assert_eq!(fixture.run.attempts,1);
    assert_eq!(fixture.run.proposal.as_ref().unwrap().citations[0].start_line,1);
    assert!(fixture.engine.conversation().iter().any(|entry|entry.text.contains("anchored citation")));
    server.join().unwrap();
}

#[test]
fn source_reads_are_executed_read_only_and_supplied_to_the_following_attempt() {
    let mut fixture=Fixture::new();
    std::fs::write(fixture.run.root.join("pristine/src/secondary.rs"),"pub fn related_behavior() -> bool { true }\n").unwrap();
    let investigate=json!({"explanation":"I will inspect the related implementation before choosing a fix.","citations":[],"planned_files":[],"edits":[],"reproduction":null,"no_fix":false,"tool_requests":[{"server":"gpufold-source","name":"read_file","arguments":{"path":"src/secondary.rs","start_line":1,"line_count":80}}]});
    let (settings,server)=server(vec![investigate,good()]);
    assert!(fixture.engine.analyze_once(&mut fixture.run,&settings,&process::cancel_token()).unwrap_err().downcast_ref::<model::RetryableProposalError>().is_some());
    assert_eq!(fixture.run.inspections[0].path,"src/secondary.rs");
    fixture.engine.analyze_once(&mut fixture.run,&settings,&process::cancel_token()).unwrap();
    let requests=server.join().unwrap(); let second=requests[1]["messages"][1]["content"].as_str().unwrap();
    assert!(second.contains("requested inspection") && second.contains("related_behavior"));
    assert_eq!(fixture.run.state,State::AwaitPlan);
    assert!(fixture.run.dialogue.iter().any(|entry|entry.role==DialogueRole::Assistant && entry.text.contains("inspect the related")));
    assert!(fixture.run.dialogue.iter().any(|entry|entry.text.contains("Opened src/secondary.rs")));
}

#[test]
fn source_search_finds_call_sites_and_cannot_execute_queries_or_escape_the_snapshot() {
    use repair_core::inspection;
    let mut fixture=Fixture::new();
    let request=ToolRequest { server:inspection::SERVER.into(),name:"search_code".into(),arguments:json!({"query":"add","path_prefix":"src"}).as_object().unwrap().clone() };
    let result=inspection::execute(&mut fixture.run,&request,&process::cancel_token()).unwrap();
    assert!(result.contains("src/lib.rs:1"));
    let outside=ToolRequest { name:"read_file".into(),arguments:json!({"path":"../../etc/passwd"}).as_object().unwrap().clone(),..request.clone() };
    assert!(inspection::execute(&mut fixture.run,&outside,&process::cancel_token()).is_err());
    let sentinel=fixture.root.join("must-not-exist");
    let literal=ToolRequest { arguments:json!({"query":format!("$(touch {})",sentinel.display())}).as_object().unwrap().clone(),..request };
    inspection::execute(&mut fixture.run,&literal,&process::cancel_token()).unwrap();
    assert!(!sentinel.exists());
}

#[test]
fn citation_repair_does_not_accept_invented_or_ambiguous_source() {
    use repair_core::inspection;
    let fixture=Fixture::new(); let root=fixture.run.root.join("pristine");
    let mut proposal:Proposal=serde_json::from_value(good()).unwrap();
    let original=std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    std::fs::write(root.join("src/lib.rs"),format!("{original}{original}")).unwrap();
    proposal.citations[0].start_line=13;
    assert!(inspection::anchor_citations(&root,&mut proposal,&["src/lib.rs".into()]).unwrap().is_empty());
    assert_eq!(proposal.citations[0].start_line,13);
    proposal.citations[0].snippet="invented source".into();
    assert!(inspection::anchor_citations(&root,&mut proposal,&["src/lib.rs".into()]).unwrap().is_empty());
    assert!(repair_core::validation::validate_proposal(&root,&proposal).is_err());
    std::fs::write(root.join("src/lib.rs"),&original).unwrap();
    proposal.citations[0].snippet=format!("13: {}",original.trim_end());
    inspection::anchor_citations(&root,&mut proposal,&["src/lib.rs".into()]).unwrap();
    assert_eq!(proposal.citations[0].snippet,original.trim_end());
    assert_eq!(proposal.citations[0].start_line,1);
}

#[test]
fn citation_anchoring_preserves_real_indentation_without_fuzzy_code_matching() {
    let fixture=Fixture::new(); let root=fixture.run.root.join("pristine");
    let mut p:Proposal=serde_json::from_value(good()).unwrap();
    std::fs::write(root.join("src/lib.rs"),"impl Example {\n    fn stylus() { exact_behavior(); }\n}\n").unwrap();
    p.citations[0].snippet="fn stylus() { exact_behavior(); }".into(); p.citations[0].start_line=13;
    repair_core::inspection::anchor_citations(&root,&mut p,&["src/lib.rs".into()]).unwrap();
    assert_eq!(p.citations[0].start_line,2);
    assert_eq!(p.citations[0].snippet,"    fn stylus() { exact_behavior(); }");
}

#[test]
fn repeated_invalid_patches_trigger_a_related_source_search() {
    let mut fixture=Fixture::new();
    std::fs::write(fixture.run.root.join("pristine/src/stylus.rs"),"pub fn stylus_pressure() -> u32 { 0 }\n").unwrap();
    std::fs::write(fixture.run.root.join("pristine/src/input.rs"),"pub fn process_stylus_input() {}\n").unwrap();
    let mut bad=good(); bad["citations"][0]["path"]=json!("src/stylus.rs"); bad["citations"][0]["snippet"]=json!("invented");
    fixture.run.model_outputs=vec![bad.to_string(),bad.to_string()]; fixture.run.attempts=2;
    let result=repair_core::inspection::broaden_repeated_attempt(&mut fixture.run,&process::cancel_token()).unwrap().unwrap();
    assert!(result.contains("Changed strategy") && result.contains("src/input.rs"));
    assert!(fixture.run.inspections.iter().any(|request|request.path=="src/input.rs"));
}
