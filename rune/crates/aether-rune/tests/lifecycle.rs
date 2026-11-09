//! Close-policy integration tests use their own disposable process-wide profile.

use aether_rune::AppState;
use aether_rune::lifecycle::*;
use aether_rune::{app::ShellState, panels::UiState, rich_content::RichContent};
use rune::Tx;
use std::sync::Arc;
use std::{
    cell::RefCell,
    sync::atomic::{AtomicUsize, Ordering},
};
fn state() -> AppState {
    static PROFILE: std::sync::Once = std::sync::Once::new();
    PROFILE.call_once(|| {
        aether_core::agent::profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-lifecycle-tests-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
    AppState {
        controller: aether_state::Controller::new().unwrap(),
        ui: UiState::default(),
        rich: RefCell::new(RichContent::new(Arc::new(|| {}))),
        shell: ShellState::default(),
    }
}
fn quits(tx: &Tx) -> bool {
    format!("{tx:?}").contains("shell: Some(Quit)")
}
#[test]
fn native_close_hook_starts_shutdown_and_wakes_without_premature_quit() {
    let mut s = state();
    let mut tx = Tx::default();
    on_close(&mut s, &mut tx);
    assert_eq!(s.shell.lifecycle, Lifecycle::Closing);
    assert!(s.controller.is_shutting_down());
    assert!(!quits(&tx));
    assert!(format!("{tx:?}").contains("50ms"));
    let mut waiting = Tx::default();
    assert!(!advance(&mut s, false, false, &mut waiting));
    assert!(!quits(&waiting));
    let mut ready = Tx::default();
    assert!(advance(&mut s, true, false, &mut ready));
    assert!(quits(&ready));
}
#[test]
fn timeout_reopens_once_and_never_claims_saved_or_quits() {
    let mut s = state();
    let opens = Arc::new(AtomicUsize::new(0));
    let count = opens.clone();
    s.shell.window_open = Some(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    on_close(&mut s, &mut Tx::default());
    let mut tx = Tx::default();
    assert!(advance(&mut s, false, true, &mut tx));
    assert_eq!(s.shell.lifecycle, Lifecycle::Unsaved);
    assert_eq!(opens.load(Ordering::SeqCst), 1);
    assert!(!quits(&tx));
    for _ in 0..10 {
        assert!(!advance(&mut s, true, true, &mut tx));
    }
    assert_eq!(opens.load(Ordering::SeqCst), 1);
    assert!(
        !quits(&tx),
        "late saves do not discard an explicit unsaved decision"
    );
    on_close(&mut s, &mut tx);
    assert_eq!(
        opens.load(Ordering::SeqCst),
        2,
        "closing the prompt reopens it once"
    );
    advance(&mut s, false, true, &mut tx);
    assert_eq!(opens.load(Ordering::SeqCst), 2);
}
#[test]
fn keep_open_resumes_controller_and_preserves_composer_draft() {
    let mut s = state();
    s.shell.composer.text = "draft survives".into();
    on_close(&mut s, &mut Tx::default());
    advance(&mut s, false, true, &mut Tx::default());
    keep_open(&mut s, &mut Tx::default());
    assert_eq!(s.shell.lifecycle, Lifecycle::Open);
    assert!(!s.controller.is_shutting_down());
    assert_eq!(s.shell.composer.text, "draft survives");
    assert!(s.controller.model().pending_commands.is_empty());
}
#[test]
fn loss_risk_action_is_explicit_and_cannot_quit_from_open_state() {
    let mut s = state();
    let mut tx = Tx::default();
    close_without_saving(&mut s, &mut tx);
    assert!(!quits(&tx));
    on_close(&mut s, &mut tx);
    advance(&mut s, false, true, &mut tx);
    close_without_saving(&mut s, &mut tx);
    assert!(quits(&tx));
}
#[test]
fn one_window_cannot_quit_another_and_reopened_window_reregisters() {
    let group = close_group(2);
    let mut a = state();
    let mut b = state();
    a.shell.close_group = Some((0, group.clone()));
    b.shell.close_group = Some((1, group.clone()));
    on_close(&mut a, &mut Tx::default());
    let mut first = Tx::default();
    advance(&mut a, true, false, &mut first);
    assert!(!quits(&first));
    on_focus(&mut a, true, &mut Tx::default());
    assert_eq!(a.shell.lifecycle, Lifecycle::Open);
    assert_eq!(group.lock().unwrap().len(), 2);
    on_close(&mut a, &mut Tx::default());
    advance(&mut a, true, false, &mut Tx::default());
    on_close(&mut b, &mut Tx::default());
    let mut last = Tx::default();
    advance(&mut b, true, false, &mut last);
    assert!(quits(&last));
}
#[test]
fn quit_hook_requests_cancellation_but_is_not_a_veto() {
    let mut s = state();
    on_quit(&mut s, &mut Tx::default());
    assert!(s.controller.is_shutting_down());
}

#[test]
fn actual_rune_modal_traps_focus_keeps_keyboard_actions_and_resumes() {
    use rune::{Env, Key, KeyEvent, Mods, Runtime};
    let mut s = state();
    s.shell.composer.text = "untouched draft".into();
    on_close(&mut s, &mut Tx::default());
    advance(&mut s, false, true, &mut Tx::default());
    let mut rt = Runtime::new(
        s,
        aether_rune::view::view,
        (900.0, 600.0),
        Env::new(true, 1.0),
    );
    rt.on_key = aether_rune::app::spec().on_key;
    rt.frame(0.0);
    assert!(rt.find("shutdown-modal").is_some());
    assert!(rt.find("shutdown-retry").is_some());
    assert!(rt.find("shutdown-keep-open").is_some());
    assert!(rt.find("shutdown-discard").is_some());
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
    assert_eq!(rt.state.shell.lifecycle, Lifecycle::Unsaved);
    for _ in 0..8 {
        rt.key(KeyEvent {
            key: Key::Tab,
            mods: Mods::default(),
        });
        rt.frame(0.016);
    }
    let elements = rt.elements();
    let focused: Vec<_> = elements.iter().filter(|e| e.focused).collect();
    assert!(
        !focused.is_empty(),
        "Tab must still reach unsaved dialog controls"
    );
    assert!(focused.iter().all(|e| e.path.contains("shutdown-modal")));
    let before = rt
        .state
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    rt.key(KeyEvent {
        key: Key::Command('n'),
        mods: Mods {
            ctrl: true,
            ..Default::default()
        },
    });
    assert_eq!(
        rt.state
            .controller
            .model()
            .current
            .as_ref()
            .unwrap()
            .session
            .id,
        before
    );
    let (x, y) = rt.find("shutdown-keep-open").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.016);
    assert_eq!(rt.state.shell.lifecycle, Lifecycle::Open);
    assert!(!rt.state.controller.is_shutting_down());
    assert_eq!(rt.state.shell.composer.text, "untouched draft");
    assert!(rt.find("shutdown-modal").is_none());
}
#[test]
fn retry_button_does_not_fake_success_when_no_dirty_snapshot_exists() {
    let mut s = state();
    on_close(&mut s, &mut Tx::default());
    advance(&mut s, false, true, &mut Tx::default());
    let mut tx = Tx::default();
    retry(&mut s, &mut tx);
    assert_eq!(s.shell.lifecycle, Lifecycle::Unsaved);
    assert!(s.shell.error.is_some());
    assert!(!quits(&tx));
}
