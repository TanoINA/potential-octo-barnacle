use aether_core::agent::{profile, storage, ExecutionMode};
use aether_state::{
    commands::{prompt_options, UI_ACTION_CAPACITY},
    Controller, InputAction,
};
use serde_json::json;
use std::{
    sync::Once,
    time::{Duration, Instant},
};

fn controller() -> Controller {
    static PROFILE: Once = Once::new();
    PROFILE.call_once(|| {
        profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-slash-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
    Controller::new_with_workspace(String::new()).unwrap()
}
fn drain(c: &mut Controller) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        c.poll();
        if c.model.pending_commands.is_empty() && !c.model.busy {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "pending: {:?}; error: {:?}",
            c.model.pending_commands,
            c.model.error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn configured(c: &mut Controller) {
    c.model.settings = serde_json::from_value(json!({"providers":[{"id":"local","baseUrl":"http://127.0.0.1:9/v1","model":"unused"}],"activeProviderId":"local","self_improvement_enabled":false})).unwrap();
}
#[test]
fn new_and_ui_commands_work_without_a_provider_and_never_add_messages() {
    let mut c = controller();
    assert!(c.model.settings.providers.is_empty());
    let old = c.model.current.as_ref().unwrap().session.id.clone();
    c.send(" /NeW \n".into()).unwrap();
    assert_ne!(c.model.current.as_ref().unwrap().session.id, old);
    for (command, panel, args) in [
        ("/model custom/model", "models", "custom/model"),
        ("/settings", "settings", ""),
        ("/history", "history", ""),
        ("/artifacts", "artifacts", ""),
        ("/tools", "tools", ""),
        ("/memory", "memory", ""),
        ("/terminal", "shell", ""),
        ("/git", "git", ""),
        ("/tasks", "tasks", ""),
        ("/skills", "tools", "skills"),
        ("/init", "memory", "init"),
    ] {
        c.model.draft = command.into();
        c.send(command.into()).unwrap();
        let action = c.model.ui_actions.last().unwrap();
        assert_eq!((&*action.panel, &*action.args), (panel, args));
        assert!(c.model.draft.is_empty());
    }
    assert!(c.model.current.as_ref().unwrap().messages.is_empty());
    assert!(c.model.pending_commands.is_empty());
}
#[test]
fn invalid_syntax_never_starts_a_provider_or_loses_the_draft() {
    let mut c = controller();
    configured(&mut c);
    for input in [
        "/title",
        "/review extra",
        "/reflect focus",
        "/refine focus",
        "/stop extra",
        "/clear extra",
        "/compact extra",
        "/compress extra",
        "/undo extra",
        "/retry extra",
        "/pin extra",
        "/settings extra",
        "/agent",
        "/ask",
    ] {
        c.model.draft = input.into();
        assert!(c.send(input.into()).is_err(), "accepted {input}");
        assert_eq!(c.model.draft, input);
        assert!(c.model.current.as_ref().unwrap().messages.is_empty());
        assert!(c.model.pending_commands.is_empty());
        assert!(!c.model.busy);
    }
}
#[test]
fn unknown_slashes_are_literal_and_run_directives_have_request_only_options() {
    let mut c = controller();
    for text in [
        "/unknown value",
        "/newspaper",
        "literal /clear",
        "/refinement",
    ] {
        assert!(
            matches!(c.route_input(text).unwrap(), InputAction::Send { display, api:None, execution_mode:None } if display == text)
        );
    }
    for (text, mode, api) in [
        (
            "/ask inspect this",
            ExecutionMode::RequestReview,
            "inspect this",
        ),
        (
            "/agent implement this",
            ExecutionMode::Turbo,
            "implement this",
        ),
    ] {
        assert!(
            matches!(c.route_input(text).unwrap(), InputAction::Send { display, api:Some(prompt), execution_mode:Some(actual) } if display == text && prompt == api && actual == mode)
        );
    }
    let options = prompt_options("/plan build a parser").unwrap();
    assert!(options
        .api_prompt
        .unwrap()
        .contains("execution plan for: build a parser"));
    assert!(prompt_options("/plan")
        .unwrap()
        .api_prompt
        .unwrap()
        .contains("the current task"));
    assert!(
        prompt_options("/new").unwrap().api_prompt.is_none(),
        "literal replay must not run /new"
    );
}
#[test]
fn ui_queue_is_bounded_without_silently_dropping_actions() {
    let mut c = controller();
    for _ in 0..UI_ACTION_CAPACITY {
        c.send("/settings".into()).unwrap();
    }
    c.model.draft = "/history".into();
    assert!(c.send("/history".into()).is_err());
    assert_eq!(c.model.ui_actions.len(), UI_ACTION_CAPACITY);
    assert_eq!(c.model.draft, "/history");
}
#[test]
fn stop_cancels_preparing_run_without_interpreting_a_new_message() {
    let mut c = controller();
    configured(&mut c);
    let settings = json!(c.model.settings);
    c.send("/ask do not execute".into()).unwrap();
    assert_eq!(json!(c.model.settings), settings);
    c.send("/stop".into()).unwrap();
    drain(&mut c);
    assert_eq!(
        c.model.run_status.as_ref().unwrap().phase,
        aether_state::RunPhase::Cancelled
    );
    let messages = &c.model.current.as_ref().unwrap().messages;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "/ask do not execute");
    assert!(c.send("/stop".into()).is_err());
}
#[test]
fn export_defers_path_writes_to_the_ui_and_keeps_format_and_destination() {
    let mut c = controller();
    c.send("/export json reports/chat.json".into()).unwrap();
    let action = c.model.ui_actions.last().unwrap();
    assert_eq!(action.panel, "export");
    let args: serde_json::Value = serde_json::from_str(&action.args).unwrap();
    assert_eq!(args["format"], "json");
    assert_eq!(args["destination"], "reports/chat.json");
    assert!(c.model.pending_commands.is_empty());
    c.send("/save".into()).unwrap();
    let args: serde_json::Value =
        serde_json::from_str(&c.model.ui_actions.last().unwrap().args).unwrap();
    assert_eq!(args["format"], "json");
}
#[test]
fn title_and_pin_persist_without_chat_on_a_new_session() {
    let mut c = controller();
    c.send("/title named chat".into()).unwrap();
    drain(&mut c);
    c.send("/pin".into()).unwrap();
    drain(&mut c);
    let id = c.model.current.as_ref().unwrap().session.id.clone();
    let saved = c.runtime.block_on(storage::load_session(&id)).unwrap();
    assert_eq!(saved.session.title, "named chat");
    assert!(saved.session.pinned);
    assert!(saved.messages.is_empty());
}
#[test]
fn review_aliases_on_empty_conversation_fail_before_provider_dispatch() {
    let mut c = controller();
    configured(&mut c);
    for command in ["/review", "/reflect", "/refine"] {
        assert!(c.send(command.into()).is_err());
    }
    assert!(c.model.pending_commands.is_empty());
    assert!(c.model.current.as_ref().unwrap().messages.is_empty());
}

#[test]
fn actual_provider_receives_expansion_and_per_request_permissions_but_storage_keeps_display() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    for (input, expected, mode) in [
        (
            "/plan build a parser",
            "execution plan for: build a parser",
            "TURBO",
        ),
        ("/ask inspect this", "inspect this", "REQUEST REVIEW"),
        ("/agent implement this", "implement this", "TURBO"),
    ] {
        let mut c = controller();
        let workspace = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut socket = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "no HTTP request");
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let (start, length) = loop {
                let mut buf = [0; 4096];
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(at) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..at]);
                    let len = headers
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (at + 4, len);
                }
            };
            while bytes.len() < start + length {
                let mut buf = [0; 4096];
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
            }
            let value: serde_json::Value =
                serde_json::from_slice(&bytes[start..start + length]).unwrap();
            let body="data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            value
        });
        c.model.settings=serde_json::from_value(json!({"providers":[{"id":"fixture","kind":"openai","baseUrl":format!("http://{addr}/v1"),"model":"fixture"}],"activeProviderId":"fixture","executionMode":"default","self_improvement_enabled":false,"autoCompactionEnabled":false,"skillPaths":[],"reviewConfig":{"createDir":workspace.path().join("review").to_string_lossy()}})).unwrap();
        let before = json!(c.model.settings);
        c.send(input.into()).unwrap();
        drain(&mut c);
        assert!(c.model.error.is_none(), "{:?}", c.model.error);
        let request = server.join().unwrap();
        let messages = request["messages"].as_array().unwrap();
        let user = messages.iter().rev().find(|m| m["role"] == "user").unwrap();
        assert!(
            user["content"].as_str().unwrap().contains(expected),
            "{request}"
        );
        let expected_mode = if input.starts_with("/plan") {
            "DEFAULT"
        } else {
            mode
        };
        assert!(
            messages
                .iter()
                .filter(|m| m["role"] == "system")
                .any(|m| m["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains(&format!("Execution Mode: {expected_mode}"))),
            "missing {expected_mode}"
        );
        assert_eq!(json!(c.model.settings), before);
        let id = c.model.current.as_ref().unwrap().session.id.clone();
        assert_eq!(
            c.runtime
                .block_on(storage::load_session(&id))
                .unwrap()
                .messages[0]
                .content,
            input
        );
    }
}

#[test]
fn clear_and_undo_are_local_history_operations_without_a_provider() {
    let mut c = controller();
    c.send("/clear".into()).unwrap();
    drain(&mut c);
    assert!(c.model.error.is_none(), "{:?}", c.model.error);
    assert!(c.model.current.as_ref().unwrap().messages.is_empty());
    c.send("/undo".into()).unwrap();
    drain(&mut c);
    assert!(c.model.error.is_some());
    assert!(c.model.current.as_ref().unwrap().messages.is_empty());
}

#[test]
fn automatic_retry_of_literal_slash_prompt_does_not_run_a_local_command() {
    let mut c = controller();
    configured(&mut c);
    let id = c.model.current.as_ref().unwrap().session.id.clone();
    c.send_prompt("/new".into()).unwrap();
    c.abort().unwrap();
    drain(&mut c);
    assert_eq!(c.model.current.as_ref().unwrap().session.id, id);
    c.model.settings.providers.clear();
    c.send("/retry".into()).unwrap();
    drain(&mut c);
    assert_eq!(
        c.model.current.as_ref().unwrap().session.id,
        id,
        "retry executed a local /new command"
    );
    assert_eq!(c.model.draft, "/new");
    assert!(c
        .model
        .error
        .as_deref()
        .is_some_and(|e| e.contains("configured provider")));
    assert!(c.model.current.as_ref().unwrap().messages.is_empty());
}
