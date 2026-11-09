//! Explicit history replacement: hydrate canonical payloads, trim, then compare
//! and swap under the native session lock. Never replace history with a viewport.
use crate::{
    model::{now, LiveSession, SaveRecovery},
    Controller,
};
use aether_core::{agent::*, service::Backend};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) const HISTORY_COMMAND: &str = "controller:history";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryAction {
    UpdateContent {
        message_id: String,
        expected: String,
        new: String,
    },
    Undo,
    Clear,
    Rollback {
        message_id: String,
    },
    Retry {
        message_id: String,
    },
    Edit {
        message_id: String,
        text: String,
    },
}

/// Owns canonical bytes: source files need not still exist when replaying.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayPrompt {
    pub content: String,
    pub attachments: Option<Vec<AttachedContext>>,
    pub image_inputs: Option<Vec<ImageInput>>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    action: HistoryAction,
    data: SessionData,
    window: Option<SessionWindow>,
    identity: String,
    version: u64,
    run_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Outcome {
    data: SessionData,
    window: Option<SessionWindow>,
    replay: Option<ReplayPrompt>,
    resend: bool,
    canonical_committed: bool,
    #[serde(default)]
    preserve_composer: bool,
    acknowledgment_error: Option<String>,
    #[serde(default)]
    recovery_proof: Option<storage::CommittedSaveProof>,
    identity: String,
    version: u64,
    run_id: Option<String>,
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| e.to_string())
}

impl Controller {
    pub fn undo(&mut self) -> Result<u64, String> {
        self.undo_turn()
    }
    pub fn undo_turn(&mut self) -> Result<u64, String> {
        self.history(HistoryAction::Undo)
    }
    pub fn clear_history(&mut self) -> Result<u64, String> {
        self.history(HistoryAction::Clear)
    }
    pub fn rollback_turn(&mut self, message_id: impl Into<String>) -> Result<u64, String> {
        self.history(HistoryAction::Rollback {
            message_id: message_id.into(),
        })
    }
    pub fn retry_turn(&mut self, message_id: impl Into<String>) -> Result<u64, String> {
        self.history(HistoryAction::Retry {
            message_id: message_id.into(),
        })
    }
    pub fn edit_and_resend(
        &mut self,
        message_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<u64, String> {
        self.history(HistoryAction::Edit {
            message_id: message_id.into(),
            text: text.into(),
        })
    }
    pub(crate) fn dispatch_history_action(
        &mut self,
        command: &str,
        args: &Value,
    ) -> Result<u64, String> {
        fn argument<'a>(
            args: &'a Value,
            camel: &str,
            snake: &str,
        ) -> Result<Option<&'a str>, String> {
            if camel != snake && args.get(camel).is_some() && args.get(snake).is_some() {
                return Err(format!("Ambiguous argument: {camel}"));
            }
            args.get(camel)
                .or_else(|| args.get(snake))
                .map(|v| {
                    v.as_str()
                        .ok_or_else(|| format!("Invalid argument: {camel}"))
                })
                .transpose()
        }
        let id = argument(args, "sessionId", "session_id")?
            .ok_or("History action requires sessionId")?;
        if self.selected.as_deref() != Some(id) {
            return Err("History sessionId does not match selected session".into());
        }
        let message = argument(args, "messageId", "message_id")?.unwrap_or("");
        match command {
            "undo_turn" => {
                if !message.is_empty()
                    && !self
                        .live
                        .get(id)
                        .is_some_and(|l| l.data.messages.iter().any(|m| m.id == message))
                {
                    return Err("Message is not in this session".into());
                }
                self.undo_turn()
            }
            "clear_history" => self.clear_history(),
            "rollback_turn" => self.rollback_turn(message),
            "retry_turn" => self.retry_turn(message),
            "edit_and_resend" => self.edit_and_resend(
                message,
                argument(args, "text", "text")?.ok_or("Edit requires text")?,
            ),
            _ => Err("Unknown history action".into()),
        }
    }
    pub fn history(&mut self, action: HistoryAction) -> Result<u64, String> {
        if self.is_shutting_down() {
            return Err("Controller is shutting down".into());
        }
        if self.model.loading || self.model.preview.is_some() {
            return Err("Return to latest history before changing a conversation".into());
        }
        let id = self.selected.clone().ok_or("No session selected")?;
        let live = self.live.get_mut(&id).ok_or("Session unavailable")?;
        if live.protected() || self.refresh.contains_key(&id) {
            return Err("Stop the active run and wait for session operations".into());
        }
        if let HistoryAction::UpdateContent {
            message_id,
            expected,
            new,
        } = &action
        {
            if live.window.is_none() {
                return Err("Save the session before editing message content".into());
            }
            crate::message_edit::replacement(&live.data, message_id, expected, new)?;
        }
        let request = Request {
            action,
            data: live.data.clone(),
            window: live.window.clone(),
            identity: live.identity.clone(),
            version: live.version,
            run_id: live.run_id.clone(),
        };
        let args = serde_json::to_value(request).map_err(|e| e.to_string())?;
        live.persistence_busy = true;
        let op = self.issue_history(args, id);
        self.publish();
        Ok(op)
    }

    pub(crate) fn complete_history(
        &mut self,
        opid: u64,
        id: Option<&str>,
        generation: u64,
        result: Result<Value, String>,
    ) {
        let Some(id) = id else {
            return;
        };
        let origin = self.selected.as_deref() == Some(id) && self.generation == generation;
        let Some(live) = self.live.get_mut(id) else {
            return;
        };
        live.persistence_busy = false;
        let mut resend = None;
        let result = result.and_then(decode::<Outcome>).and_then(|outcome| {
            if outcome.data.session.id != id
                || outcome.identity != live.identity
                || outcome.version != live.version
                || outcome.run_id != live.run_id
            {
                return Err("Stale history acknowledgment; origin snapshot changed".into());
            }
            if let Some(error) = outcome.acknowledgment_error {
                // Native may already have written the trimmed data. Keep it dirty,
                // not the old history, and never claim an acknowledged baseline.
                live.data = outcome.data;
                live.changed();
                if let Some(proof) = outcome.recovery_proof {
                    live.recovery = Some(SaveRecovery::CanonicalCommitted {
                        proof,
                        version: live.version,
                    });
                }
                // Keep replay material available, but never automatically resend
                // a provider turn before the canonical cursor is acknowledged.
                if origin && !outcome.preserve_composer {
                    if let Some(replay) = outcome.replay {
                        self.model.draft = replay.content;
                        self.model.attachments = replay.attachments.unwrap_or_default();
                        self.model.replay_image_inputs = replay.image_inputs.unwrap_or_default();
                    }
                }
                live.failure_phase = Some("final_save".into());
                return Err(error);
            }
            if !outcome.canonical_committed {
                return Err("History was not committed".into());
            }
            let window = outcome
                .window
                .ok_or("History acknowledgment has no cursor")?;
            let selected_artifact = live
                .selected_artifact
                .clone()
                .filter(|id| outcome.data.artifacts.iter().any(|a| &a.id == id));
            let summary = outcome.data.session.clone();
            let mut next = LiveSession::new(outcome.data, Some(window));
            next.version = live.version + 1;
            next.selected_artifact = selected_artifact;
            *live = next;
            Ok((
                summary,
                outcome.replay,
                outcome.resend,
                outcome.preserve_composer,
            ))
        });
        match result {
            Ok((summary, replay, automatic, preserve_composer)) => {
                self.refresh.remove(id);
                self.catalog(summary);
                if origin {
                    self.model.error = None;
                    if !preserve_composer {
                        self.model.replay_image_inputs.clear();
                        if let Some(replay) = replay {
                            self.model.draft = replay.content;
                            self.model.attachments = replay.attachments.unwrap_or_default();
                            self.model.replay_image_inputs =
                                replay.image_inputs.unwrap_or_default();
                            if automatic {
                                resend = Some(self.model.draft.clone());
                            }
                        } else {
                            self.model.draft.clear();
                            self.model.attachments.clear();
                        }
                    }
                    self.published = None;
                    self.publish();
                }
            }
            Err(error) => {
                if let Some(live) = self.live.get_mut(id) {
                    live.error = Some(error.clone());
                }
                // Structured post-write failures must not escape as successful
                // command results merely because the runtime returned an object.
                if let Some(result) = self.model.results.get_mut(&opid) {
                    result.value = None;
                    result.error = Some(error.clone());
                    if let Some(last) = &mut self.model.last_command {
                        if last.id == opid {
                            *last = result.clone();
                        }
                    }
                }
                if origin {
                    self.model.error = Some(error);
                }
            }
        }
        if let Some(prompt) = resend {
            if let Err(error) = self.send_prompt(prompt) {
                self.model.error = Some(error);
            }
        }
    }
}

/// Runs on the runtime. The final CAS catches mutations between hydration and save.
pub(crate) async fn execute(backend: &Backend, args: Value) -> Result<Value, String> {
    let request: Request = decode(args)?;
    let id = request.data.session.id.clone();
    let mut full = if let Some(window) = &request.window {
        if window.end.is_some() {
            return Err("History preview is read-only".into());
        }
        backend
            .dispatch(
                "load_session_page",
                json!({"sessionId":id,"before":0,"limit":1,"revision":window.revision}),
            )
            .await?;
        let full: SessionData = decode(
            backend
                .dispatch("load_session", json!({"sessionId":id}))
                .await?,
        )?;
        validate_suffix(&full, &request.data, window)?;
        full
    } else {
        request.data
    };
    let expected = request.window.map(|w| SessionWindow {
        end: None,
        offset: 0,
        total: full.messages.len(),
        revision: w.revision,
        first_message_id: full.messages.first().map(|m| m.id.clone()),
    });
    let preserve_composer = matches!(&request.action, HistoryAction::UpdateContent { .. });
    let resend = matches!(
        request.action,
        HistoryAction::Retry { .. } | HistoryAction::Edit { .. }
    );
    let replay = match request.action {
        HistoryAction::UpdateContent {
            message_id,
            expected,
            new,
        } => {
            let (index, message) =
                crate::message_edit::replacement(&full, &message_id, &expected, &new)?;
            full.messages[index] = message;
            None
        }
        HistoryAction::Clear => {
            full.messages.clear();
            full.artifacts.clear();
            None
        }
        HistoryAction::Rollback { message_id } => {
            let index = target(&full, &message_id)?;
            prune(&mut full, index + 1, true);
            None
        }
        action => {
            let (index, text) = match action {
                HistoryAction::Undo => (
                    full.messages
                        .iter()
                        .rposition(|m| m.role == "user")
                        .ok_or("No user turn to undo")?,
                    None,
                ),
                HistoryAction::Retry { message_id } => {
                    let at = target(&full, &message_id)?;
                    (
                        full.messages[..=at]
                            .iter()
                            .rposition(|m| m.role == "user")
                            .ok_or("No user prompt for this turn")?,
                        None,
                    )
                }
                HistoryAction::Edit { message_id, text } => {
                    let at = target(&full, &message_id)?;
                    if full.messages[at].role != "user" {
                        return Err("Only user prompts can be edited".into());
                    }
                    if text.trim().is_empty()
                        && full.messages[at]
                            .attachments
                            .as_ref()
                            .is_none_or(Vec::is_empty)
                    {
                        return Err("Message is empty".into());
                    }
                    (at, Some(text))
                }
                _ => unreachable!(),
            };
            let user = &full.messages[index];
            let replay = ReplayPrompt {
                content: text.unwrap_or_else(|| user.content.clone()),
                attachments: user.attachments.clone(),
                image_inputs: user.image_inputs.clone(),
            };
            prune(&mut full, index, false);
            Some(replay)
        }
    };
    full.session.updated_at = now();
    full.session.message_count = full.messages.len();
    let (data, window, acknowledgment_error, recovery_proof) = match backend
        .dispatch(
            "replace_session_checked",
            json!({"data":full,"expectedWindow":expected}),
        )
        .await
    {
        Ok(value) => {
            let page: SessionPage = decode(value)?;
            (page.data, Some(page.window), None, None)
        }
        Err(error) if storage::CommittedSaveError::parse(&error).is_some() => {
            let receipt =
                storage::CommittedSaveError::parse(&error).expect("typed receipt checked");
            (full, None, Some(error), Some(receipt.proof))
        }
        Err(error) => return Err(error),
    };
    let canonical_committed = true;
    serde_json::to_value(Outcome {
        data,
        window,
        replay,
        resend,
        canonical_committed,
        preserve_composer,
        acknowledgment_error,
        recovery_proof,
        identity: request.identity,
        version: request.version,
        run_id: request.run_id,
    })
    .map_err(|e| e.to_string())
}
fn target(data: &SessionData, id: &str) -> Result<usize, String> {
    if id.is_empty() {
        return Err("Message ID is required".into());
    }
    let mut found = data.messages.iter().enumerate().filter(|(_, m)| m.id == id);
    let at = found.next().ok_or("Message is not in this session")?.0;
    if found.next().is_some() {
        return Err("Ambiguous message ID".into());
    }
    Ok(at)
}
fn validate_suffix(
    full: &SessionData,
    view: &SessionData,
    window: &SessionWindow,
) -> Result<(), String> {
    if full.session.id != view.session.id
        || full.messages.len() != window.total
        || window.offset > window.total
        || view.messages.len() != window.total - window.offset
        || window.first_message_id != view.messages.first().map(|m| m.id.clone())
    {
        return Err("Session history changed; reload before editing".into());
    }
    for (canonical, visible) in full.messages[window.offset..].iter().zip(&view.messages) {
        let mut restored = visible.clone();
        if restored.image_inputs.is_none() {
            restored.image_inputs = canonical.image_inputs.clone();
        }
        if restored.provider_messages.is_none() {
            restored.provider_messages = canonical.provider_messages.clone();
        }
        restored.has_image_inputs = canonical.has_image_inputs;
        restored.has_provider_messages = canonical.has_provider_messages;
        if json!(restored) != json!(canonical) {
            return Err("Session history changed; reload before editing".into());
        }
    }
    Ok(())
}
fn artifact_ids(messages: &[ChatMessage]) -> HashSet<&str> {
    messages
        .iter()
        .flat_map(|m| {
            m.tool_calls.iter().flatten().chain(
                m.segments
                    .iter()
                    .flatten()
                    .flat_map(|s| s.tool_calls.iter().flatten()),
            )
        })
        .filter_map(|t| t.artifact_id.as_deref())
        .collect()
}
fn prune(data: &mut SessionData, keep: usize, inclusive: bool) {
    let timestamp = if inclusive {
        data.messages[keep - 1].timestamp
    } else {
        data.messages[keep].timestamp
    };
    let retained = artifact_ids(&data.messages[..keep]);
    let removed = artifact_ids(&data.messages[keep..]);
    data.artifacts.retain(|a| {
        retained.contains(a.id.as_str())
            || (!removed.contains(a.id.as_str())
                && if inclusive {
                    a.timestamp <= timestamp
                } else {
                    a.timestamp < timestamp
                })
    });
    data.messages.truncate(keep);
}
