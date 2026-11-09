//! Runtime-only, per-session composer queues. Never part of Model or canonical history.
use crate::{
    app::AppState,
    native,
    widgets::{button, caption},
};
use aether_core::agent::{AgentSettings, AttachedContext, ImageInput, ProviderConfig};
use aether_state::commands::{SendOptions, prompt_options};
use rune::{El, ScrollSpec, TextEdit, TextStyle, Tx, col, row, text_input};
use rune_kit::Kit;
use std::collections::{HashMap, VecDeque};
pub const MAX_ITEMS: usize = 20;
pub const MAX_BYTES: usize = 5 * 1024 * 1024;
#[derive(Clone)]
pub struct QueuedInstruction {
    pub id: String,
    pub prompt: String,
    pub attachments: Vec<AttachedContext>,
    pub image_inputs: Vec<ImageInput>,
    pub workspace: String,
    pub settings: AgentSettings,
    pub providers: Vec<ProviderConfig>,
    pub options: SendOptions,
}
#[derive(Default)]
pub struct SessionQueue {
    pub items: VecDeque<QueuedInstruction>,
    pub paused: bool,
}
#[derive(Default)]
pub struct QueueState {
    pub sessions: HashMap<String, SessionQueue>,
    pub edit: Option<QueueEdit>,
}
pub struct QueueEdit {
    pub session: String,
    pub id: String,
    pub text: TextEdit,
}
impl QueuedInstruction {
    fn bytes(&self) -> usize {
        let mut counter = CountingWriter::default();
        if serde_json::to_writer(
            &mut counter,
            &(
                &self.id,
                &self.prompt,
                &self.attachments,
                &self.image_inputs,
                &self.workspace,
                &self.settings,
                &self.providers,
                &self.options.api_prompt,
                &self.options.execution_mode,
            ),
        )
        .is_err()
        {
            return usize::MAX;
        }
        counter.0
    }
}
#[derive(Default)]
struct CountingWriter(usize);
impl std::io::Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
// Session keys are retained too. Six bytes per UTF-8 byte conservatively
// covers JSON escaping and the small per-entry container overhead.
fn entry_bytes(session: &str, item: &QueuedInstruction) -> usize {
    item.bytes()
        .saturating_add(session.len().saturating_mul(6).saturating_add(4))
}
impl QueueState {
    pub fn total(&self) -> usize {
        self.sessions.values().map(|q| q.items.len()).sum()
    }
    pub fn bytes(&self) -> usize {
        self.sessions
            .iter()
            .flat_map(|(session, q)| q.items.iter().map(move |item| entry_bytes(session, item)))
            .fold(0, usize::saturating_add)
    }
    pub fn push(&mut self, session: String, item: QueuedInstruction) -> Result<(), String> {
        if self.total() >= MAX_ITEMS
            || self.bytes().saturating_add(entry_bytes(&session, &item)) > MAX_BYTES
        {
            return Err("Queue limit reached (20 messages / 5 MiB total); draft retained".into());
        }
        self.sessions
            .entry(session)
            .or_default()
            .items
            .push_back(item);
        Ok(())
    }
    pub fn remove(&mut self, session: &str, id: &str) {
        if let Some(q) = self.sessions.get_mut(session) {
            q.items.retain(|i| i.id != id);
        }
        self.sessions.retain(|_, q| !q.items.is_empty());
    }
}
pub fn enqueue(s: &mut AppState) -> Result<(), String> {
    let model = s.controller.model();
    if model.preview.is_some() || model.loading {
        return Err("Return to latest before queuing".into());
    }
    let prompt = s.shell.composer.text.clone();
    if prompt.trim().is_empty() && model.attachments.is_empty() {
        return Err("Message is empty".into());
    }
    let id = model
        .current
        .as_ref()
        .ok_or("No session selected")?
        .session
        .id
        .clone();
    let options = prompt_options(&prompt)?;
    let mut settings = model.settings.clone();
    if let Some(over) = model
        .current
        .as_ref()
        .and_then(|d| d.model_override.as_ref())
    {
        if let Some(v) = over.thinking {
            settings.thinking = v;
        }
        if let Some(v) = over.reasoning_effort {
            settings.reasoning_effort = v;
        }
    }
    if let Some(v) = options.execution_mode {
        settings.execution_mode = v;
    }
    let mut providers = vec![s.candidate_a().ok_or("Select a configured provider")?];
    if s.shell.council && !prompt.trim_start().starts_with('/') {
        providers.push(s.candidate_b().ok_or("Configure Council candidate B")?);
    }
    let item = QueuedInstruction {
        id: uuid::Uuid::new_v4().to_string(),
        prompt,
        attachments: model.attachments.clone(),
        image_inputs: model.replay_image_inputs.clone(),
        workspace: model.workspace.clone(),
        settings,
        providers,
        options,
    };
    if item
        .attachments
        .iter()
        .filter(|a| native::image_candidate(a))
        .any(|a| !item.image_inputs.iter().any(|i| i.source_path == a.path))
    {
        return Err(
            "Prepare images before queuing; remove and reattach the image if necessary".into(),
        );
    }
    s.shell.queue.push(id, item)?;
    s.controller.model_mut().draft.clear();
    s.controller.model_mut().attachments.clear();
    s.controller.model_mut().replay_image_inputs.clear();
    s.shell.composer.clear();
    Ok(())
}
/// Only the selected origin can launch: Controller::send is intentionally selection-bound.
pub fn sync(s: &mut AppState, tx: &mut Tx) -> bool {
    if s.shell.lifecycle != crate::lifecycle::Lifecycle::Open
        || s.ui.panel.is_some()
        || s.shell.export.confirmation.is_some()
        || s.shell.native.preview.is_some()
        || s.shell.queue.edit.is_some()
        || s.shell.native.preparing()
        || !s.controller.ready_to_send()
    {
        return false;
    }
    let Some(id) = s
        .controller
        .model()
        .current
        .as_ref()
        .map(|d| d.session.id.clone())
    else {
        return false;
    };
    let Some(q) = s.shell.queue.sessions.get(&id) else {
        return false;
    };
    if q.paused {
        return false;
    }
    let Some(item) = q.items.front().cloned() else {
        return false;
    };
    if item.workspace != s.controller.model().workspace {
        s.shell.queue.sessions.get_mut(&id).unwrap().paused = true;
        s.set_error("Queue paused: return to its original workspace before resuming");
        return true;
    }
    let model = s.controller.model_mut();
    let draft = std::mem::take(&mut model.draft);
    let attachments = std::mem::replace(&mut model.attachments, item.attachments.clone());
    let images = std::mem::replace(&mut model.replay_image_inputs, item.image_inputs.clone());
    let result = s.controller.send_captured(
        item.prompt.clone(),
        item.options.clone(),
        item.settings.clone(),
        item.providers.clone(),
    );
    let model = s.controller.model_mut();
    model.draft = draft;
    model.attachments = attachments;
    model.replay_image_inputs = images;
    match result {
        Ok(()) => {
            s.shell.queue.remove(&id, &item.id);
            tx.scroll_to_bottom("chat-viewport");
        }
        Err(error) => {
            s.shell.queue.sessions.get_mut(&id).unwrap().paused = true;
            s.set_error(format!("Queue paused: {error}"));
        }
    }
    true
}
fn editable(s: &AppState) -> bool {
    s.controller.model().preview.is_none() && !s.controller.model().loading
}
pub fn save_edit(s: &mut AppState) {
    let Some(edit) = s.shell.queue.edit.as_ref() else {
        return;
    };
    if !editable(s) {
        return;
    }
    let session = edit.session.clone();
    let id = edit.id.clone();
    let prompt = edit.text.text.clone();
    let Some(old) = s
        .shell
        .queue
        .sessions
        .get(&session)
        .and_then(|q| q.items.iter().find(|i| i.id == id))
        .cloned()
    else {
        s.shell.queue.edit = None;
        return;
    };
    if prompt.trim().is_empty() && old.attachments.is_empty() {
        s.set_error("Queued message is empty");
        return;
    }
    if crate::composer::is_local(&prompt) {
        s.set_error("Local commands run from the composer, not the message queue");
        return;
    }
    let Ok(options) = prompt_options(&prompt) else {
        s.set_error("Command requires a prompt");
        return;
    };
    let mut updated = old.clone();
    updated.prompt = prompt;
    updated.options = options;
    if s.shell
        .queue
        .bytes()
        .saturating_sub(old.bytes())
        .saturating_add(updated.bytes())
        > MAX_BYTES
    {
        s.set_error("Queue exceeds 5 MiB; edit retained");
        return;
    }
    let q = s.shell.queue.sessions.get_mut(&session).unwrap();
    *q.items.iter_mut().find(|i| i.id == id).unwrap() = updated;
    s.shell.queue.edit = None;
}
pub fn render(s: &AppState, kit: &Kit) -> El {
    let selected = s
        .controller
        .model()
        .current
        .as_ref()
        .map(|d| d.session.id.as_str());
    let disabled = !editable(s);
    let mut list = col()
        .key("message-queue")
        .w_full()
        .gap(4.0)
        .max_h(155.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    let mut queues: Vec<_> = s
        .shell
        .queue
        .sessions
        .iter()
        .filter(|(_, q)| !q.items.is_empty())
        .collect();
    queues.sort_by_key(|(id, _)| *id);
    for (session, q) in queues {
        let offscreen = selected != Some(session.as_str());
        let title = if offscreen {
            "Paused until selected"
        } else if q.paused {
            "Queue paused"
        } else {
            "Queued - runs after save acknowledgment"
        };
        let owner = session.clone();
        let pause_key = if offscreen {
            format!("queue-pause-{session}")
        } else {
            "queue-pause".into()
        };
        list = list.child(
            row()
                .gap(6.0)
                .child(
                    caption(format!("{title} ({}) - {}", q.items.len(), session), kit)
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(
                    button(
                        &pause_key,
                        if q.paused { "Resume" } else { "Pause" },
                        kit,
                        move |s, tx| {
                            if let Some(q) = s.shell.queue.sessions.get_mut(&owner) {
                                q.paused = !q.paused;
                            }
                            tx.wake_after(std::time::Duration::ZERO);
                        },
                    )
                    .disabled(disabled),
                ),
        );
        for item in &q.items {
            let owner = session.clone();
            let id = item.id.clone();
            let edit_owner = session.clone();
            let edit_id = item.id.clone();
            let summary: String = item.prompt.chars().take(100).collect();
            list = list.child(
                row()
                    .gap(4.0)
                    .child(
                        caption(
                            format!(
                                "{} [{} attachment(s), {}]",
                                summary,
                                item.attachments.len(),
                                if item.providers.len() == 2 {
                                    "Council"
                                } else {
                                    "single model"
                                }
                            ),
                            kit,
                        )
                        .grow(1.0)
                        .min_w(0.0),
                    )
                    .child(
                        button(
                            &format!("queue-edit-{}", item.id),
                            "Edit",
                            kit,
                            move |s, _| {
                                if let Some(item) = s
                                    .shell
                                    .queue
                                    .sessions
                                    .get(&edit_owner)
                                    .and_then(|q| q.items.iter().find(|i| i.id == edit_id))
                                {
                                    s.shell.queue.edit = Some(QueueEdit {
                                        session: edit_owner.clone(),
                                        id: edit_id.clone(),
                                        text: TextEdit::new(&item.prompt),
                                    });
                                }
                            },
                        )
                        .disabled(disabled),
                    )
                    .child(
                        button(
                            &format!("queue-remove-{}", item.id),
                            "Remove",
                            kit,
                            move |s, _| s.shell.queue.remove(&owner, &id),
                        )
                        .disabled(disabled),
                    ),
            );
        }
    }
    list
}
pub fn modal(s: &AppState, kit: &Kit) -> Option<El> {
    let edit = s.shell.queue.edit.as_ref()?;
    Some(
        col()
            .key("queue-edit-modal")
            .role("dialog")
            .label("Edit queued instruction")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(24.0)
            .gap(12.0)
            .fill(kit.c.bg_deep)
            .modal(false, |s: &mut AppState| s.shell.queue.edit = None)
            .focus_return("composer")
            .child(caption(
                "Edit queued prompt (attachments and captured model options are retained)",
                kit,
            ))
            .child(
                text_input(&edit.text, TextStyle::new(14.0, 400.0))
                    .key("queue-edit-input")
                    .label("Queued message")
                    .w_full()
                    .grow(1.0)
                    .min_h(100.0)
                    .fill(kit.c.layer_01)
                    .padding(12.0)
                    .on_edit(|s: &mut AppState| &mut s.shell.queue.edit.as_mut().unwrap().text),
            )
            .child(
                row()
                    .gap(8.0)
                    .child(
                        button("queue-edit-save", "Save queued edit", kit, |s, tx| {
                            save_edit(s);
                            tx.wake_after(std::time::Duration::ZERO);
                        })
                        .disabled(!editable(s)),
                    )
                    .child(button("queue-edit-cancel", "Cancel", kit, |s, _| {
                        s.shell.queue.edit = None
                    })),
            )
            .child(caption(
                s.shell
                    .error
                    .as_deref()
                    .unwrap_or("Editing never replaces the current composer draft"),
                kit,
            )),
    )
}
