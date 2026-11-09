//! Native-only desktop effects. Default/test/headless state never touches the OS.
//! Completion payloads contain identifiers only; toasts never include conversation text.
use crate::{app::AppState, panels::Panel};
use aether_state::CompletedRun;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

pub fn should_register_shortcut(primary: bool, headless: bool) -> bool {
    primary && !headless
}

#[derive(Default)]
pub struct DesktopState {
    enabled: bool,
    seen: VecDeque<(String, String)>,
    worker: Option<Worker>,
    waker: Option<rune::Waker>,
    shortcut_errors: Option<Arc<dyn Fn() -> Vec<String> + Send + Sync>>,
    activation: Option<Arc<dyn Fn(String) + Send + Sync>>,
    return_composer_focus: Option<bool>,
    return_focus: Option<String>,
}
impl DesktopState {
    /// Called only by main's native window builder, never by AppState::new/tests.
    pub fn install_native(
        &mut self,
        errors: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
        activation: Arc<dyn Fn(String) + Send + Sync>,
    ) {
        self.enabled = true;
        self.shortcut_errors = Some(errors);
        self.activation = Some(activation);
    }
    pub fn set_waker(&mut self, waker: rune::Waker) {
        self.waker = Some(waker);
    }
    fn prepare(&mut self, event: CompletedRun, notify: bool, chime: bool) -> Option<Job> {
        let key = (event.session_id.clone(), event.run_id.clone());
        if self.seen.contains(&key) {
            return None;
        }
        self.seen.push_back(key);
        if self.seen.len() > 128 {
            self.seen.pop_front();
        }
        (self.enabled && (notify || chime)).then_some(Job {
            event,
            notify,
            chime,
        })
    }
    fn submit(&mut self, job: Job) -> Result<(), String> {
        if self.worker.is_none() {
            let waker = self
                .waker
                .clone()
                .ok_or("Native completion worker has no wake bridge")?;
            self.worker = Some(Worker::start(waker)?);
        }
        self.worker
            .as_ref()
            .unwrap()
            .tx
            .try_send(job)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => {
                    "Completion effects queue is full (capacity 2); effect skipped".into()
                }
                mpsc::TrySendError::Disconnected(_) => "Native completion worker stopped".into(),
            })
    }
}
struct Job {
    event: CompletedRun,
    notify: bool,
    chime: bool,
}
trait CompletionSink {
    fn notify(&mut self, event: &CompletedRun) -> Result<(), String>;
    fn chime(&mut self) -> Result<(), String>;
}
fn perform(sink: &mut dyn CompletionSink, job: Job) -> Result<(), String> {
    let mut errors = Vec::new();
    if job.notify {
        if let Err(error) = sink.notify(&job.event) {
            errors.push(error);
        }
    }
    if job.chime {
        if let Err(error) = sink.chime() {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
struct Worker {
    tx: SyncSender<Job>,
    rx: Receiver<Result<(), String>>,
}
impl Worker {
    fn start(waker: rune::Waker) -> Result<Self, String> {
        let (tx, requests) = mpsc::sync_channel(2);
        let (results, rx) = mpsc::sync_channel(4);
        std::thread::Builder::new()
            .name("aether-desktop-effects".into())
            .spawn(move || {
                let mut sink = NativeSink::default();
                while let Ok(job) = requests.recv() {
                    if results.send(perform(&mut sink, job)).is_err() {
                        break;
                    }
                    waker();
                }
            })
            .map_err(|error| format!("Start native completion worker: {error}"))?;
        Ok(Self { tx, rx })
    }
}
#[derive(Default)]
struct NativeSink {
    notification_identity: bool,
}
impl CompletionSink for NativeSink {
    fn notify(&mut self, event: &CompletedRun) -> Result<(), String> {
        if !self.notification_identity {
            rune_notify::register(&rune_notify::Identity::new(
                "dev.aether.agent.rune",
                "Aether",
            ))
            .map_err(|error| format!("Notification identity unavailable: {error}"))?;
            self.notification_identity = true;
        }
        if !rune_notify::available() {
            return Err("Native notifications unavailable on this platform or outside an application bundle".into());
        }
        let notification =
            rune_notify::Notification::new(&event.run_id, "Aether").body("Task complete");
        if rune_notify::post(&notification) {
            Ok(())
        } else {
            Err("Native completion notification refused by the OS; check notification permission and application identity".into())
        }
    }
    fn chime(&mut self) -> Result<(), String> {
        // Playback only: no microphone, network, subprocess, file or persistent audio thread.
        let mut stream = rodio::OutputStreamBuilder::open_default_stream()
            .map_err(|error| format!("Completion chime audio device unavailable: {error}"))?;
        stream.log_on_drop(false);
        let sink = rodio::Sink::connect_new(stream.mixer());
        sink.append(rodio::buffer::SamplesBuffer::new(
            1,
            44_100,
            chime_samples(),
        ));
        sink.sleep_until_end(); // bounded, off the UI thread; keep the output stream alive
        Ok(())
    }
}
fn chime_samples() -> Vec<f32> {
    let count = 44_100 / 4;
    (0..count)
        .map(|n| {
            let time = n as f32 / 44_100.0;
            let envelope = (n as f32 / 441.0).min(1.0) * (1.0 - n as f32 / count as f32).powi(2);
            let frequency = if n < count / 2 { 660.0 } else { 880.0 };
            0.10 * envelope * (std::f32::consts::TAU * frequency * time).sin()
        })
        .collect()
}
/// Called once per shell poll; consumes only acknowledged successful run events.
pub fn sync(s: &mut AppState) -> bool {
    let mut errors = Vec::new();
    if let Some(read) = &s.shell.desktop.shortcut_errors {
        errors.extend(read());
    }
    if let Some(worker) = &s.shell.desktop.worker {
        while let Ok(result) = worker.rx.try_recv() {
            if let Err(error) = result {
                errors.push(error);
            }
        }
    }
    let completed = std::mem::take(&mut s.controller.model_mut().completed_runs);
    let changed = !completed.is_empty() || !errors.is_empty();
    let settings = &s.controller.model().settings;
    let (notify, chime) = (settings.notifications_enabled, settings.sound_chime_enabled);
    for event in completed {
        if let Some(job) = s.shell.desktop.prepare(event, notify, chime) {
            if let Err(error) = s.shell.desktop.submit(job) {
                errors.push(error);
            }
        }
    }
    if !errors.is_empty() {
        s.set_error(errors.join("; "));
    }
    changed
}
/// The modal restores the exact previous native element, not a hard-coded settings button.
pub fn palette_focus_return(s: &AppState) -> &str {
    if s.ui.panel == Some(Panel::History) {
        s.shell
            .desktop
            .return_focus
            .as_deref()
            .unwrap_or("composer")
    } else {
        "settings"
    }
}
/// Registered with Rune's function-pointer API. Captured same-window handles live in state.
/// This is the native History palette, not a separate Spotlight window or provider request.
pub fn toggle_history(s: &mut AppState, tx: &mut rune::Tx) {
    crate::lifecycle::on_focus(s, true, tx);
    if s.shell.lifecycle != crate::lifecycle::Lifecycle::Open {
        return;
    }
    if let (Some(token), Some(activate)) = (tx.activation_token(), &s.shell.desktop.activation) {
        activate(token.to_owned()); // retain Wayland's compositor-authorized focus token
    } else if let Some(open) = &s.shell.window_open {
        open();
    }
    if s.ui.panel == Some(Panel::History) {
        s.ui.panel = None;
        if let Some(focus) = s.shell.desktop.return_composer_focus.take() {
            s.shell.composer.focused = focus;
        }
    } else {
        s.shell.desktop.return_composer_focus = Some(s.shell.composer.focused);
        s.shell.desktop.return_focus = Some(tx.focused().unwrap_or("composer").to_owned());
        s.ui.open(Panel::History, &mut s.controller);
    }
    tx.wake_after(Duration::ZERO);
}
#[cfg(test)]
mod tests {
    use super::*;
    fn event(n: usize) -> CompletedRun {
        CompletedRun {
            session_id: "offscreen".into(),
            run_id: format!("run-{n}"),
        }
    }
    #[derive(Default)]
    struct Fake {
        notifications: usize,
        sounds: usize,
        fail: bool,
    }
    impl CompletionSink for Fake {
        fn notify(&mut self, _: &CompletedRun) -> Result<(), String> {
            self.notifications += 1;
            if self.fail {
                Err("toast refused".into())
            } else {
                Ok(())
            }
        }
        fn chime(&mut self) -> Result<(), String> {
            self.sounds += 1;
            Ok(())
        }
    }
    #[test]
    fn primary_native_window_alone_owns_alt_space() {
        assert!(should_register_shortcut(true, false));
        assert!(!should_register_shortcut(false, false));
        assert!(!should_register_shortcut(true, true));
    }
    #[test]
    fn headless_and_default_never_dispatch_os_effects() {
        let mut desktop = DesktopState::default();
        assert!(desktop.prepare(event(0), true, true).is_none());
        desktop.enabled = true;
        assert!(
            desktop.prepare(event(0), true, true).is_none(),
            "disabled events are not replayed"
        );
        assert!(desktop.prepare(event(1), false, false).is_none());
    }
    #[test]
    fn configured_completion_dispatches_once_with_bounded_dedup() {
        let mut desktop = DesktopState::default();
        desktop.enabled = true;
        let job = desktop
            .prepare(event(0), true, false)
            .expect("enabled notification");
        let mut sink = Fake::default();
        perform(&mut sink, job).unwrap();
        assert_eq!((sink.notifications, sink.sounds), (1, 0));
        assert!(desktop.prepare(event(0), true, true).is_none());
        for n in 1..1000 {
            desktop.prepare(event(n), true, true);
        }
        assert_eq!(desktop.seen.len(), 128);
    }
    #[test]
    fn independent_settings_and_visible_failure() {
        let mut sink = Fake {
            fail: true,
            ..Default::default()
        };
        let error = perform(
            &mut sink,
            Job {
                event: event(0),
                notify: true,
                chime: true,
            },
        )
        .unwrap_err();
        assert!(error.contains("toast refused"));
        assert_eq!(
            (sink.notifications, sink.sounds),
            (1, 1),
            "toast failure must not suppress the opted-in chime"
        );
        perform(
            &mut sink,
            Job {
                event: event(1),
                notify: false,
                chime: true,
            },
        )
        .unwrap();
        assert_eq!((sink.notifications, sink.sounds), (1, 2));
    }
}
#[cfg(test)]
mod audio_tests {
    use super::*;
    #[test]
    fn generated_chime_is_short_finite_and_quiet_without_playing_it() {
        let samples = chime_samples();
        assert_eq!(samples.len(), 11025);
        assert_eq!(samples[0], 0.0);
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 0.11));
        assert!(!aether_core::agent::AgentSettings::default().sound_chime_enabled);
    }
    #[test]
    fn effects_queue_is_bounded_and_overflow_is_visible_without_native_worker() {
        let (tx, _jobs) = mpsc::sync_channel(2);
        let (_results, rx) = mpsc::channel();
        let mut desktop = DesktopState::default();
        desktop.worker = Some(Worker { tx, rx });
        let job = || Job {
            event: CompletedRun {
                session_id: "s".into(),
                run_id: "r".into(),
            },
            notify: true,
            chime: false,
        };
        desktop.submit(job()).unwrap();
        desktop.submit(job()).unwrap();
        assert!(desktop.submit(job()).unwrap_err().contains("capacity 2"));
    }
}
