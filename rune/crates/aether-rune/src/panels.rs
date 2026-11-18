//! Native service panels. Shell integration: see panels/PANEL_INTEGRATION.md.
//! This module owns UiState: the shell must import it, not define a second copy.
mod domain;
#[cfg(test)]
mod tests;

use crate::app::AppState;
use aether_core::agent::{AgentSettings, ProviderConfig};
use aether_state::Controller;
use rune::{Cx, El, PointerPress, TextEdit, col, row, text};
use rune_kit::{Button, Kit, MenuItem, TextArea, TextInput, context_menu};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Settings,
    Models,
    History,
    Artifacts,
    Tools,
    Memory,
    Shell,
    Tasks,
    Review,
    Git,
    About,
}
impl Panel {
    pub const ALL: [Self; 11] = [
        Self::Settings,
        Self::Models,
        Self::History,
        Self::Artifacts,
        Self::Tools,
        Self::Memory,
        Self::Shell,
        Self::Tasks,
        Self::Review,
        Self::Git,
        Self::About,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Models => "Models",
            Self::History => "History",
            Self::Artifacts => "Artifacts",
            Self::Tools => "Tools & skills",
            Self::Memory => "Workspace memory",
            Self::Shell => "Live Shell",
            Self::Tasks => "Background tasks",
            Self::Review => "Review",
            Self::Git => "Git",
            Self::About => "About",
        }
    }
}

#[derive(Clone)]
struct Request {
    command: String,
    args: Value,
    slot: String,
}
impl Request {
    fn new(command: &str, args: Value, slot: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args,
            slot: slot.into(),
        }
    }
}
struct Ticket {
    request: Request,
    follow: Option<Request>,
    workspace: String,
    session: Option<String>,
    settings_forms: BTreeMap<String, String>,
}
#[derive(Clone)]
struct Reply {
    id: u64,
    value: Option<Value>,
    error: Option<String>,
}
struct Confirmation {
    request: Request,
    follow: Option<Request>,
    workspace: String,
    session: Option<String>,
    label: String,
}

struct ConversationEdit {
    session: String,
    workspace: String,
    message: String,
}

#[derive(Default)]
pub struct UiState {
    pub panel: Option<Panel>,
    pub forms: BTreeMap<String, String>,
    pub search: String,
    pub selected: String,
    pub result_cursor: u64,
    pub expanded: BTreeSet<String>,
    pub image_preview: Option<String>,
    pub minimized: bool,
    editors: BTreeMap<String, TextEdit>,
    tickets: BTreeMap<u64, Ticket>,
    replies: BTreeMap<String, Reply>,
    settings_original: Value,
    cached_workspace: String,
    proposal_history: BTreeMap<String, Vec<Value>>,
    confirmation: Option<Confirmation>,
    conversation_edit: Option<ConversationEdit>,
    message_context: Option<(String, String, PointerPress)>,
    pub validation_error: Option<String>,
}

impl UiState {
    pub fn set_form(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        self.forms.insert(key.into(), value.clone());
        self.editors.insert(key.into(), TextEdit::new(value));
    }
    pub fn form(&self, key: &str) -> &str {
        self.forms.get(key).map(String::as_str).unwrap_or("")
    }
    fn seed(&mut self, key: &str, value: impl Into<String>) {
        if !self.forms.contains_key(key) {
            self.set_form(key, value);
        }
    }
    fn session(controller: &Controller) -> Option<String> {
        controller
            .model()
            .current
            .as_ref()
            .map(|d| d.session.id.clone())
    }
    fn issue(&mut self, controller: &mut Controller, request: Request, follow: Option<Request>) {
        if self
            .tickets
            .values()
            .any(|t| t.request.slot == request.slot)
        {
            return;
        }
        self.validation_error = None;
        let id = controller.dispatch(&request.command, request.args.clone());
        self.tickets.insert(
            id,
            Ticket {
                request,
                follow,
                workspace: controller.model().workspace.clone(),
                session: Self::session(controller),
                settings_forms: self.forms.clone(),
            },
        );
    }
    fn request(
        &mut self,
        controller: &mut Controller,
        command: &str,
        args: Value,
        slot: impl Into<String>,
    ) {
        self.issue(controller, Request::new(command, args, slot), None);
    }
    fn mutate(
        &mut self,
        controller: &mut Controller,
        request: Request,
        follow: Option<Request>,
        label: &str,
    ) {
        if domain::needs_confirmation(&request.command) {
            self.confirmation = Some(Confirmation {
                request,
                follow,
                workspace: controller.model().workspace.clone(),
                session: Self::session(controller),
                label: label.into(),
            });
        } else {
            self.issue(controller, request, follow);
        }
    }
    pub fn pending(&self, slot: &str) -> bool {
        self.tickets.values().any(|t| t.request.slot == slot)
    }
    fn value(&self, slot: &str) -> Option<&Value> {
        self.replies.get(slot).and_then(|r| r.value.as_ref())
    }
    fn rows(&self, slot: &str) -> Vec<Value> {
        self.value(slot)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }
    fn load_settings(&mut self, settings: Value) {
        self.settings_original = settings.clone();
        self.set_form("settings.json", pretty(&domain::redact(&settings)));
        for key in [
            "systemPrompt",
            "temperature",
            "maxTokens",
            "maxTurns",
            "autoCompactionThresholdTokens",
        ] {
            self.set_form(
                &format!("settings.{key}"),
                settings
                    .get(key)
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default(),
            );
        }
    }
    pub fn open(&mut self, panel: Panel, controller: &mut Controller) {
        if self.panel != Some(panel) {
            self.selected.clear();
        }
        self.panel = Some(panel);
        self.minimized = false;
        self.validation_error = None;
        self.seed("shell.command", "");
        self.seed("mcp.arguments", "{}");
        self.seed("export.format", "markdown");
        self.seed("task.tail", "100");
        self.seed("advanced.arguments", "{}");
        match panel {
            Panel::Settings | Panel::Models => {
                if self.settings_original.is_null() {
                    self.load_settings(
                        serde_json::to_value(&controller.model().settings).unwrap_or_default(),
                    );
                }
                // Reuse the startup read rather than racing a second read against saves.
                if let Some((&id, _)) = controller.model().pending_commands.iter().find(|(_, c)| c.as_str()=="get_settings") {
                    if !self.pending("settings") {
                        self.tickets.insert(id, Ticket { request:Request::new("get_settings",json!({}),"settings"),follow:None,
                            workspace:controller.model().workspace.clone(),session:Self::session(controller),settings_forms:self.forms.clone() });
                    }
                } else {
                    self.request(controller, "get_settings", json!({}), "settings");
                }
            }
            Panel::History => self.request(controller, "list_sessions", json!({}), "sessions"),
            Panel::Artifacts => {
                self.request(controller, "list_all_artifacts", json!({}), "artifacts")
            }
            Panel::Tools => {
                self.request(controller, "list_native_tools", json!({}), "native-tools");
                self.request(controller, "list_skills", json!({}), "skills");
            }
            Panel::Memory => {
                self.set_form("memory.workspace", controller.model().workspace.clone());
                let args = json!({"expectedWorkspace": self.form("memory.workspace")});
                self.request(controller, "read_workspace_agents_md", args, "memory");
            }
            Panel::Shell => self.request(controller, "get_workspace_cwd", json!({}), "workspace"),
            Panel::Tasks | Panel::Review => {
                if let Some(session) = Self::session(controller) {
                    let (command, kind) = if panel == Panel::Tasks {
                        ("list_background_tasks", "tasks")
                    } else {
                        ("list_review_proposals", "review")
                    };
                    self.request(
                        controller,
                        command,
                        json!({"sessionId":session}),
                        domain::scope(kind, &session, ""),
                    );
                }
            }
            Panel::Git => self.request(controller, "git_status", json!({}), "git"),
            Panel::About => self.request(controller, "get_release_info", json!({}), "release"),
        }
    }
    /// Call after Controller::poll/drain. Completion, not click, triggers refresh.
    pub fn sync(&mut self, controller: &mut Controller) {
        if self.cached_workspace != controller.model().workspace {
            self.cached_workspace = controller.model().workspace.clone();
            self.replies.remove("git");
        }
        let completed: Vec<_> = self
            .tickets
            .keys()
            .filter_map(|id| controller.model().results.get(id).cloned())
            .collect();
        for result in completed {
            let Some(ticket) = self.tickets.remove(&result.id) else {
                continue;
            };
            self.result_cursor = self.result_cursor.max(result.id);
            let same_scope = ticket.workspace == controller.model().workspace
                && ticket.session == Self::session(controller);
            if same_scope && let Some(error) = &result.error {
                self.validation_error = Some(format!("{}: {error}", ticket.request.command));
            }
            if result.error.is_none() {
                // Retain the editor through confirmation and failure. A completion
                // from another conversation must not dismiss or overwrite a draft.
                if same_scope
                    && ticket.request.command == "edit_and_resend"
                    && self.conversation_edit.as_ref().is_some_and(|edit| {
                        ticket.request.args.get("sessionId").and_then(Value::as_str)
                            == Some(edit.session.as_str())
                            && ticket.request.args.get("messageId").and_then(Value::as_str)
                                == Some(edit.message.as_str())
                            && ticket.request.args.get("text").and_then(Value::as_str)
                                == Some(self.form("conversation.text"))
                    })
                {
                    self.conversation_edit = None;
                    self.forms.remove("conversation.text");
                    self.editors.remove("conversation.text");
                }
                if let Some(value) = result.value.as_ref() {
                    match ticket.request.slot.as_str() {
                        "settings" => {
                            // A late read refreshes the baseline and untouched controls only.
                            let edits: Vec<_> = self.forms.iter().filter(|(key,value)| key.starts_with("settings.") && ticket.settings_forms.get(*key)!=Some(*value))
                                .map(|(key,value)|(key.clone(),value.clone(),self.editors.get(key).cloned())).collect();
                            self.load_settings(value.clone());
                            for (key,value,editor) in edits { self.forms.insert(key.clone(),value); if let Some(editor)=editor {self.editors.insert(key,editor);} }
                        },
                        "memory"
                            if ticket
                                .request
                                .args
                                .get("expectedWorkspace")
                                .and_then(Value::as_str)
                                == Some(self.form("memory.workspace")) =>
                        {
                            self.set_form("memory.content", value.as_str().unwrap_or(""))
                        }
                        "workspace" => {
                            if let Some(workspace) = value.as_str() {
                                self.set_form("workspace.path", workspace);
                            }
                        }
                        _ => {}
                    }
                }
                if ticket.request.command == "list_review_proposals" {
                    let session = string(&ticket.request.args, "sessionId");
                    let history = self.proposal_history.entry(session).or_default();
                    for proposal in result
                        .value
                        .as_ref()
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        let id = string(proposal, "id");
                        if let Some(old) = history.iter_mut().find(|p| string(p, "id") == id) {
                            *old = proposal.clone();
                        } else {
                            history.push(proposal.clone());
                        }
                    }
                    if history.len() > 100 {
                        history.drain(..history.len() - 100);
                    }
                }
                if ["approve_review_proposal", "reject_review_proposal"]
                    .contains(&ticket.request.command.as_str())
                {
                    let session = string(&ticket.request.args, "sessionId");
                    let id = string(&ticket.request.args, "id");
                    if let Some(proposal) = self
                        .proposal_history
                        .get_mut(&session)
                        .and_then(|rows| rows.iter_mut().find(|p| string(p, "id") == id))
                    {
                        proposal["status"] =
                            json!(if ticket.request.command == "approve_review_proposal" {
                                "approved"
                            } else {
                                "rejected"
                            });
                    }
                }
                if ticket.request.command == "init_workspace_agents_md" {
                    self.request(
                        controller,
                        "read_workspace_agents_md",
                        ticket.request.args.clone(),
                        "memory",
                    );
                }
                if let Some(follow) = ticket.follow {
                    self.issue(controller, follow, None);
                }
            }
            self.replies.insert(
                ticket.request.slot,
                Reply {
                    id: result.id,
                    value: result.value,
                    error: result.error,
                },
            );
        }
        while self.replies.len() > 96 {
            let Some(key) = self
                .replies
                .iter()
                .min_by_key(|(_, r)| r.id)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.replies.remove(&key);
        }
    }
    fn settings(&self) -> Result<Value, String> {
        let value = domain::settings_document(self.form("settings.json"), &self.settings_original)?;
        serde_json::from_value::<AgentSettings>(value.clone())
            .map_err(|e| format!("Invalid settings: {e}"))?;
        Ok(value)
    }
    fn settings_controls(&self) -> Result<Value, String> {
        let mut value = self.settings()?;
        value["systemPrompt"] = json!(self.form("settings.systemPrompt"));
        for key in [
            "temperature",
            "maxTokens",
            "maxTurns",
            "autoCompactionThresholdTokens",
        ] {
            let parsed: Value = serde_json::from_str(self.form(&format!("settings.{key}")))
                .map_err(|_| {
                    format!("{key} must be a valid number (maxTokens also accepts null)")
                })?;
            if key == "maxTurns" && !parsed.as_u64().is_some_and(|turns| turns <= u32::MAX as u64) {
                return Err("Max turns must be a whole number from 0 to 4,294,967,295 (0 = unlimited)".into());
            }
            value[key] = parsed;
        }
        let typed: AgentSettings =
            serde_json::from_value(value.clone()).map_err(|e| format!("Invalid settings: {e}"))?;
        if !typed.temperature.is_finite() || !(0.0..=2.0).contains(&typed.temperature) {
            return Err("Temperature must be between 0 and 2".into());
        }
        if typed.max_tokens == Some(0)
            || typed.auto_compaction_threshold_tokens == 0
        {
            return Err(
                "Token limits and compaction thresholds must be positive".into(),
            );
        }
        Ok(value)
    }
    fn save_settings(&mut self, controller: &mut Controller, value: Value) {
        if let Err(e) = serde_json::from_value::<AgentSettings>(value.clone()) {
            self.validation_error = Some(format!("Invalid settings: {e}"));
            return;
        }
        self.issue(
            controller,
            Request::new("save_settings", json!({"settings": value}), "save-settings"),
            Some(Request::new("get_settings", json!({}), "settings")),
        );
    }
    /// Controller-owned history actions. These dispatch names are intentionally
    /// not service commands: the controller must serialize canonical replay/save.
    /// They must never be implemented by editing Model.current in this module.
    pub fn conversation_action(
        &mut self,
        controller: &mut Controller,
        command: &str,
        message: &str,
    ) {
        let Some(session) = Self::session(controller) else {
            return;
        };
        let model = controller.model();
        if model.busy || model.loading || model.loading_older || model.preview.is_some() {
            self.validation_error = Some(
                "Return to latest and wait for the active operation before changing history".into(),
            );
            return;
        }
        if !message.is_empty()
            && !model
                .current
                .as_ref()
                .is_some_and(|d| d.messages.iter().any(|m| m.id == message))
        {
            self.validation_error = Some("Message is no longer in the current conversation".into());
            return;
        }
        if command == "edit_and_resend" {
            let Some(message) = model.current.as_ref().and_then(|d| {
                d.messages
                    .iter()
                    .find(|m| m.id == message && m.role == "user")
            }) else {
                self.validation_error = Some("Only user prompts can be edited and resent".into());
                return;
            };
            self.conversation_edit = Some(ConversationEdit {
                session,
                workspace: model.workspace.clone(),
                message: message.id.clone(),
            });
            self.set_form("conversation.text", message.content.clone());
            self.panel = Some(Panel::History);
            self.minimized = false;
        } else if ["undo_turn", "rollback_turn", "retry_turn"].contains(&command) {
            self.panel = Some(Panel::History);
            self.minimized = false;
            self.mutate(
                controller,
                Request::new(
                    command,
                    json!({"sessionId":session,"messageId":message}),
                    "conversation-action",
                ),
                None,
                match command {
                    "undo_turn" => "undo the latest turn",
                    "rollback_turn" => "roll back to this turn",
                    _ => "retry this turn",
                },
            );
        }
    }
    fn select_provider(&mut self, provider: &Value) {
        self.set_form("provider.id", string(provider, "id"));
        self.set_form("provider.name", string(provider, "name"));
        self.set_form("provider.baseUrl", string(provider, "baseUrl"));
        self.set_form("provider.model", string(provider, "model"));
        self.set_form("provider.apiKey", "");
    }
}

fn string(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").into()
}
fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "Could not format result".into())
}
fn label(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|key| string(value, key))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}
fn note(message: impl Into<String>) -> El {
    text(message).selectable(true)
}
fn action(
    kit: &Kit,
    key: impl Into<String>,
    title: impl Into<String>,
    disabled: bool,
    run: impl Fn(&mut AppState) + 'static,
) -> El {
    Button::new(title)
        .disabled(disabled)
        .on_press(run)
        .build(kit)
        .key(key)
}
fn field(state: &AppState, kit: &Kit, key: &str, title: &str, multiline: bool, secret: bool) -> El {
    let edit = state
        .ui
        .editors
        .get(key)
        .cloned()
        .unwrap_or_else(|| TextEdit::new(state.ui.form(key)));
    let editor_key = key.to_owned();
    let change_key = key.to_owned();
    let initial = state.ui.form(key).to_owned();
    let initial2 = initial.clone();
    let input = if multiline {
        let mut area = TextArea::new(&edit)
            .placeholder(title)
            .width(None)
            .rows(8)
            .on_edit(move |s: &mut AppState| {
                s.ui.editors
                    .entry(editor_key.clone())
                    .or_insert_with(|| TextEdit::new(initial.clone()))
            })
            .on_change(move |s: &mut AppState, _| {
                if let Some(e) = s.ui.editors.get(&change_key) {
                    s.ui.forms.insert(change_key.clone(), e.text.to_string());
                }
            });
        if secret {
            area = area.secure();
        }
        area.build(kit)
    } else {
        let mut input = TextInput::new(&edit)
            .label(title)
            .placeholder(title)
            .width(None)
            .on_edit(move |s: &mut AppState| {
                s.ui.editors
                    .entry(editor_key.clone())
                    .or_insert_with(|| TextEdit::new(initial2.clone()))
            })
            .on_change(move |s: &mut AppState, _| {
                if let Some(e) = s.ui.editors.get(&change_key) {
                    s.ui.forms.insert(change_key.clone(), e.text.to_string());
                }
            });
        if secret {
            input = input.secure();
        }
        input.build(kit)
    };
    col()
        .w_full()
        .gap(4.0)
        .child(note(title))
        .child(input.key(format!("field-{key}")))
}
fn command(
    state: &AppState,
    kit: &Kit,
    title: &str,
    command: &str,
    args: Value,
    slot: impl Into<String>,
) -> El {
    let slot = slot.into();
    let request = Request::new(command, args, &slot);
    action(
        kit,
        format!("command-{slot}"),
        if state.ui.pending(&slot) {
            format!("{title} — pending…")
        } else {
            title.into()
        },
        state.ui.pending(&slot) || (!state.configuration_ready() && matches!(command,"review_session"|"fetch_provider_models"|"save_settings"|"send_message"|"send_council_message")),
        move |s| {
            if !s.configuration_ready() && matches!(request.command.as_str(),"review_session"|"fetch_provider_models"|"save_settings"|"send_message"|"send_council_message") {return;}
            s.ui.mutate(&mut s.controller, request.clone(), None, &request.command);
        },
    )
}
fn toggle(
    kit: &Kit,
    key: &str,
    title: &str,
    on: bool,
    run: impl Fn(&mut AppState) + 'static,
) -> El {
    action(
        kit,
        key,
        format!("{} {title}", if on { "[✓]" } else { "[ ]" }),
        false,
        run,
    )
}
fn rich(state: &AppState, kit: &Kit, source: &str, key: &str) -> El {
    state.rich.borrow_mut().render(source, key, kit)
}
fn result(state: &AppState, kit: &Kit, slot: &str) -> El {
    if state.ui.pending(slot) {
        return note("Operation pending — waiting for the service.");
    }
    match state.ui.replies.get(slot) {
        Some(reply) if reply.error.is_some() => note(format!(
            "Error: {}",
            reply.error.as_deref().unwrap_or("Unknown error")
        )),
        Some(reply) => match reply.value.as_ref() {
            Some(Value::Null) | None => note("Service completed successfully."),
            Some(Value::String(source)) => rich(
                state,
                kit,
                &domain::code(source, "text"),
                &format!("result-{slot}-{}", reply.id),
            ),
            Some(value) if slot == "shell" => col()
                .gap(8.0)
                .child(note(format!(
                    "Exit code: {} · Duration: {} ms",
                    value.get("exitCode").unwrap_or(&Value::Null),
                    value.get("durationMs").unwrap_or(&Value::Null)
                )))
                .child(rich(
                    state,
                    kit,
                    &domain::code(&string(value, "stdout"), "shell"),
                    &format!("shell-stdout-{}", reply.id),
                ))
                .child(rich(
                    state,
                    kit,
                    &domain::code(&string(value, "stderr"), "shell"),
                    &format!("shell-stderr-{}", reply.id),
                )),
            Some(value) => rich(
                state,
                kit,
                &domain::code(&pretty(&domain::redact(value)), "json"),
                &format!("result-{slot}-{}", reply.id),
            ),
        },
        None => note("No result yet."),
    }
}

pub fn render(state: &AppState, _cx: &mut Cx<AppState>, kit: &Kit) -> Option<El> {
    let panel = state.ui.panel?;
    let mut root = col()
        .w_full()
        .h_full()
        .gap(12.0)
        .padding(16.0)
        .key("native-feature-panel");
    root = root.child(
        row()
            .gap(8.0)
            .child(note(panel.title()))
            .child(action(
                kit,
                "panel-minimize",
                if state.ui.minimized {
                    "Restore"
                } else {
                    "Minimize"
                },
                false,
                |s| s.ui.minimized = !s.ui.minimized,
            ))
            .child(action(kit, "panel-close", "Close", false, |s| {
                s.ui.panel = None;
                s.ui.confirmation = None;
            })),
    );
    if state.ui.minimized {
        return Some(root);
    }
    let mut nav = row().gap(4.0).scroll_x();
    for target in Panel::ALL {
        nav = nav.child(action(
            kit,
            format!("panel-open-{target:?}"),
            target.title(),
            false,
            move |s| s.ui.open(target, &mut s.controller),
        ));
    }
    root = root.child(nav);
    if let Some(error) = &state.ui.validation_error {
        root = root.child(note(format!("Validation: {error}")));
    }
    if let Some(edit) = state
        .ui
        .conversation_edit
        .as_ref()
        .filter(|_| state.ui.confirmation.is_none())
    {
        let changed = UiState::session(&state.controller).as_deref() != Some(&edit.session)
            || state.controller.model().workspace != edit.workspace;
        return Some(root.child(col().key("conversation-edit-modal").role("dialog").label("Edit prompt and resend")
            .modal(true, |s: &mut AppState| s.ui.conversation_edit = None)
            .gap(10.0).padding(16.0)
            .child(note(format!("Edit prompt · {}\nResending replaces this turn and its later history. Attachments are replayed by the controller.", edit.message)))
            .child(if changed { note("Conversation scope changed. Draft is preserved; cancel and reopen the intended message.") } else { note("Review your changes before resending.") })
            .child(field(state, kit, "conversation.text", "Prompt text", true, false))
            .child(action(kit, "conversation-edit-cancel", "Cancel", false, |s| s.ui.conversation_edit = None))
            .child(action(kit, "conversation-edit-submit", "Review edit & resend…", state.ui.pending("conversation-action"), |s| {
                let Some(edit) = &s.ui.conversation_edit else { return; };
                if UiState::session(&s.controller).as_deref() != Some(&edit.session) || s.controller.model().workspace != edit.workspace {
                    s.ui.validation_error = Some("Conversation scope changed; edited draft preserved".into()); return;
                }
                if s.controller.model().busy || s.controller.model().preview.is_some() || s.controller.model().loading || s.controller.model().loading_older {
                    s.ui.validation_error = Some("Return to latest and wait for the active operation".into()); return;
                }
                if s.ui.form("conversation.text").trim().is_empty() {
                    s.ui.validation_error = Some("Prompt text cannot be empty".into()); return;
                }
                let request = Request::new("edit_and_resend", json!({"sessionId":edit.session,"messageId":edit.message,"text":s.ui.form("conversation.text")}), "conversation-action");
                s.ui.mutate(&mut s.controller, request, None, "edit this prompt and replace its later conversation");
            }))));
    }
    if let Some(confirm) = &state.ui.confirmation {
        let scope_changed = confirm.workspace != state.controller.model().workspace
            || confirm.session != UiState::session(&state.controller);
        root = root.child(col().padding(12.0).gap(8.0).role("alertdialog").key("panel-confirmation")
            .child(note(format!("Confirm {}\nWorkspace: {}\nArguments: {}\nThis action may change or delete persisted data. It cannot be silently undone.", confirm.label, confirm.workspace, pretty(&domain::redact(&confirm.request.args)))))
            .child(if scope_changed { note("Scope changed. Cancel and select the action again.") } else { note("Only the exact session/item shown above will be affected.") })
            .child(row().gap(8.0)
                .child(action(kit, "confirm-cancel", "Cancel", false, |s| s.ui.confirmation = None))
                .child(action(kit, "confirm-accept", "Confirm action", scope_changed, |s| {
                    if let Some(confirm) = s.ui.confirmation.take() {
                        if confirm.workspace != s.controller.model().workspace || confirm.session != UiState::session(&s.controller) { s.ui.validation_error = Some("Action cancelled because scope changed".into()); return; }
                        s.ui.issue(&mut s.controller, confirm.request, confirm.follow);
                    }
                }))));
        // Modal semantics: underlying panel actions are not in the element tree.
        return Some(root);
    }
    let mut body = match panel {
        Panel::Settings => settings_panel(state, kit),
        Panel::Models => models_panel(state, kit),
        Panel::History => history_panel(state, kit),
        Panel::Artifacts => artifacts_panel(state, kit),
        Panel::Tools => tools_panel(state, kit),
        Panel::Memory => memory_panel(state, kit),
        Panel::Shell => shell_panel(state, kit),
        Panel::Tasks => tasks_panel(state, kit),
        Panel::Review => review_panel(state, kit),
        Panel::Git => git_panel(state, kit),
        Panel::About => about_panel(state, kit),
    };
    if !state.configuration_ready() && matches!(panel, Panel::Settings | Panel::Models) {
        body = col().child(crate::app::configuration_status(state,kit)).child(body.inert());
    }
    Some(
        root.child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .gap(12.0)
                .child(body)
                .child(advanced_panel(state, kit)),
        ),
    )
}

fn settings_panel(state: &AppState, kit: &Kit) -> El {
    let value = state
        .ui
        .settings()
        .unwrap_or_else(|_| state.ui.settings_original.clone());
    let mut body = col().gap(10.0).w_full().child(note("All configuration fields are preserved. Save only reports completion after the backend acknowledges it."))
        .child(command(state, kit, "Reload saved settings (replaces editor)", "get_settings", json!({}), "settings"));
    for (key, title, multi) in [
        ("systemPrompt", "System prompt", true),
        ("temperature", "Temperature", false),
        ("maxTokens", "Max tokens (null = provider default)", false),
        ("maxTurns", "Max turns (0 = unlimited)", false),
        (
            "autoCompactionThresholdTokens",
            "Auto-compaction token threshold",
            false,
        ),
    ] {
        body = body.child(field(
            state,
            kit,
            &format!("settings.{key}"),
            title,
            multi,
            false,
        ));
    }
    for (key, title) in [
        ("streamResponse", "Stream responses"),
        ("thinking", "Thinking"),
        ("notificationsEnabled", "Notifications"),
        ("soundChimeEnabled", "Sound chime"),
        ("self_improvement_enabled", "Self improvement"),
        ("autoCompactionEnabled", "Automatic compaction"),
    ] {
        let on = value.get(key).and_then(Value::as_bool).unwrap_or(false);
        body = body.child(toggle(
            kit,
            &format!("setting-{key}"),
            title,
            on,
            move |s| match s.ui.settings() {
                Ok(mut value) => {
                    value[key] = json!(!on);
                    s.ui.set_form("settings.json", pretty(&domain::redact(&value)));
                }
                Err(e) => s.ui.validation_error = Some(e),
            },
        ));
    }
    body = body.child(action(
        kit,
        "settings-apply-basic",
        "Apply basic controls to complete JSON",
        false,
        |s| {
            let outcome = (|| -> Result<Value, String> {
                let mut value = s.ui.settings()?;
                value["systemPrompt"] = json!(s.ui.form("settings.systemPrompt"));
                for key in [
                    "temperature",
                    "maxTokens",
                    "maxTurns",
                    "autoCompactionThresholdTokens",
                ] {
                    let parsed: Value = serde_json::from_str(s.ui.form(&format!("settings.{key}")))
                        .map_err(|_| {
                            format!("{key} must be a valid number (maxTokens also accepts null)")
                        })?;
                    if key == "maxTurns" && !parsed.as_u64().is_some_and(|turns| turns <= u32::MAX as u64) {
                        return Err("Max turns must be a whole number from 0 to 4,294,967,295 (0 = unlimited)".into());
                    }
                    value[key] = parsed;
                }
                serde_json::from_value::<AgentSettings>(value.clone())
                    .map_err(|e| e.to_string())?;
                Ok(value)
            })();
            match outcome {
                Ok(value) => {
                    s.ui.set_form("settings.json", pretty(&domain::redact(&value)))
                }
                Err(e) => s.ui.validation_error = Some(e),
            }
        },
    ));
    body.child(field(state, kit, "settings.json", "Complete settings JSON — keys are redacted; unchanged-secret markers are restored securely", true, false))
        .child(action(kit, "settings-save", "Validate & save complete settings", state.ui.pending("save-settings"), |s| {
            if !s.configuration_ready() {return;}
            match s.ui.settings_controls() { Ok(value) => s.ui.save_settings(&mut s.controller, value), Err(e) => s.ui.validation_error = Some(e) }
        })).child(result(state, kit, "save-settings"))
}

fn models_panel(state: &AppState, kit: &Kit) -> El {
    let settings = state.ui.settings().unwrap_or_else(|_| {
        serde_json::to_value(&state.controller.model().settings).unwrap_or_default()
    });
    let mut body = col().gap(10.0).child(note(
        "Choose an actual configured provider. Saving changes preserves every other setting.",
    ));
    let providers = settings
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for provider in &providers {
        let selected = state.ui.form("provider.id") == string(provider, "id");
        let p = provider.clone();
        body = body.child(action(
            kit,
            format!("provider-{}", string(provider, "id")),
            format!(
                "{}{}",
                if selected { "✓ " } else { "" },
                label(provider, &["name", "model"])
            ),
            false,
            move |s| s.ui.select_provider(&p),
        ));
    }
    body = body.child(action(kit, "provider-add", "Add provider", false, |s| {
        for (key, value) in [
            ("provider.id", ""),
            ("provider.name", ""),
            ("provider.baseUrl", ""),
            ("provider.model", ""),
            ("provider.apiKey", ""),
        ] {
            s.ui.set_form(key, value);
        }
        s.ui.expanded.insert("new-provider".into());
    }));
    for (key, title, secure) in [
        ("id", "Provider ID (required, stable)", false),
        ("name", "Provider name", false),
        ("baseUrl", "API base URL", false),
        ("model", "Model identifier", false),
        ("apiKey", "API key (blank keeps existing key)", true),
    ] {
        body = body.child(field(
            state,
            kit,
            &format!("provider.{key}"),
            title,
            false,
            secure,
        ));
    }
    let selected = providers
        .iter()
        .find(|p| string(p, "id") == state.ui.form("provider.id"))
        .cloned();
    if let Some(provider) = selected {
        body = body.child(command(
            state,
            kit,
            "Fetch models from selected provider",
            "fetch_provider_models",
            json!({"provider": provider}),
            format!("models-{}", state.ui.form("provider.id")),
        ));
        let mut models = provider
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        models.extend(
            provider
                .get("customModels")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        models.extend(
            state
                .ui
                .rows(&format!("models-{}", state.ui.form("provider.id"))),
        );
        let mut seen = BTreeSet::new();
        for model in models
            .into_iter()
            .filter_map(|m| m.as_str().map(str::to_owned))
            .filter(|m| seen.insert(m.clone()))
        {
            let m = model.clone();
            body = body.child(action(
                kit,
                format!("model-{model}"),
                model,
                false,
                move |s| s.ui.set_form("provider.model", m.clone()),
            ));
        }
    }
    body.child(action(kit, "provider-save", "Save provider & make active", state.ui.pending("save-settings"), |s| {
        if !s.configuration_ready() {return;}
        let outcome = (|| -> Result<Value, String> {
            let mut settings = s.ui.settings()?; let id = s.ui.form("provider.id").trim();
            if id.is_empty() { return Err("Provider ID is required".into()); }
            let providers = settings.get_mut("providers").and_then(Value::as_array_mut).ok_or("Settings providers must be an array")?;
            let position = providers.iter().position(|p| string(p, "id") == id);
            let mut provider = position.map(|i| providers[i].clone()).unwrap_or_else(|| serde_json::to_value(ProviderConfig::default()).unwrap_or_default());
            for key in ["id", "name", "baseUrl", "model"] { provider[key] = json!(s.ui.form(&format!("provider.{key}"))); }
            if !s.ui.form("provider.apiKey").is_empty() { provider["apiKey"] = json!(s.ui.form("provider.apiKey")); }
            if let Some(i) = position { providers[i] = provider; } else { providers.push(provider); }
            settings["activeProviderId"] = json!(id); Ok(settings)
        })();
        match outcome { Ok(value) => s.ui.save_settings(&mut s.controller, value), Err(e) => s.ui.validation_error = Some(e) }
    })).child(note("Provider kind, headers, extra body, favorites and custom model lists are editable in complete Settings JSON; secret headers remain redacted."))
        .child(result(state, kit, "save-settings"))
}

fn conversation_blocked(state: &AppState) -> bool {
    let model = state.controller.model();
    model.busy
        || model.loading
        || model.loading_older
        || model.preview.is_some()
        || state.ui.pending("conversation-action")
}

/// Current loaded history only; mutations and canonical replay belong to Controller.
fn conversation_panel(state: &AppState, kit: &Kit) -> El {
    let Some(current) = &state.controller.model().current else {
        return note("Open a conversation to edit, retry, roll back or undo a turn.");
    };
    let disabled = conversation_blocked(state);
    let mut body = col()
        .key("conversation-actions")
        .w_full()
        .gap(10.0)
        .child(note(
            "Current conversation — history changes require confirmation.",
        ));
    if current.messages.iter().any(|m| m.role == "user") {
        body = body.child(action(
            kit,
            "conversation-undo",
            "Undo latest turn…",
            disabled,
            |s| s.ui.conversation_action(&mut s.controller, "undo_turn", ""),
        ));
    }
    for message in &current.messages {
        if !["user", "assistant"].contains(&message.role.as_str()) {
            continue;
        }
        let mut entry = col().w_full().gap(6.0).child(note(format!(
            "{}: {}",
            message.role,
            message.content.chars().take(220).collect::<String>()
        )));
        let mut controls = row().gap(6.0).flex_wrap();
        let actions: &[(&str, &str, &str)] = if message.role == "user" {
            &[
                ("edit", "Edit & resend…", "edit_and_resend"),
                ("retry", "Retry turn…", "retry_turn"),
            ]
        } else {
            &[("rollback", "Roll back to this message…", "rollback_turn")]
        };
        for &(key, title, command) in actions {
            let id = message.id.clone();
            controls = controls.child(action(
                kit,
                format!("conversation-{key}-{id}"),
                title,
                disabled,
                move |s| s.ui.conversation_action(&mut s.controller, command, &id),
            ));
        }
        entry = entry.child(controls);
        body = body.child(wrap_message(state, &message.id, entry, kit));
    }
    body.child(result(state, kit, "conversation-action"))
}

/// Wrap a message with Rune's native secondary-press / Menu-key handler.
/// PointerEvent::window(), not local coordinates, anchors the popup correctly.
pub fn wrap_message(s: &AppState, message_id: &str, trigger: El, kit: &Kit) -> El {
    message_menu(s, message_id, trigger, kit)
}

/// A real RuneKit modal context menu: keyboard navigation, outside/Escape
/// dismissal and focus restoration are supplied by Menu, not a fake action list.
pub fn message_menu(s: &AppState, message_id: &str, trigger: El, kit: &Kit) -> El {
    let model = s.controller.model();
    let Some(data) = model
        .preview
        .as_ref()
        .map(|p| &p.data)
        .or(model.current.as_ref())
    else {
        return trigger;
    };
    let Some(message) = data.messages.iter().find(|m| m.id == message_id) else {
        return trigger;
    };
    let session = data.session.id.clone();
    let id = message_id.to_owned();
    let point =
        s.ui.message_context
            .as_ref()
            .filter(|(sid, mid, _)| sid == &session && mid == &id)
            .map(|(_, _, p)| *p);
    let trigger =
        trigger
            .focusable(true)
            .on_secondary_press_local(move |s: &mut AppState, p, tx| {
                s.ui.message_context = Some((session.clone(), id.clone(), p.window()));
                if s.shell.reduced_motion {
                    tx.instant();
                }
            });
    let key = format!("message-context-{message_id}");
    let raw = message.content.clone();
    let plain = domain::plain_text(&raw);
    let mut menu = context_menu(&key, trigger, point)
        .on_close(|s: &mut AppState| s.ui.message_context = None)
        .item(
            MenuItem::new("Copy raw message").on_select_tx(move |s: &mut AppState, tx| {
                tx.copy(raw.clone());
                s.ui.message_context = None;
            }),
        )
        .item(
            MenuItem::new("Copy text").on_select_tx(move |s: &mut AppState, tx| {
                tx.copy(plain.clone());
                s.ui.message_context = None;
            }),
        );
    let disabled = conversation_blocked(s);
    let actions: &[(&str, &str)] = match message.role.as_str() {
        "user" => &[
            ("Edit & resend…", "edit_and_resend"),
            ("Retry turn…", "retry_turn"),
        ],
        "assistant" => &[("Roll back to this message…", "rollback_turn")],
        _ => &[],
    };
    for &(title, command) in actions {
        let id = message_id.to_owned();
        menu = menu.item(MenuItem::new(title).disabled(disabled).on_select(
            move |s: &mut AppState| {
                s.ui.message_context = None;
                s.ui.conversation_action(&mut s.controller, command, &id);
            },
        ));
    }
    if data.messages.last().is_some_and(|m| m.id == message_id)
        && data.messages.iter().any(|m| m.role == "user")
    {
        menu = menu.item(
            MenuItem::new("Undo latest turn…")
                .disabled(disabled)
                .on_select(|s: &mut AppState| {
                    s.ui.message_context = None;
                    s.ui.conversation_action(&mut s.controller, "undo_turn", "");
                }),
        );
    }
    let mut layer = Vec::new();
    let body = menu.build(kit, &mut layer);
    // Mount the actual overlay layer beside its anchor. Unlike kit::root this
    // wrapper must not force a message row to occupy the full viewport height.
    rune::el()
        .key(format!("{key}-root"))
        .w_full()
        .child(body)
        .children(layer)
}
fn history_panel(state: &AppState, kit: &Kit) -> El {
    let mut body = col()
        .gap(10.0)
        .child(conversation_panel(state, kit))
        .child(field(
            state,
            kit,
            "history.query",
            "Search titles, messages and artifacts",
            false,
            false,
        ))
        .child(action(
            kit,
            "history-search",
            "Deep search",
            state.ui.pending("search"),
            |s| {
                let query = s.ui.form("history.query").trim().to_owned();
                if query.is_empty() {
                    s.ui.validation_error = Some("Enter a search query".into());
                    return;
                }
                s.ui.request(
                    &mut s.controller,
                    "deep_search",
                    json!({"query":query,"limit":50}),
                    "search",
                );
            },
        ))
        .child(command(
            state,
            kit,
            "Refresh sessions",
            "list_sessions",
            json!({}),
            "sessions",
        ));
    for hit in state.ui.rows("search") {
        let session = string(&hit, "sessionId");
        let message = string(&hit, "matchMessageId");
        body = body.child(action(
            kit,
            format!("search-{session}-{message}"),
            label(&hit, &["sessionTitle", "snippet"]),
            false,
            move |s| {
                if message.is_empty() {
                    s.controller.open_session(&session);
                } else {
                    s.controller.open_session(&session);
                    s.controller.dispatch(
                        "load_session_around",
                        json!({"sessionId":session,"messageId":message,"limit":50}),
                    );
                }
                s.ui.selected = session.clone();
                s.ui.panel = None;
            },
        ));
    }
    let sessions = serde_json::to_value(&state.controller.model().sessions)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    let sessions = if state.ui.value("sessions").is_some() {
        state.ui.rows("sessions")
    } else {
        sessions
    };
    for session in sessions {
        let id = string(&session, "id");
        let open_id = id.clone();
        let select_id = id.clone();
        body = body.child(
            row()
                .gap(6.0)
                .child(action(
                    kit,
                    format!("session-open-{id}"),
                    label(&session, &["title", "preview"]),
                    false,
                    move |s| {
                        s.controller.open_session(&open_id);
                        s.ui.panel = None;
                    },
                ))
                .child(action(
                    kit,
                    format!("session-select-{id}"),
                    "Actions",
                    false,
                    move |s| {
                        s.ui.selected = select_id.clone();
                        s.ui.set_form("session.title", string(&session, "title"));
                    },
                )),
        );
    }
    if !state.ui.selected.is_empty() {
        let id = state.ui.selected.clone();
        body = body
            .child(note(format!("Selected session: {id}")))
            .child(field(
                state,
                kit,
                "session.title",
                "Session title",
                false,
                false,
            ))
            .child(action(
                kit,
                "session-rename",
                "Rename",
                state.ui.pending("rename"),
                move |s| {
                    let args = json!({"sessionId":id,"title":s.ui.form("session.title")});
                    s.ui.issue(
                        &mut s.controller,
                        Request::new("rename_session_summary", args, "rename"),
                        Some(Request::new("list_sessions", json!({}), "sessions")),
                    );
                },
            ))
            .child(command(
                state,
                kit,
                "Toggle pinned",
                "toggle_pin_session",
                json!({"sessionId":state.ui.selected}),
                "session-pin",
            ))
            .child(command(
                state,
                kit,
                "Compact session…",
                "compact_session",
                json!({"sessionId":state.ui.selected}),
                "session-compact",
            ))
            .child(action(
                kit,
                "session-delete",
                "Delete selected session…",
                false,
                |s| {
                    s.ui.mutate(
                        &mut s.controller,
                        Request::new(
                            "delete_session",
                            json!({"sessionId":s.ui.selected}),
                            "session-delete",
                        ),
                        Some(Request::new("list_sessions", json!({}), "sessions")),
                        "permanently delete the selected session",
                    );
                },
            ))
            .child(field(
                state,
                kit,
                "export.destination",
                "Export destination path (explicit fallback; no native save dialog in this panel)",
                false,
                false,
            ))
            .child(field(
                state,
                kit,
                "export.format",
                "Export format: markdown or json",
                false,
                false,
            ))
            .child(action(
                kit,
                "session-export",
                "Export to explicit destination",
                state.ui.pending("export"),
                |s| {
                    let path = s.ui.form("export.destination").trim();
                    let format = s.ui.form("export.format").trim();
                    if path.is_empty() || !["markdown", "json"].contains(&format) {
                        s.ui.validation_error =
                            Some("Enter a destination and choose markdown or json".into());
                        return;
                    }
                    let args =
                        json!({"sessionId":s.ui.selected,"destination":path,"format":format});
                    s.ui.request(&mut s.controller, "export_session", args, "export");
                },
            ))
            .child(result(state, kit, "export"));
    }
    body.child(result(state, kit, "search"))
}

fn artifacts_panel(state: &AppState, kit: &Kit) -> El {
    let mut body = col().gap(10.0).child(command(
        state,
        kit,
        "Refresh artifact library",
        "list_all_artifacts",
        json!({}),
        "artifacts",
    ));
    let mut artifacts = state.ui.rows("artifacts");
    if let Some(current) = &state.controller.model().current {
        for artifact in &current.artifacts {
            artifacts.push(json!({"sessionId":current.session.id,"sessionTitle":current.session.title,"artifact":artifact}));
        }
    }
    let mut seen = BTreeSet::new();
    for item in artifacts {
        let artifact = item.get("artifact").unwrap_or(&item);
        let session = string(&item, "sessionId");
        let id = string(artifact, "id");
        let slot = domain::scope("artifact", &session, &id);
        if !seen.insert(slot.clone()) {
            continue;
        }
        let select = slot.clone();
        let read_session = session.clone();
        let read_id = id.clone();
        body = body.child(action(
            kit,
            format!("artifact-tab-{slot}"),
            format!(
                "{} · {}",
                string(artifact, "title"),
                string(&item, "sessionTitle")
            ),
            state.ui.pending(&slot),
            move |s| {
                s.ui.selected = select.clone();
                s.ui.request(
                    &mut s.controller,
                    "read_session_artifact",
                    json!({"sessionId":read_session,"artifactId":read_id}),
                    select.clone(),
                );
            },
        ));
        if state.ui.selected == slot {
            let content_artifact = state.ui.value(&slot).unwrap_or(artifact);
            let source = string(content_artifact, "content");
            let kind = string(content_artifact, "type");
            let source_mode = state.ui.expanded.contains("artifact-source");
            body = body.child(toggle(
                kit,
                "artifact-view-source",
                "Source view",
                source_mode,
                |s| {
                    if !s.ui.expanded.remove("artifact-source") {
                        s.ui.expanded.insert("artifact-source".into());
                    }
                },
            ));
            let language = string(content_artifact, "language");
            let rendered =
                if !source_mode && ["markdown", "plan", "document"].contains(&kind.as_str()) {
                    source
                } else {
                    domain::code(
                        &source,
                        if kind == "diff" {
                            "diff"
                        } else if language.is_empty() {
                            "text"
                        } else {
                            &language
                        },
                    )
                };
            body = body.child(rich(state, kit, &rendered, &slot));
            let path = string(content_artifact, "filePath");
            if !path.is_empty() {
                body = body
                    .child(command(
                        state,
                        kit,
                        "Open artifact file",
                        "open_path_or_url",
                        json!({"pathOrUrl":path}),
                        "artifact-open",
                    ))
                    .child(command(
                        state,
                        kit,
                        "Reveal artifact file",
                        "reveal_in_explorer",
                        json!({"path":path}),
                        "artifact-reveal",
                    ));
            }
        }
    }
    body
}

fn tools_panel(state: &AppState, kit: &Kit) -> El {
    let mut body = col().gap(10.0).child(command(
        state,
        kit,
        "Refresh native tools",
        "list_native_tools",
        json!({}),
        "native-tools",
    ));
    let native = state
        .ui
        .value("native-tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for tool in native {
        let data = tool.get("function").unwrap_or(&tool);
        body = body.child(note(label(data, &["name", "description"])));
    }
    body = body.child(note("MCP calls are bound to the exact selected configured server ID. Selecting another server never reuses the previous server's tool list."));
    for server in &state.controller.model().settings.mcp_servers {
        let id = server.id.clone();
        let config = serde_json::to_value(server).unwrap_or_default();
        let slot = domain::scope("mcp-tools", &id, "");
        let selected = state.ui.form("mcp.server") == id;
        body = body.child(action(
            kit,
            format!("mcp-server-{id}"),
            format!(
                "{}{} ({})",
                if selected { "✓ " } else { "" },
                server.name,
                id
            ),
            !server.enabled,
            move |s| {
                s.ui.set_form("mcp.server", id.clone());
                s.ui.set_form("mcp.tool", "");
                s.ui.request(
                    &mut s.controller,
                    "list_mcp_tools",
                    json!({"server":config}),
                    slot.clone(),
                );
            },
        ));
    }
    let server_id = state.ui.form("mcp.server");
    for tool in state.ui.rows(&domain::scope("mcp-tools", server_id, "")) {
        let name = string(&tool, "name");
        let tool_name = name.clone();
        body = body.child(action(
            kit,
            format!("mcp-tool-{server_id}-{name}"),
            label(&tool, &["name", "description"]),
            false,
            move |s| s.ui.set_form("mcp.tool", tool_name.clone()),
        ));
        if state.ui.form("mcp.tool") == name {
            body = body.child(rich(
                state,
                kit,
                &domain::code(
                    &pretty(tool.get("inputSchema").unwrap_or(&Value::Null)),
                    "json",
                ),
                &format!("schema-{server_id}-{name}"),
            ));
        }
    }
    body = body
        .child(field(
            state,
            kit,
            "mcp.arguments",
            "Selected MCP tool arguments (validated JSON object)",
            true,
            false,
        ))
        .child(action(
            kit,
            "mcp-call",
            "Call selected configured MCP tool",
            state.ui.pending("mcp-call"),
            |s| {
                let id = s.ui.form("mcp.server");
                let name = s.ui.form("mcp.tool");
                let Some(server) = s
                    .controller
                    .model()
                    .settings
                    .mcp_servers
                    .iter()
                    .find(|server| server.id == id && server.enabled)
                else {
                    s.ui.validation_error = Some("Select an enabled configured MCP server".into());
                    return;
                };
                if !s
                    .ui
                    .rows(&domain::scope("mcp-tools", id, ""))
                    .iter()
                    .any(|tool| string(tool, "name") == name)
                {
                    s.ui.validation_error =
                        Some("Select a tool returned by this exact server".into());
                    return;
                }
                match domain::object(s.ui.form("mcp.arguments")) {
                    Ok(arguments) => {
                        let args = json!({"server":server,"name":name,"arguments":arguments});
                        s.ui.request(&mut s.controller, "call_mcp_tool", args, "mcp-call");
                    }
                    Err(e) => s.ui.validation_error = Some(e),
                }
            },
        ))
        .child(result(state, kit, "mcp-call"))
        .child(command(
            state,
            kit,
            "Refresh skills",
            "list_skills",
            json!({}),
            "skills",
        ));
    for skill in state.ui.rows("skills") {
        body = body.child(note(label(&skill, &["name", "description", "filePath"])));
    }
    body.child(field(state, kit, "skill.name", "Skill name", false, false))
        .child(field(state, kit, "skill.description", "Skill description", false, false))
        .child(field(state, kit, "skill.instructions", "Skill Markdown instructions", true, false))
        .child(action(kit, "skill-save", "Save skill…", false, |s| { let args = json!({"name":s.ui.form("skill.name"),"description":s.ui.form("skill.description"),"instructions":s.ui.form("skill.instructions")}); s.ui.mutate(&mut s.controller, Request::new("save_skill", args, "skill-save"), Some(Request::new("list_skills", json!({}), "skills")), "save skill Markdown"); }))
}

fn memory_panel(state: &AppState, kit: &Kit) -> El {
    let workspace = state.ui.form("memory.workspace");
    let changed = workspace != state.controller.model().workspace;
    let args = json!({"expectedWorkspace":workspace});
    col().gap(10.0).child(note(format!("Workspace: {workspace}\nPath: {}\nMemory is scoped to this workspace, not the user's global profile.", std::path::Path::new(workspace).join("AGENTS.md").display())))
        .child(if changed { note("Workspace changed. Reopen this panel before saving; the service will also reject a stale workspace.") } else { note("AGENTS.md is the workspace's durable agent memory. Read / Edit / Preview share the same actual file.") })
        .child(command(state, kit, "Read workspace AGENTS.md", "read_workspace_agents_md", args.clone(), "memory"))
        .child(command(state, kit, "Initialize starter AGENTS.md if absent…", "init_workspace_agents_md", args, "memory-init"))
        .child(field(state, kit, "memory.content", "AGENTS.md Markdown", true, false))
        .child(action(kit, "memory-save", "Save workspace memory…", changed || state.ui.pending("memory-save"), |s| {
            let args = json!({"content":s.ui.form("memory.content"),"expectedWorkspace":s.ui.form("memory.workspace")});
            s.ui.mutate(&mut s.controller, Request::new("save_workspace_agents_md", args, "memory-save"), Some(Request::new("read_workspace_agents_md", json!({"expectedWorkspace":s.ui.form("memory.workspace")}), "memory")), "replace this workspace's AGENTS.md");
        })).child(rich(state, kit, state.ui.form("memory.content"), "memory-preview"))
        .child(result(state, kit, "memory-save"))
}

fn shell_panel(state: &AppState, kit: &Kit) -> El {
    col().gap(10.0).child(note("Live Shell runs one bounded noninteractive command in the actual workspace. It is not a PTY: no terminal input, shell session persistence, or interactive programs. Long-running agent tasks are shown separately."))
        .child(note(format!("Current workspace: {}", state.controller.model().workspace)))
        .child(field(state, kit, "workspace.path", "Workspace path", false, false))
        .child(action(kit, "workspace-set", "Set workspace", state.ui.pending("workspace-set"), |s| { let path = s.ui.form("workspace.path").to_owned(); if path.trim().is_empty() { s.ui.validation_error = Some("Workspace path is required".into()); return; } s.ui.issue(&mut s.controller, Request::new("set_workspace_cwd", json!({"newCwd":path}), "workspace-set"), Some(Request::new("get_workspace_cwd", json!({}), "workspace"))); }))
        .child(field(state, kit, "shell.command", "Command", true, false))
        .child(action(kit, "shell-run", if state.ui.pending("shell") { "Running…" } else { "Run command" }, state.ui.pending("shell"), |s| { let command = s.ui.form("shell.command").trim().to_owned(); if command.is_empty() { s.ui.validation_error = Some("Enter a command".into()); return; } s.ui.request(&mut s.controller, "run_shell_command", json!({"command":command}), "shell"); }))
        .child(result(state, kit, "shell"))
        .child(command(state, kit, "Open OS terminal", "open_terminal", json!({"path":state.controller.model().workspace}), "terminal"))
        .child(command(state, kit, "Reveal workspace", "reveal_in_explorer", json!({"path":state.controller.model().workspace}), "workspace-reveal"))
}

fn tasks_panel(state: &AppState, kit: &Kit) -> El {
    let Some(session) = UiState::session(&state.controller) else {
        return note("Open a session to inspect its actual background tasks.");
    };
    let slot = domain::scope("tasks", &session, "");
    let mut body = col()
        .gap(10.0)
        .child(note(format!("Tasks for exact session: {session}")))
        .child(command(
            state,
            kit,
            "Refresh session tasks",
            "list_background_tasks",
            json!({"sessionId":session}),
            &slot,
        ));
    for task in state.ui.rows(&slot) {
        // Fail closed if a malformed result reports another session.
        if string(&task, "session_id") != session && string(&task, "sessionId") != session {
            continue;
        }
        let id = string(&task, "id");
        let output_slot = domain::scope("task-output", &session, &id);
        body = body
            .child(note(label(&task, &["id", "status", "command"])))
            .child(command(
                state,
                kit,
                "Read recent output / status",
                "get_background_task_output",
                json!({"sessionId":session,"taskId":id,"tailLines":100}),
                &output_slot,
            ));
        if ["running", "starting"].contains(&string(&task, "status").as_str()) {
            let task_id = id.clone();
            let session_id = session.clone();
            let refresh_slot = slot.clone();
            body = body.child(action(
                kit,
                format!("task-kill-{id}"),
                "Kill this task…",
                state.ui.pending(&domain::scope("task-kill", &session, &id)),
                move |s| {
                    s.ui.mutate(
                        &mut s.controller,
                        Request::new(
                            "kill_background_task",
                            json!({"sessionId":session_id,"taskId":task_id}),
                            domain::scope("task-kill", &session_id, &task_id),
                        ),
                        Some(Request::new(
                            "list_background_tasks",
                            json!({"sessionId":session_id}),
                            refresh_slot.clone(),
                        )),
                        "terminate this task",
                    );
                },
            ));
        }
        if state.ui.value(&output_slot).is_some() || state.ui.pending(&output_slot) {
            body = body.child(result(state, kit, &output_slot));
        }
        let recent = string(&task, "recent_output");
        if !recent.is_empty() {
            body = body.child(rich(
                state,
                kit,
                &domain::code(&recent, "shell"),
                &format!("recent-{session}-{id}"),
            ));
        }
    }
    body.child(result(state, kit, &slot))
}

fn review_panel(state: &AppState, kit: &Kit) -> El {
    let Some(session) = UiState::session(&state.controller) else {
        return note("Open a session to inspect its real review proposals.");
    };
    let slot = domain::scope("review", &session, "");
    let mut body = col().gap(10.0)
        .child(note(format!("Review scope: {session}. Decisions refresh after service acknowledgement; prior proposal history stays visible.")))
        .child(command(state, kit, "Refresh proposals", "list_review_proposals", json!({"sessionId":session}), &slot));
    if let Some(provider) = state.candidate_a()
    {
        body = body.child(command(
            state,
            kit,
            &format!("Request session review - {} / {}",provider.name,provider.model),
            "review_session",
            json!({"sessionId":session,"provider":provider}),
            domain::scope("review-run", &session, ""),
        ));
    }
    for proposal in state
        .ui
        .proposal_history
        .get(&session)
        .cloned()
        .unwrap_or_else(|| state.ui.rows(&slot))
    {
        let id = string(&proposal, "id");
        if id.is_empty() {
            continue;
        }
        let diff_slot = domain::scope("proposal-diff", &session, &id);
        body = body
            .child(note(label(
                &proposal,
                &["id", "name", "status", "skill_name", "summary"],
            )))
            .child(command(
                state,
                kit,
                "Read proposal diff",
                "review_proposal_diff",
                json!({"sessionId":session,"id":id}),
                &diff_slot,
            ));
        if let Some(value) = state.ui.value(&diff_slot) {
            let diff = value.as_str().map(str::to_owned).unwrap_or_else(|| {
                let diff = string(value, "diff");
                if diff.is_empty() { pretty(value) } else { diff }
            });
            body = body.child(rich(state, kit, &domain::code(&diff, "diff"), &diff_slot));
        }
        let status = string(&proposal, "status");
        if status.is_empty() || ["pending", "proposed"].contains(&status.as_str()) {
            for (command, title) in [
                ("approve_review_proposal", "Approve…"),
                ("reject_review_proposal", "Reject…"),
            ] {
                let proposal_id = id.clone();
                let session_id = session.clone();
                let refresh_slot = slot.clone();
                let action_slot = domain::scope(command, &session, &id);
                let pending =
                    state
                        .ui
                        .pending(&domain::scope("approve_review_proposal", &session, &id))
                        || state.ui.pending(&domain::scope(
                            "reject_review_proposal",
                            &session,
                            &id,
                        ));
                body = body.child(action(
                    kit,
                    format!("review-{command}-{id}"),
                    if pending {
                        "Decision pending…"
                    } else {
                        title
                    },
                    pending,
                    move |s| {
                        s.ui.mutate(
                            &mut s.controller,
                            Request::new(
                                command,
                                json!({"sessionId":session_id,"id":proposal_id}),
                                action_slot.clone(),
                            ),
                            Some(Request::new(
                                "list_review_proposals",
                                json!({"sessionId":session_id}),
                                refresh_slot.clone(),
                            )),
                            command,
                        );
                    },
                ));
            }
        }
    }
    body
}

fn git_panel(state: &AppState, kit: &Kit) -> El {
    let mut body = col()
        .gap(10.0)
        .child(note(format!(
            "Repository workspace: {}",
            state.controller.model().workspace
        )))
        .child(command(
            state,
            kit,
            "Refresh Git status",
            "git_status",
            json!({}),
            "git",
        ));
    if let Some(status) = state.ui.value("git") {
        body = body.child(note(label(status, &["branch", "summary"])));
        for file in status
            .get("files")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let path = string(&file, "path");
            body = body.child(note(label(&file, &["status", "path"])));
            for (command, title) in [
                ("stage_file_changes", "Stage file"),
                ("revert_file_changes", "Revert file changes…"),
            ] {
                let p = path.clone();
                let slot = domain::scope(command, &state.controller.model().workspace, &path);
                body = body.child(action(
                    kit,
                    format!("git-{command}-{path}"),
                    title,
                    state.ui.pending(&slot),
                    move |s| {
                        s.ui.mutate(
                            &mut s.controller,
                            Request::new(command, json!({"path":p}), slot.clone()),
                            Some(Request::new("git_status", json!({}), "git")),
                            &format!("{command}: {p}"),
                        );
                    },
                ));
            }
        }
    }
    body = body
        .child(field(
            state,
            kit,
            "git.message",
            "Commit message",
            true,
            false,
        ))
        .child(action(
            kit,
            "git-commit",
            "Commit staged changes",
            state.ui.pending("git-commit"),
            |s| {
                let message = s.ui.form("git.message").trim().to_owned();
                if message.is_empty() {
                    s.ui.validation_error = Some("Commit message is required".into());
                    return;
                }
                s.ui.issue(
                    &mut s.controller,
                    Request::new("git_commit", json!({"message":message}), "git-commit"),
                    Some(Request::new("git_status", json!({}), "git")),
                );
            },
        ))
        .child(command(
            state,
            kit,
            "Fetch remote",
            "git_fetch",
            json!({}),
            "git-fetch",
        ))
        .child(command(
            state,
            kit,
            "Push…",
            "git_push",
            json!({}),
            "git-push",
        ));
    body.child(result(state, kit, "git"))
        .child(result(state, kit, "git-commit"))
}

fn about_panel(state: &AppState, kit: &Kit) -> El {
    let mut body = col().gap(10.0).child(command(
        state,
        kit,
        "Refresh release information",
        "get_release_info",
        json!({}),
        "release",
    ));
    if let Some(info) = state.ui.value("release") {
        for key in [
            "version",
            "buildId",
            "platform",
            "arch",
            "signingStatus",
            "updateStatus",
            "diagnosticLogsPath",
        ] {
            body = body.child(note(format!("{key}: {}", string(info, key))));
        }
        let path = string(info, "diagnosticLogsPath");
        if !path.is_empty() {
            body = body.child(command(
                state,
                kit,
                "Reveal diagnostic logs",
                "reveal_in_explorer",
                json!({"path":path}),
                "logs",
            ));
        }
    }
    let configured = state.ui.value("release").is_some_and(|v| v["updatesConfigured"] == true);
    body = body.child(action(kit, "release-check", "Check for signed updates", !configured || state.ui.pending("release-check") || state.ui.pending("release-install"), |s| {
        s.ui.replies.remove("release-check");
        s.ui.replies.remove("release-install");
        s.ui.request(&mut s.controller, "check_release_update", json!({}), "release-check");
    }));
    if let Some(check) = state.ui.value("release-check") {
        body = body.child(note(string(check, "status")));
        if let (Some(version), Some(ticket)) = (check["version"].as_str(), check["ticketId"].as_str()) {
            let stage = check["action"] == "stage";
            let args = json!({"expectedVersion":version,"ticketId":ticket,"confirmed":true});
            let consumed = state.ui.replies.contains_key("release-install");
            body = body.child(action(kit, "release-install", if stage { "Download & verify AppImage for manual installation..." } else { "Download, verify & open installer..." }, consumed || state.ui.pending("release-install") || state.ui.pending("release-check"), move |s| {
                s.ui.mutate(&mut s.controller, Request::new("install_release_update", args.clone(), "release-install"), None,
                    "download and verify this exact signed version, then open its installer on Windows or stage it for manual installation on Linux");
            }));
        }
        if let Some(notes) = check["notes"].as_str() { body = body.child(note(format!("Release notes (server supplied): {notes}"))); }
    }
    body.child(result(state, kit, "release-check"))
        .child(result(state, kit, "release-install"))
        .child(note("No background downloads. A checked version and explicit confirmation are required. Linux only stages a verified AppImage; it does not replace the running application. Installer signatures do not establish this running binary's OS signing status."))
}

fn advanced_panel(state: &AppState, kit: &Kit) -> El {
    let expanded = state.ui.expanded.contains("advanced-service");
    let mut body = col().gap(8.0).child(toggle(
        kit,
        "advanced-toggle",
        "Advanced service operations & completion history",
        expanded,
        |s| {
            if !s.ui.expanded.remove("advanced-service") {
                s.ui.expanded.insert("advanced-service".into());
            }
        },
    ));
    if !expanded {
        return body;
    }
    body = body.child(note("Real backend operations. Select a command to populate its actual argument schema, edit required values, then dispatch. Session persistence operations are advanced: prefer the shell's canonical session controls. Secrets are redacted from completion history."));
    for name in aether_core::service::Backend::supported_commands() {
        let name = *name;
        body = body.child(action(
            kit,
            format!("advanced-select-{name}"),
            if lifecycle_owned(name) {format!("{name} - Controller-owned (schema only)")} else {name.into()},
            false,
            move |s| {
                s.ui.set_form("advanced.command", name);
                let template = argument_template(name, &s.controller);
                s.ui.set_form("advanced.arguments", pretty(&domain::redact(&template)));
                // Secrets in templates are restored by identity from this private source.
                s.ui.replies.insert(
                    "advanced-template".into(),
                    Reply {
                        id: 0,
                        value: Some(template),
                        error: None,
                    },
                );
            },
        ));
    }
    if lifecycle_owned(state.ui.form("advanced.command")) {
        body = body.child(note("Controller-owned operation: use the session/history controls or Retry save. Recovery proof is issued only by native storage after a committed save; null is a schema placeholder, never a fabricated acknowledgement."));
    }
    body = body.child(note(format!("Selected command: {}", state.ui.form("advanced.command"))))
        .child(field(state, kit, "advanced.arguments", "Command arguments (JSON object)", true, false))
        .child(action(kit, "advanced-dispatch", "Validate & dispatch selected command", state.ui.pending("advanced") || lifecycle_owned(state.ui.form("advanced.command")) || !state.configuration_ready(), |s| {
            let command = s.ui.form("advanced.command").to_owned();
            if lifecycle_owned(&command) || !s.configuration_ready() { s.ui.validation_error=Some("Wait for configuration and use typed Controller controls for lifecycle-owned operations".into()); return; }
            if !aether_core::service::Backend::supported_commands().contains(&command.as_str()) { s.ui.validation_error = Some("Select a supported command".into()); return; }
            let base = s.ui.value("advanced-template").cloned().unwrap_or_else(|| json!({}));
            let args = domain::settings_document(s.ui.form("advanced.arguments"), &base);
            match args {
                Ok(mut args) => {
                    if ["list_mcp_tools", "call_mcp_tool"].contains(&command.as_str()) {
                        let id = args.get("server").map(|v| string(v, "id")).unwrap_or_default();
                        let Some(server) = s.controller.model().settings.mcp_servers.iter().find(|server| server.enabled && server.id == id) else {
                            s.ui.validation_error = Some("Select an enabled configured MCP server ID; arbitrary server processes are not allowed in this panel".into()); return;
                        };
                        args["server"] = serde_json::to_value(server).unwrap_or_default();
                    }
                    s.ui.mutate(&mut s.controller, Request::new(&command, args, "advanced"), None, &command);
                }
                Err(e) => s.ui.validation_error = Some(e),
            }
        })).child(result(state, kit, "advanced"));
    let mut replies: Vec<_> = state
        .ui
        .replies
        .iter()
        .filter(|(slot, _)| slot.as_str() != "advanced-template")
        .collect();
    replies.sort_by_key(|(_, reply)| std::cmp::Reverse(reply.id));
    for (slot, reply) in replies.into_iter().take(12) {
        body = body
            .child(note(format!("Completion #{} · {slot}", reply.id)))
            .child(result(state, kit, slot));
    }
    body
}

fn lifecycle_owned(command: &str) -> bool {
    matches!(command,"recover_save_ack"|"replace_session_checked"|"save_session"|"checkpoint_session"|"checkpoint_session_delta"|"refresh_session_window")
}
/// Required argument names mirror service.rs; these are editable templates, not
/// fabricated records. Empty IDs must be filled explicitly by the operator.
fn argument_template(command: &str, controller: &Controller) -> Value {
    let session = UiState::session(controller).unwrap_or_default();
    let current = controller
        .model()
        .current
        .as_ref()
        .and_then(|v| serde_json::to_value(v).ok())
        .unwrap_or(Value::Null);
    let window = controller
        .model()
        .window
        .as_ref()
        .and_then(|v| serde_json::to_value(v).ok())
        .unwrap_or(Value::Null);
    let over = controller.model().current.as_ref().and_then(|d|d.model_override.as_ref());
    let id = over.and_then(|o|o.provider_id.as_deref()).unwrap_or(&controller.model().settings.active_provider_id);
    let provider = controller.model().settings.providers.iter().find(|p|p.id==id).and_then(|p| {
        let mut p=p.clone(); if let Some(model)=over.and_then(|o|o.model.as_ref()) {p.model=model.clone();}
        serde_json::to_value(p).ok()
    }).unwrap_or(Value::Null);
    match command {
        "send_message" => {
            json!({"request":{"runId":"","sessionId":session,"provider":provider,"messages":[]}})
        }
        "send_council_message" => {
            json!({"requestA":{"runId":"","sessionId":session,"provider":provider,"messages":[]},"requestB":{"runId":"","sessionId":session,"provider":provider,"messages":[]}})
        }
        "review_session" => json!({"sessionId":session,"provider":provider}),
        "read_reflection_skill" => {
            json!({"sessionId":session,"skillName":"","runId":null,"filePath":null})
        }
        "review_proposal_diff" | "approve_review_proposal" | "reject_review_proposal" => {
            json!({"sessionId":session,"id":""})
        }
        "abort_agent" => {
            json!({"runId":controller.model().run_status.as_ref().map(|s| s.run_id.clone()).unwrap_or_default()})
        }
        "save_settings" => json!({"settings":controller.model().settings}),
        "warm_session_index" => json!({"limit":50,"budgetMs":100}),
        "load_session_page" => {
            json!({"sessionId":session,"before":null,"limit":50,"revision":null})
        }
        "load_session_around" => json!({"sessionId":session,"messageId":"","limit":50}),
        "save_session_page" | "checkpoint_session_page" => json!({"data":current,"window":window}),
        "recover_save_ack" => json!({"sessionId":session,"proof":null}),
        "replace_session_checked" => json!({"data":current,"expectedWindow":window}),
        "refresh_session_window" => {
            json!({"sessionId":session,"data":current,"window":window,"allowMetadata":false,"reviewRunId":null})
        }
        "read_message_image_inputs" => json!({"sessionId":session,"messageId":""}),
        "read_session_artifact" => json!({"sessionId":session,"artifactId":""}),
        "rename_session" | "rename_session_summary" => json!({"sessionId":session,"title":""}),
        "save_session" | "checkpoint_session" => json!({"data":current}),
        "checkpoint_session_delta" => json!({"sessionId":session,"request":{}}),
        "deep_search" => json!({"query":"","limit":50}),
        "export_session" => json!({"sessionId":session,"destination":"","format":"markdown"}),
        "list_review_proposals"
        | "load_session"
        | "toggle_pin_session"
        | "delete_session"
        | "compact_session"
        | "list_background_tasks" => json!({"sessionId":session}),
        "init_workspace_agents_md" | "read_workspace_agents_md" => {
            json!({"expectedWorkspace":controller.model().workspace})
        }
        "save_workspace_agents_md" => {
            json!({"content":"","expectedWorkspace":controller.model().workspace})
        }
        "fetch_provider_models" => json!({"provider":provider}),
        "set_workspace_cwd" => json!({"newCwd":controller.model().workspace}),
        "list_workspace_files" => json!({"query":"","limit":100}),
        "list_skills" => json!({"paths":controller.model().settings.skill_paths}),
        "save_skill" => json!({"name":"","description":"","instructions":""}),
        "list_mcp_tools" | "call_mcp_tool" => {
            let server = controller
                .model()
                .settings
                .mcp_servers
                .iter()
                .find(|s| s.enabled)
                .and_then(|s| serde_json::to_value(s).ok())
                .unwrap_or(Value::Null);
            if command == "list_mcp_tools" {
                json!({"server":server})
            } else {
                json!({"server":server,"name":"","arguments":{}})
            }
        }
        "git_commit" => json!({"message":""}),
        "run_shell_command" => json!({"command":""}),
        "revert_file_changes" | "stage_file_changes" | "reveal_in_explorer" => json!({"path":""}),
        "inspect_paths" => json!({"paths":[]}),
        "prepare_image_attachments" => json!({"attachments":[]}),
        "save_pasted_attachment" => {
            json!({"name":"","bytes":[],"expectedWorkspace":controller.model().workspace})
        }
        "open_path_or_url" => json!({"pathOrUrl":""}),
        "open_terminal" => json!({"path":controller.model().workspace}),
        "kill_background_task" => json!({"sessionId":session,"taskId":""}),
        "get_background_task_output" => json!({"sessionId":session,"taskId":"","tailLines":100}),
        _ => json!({}),
    }
}
