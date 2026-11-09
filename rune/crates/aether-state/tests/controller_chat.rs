use aether_core::agent::{profile, storage, AgentSettings};
use aether_state::Controller;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Once,
    time::{Duration, Instant},
};

fn isolate() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-controller-tests-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
}
fn tick(c: &mut Controller, done: impl Fn(&Controller) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(c) {
        c.poll();
        assert!(
            Instant::now() < deadline,
            "controller timed out: {:?}",
            c.model
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn real_http_chat_streams_and_persists_without_a_gui_waker() {
    isolate();
    let workspace = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut socket = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "no HTTP request");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let (start, length) = loop {
            let mut buf = [0; 4096];
            let n = socket.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(at) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..at]);
                let length = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                break (at + 4, length);
            }
        };
        while bytes.len() < start + length {
            let mut buf = [0; 4096];
            let n = socket.read(&mut buf).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        let request: Value = serde_json::from_slice(&bytes[start..start + length]).unwrap();
        let saved = std::fs::read_dir(storage::config_dir().join("sessions"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| std::fs::read(e.path()).ok())
            .filter_map(|b| serde_json::from_slice::<Value>(&b).ok())
            .any(|d| {
                d["messages"].as_array().is_some_and(|m| {
                    m.iter().any(|m| m["content"] == "tracer prompt")
                        && m.last().is_some_and(|m| m["isStreaming"] == true)
                })
            });
        tx.send((request, saved)).unwrap();
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"native\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    });
    let mut c = Controller::new_with_workspace(workspace.path().to_string_lossy().into()).unwrap();
    c.model.settings = serde_json::from_value::<AgentSettings>(json!({"providers":[{"id":"local","kind":"openai","baseUrl":format!("http://{address}/v1"),"model":"fixture"}],"activeProviderId":"local","self_improvement_enabled":false,"skillPaths":[],"autoCompactionEnabled":false,"reviewConfig":{"createDir":workspace.path().join("review-fixture").to_string_lossy()}})).unwrap();
    c.new_session();
    let id = c.model.current.as_ref().unwrap().session.id.clone();
    c.send("tracer prompt".into()).unwrap();
    assert!(c.model.busy);
    tick(&mut c, |c| !c.model.busy);
    assert!(c.model.error.is_none(), "{:?}", c.model.error);
    let current = c.model.current.as_ref().unwrap();
    assert_eq!(current.messages.last().unwrap().content, "Hello native");
    assert_eq!(
        current.messages.last().unwrap().run_status.as_deref(),
        Some("completed")
    );
    let stored = c.runtime.block_on(storage::load_session(&id)).unwrap();
    assert_eq!(stored.messages.last().unwrap().content, "Hello native");
    assert_eq!(stored.messages.len(), 2);
    assert!(c
        .model
        .window
        .as_ref()
        .is_some_and(|w| !w.revision.is_empty()));
    let (request, persisted_before_http) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(
        persisted_before_http,
        "placeholder must be durable before provider dispatch"
    );
    let conversation: Vec<_> = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] != "system")
        .collect();
    assert_eq!(
        conversation.len(),
        1,
        "assistant placeholder leaked into context"
    );
    assert_eq!(conversation[0]["content"], "tracer prompt");
    server.join().unwrap();
}

#[test]
fn navigation_pages_metadata_preview_and_unknown_results_are_safe() {
    isolate();
    let workspace = tempfile::tempdir().unwrap();
    let mut c = Controller::new_with_workspace(workspace.path().to_string_lossy().into()).unwrap();
    let original = c.model.current.as_ref().unwrap().session.id.clone();
    let id = format!("paged_{}", uuid::Uuid::new_v4());
    let data = serde_json::from_value(json!({"session":{"id":id,"title":"paged","createdAt":1,"updatedAt":1,"messageCount":120},"messages":(0..120).map(|n|json!({"id":format!("m{n}"),"role":if n%2==0 {"user"} else {"assistant"},"content":format!("message {n}"),"timestamp":n})).collect::<Vec<_>>(),"artifacts":[]})).unwrap();
    c.runtime.block_on(storage::save_session(data)).unwrap();
    let abandoned = c.open_session(&id);
    c.open_session(&original);
    tick(&mut c, |c| c.model.results.contains_key(&abandoned));
    assert!(!c.model.loading);
    assert_eq!(c.model.current.as_ref().unwrap().session.id, original);
    c.open_session(&id);
    tick(&mut c, |c| !c.model.loading);
    assert_eq!(c.model.current.as_ref().unwrap().messages.len(), 50);
    assert_eq!(c.model.window.as_ref().unwrap().offset, 70);
    c.older().unwrap();
    tick(&mut c, |c| !c.model.loading_older);
    assert_eq!(c.model.current.as_ref().unwrap().messages.len(), 100);
    assert_eq!(c.model.window.as_ref().unwrap().offset, 20);
    let pin = c.dispatch("toggle_pin_session", json!({"sessionId":id}));
    tick(&mut c, |c| {
        c.model.results.contains_key(&pin) && !c.model.busy
    });
    assert!(c.model.current.as_ref().unwrap().session.pinned);
    let preview = c.dispatch(
        "load_session_around",
        json!({"sessionId":id,"messageId":"m2","limit":10}),
    );
    tick(&mut c, |c| c.model.results.contains_key(&preview));
    assert!(c.model.preview.is_some());
    assert_eq!(c.model.current.as_ref().unwrap().messages.len(), 100);
    assert!(c.send("no preview writes".into()).is_err());
    c.close_preview();
    let unknown = c.dispatch("not_a_script", json!({}));
    tick(&mut c, |c| c.model.results.contains_key(&unknown));
    assert!(c
        .take_result(unknown)
        .unwrap()
        .error
        .unwrap()
        .contains("Unknown command"));
    // Explicit raw page-save is still routed through the canonical cursor.
    let mut data = c.model.current.clone().unwrap();
    data.messages
        .last_mut()
        .unwrap()
        .content
        .push_str(" appended");
    let save = c.dispatch(
        "save_session_page",
        json!({"data":data,"window":c.model.window}),
    );
    tick(&mut c, |c| c.model.results.contains_key(&save));
    assert!(
        c.model.results[&save].error.is_none(),
        "{:?}",
        c.model.results[&save]
    );
    let saved = c.runtime.block_on(storage::load_session(&id)).unwrap();
    assert_eq!(saved.messages.len(), 120);
    assert!(saved
        .messages
        .last()
        .unwrap()
        .content
        .ends_with(" appended"));
}

#[test]
fn failed_send_preserves_composer_and_workspace_change_binds_empty_chat() {
    isolate();
    let workspace = tempfile::tempdir().unwrap();
    let mut c = Controller::new_with_workspace(String::new()).unwrap();
    c.model.draft = "keep me".into();
    assert!(c.send("keep me".into()).is_err());
    assert_eq!(c.model.draft, "keep me");
    let op = c.dispatch(
        "set_workspace_cwd",
        json!({"newCwd":workspace.path().to_string_lossy()}),
    );
    tick(&mut c, |c| c.model.results.contains_key(&op));
    assert_eq!(
        c.model
            .current
            .as_ref()
            .unwrap()
            .session
            .workspace_cwd
            .as_deref(),
        Some(c.model.workspace.as_str()),
        "empty chat must follow workspace selection"
    );
}

#[test]
fn abort_before_registration_persists_cancelled_turn_without_http() {
    isolate();
    let workspace = tempfile::tempdir().unwrap();
    let mut c = Controller::new_with_workspace(workspace.path().to_string_lossy().into()).unwrap();
    c.model.settings=serde_json::from_value(json!({"providers":[{"id":"local","baseUrl":"http://127.0.0.1:9/v1","model":"unused"}],"activeProviderId":"local","self_improvement_enabled":false})).unwrap();
    let id = c.model.current.as_ref().unwrap().session.id.clone();
    c.send("cancel before start".into()).unwrap();
    c.abort().unwrap();
    tick(&mut c, |c| !c.model.busy);
    assert!(c.model.error.is_none(), "{:?}", c.model.error);
    assert_eq!(
        c.model.run_status.as_ref().unwrap().phase,
        aether_state::RunPhase::Cancelled
    );
    assert_eq!(
        c.runtime
            .block_on(storage::load_session(&id))
            .unwrap()
            .messages[1]
            .run_status
            .as_deref(),
        Some("cancelled")
    );
}

#[test]
fn external_cancellation_is_bounded_and_shutdown_flushes_the_partial_turn() {
    isolate();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "provider request never arrived");
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) => panic!("{e}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let n = socket.read(&mut buffer).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
            if let Some(at) = request.windows(4).position(|b| b == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..at]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() >= at + 4 + length {
                    break;
                }
            }
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"partial answer\"},\"finish_reason\":null}]}\n\n").unwrap();
        started_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(5));
    });
    let mut c = Controller::new_with_workspace(String::new()).unwrap();
    c.model.settings = serde_json::from_value(json!({"providers":[{"id":"local","kind":"openai","baseUrl":format!("http://{address}/v1"),"model":"fixture"}],"activeProviderId":"local","self_improvement_enabled":false,"autoCompactionEnabled":false})).unwrap();
    c.send("cancel externally".into()).unwrap();
    tick(&mut c, |_| started_rx.try_recv().is_ok());
    tick(&mut c, |c| {
        c.model.current.as_ref().unwrap().messages[1].content == "partial answer"
    });
    let status = c.model.run_status.clone().unwrap();
    let start = Instant::now();
    // Cancellation originates outside Controller::abort, as with a host exit.
    c.backend.runner.abort(&status.run_id).unwrap();
    c.begin_shutdown();
    tick(&mut c, Controller::shutdown_ready);
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "external cancellation did not interrupt provider wait"
    );
    let saved = c
        .runtime
        .block_on(storage::load_session(&status.session_id))
        .unwrap();
    assert_eq!(saved.messages[1].content, "partial answer");
    assert_eq!(saved.messages[1].is_streaming, Some(false));
    assert_eq!(saved.messages[1].run_status.as_deref(), Some("cancelled"));
    release_tx.send(()).unwrap();
    server.join().unwrap();
}
