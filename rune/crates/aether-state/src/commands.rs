//! Composer command routing. Local actions are resolved before a user turn or
//! provider request exists; unknown slash-prefixed text is still ordinary chat.
use crate::Controller;
use aether_core::agent::{ExecutionMode, ProviderConfig};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const UI_ACTION_CAPACITY: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiAction {
    pub panel: String,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    Send {
        display: String,
        api: Option<String>,
        execution_mode: Option<ExecutionMode>,
    },
    Handled,
    Ui {
        panel: String,
        args: String,
    },
}

/// Overrides belong to one request only. Never save these into Model::settings.
#[derive(Debug, Clone, Default)]
pub struct SendOptions {
    pub api_prompt: Option<String>,
    pub execution_mode: Option<ExecutionMode>,
}

fn parts(text: &str) -> (String, &str) {
    let clean = text.trim();
    let end = clean.find(char::is_whitespace).unwrap_or(clean.len());
    (clean[..end].to_ascii_lowercase(), clean[end..].trim())
}
fn bare(command: &str, args: &str) -> Result<(), String> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(format!("{command} does not accept arguments"))
    }
}
fn ui(panel: &str, args: &str) -> InputAction {
    InputAction::Ui {
        panel: panel.into(),
        args: args.into(),
    }
}

/// Also used for literal history replay: expand run directives, but never execute
/// local commands such as /new, /clear or /review from a stored user prompt.
pub fn prompt_options(text: &str) -> Result<SendOptions, String> {
    let (command, args) = parts(text);
    match command.as_str() {
        "/plan" => Ok(SendOptions {
            api_prompt: Some(format!("Please formulate a structured step-by-step execution plan for: {}.\n\nFormat your plan inside a ```plan code block with checkable steps:\n```plan\n- [ ] Step 1: <Description>\n- [ ] Step 2: <Description>\n- [ ] Step 3: <Description>\n```\n\nThen proceed to carry out the steps sequentially, updating and reporting progress as each step completes.", if args.is_empty() { "the current task" } else { args })),
            execution_mode: None,
        }),
        "/agent" | "/ask" => {
            if args.is_empty() { return Err(format!("Usage: {command} <prompt>")); }
            Ok(SendOptions {
                api_prompt: Some(args.into()),
                execution_mode: Some(if command == "/ask" { ExecutionMode::RequestReview } else { ExecutionMode::Turbo }),
            })
        }
        _ => Ok(SendOptions::default()),
    }
}

impl Controller {
    /// All UI composer callers go through this entry point (or `send`).
    pub fn execute_input(&mut self, text: impl AsRef<str>) -> Result<(), String> {
        let action = self.route_input(text.as_ref())?;
        match action {
            InputAction::Send {
                display,
                api,
                execution_mode,
            } => self.send_with_options(
                display,
                SendOptions {
                    api_prompt: api,
                    execution_mode,
                },
            ),
            InputAction::Handled => Ok(()),
            InputAction::Ui { panel, args } => {
                if self.model.ui_actions.len() >= UI_ACTION_CAPACITY {
                    return Err(
                        "UI action queue is full; open or dismiss the pending panels first".into(),
                    );
                }
                self.model.ui_actions.push(UiAction { panel, args });
                self.model.draft.clear();
                Ok(())
            }
        }
    }

    /// Executes local actions and returns UI/send intents without starting chat.
    /// A returned error is never reinterpreted as an ordinary provider prompt.
    pub fn route_input(&mut self, text: &str) -> Result<InputAction, String> {
        if self.is_shutting_down() {
            return Err("Controller is shutting down".into());
        }
        let (command, args) = parts(text);
        match command.as_str() {
            "/new" => {
                self.new_session();
                if !args.is_empty() {
                    let id = self
                        .model
                        .current
                        .as_ref()
                        .ok_or("No session selected")?
                        .session
                        .id
                        .clone();
                    let live = self.live.get_mut(&id).ok_or("Session unavailable")?;
                    live.data.session.title = args.into();
                    live.changed();
                    self.publish();
                }
            }
            "/clear" | "/undo" | "/retry" | "/compact" | "/compress" => {
                bare(&command, args)?;
                self.command_idle()?;
                match command.as_str() {
                    "/clear" => {
                        self.clear_history()?;
                    }
                    "/undo" => {
                        self.undo_turn()?;
                    }
                    "/retry" => {
                        let message_id = self
                            .model
                            .current
                            .as_ref()
                            .and_then(|d| d.messages.last())
                            .ok_or("No turn to retry")?
                            .id
                            .clone();
                        self.retry_turn(message_id)?;
                    }
                    _ => {
                        let id = self
                            .model
                            .current
                            .as_ref()
                            .ok_or("No session selected")?
                            .session
                            .id
                            .clone();
                        self.dispatch("compact_session", json!({"sessionId":id}));
                    }
                }
            }
            "/stop" => {
                bare(&command, args)?;
                self.abort()?;
            }
            "/review" | "/reflect" | "/refine" => {
                bare(&command, args)?;
                self.command_idle()?;
                let data = self.model.current.as_ref().ok_or("No session selected")?;
                let last = data
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "assistant")
                    .ok_or("Complete a conversational turn before reviewing")?;
                if last.is_streaming == Some(true)
                    || matches!(last.run_status.as_deref(), Some("failed" | "cancelled"))
                {
                    return Err(
                        "Only a successfully completed conversational turn can be reviewed".into(),
                    );
                }
                let id = data.session.id.clone();
                let provider = self.command_provider()?;
                self.dispatch(
                    "review_session",
                    json!({"sessionId":id,"provider":provider}),
                );
            }
            "/title" => {
                if args.is_empty() {
                    return Err("Usage: /title <name>".into());
                }
                self.command_idle()?;
                let id = self
                    .model
                    .current
                    .as_ref()
                    .ok_or("No session selected")?
                    .session
                    .id
                    .clone();
                // A new, unsaved session has no native metadata record yet.
                let live = self.live.get_mut(&id).ok_or("Session unavailable")?;
                if live.window.is_none() {
                    live.data.session.title = args.into();
                    live.changed();
                    self.publish();
                } else {
                    self.dispatch(
                        "rename_session_summary",
                        json!({"sessionId":id,"title":args}),
                    );
                }
            }
            "/pin" => {
                bare(&command, args)?;
                self.command_idle()?;
                let id = self
                    .model
                    .current
                    .as_ref()
                    .ok_or("No session selected")?
                    .session
                    .id
                    .clone();
                let live = self.live.get_mut(&id).ok_or("Session unavailable")?;
                if live.window.is_none() {
                    live.data.session.pinned = !live.data.session.pinned;
                    live.data.session.pinned_at = live.data.session.pinned.then(crate::model::now);
                    live.changed();
                    self.publish();
                } else {
                    self.dispatch("toggle_pin_session", json!({"sessionId":id}));
                }
            }
            "/export" | "/save" => {
                self.command_idle()?;
                let default = if command == "/save" {
                    "json"
                } else {
                    "markdown"
                };
                let (first, rest) = parts(args);
                let format = match first.as_str() {
                    "md" | "markdown" => Some("markdown"),
                    "json" => Some("json"),
                    "html" => Some("html"),
                    _ => None,
                };
                let (format, destination) = format.map_or((default, args), |f| (f, rest));
                // The UI owns the save dialog and confirmation; no direct path write.
                return Ok(ui("export", &json!({"sessionId":self.model.current.as_ref().map(|d| &d.session.id),"format":format,"destination":destination}).to_string()));
            }
            "/model" => return Ok(ui("models", args)),
            "/settings" | "/history" | "/artifacts" | "/tools" | "/memory" | "/terminal"
            | "/git" | "/tasks" | "/skills" => {
                bare(&command, args)?;
                return Ok(match command.as_str() {
                    "/terminal" => ui("shell", ""),
                    "/skills" => ui("tools", "skills"),
                    _ => ui(&command[1..], ""),
                });
            }
            "/init" => {
                bare(&command, args)?;
                // Initialization writes workspace instructions; the memory panel
                // provides its existing explicit action rather than silently writing.
                return Ok(ui("memory", "init"));
            }
            _ => {
                let options = prompt_options(text)?;
                return Ok(InputAction::Send {
                    display: text.into(),
                    api: options.api_prompt,
                    execution_mode: options.execution_mode,
                });
            }
        }
        self.model.draft.clear();
        Ok(InputAction::Handled)
    }

    fn command_idle(&self) -> Result<(), String> {
        if self.model.loading || self.model.preview.is_some() {
            return Err("Return to latest history before changing a conversation".into());
        }
        let data = self.model.current.as_ref().ok_or("No session selected")?;
        let live = self
            .live
            .get(&data.session.id)
            .ok_or("Session unavailable")?;
        if live.protected() {
            return Err("Stop the active run and wait for session operations".into());
        }
        Ok(())
    }
    fn command_provider(&self) -> Result<ProviderConfig, String> {
        let over = self
            .model
            .current
            .as_ref()
            .and_then(|d| d.model_override.as_ref());
        let id = over
            .and_then(|o| o.provider_id.as_deref())
            .unwrap_or(&self.model.settings.active_provider_id);
        let mut provider = self
            .model
            .settings
            .providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("Select a configured provider")?;
        if let Some(model) = over.and_then(|o| o.model.as_ref()) {
            provider.model = model.clone();
        }
        Ok(provider)
    }
}
