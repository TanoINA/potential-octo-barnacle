//! Native window-close policy. A hidden keep-running window keeps polling until
//! storage acknowledges every snapshot; a deadline is an unsaved decision, not success.
use crate::{
    app::AppState,
    native,
    widgets::{button, caption},
};
use rune::{El, Tx, col, row};
use rune_kit::Kit;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lifecycle {
    #[default]
    Open,
    Closing,
    Unsaved,
}
pub type CloseGroup = Arc<Mutex<HashSet<usize>>>;
pub fn close_group(count: usize) -> CloseGroup {
    Arc::new(Mutex::new((0..count).collect()))
}

/// App::keep_running hides the window after this hook. Never wait on storage here.
pub fn on_close(s: &mut AppState, tx: &mut Tx) {
    match s.shell.lifecycle {
        Lifecycle::Open => {
            native::cancel(s);
            s.controller.begin_shutdown();
            s.shell.lifecycle = Lifecycle::Closing;
            s.shell.window_finished = false;
        }
        Lifecycle::Closing => {}
        Lifecycle::Unsaved => {}
    }
    // This callback means the visible window is closing again, even if its
    // previous unsaved prompt was already shown. Reopen once for this close,
    // never once per poll (which would continuously steal focus).
    s.shell.close_prompt_shown = false;
    if s.shell.lifecycle == Lifecycle::Unsaved {
        reopen_once(s);
    }
    if !s.shell.window_finished {
        tx.wake_after(Duration::from_millis(50));
    }
}

/// Rune's OS/session-end hook cannot veto exit. Start cancellation as best effort;
/// ordinary close takes the acknowledgment-driven on_close/poll path instead.
pub fn on_quit(s: &mut AppState, _: &mut Tx) {
    if s.shell.lifecycle == Lifecycle::Open {
        native::cancel(s);
        s.controller.begin_shutdown();
    }
}

/// A completed window retained by Rune can be reopened by the native shell while
/// another window is still open. Re-register it before accepting edits again.
pub fn on_focus(s: &mut AppState, focused: bool, _: &mut Tx) {
    if focused && s.shell.window_finished && s.controller.cancel_shutdown() {
        s.shell.window_finished = false;
        s.shell.lifecycle = Lifecycle::Open;
        s.shell.close_prompt_shown = false;
        if let Some((id, group)) = &s.shell.close_group {
            group.lock().unwrap_or_else(|e| e.into_inner()).insert(*id);
        }
    }
}
fn reopen_once(s: &mut AppState) {
    if !s.shell.close_prompt_shown {
        s.shell.close_prompt_shown = true;
        if let Some(open) = &s.shell.window_open {
            open();
        }
    }
}
pub fn poll(s: &mut AppState, tx: &mut Tx) -> bool {
    advance(
        s,
        s.controller.shutdown_ready(),
        s.controller.shutdown_timed_out(),
        tx,
    )
}
fn finish_window(s: &mut AppState, tx: &mut Tx) {
    s.shell.window_finished = true;
    if let Some(hide) = &s.shell.window_hide {
        hide();
    }
    let last = s.shell.close_group.as_ref().is_none_or(|(id, group)| {
        let mut open = group.lock().unwrap_or_else(|e| e.into_inner());
        open.remove(id);
        open.is_empty()
    });
    if last {
        tx.quit();
    }
}
/// Apply the controller's shutdown observation to native window policy. Kept
/// separate from polling so native adapters and tests can exercise close events.
#[doc(hidden)]
pub fn advance(s: &mut AppState, ready: bool, timed_out: bool, tx: &mut Tx) -> bool {
    match s.shell.lifecycle {
        Lifecycle::Open => false,
        Lifecycle::Closing if s.shell.window_finished => false,
        Lifecycle::Closing if ready => {
            finish_window(s, tx);
            true
        }
        Lifecycle::Closing if timed_out => {
            s.shell.lifecycle = Lifecycle::Unsaved;
            reopen_once(s);
            tx.wake_after(Duration::from_millis(50));
            true
        }
        Lifecycle::Closing | Lifecycle::Unsaved => {
            // Continue draining in-flight commits even while a decision is shown.
            // Once Unsaved is visible, late acknowledgments do not dismiss it.
            tx.wake_after(Duration::from_millis(50));
            false
        }
    }
}
pub fn retry(s: &mut AppState, tx: &mut Tx) {
    if s.shell.lifecycle != Lifecycle::Unsaved {
        return;
    }
    match s.controller.retry_shutdown_saves() {
        Ok(()) => {
            s.shell.error = None;
            s.shell.lifecycle = Lifecycle::Closing;
            tx.wake_after(Duration::from_millis(50));
        }
        Err(error) => s.set_error(error),
    }
}
pub fn keep_open(s: &mut AppState, _: &mut Tx) {
    if s.shell.lifecycle != Lifecycle::Unsaved {
        return;
    }
    if s.controller.cancel_shutdown() {
        s.shell.lifecycle = Lifecycle::Open;
        s.shell.close_prompt_shown = false;
        s.shell.error = None;
    } else {
        s.set_error("Wait for the in-flight session operations, then choose Keep open again. Unsaved changes are still retained.");
    }
}
pub fn close_without_saving(s: &mut AppState, tx: &mut Tx) {
    if s.shell.lifecycle == Lifecycle::Unsaved {
        // This explicitly labeled action is the user's loss-risk confirmation.
        // In a multi-window process it closes only this window's participation.
        finish_window(s, tx);
        s.shell.lifecycle = Lifecycle::Closing;
    }
}
pub fn render(s: &AppState, kit: &Kit) -> Option<El> {
    if s.shell.lifecycle == Lifecycle::Open || s.shell.window_finished {
        return None;
    }
    let unsaved = s.shell.lifecycle == Lifecycle::Unsaved;
    let mut content = col().w_full().gap(14.0).padding(24.0).fill(kit.c.bg_base)
        .border(1.0, kit.c.border_muted).radius(12.0)
        .child(caption(if unsaved { "Unsaved changes — Aether has not closed" } else { "Saving before closing Aether…" }, kit))
        .child(caption(if unsaved {
            "The shutdown save deadline expired. Not all changes are confirmed saved. Pending saves may still finish. Retry saving, keep this window open, or explicitly accept the risk of losing unsaved changes."
        } else { "Stopping active runs and waiting for storage acknowledgments. The window will close only after saves are confirmed." }, kit));
    if let Some(error) = &s.shell.error {
        content = content.child(caption(error, kit));
    }
    if let Some(error) = &s.controller.model().error {
        content = content.child(caption(error, kit));
    }
    if unsaved {
        content = content.child(
            row()
                .w_full()
                .gap(10.0)
                .flex_wrap()
                .child(button("shutdown-retry", "Retry saving", kit, retry))
                .child(button("shutdown-keep-open", "Keep open", kit, keep_open))
                .child(button(
                    "shutdown-discard",
                    "Close without saving — unsaved changes may be lost",
                    kit,
                    close_without_saving,
                )),
        );
    }
    Some(
        col()
            .key("shutdown-modal")
            .role("dialog")
            .label(if unsaved {
                "Unsaved changes"
            } else {
                "Saving before closing"
            })
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(24.0)
            .fill(kit.c.bg_deep)
            // Neither Escape nor an outside click constitutes consent to data loss.
            .modal(false, |_: &mut AppState| {})
            .child(content),
    )
}
