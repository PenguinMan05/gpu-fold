use repair_core::{model, runtime, types::Settings};
use std::{io::Write, net::{TcpListener, TcpStream}, time::{Duration, Instant}};

fn temporary_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("gpufold-gguf-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
}

#[test]
fn a_model_file_is_sufficient_to_configure_a_private_connection() {
    let root = temporary_root();
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("local model.gguf");
    std::fs::File::create(&path).unwrap().write_all(b"GGUF\x03\0\0\0").unwrap();
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut settings = Settings::default();
    settings.endpoint = format!("http://{}/v1/chat/completions", occupied.local_addr().unwrap());
    settings.model_path = path.display().to_string();
    let prepared = runtime::local_gguf_settings(&settings).unwrap();
    assert_eq!(prepared.runtime_kind, "bundled");
    assert!(prepared.runtime_path.is_empty(), "no executable selection should be required");
    assert_ne!(prepared.endpoint, settings.endpoint, "do not reuse another server's port");
    assert_eq!(prepared.model_path, settings.model_path);
    assert_eq!(model::local_url(&prepared.endpoint).unwrap().host_str(), Some("127.0.0.1"));
    std::fs::write(&path, b"<html>not a model</html>").unwrap();
    assert!(runtime::local_gguf_settings(&settings).unwrap_err().to_string().contains("not a GGUF"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the bundled engine and the pinned CI GGUF test model"]
fn included_engine_loads_a_real_gguf_generates_tokens_and_unloads() {
    let model_path = std::env::var("GPUFOLD_TEST_GGUF").expect("set GPUFOLD_TEST_GGUF to the tiny test model");
    let root = temporary_root();
    std::fs::create_dir_all(&root).unwrap();
    let mut selected = Settings::default();
    selected.model_path = model_path;
    selected.cpu = true;
    selected.context_tokens = 512;
    selected.max_output_tokens = 1024;
    assert!(selected.runtime_path.is_empty());
    let settings = runtime::local_gguf_settings(&selected).unwrap();
    if std::env::var_os("GPUFOLD_TEST_INSTALLED").is_some() {
        assert_eq!(runtime::bundled_engine_path().unwrap(), std::path::PathBuf::from("/usr/lib/gpufold/engine/llama-server"));
    }
    let mut engine = runtime::ManagedRuntime::default();
    engine.load(&settings, &root, true).unwrap();
    let started = Instant::now();
    loop {
        if model::available_models(&settings).is_ok() { break; }
        assert!(engine.running(), "engine exited: {}", engine.log.lock().unwrap());
        assert!(started.elapsed() < Duration::from_secs(45), "engine did not load: {}", engine.log.lock().unwrap());
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut url = model::local_url(&settings.endpoint).unwrap();
    url.set_path("/completion");
    let http = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(20)).build().unwrap();
    let result: serde_json::Value = http.post(url).json(&serde_json::json!({"prompt":"Once upon a time","n_predict":8,"temperature":0,"seed":42})).send().unwrap().error_for_status().unwrap().json().unwrap();
    assert!(!result["content"].as_str().unwrap_or_default().is_empty(), "no model text: {result}");
    assert!(result["tokens_predicted"].as_u64().or_else(|| result["timings"]["predicted_n"].as_u64()).unwrap_or(0) > 0, "no generated tokens: {result}");
    let mut counted_settings = settings.clone();
    counted_settings.tokenizer_path = "/does-not-exist/tokenizer.json".into();
    let (count, exact) = model::token_count(&counted_settings, "A real model tokenizer counts this sentence.").unwrap();
    assert!(exact && count > 0, "the included engine must supply its own tokenizer");
    let expected = serde_json::json!({"explanation":"Schema test","citations":[],"planned_files":[],"edits":[],"reproduction":null,"no_fix":true,"tool_requests":[]});
    let mut schema = model::proposal_schema();
    for (key,value) in expected.as_object().unwrap() {
        schema["properties"][key] = serde_json::json!({"const":value});
    }
    let response = http.post(&settings.endpoint).json(&serde_json::json!({
        "model":"GPUFold-local","messages":[{"role":"user","content":format!("Return this JSON object: {expected}")}],
        "max_tokens":1024,"temperature":0,"seed":42,"stream":false,
        "response_format":{"type":"json_object","schema":schema}
    })).send().unwrap();
    let status = response.status();
    let body = response.text().unwrap();
    assert!(status.is_success(), "Schema chat HTTP {status}: {body}");
    let constrained: serde_json::Value = serde_json::from_str(&body).unwrap();
    let content = constrained["choices"][0]["message"]["content"].as_str().expect("chat content");
    let parsed = model::parse_proposal(content).unwrap_or_else(|error| panic!("Schema-constrained chat failed: {error:#}; full reply: {constrained}"));
    assert_eq!(parsed.explanation,"Schema test");
    assert!(parsed.no_fix);
    println!("PASS: bundled tokenizer and actual schema-constrained chat completion include all proposal fields.");
    let port = model::local_url(&settings.endpoint).unwrap().port().unwrap();
    engine.unload();
    assert!(!engine.running());
    assert!(TcpStream::connect_timeout(&std::net::SocketAddr::from(([127,0,0,1], port)), Duration::from_millis(200)).is_err());
    std::fs::remove_dir_all(root).unwrap();
    println!("PASS: only a local GGUF path was supplied; the included engine loaded it, generated real tokens, and unloaded cleanly.");
}
