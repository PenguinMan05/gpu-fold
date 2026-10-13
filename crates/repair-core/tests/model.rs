use repair_core::{
    model::{LocalHttpProvider, ModelProvider, StreamDecoder},
    process,
    types::Settings,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

#[test]
fn stream_accepts_split_unicode_and_rejects_truncated_output() {
    let event = "data: {\"choices\":[{\"delta\":{\"content\":\"café\"}}]}\n\ndata: [DONE]\n\n";
    let mut decoder = StreamDecoder::default();
    let mut observed = String::new();
    for b in event.as_bytes() {
        decoder.feed(&[*b], &mut |s| observed.push_str(s)).unwrap();
    }
    assert!(decoder.done);
    assert_eq!(decoder.text, "café");
    assert_eq!(observed, "café");
    assert!(StreamDecoder::default()
        .feed(
            b"data: {\"choices\":[{\"finish_reason\":\"length\"}]}\n",
            &mut |_| {}
        )
        .is_err());
}
#[test]
fn model_rejects_remote_endpoints_before_connecting() {
    for endpoint in [
        "https://api.example.org/chat",
        "http://localhost:1234/chat",
        "http://user:secret@127.0.0.1:1234/chat",
        "http://127.0.0.1/chat?token=abc",
    ] {
        let mut settings = Settings::default();
        settings.endpoint = endpoint.into();
        assert!(LocalHttpProvider { settings }
            .generate("test", &process::cancel_token())
            .is_err());
    }
}
#[test]
fn local_http_adapter_handles_actual_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 8192];
        let _ = socket.read(&mut request).unwrap();
        let body="data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"no_fix\\\":true}\"}}]}\n\ndata: [DONE]\n\n";
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let mut settings = Settings::default();
    settings.endpoint = format!("http://{address}/v1/chat/completions");
    let result = LocalHttpProvider { settings }
        .generate("fixture", &process::cancel_token())
        .unwrap();
    assert_eq!(result, "{\"no_fix\":true}");
    server.join().unwrap();
}
#[test]
fn cancellation_disconnects_a_stalled_model_within_one_second() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 8192];
        let _ = socket.read(&mut request);
        std::thread::sleep(Duration::from_millis(600));
    });
    let cancel = process::cancel_token();
    let stop = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let mut settings = Settings::default();
    settings.endpoint = format!("http://{address}/v1/chat/completions");
    let start = Instant::now();
    assert!(LocalHttpProvider { settings }
        .generate("fixture", &cancel)
        .is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
}
