//! Native control integration tests: no display server and no provider request.
use aether_rune::{
    app::{AppState, spec},
    view::view,
};
use rune::{Env, Key, KeyEvent, Mods, Runtime};
use std::sync::Once;
static PROFILE: Once = Once::new();
fn runtime() -> Runtime<AppState> {
    PROFILE.call_once(|| {
        let root =
            std::env::temp_dir().join(format!("aether-rune-shell-test-{}", uuid::Uuid::new_v4()));
        aether_core::agent::profile::configure_storage_root(&root).unwrap();
    });
    let mut state = AppState::new();
    let settings_id = *state
        .controller
        .model()
        .pending_commands
        .iter()
        .find(|(_, command)| command.as_str() == "get_settings")
        .expect("AppState must load saved settings at startup")
        .0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        state.controller.poll();
        if state.controller.model().pending_commands.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "startup service reads timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let settings = state
        .controller
        .model()
        .results
        .get(&settings_id)
        .expect("startup settings read must complete before fixture writes");
    assert!(
        settings.error.is_none(),
        "startup settings failed: {:?}",
        settings.error
    );
    // Fixture settings belong after the real async read, never before a stale
    // startup completion can replace them during the slash-intent poll.
    let mut rt = Runtime::new(state, view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.on_key = spec().on_key;
    rt.frame(0.0);
    rt
}
fn click(rt: &mut Runtime<AppState>, key: &str) {
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("missing native control {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.016);
}
#[test]
fn native_new_chat_button_creates_a_real_session() {
    let mut rt = runtime();
    let previous = rt
        .state
        .controller
        .model()
        .current
        .as_ref()
        .map(|data| data.session.id.clone());
    click(&mut rt, "new-chat");
    let current = rt
        .state
        .controller
        .model()
        .current
        .as_ref()
        .expect("New creates canonical session state");
    assert_ne!(Some(current.session.id.clone()), previous);
    assert!(current.messages.is_empty());
    assert!(rt.find("composer").is_some());
}
#[test]
fn native_composer_updates_draft_and_preserves_multiline_text() {
    let mut rt = runtime();
    click(&mut rt, "composer");
    rt.key(KeyEvent {
        key: Key::Text("First line".into()),
        mods: Mods::default(),
    });
    rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods {
            shift: true,
            ..Mods::default()
        },
    });
    rt.key(KeyEvent {
        key: Key::Text("Second line".into()),
        mods: Mods::default(),
    });
    rt.frame(0.016);
    assert!(rt.state.controller.model().draft.contains("First line"));
    assert!(rt.state.controller.model().draft.contains("\n"));
    assert!(rt.state.controller.model().draft.contains("Second line"));
    assert!(
        !rt.state.controller.model().busy,
        "typing must never start a network run"
    );
}
#[test]
fn native_sidebar_and_settings_buttons_change_visible_state() {
    let mut rt = runtime();
    click(&mut rt, "toggle-sidebar");
    assert!(!rt.state.shell.sidebar_open);
    click(&mut rt, "settings");
    assert!(rt.state.ui.panel.is_some());
}

#[test]
fn stale_attachment_completion_never_enters_a_new_session() {
    use aether_rune::native;
    use aether_state::CommandResult;
    let mut rt = runtime();
    rt.state.new_session(&mut rune::Tx::default());
    native::attach_paths(&mut rt.state, &[std::path::PathBuf::from("old.txt")]);
    let id = *rt.state.shell.native.pending.keys().next().unwrap();
    rt.state.new_session(&mut rune::Tx::default());
    rt.state.controller.model_mut().results.insert(
        id,
        CommandResult {
            id,
            command: "inspect_paths".into(),
            session_id: None,
            generation: 0,
            value: Some(
                serde_json::json!([{"id":"old","path":"old.txt","name":"old.txt","isDir":false}]),
            ),
            error: None,
            failure_phase: None,
        },
    );
    native::sync(&mut rt.state);
    assert!(
        rt.state.controller.model().attachments.is_empty(),
        "stale files must not attach to the newly selected session"
    );
}
#[test]
fn inspection_blocks_every_submit_path_until_cancelled() {
    let mut rt = runtime();
    rt.state.new_session(&mut rune::Tx::default());
    rt.state.shell.composer = rune::TextEdit::new("retain this draft");
    aether_rune::native::attach_paths(&mut rt.state, &[std::path::PathBuf::from("pending.txt")]);
    rt.state.send(&mut rune::Tx::default());
    assert!(
        rt.state
            .shell
            .error
            .as_deref()
            .unwrap_or("")
            .contains("attachment"),
        "pending inspection should explain why sending is blocked: {:?}",
        rt.state.shell.error
    );
    assert_eq!(rt.state.shell.composer.text, "retain this draft");
}
#[test]
fn service_panel_stays_inside_resized_native_window_and_traps_focus() {
    let mut rt = runtime();
    click(&mut rt, "settings");
    rt.set_viewport((900.0, 600.0), 1.0);
    for _ in 0..5 {
        rt.frame(0.016);
    }
    let elements = rt.elements();
    let overlay = elements
        .iter()
        .find(|e| e.key.as_deref() == Some("panel-overlay"))
        .unwrap();
    let r = overlay.rect;
    assert!(
        r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= 900.5 && r[1] + r[3] <= 600.5,
        "panel overflows native viewport: {r:?}"
    );
    assert!(
        elements
            .iter()
            .find(|e| e.key.as_deref() == Some("composer"))
            .unwrap()
            .inert,
        "modal background must be inert"
    );
}
#[test]
fn council_composer_exposes_configured_model_choices() {
    let mut rt = runtime();
    rt.state.controller.model_mut().settings.providers = vec![
        aether_core::agent::ProviderConfig {
            id: "a".into(),
            name: "Alpha".into(),
            model: "one".into(),
            ..Default::default()
        },
        aether_core::agent::ProviderConfig {
            id: "b".into(),
            name: "Beta".into(),
            model: "two".into(),
            models: vec!["two".into(), "three".into()],
            ..Default::default()
        },
    ];
    rt.state.controller.model_mut().settings.active_provider_id = "a".into();
    rt.invalidate();
    rt.frame(0.016);
    click(&mut rt, "council-toggle");
    assert!(rt.find("council-candidate-b").is_some());
    click(&mut rt, "council-candidate-b");
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|text| text.contains("three"))),
        "candidate cycle must include configured alternative models"
    );
}
#[test]
fn image_attachment_has_native_preview_action() {
    let mut rt = runtime();
    rt.state.controller.model_mut().attachments=serde_json::from_value(serde_json::json!([{"id":"picture","path":"C:/explicit.png","name":"explicit.png","isDir":false}])).unwrap();
    rt.invalidate();
    rt.frame(0.016);
    for _ in 0..4 {
        rt.frame(0.016);
    }
    click(&mut rt, "preview-attachment-picture");
    assert!(rt.find("image-preview-modal").is_some());
    assert!(
        rt.elements()
            .iter()
            .find(|e| e.key.as_deref() == Some("composer"))
            .unwrap()
            .inert
    );
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    rt.frame(0.016);
    assert!(rt.find("image-preview-modal").is_none());
}
#[test]
fn formula_worker_completion_invalidates_an_idle_shell() {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    let mut rt = runtime();
    for _ in 0..100 {
        rt.state.controller.poll();
        if rt.state.controller.model().pending_commands.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let woke = Arc::new(AtomicBool::new(false));
    let flag = woke.clone();
    rt.state.rich = std::cell::RefCell::new(aether_rune::rich_content::RichContent::new(Arc::new(
        move || {
            flag.store(true, Ordering::SeqCst);
        },
    )));
    let _ = rt
        .state
        .rich
        .borrow_mut()
        .render("$x^2$", "wake-test", &rune_kit::Kit::neutral(true));
    for _ in 0..200 {
        if woke.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(woke.load(Ordering::SeqCst), "math worker must wake");
    assert!(
        (spec().poll.unwrap())(&mut rt.state, 0.0, &mut rune::Tx::default()),
        "a wake must invalidate even when Controller::poll returns false"
    );
}

#[test]
fn cancelling_inspection_restores_submit_and_discards_late_completion() {
    use aether_rune::native;
    let mut rt = runtime();
    rt.state.new_session(&mut rune::Tx::default());
    native::attach_paths(&mut rt.state, &[std::path::PathBuf::from("cancelled.png")]);
    let id = *rt.state.shell.native.pending.keys().next().unwrap();
    rt.invalidate();
    rt.frame(0.016);
    click(&mut rt, "cancel-attachment-preparation");
    assert!(!rt.state.shell.native.preparing());
    rt.state.controller.model_mut().results.insert(id,aether_state::CommandResult{id,command:"inspect_paths".into(),session_id:None,generation:0,value:Some(serde_json::json!([{"id":"late","path":"cancelled.png","name":"cancelled.png","isDir":false}])),error:None,failure_phase:None});
    native::sync(&mut rt.state);
    assert!(rt.state.controller.model().attachments.is_empty());
}
#[test]
fn actual_prepared_image_reply_decodes_in_the_native_cache() {
    use aether_rune::native;
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut rt = runtime();
    rt.state.new_session(&mut rune::Tx::default());
    let file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    image::RgbaImage::from_pixel(2, 3, image::Rgba([255, 0, 0, 255]))
        .save_with_format(file.path(), image::ImageFormat::Png)
        .unwrap();
    let path = file.path().to_string_lossy().into_owned();
    let attachment = aether_core::agent::AttachedContext {
        id: "png".into(),
        path: path.clone(),
        name: "fixture.png".into(),
        is_dir: false,
    };
    native::preview(&mut rt.state, &attachment);
    let id = *rt.state.shell.native.pending.keys().next().unwrap();
    let payload = serde_json::json!([{"sourcePath":path,"mediaType":"image/png","data":STANDARD.encode(std::fs::read(file.path()).unwrap())}]);
    rt.state.controller.model_mut().results.insert(
        id,
        aether_state::CommandResult {
            id,
            command: "prepare_image_attachments".into(),
            session_id: None,
            generation: 0,
            value: Some(payload),
            error: None,
            failure_phase: None,
        },
    );
    assert!(native::sync(&mut rt.state));
    assert_eq!(
        rt.state
            .shell
            .native
            .image(&path)
            .expect("reply must hydrate ImageCache")
            .wait()
            .unwrap(),
        [2, 3]
    );
    assert!(
        native::sync(&mut rt.state),
        "decode wake must invalidate shell"
    );
    assert!(rt.state.shell.native.cached_bytes() <= 20 * 1024 * 1024);
    rt.invalidate();
    rt.frame(0.016);
    assert!(rt.find("preview-pixels").is_some());
    for _ in 0..8 {
        rt.key(KeyEvent {
            key: Key::Tab,
            mods: Mods::default(),
        });
        rt.frame(0.016);
    }
    assert!(
        rt.elements()
            .iter()
            .filter(|e| e.focused)
            .all(|e| e.path.contains("image-preview-modal")),
        "Tab focus must remain in native image modal"
    );
    assert!(
        rt.state.controller.model().attachments.is_empty(),
        "preview-only must never attach an orphan"
    );
}
#[test]
fn model_picker_opens_actual_models_panel() {
    let mut rt = runtime();
    click(&mut rt, "model-picker");
    assert_eq!(rt.state.ui.panel, Some(aether_rune::panels::Panel::Models));
}
#[test]
fn composer_and_send_fit_compact_resized_window() {
    let mut rt = runtime();
    rt.set_viewport((900.0, 600.0), 1.0);
    for _ in 0..6 {
        rt.frame(0.016);
    }
    for key in ["composer", "send", "settings"] {
        let e = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap();
        let r = e.rect;
        assert!(
            r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= 900.5 && r[1] + r[3] <= 600.5,
            "{key} overflow: {r:?}"
        );
    }
}
#[test]
fn sent_image_keeps_its_preview_control_in_the_transcript() {
    let mut rt = runtime();
    rt.state.new_session(&mut rune::Tx::default());
    rt.state.controller.model_mut().current.as_mut().unwrap().messages=serde_json::from_value(serde_json::json!([{"id":"sent","role":"user","content":"Look at this","timestamp":1,"attachments":[{"id":"sent-image","name":"sent.png","path":"C:/sent.png","isDir":false}]}])).unwrap();
    rt.invalidate();
    rt.frame(0.016);
    assert!(
        rt.find("preview-message-sent-sent-image").is_some(),
        "image preview must remain accessible after send"
    );
}
#[test]
fn queued_slash_intents_open_real_service_panels_once() {
    use aether_rune::panels::Panel;
    let mut rt = runtime();
    for (name, expected) in [
        ("models", Panel::Models),
        ("settings", Panel::Settings),
        ("history", Panel::History),
        ("artifacts", Panel::Artifacts),
        ("tools", Panel::Tools),
        ("memory", Panel::Memory),
        ("shell", Panel::Shell),
        ("git", Panel::Git),
        ("tasks", Panel::Tasks),
        ("skills", Panel::Tools),
        ("init", Panel::Memory),
    ] {
        rt.state
            .controller
            .model_mut()
            .ui_actions
            .push(aether_state::UiAction {
                panel: name.into(),
                args: String::new(),
            });
        assert!((spec().poll.unwrap())(
            &mut rt.state,
            0.0,
            &mut rune::Tx::default()
        ));
        assert_eq!(
            rt.state.ui.panel,
            Some(expected),
            "queued {name} intent was not routed"
        );
        assert!(
            rt.state.controller.model().ui_actions.is_empty(),
            "intent must be consumed once"
        );
    }
    assert!(
        rt.state
            .controller
            .model()
            .pending_commands
            .values()
            .any(|c| c == "list_native_tools")
    );
    assert!(
        !rt.state
            .controller
            .model()
            .pending_commands
            .values()
            .any(|c| c == "init_workspace_agents_md"),
        "init must still need explicit confirmation"
    );
}

#[test]
fn model_slash_argument_seeds_real_provider_editor_without_saving() {
    let mut rt = runtime();
    rt.state.controller.model_mut().settings.providers = vec![aether_core::agent::ProviderConfig {
        id: "local".into(),
        name: "Local".into(),
        base_url: "http://localhost/v1".into(),
        model: "old-model".into(),
        api_key: "secret".into(),
        ..Default::default()
    }];
    rt.state.controller.model_mut().settings.active_provider_id = "local".into();
    rt.state.shell.composer = rune::TextEdit::new("/model new-model");
    rt.state.send(&mut rune::Tx::default());
    (spec().poll.unwrap())(&mut rt.state, 0.0, &mut rune::Tx::default());
    assert_eq!(rt.state.ui.form("provider.id"), "local");
    assert_eq!(rt.state.ui.form("provider.model"), "new-model");
    assert_eq!(
        rt.state.ui.form("provider.apiKey"),
        "",
        "never put a stored secret in an intent seed"
    );
    assert!(
        !rt.state
            .controller
            .model()
            .pending_commands
            .values()
            .any(|c| c == "save_settings")
    );
}

#[test]
fn control_l_moves_real_keyboard_focus_back_to_composer() {
    let mut rt = runtime();
    click(&mut rt, "session-search");
    rt.key(KeyEvent {
        key: Key::Command('l'),
        mods: Mods {
            ctrl: true,
            ..Mods::default()
        },
    });
    rt.frame(0.016);
    rt.key(KeyEvent {
        key: Key::Text("focused composer".into()),
        mods: Mods::default(),
    });
    rt.frame(0.016);
    assert_eq!(rt.state.shell.composer.text, "focused composer");
}

#[test]
fn startup_settings_ack_blocks_send_and_settings_edits_but_allows_new() {
    let mut rt=runtime();
    rt.state=AppState::new();
    rt.state.controller.model_mut().settings.providers.clear(); // red test cannot reach a provider
    rt.state.shell.composer=rune::TextEdit::new("keep startup draft");
    rt.state.send(&mut rune::Tx::default());
    assert_eq!(rt.state.shell.composer.text,"keep startup draft");
    assert_eq!(rt.state.controller.model().draft,"keep startup draft");
    assert!(rt.state.shell.error.as_deref().unwrap_or("").contains("configuration"));
    assert!(!rt.state.controller.model().busy);
    assert!(!rt.state.controller.model().pending_commands.values().any(|c| c=="send_message" || c=="send_council_message"));
    let old=rt.state.controller.model().current.as_ref().unwrap().session.id.clone();
    rt.state.shell.composer=rune::TextEdit::new("/new");rt.state.send(&mut rune::Tx::default());
    assert_ne!(rt.state.controller.model().current.as_ref().unwrap().session.id,old);
    rt.state.ui.open(aether_rune::panels::Panel::Settings,&mut rt.state.controller);
    rt.invalidate();rt.frame(0.016);
    assert!(rt.elements().iter().any(|e| e.text.as_deref().is_some_and(|t|t.contains("Loading configuration"))));
    assert!(rt.elements().iter().find(|e|e.key.as_deref()==Some("field-settings.systemPrompt")).unwrap().inert);
    let end=std::time::Instant::now()+std::time::Duration::from_secs(5);
    while !rt.state.controller.model().pending_commands.is_empty() {
        rt.state.controller.poll();rt.state.ui.sync(&mut rt.state.controller);
        assert!(std::time::Instant::now()<end);std::thread::sleep(std::time::Duration::from_millis(5));
    }
    rt.invalidate();rt.frame(0.016);
    assert!(!rt.elements().iter().find(|e|e.key.as_deref()==Some("field-settings.systemPrompt")).unwrap().inert);
    assert_eq!(rt.state.ui.form("settings.systemPrompt"),rt.state.controller.model().settings.system_prompt);
    rt.state.shell.composer=rune::TextEdit::new("/model");rt.state.ui.panel=None;
    rt.state.send(&mut rune::Tx::default());assert!(rt.state.shell.composer.text.is_empty());
}
#[test]
fn failed_startup_configuration_exposes_real_retry_and_retains_draft() {
    let mut rt=runtime();
    let path=aether_core::agent::storage::config_dir().join("settings.json");
    let original=std::fs::read(&path).unwrap_or_else(|_|serde_json::to_vec(&aether_core::agent::AgentSettings::default()).unwrap());
    std::fs::write(&path,b"{ invalid configuration").unwrap();
    rt.state=AppState::new();
    let end=std::time::Instant::now()+std::time::Duration::from_secs(5);
    while !rt.state.controller.model().pending_commands.is_empty() {
        rt.state.controller.poll();assert!(std::time::Instant::now()<end);std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // Restore before assertions so later fixtures are safe even if this test fails.
    std::fs::write(&path,original).unwrap();
    rt.state.shell.composer=rune::TextEdit::new("retain through retry");
    rt.state.send(&mut rune::Tx::default());
    assert_eq!(rt.state.shell.composer.text,"retain through retry");
    assert!(!rt.state.controller.model().busy);
    rt.invalidate();rt.frame(0.016);
    click(&mut rt,"configuration-retry");
    assert!(rt.state.controller.model().pending_commands.values().any(|c|c=="get_settings"));
    while !rt.state.controller.model().pending_commands.is_empty() {
        (spec().poll.unwrap())(&mut rt.state,0.0,&mut rune::Tx::default());
        assert!(std::time::Instant::now()<end);std::thread::sleep(std::time::Duration::from_millis(5));
    }
    rt.invalidate();rt.frame(0.016);
    assert!(rt.find("configuration-retry").is_none());
    assert_eq!(rt.state.shell.composer.text,"retain through retry");
}