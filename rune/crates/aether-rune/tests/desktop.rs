//! Disposable process profile; no global shortcut registrations, toasts or audio devices.
use aether_rune::{
    AppState,
    desktop::{sync, toggle_history},
    panels::Panel,
};
use aether_state::CompletedRun;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn state() -> AppState {
    static PROFILE: std::sync::Once = std::sync::Once::new();
    PROFILE.call_once(|| {
        aether_core::agent::profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-desktop-test-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
    AppState::new()
}
#[test]
fn alt_space_reopens_same_window_and_preserves_draft_session_and_focus() {
    let mut s = state();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    s.shell.window_open = Some(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    s.shell.composer.text = "do not send this".into();
    s.shell.composer.focused = true;
    let session = s
        .controller
        .model()
        .current
        .as_ref()
        .map(|d| d.session.id.clone());
    toggle_history(&mut s, &mut rune::Tx::default());
    assert_eq!(s.ui.panel, Some(Panel::History));
    s.shell.composer.focused = false;
    toggle_history(&mut s, &mut rune::Tx::default());
    assert_eq!(s.ui.panel, None);
    assert!(s.shell.composer.focused);
    assert_eq!(s.shell.composer.text, "do not send this");
    assert_eq!(
        session,
        s.controller
            .model()
            .current
            .as_ref()
            .map(|d| d.session.id.clone())
    );
    assert!(!s.controller.selected_run_active());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[test]
fn sync_drains_offscreen_inbox_and_surfaces_shortcut_error_without_os_calls() {
    let mut s = state();
    s.controller.model_mut().settings.notifications_enabled = false;
    s.controller.model_mut().settings.sound_chime_enabled = false;
    s.controller.model_mut().completed_runs.push(CompletedRun {
        session_id: "offscreen".into(),
        run_id: "r".into(),
    });
    s.shell.desktop.install_native(
        Arc::new(|| vec!["Global Alt+Space unavailable: shortcut is already registered".into()]),
        Arc::new(|_| {}),
    );
    assert!(sync(&mut s));
    assert!(s.controller.model().completed_runs.is_empty());
    assert!(
        s.shell
            .error
            .as_deref()
            .unwrap()
            .contains("already registered")
    );
}
