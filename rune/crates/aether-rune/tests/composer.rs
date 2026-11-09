//! Native composer regressions, using disposable storage and no external provider.
use aether_rune::{AppState, app::spec, view::view};
use rune::{Env, Runtime, TextEdit, Tx};
use std::{
    sync::Once,
    time::{Duration, Instant},
};
fn state() -> AppState {
    static PROFILE: Once = Once::new();
    PROFILE.call_once(|| {
        aether_core::agent::profile::configure_storage_root(
            std::env::temp_dir().join(format!("native-composer-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
    let mut s = AppState::new();
    let until = Instant::now() + Duration::from_secs(10);
    while !s.controller.model().pending_commands.is_empty() {
        s.controller.poll();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    s.controller.model_mut().settings = serde_json::from_value(serde_json::json!({"providers":[{"id":"fixture","model":"old","baseUrl":"http://127.0.0.1:1/v1"}],"activeProviderId":"fixture","self_improvement_enabled":false})).unwrap();
    s
}
#[test]
fn send_while_preparing_accepts_followup_and_exposes_native_queue() {
    let mut s = state();
    s.shell.composer = TextEdit::new("first turn");
    s.send(&mut Tx::default());
    assert!(s.controller.model().busy);
    s.shell.composer = TextEdit::new("follow-up");
    s.send(&mut Tx::default());
    assert!(
        s.shell.composer.text.is_empty(),
        "accepted queued input must clear the composer"
    );
    assert!(s.controller.model().attachments.is_empty());
    let mut rt = Runtime::new(s, view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.on_key = spec().on_key;
    rt.frame(0.0);
    assert!(
        rt.find("queue-pause").is_some(),
        "queue needs real pause/resume controls"
    );
}

use aether_core::agent::{AgentSettings, storage};
use aether_rune::statequeue;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
};

fn http_fixture() -> (
    String,
    mpsc::Receiver<Value>,
    mpsc::Sender<()>,
    std::thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let (seen, requests) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(10);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < until, "no HTTP request");
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
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
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
        seen.send(serde_json::from_slice(&bytes[start..start + length]).unwrap())
            .unwrap();
        gate.recv_timeout(Duration::from_secs(10)).unwrap();
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"captured result\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    });
    (url, requests, release, server)
}
fn pump(s: &mut AppState, done: impl Fn(&AppState) -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done(s) {
        s.controller.poll();
        assert!(
            Instant::now() < until,
            "controller timed out: {:?}",
            s.controller.model()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn configure(s: &mut AppState, url: &str) {
    s.controller.model_mut().settings = serde_json::from_value::<AgentSettings>(json!({
        "providers":[{"id":"local","kind":"openai","baseUrl":url,"model":"global"}],
        "activeProviderId":"local", "self_improvement_enabled":false,
        "skillPaths":[], "autoCompactionEnabled":false
    }))
    .unwrap();
}
#[test]
fn queued_capture_uses_y_without_persisting_over_session_override_x() {
    let mut s = state();
    let workspace = tempfile::tempdir().unwrap();
    s.controller =
        aether_state::Controller::new_with_workspace(workspace.path().to_string_lossy().into())
            .unwrap();
    let (url, requests, release, server) = http_fixture();
    configure(&mut s, &url);
    let id = s
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    s.controller
        .model_mut()
        .current
        .as_mut()
        .unwrap()
        .model_override = Some(
        serde_json::from_value(json!({"providerId":"local","model":"Y","thinking":false})).unwrap(),
    );
    s.shell.composer = TextEdit::new("queued capture");
    statequeue::enqueue(&mut s).unwrap();
    s.controller
        .model_mut()
        .current
        .as_mut()
        .unwrap()
        .model_override = Some(
        serde_json::from_value(json!({"providerId":"local","model":"X","thinking":true})).unwrap(),
    );
    let global = json!(s.controller.model().settings);
    let global_disk = json!(
        s.controller
            .runtime
            .block_on(storage::load_settings())
            .unwrap()
    );
    assert!(statequeue::sync(&mut s, &mut Tx::default()));
    pump(&mut s, |s| {
        s.controller
            .model()
            .run_status
            .as_ref()
            .is_some_and(|status| status.phase == aether_state::RunPhase::WaitingProvider)
    });
    let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    let initial = s
        .controller
        .runtime
        .block_on(storage::load_session(&id))
        .unwrap();
    let initial_global_disk = json!(
        s.controller
            .runtime
            .block_on(storage::load_settings())
            .unwrap()
    );
    // Release before asserting so a red regression does not strand the HTTP fixture.
    release.send(()).unwrap();
    server.join().unwrap();
    assert_eq!(request["model"], "Y");
    assert_eq!(initial_global_disk, global_disk);
    assert_eq!(
        json!(initial.model_override)["model"],
        "X",
        "initial snapshot lost the canonical override"
    );
    assert_eq!(json!(s.controller.model().settings), global);
    pump(&mut s, |s| s.controller.ready_to_send());
    let final_data = s
        .controller
        .runtime
        .block_on(storage::load_session(&id))
        .unwrap();
    assert_eq!(json!(final_data.model_override)["model"], "X");
    assert_eq!(json!(s.controller.model().settings), global);
    assert_eq!(
        final_data.messages.last().unwrap().content,
        "captured result"
    );
    assert_eq!(
        json!(
            s.controller
                .runtime
                .block_on(storage::load_settings())
                .unwrap()
        ),
        global_disk
    );
}
#[test]
fn queue_limits_count_api_prompt_and_all_captured_fields() {
    let mut s = state();
    s.shell.composer = TextEdit::new("small");
    statequeue::enqueue(&mut s).unwrap();
    let id = s
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    let original = s.shell.queue.sessions[&id].items[0].clone();
    let mut huge = original.clone();
    huge.options.api_prompt = Some("x".repeat(statequeue::MAX_BYTES));
    assert!(
        statequeue::QueueState::default()
            .push(id.clone(), huge)
            .is_err(),
        "API-only prompt bypassed the byte cap"
    );
    let mut huge = original.clone();
    huge.workspace = "x".repeat(statequeue::MAX_BYTES);
    assert!(
        statequeue::QueueState::default()
            .push(id.clone(), huge)
            .is_err(),
        "workspace bypassed the byte cap"
    );
    for n in 1..statequeue::MAX_ITEMS {
        let mut item = original.clone();
        item.id = format!("row-{n}");
        s.shell.queue.push(format!("origin-{n}"), item).unwrap();
    }
    assert_eq!(s.shell.queue.total(), 20);
    s.shell.composer = TextEdit::new("retain draft");
    assert!(statequeue::enqueue(&mut s).is_err());
    assert_eq!(s.shell.composer.text, "retain draft");
    assert_eq!(s.shell.queue.total(), 20);
    s.shell.queue = Default::default();
    s.shell.queue.push(id.clone(), original).unwrap();
    let remaining = statequeue::MAX_BYTES - s.shell.queue.bytes();
    s.shell.queue.sessions.get_mut(&id).unwrap().items[0]
        .prompt
        .push_str(&"x".repeat(remaining));
    assert_eq!(s.shell.queue.bytes(), statequeue::MAX_BYTES);
    assert!(
        s.shell
            .queue
            .push(
                id,
                s.shell.queue.sessions.values().next().unwrap().items[0].clone()
            )
            .is_err()
    );
}
#[test]
fn queue_edit_modal_blocks_global_shortcuts_and_direct_send() {
    use rune::{Key, KeyEvent, Mods};
    let mut s = state();
    s.shell.composer = TextEdit::new("queued");
    statequeue::enqueue(&mut s).unwrap();
    let session = s
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    let id = s.shell.queue.sessions[&session].items[0].id.clone();
    s.shell.queue.edit = Some(statequeue::QueueEdit {
        session: session.clone(),
        id,
        text: TextEdit::new("edited"),
    });
    s.shell.composer = TextEdit::new("/new must not execute");
    s.send(&mut Tx::default());
    assert_eq!(
        s.controller.model().current.as_ref().unwrap().session.id,
        session
    );
    assert_eq!(s.shell.composer.text, "/new must not execute");
    for key in [
        Key::Command('n'),
        Key::Command('b'),
        Key::Command(','),
        Key::Command('p'),
        Key::Command('l'),
        Key::Enter,
    ] {
        aether_rune::app::on_key(
            &mut s,
            &KeyEvent {
                key,
                mods: Mods {
                    ctrl: true,
                    ..Mods::default()
                },
            },
            &mut Tx::default(),
        );
        assert_eq!(
            s.controller.model().current.as_ref().unwrap().session.id,
            session
        );
        assert!(s.ui.panel.is_none());
        assert!(s.shell.sidebar_open);
        assert!(!s.shell.composer.focused);
    }
    assert!(s.shell.queue.edit.is_some());
}
fn runtime(s: AppState) -> Runtime<AppState> {
    let mut rt = Runtime::new(s, view, (1280.0, 1000.0), Env::new(true, 1.0));
    rt.on_key = spec().on_key;
    rt.frame(0.0);
    rt
}
fn click(rt: &mut Runtime<AppState>, key: &str) {
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("missing queue control {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.016);
}
#[test]
fn queue_pause_edit_remove_controls_retain_capture_and_live_draft() {
    let mut s = state();
    s.shell.composer = TextEdit::new("original queued prompt");
    statequeue::enqueue(&mut s).unwrap();
    let session = s
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    let item = s.shell.queue.sessions[&session].items[0].clone();
    s.shell.composer = TextEdit::new("live draft");
    s.controller.model_mut().draft = "live draft".into();
    let mut rt = runtime(s);
    click(&mut rt, "queue-pause");
    assert!(rt.state.shell.queue.sessions[&session].paused);
    assert!(!statequeue::sync(&mut rt.state, &mut Tx::default()));
    click(&mut rt, "queue-pause");
    assert!(!rt.state.shell.queue.sessions[&session].paused);
    click(&mut rt, &format!("queue-edit-{}", item.id));
    assert!(rt.find("queue-edit-modal").is_some());
    rt.state.shell.queue.edit.as_mut().unwrap().text = TextEdit::new("/ask edited queued prompt");
    rt.frame(0.016);
    click(&mut rt, "queue-edit-save");
    let edited = &rt.state.shell.queue.sessions[&session].items[0];
    assert_eq!(edited.prompt, "/ask edited queued prompt");
    assert_eq!(
        edited.options.api_prompt.as_deref(),
        Some("edited queued prompt")
    );
    assert_eq!(json!(edited.settings), json!(item.settings));
    assert_eq!(json!(edited.providers), json!(item.providers));
    assert!(rt.state.shell.queue.edit.is_none());
    assert_eq!(rt.state.shell.composer.text, "live draft");
    assert_eq!(rt.state.controller.model().draft, "live draft");
    click(&mut rt, &format!("queue-remove-{}", item.id));
    assert_eq!(rt.state.shell.queue.total(), 0);
    assert!(!rt.state.shell.queue.sessions.contains_key(&session));
}
#[test]
fn oversized_edit_is_retained_without_replacing_the_row() {
    let mut s = state();
    s.shell.composer = TextEdit::new("small original");
    statequeue::enqueue(&mut s).unwrap();
    let session = s
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    let item = s.shell.queue.sessions[&session].items[0].clone();
    s.shell.queue.edit = Some(statequeue::QueueEdit {
        session: session.clone(),
        id: item.id,
        text: TextEdit::new(format!(
            "/plan {}",
            "x".repeat(statequeue::MAX_BYTES / 2 + 1)
        )),
    });
    statequeue::save_edit(&mut s);
    assert!(s.shell.queue.edit.is_some());
    assert_eq!(
        s.shell.queue.sessions[&session].items[0].prompt,
        "small original"
    );
    assert!(s.shell.error.as_deref().unwrap().contains("5 MiB"));
}
#[test]
fn failed_send_keeps_row_and_restores_live_composer_payload() {
    let mut s = state();
    s.shell.composer = TextEdit::new("queued");
    statequeue::enqueue(&mut s).unwrap();
    let id = s
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    s.shell.queue.sessions.get_mut(&id).unwrap().items[0]
        .providers
        .clear();
    s.shell.composer = TextEdit::new("live draft");
    s.controller.model_mut().draft = "live draft".into();
    let attachments = json!(s.controller.model().attachments);
    let images = json!(s.controller.model().replay_image_inputs);
    assert!(statequeue::sync(&mut s, &mut Tx::default()));
    assert_eq!(s.shell.queue.total(), 1);
    assert!(s.shell.queue.sessions[&id].paused);
    assert_eq!(s.shell.composer.text, "live draft");
    assert_eq!(s.controller.model().draft, "live draft");
    assert_eq!(json!(s.controller.model().attachments), attachments);
    assert_eq!(json!(s.controller.model().replay_image_inputs), images);
    assert!(
        s.controller
            .model()
            .current
            .as_ref()
            .unwrap()
            .messages
            .is_empty()
    );
    assert!(
        s.shell
            .error
            .as_deref()
            .unwrap()
            .starts_with("Queue paused:")
    );
}
#[test]
fn offscreen_queue_waits_for_selection_and_workspace_resume() {
    let mut s = state();
    let (url, requests, release, server) = http_fixture();
    configure(&mut s, &url);
    s.shell.composer = TextEdit::new("origin queue");
    statequeue::enqueue(&mut s).unwrap();
    let origin = s
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    s.controller.new_session();
    assert!(!statequeue::sync(&mut s, &mut Tx::default()));
    assert!(requests.try_recv().is_err());
    assert_eq!(s.shell.queue.total(), 1);
    s.controller.open_session(&origin);
    pump(&mut s, |s| !s.controller.model().loading);
    assert_eq!(
        s.controller.model().current.as_ref().unwrap().session.id,
        origin
    );
    let original_workspace = s.controller.model().workspace.clone();
    s.controller.model_mut().workspace = "different-workspace".into();
    assert!(statequeue::sync(&mut s, &mut Tx::default()));
    assert!(s.shell.queue.sessions[&origin].paused);
    assert_eq!(s.shell.queue.total(), 1);
    assert!(requests.try_recv().is_err());
    s.controller.model_mut().workspace = original_workspace;
    assert!(
        !statequeue::sync(&mut s, &mut Tx::default()),
        "workspace restoration must not silently resume"
    );
    s.shell.queue.sessions.get_mut(&origin).unwrap().paused = false;
    assert!(statequeue::sync(&mut s, &mut Tx::default()));
    pump(&mut s, |s| {
        s.controller
            .model()
            .run_status
            .as_ref()
            .is_some_and(|r| r.phase == aether_state::RunPhase::WaitingProvider)
    });
    let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    server.join().unwrap();
    assert_eq!(request["model"], "global");
    assert_eq!(s.shell.queue.total(), 0);
    pump(&mut s, |s| s.controller.ready_to_send());
}
#[test]
fn queue_waits_for_terminal_save_ack_and_reuses_deleted_image_bytes() {
    let mut s = state();
    let workspace = tempfile::tempdir().unwrap();
    s.controller =
        aether_state::Controller::new_with_workspace(workspace.path().to_string_lossy().into())
            .unwrap();
    let (first_url, first_requests, first_release, first_server) = http_fixture();
    let (second_url, second_requests, second_release, second_server) = http_fixture();
    configure(&mut s, &first_url);
    s.shell.composer = TextEdit::new("first turn");
    s.send(&mut Tx::default());
    assert!(s.controller.model().busy);
    let first_run_id = s
        .controller
        .model()
        .run_status
        .as_ref()
        .unwrap()
        .run_id
        .clone();
    configure(&mut s, &second_url);
    let source = workspace.path().join("temporary-queued-image.png");
    image::RgbImage::new(2, 2).save(&source).unwrap();
    let attachment: aether_core::agent::AttachedContext = serde_json::from_value(json!({"id":"queued-image", "name":"temporary-queued-image.png", "path":source.to_string_lossy(), "isDir":false})).unwrap();
    let prepared = aether_core::agent::images::prepare_attachments(&[attachment.clone()]).unwrap();
    let image_data = prepared[0].data.clone();
    s.controller.model_mut().attachments = vec![attachment];
    s.controller.model_mut().replay_image_inputs = prepared;
    s.shell.composer = TextEdit::new("follow-up with prepared image");
    s.send(&mut Tx::default());
    assert_eq!(s.shell.queue.total(), 1);
    assert!(s.shell.composer.text.is_empty());
    // This file belongs solely to this disposable fixture. A queue must not reopen it.
    std::fs::remove_file(&source).unwrap();
    assert!(!source.exists());
    assert!(!statequeue::sync(&mut s, &mut Tx::default()));
    assert!(
        first_requests.try_recv().is_err(),
        "initial save must be acknowledged before HTTP dispatch"
    );
    pump(&mut s, |s| {
        s.controller
            .model()
            .run_status
            .as_ref()
            .is_some_and(|r| r.phase == aether_state::RunPhase::WaitingProvider)
    });
    first_requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(second_requests.try_recv().is_err());
    first_release.send(()).unwrap();
    first_server.join().unwrap();
    pump(&mut s, |s| {
        s.controller
            .model()
            .run_status
            .as_ref()
            .is_some_and(|r| r.phase == aether_state::RunPhase::Saving)
    });
    // Done has been reduced; even an already buffered save ACK is not an ACK
    // until Controller polls it. This assertion needs no sleep/race with disk IO.
    assert!(!s.controller.ready_to_send());
    assert!(
        s.controller.model().completed_runs.is_empty(),
        "Done alone must not publish completion"
    );
    assert!(!statequeue::sync(&mut s, &mut Tx::default()));
    assert_eq!(s.shell.queue.total(), 1);
    assert!(second_requests.try_recv().is_err());
    pump(&mut s, |s| s.controller.ready_to_send());
    assert_eq!(s.controller.model().completed_runs.len(), 1);
    assert_eq!(s.controller.model().completed_runs[0].run_id, first_run_id);
    s.shell.composer = TextEdit::new("unrelated live draft");
    s.controller.model_mut().draft = "unrelated live draft".into();
    assert!(statequeue::sync(&mut s, &mut Tx::default()));
    assert_eq!(s.shell.queue.total(), 0);
    assert_eq!(s.controller.model().draft, "unrelated live draft");
    assert_eq!(s.shell.composer.text, "unrelated live draft");
    pump(&mut s, |s| {
        s.controller
            .model()
            .run_status
            .as_ref()
            .is_some_and(|r| r.phase == aether_state::RunPhase::WaitingProvider)
    });
    let request = second_requests
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    second_release.send(()).unwrap();
    second_server.join().unwrap();
    assert!(
        request.to_string().contains(&image_data),
        "queued image bytes did not reach HTTP"
    );
    pump(&mut s, |s| s.controller.ready_to_send());
    assert!(
        s.controller.model().error.is_none(),
        "{:?}",
        s.controller.model().error
    );
    let id = s
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    let disk = s
        .controller
        .runtime
        .block_on(storage::load_session(&id))
        .unwrap();
    assert_eq!(disk.messages.len(), 4);
    assert_eq!(
        disk.messages[2].image_inputs.as_ref().unwrap()[0].data,
        image_data
    );
}
#[test]
fn captured_provider_cardinality_is_validated_without_mutating_model() {
    let mut s = state();
    let settings = s.controller.model().settings.clone();
    let provider = settings.providers[0].clone();
    let baseline = json!(s.controller.model().current);
    for providers in [
        vec![],
        vec![provider.clone(), provider.clone(), provider.clone()],
    ] {
        assert!(
            s.controller
                .send_captured(
                    "invalid capture".into(),
                    Default::default(),
                    settings.clone(),
                    providers
                )
                .is_err()
        );
        assert_eq!(json!(s.controller.model().current), baseline);
        assert_eq!(json!(s.controller.model().settings), json!(settings));
        assert!(!s.controller.model().busy);
    }
}
#[test]
fn captured_two_provider_council_preserves_both_requests() {
    let mut s = state();
    let (url_a, requests_a, release_a, server_a) = http_fixture();
    let (url_b, requests_b, release_b, server_b) = http_fixture();
    configure(&mut s, &url_a);
    let settings = s.controller.model().settings.clone();
    let mut a = settings.providers[0].clone();
    a.model = "captured-A".into();
    let mut b = a.clone();
    b.id = "candidate-b".into();
    b.model = "captured-B".into();
    b.base_url = url_b;
    s.controller
        .model_mut()
        .current
        .as_mut()
        .unwrap()
        .model_override = Some(
        serde_json::from_value(
            json!({"providerId":"not-currently-configured", "model":"canonical-X"}),
        )
        .unwrap(),
    );
    s.controller
        .send_captured(
            "council capture".into(),
            Default::default(),
            settings.clone(),
            vec![a, b],
        )
        .unwrap();
    pump(&mut s, |s| {
        s.controller
            .model()
            .run_status
            .as_ref()
            .is_some_and(|r| r.phase == aether_state::RunPhase::WaitingProvider)
    });
    let request_a = requests_a.recv_timeout(Duration::from_secs(5)).unwrap();
    let request_b = requests_b.recv_timeout(Duration::from_secs(5)).unwrap();
    release_a.send(()).unwrap();
    release_b.send(()).unwrap();
    server_a.join().unwrap();
    server_b.join().unwrap();
    assert_eq!(request_a["model"], "captured-A");
    assert_eq!(request_b["model"], "captured-B");
    pump(&mut s, |s| s.controller.ready_to_send());
    let data = s.controller.model().current.as_ref().unwrap();
    let turn = data.messages.last().unwrap().council_turn.as_ref().unwrap();
    assert_eq!(turn.candidate_a.model, "captured-A");
    assert_eq!(turn.candidate_b.model, "captured-B");
    assert_eq!(
        data.model_override.as_ref().unwrap().model.as_deref(),
        Some("canonical-X")
    );
    assert_eq!(json!(s.controller.model().settings), json!(settings));
}
