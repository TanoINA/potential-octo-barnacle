//! Native export consent regression tests; only isolated temporary files are used.
use aether_rune::app::{AppState, spec};
use rune::{Env, Runtime, Tx};
use std::sync::Once;
static PROFILE: Once = Once::new();
fn runtime() -> Runtime<AppState> {
    PROFILE.call_once(|| {
        aether_core::agent::profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-rune-export-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
    let mut rt = Runtime::new(
        AppState::new(),
        aether_rune::view::view,
        (1280.0, 800.0),
        Env::new(true, 1.0),
    );
    rt.on_key = spec().on_key;
    rt.state.new_session(&mut Tx::default());
    rt.frame(0.0);
    rt
}
fn intent(rt: &mut Runtime<AppState>, args: serde_json::Value) {
    rt.update(move |s, tx| {
        s.controller
            .model_mut()
            .ui_actions
            .push(aether_state::UiAction {
                panel: "export".into(),
                args: args.to_string(),
            });
        (spec().poll.unwrap())(s, 0.0, tx);
    });
    rt.frame(0.016);
}
fn click(rt: &mut Runtime<AppState>, key: &str) {
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("missing {key}: {:?}", rt.state.shell.error));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.016);
}
fn settle(rt: &mut Runtime<AppState>) {
    for _ in 0..400 {
        (spec().poll.unwrap())(&mut rt.state, 0.0, &mut Tx::default());
        if rt.state.controller.model().pending_commands.is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!(
        "backend did not settle: {:?}",
        rt.state.controller.model().pending_commands
    );
}
fn persisted(rt: &mut Runtime<AppState>) -> String {
    settle(rt);
    let mut data = rt.state.controller.model().current.clone().unwrap();
    data.messages=serde_json::from_value(serde_json::json!([{"id":"export-fixture","role":"user","content":"Native export sentinel","timestamp":1}])).unwrap();
    data.session.message_count = 1;
    let id = data.session.id.clone();
    rt.state
        .controller
        .runtime
        .block_on(
            rt.state
                .controller
                .backend
                .dispatch("save_session", serde_json::json!({"data":data})),
        )
        .unwrap();
    id
}
fn no_export(rt: &Runtime<AppState>) {
    assert!(
        !rt.state
            .controller
            .model()
            .pending_commands
            .values()
            .any(|c| c == "export_session")
    );
}
#[test]
fn explicit_destination_requires_native_confirmation_before_dispatch() {
    let mut rt = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("consented.json");
    let id = rt
        .state
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    intent(
        &mut rt,
        serde_json::json!({"sessionId":id,"format":"json","destination":path}),
    );
    assert!(
        rt.find("export-confirm-modal").is_some(),
        "export intent must open the native confirmation modal, got {:?}",
        rt.state.shell.error
    );
    assert!(
        !path.exists(),
        "parsing an explicit destination is not permission to write"
    );
    assert!(
        !rt.state
            .controller
            .model()
            .pending_commands
            .values()
            .any(|c| c == "export_session")
    );
}
#[test]
fn explicit_confirmation_click_writes_backend_json_content() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("confirmed.json");
    intent(
        &mut rt,
        serde_json::json!({"sessionId":id,"format":"json","destination":path}),
    );
    assert!(!path.exists());
    assert!(
        rt.elements()
            .iter()
            .find(|e| e.key.as_deref() == Some("composer"))
            .unwrap()
            .inert
    );
    assert!(rt.elements().iter().any(|e| {
        e.text
            .as_deref()
            .is_some_and(|text| text.contains(path.to_str().unwrap()))
    }));
    click(&mut rt, "export-confirm");
    settle(&mut rt);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("no export: {e}; {:?}", rt.state.shell.error));
    let data: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(data["session"]["id"], id);
    assert!(content.contains("Native export sentinel"));
    assert!(
        rt.state
            .shell
            .export
            .status
            .as_deref()
            .is_some_and(|text| text.contains("Exported to"))
    );
    assert!(rt.state.shell.export.confirmation.is_none());
}
#[test]
fn save_dialog_selection_exports_every_supported_format_without_second_confirmation() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    for (format, extension) in [("json", "json"), ("markdown", "md"), ("html", "html")] {
        let path = dir.path().join(format!("saved.{extension}"));
        intent(&mut rt, serde_json::json!({"sessionId":id,"format":format}));
        let (kind, options) = rt
            .file_dialog()
            .expect("actual runtime Save dialog request");
        assert_eq!(kind, rune::FileDialogKind::Save);
        assert!(!options.title.is_empty());
        assert!(
            options
                .file_name
                .as_deref()
                .unwrap()
                .ends_with(&format!(".{extension}"))
        );
        assert_eq!(options.filters[0].1, vec![extension.to_owned()]);
        no_export(&rt);
        assert!(!path.exists());
        rt.resolve_file_dialog(Some(path.clone())).unwrap();
        rt.frame(0.016);
        assert!(
            rt.find("export-confirm-modal").is_none(),
            "Save selection is already explicit consent"
        );
        settle(&mut rt);
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{format}: {e}; {:?}", rt.state.shell.error));
        assert!(
            content.contains("Native export sentinel"),
            "{format} must export canonical session content"
        );
        if format == "json" {
            assert!(serde_json::from_str::<serde_json::Value>(&content).is_ok());
        }
        if format == "html" {
            assert!(content.contains("<html"));
        }
    }
}
#[test]
fn cancel_button_and_escape_do_not_create_or_overwrite_files() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("untouched.json");
    std::fs::write(&path, "keep this temporary fixture").unwrap();
    for escape in [false, true] {
        intent(
            &mut rt,
            serde_json::json!({"sessionId":id,"format":"json","destination":path}),
        );
        if escape {
            rt.key(rune::KeyEvent {
                key: rune::Key::Escape,
                mods: rune::Mods::default(),
            });
            rt.frame(0.016);
        } else {
            click(&mut rt, "export-cancel");
        }
        assert!(rt.state.shell.export.confirmation.is_none());
        no_export(&rt);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "keep this temporary fixture"
        );
    }
}
#[test]
fn save_dialog_cancellation_is_silent() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    intent(
        &mut rt,
        serde_json::json!({"sessionId":id,"format":"markdown"}),
    );
    rt.resolve_file_dialog(None).unwrap();
    rt.frame(0.016);
    no_export(&rt);
    assert!(rt.state.shell.error.is_none());
    assert!(rt.state.shell.export.status.is_none());
}
#[test]
fn stale_save_dialog_cannot_write_after_controller_a_b_a_navigation() {
    let mut rt = runtime();
    let a = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stale.json");
    intent(&mut rt, serde_json::json!({"sessionId":a,"format":"json"}));
    let generation = rt.state.controller.navigation_generation();
    // Do not synchronize native state in between: an ID-only fence would miss this.
    rt.state.controller.new_session();
    rt.state.controller.open_session(&a);
    assert_eq!(
        rt.state
            .controller
            .model()
            .current
            .as_ref()
            .unwrap()
            .session
            .id,
        a
    );
    assert_ne!(rt.state.controller.navigation_generation(), generation);
    rt.resolve_file_dialog(Some(path.clone())).unwrap();
    rt.frame(0.016);
    settle(&mut rt);
    assert!(!path.exists());
    no_export(&rt);
    assert!(rt.state.shell.error.is_none());
}
#[test]
fn stale_confirmation_click_cannot_write_after_a_b_a_navigation() {
    let mut rt = runtime();
    let a = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stale-confirm.json");
    intent(
        &mut rt,
        serde_json::json!({"sessionId":a,"format":"json","destination":path}),
    );
    rt.state.controller.new_session();
    rt.state.controller.open_session(&a);
    click(&mut rt, "export-confirm");
    settle(&mut rt);
    assert!(!path.exists());
    no_export(&rt);
}
#[test]
fn workspace_change_discards_late_save_dialog_selection() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wrong-workspace.json");
    intent(&mut rt, serde_json::json!({"sessionId":id,"format":"json"}));
    rt.state.controller.model_mut().workspace = dir.path().to_string_lossy().into_owned();
    rt.resolve_file_dialog(Some(path.clone())).unwrap();
    rt.frame(0.016);
    no_export(&rt);
    assert!(!path.exists());
    assert!(rt.state.shell.error.is_none());
}
#[test]
fn malformed_export_intents_never_open_dialogs_or_dispatch_writes() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    for args in [
        serde_json::json!({"sessionId":id,"format":"pdf"}),
        serde_json::json!({"sessionId":id,"format":"JSON"}),
        serde_json::json!({"sessionId":id,"format":"json","destination":42}),
        serde_json::json!({"sessionId":id,"format":"json","destination":""}),
        serde_json::json!({"sessionId":"foreign","format":"json"}),
        serde_json::json!({"format":"json"}),
        serde_json::json!([]),
    ] {
        rt.state.shell.error = None;
        intent(&mut rt, args);
        assert!(rt.state.shell.error.is_some());
        assert!(rt.file_dialog().is_none());
        assert!(rt.state.shell.export.confirmation.is_none());
        no_export(&rt);
    }
    rt.update(|s, tx| {
        assert!(aether_rune::export::handle(s, "not json", tx));
    });
    assert!(
        rt.state
            .shell
            .error
            .as_deref()
            .unwrap()
            .contains("Invalid export intent")
    );
}
#[test]
fn backend_export_failure_is_reported_by_native_poll() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    intent(
        &mut rt,
        serde_json::json!({"sessionId":id,"format":"json","destination":dir.path()}),
    );
    click(&mut rt, "export-confirm");
    settle(&mut rt);
    assert!(
        rt.state
            .shell
            .error
            .as_deref()
            .is_some_and(|e| e.starts_with("Export:"))
    );
    assert!(rt.state.shell.export.status.is_none());
}
#[test]
fn relative_destination_is_bound_to_originating_workspace_in_modal() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    rt.state.controller.model_mut().workspace = dir.path().to_string_lossy().into_owned();
    intent(
        &mut rt,
        serde_json::json!({"sessionId":id,"format":"json","destination":"relative.json"}),
    );
    let path = dir.path().join("relative.json");
    assert!(rt.elements().iter().any(|e| {
        e.text
            .as_deref()
            .is_some_and(|text| text.contains(path.to_str().unwrap()))
    }));
    click(&mut rt, "export-cancel");
    assert!(!path.exists());
}
#[test]
fn export_success_status_does_not_follow_navigation_to_another_session() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("status.json");
    intent(
        &mut rt,
        serde_json::json!({"sessionId":id,"format":"json","destination":path}),
    );
    click(&mut rt, "export-confirm");
    settle(&mut rt);
    assert!(rt.state.shell.export.status.is_some());
    rt.state.controller.new_session();
    (spec().poll.unwrap())(&mut rt.state, 0.0, &mut Tx::default());
    assert!(
        rt.state.shell.export.status.is_none(),
        "export status belongs to the originating session"
    );
}
#[test]
fn stale_backend_export_error_never_enters_the_current_shell() {
    let mut rt = runtime();
    let id = persisted(&mut rt);
    let dir = tempfile::tempdir().unwrap();
    intent(
        &mut rt,
        serde_json::json!({"sessionId":id,"format":"json","destination":dir.path()}),
    );
    click(&mut rt, "export-confirm");
    rt.state.controller.new_session();
    rt.state.controller.open_session(&id);
    settle(&mut rt);
    assert!(
        rt.state.shell.error.is_none(),
        "late export errors must not enter a later visit: {:?}",
        rt.state.shell.error
    );
    assert!(rt.state.shell.export.status.is_none());
}
