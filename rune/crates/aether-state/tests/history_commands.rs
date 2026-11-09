use aether_core::agent::{profile, storage};
use aether_state::Controller;
use serde_json::{json, Value};
use std::{
    sync::Once,
    time::{Duration, Instant},
};

fn controller() -> Controller {
    static PROFILE: Once = Once::new();
    PROFILE.call_once(|| {
        profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-history-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
    Controller::new_with_workspace(String::new()).unwrap()
}
fn wait(c: &mut Controller, op: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        c.poll();
        if let Some(result) = c.take_result(op) {
            assert!(result.error.is_none(), "{:?}", result.error);
            return result.value.unwrap();
        }
        assert!(Instant::now() < deadline, "operation {op} timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn saved(c: &mut Controller, count: usize) -> String {
    let mut data = c.model().current.clone().unwrap();
    data.session.id = format!("sess_{}", uuid::Uuid::new_v4());
    data.messages = (0..count).map(|n| serde_json::from_value(json!({"id":format!("m{n}"),"role":if n%2==0 {"user"} else {"assistant"},"content":format!("content-{n}"),"timestamp":n})).unwrap()).collect();
    data.artifacts = vec![serde_json::from_value(
        json!({"id":"hidden","type":"code","title":"hidden prefix","content":"keep","timestamp":1}),
    )
    .unwrap()];
    let id = data.session.id.clone();
    c.runtime.block_on(storage::save_session(data)).unwrap();
    c.new_session();
    let op = c.open_session(&id);
    wait(c, op);
    id
}
#[test]
fn undo_hydrates_hidden_prefix_and_restores_prompt_after_commit() {
    let mut c = controller();
    let id = saved(&mut c, 122);
    assert!(c.model().window.as_ref().unwrap().offset > 0);
    let op = c.undo().unwrap();
    assert!(
        c.model().busy,
        "history operation must reserve origin until acknowledgement"
    );
    assert!(c.send("do not overlap".into()).is_err());
    let result = wait(&mut c, op);
    assert_eq!(result["replay"]["content"], "content-120");
    assert_eq!(c.model().draft, "content-120");
    let canonical = c.runtime.block_on(storage::load_session(&id)).unwrap();
    assert_eq!(canonical.messages.len(), 120);
    assert_eq!(canonical.messages[0].id, "m0");
    assert_eq!(canonical.artifacts[0].id, "hidden");
    assert_eq!(c.model().current.as_ref().unwrap().messages.len(), 120);
    assert_eq!(c.model().window.as_ref().unwrap().offset, 0);
    assert!(!c.model().busy);
}

fn expect_error(c: &mut Controller, op: u64) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        c.poll();
        if let Some(result) = c.take_result(op) {
            return result.error.expect("expected rejection");
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn cas_rejects_stale_cursor_and_allows_full_truncation() {
    let mut c = controller();
    let id = saved(&mut c, 8);
    let full = c.runtime.block_on(storage::load_session(&id)).unwrap();
    let window = c.model.window.clone().unwrap();
    let mut trimmed = full.clone();
    trimmed.messages.truncate(2);
    let accepted = c
        .runtime
        .block_on(storage::replace_session_checked(
            trimmed.clone(),
            Some(window.clone()),
        ))
        .unwrap();
    assert_eq!(accepted.data.messages.len(), 2);
    assert_eq!(accepted.window.offset, 0);
    assert_eq!(accepted.window.total, 2);
    assert_ne!(accepted.window.revision, window.revision);
    assert!(c
        .runtime
        .block_on(storage::replace_session_checked(full, Some(window)))
        .unwrap_err()
        .contains("stale"));
    assert_eq!(
        c.runtime
            .block_on(storage::load_session(&id))
            .unwrap()
            .messages
            .len(),
        2
    );
    assert!(c
        .runtime
        .block_on(storage::replace_session_checked(trimmed, None))
        .is_err());
    assert!(c
        .runtime
        .block_on(
            c.backend
                .dispatch("replace_session_checked", json!({"data":accepted.data}))
        )
        .unwrap_err()
        .contains("expectedWindow"));
}
#[test]
fn navigation_suppresses_retry_and_edit_resend_but_updates_origin_cache() {
    for edit in [false, true] {
        let mut c = controller();
        let id = saved(&mut c, 8);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        c.model.settings = serde_json::from_value(json!({"providers":[{"id":"local","kind":"openai","baseUrl":format!("http://{}/v1",listener.local_addr().unwrap()),"model":"fixture"}],"activeProviderId":"local","self_improvement_enabled":false,"skillPaths":[]})).unwrap();
        let op = if edit {
            c.edit_and_resend("m6", "edited prompt").unwrap()
        } else {
            c.retry_turn("m7").unwrap()
        };
        c.new_session();
        let selected = c.model.current.as_ref().unwrap().session.id.clone();
        c.model.draft = "new session draft".into();
        let result = wait(&mut c, op);
        assert_eq!(result["canonicalCommitted"], true);
        assert_eq!(c.model.current.as_ref().unwrap().session.id, selected);
        assert_eq!(c.model.draft, "new session draft");
        assert!(c.model.attachments.is_empty());
        assert!(c.model.replay_image_inputs.is_empty());
        assert!(!c.model.busy);
        assert_eq!(
            c.runtime
                .block_on(storage::load_session(&id))
                .unwrap()
                .messages
                .len(),
            6
        );
        assert_eq!(
            c.open_session(&id),
            0,
            "acknowledgment updates inactive origin cache"
        );
        assert_eq!(c.model.current.as_ref().unwrap().messages.len(), 6);
        assert!(
            !c.model.busy,
            "no automatic provider request after navigation"
        );
        for _ in 0..20 {
            c.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "navigation must suppress HTTP, not merely suppress UI publication"
        );
    }
}
#[test]
fn ui_dispatch_routes_history_and_rejects_foreign_ids() {
    let mut c = controller();
    let id = saved(&mut c, 8);
    let op = c.dispatch("rollback_turn", json!({"sessionId":id,"messageId":"m3"}));
    wait(&mut c, op);
    assert_eq!(c.model.current.as_ref().unwrap().messages.len(), 4);
    let op = c.dispatch(
        "retry_turn",
        json!({"sessionId":"foreign","messageId":"m3"}),
    );
    assert!(expect_error(&mut c, op).contains("sessionId"));
    let op = c.dispatch(
        "edit_and_resend",
        json!({"sessionId":id,"messageId":"foreign","text":"bad"}),
    );
    assert!(expect_error(&mut c, op).contains("not in this session"));
    c.send("/undo".into()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while c.model.busy {
        c.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(c.model.draft, "content-2");
    assert!(c
        .model
        .last_command
        .as_ref()
        .unwrap()
        .value
        .as_ref()
        .unwrap()["replay"]
        .get("imageInputs")
        .is_some());
    assert_eq!(c.model.current.as_ref().unwrap().messages.len(), 2);
    let op = c.dispatch("clear_history", json!({"sessionId":id}));
    wait(&mut c, op);
    let full = c.runtime.block_on(storage::load_session(&id)).unwrap();
    assert!(full.messages.is_empty());
    assert!(full.artifacts.is_empty());
}
#[test]
fn real_http_retry_reuses_deleted_image_and_commits_before_provider() {
    use aether_core::agent::{images, AgentSettings};
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let mut c = controller();
    let workspace = tempfile::tempdir().unwrap();
    let image = workspace.path().join("gone.png");
    std::fs::write(&image, b"\x89PNG\r\n\x1a\nfixture").unwrap();
    let attachment = serde_json::from_value(
        json!({"id":"image","name":"gone.png","path":image.to_string_lossy(),"isDir":false}),
    )
    .unwrap();
    let inputs = images::prepare_attachments(&[attachment]).unwrap();
    let mut data = c.model.current.clone().unwrap();
    data.session.id = format!("sess_{}", uuid::Uuid::new_v4());
    let id = data.session.id.clone();
    data.session.workspace_cwd = Some(workspace.path().to_string_lossy().into());
    data.messages = vec![serde_json::from_value(json!({"id":"user","role":"user","content":"describe saved image","timestamp":1,"attachments":[{"id":"image","name":"gone.png","path":image.to_string_lossy(),"isDir":false}],"imageInputs":inputs})).unwrap(),serde_json::from_value(json!({"id":"answer","role":"assistant","content":"old answer","timestamp":2})).unwrap()];
    c.runtime.block_on(storage::save_session(data)).unwrap();
    c.new_session();
    let op = c.open_session(&id);
    wait(&mut c, op);
    assert!(
        c.model.current.as_ref().unwrap().messages[0]
            .image_inputs
            .is_none(),
        "starts from a thin viewport"
    );
    std::fs::remove_file(&image).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let server_id = id.clone();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut socket = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "no retry HTTP request");
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
        let persisted: Value = serde_json::from_slice(
            &std::fs::read(
                storage::config_dir()
                    .join("sessions")
                    .join(format!("{server_id}.json")),
            )
            .unwrap(),
        )
        .unwrap();
        tx.send((request, persisted)).unwrap();
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"replayed image\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    c.model.settings = serde_json::from_value::<AgentSettings>(json!({"providers":[{"id":"local","kind":"openai","baseUrl":format!("http://{address}/v1"),"model":"fixture"}],"activeProviderId":"local","self_improvement_enabled":false,"skillPaths":[],"autoCompactionEnabled":false,"reviewConfig":{"createDir":workspace.path().join("review").to_string_lossy()}})).unwrap();
    let op = c.dispatch("retry_turn", json!({"sessionId":id,"messageId":"answer"}));
    wait(&mut c, op);
    let deadline = Instant::now() + Duration::from_secs(10);
    while c.model.busy {
        c.poll();
        assert!(Instant::now() < deadline, "{:?}", c.model.error);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(c.model.error.is_none(), "{:?}", c.model.error);
    let (request, persisted) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let conversation: Vec<_> = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] != "system")
        .collect();
    assert_eq!(conversation.len(), 1);
    let blocks = conversation[0]["content"].as_array().unwrap();
    assert!(blocks
        .iter()
        .any(|b| b["image_url"]["url"] == format!("data:image/png;base64,{}", inputs[0].data)));
    assert_eq!(persisted["messages"].as_array().unwrap().len(), 2);
    assert_eq!(
        persisted["messages"][0]["imageInputs"][0]["data"],
        inputs[0].data
    );
    assert_eq!(
        c.runtime
            .block_on(storage::load_session(&id))
            .unwrap()
            .messages
            .last()
            .unwrap()
            .content,
        "replayed image"
    );
    server.join().unwrap();
}
#[test]
fn competing_cas_writers_have_exactly_one_winner() {
    let mut c = controller();
    let id = saved(&mut c, 8);
    let mut first = c.runtime.block_on(storage::load_session(&id)).unwrap();
    let mut second = first.clone();
    first.messages.truncate(2);
    second.messages.truncate(4);
    let window = c.model.window.clone().unwrap();
    let (a, b) = c.runtime.block_on(async {
        tokio::join!(
            storage::replace_session_checked(first, Some(window.clone())),
            storage::replace_session_checked(second, Some(window))
        )
    });
    assert_ne!(
        a.is_ok(),
        b.is_ok(),
        "the keyed lock must cover revision validation AND replacement"
    );
    let full = c.runtime.block_on(storage::load_session(&id)).unwrap();
    assert!(matches!(full.messages.len(), 2 | 4));
}
#[test]
fn stale_controller_history_does_not_overwrite_external_native_change() {
    let mut c = controller();
    let id = saved(&mut c, 8);
    let mut external = c.runtime.block_on(storage::load_session(&id)).unwrap();
    external.messages[0].content = "external mutation".into();
    c.runtime.block_on(storage::save_session(external)).unwrap();
    let op = c.undo_turn().unwrap();
    assert!(expect_error(&mut c, op).contains("stale"));
    let full = c.runtime.block_on(storage::load_session(&id)).unwrap();
    assert_eq!(full.messages.len(), 8);
    assert_eq!(full.messages[0].content, "external mutation");
    assert!(!c.model.busy);
}
