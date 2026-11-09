//! Actual Rune Runtime controls and actual Controller/Backend fixtures. These
//! tests require no display server, network provider or production-profile writes.
use super::*;
use rune::{Env, Runtime};
use std::{
    sync::Once,
    time::{Duration, Instant},
};
static PROFILE: Once = Once::new();

fn runtime(panel: Panel) -> Runtime<AppState> {
    PROFILE.call_once(|| {
        let root =
            std::env::temp_dir().join(format!("aether-native-panels-{}", uuid::Uuid::new_v4()));
        aether_core::agent::profile::configure_storage_root(&root).unwrap();
    });
    let mut state = AppState::new();
    state.ui.open(panel, &mut state.controller);
    let mut rt = Runtime::new(
        state,
        |s, cx| render(s, cx, &Kit::neutral(true)).unwrap_or_else(col),
        (1200.0, 1800.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    rt
}
fn click(rt: &mut Runtime<AppState>, key: &str) {
    // Direct fixture writes are not input events: invalidate explicitly. Wait
    // for insertion/scroll transitions before hit-testing the native control.
    rt.invalidate();
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    let scroll_key = key.to_owned();
    rt.update(move |_, tx| {
        tx.instant();
        tx.scroll_into_view(scroll_key.clone());
    });
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("missing native control: {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
}
fn settle(rt: &mut Runtime<AppState>) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        rt.state.controller.poll();
        rt.state.ui.sync(&mut rt.state.controller);
        if rt.state.ui.tickets.is_empty() {
            break;
        }
        assert!(Instant::now() < until, "panel service operation timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
    rt.frame(0.016);
}
#[test]
fn basic_settings_save_uses_visible_fields_without_json_apply_step() {
    let mut rt = runtime(Panel::Settings);
    settle(&mut rt);
    rt.state
        .ui
        .set_form("settings.systemPrompt", "Native visible controls");
    rt.state.ui.set_form("settings.temperature", "0.25");
    rt.state.ui.set_form("settings.maxTurns", "31");
    rt.frame(0.016);
    click(&mut rt, "settings-save");
    settle(&mut rt);
    assert_eq!(
        rt.state.ui.settings_original["systemPrompt"],
        "Native visible controls"
    );
    assert_eq!(rt.state.ui.settings_original["temperature"], 0.25);
    assert_eq!(rt.state.ui.settings_original["maxTurns"], 31);
}

#[test]
fn conversation_actions_show_guarded_edit_modal_and_preserve_draft_on_scope_change() {
    let mut rt = runtime(Panel::History);
    settle(&mut rt);
    rt.state
        .controller
        .model_mut()
        .current
        .as_mut()
        .unwrap()
        .messages = serde_json::from_value(json!([
        {"id":"edit-me","role":"user","content":"Original prompt","timestamp":1},
        {"id":"answer","role":"assistant","content":"Original answer","timestamp":2}
    ]))
    .unwrap();
    rt.frame(0.016);
    rt.invalidate();
    rt.frame(0.016);
    click(&mut rt, "conversation-edit-edit-me");
    assert_eq!(rt.state.ui.form("conversation.text"), "Original prompt");
    assert!(rt.find("conversation-edit-submit").is_some());
    rt.state
        .ui
        .set_form("conversation.text", "Keep my edited draft");
    rt.state.controller.new_session();
    rt.frame(0.016);
    click(&mut rt, "conversation-edit-submit");
    assert_eq!(
        rt.state.ui.form("conversation.text"),
        "Keep my edited draft"
    );
    assert!(!rt.state.ui.pending("conversation-action"));
    assert!(
        rt.state
            .ui
            .validation_error
            .as_deref()
            .unwrap_or("")
            .contains("changed")
    );
}

#[test]
fn native_toolbar_opens_panels_and_dispatches_real_read() {
    let mut rt = runtime(Panel::About);
    settle(&mut rt);
    assert!(rt.state.ui.value("release").unwrap()["version"].is_string());
    click(&mut rt, "panel-open-Tools");
    settle(&mut rt);
    assert!(rt.state.ui.value("native-tools").unwrap().is_array());
    assert_eq!(rt.state.ui.panel, Some(Panel::Tools));
}
#[test]
fn native_multiline_field_validation_does_not_dispatch_bad_settings() {
    let mut rt = runtime(Panel::Settings);
    settle(&mut rt);
    rt.state.ui.set_form("settings.json", "[]");
    rt.frame(0.016);
    click(&mut rt, "settings-save");
    assert!(
        rt.state
            .ui
            .validation_error
            .as_deref()
            .unwrap_or("")
            .contains("object")
    );
    assert!(!rt.state.ui.pending("save-settings"));
}
#[test]
fn memory_editor_read_write_has_exact_workspace_and_acknowledged_refresh() {
    let mut rt = runtime(Panel::Memory);
    settle(&mut rt);
    let workspace =
        std::env::temp_dir().join(format!("aether-panel-memory-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    *rt.state.controller.backend.workspace.lock().unwrap() = workspace.clone();
    rt.state.controller.model_mut().workspace = workspace.clone();
    rt.state.ui.open(Panel::Memory, &mut rt.state.controller);
    settle(&mut rt);
    rt.state.ui.set_form(
        "memory.content",
        "# Verified native panel\nActual service round trip.\n",
    );
    rt.frame(0.016);
    click(&mut rt, "memory-save");
    assert!(rt.state.ui.confirmation.is_some());
    assert!(!std::path::Path::new(&workspace).join("AGENTS.md").exists());
    click(&mut rt, "confirm-accept");
    assert!(rt.state.ui.pending("memory-save"));
    settle(&mut rt);
    assert!(rt.state.ui.replies["memory-save"].error.is_none());
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&workspace).join("AGENTS.md")).unwrap(),
        rt.state.ui.form("memory.content")
    );
    assert!(
        rt.state
            .ui
            .form("memory.content")
            .contains("Actual service round trip")
    );
}
#[test]
fn confirmation_cannot_cross_workspace_scope() {
    let mut rt = runtime(Panel::Memory);
    settle(&mut rt);
    rt.state.ui.mutate(
        &mut rt.state.controller,
        Request::new(
            "save_workspace_agents_md",
            json!({"content":"do not write"}),
            "guard-test",
        ),
        None,
        "save",
    );
    rt.state
        .controller
        .model_mut()
        .workspace
        .push_str("-changed");
    rt.frame(0.016);
    click(&mut rt, "confirm-accept");
    assert!(!rt.state.ui.pending("guard-test"));
}
#[test]
fn actual_supported_service_inventory_has_argument_templates() {
    let rt = runtime(Panel::About);
    for command in aether_core::service::Backend::supported_commands() {
        let args = argument_template(command, &rt.state.controller);
        assert!(args.is_object(), "{command} needs an object");
        if ![
            "get_settings",
            "list_sessions",
            "list_all_artifacts",
            "get_workspace_cwd",
            "list_native_tools",
            "git_status",
            "git_fetch",
            "git_push",
            "get_release_info",
            "check_release_update",
        ]
        .contains(command)
        {
            assert!(
                !args.as_object().unwrap().is_empty(),
                "{command} must expose its real argument schema"
            );
        }
    }
}

#[test]
fn shell_control_runs_real_bounded_command_and_returns_actual_stdout() {
    let mut rt = runtime(Panel::Shell);
    settle(&mut rt);
    rt.state
        .ui
        .set_form("shell.command", "echo AETHER_NATIVE_PANEL_VERIFIED");
    rt.frame(0.016);
    click(&mut rt, "shell-run");
    assert!(rt.state.ui.pending("shell"));
    settle(&mut rt);
    let reply = &rt.state.ui.replies["shell"];
    assert!(reply.error.is_none(), "{:?}", reply.error);
    assert!(
        reply.value.as_ref().unwrap()["stdout"]
            .as_str()
            .unwrap()
            .contains("AETHER_NATIVE_PANEL_VERIFIED")
    );
    assert_eq!(reply.value.as_ref().unwrap()["exitCode"], 0);
}

fn menu_runtime() -> Runtime<AppState> {
    // Share the real temporary profile setup; never touch the operator's profile.
    drop(runtime(Panel::About));
    let mut state = AppState::new();
    state.controller.new_session();
    state.controller.model_mut().current.as_mut().unwrap().messages = serde_json::from_value(json!([
        {"id":"menu-user","role":"user","content":"# Heading\n\nA **bold** [link](https://example.com).","timestamp":1},
        {"id":"menu-answer","role":"assistant","content":"Answer","timestamp":2}
    ])).unwrap();
    let mut rt = Runtime::new(
        state,
        |s, cx| {
            let kit = Kit::neutral(true);
            render(s, cx, &kit).unwrap_or_else(|| {
                wrap_message(
                    s,
                    "menu-user",
                    rune::el()
                        .key("context-test-row")
                        .size(500.0, 100.0)
                        .child(text("Message area")),
                    &kit,
                )
            })
        },
        (800.0, 600.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    rt
}
fn settle_menu(rt: &mut Runtime<AppState>) {
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
}
fn right_click_message(rt: &mut Runtime<AppState>) {
    let (x, y) = rt.find("context-test-row").unwrap();
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle_menu(rt);
    let (_, id, point) = rt
        .state
        .ui
        .message_context
        .as_ref()
        .expect("native secondary press opens message menu");
    assert_eq!(id, "menu-user");
    assert_eq!((point.x, point.y), (x, y));
    assert!(rt.find("message-context-menu-user-menu").is_some());
}
#[test]
fn message_context_menu_copies_raw_and_plain_text_through_native_clipboard() {
    let mut rt = menu_runtime();
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    right_click_message(&mut rt);
    click(&mut rt, "message-context-menu-user-item0");
    settle_menu(&mut rt);
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some("# Heading\n\nA **bold** [link](https://example.com).")
    );
    assert!(rt.state.ui.message_context.is_none());
    right_click_message(&mut rt);
    click(&mut rt, "message-context-menu-user-item1");
    settle_menu(&mut rt);
    let plain = clipboard.text().unwrap();
    assert!(
        plain.contains("Heading") && plain.contains("A bold link."),
        "{plain:?}"
    );
    assert!(!plain.contains("**") && !plain.contains("https://"));
    assert!(rt.state.ui.message_context.is_none());
}
#[test]
fn message_context_keyboard_escape_restores_focus_and_edit_opens_real_editor() {
    use rune::{Key, KeyEvent, Mods};
    let mut rt = menu_runtime();
    click(&mut rt, "context-test-row");
    rt.key(KeyEvent {
        key: Key::ContextMenu,
        mods: Mods::default(),
    });
    settle_menu(&mut rt);
    assert!(rt.find("message-context-menu-user-menu").is_some());
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    settle_menu(&mut rt);
    assert!(rt.state.ui.message_context.is_none());
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("context-test-row") && e.focused),
        "Escape must return focus to the message"
    );
    right_click_message(&mut rt);
    click(&mut rt, "message-context-menu-user-item2");
    assert!(rt.state.ui.conversation_edit.is_some());
    assert!(rt.find("conversation-edit-submit").is_some());
    assert_eq!(
        rt.state.ui.form("conversation.text"),
        "# Heading\n\nA **bold** [link](https://example.com)."
    );
}
#[test]
fn history_panel_exposes_role_specific_actions_and_latest_undo() {
    let mut rt = runtime(Panel::History);
    settle(&mut rt);
    rt.state
        .controller
        .model_mut()
        .current
        .as_mut()
        .unwrap()
        .messages = serde_json::from_value(json!([
        {"id":"prompt","role":"user","content":"Prompt","timestamp":1},
        {"id":"answer","role":"assistant","content":"Answer","timestamp":2}
    ]))
    .unwrap();
    rt.invalidate();
    rt.frame(0.016);
    for key in [
        "conversation-edit-prompt",
        "conversation-retry-prompt",
        "conversation-rollback-answer",
        "conversation-undo",
    ] {
        assert!(rt.find(key).is_some(), "missing {key}");
    }
    assert!(rt.find("conversation-edit-answer").is_none());
    click(&mut rt, "conversation-retry-prompt");
    assert!(
        rt.state.ui.confirmation.is_some(),
        "Retry opens confirmation before dispatch"
    );
    assert_eq!(
        rt.state.ui.confirmation.as_ref().unwrap().request.command,
        "retry_turn"
    );
    assert!(
        !rt.state.ui.pending("conversation-action"),
        "confirmation must precede dispatch"
    );
}
#[test]
fn stale_completion_preserves_current_validation_and_edit_draft() {
    use aether_state::CommandResult;
    let mut rt = runtime(Panel::History);
    settle(&mut rt);
    rt.state.ui.request(
        &mut rt.state.controller,
        "get_release_info",
        json!({}),
        "old-scope",
    );
    let id = *rt.state.ui.tickets.keys().next().unwrap();
    rt.state.controller.new_session();
    rt.state.ui.set_form("conversation.text", "Keep this draft");
    rt.state.ui.validation_error = Some("Current draft validation".into());
    rt.state.controller.model_mut().results.insert(
        id,
        CommandResult {
            id,
            command: "get_release_info".into(),
            session_id: None,
            generation: 0,
            value: None,
            error: Some("old scope failed".into()),
            failure_phase: None,
        },
    );
    rt.state.ui.sync(&mut rt.state.controller);
    assert_eq!(
        rt.state.ui.validation_error.as_deref(),
        Some("Current draft validation")
    );
    assert_eq!(rt.state.ui.form("conversation.text"), "Keep this draft");
    assert!(!rt.state.ui.pending("old-scope"));
}
#[test]
fn failed_edit_acknowledgement_keeps_editor_and_draft() {
    let mut rt = menu_runtime();
    rt.state
        .ui
        .conversation_action(&mut rt.state.controller, "edit_and_resend", "menu-user");
    rt.state
        .ui
        .set_form("conversation.text", "Preserve after failed save");
    rt.invalidate();
    rt.frame(0.016);
    click(&mut rt, "conversation-edit-submit");
    assert!(rt.state.ui.confirmation.is_some());
    assert!(
        rt.state.ui.conversation_edit.is_some(),
        "confirmation must retain draft ownership"
    );
    click(&mut rt, "confirm-accept");
    // The fixture is deliberately only a viewport projection, not a committed
    // canonical turn. The real controller must reject it rather than save it.
    settle(&mut rt);
    assert!(rt.state.ui.replies["conversation-action"].error.is_some());
    assert!(rt.state.ui.conversation_edit.is_some());
    assert_eq!(
        rt.state.ui.form("conversation.text"),
        "Preserve after failed save"
    );
}

#[test]
fn service_dispatch_inventory_and_template_keys_match_full_source() {
    let rt = runtime(Panel::About);
    let source = include_str!("../../../aether-core/src/service.rs");
    let dispatch = source.split("match command {").nth(1).unwrap()
        .split("\n    pub fn supported_commands()").next().unwrap();
    let inventory = aether_core::service::Backend::supported_commands();
    for line in dispatch.lines().map(str::trim).filter(|line| line.starts_with('"')) {
        let Some((name, _)) = line[1..].split_once('"') else { continue; };
        if name == "install_release_update" { continue; } // explicitly unsupported
        assert!(inventory.contains(&name), "implemented command missing from inventory: {name}");
        let args = argument_template(name, &rt.state.controller);
        // Includes typed argument::<T> and optional_argument calls, not a sampled list.
        for argument in line.split("(&args, \"").skip(1) {
            let key = argument.split('"').next().unwrap();
            assert!(args.get(key).is_some(), "{name} missing actual service argument {key}: {args}");
        }
    }
    assert_eq!(argument_template("recover_save_ack", &rt.state.controller),
        json!({"sessionId":UiState::session(&rt.state.controller).unwrap(),"proof":null}));
}
#[test]
fn review_uses_session_provider_and_model_override() {
    let mut rt = runtime(Panel::Review);
    settle(&mut rt);
    rt.state.controller.model_mut().settings.providers = vec![
        ProviderConfig { id:"global".into(), name:"Global".into(), model:"global-model".into(), ..Default::default() },
        ProviderConfig { id:"session".into(), name:"Session".into(), model:"base-model".into(), ..Default::default() },
    ];
    rt.state.controller.model_mut().settings.active_provider_id = "global".into();
    rt.state.controller.model_mut().current.as_mut().unwrap().model_override =
        Some(serde_json::from_value(json!({"providerId":"session","model":"session-model"})).unwrap());
    let args = argument_template("review_session", &rt.state.controller);
    assert_eq!(args["provider"]["id"], "session");
    assert_eq!(args["provider"]["model"], "session-model");
    rt.invalidate(); rt.frame(0.016);
    assert!(rt.elements().iter().any(|e| e.text.as_deref().is_some_and(|t|t.contains("Session / session-model"))));
}
#[test]
fn lifecycle_owned_operations_show_schema_but_cannot_dispatch() {
    let mut rt = runtime(Panel::About);
    settle(&mut rt);
    rt.state.ui.expanded.insert("advanced-service".into());
    for command in ["recover_save_ack","replace_session_checked","save_session","checkpoint_session","checkpoint_session_delta","refresh_session_window"] {
        click(&mut rt, &format!("advanced-select-{command}"));
        assert_eq!(rt.state.ui.form("advanced.command"), command);
        let before = rt.state.controller.model().pending_commands.len();
        click(&mut rt, "advanced-dispatch");
        assert!(!rt.state.ui.pending("advanced"), "{command} must use typed Controller methods");
        assert_eq!(rt.state.controller.model().pending_commands.len(),before);
    }
}
#[test]
fn settings_read_completion_preserves_fields_edited_after_dispatch() {
    let mut rt = runtime(Panel::Settings);
    settle(&mut rt);
    rt.state.ui.request(&mut rt.state.controller,"get_settings",json!({}),"settings");
    rt.state.ui.set_form("settings.systemPrompt","Keep this unsaved edit");
    settle(&mut rt);
    assert_eq!(rt.state.ui.form("settings.systemPrompt"),"Keep this unsaved edit");
    assert_eq!(rt.state.ui.settings_original["systemPrompt"],json!(rt.state.controller.model().settings.system_prompt));
}