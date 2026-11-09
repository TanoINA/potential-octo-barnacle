//! Native state lifetime and wake bridge. Profile routing is configured by main first.
use crate::{
    native,
    panels::{Panel, UiState},
    rich_content::RichContent,
    view,
};
use aether_core::agent::{AttachedContext, ProviderConfig};
use aether_state::Controller;
use rune::{AppSpec, Key, KeyEvent, TextEdit, Tx, Waker};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

pub struct AppState {
    pub controller: Controller,
    pub ui: UiState,
    pub rich: RefCell<RichContent>,
    pub shell: ShellState,
}
pub struct ShellState {
    pub startup_operation: Option<u64>,
    pub startup_error: Option<String>,
    pub desktop: crate::desktop::DesktopState,
    pub export: crate::export::ExportState,
    pub composer: TextEdit,
    pub completion: crate::completion::CompletionState,
    pub queue: crate::statequeue::QueueState,
    pub search: TextEdit,
    pub sidebar_open: bool,
    pub council: bool,
    pub candidate_b: Option<(String, String)>,
    pub reduced_motion: bool,
    /// Manual card expansion, scoped to the running/completed phase.
    pub trace_expansion: BTreeMap<String, (bool, bool)>,
    pub row_layouts: HashMap<String, [f32; 4]>,
    pub viewport: [f32; 4],
    pub selected_message: Option<String>,
    pub native: native::NativeState,
    pub error: Option<String>,
    pub smoke: bool,
    pub lifecycle: crate::lifecycle::Lifecycle,
    pub window_open: Option<Arc<dyn Fn() + Send + Sync>>,
    pub window_hide: Option<Arc<dyn Fn() + Send + Sync>>,
    pub close_prompt_shown: bool,
    pub window_finished: bool,
    pub close_group: Option<(usize, crate::lifecycle::CloseGroup)>,
}
impl Default for ShellState {
    fn default() -> Self {
        Self {
            startup_operation: None,
            startup_error: None,
            desktop: Default::default(),
            export: Default::default(),
            composer: TextEdit::default(),
            completion: Default::default(),
            queue: Default::default(),
            search: TextEdit::default(),
            sidebar_open: true,
            council: false,
            candidate_b: None,
            reduced_motion: true,
            trace_expansion: BTreeMap::new(),
            row_layouts: HashMap::new(),
            viewport: [0.0; 4],
            selected_message: None,
            native: Default::default(),
            error: None,
            smoke: false,
            lifecycle: Default::default(),
            window_open: None,
            window_hide: None,
            close_prompt_shown: false,
            window_finished: false,
            close_group: None,
        }
    }
}
impl AppState {
    pub fn new() -> Self {
        let mut controller =
            Controller::new().expect("Native controller runtime initialization failed");
        let startup_operation = controller.dispatch("get_settings", serde_json::json!({}));
        controller.dispatch("list_sessions", serde_json::json!({}));
        Self {
            controller,
            ui: UiState::default(),
            rich: RefCell::new(RichContent::new(Arc::new(|| {}))),
            shell: ShellState {startup_operation:Some(startup_operation),..Default::default()},
        }
    }
    /// The constructor's defaults must never authorize provider work or settings writes.
    /// Read the real result as well as the latch so Controller-only fixture polling works.
    pub fn configuration_ready(&self) -> bool {
        let acknowledged = self.shell.startup_operation.is_none() || self.shell.startup_operation
            .and_then(|id|self.controller.model().results.get(&id))
            .is_some_and(|r|r.command=="get_settings" && r.error.is_none() && r.value.as_ref()
                .is_some_and(|v|serde_json::from_value::<aether_core::agent::AgentSettings>(v.clone()).is_ok()));
        acknowledged && !self.controller.model().pending_commands.values().any(|c|c=="get_settings")
    }
    pub fn configuration_error(&self) -> Option<String> {
        self.shell.startup_error.clone().or_else(|| self.shell.startup_operation
            .and_then(|id|self.controller.model().results.get(&id))
            .filter(|r|r.command=="get_settings").and_then(|r|r.error.clone()))
    }
    pub fn retry_configuration(&mut self) {
        if self.configuration_ready() || self.controller.model().pending_commands.values().any(|c|c=="get_settings") {return;}
        self.shell.startup_error=None;
        self.shell.startup_operation=Some(self.controller.dispatch("get_settings",serde_json::json!({})));
    }
    fn sync_configuration(&mut self) -> bool {
        let Some(id)=self.shell.startup_operation else {return false;};
        let Some(result)=self.controller.model().results.get(&id).filter(|r|r.command=="get_settings") else {return false;};
        let error=result.error.clone().or_else(|| result.value.as_ref()
            .and_then(|v|serde_json::from_value::<aether_core::agent::AgentSettings>(v.clone()).err().map(|e|e.to_string())));
        if let Some(error)=error {
            if self.shell.startup_error.as_ref()==Some(&error) {return false;}
            self.shell.startup_error=Some(error); return true;
        }
        if result.value.is_none() {return false;}
        self.shell.startup_operation=None;self.shell.startup_error=None;
        true
    }
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.shell.error = Some(error.into());
    }
    pub fn new_session(&mut self, tx: &mut Tx) {
        if self.shell.lifecycle != crate::lifecycle::Lifecycle::Open {
            return;
        }
        native::cancel(self);
        self.controller.new_session();
        self.shell.composer.clear();
        self.shell.row_layouts.clear();
        self.shell.selected_message = None;
        if self.shell.reduced_motion {
            tx.instant();
        }
    }
    pub fn send(&mut self, tx: &mut Tx) {
        if self.shell.lifecycle != crate::lifecycle::Lifecycle::Open {
            return;
        }
        if self.shell.queue.edit.is_some() || self.shell.composer.preedit.is_some()
            || self.ui.panel.is_some()
            || self.shell.export.confirmation.is_some()
            || self.shell.native.preview.is_some()
        {
            return;
        }
        if self.shell.native.preparing() {
            self.set_error("Wait for attachment preparation, or cancel it before sending");
            return;
        }
        self.controller.model_mut().draft = self.shell.composer.text.clone();
        let prompt = self.shell.composer.text.clone();
        let command=prompt.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
        let local=crate::composer::is_local(&prompt) && !matches!(command.as_str(),"/retry"|"/review"|"/reflect"|"/refine");
        if !self.configuration_ready() && !local {
            self.set_error(if self.configuration_error().is_some() {"Configuration unavailable; Retry configuration before sending"} else {"Loading configuration; draft retained"});
            return;
        }
        if !crate::composer::is_local(&prompt) && (self.controller.selected_run_active() || self.shell.queue.sessions.get(&self.controller.model().current.as_ref().map(|d| d.session.id.clone()).unwrap_or_default()).is_some_and(|q| !q.items.is_empty())) {
            if let Err(error) = crate::statequeue::enqueue(self) { self.set_error(error); }
            tx.wake_after(std::time::Duration::ZERO);
            return;
        }
        // Local command routing is owned by Controller::send, not duplicated here.
        let result = if self.shell.council && !prompt.trim_start().starts_with('/') {
            match (self.candidate_a(), self.candidate_b()) {
                (Some(a), Some(b)) => self.controller.send_council(prompt, a, b),
                _ => Err("Configure candidates A and B in Models before starting Council".into()),
            }
        } else {
            self.controller.send(prompt)
        };
        if let Err(error) = result {
            self.set_error(error);
        }
        if self.controller.model().draft.is_empty() {
            self.shell.composer.clear();
        }
        if self.shell.reduced_motion {
            tx.instant();
        }
        tx.scroll_to_bottom("chat-viewport");
    }
    pub fn candidate_a(&self) -> Option<ProviderConfig> {
        let model = self.controller.model();
        let over = model
            .current
            .as_ref()
            .and_then(|d| d.model_override.as_ref());
        let id = over
            .and_then(|o| o.provider_id.as_deref())
            .unwrap_or(&model.settings.active_provider_id);
        let mut provider = model
            .settings
            .providers
            .iter()
            .find(|p| p.id == id)?
            .clone();
        if let Some(name) = over.and_then(|o| o.model.as_ref()) {
            provider.model = name.clone();
        }
        Some(provider)
    }
    pub fn council_choices(&self) -> Vec<ProviderConfig> {
        self.controller
            .model()
            .settings
            .providers
            .iter()
            .flat_map(|p| {
                let mut names = vec![p.model.clone()];
                for name in p.models.iter().chain(p.custom_models.iter()) {
                    if !names.contains(name) {
                        names.push(name.clone());
                    }
                }
                names
                    .into_iter()
                    .filter(|m| !m.trim().is_empty())
                    .map(move |model| {
                        let mut p = p.clone();
                        p.model = model;
                        p
                    })
            })
            .collect()
    }
    pub fn candidate_b(&self) -> Option<ProviderConfig> {
        let choices = self.council_choices();
        if let Some((id, model)) = &self.shell.candidate_b {
            return choices
                .into_iter()
                .find(|p| &p.id == id && &p.model == model);
        }
        let a = self.candidate_a();
        choices
            .iter()
            .find(|b| a.as_ref().is_none_or(|a| a.id != b.id))
            .or_else(|| {
                choices
                    .iter()
                    .find(|b| a.as_ref().is_none_or(|a| a.model != b.model))
            })
            .or(choices.first())
            .cloned()
    }
    pub fn cycle_candidate_b(&mut self) {
        let choices = self.council_choices();
        let current = self.candidate_b();
        if choices.is_empty() {
            self.ui
                .open(crate::panels::Panel::Models, &mut self.controller);
            return;
        }
        let next = current
            .and_then(|p| {
                choices
                    .iter()
                    .position(|c| c.id == p.id && c.model == p.model)
            })
            .map_or(0, |i| (i + 1) % choices.len());
        self.shell.candidate_b = Some((choices[next].id.clone(), choices[next].model.clone()));
    }
    /// Drain host-only commands even when no backend completion changed the model.
    fn consume_ui_actions(&mut self, tx: &mut Tx) -> bool {
        let actions = std::mem::take(&mut self.controller.model_mut().ui_actions);
        let changed = !actions.is_empty();
        for action in actions {
            let panel = match action.panel.as_str() {
                "export" => {
                    crate::export::handle(self, &action.args, tx);
                    continue;
                }
                "models" | "model" => Panel::Models,
                "settings" => Panel::Settings,
                "history" => Panel::History,
                "artifacts" => Panel::Artifacts,
                "tools" | "skills" => Panel::Tools,
                "memory" | "init" => Panel::Memory,
                "shell" | "terminal" => Panel::Shell,
                "git" => Panel::Git,
                "tasks" => Panel::Tasks,
                "review" => Panel::Review,
                "about" => Panel::About,
                other => {
                    self.set_error(format!("Unknown native UI intent: {other}"));
                    continue;
                }
            };
            self.ui.open(panel, &mut self.controller);
            if panel == Panel::Models {
                if let Some(provider) = self.candidate_a() {
                    for (key, value) in [
                        ("id", provider.id),
                        ("name", provider.name),
                        ("baseUrl", provider.base_url),
                        ("model", provider.model),
                    ] {
                        self.ui.set_form(&format!("provider.{key}"), value);
                    }
                    self.ui.set_form("provider.apiKey", "");
                }
                if !action.args.trim().is_empty() {
                    self.ui.set_form("provider.model", action.args.trim());
                }
            }
            if action.panel == "skills" || (panel == Panel::Tools && action.args == "skills") {
                self.ui.expanded.insert("skills".into());
            }
            if action.panel == "init" || (panel == Panel::Memory && action.args == "init") {
                self.ui.validation_error = Some("Review workspace instructions, then choose Initialize AGENTS.md to confirm creation.".into());
            }
        }
        changed
    }
    pub fn add_attachments(&mut self, attachments: Vec<AttachedContext>) {
        let model = self.controller.model_mut();
        for attachment in attachments {
            if !model
                .attachments
                .iter()
                .any(|item| item.path == attachment.path)
            {
                model.attachments.push(attachment);
            }
        }
    }
}
impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
fn setup(s: &mut AppState, waker: Waker) {
    s.shell.desktop.set_waker(waker.clone());
    s.controller.set_waker(waker.clone());
    s.shell.native = native::NativeState::new(waker.clone());
    *s.rich.borrow_mut() = RichContent::new(waker);
}
fn poll(s: &mut AppState, _: f64, tx: &mut Tx) -> bool {
    let changed = s.controller.poll();
    let configuration_changed=s.sync_configuration();
    s.ui.sync(&mut s.controller);
    let desktop_changed = crate::desktop::sync(s);
    let completion_changed = crate::completion::sync(s, tx);
    let native_changed = native::sync(s);
    let export_changed = crate::export::sync(s);
    let queue_changed = crate::statequeue::sync(s, tx);
    let intents_changed = s.consume_ui_actions(tx);
    if s.shell.reduced_motion {
        tx.instant();
    }
    let rich_changed = s.rich.borrow().take_changed();
    let lifecycle_changed = crate::lifecycle::poll(s, tx);
    changed
        || configuration_changed
        || desktop_changed
        || completion_changed
        || native_changed
        || rich_changed
        || intents_changed
        || lifecycle_changed
        || export_changed
        || queue_changed
}
pub fn on_key(s: &mut AppState, event: &KeyEvent, tx: &mut Tx) -> bool {
    if s.shell.lifecycle != crate::lifecycle::Lifecycle::Open {
        // Do not swallow Tab/Enter: the unsaved modal remains keyboard usable.
        return event.key == Key::Escape || event.mods.cmd || event.mods.ctrl;
    }
    if event.key == Key::Escape {
        if s.shell.native.preview.take().is_some() {
            return true;
        }
        if s.ui.panel.is_some() {
            s.ui.panel = None;
            return true;
        }
        return false;
    }
    if s.shell.queue.edit.is_some() || s.ui.panel.is_some() || s.shell.export.confirmation.is_some() || s.shell.native.preview.is_some() {
        return false;
    }
    if !event.mods.cmd && !event.mods.ctrl {
        return false;
    }
    if event.mods.alt {
        return false;
    }
    match event.key {
        Key::Command('n') => {
            if !tx.key_repeat() {
                s.new_session(tx);
            }
        }
        Key::Command('b') => {
            if !tx.key_repeat() {
                s.shell.sidebar_open = !s.shell.sidebar_open;
            }
        }
        Key::Command(',') => s.ui.open(crate::panels::Panel::Settings, &mut s.controller),
        Key::Command('p') => s.ui.open(crate::panels::Panel::History, &mut s.controller),
        Key::Command('l') => {
            s.shell.composer.focused = true;
        }
        Key::Enter => {
            s.send(tx);
        }
        _ => return false,
    }
    true
}
pub fn spec() -> AppSpec<AppState> {
    AppSpec {
        min_size: Some((760.0, 540.0)),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: &[("chat", scene_chat)],
        ..AppSpec::new(
            "Aether  -  Native",
            (1280.0, 840.0),
            AppState::new,
            view::view,
        )
    }
}
/// Only Rune's explicit --scene chat selects this fixture; ordinary launch loads real data.
fn scene_chat(s: &mut AppState) {
    s.controller.new_session();
    if let Some(current) = s.controller.model_mut().current.as_mut() {
        current.messages=serde_json::from_value(serde_json::json!([
            {"id":"fixture-user","role":"user","content":"Show a native rendering example.","timestamp":1},
            {"id":"fixture-assistant","role":"assistant","content":"## Native rich content\n\nA selectable response with **Markdown**, a table and mathematics: $E=mc^2$.\n\n| Renderer | Host |\n|---|---|\n| Rune | Native GPU |\n\n```rust\nfn main() { println!(\"Hello, Rune\"); }\n```","timestamp":2}
        ])).expect("fixture schema");
    }
}

/// Shared startup/retry status, visible even when Settings is opened before its read.
pub fn configuration_status(s: &AppState, kit: &rune_kit::Kit) -> rune::El {
    if let Some(error)=s.configuration_error() {
        rune::col().gap(6.0).child(crate::widgets::caption(format!("Configuration unavailable: {error}"),kit).role("status"))
            .child(crate::widgets::button("configuration-retry","Retry configuration",kit,|s,_|s.retry_configuration()))
    } else {
        crate::widgets::caption("Loading configuration - provider sends and settings edits are paused",kit).key("configuration-loading").role("status")
    }
}