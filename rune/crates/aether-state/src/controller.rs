use crate::{
    events::{BackendEvent, EventQueue, Waker},
    model::{new_data, now, LiveSession, SaveRecovery},
    reducer, CommandResult, Model, RunPhase, RunStatus,
};
use aether_core::{agent::*, service::Backend};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, mpsc::Receiver, Arc},
    time::{Duration, Instant},
};
use tokio::runtime::Runtime;

const PAGE_SIZE: usize = 50;
const CACHE_SIZE: usize = 6; // clean, inactive sessions only
const CACHE_BYTES: usize = 32 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Completion {
    opid: u64,
    command: String,
    session_id: Option<String>,
    pub(crate) generation: u64,
    result: Result<Value, String>,
    #[serde(default)]
    failure_phase: Option<String>,
}
struct CapturedSend {
    settings: AgentSettings,
    providers: Vec<ProviderConfig>,
}
struct Pending {
    args: Value,
    kind: Operation,
}
enum Operation {
    Generic,
    Mutation {
        origin: Option<(String, u64, Option<String>)>,
        external_page: bool,
    },
    Page {
        before: Option<usize>,
    },
    Initial {
        requests: Vec<AgentRunRequest>,
        previous: SessionData,
        draft: String,
        attachments: Vec<AttachedContext>,
        run_id: String,
        version: u64,
    },
    Started {
        run_id: String,
    },
    Save {
        data: SessionData,
        version: u64,
        run_id: Option<String>,
    },
    RecoverSave {
        proof: storage::CommittedSaveProof,
        version: u64,
    },
    Checkpoint {
        run_id: String,
        version: u64,
        sequence: u64,
    },
    Refresh {
        baseline: SessionData,
    },
}
/// UI-thread controller. The parent configures its native profile before
/// construction. No constructor changes the environment or saved settings.
pub struct Controller {
    pub backend: Arc<Backend>,
    pub runtime: Arc<Runtime>,
    pub model: Model,
    queue: EventQueue,
    receiver: Receiver<BackendEvent>,
    pub(crate) live: HashMap<String, LiveSession>,
    pub(crate) selected: Option<String>,
    pub(crate) generation: u64,
    next_operation: u64,
    pending: HashMap<u64, Pending>,
    pub(crate) published: Option<(String, u64)>,
    pub(crate) refresh: HashMap<String, Option<String>>,
    clock: u64,
    mutations: HashMap<String, u64>,
    shutdown_started: Option<Instant>,
    shutdown_fallback: bool,
}
impl Controller {
    pub fn new() -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self::with_runtime(Arc::new(runtime)))
    }
    pub fn new_with_workspace(workspace: String) -> Result<Self, String> {
        let workspace = storage::canonical_workspace(&workspace)?;
        let mut controller = Self::new()?;
        *controller
            .backend
            .workspace
            .lock()
            .map_err(|_| "Workspace unavailable")? = workspace.clone();
        controller.model.workspace = workspace;
        if let Some(id) = &controller.selected {
            controller
                .live
                .get_mut(id)
                .unwrap()
                .data
                .session
                .workspace_cwd = Some(controller.model.workspace.clone());
        }
        controller.published = None;
        controller.publish();
        Ok(controller)
    }
    pub fn with_runtime(runtime: Arc<Runtime>) -> Self {
        Self::with_backend(runtime, |host| Arc::new(Backend::new(host)))
    }
    /// Factory injection preserves the queue's host, including all run events.
    pub fn with_backend(
        runtime: Arc<Runtime>,
        build: impl FnOnce(AgentHost) -> Arc<Backend>,
    ) -> Self {
        let (queue, receiver) = EventQueue::new();
        let backend = build(AgentHost::new(queue.clone()));
        let workspace = backend
            .workspace
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut controller = Self {
            backend,
            runtime,
            model: Model {
                workspace,
                ..Model::default()
            },
            queue,
            receiver,
            live: HashMap::new(),
            selected: None,
            generation: 0,
            next_operation: 0,
            pending: HashMap::new(),
            published: None,
            refresh: HashMap::new(),
            clock: 0,
            mutations: HashMap::new(),
            shutdown_started: None,
            shutdown_fallback: false,
        };
        controller.new_session();
        controller
    }
    /// Share an injected backend's runner/workspace, replacing only its host.
    /// Reusing an unrelated host would silently lose all streaming events.
    pub fn from_backend(runtime: Arc<Runtime>, backend: Arc<Backend>) -> Self {
        Self::with_backend(runtime, |host| {
            Arc::new(Backend {
                runner: backend.runner.clone(),
                workspace: backend.workspace.clone(),
                updater: backend.updater.clone(),
                host,
            })
        })
    }
    /// Queue hosts must wait for terminal persistence, not merely a Done event.
    pub fn ready_to_send(&self) -> bool {
        !self.is_shutting_down() && !self.model.loading && self.model.preview.is_none()
            && self.selected.as_ref().and_then(|id| self.live.get(id)).is_some_and(|live| !live.protected())
    }
    pub fn selected_run_active(&self) -> bool {
        self.selected.as_ref().and_then(|id| self.live.get(id)).is_some_and(|live| live.run_id.is_some())
    }
    /// Request-scoped captured queue options; does not persist or replace current settings.
    pub fn send_captured(&mut self, prompt: String, options: crate::commands::SendOptions, settings: AgentSettings, providers: Vec<ProviderConfig>) -> Result<(), String> {
        if !self.ready_to_send() { return Err("Wait for the selected session to finish saving".into()); }
        if !(1..=2).contains(&providers.len()) {
            return Err("Captured sends require one provider or two Council providers".into());
        }
        let council = if providers.len() == 2 { Some((providers[0].clone(), providers[1].clone())) } else { None };
        // These trusted, in-memory queue copies include credentials and already
        // resolved thinking/model options. Never install them in canonical state.
        self.begin_send_inner(prompt, council, options, Some(CapturedSend { settings, providers }))
    }
    pub fn navigation_generation(&self) -> u64 {
        self.generation
    }
    pub fn model(&self) -> &Model {
        &self.model
    }
    pub fn model_mut(&mut self) -> &mut Model {
        &mut self.model
    }
    pub fn set_waker(&self, waker: Waker) {
        self.queue.set_waker(waker);
    }
    pub fn take_result(&mut self, id: u64) -> Option<CommandResult> {
        self.model.results.remove(&id)
    }
    pub fn new_session(&mut self) {
        if self.is_shutting_down() {
            return;
        }
        self.generation += 1;
        self.model.replay_image_inputs.clear();
        self.model.loading = false;
        self.model.loading_older = false;
        self.model.preview = None;
        self.model.error = None;
        self.model.draft.clear();
        self.model.attachments.clear();
        self.model.replay_image_inputs.clear();
        let data = new_data(&self.model.workspace);
        let id = data.session.id.clone();
        self.live.insert(id.clone(), LiveSession::new(data, None));
        self.selected = Some(id);
        self.publish();
        self.evict();
    }
    pub fn open_session(&mut self, id: impl AsRef<str>) -> u64 {
        if self.is_shutting_down() {
            return self.reject(
                "load_session_page",
                Value::Null,
                Some(id.as_ref().into()),
                "Controller is shutting down".into(),
            );
        }
        let id = id.as_ref();
        if self.mutations.contains_key(id) {
            return self.reject(
                "load_session_page",
                json!({"sessionId":id}),
                Some(id.into()),
                "Session mutation awaiting acknowledgment".into(),
            );
        }
        self.generation += 1;
        self.model.replay_image_inputs.clear();
        self.model.preview = None;
        self.model.error = None;
        self.model.loading = false;
        if self.live.contains_key(id) {
            self.selected = Some(id.into());
            self.publish();
            return 0;
        }
        self.model.loading = true;
        self.issue(
            "load_session_page",
            json!({"sessionId":id,"limit":PAGE_SIZE}),
            Some(id.into()),
            Operation::Page { before: None },
        )
    }
    pub fn older(&mut self) -> Result<u64, String> {
        self.ensure_running()?;
        if self.model.preview.is_some() {
            return Err("Return to latest before loading older history".into());
        }
        let id = self.selected.clone().ok_or("No session selected")?;
        let live = self.live.get_mut(&id).ok_or("Session unavailable")?;
        let window = live.window.clone().ok_or("History is not saved yet")?;
        if window.offset == 0 {
            return Ok(0);
        }
        if live.loading_older || live.persistence_busy || live.mutation_busy.is_some() {
            return Err("Session operation in progress".into());
        }
        live.loading_older = true;
        let args = json!({"sessionId":id,"before":window.offset,"revision":window.revision,"limit":PAGE_SIZE});
        let op = self.issue(
            "load_session_page",
            args,
            Some(id),
            Operation::Page {
                before: Some(window.offset),
            },
        );
        self.publish();
        Ok(op)
    }
    pub fn close_preview(&mut self) {
        self.model.preview = None;
        self.generation += 1;
        self.model.replay_image_inputs.clear();
        self.model.loading = false;
    }
    /// Unknown commands are native errors, never implicit scripts or shell code.
    pub fn dispatch(&mut self, command: &str, args: Value) -> u64 {
        if self.is_shutting_down() {
            return self.reject(command, args, None, "Controller is shutting down".into());
        }
        let session = args
            .get("sessionId")
            .or_else(|| args.get("session_id"))
            .and_then(Value::as_str)
            .or_else(|| args.pointer("/data/session/id").and_then(Value::as_str))
            .map(str::to_owned);
        // Lifecycle workers issue these internally with typed acknowledgment state.
        // Generic dispatch must not reserve a session or bypass that ownership.
        if matches!(
            command,
            "save_session"
                | "checkpoint_session"
                | "checkpoint_session_delta"
                | "refresh_session_window"
                | "replace_session_checked"
        ) {
            return self.reject(
                command,
                args,
                session,
                format!("{command} is lifecycle-owned; use typed Controller methods instead of generic dispatch"),
            );
        }
        if let (Some(session), Some(data_id)) = (
            session.as_deref(),
            args.pointer("/data/session/id").and_then(Value::as_str),
        ) {
            if session != data_id {
                return self.reject(
                    command,
                    args,
                    None,
                    "Mutation data/sessionId mismatch".into(),
                );
            }
        }
        if matches!(
            command,
            "undo_turn" | "rollback_turn" | "retry_turn" | "edit_and_resend" | "clear_history"
        ) {
            return match self.dispatch_history_action(command, &args) {
                Ok(op) => op,
                Err(error) => self.reject(command, args, session, error),
            };
        }
        if command == crate::history::HISTORY_COMMAND || command == "recover_save_ack" {
            return self.reject(command, args, session, "Use a typed history action".into());
        }
        let mut kind = Operation::Generic;
        let mutation = matches!(
            command,
            "compact_session"
                | "delete_session"
                | "toggle_pin_session"
                | "rename_session"
                | "rename_session_summary"
                | "save_session_page"
                | "checkpoint_session_page"
        );
        if let Some(id) = &session {
            let live = self.live.get(id);
            if self.mutations.contains_key(id)
                || (mutation && live.is_some_and(LiveSession::protected))
                || (command == "load_session" && live.is_some_and(LiveSession::protected))
            {
                return self.reject(command, args, session,
                    "Session is active, dirty, paged or awaiting acknowledgment; wait for idle or retry save".into());
            }
        }
        if matches!(command, "load_session_page" | "load_session") {
            let before = args
                .get("before")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            if before.is_none() {
                self.generation += 1;
                self.model.replay_image_inputs.clear();
                self.model.loading = true;
                self.model.preview = None;
            }
            kind = Operation::Page { before };
        }
        if mutation {
            let Some(id) = session.as_ref() else {
                return self.reject(
                    command,
                    args,
                    None,
                    "Mutation requires sessionId or data.session.id".into(),
                );
            };
            let reservation = self.next_operation + 1;
            self.mutations.insert(id.clone(), reservation);
            let external_page = matches!(command, "save_session_page" | "checkpoint_session_page");
            let origin = self.live.get_mut(id).map(|live| {
                live.mutation_busy = Some(reservation);
                if external_page {
                    live.persistence_busy = true;
                }
                (live.identity.clone(), live.version, live.run_id.clone())
            });
            kind = Operation::Mutation {
                origin,
                external_page,
            };
        }
        let operation = self.issue(command, args, session, kind);
        self.publish();
        operation
    }
    pub(crate) fn issue_history(&mut self, args: Value, id: String) -> u64 {
        self.issue(
            crate::history::HISTORY_COMMAND,
            args,
            Some(id),
            Operation::Generic,
        )
    }
    fn issue(
        &mut self,
        command: &str,
        args: Value,
        session_id: Option<String>,
        kind: Operation,
    ) -> u64 {
        self.next_operation += 1;
        let id = self.next_operation;
        self.pending.insert(
            id,
            Pending {
                args: args.clone(),
                kind,
            },
        );
        self.model.pending_commands.insert(id, command.into());
        let backend = self.backend.clone();
        let queue = self.queue.clone();
        let command = command.to_owned();
        let generation = self.generation;
        self.runtime.spawn(async move {
            let result = if command == crate::history::HISTORY_COMMAND {
                crate::history::execute(&backend, args).await
            } else if command == "load_session" {
                match backend.dispatch(&command, args).await {
                    Ok(data) => {
                        let count = data["messages"].as_array().map_or(1, Vec::len);
                        backend
                            .dispatch(
                                "load_session_page",
                                json!({"sessionId":session_id,"limit":count.max(1)}),
                            )
                            .await
                            .map(|page| json!({"__controllerPage":page,"__nativeData":data}))
                    }
                    Err(error) => Err(error),
                }
            } else {
                backend.dispatch(&command, args).await
            };
            let _ = queue.emit(
                "controller:result",
                json!(Completion {
                    opid: id,
                    command,
                    session_id,
                    generation,
                    failure_phase: None,
                    result
                }),
            );
        });
        id
    }
    fn reject(
        &mut self,
        command: &str,
        args: Value,
        session_id: Option<String>,
        error: String,
    ) -> u64 {
        self.next_operation += 1;
        let id = self.next_operation;
        self.pending.insert(
            id,
            Pending {
                args,
                kind: Operation::Generic,
            },
        );
        self.model.pending_commands.insert(id, command.into());
        // Never block the UI on its own bounded queue.
        let queue = self.queue.clone();
        let event = Completion {
            opid: id,
            command: command.into(),
            session_id,
            generation: self.generation,
            result: Err(error),
            failure_phase: None,
        };
        self.runtime.spawn(async move {
            let _ = queue.emit("controller:result", json!(event));
        });
        id
    }
    pub fn send(&mut self, prompt: String) -> Result<(), String> {
        self.execute_input(prompt)
    }
    /// Literal/replayed prompts must never execute local slash commands.
    pub fn send_prompt(&mut self, prompt: String) -> Result<(), String> {
        let options = crate::commands::prompt_options(&prompt)?;
        self.send_with_options(prompt, options)
    }
    pub fn send_with_options(
        &mut self,
        prompt: String,
        options: crate::commands::SendOptions,
    ) -> Result<(), String> {
        self.begin_send(prompt, None, options)
    }
    pub fn send_with_images(
        &mut self,
        prompt: String,
        images: Option<Vec<ImageInput>>,
    ) -> Result<(), String> {
        self.model.replay_image_inputs = images.unwrap_or_default();
        self.send_prompt(prompt)
    }
    pub fn send_council(
        &mut self,
        prompt: String,
        a: ProviderConfig,
        b: ProviderConfig,
    ) -> Result<(), String> {
        self.begin_send(
            prompt,
            Some((a, b)),
            crate::commands::SendOptions::default(),
        )
    }
    fn begin_send(
        &mut self,
        prompt: String,
        council: Option<(ProviderConfig, ProviderConfig)>,
        options: crate::commands::SendOptions,
    ) -> Result<(), String> {
        self.begin_send_inner(prompt, council, options, None)
    }
    fn begin_send_inner(
        &mut self,
        prompt: String,
        council: Option<(ProviderConfig, ProviderConfig)>,
        options: crate::commands::SendOptions,
        capture: Option<CapturedSend>,
    ) -> Result<(), String> {
        self.ensure_running()?;
        if self.model.loading || self.model.preview.is_some() {
            return Err("Return to latest history before sending".into());
        }
        if prompt.trim().is_empty() && self.model.attachments.is_empty() {
            return Err("Message is empty".into());
        }
        let id = self.selected.clone().ok_or("No session selected")?;
        let live = self.live.get_mut(&id).ok_or("Session unavailable")?;
        if live.protected() {
            return Err("Session has an unfinished operation".into());
        }
        if let Some(current) = &self.model.current {
            if current.session.id == id {
                live.data.model_override = current.model_override.clone();
            }
        }
        let mut settings = capture.as_ref().map_or_else(|| self.model.settings.clone(), |c| c.settings.clone());
        if let Some(mode) = options.execution_mode {
            settings.execution_mode = mode;
        }
        // A capture is already resolved; only normal sends apply session overrides.
        let over = if capture.is_some() { None } else { live.data.model_override.as_ref() };
        let provider_id = over
            .and_then(|o| o.provider_id.as_deref())
            .unwrap_or(&settings.active_provider_id);
        let provider = settings
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .cloned();
        if let Some(over) = over {
            if let Some(thinking) = over.thinking {
                settings.thinking = thinking;
            }
            if let Some(effort) = over.reasoning_effort {
                settings.reasoning_effort = effort;
            }
        }
        let providers = if let Some(capture) = capture {
            capture.providers
        } else {
            match &council {
                Some((a, b)) => vec![a.clone(), b.clone()],
                None => {
                    let mut provider = provider.ok_or("Select a configured provider")?;
                    if let Some(model) = over.and_then(|o| o.model.as_ref()) {
                        provider.model = model.clone();
                    }
                    vec![provider]
                }
            }
        };
        let run_id = format!("run_{}", uuid::Uuid::new_v4());
        let message_id = format!("msg_assistant_{run_id}");
        let timestamp = now();
        let user: ChatMessage = decode(
            &json!({"id":format!("msg_{}",uuid::Uuid::new_v4()),"role":"user","content":prompt,"timestamp":timestamp,"attachments":self.model.attachments}),
        )?;
        let mut assistant: ChatMessage = decode(
            &json!({"id":message_id,"role":"assistant","content":"","timestamp":timestamp,"runId":run_id,"isStreaming":true,"toolCalls":[],"segments":[]}),
        )?;
        if let Some((a, b)) = council {
            assistant.council_turn = Some(CouncilTurn {
                candidate_a: CouncilCandidate {
                    provider_id: a.id,
                    model: a.model,
                    is_streaming: Some(true),
                    ..CouncilCandidate::default()
                },
                candidate_b: CouncilCandidate {
                    provider_id: b.id,
                    model: b.model,
                    is_streaming: Some(true),
                    ..CouncilCandidate::default()
                },
                chosen_winner: None,
            });
        }
        let previous = live.data.clone();
        let mut data = live.data.clone();
        if data.messages.is_empty() && live.window.as_ref().is_none_or(|w| w.offset == 0) {
            data.session.title = prompt
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(70)
                .collect();
        }
        data.messages.push(user);
        let mut requests = Vec::new();
        for provider in providers {
            let mut value = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
            value["provider"] = json!(provider);
            value["runId"] = json!(run_id);
            value["sessionId"] = json!(id);
            value["workspaceCwd"] = json!(data
                .session
                .workspace_cwd
                .as_ref()
                .unwrap_or(&self.model.workspace));
            value["messages"] = json!(data.messages);
            if let Some(api) = &options.api_prompt {
                if let Some(user) = value["messages"]
                    .as_array_mut()
                    .and_then(|messages| messages.last_mut())
                {
                    user["content"] = json!(api);
                }
            }
            requests.push(decode::<AgentRunRequest>(&value)?);
        }
        data.messages.push(assistant);
        data.session.updated_at = timestamp;
        data.session.message_count =
            live.window.as_ref().map_or(0, |w| w.offset) + data.messages.len();
        live.data = data.clone();
        live.completion_pending = None;
        live.run_id = Some(run_id.clone());
        live.message_id = Some(message_id);
        live.status = Some(RunStatus {
            run_id: run_id.clone(),
            session_id: id.clone(),
            phase: RunPhase::Preparing,
            started_at: timestamp,
            ended_at: None,
            save_failed: false,
        });
        live.failure_phase = None;
        live.recovery_draft = None;
        live.done = false;
        live.failed = false;
        live.error = None;
        live.notice = None;
        self.model.error = None;
        live.cancelled.store(false, Ordering::Release);
        live.checkpoint_sequence = 0;
        live.checkpoint_version = 0;
        live.checkpoint_artifacts.clear();
        live.modified_artifacts.clear();
        live.checkpoint_at = Instant::now();
        live.subagents.clear();
        live.skills.clear();
        live.changed();
        live.persistence_busy = true;
        let version = live.version;
        let window = live.window.clone();
        self.next_operation += 1;
        let opid = self.next_operation;
        self.model
            .pending_commands
            .insert(opid, "controller:prepare".into());
        self.pending.insert(
            opid,
            Pending {
                args: Value::Null,
                kind: Operation::Initial {
                    requests,
                    previous,
                    draft: prompt.clone(),
                    attachments: self.model.attachments.clone(),
                    run_id,
                    version,
                },
            },
        );
        let backend = self.backend.clone();
        let queue = self.queue.clone();
        let generation = self.generation;
        let replay_images = self.model.replay_image_inputs.clone();
        self.runtime.spawn(async move {
            let mut failure_phase = Some("attachment_preparation".to_string());
            let result: Result<Value, String> = async {
                let attachments: Vec<_> = data.messages[data.messages.len() - 2]
                    .attachments
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|a| !a.is_dir)
                    .collect();

                let images = tokio::task::spawn_blocking(move || {
                    prepare_replay_images(&attachments, &replay_images)
                })
                .await
                .map_err(|e| e.to_string())??;
                let mut data = data;
                let user_at = data.messages.len() - 2;
                if !images.is_empty() {
                    data.messages[user_at].image_inputs = Some(images);
                }
                failure_phase = Some("initial_save".into());
                let window = save_snapshot(&backend, &data, window).await?;
                Ok(json!({"window":window,"data":data}))
            }
            .await;
            let _ = queue.emit(
                "controller:result",
                json!(Completion {
                    opid,
                    failure_phase: if result.is_err() {
                        failure_phase.take()
                    } else {
                        None
                    },
                    command: "controller:prepare".into(),
                    session_id: Some(id),
                    generation,
                    result
                }),
            );
        });
        self.model.draft.clear();
        self.model.attachments.clear();
        self.model.replay_image_inputs.clear();
        self.publish();
        Ok(())
    }
    pub fn abort(&mut self) -> Result<(), String> {
        let id = self.selected.as_ref().ok_or("No session selected")?;
        let live = self.live.get_mut(id).ok_or("Session unavailable")?;
        let run = live.run_id.as_ref().ok_or("No active run")?;
        live.cancelled.store(true, Ordering::Release);
        // Preparing runs have no runner registration; the initial ack cancels them.
        let _ = self.backend.runner.abort(run);
        Ok(())
    }
    pub fn choose_council(
        &mut self,
        message_id: impl AsRef<str>,
        candidate: impl AsRef<str>,
    ) -> Result<(), String> {
        self.ensure_running()?;
        let candidate = candidate.as_ref();
        if !matches!(candidate, "A" | "B") {
            return Err("Candidate must be A or B".into());
        }
        if self.model.preview.is_some() {
            return Err("History preview is read-only".into());
        }
        let id = self.selected.clone().ok_or("No session selected")?;
        let live = self.live.get_mut(&id).ok_or("Session unavailable")?;
        if live.protected() {
            return Err("Wait for session to finish saving".into());
        }
        let message = live
            .data
            .messages
            .iter_mut()
            .find(|m| m.id == message_id.as_ref())
            .ok_or("Message unavailable")?;
        let turn = message
            .council_turn
            .as_mut()
            .ok_or("Not a council message")?;
        turn.chosen_winner = Some(candidate.into());
        turn.candidate_a.winner = Some(candidate == "A");
        turn.candidate_b.winner = Some(candidate == "B");
        let winner = if candidate == "A" {
            &turn.candidate_a
        } else {
            &turn.candidate_b
        };
        message.content = winner.content.clone();
        message.reasoning = winner.reasoning.clone();
        live.changed();
        self.schedule();
        self.publish();
        Ok(())
    }
    /// At most 256 events, with one history publication after the entire batch.
    pub fn poll(&mut self) -> bool {
        if let Some(live) = self.selected.as_ref().and_then(|id| self.live.get_mut(id)) {
            if self
                .published
                .as_ref()
                .is_some_and(|(id, _)| id == &live.data.session.id)
            {
                live.selected_artifact = self.model.selected_artifact_id.clone();
            }
        }
        let mut changed = false;
        for _ in 0..256 {
            let Ok(event) = self.receiver.try_recv() else {
                break;
            };
            changed = true;
            if event.topic == "controller:result" {
                match serde_json::from_value::<Completion>(event.payload) {
                    Ok(c) => self.complete(c),
                    Err(e) => self.model.error = Some(format!("Invalid controller result: {e}")),
                }
            } else if event.topic == "agent-event" {
                let kind = event.payload["type"].as_str().unwrap_or("");
                let data = &event.payload["data"];
                if let Some(live) = data["sessionId"]
                    .as_str()
                    .and_then(|id| self.live.get_mut(id))
                {
                    if let Err(error) = reducer::apply(live, kind, data) {
                        live.error = Some(error);
                    }
                }
            } else {
                self.background(&event);
                self.model.outbound.push(event);
                if self.model.outbound.len() > 128 {
                    self.model.outbound.remove(0);
                }
            }
        }
        self.advance_shutdown();
        changed |= self.schedule();
        self.publish();
        self.evict();
        changed
    }
    fn complete(&mut self, completion: Completion) {
        let Some(pending) = self.pending.remove(&completion.opid) else {
            return;
        };
        self.model.pending_commands.remove(&completion.opid);
        let result = CommandResult {
            id: completion.opid,
            command: completion.command.clone(),
            session_id: completion.session_id.clone(),
            generation: completion.generation,
            value: completion
                .result
                .as_ref()
                .ok()
                .map(|v| v.get("__nativeData").unwrap_or(v).clone()),
            error: completion.result.as_ref().err().cloned(),
            failure_phase: completion.failure_phase.clone().or_else(|| {
                (completion.result.is_err() && completion.command == "controller:save")
                    .then(|| "final_save".into())
            }),
        };
        self.model.last_command = Some(result.clone());
        self.model.results.insert(result.id, result);
        while self.model.results.len() > 256 {
            self.model.results.pop_first();
        }
        if completion.command == crate::history::HISTORY_COMMAND {
            self.complete_history(
                completion.opid,
                completion.session_id.as_deref(),
                completion.generation,
                completion.result,
            );
            return;
        }
        let id = completion.session_id.clone();
        match pending.kind {
            Operation::Initial {
                mut requests,
                previous,
                draft,
                attachments,
                run_id,
                version,
            } => {
                let Some(live) = id.as_ref().and_then(|id| self.live.get_mut(id)) else {
                    return;
                };
                if live.run_id.as_deref() != Some(&run_id) {
                    return;
                }
                live.persistence_busy = false;
                match completion.result.and_then(|v| {
                    Ok((
                        decode::<SessionWindow>(&v["window"])?,
                        decode::<SessionData>(&v["data"])?,
                    ))
                }) {
                    Ok((window, data)) => {
                        live.committed = Some(data.clone());
                        live.window = Some(window.clone());
                        live.dirty = live.version != version;
                        if live.version == version {
                            live.data = data.clone();
                        } else {
                            // Preparation may finish after cancellation. Hydrate images
                            // only; never overwrite newer terminal/message state.
                            for user in data.messages.iter().filter(|m| m.role == "user") {
                                if let Some(current) =
                                    live.data.messages.iter_mut().find(|m| m.id == user.id)
                                {
                                    current.image_inputs = user.image_inputs.clone();
                                }
                            }
                        }
                        live.version += 1;
                        live.checkpoint_version = live.version;
                        if live.cancelled.load(Ordering::Acquire) {
                            self.fail_run(id.as_ref().unwrap(), &run_id, None, true);
                        } else {
                            if let Some(status) = &mut live.status {
                                status.phase = RunPhase::WaitingProvider;
                            }
                            for request in &mut requests {
                                request.history_window = Some(window.clone());
                                // Hydrate native image/protocol payloads, but preserve
                                // the current API-only prompt (/plan, /ask, /agent).
                                let api_prompt = request.messages.last().map(|m| m.content.clone());
                                request.messages =
                                    data.messages[..data.messages.len() - 1].to_vec();
                                if let Some(user) = request.messages.last_mut() {
                                    if let Some(api_prompt) = api_prompt {
                                        user.content = api_prompt;
                                    }
                                    if let Some(attachments) = &user.attachments {
                                        if !attachments.is_empty() {
                                            user.content.push_str("\n\n## Attached context:\n");
                                            for attachment in attachments {
                                                user.content.push_str(&format!(
                                                    "- `{}`\n",
                                                    attachment.path
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                            let (command, args) = if requests.len() == 2 {
                                (
                                    "send_council_message",
                                    json!({"requestA":requests[0],"requestB":requests[1]}),
                                )
                            } else {
                                ("send_message", json!({"request":requests[0]}))
                            };
                            self.issue(command, args, id, Operation::Started { run_id });
                        }
                    }
                    Err(error) => {
                        let phase = completion
                            .failure_phase
                            .unwrap_or_else(|| "initial_save".into());
                        if let Some(receipt) = storage::CommittedSaveError::parse(&error) {
                            live.recovery = Some(SaveRecovery::CanonicalCommitted {
                                proof: receipt.proof,
                                version,
                            });
                        }
                        live.error = Some(error);
                        live.failure_phase = Some(phase.clone());
                        live.failed = true;
                        live.done = true;
                        if phase == "attachment_preparation" && live.version == version {
                            // This is the only proven pre-write failure. An error from
                            // save_snapshot may occur AFTER the canonical write, so it
                            // must never trigger this rollback.
                            live.data = previous;
                            live.dirty = false;
                            live.recovery_draft = Some((draft.clone(), attachments.clone()));
                            if self.selected == id
                                && completion.generation == self.generation
                                && self.model.draft.is_empty()
                                && self.model.attachments.is_empty()
                            {
                                self.model.draft = draft;
                                self.model.attachments = attachments;
                                live.recovery_draft = None;
                            }
                        } else {
                            close_unfinished(live, false);
                            live.dirty = true;
                        }
                        if let Some(status) = &mut live.status {
                            status.phase = RunPhase::Error;
                            status.save_failed = live.dirty;
                            status.ended_at = Some(now());
                        }
                        live.run_id = None;
                        live.version += 1;
                    }
                }
            }
            Operation::Started { run_id } => {
                if let Some(id) = id {
                    if let Err(error) = completion.result {
                        self.fail_run(&id, &run_id, Some(error), false);
                    } else if self.live.get(&id).is_some_and(|l| {
                        l.run_id.as_deref() == Some(&run_id) && l.cancelled.load(Ordering::Acquire)
                    }) {
                        let _ = self.backend.runner.abort(&run_id);
                    }
                }
            }
            Operation::Save {
                data,
                version,
                run_id,
            } => {
                let Some(live) = id.as_ref().and_then(|id| self.live.get_mut(id)) else {
                    return;
                };
                live.persistence_busy = false;
                if run_id.is_some() && live.run_id == run_id && live.done && !live.failed
                    && !live.cancelled.load(Ordering::Acquire) {
                    live.completion_pending = run_id.clone();
                }
                match completion.result.and_then(|v| decode::<SessionWindow>(&v)) {
                    Ok(window) => {
                        live.failure_phase = None;
                        live.error = None;
                        live.window = Some(window);
                        live.committed = Some(data);
                        live.recovery = None;
                        live.bytes_version = None;
                        live.dirty = live.version != version;
                        if live.run_id == run_id {
                            if let Some(status) = &mut live.status {
                                status.phase = if live.cancelled.load(Ordering::Acquire) {
                                    RunPhase::Cancelled
                                } else if live.failed {
                                    RunPhase::Error
                                } else {
                                    RunPhase::Completed
                                };
                                status.ended_at = Some(now());
                                status.save_failed = false;
                            }
                            live.run_id = None;
                        }
                        self.model.record_completion(live.take_completion());
                        let summary = live.data.session.clone();
                        self.catalog(summary);
                    }
                    Err(error) => {
                        if let Some(receipt) = storage::CommittedSaveError::parse(&error) {
                            live.recovery = Some(SaveRecovery::CanonicalCommitted {
                                proof: receipt.proof,
                                version,
                            });
                        }
                        live.error = Some(error);
                        live.failure_phase = Some("final_save".into());
                        live.dirty = true;
                        live.run_id = None;
                        if let Some(status) = &mut live.status {
                            status.save_failed = true;
                            status.phase = RunPhase::Error;
                            status.ended_at = Some(now());
                        }
                    }
                }
            }
            Operation::RecoverSave { proof, version } => {
                let Some(live) = id.as_ref().and_then(|id| self.live.get_mut(id)) else {
                    return;
                };
                live.persistence_busy = false;
                match completion
                    .result
                    .and_then(|v| decode::<SessionPage>(&v))
                    .and_then(|page| {
                        if page.data.session.id != proof.session_id
                            || page.window.offset != proof.offset
                            || page.window.total != proof.total
                            || page.window.first_message_id != proof.first_message_id
                            || page.window.revision.is_empty()
                            || page.window.end.is_some()
                        {
                            return Err(
                                "Invalid committed recovery acknowledgment; snapshot preserved"
                                    .into(),
                            );
                        }
                        Ok(page)
                    }) {
                    Ok(page) => {
                        // Cursor/baseline-only acknowledgment. Do not replace live
                        // content: streams/cancellation may have advanced since commit.
                        live.window = Some(page.window);
                        live.committed = Some(page.data);
                        live.recovery = None;
                        live.bytes_version = None;
                        live.dirty = live.version != version;
                        live.error = None;
                        live.failure_phase = None;
                        if let Some(status) = &mut live.status {
                            status.save_failed = false;
                            status.phase = if live.dirty {
                                RunPhase::Saving
                            } else {
                                RunPhase::Completed
                            };
                        }
                        if self.selected == id {
                            self.model.error = None;
                        }
                        self.model.record_completion(live.take_completion());
                        self.refresh.remove(proof.session_id.as_str());
                    }
                    Err(error) => {
                        // Retain the proof and dirty snapshot for another explicit
                        // retry; never load an unrelated cursor or overwrite disk.
                        live.dirty = true;
                        live.error = Some(error);
                        live.failure_phase = Some("final_save".into());
                        if let Some(status) = &mut live.status {
                            status.save_failed = true;
                            status.phase = RunPhase::Error;
                        }
                    }
                }
            }
            Operation::Checkpoint {
                run_id,
                version,
                sequence,
            } => {
                let Some(live) = id.as_ref().and_then(|id| self.live.get_mut(id)) else {
                    return;
                };
                live.persistence_busy = false;
                live.checkpoint_at = Instant::now();
                if live.run_id.as_deref() != Some(&run_id) {
                    return;
                }
                match completion.result.and_then(|v| decode::<CheckpointAck>(&v)) {
                    Ok(ack) if ack.sequence == sequence => {
                        if let Some(window) = &mut live.window {
                            window.revision = ack.revision;
                        }
                        live.checkpoint_version = version;
                    }
                    Ok(_) => live.error = Some("Checkpoint sequence mismatch".into()),
                    Err(error) => live.error = Some(format!("Checkpoint failed: {error}")),
                }
            }
            Operation::Refresh { baseline } => {
                let Some(live) = id.as_ref().and_then(|id| self.live.get_mut(id)) else {
                    return;
                };
                live.persistence_busy = false;

                match completion.result.and_then(|v| decode::<SessionWindow>(&v)) {
                    Ok(window) => {
                        live.window = Some(window);
                        live.committed = Some(baseline);
                        live.bytes_version = None;
                    }
                    Err(error) => {
                        live.error = Some(format!("Native revision reconciliation failed: {error}"))
                    }
                }
            }
            Operation::Page { before } => {
                if let Some(live) = id.as_ref().and_then(|id| self.live.get_mut(id)) {
                    live.loading_older = false;
                }
                if completion.generation != self.generation {
                    return;
                }
                self.model.loading = false;
                let page = completion
                    .result
                    .and_then(|v| decode::<SessionPage>(v.get("__controllerPage").unwrap_or(&v)));
                match page {
                    Ok(page) => {
                        if id.as_deref() != Some(&page.data.session.id) {
                            self.model.error = Some("Page session mismatch".into());
                            return;
                        }
                        let session_id = page.data.session.id.clone();
                        if self.mutations.contains_key(&session_id) {
                            return;
                        }
                        if let Some(before) = before {
                            if self.selected.as_deref() != Some(&session_id) {
                                return;
                            }
                            if let Some(live) = self.live.get_mut(&session_id) {
                                let valid = live.window.as_ref().is_some_and(|w| {
                                    w.offset == before && w.revision == page.window.revision
                                }) && page.window.offset + page.data.messages.len()
                                    == before;
                                if !valid {
                                    live.error = Some("Older page cursor changed; retry".into());
                                    return;
                                }
                                let mut messages = page.data.messages.clone();
                                messages.extend(live.data.messages.clone());
                                live.data.messages = messages;
                                for artifact in page.data.artifacts {
                                    if !live.data.artifacts.iter().any(|a| a.id == artifact.id) {
                                        live.data.artifacts.push(artifact.clone());
                                    }
                                    if let Some(base) = &mut live.committed {
                                        if !base.artifacts.iter().any(|a| a.id == artifact.id) {
                                            base.artifacts.push(artifact);
                                        }
                                    }
                                }
                                if let Some(base) = &mut live.committed {
                                    let mut messages = page.data.messages;
                                    messages.extend(base.messages.clone());
                                    base.messages = messages;
                                }
                                if let Some(window) = &mut live.window {
                                    window.offset = page.window.offset;
                                    window.first_message_id = page.window.first_message_id;
                                }
                                live.version += 1;
                            }
                        } else {
                            if self
                                .live
                                .get(&session_id)
                                .is_some_and(LiveSession::protected)
                            {
                                return;
                            }
                            self.live.insert(
                                session_id.clone(),
                                LiveSession::new(page.data, Some(page.window)),
                            );
                            self.selected = Some(session_id);
                            self.published = None;
                        }
                    }
                    Err(error) => self.model.error = Some(error),
                }
            }
            Operation::Mutation {
                origin,
                external_page,
            } => {
                let Some(session_id) = id.as_ref() else {
                    return;
                };
                if self.mutations.get(session_id) != Some(&completion.opid) {
                    return;
                }
                self.mutations.remove(session_id);
                let valid = match (origin, self.live.get_mut(session_id)) {
                    (Some((identity, version, run_id)), Some(live)) => {
                        if live.identity != identity || live.mutation_busy != Some(completion.opid)
                        {
                            false
                        } else {
                            live.mutation_busy = None;
                            if external_page {
                                live.persistence_busy = false;
                            }
                            live.version == version && live.run_id == run_id && !live.dirty
                        }
                    }
                    (None, None) => true,
                    _ => false,
                };
                if !valid {
                    self.model.error =
                        Some("Stale mutation acknowledgment; newer live state retained".into());
                    return;
                }
                match completion.result {
                    Ok(value) => {
                        if let Err(error) = self.apply_result(
                            &completion.command,
                            &pending.args,
                            &value,
                            id.as_deref(),
                            completion.generation,
                        ) {
                            self.model.error = Some(error);
                        }
                    }
                    Err(error) => {
                        if let Some(receipt) = storage::CommittedSaveError::parse(&error) {
                            if let Some(live) = self.live.get_mut(session_id) {
                                if let Ok(data) = decode::<SessionData>(&pending.args["data"]) {
                                    live.data = data;
                                    live.changed();
                                    live.recovery = Some(SaveRecovery::CanonicalCommitted {
                                        proof: receipt.proof,
                                        version: live.version,
                                    });
                                    live.failure_phase = Some("final_save".into());
                                    live.error = Some(error.clone());
                                }
                            }
                        }
                        if completion.generation == self.generation {
                            self.model.error = Some(error);
                        }
                    }
                }
            }
            Operation::Generic => match completion.result {
                Ok(value) => {
                    if let Err(error) = self.apply_result(
                        &completion.command,
                        &pending.args,
                        &value,
                        id.as_deref(),
                        completion.generation,
                    ) {
                        self.model.error = Some(error);
                    }
                }
                Err(error) => {
                    if completion.generation == self.generation {
                        self.model.error = Some(error);
                    }
                }
            },
        }
    }
    fn fail_run(&mut self, id: &str, run_id: &str, error: Option<String>, cancelled: bool) {
        if let Some(live) = self.live.get_mut(id) {
            if let Some(error) = error {
                let _ = reducer::apply(
                    live,
                    "error",
                    &json!({"sessionId":id,"runId":run_id,"message":error}),
                );
            }
            let _ = reducer::apply(
                live,
                "done",
                &json!({"sessionId":id,"runId":run_id,"cancelled":cancelled,"successful":false}),
            );
        }
    }
    fn apply_result(
        &mut self,
        command: &str,
        args: &Value,
        value: &Value,
        id: Option<&str>,
        generation: u64,
    ) -> Result<(), String> {
        match command {
            "get_settings" => self.model.settings = decode(value)?,
            "save_settings" => self.model.settings = decode(&args["settings"])?,
            "list_sessions" => self.model.sessions = decode(value)?,
            "get_workspace_cwd" | "set_workspace_cwd" => {
                self.model.workspace = decode(value)?;
                if let Some(live) = self.selected.as_ref().and_then(|id| self.live.get_mut(id)) {
                    if live.data.messages.is_empty() && !live.protected() {
                        live.data.session.workspace_cwd = Some(self.model.workspace.clone());
                        live.version += 1;
                    }
                }
            }
            "load_session_around" if generation == self.generation => {
                self.model.preview = Some(decode(value)?)
            }
            "rename_session_summary" | "toggle_pin_session" | "rename_session" => {
                let summary: Session = if command == "rename_session" {
                    decode(&value["session"])?
                } else {
                    decode(value)?
                };
                if Some(summary.id.as_str()) != id {
                    return Err("Metadata session mismatch".into());
                }
                if let Some(live) = self.live.get_mut(&summary.id) {
                    live.data.session.title = summary.title.clone();
                    live.data.session.pinned = summary.pinned;
                    live.data.session.pinned_at = summary.pinned_at;
                    live.version += 1;
                    self.refresh.insert(summary.id.clone(), None);
                }
                self.catalog(summary);
            }
            "delete_session" => {
                if let Some(id) = id {
                    self.live.remove(id);
                    self.refresh.remove(id);
                    self.model.sessions.retain(|s| s.id != id);
                    if self.selected.as_deref() == Some(id) {
                        self.new_session();
                    }
                }
            }
            "compact_session" => {
                let data: SessionData = decode(value)?;
                if id != Some(data.session.id.as_str()) {
                    return Err("Compaction session mismatch".into());
                }
                self.catalog(data.session.clone());
                if let Some(id) = id {
                    self.live.remove(id);
                    if self.selected.as_deref() == Some(id) {
                        self.open_session(id);
                    }
                }
            }
            "read_session_artifact" => {
                let artifact: Artifact = decode(value)?;
                if let Some(live) = id.and_then(|id| self.live.get_mut(id)) {
                    if !live.data.artifacts.iter().any(|a| a.id == artifact.id) {
                        live.data.artifacts.push(artifact.clone());
                        live.version += 1;
                    }
                    if let Some(base) = &mut live.committed {
                        if !base.artifacts.iter().any(|a| a.id == artifact.id) {
                            base.artifacts.push(artifact);
                        }
                    }
                }
            }
            "save_session_page" | "checkpoint_session_page" => {
                let window: SessionWindow = decode(value)?;
                let data: SessionData = decode(&args["data"])?;
                if let Some(live) = id.and_then(|id| self.live.get_mut(id)) {
                    if live.protected() {
                        return Err(
                            "External save completed during active session; reload required".into(),
                        );
                    }
                    live.data = data.clone();
                    live.committed = Some(data);
                    live.window = Some(window);
                    live.version += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn background(&mut self, event: &BackendEvent) {
        if event.topic == "agent:self-improvement" {
            if let Ok(review) = decode::<SelfImprovementEvent>(&event.payload) {
                if let Some(live) = self.live.get_mut(&review.session_id) {
                    let mut changed = false;
                    for data in std::iter::once(&mut live.data) {
                        if let Some(m) = data.messages.iter_mut().find(|m| {
                            m.role == "assistant"
                                && (m.run_id.as_deref() == Some(&review.run_id)
                                    || m.id == review.run_id)
                        }) {
                            m.self_improvement = Some(review.clone());
                            changed = true;
                        }
                    }
                    if changed {
                        live.version += 1;
                    }
                    self.refresh.insert(review.session_id, Some(review.run_id));
                }
            }
        }
    }
    fn schedule(&mut self) -> bool {
        let ids: Vec<_> = self.live.keys().cloned().collect();
        let mut changed = false;
        for id in ids {
            let live = self.live.get_mut(&id).unwrap();
            if live.persistence_busy || live.loading_older || live.mutation_busy.is_some() {
                continue;
            }
            if live.recovery.is_none() {
                if let Some(review) = self.refresh.remove(&id) {
                    if let (Some(mut baseline), Some(window)) =
                        (live.committed.clone(), live.window.clone())
                    {
                        baseline.session.title = live.data.session.title.clone();
                        baseline.session.pinned = live.data.session.pinned;
                        baseline.session.pinned_at = live.data.session.pinned_at;
                        if let Some(run) = review.as_deref() {
                            for message in &mut baseline.messages {
                                if message.run_id.as_deref() == Some(run) || message.id == run {
                                    if let Some(current) =
                                        live.data.messages.iter().find(|m| m.id == message.id)
                                    {
                                        message.self_improvement = current.self_improvement.clone();
                                    }
                                }
                            }
                        }
                        live.persistence_busy = true;
                        let args = json!({"sessionId":id,"data":baseline,"window":window,"allowMetadata":true,"reviewRunId":review});
                        self.issue(
                            "refresh_session_window",
                            args,
                            Some(id),
                            Operation::Refresh { baseline },
                        );
                        changed = true;
                        continue;
                    }
                }
            }
            let save_failed = live.status.as_ref().is_some_and(|s| s.save_failed)
                || matches!(
                    live.failure_phase.as_deref(),
                    Some("initial_save" | "final_save")
                );
            if live.dirty && !save_failed && live.recovery.is_some() {
                let SaveRecovery::CanonicalCommitted { proof, version } =
                    live.recovery.clone().unwrap();
                live.persistence_busy = true;
                self.issue(
                    "recover_save_ack",
                    json!({"sessionId":id,"proof":proof}),
                    Some(id),
                    Operation::RecoverSave { proof, version },
                );
                changed = true;
            } else if live.dirty && (live.done || live.run_id.is_none()) && !save_failed {
                live.persistence_busy = true;
                let data = live.data.clone();
                let version = live.version;
                let run_id = live.run_id.clone();
                let window = live.window.clone();
                self.next_operation += 1;
                let opid = self.next_operation;
                self.pending.insert(
                    opid,
                    Pending {
                        args: Value::Null,
                        kind: Operation::Save {
                            data: data.clone(),
                            version,
                            run_id,
                        },
                    },
                );
                self.model
                    .pending_commands
                    .insert(opid, "controller:save".into());
                let backend = self.backend.clone();
                let queue = self.queue.clone();
                let generation = self.generation;
                let checkpoint = self.shutdown_fallback;
                self.runtime.spawn(async move {
                    let result = persist_snapshot(&backend, &data, window, checkpoint)
                        .await
                        .map(|w| json!(w));
                    let _ = queue.emit(
                        "controller:result",
                        json!(Completion {
                            opid,
                            command: "controller:save".into(),
                            session_id: Some(id),
                            generation,
                            failure_phase: None,
                            result
                        }),
                    );
                });
                changed = true;
            } else if live.run_id.is_some()
                && !live.done
                && live.version > live.checkpoint_version
                && live.checkpoint_at.elapsed() >= Duration::from_secs(2)
            {
                if let Some(window) = &live.window {
                    let run_id = live.run_id.clone().unwrap();
                    live.checkpoint_sequence += 1;
                    live.persistence_busy = true;
                    let version = live.version;
                    let sequence = live.checkpoint_sequence;
                    let messages: Vec<_> = live
                        .data
                        .messages
                        .iter()
                        .filter(|m| m.run_id.as_deref() == Some(&run_id))
                        .cloned()
                        .collect();
                    let artifacts: Vec<_> = live
                        .data
                        .artifacts
                        .iter()
                        .filter(|a| live.modified_artifacts.contains(&a.id))
                        .cloned()
                        .collect();
                    let args = json!({"sessionId":id,"request":{"runId":run_id,"baseRevision":window.revision,"sequence":sequence,"messages":messages,"artifacts":artifacts}});
                    self.issue(
                        "checkpoint_session_delta",
                        args,
                        Some(id),
                        Operation::Checkpoint {
                            run_id,
                            version,
                            sequence,
                        },
                    );
                    changed = true;
                }
            }
        }
        changed
    }
    pub(crate) fn catalog(&mut self, summary: Session) {
        if let Some(old) = self.model.sessions.iter_mut().find(|s| s.id == summary.id) {
            *old = summary;
        } else {
            self.model.sessions.push(summary);
        }
        self.model.sessions.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.updated_at.cmp(&a.updated_at))
        });
    }
    pub(crate) fn publish(&mut self) {
        let Some(id) = self.selected.as_ref() else {
            return;
        };
        let Some(live) = self.live.get_mut(id) else {
            return;
        };
        self.clock += 1;
        live.touched = self.clock;
        if self.published.as_ref() != Some(&(id.clone(), live.version)) {
            self.model.current = Some(live.data.clone());
            self.published = Some((id.clone(), live.version));
        }
        self.model.window = live.window.clone();
        self.model.busy =
            live.run_id.is_some() || live.persistence_busy || live.mutation_busy.is_some();
        self.model.loading_older = live.loading_older;
        self.model.run_status = live.status.clone();
        self.model.failure_phase = live.failure_phase.clone();
        self.model.retry_save_available = live.dirty
            && live.run_id.is_none()
            && !live.persistence_busy
            && live.mutation_busy.is_none();
        self.model.recoverable_draft = live.recovery_draft.as_ref().map(|(draft, _)| draft.clone());
        self.model.recoverable_attachments = live
            .recovery_draft
            .as_ref()
            .map(|(_, a)| a.clone())
            .unwrap_or_default();
        if live.error.is_some() {
            self.model.error = live.error.clone();
        }
        self.model.notice = live.notice.clone();
        self.model.subagents = live.subagents.clone();
        self.model.active_skills = live.skills.clone();
        self.model.selected_artifact_id = live.selected_artifact.clone();
    }
    fn ensure_running(&self) -> Result<(), String> {
        if self.is_shutting_down() {
            Err("Controller is shutting down".into())
        } else {
            Ok(())
        }
    }
    /// Retry only the selected dirty snapshot, never replay the user/provider turn.
    /// Dirty remains set until the serialized save acknowledgment arrives.
    pub fn retry_save(&mut self) -> Result<(), String> {
        self.ensure_running()?;
        let id = self.selected.clone().ok_or("No session selected")?;
        let live = self.live.get_mut(&id).ok_or("Session unavailable")?;
        if live.run_id.is_some()
            || live.persistence_busy
            || live.loading_older
            || live.mutation_busy.is_some()
        {
            return Err("Session operation in progress".into());
        }
        if !live.dirty {
            return Err("No unsaved changes".into());
        }
        close_unfinished(live, false);
        if let Some(status) = &mut live.status {
            status.save_failed = false;
            status.phase = RunPhase::Saving;
        }
        live.error = None;
        live.failure_phase = None;
        self.model.error = None;
        self.schedule();
        self.publish();
        Ok(())
    }
    /// Hosts must keep polling after closing is requested, until ready or the
    /// deadline expires. This never blocks the UI thread on storage or runtime.
    pub fn begin_shutdown(&mut self) {
        if self.is_shutting_down() {
            return;
        }
        self.shutdown_started = Some(Instant::now());
        for live in self.live.values_mut() {
            if live.run_id.is_some() && !live.done {
                live.cancelled.store(true, Ordering::Release);
            }
            if let Some(run) = &live.run_id {
                let _ = self.backend.runner.abort(run);
            }
            live.failure_phase = None;
            if let Some(status) = &mut live.status {
                status.save_failed = false;
            }
        }
        self.schedule();
        self.publish();
    }
    /// Retry every dirty shutdown snapshot without replaying a provider request or
    /// replacing a stale canonical cursor. A retry is never a save acknowledgment.
    pub fn retry_shutdown_saves(&mut self) -> Result<(), String> {
        if !self.is_shutting_down() {
            return Err("No shutdown is in progress".into());
        }
        let failed = self.live.values().any(|live| {
            live.dirty
                && (live.failure_phase.is_some()
                    || live
                        .status
                        .as_ref()
                        .is_some_and(|status| status.save_failed))
        });
        if !self.shutdown_timed_out() && !failed {
            return Err("Shutdown has not failed or reached its deadline".into());
        }
        if !self.pending.is_empty()
            || !self.refresh.is_empty()
            || !self.mutations.is_empty()
            || self.live.values().any(|live| {
                live.run_id.is_some()
                    || live.persistence_busy
                    || live.loading_older
                    || live.mutation_busy.is_some()
            })
        {
            return Err("Wait for the in-flight session operations before retrying".into());
        }
        if !self.live.values().any(|live| live.dirty) {
            return Err("No unsaved session snapshots remain; choose Keep open to continue".into());
        }
        for live in self.live.values_mut().filter(|live| live.dirty) {
            close_unfinished(live, false);
            live.failure_phase = None;
            live.error = None;
            if let Some(status) = &mut live.status {
                status.save_failed = false;
                status.phase = RunPhase::Saving;
            }
        }
        self.model.error = None;
        self.shutdown_started = Some(Instant::now());
        // Explicit retry uses the ordinary canonical save, never the timeout
        // checkpoint path. schedule retains each snapshot's original cursor.
        self.shutdown_fallback = false;
        self.schedule();
        self.publish();
        Ok(())
    }
    pub fn retry_shutdown_save(&mut self) -> Result<(), String> {
        self.retry_shutdown_saves()
    }
    /// Resume only after all existing work drains. In particular, do not start
    /// new saves here or clear failed dirty snapshots merely to dismiss a dialog.
    pub fn cancel_shutdown(&mut self) -> bool {
        if !self.is_shutting_down()
            || !self.pending.is_empty()
            || !self.refresh.is_empty()
            || !self.mutations.is_empty()
            || self.live.values().any(|live| {
                live.run_id.is_some()
                    || live.persistence_busy
                    || live.loading_older
                    || live.mutation_busy.is_some()
            })
        {
            return false;
        }
        self.shutdown_started = None;
        self.shutdown_fallback = false;
        for live in self.live.values_mut() {
            live.cancelled.store(false, Ordering::Release);
        }
        self.publish();
        true
    }
    pub fn resume_after_shutdown_failure(&mut self) -> bool {
        self.cancel_shutdown()
    }
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown_started.is_some()
    }
    /// True only after every active turn and persistence acknowledgment drains.
    /// A timeout is NOT a successful save; the last acknowledged journal remains
    /// durable even if a host must close at the deadline.
    pub fn shutdown_ready(&self) -> bool {
        self.is_shutting_down()
            && self.pending.is_empty()
            && self.refresh.is_empty()
            && self.mutations.is_empty()
            && self.live.values().all(|l| !l.protected())
    }
    pub fn shutdown_timed_out(&self) -> bool {
        self.shutdown_started
            .is_some_and(|at| at.elapsed() >= Duration::from_secs(5))
    }
    fn advance_shutdown(&mut self) {
        if !self.shutdown_timed_out() || self.shutdown_fallback {
            return;
        }
        self.shutdown_fallback = true;
        for live in self.live.values_mut() {
            if let Some(run) = &live.run_id {
                let _ = self.backend.runner.abort(run);
                close_unfinished(live, true);
                live.done = true;
            }
            live.failure_phase = None;
            if let Some(status) = &mut live.status {
                status.save_failed = false;
            }
        }
        // schedule() checkpoints remaining dirty snapshots asynchronously, but
        // never races an existing initial/delta/page write for the same session.
    }
    fn evict(&mut self) {
        // Only cold, clean snapshots are measured: one O(data) traversal per
        // version, never on streaming chunks. JSON counts nested Strings, Vecs
        // and maps without a second large allocation. Both committed and live
        // snapshots are counted, conservatively double-counting shared data.
        let mut count = 0usize;
        let mut bytes = 0usize;
        for (id, live) in &mut self.live {
            if Some(id.as_str()) == self.selected.as_deref()
                || (live.protected() || live.recovery_draft.is_some())
            {
                continue;
            }
            if live.bytes_version != Some(live.version) {
                live.bytes_estimate = counted_bytes(&(
                    &live.data,
                    &live.committed,
                    &live.checkpoint_artifacts,
                    &live.subagents,
                    &live.skills,
                ));
                live.bytes_version = Some(live.version);
            }
            count += 1;
            bytes = bytes.saturating_add(live.bytes_estimate);
        }
        while count > CACHE_SIZE || bytes > CACHE_BYTES {
            let candidate = self
                .live
                .iter()
                .filter(|(id, l)| {
                    Some(id.as_str()) != self.selected.as_deref()
                        && !l.protected()
                        && l.recovery_draft.is_none()
                })
                .min_by_key(|(_, l)| l.touched)
                .map(|(id, _)| id.clone());
            let Some(id) = candidate else {
                break;
            };
            if let Some(live) = self.live.remove(&id) {
                bytes = bytes.saturating_sub(live.bytes_estimate);
                count -= 1;
            }
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.queue.clear_waker();
        // Disconnect before dropping the runtime: bounded producers may currently
        // be blocked in synchronous emit, and runtime shutdown waits for them.
        let (_, replacement) = std::sync::mpsc::channel();
        drop(std::mem::replace(&mut self.receiver, replacement));
        for live in self.live.values() {
            live.cancelled.store(true, Ordering::Release);
            if let Some(run) = &live.run_id {
                let _ = self.backend.runner.abort(run);
            }
        } // A static inert replacement keeps the public Arc<Runtime> API while
          // moving potentially blocking destruction off both UI and async threads.
        static INERT: std::sync::OnceLock<Arc<Runtime>> = std::sync::OnceLock::new();
        let inert = INERT.get_or_init(|| {
            Arc::new(
                tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap(),
            )
        });
        let runtime = std::mem::replace(&mut self.runtime, inert.clone());
        std::thread::spawn(move || drop(runtime));
    }
}
fn close_unfinished(live: &mut LiveSession, cancelled: bool) {
    let mut changed = false;
    for m in &mut live.data.messages {
        if m.is_streaming != Some(true) {
            continue;
        }
        changed = true;
        m.is_streaming = Some(false);
        m.run_status = Some(if cancelled { "cancelled" } else { "failed" }.into());
        for tool in m.tool_calls.iter_mut().flatten().chain(
            m.segments
                .iter_mut()
                .flatten()
                .flat_map(|s| s.tool_calls.iter_mut().flatten()),
        ) {
            if matches!(tool.status.as_str(), "pending" | "running") {
                tool.status = if cancelled { "cancelled" } else { "error" }.into();
            }
        }
        if let Some(turn) = &mut m.council_turn {
            turn.candidate_a.is_streaming = Some(false);
            turn.candidate_b.is_streaming = Some(false);
        }
    }
    if changed {
        live.changed();
    }
}
/// Reuse bytes only for actual selected attachments, never arbitrary cached paths.
fn prepare_replay_images(
    attachments: &[AttachedContext],
    replay: &[ImageInput],
) -> Result<Vec<ImageInput>, String> {
    let mut inputs = Vec::new();
    let mut fresh = Vec::new();
    for attachment in attachments.iter().filter(|a| !a.is_dir) {
        if let Some(input) = replay.iter().find(|i| i.source_path == attachment.path) {
            if !inputs
                .iter()
                .any(|i: &ImageInput| i.source_path == input.source_path)
            {
                inputs.push(input.clone());
            }
        } else {
            fresh.push(attachment.clone());
        }
    }
    inputs.extend(images::prepare_attachments(&fresh)?);
    images::validate_inputs(&inputs)?;
    Ok(inputs)
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
fn counted_bytes(value: &impl Serialize) -> usize {
    let mut writer = CountingWriter::default();
    let mut serializer = serde_json::Serializer::new(&mut writer);
    if value.serialize(&mut serializer).is_err() {
        return usize::MAX;
    }
    writer.0
}
fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|e| e.to_string())
}
async fn save_snapshot(
    backend: &Backend,
    data: &SessionData,
    window: Option<SessionWindow>,
) -> Result<SessionWindow, String> {
    persist_snapshot(backend, data, window, false).await
}
async fn persist_snapshot(
    backend: &Backend,
    data: &SessionData,
    window: Option<SessionWindow>,
    checkpoint: bool,
) -> Result<SessionWindow, String> {
    if let Some(window) = window {
        decode(
            &backend
                .dispatch(
                    if checkpoint {
                        "checkpoint_session_page"
                    } else {
                        "save_session_page"
                    },
                    json!({"data":data,"window":window}),
                )
                .await?,
        )
    } else {
        let page: SessionPage = decode(
            &backend
                .dispatch(
                    "replace_session_checked",
                    json!({"data":data,"expectedWindow":null}),
                )
                .await?,
        )?;
        Ok(page.window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_completion_only_after_successful_save_including_offscreen() {
        for (failed, cancelled, save_error, expected) in [(false, false, false, 1), (true, false, false, 0), (false, true, false, 0), (false, false, true, 0)] {
            let mut c = controller();
            let id = c.selected.clone().unwrap();
            let live = c.live.get_mut(&id).unwrap();
            live.run_id = Some("desktop-run".into());
            live.done = true;
            live.failed = failed;
            live.cancelled.store(cancelled, Ordering::Release);
            live.dirty = true;
            let data = live.data.clone();
            let version = live.version;
            c.new_session(); // completion must not depend on the selected conversation
            assert!(serde_json::to_value(c.model()).unwrap().get("completedRuns").and_then(|v| v.as_array()).is_none_or(|v| v.is_empty()));
            c.pending.insert(99001, Pending { args: json!({}), kind: Operation::Save { data, version, run_id: Some("desktop-run".into()) } });
            let window = json!({"offset":0,"total":0,"revision":"desktop-revision","firstMessageId":null,"end":null});
            c.complete(Completion { opid:99001, command:"controller:save".into(), session_id:Some(id.clone()), generation:c.generation, result:if save_error {Err("disk unavailable".into())} else {Ok(window)}, failure_phase:None });
            let model = serde_json::to_value(c.model()).unwrap();
            let events = model.get("completedRuns").and_then(|v| v.as_array());
            assert_eq!(events.map_or(0, |v| v.len()), expected, "failed={failed} cancelled={cancelled} save_error={save_error}");
            if expected == 1 { assert_eq!(events.unwrap()[0]["sessionId"], id); assert_eq!(events.unwrap()[0]["runId"], "desktop-run"); }
        }
    }
    pub(super) fn controller() -> Controller {
        static PROFILE: std::sync::Once = std::sync::Once::new();
        PROFILE.call_once(|| {
            aether_core::agent::profile::configure_storage_root(
                std::env::temp_dir().join(format!("aether-state-unit-{}", uuid::Uuid::new_v4())),
            )
            .unwrap();
        });
        Controller::new_with_workspace(String::new()).unwrap()
    }
    #[test]
    fn checkpoints_ack_exact_versions_and_finish_after_inflight_delta() {
        let mut c = controller();
        let id = c.selected.clone().unwrap();
        let run = "checkpoint-run";
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages = vec![decode(&json!({"id":"u","role":"user","content":"prompt","timestamp":1})).unwrap(),decode(&json!({"id":"a","role":"assistant","content":"","timestamp":2,"runId":run,"isStreaming":true})).unwrap()];
        live.data.session.message_count = 2;
        let data = live.data.clone();
        let window = c
            .runtime
            .block_on(save_snapshot(&c.backend, &data, None))
            .unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.committed = Some(data.clone());
        live.window = Some(window);
        live.run_id = Some(run.into());
        live.message_id = Some("a".into());
        reducer::apply(
            live,
            "chunk",
            &json!({"sessionId":id,"runId":run,"text":"first"}),
        )
        .unwrap();
        live.checkpoint_at = Instant::now() - Duration::from_secs(3);
        assert!(c.schedule());
        assert!(c.live[&id].persistence_busy);
        assert!(!c.schedule(), "duplicate in-flight checkpoint");
        let baseline = c.live[&id].committed.clone().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while c.live[&id].persistence_busy {
            c.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(c.live[&id].checkpoint_sequence, 1);
        assert!(c.live[&id].error.is_none(), "{:?}", c.live[&id].error);
        assert_eq!(
            json!(c.live[&id].committed),
            json!(baseline),
            "journal ack must not replace committed history"
        );
        assert_eq!(c.live[&id].checkpoint_version, c.live[&id].version);
        assert!(!c.schedule(), "idle ack must not re-arm journal");
        let live = c.live.get_mut(&id).unwrap();
        reducer::apply(
            live,
            "chunk",
            &json!({"sessionId":id,"runId":run,"text":" second"}),
        )
        .unwrap();
        reducer::apply(
            live,
            "done",
            &json!({"sessionId":id,"runId":run,"cancelled":false,"successful":true}),
        )
        .unwrap();
        c.poll();
        while c.live[&id].persistence_busy {
            c.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(c.live[&id].error.is_none(), "{:?}", c.live[&id].error);
        let saved = c.runtime.block_on(storage::load_session(&id)).unwrap();
        assert_eq!(saved.messages[1].content, "first second");
        assert!(c.live[&id].run_id.is_none());
    }
    #[test]
    fn poll_batches_256_and_preserves_native_error_completion() {
        let mut c = controller();
        let id = c.selected.clone().unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages=vec![decode(&json!({"id":"a","role":"assistant","content":"","timestamp":1,"runId":"r","isStreaming":true})).unwrap()];
        live.run_id = Some("r".into());
        live.message_id = Some("a".into());
        for _ in 0..300 {
            c.queue
                .emit(
                    "agent-event",
                    json!({"type":"chunk","data":{"sessionId":id,"runId":"r","text":"x"}}),
                )
                .unwrap();
        }
        c.poll();
        assert_eq!(
            c.model.current.as_ref().unwrap().messages[0].content.len(),
            256
        );
        c.poll();
        assert_eq!(
            c.model.current.as_ref().unwrap().messages[0].content.len(),
            300
        );
    }
    #[test]
    fn generic_full_load_uses_real_revision_instead_of_synthetic_empty_cursor() {
        let mut c = controller();
        let mut data = c.model.current.clone().unwrap();
        data.messages =
            vec![decode(&json!({"id":"u","role":"user","content":"saved","timestamp":1})).unwrap()];
        let id = data.session.id.clone();
        c.runtime.block_on(storage::save_session(data)).unwrap();
        c.new_session();
        let op = c.dispatch("load_session", json!({"sessionId":id}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.model.results.contains_key(&op) {
            c.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !c.model.window.as_ref().unwrap().revision.is_empty(),
            "full load must not manufacture an unusable revision"
        );
    }
}

#[cfg(test)]
mod race_tests {
    use super::*;
    pub(super) fn controller() -> Controller {
        super::tests::controller()
    }
    #[test]
    fn search_preview_does_not_cancel_an_inflight_open() {
        let mut c = controller();
        let original = c.selected.clone().unwrap();
        let data = new_data("");
        let id = data.session.id.clone();
        c.runtime.block_on(storage::save_session(data)).unwrap();
        c.open_session(&id);
        let generation = c.generation;
        c.dispatch(
            "load_session_around",
            json!({"sessionId":id,"messageId":"missing","limit":10}),
        );
        assert_eq!(
            generation, c.generation,
            "search must share the pending open navigation generation"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.pending.is_empty() {
            c.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!c.model.loading);
        assert_ne!(c.selected.as_deref(), Some(original.as_str()));
    }
    #[test]
    fn review_event_leaves_acknowledged_baseline_immutable_until_refresh_ack() {
        let mut c = controller();
        let id = c.selected.clone().unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages=vec![decode(&json!({"id":"a","role":"assistant","content":"answer","timestamp":1,"runId":"r","runStatus":"completed"})).unwrap()];
        live.committed = Some(live.data.clone());
        let baseline = json!(live.committed);
        c.background(&BackendEvent {topic:"agent:self-improvement".into(),payload:json!({"run_id":"r","session_id":id,"action":"no_action","summary":"review complete"})});
        assert_eq!(
            json!(c.live[&id].committed),
            baseline,
            "only native refresh ack may change committed baseline"
        );
        assert!(c.live[&id].data.messages[0].self_improvement.is_some());
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;
    #[test]
    fn external_page_save_reserves_session_until_ack() {
        let mut c = super::tests::controller();
        let id = c.selected.clone().unwrap();
        let data = c.model.current.clone().unwrap();
        let window = c
            .runtime
            .block_on(save_snapshot(&c.backend, &data, None))
            .unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.window = Some(window.clone());
        live.committed = Some(data.clone());
        let op = c.dispatch("save_session_page", json!({"data":data,"window":window}));
        assert!(
            c.live[&id].persistence_busy,
            "external page writes must block send until their revision is acknowledged"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.model.results.contains_key(&op) {
            c.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!c.live[&id].persistence_busy);
        assert!(c.model.results[&op].error.is_none());
    }
    #[test]
    fn background_review_outside_loaded_suffix_refreshes_without_clobbering_stream() {
        let mut c = super::tests::controller();
        let id = c.selected.clone().unwrap();
        let mut data = c.model.current.clone().unwrap();
        data.messages=(0..60).map(|n|decode(&json!({"id":format!("m{n}"),"role":"assistant","content":"answer","timestamp":n,"runId":format!("r{n}"),"runStatus":"completed"})).unwrap()).collect();
        c.runtime.block_on(storage::save_session(data)).unwrap();
        let page: SessionPage = decode(
            &c.runtime
                .block_on(
                    c.backend
                        .dispatch("load_session_page", json!({"sessionId":id,"limit":10})),
                )
                .unwrap(),
        )
        .unwrap();
        c.live
            .insert(id.clone(), LiveSession::new(page.data, Some(page.window)));
        let review: SelfImprovementEvent =
            decode(&json!({"run_id":"r0","session_id":id,"action":"no_action","summary":"review"}))
                .unwrap();
        // Native events must be post-commit.
        let mut canonical = c.runtime.block_on(storage::load_session(&id)).unwrap();
        canonical.messages[0].self_improvement = Some(review.clone());
        c.runtime
            .block_on(storage::save_session(canonical))
            .unwrap();
        c.background(&BackendEvent {
            topic: "agent:self-improvement".into(),
            payload: json!(review),
        });
        c.poll();
        let deadline = Instant::now() + Duration::from_secs(5);
        while c.live[&id].persistence_busy {
            c.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(c.live[&id].error.is_none(), "{:?}", c.live[&id].error);
        assert_eq!(c.live[&id].data.messages.len(), 10);
    }
}

#[cfg(test)]
mod final_regressions {
    use super::*;
    #[test]
    fn selecting_artifact_through_public_model_survives_poll() {
        let mut c = super::tests::controller();
        c.model_mut().selected_artifact_id = Some("picked".into());
        c.poll();
        assert_eq!(c.model().selected_artifact_id.as_deref(), Some("picked"));
    }
    #[test]
    fn full_checkpoint_cannot_replace_a_partial_suffix() {
        let mut c = super::tests::controller();
        let id = c.selected.clone().unwrap();
        let data = c.model.current.clone().unwrap();
        let window = c
            .runtime
            .block_on(save_snapshot(&c.backend, &data, None))
            .unwrap();
        c.live.get_mut(&id).unwrap().window = Some(window);
        let op = c.dispatch("checkpoint_session", json!({"data":data}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.model.results.contains_key(&op) {
            c.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            c.model.results[&op]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("use typed Controller methods")),
            "full checkpoint must require typed lifecycle ownership, not persist a viewport snapshot"
        );
    }
}

#[cfg(test)]
mod completion_regression {
    use super::*;
    #[test]
    fn unsuccessful_done_without_error_event_reports_failed_controller_phase() {
        let mut c = super::tests::controller();
        let id = c.selected.clone().unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages=vec![decode(&json!({"id":"a","role":"assistant","content":"","timestamp":1,"runId":"r","isStreaming":true})).unwrap()];
        live.run_id = Some("r".into());
        live.message_id = Some("a".into());
        reducer::apply(
            live,
            "done",
            &json!({"sessionId":id,"runId":"r","cancelled":false,"successful":false}),
        )
        .unwrap();
        assert!(
            live.failed,
            "unsuccessful terminal event must not become completed phase"
        );
    }
}

#[cfg(test)]
mod shutdown_regression {
    use super::*;
    #[test]
    fn dropping_full_controller_disconnects_backpressured_producer() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let c = super::tests::controller();
            let queue = c.queue.clone();
            for _ in 0..crate::EVENT_CAPACITY {
                queue.emit("irrelevant", Value::Null).unwrap();
            }
            c.runtime.spawn(async move {
                let _ = queue.emit("terminal", Value::Null);
            });
            std::thread::sleep(Duration::from_millis(30));
            drop(c);
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(Duration::from_secs(2)).is_ok(),
            "shutdown waited forever on a full event queue"
        );
    }
}

#[cfg(test)]
mod settings_regression {
    use super::*;
    #[test]
    fn settings_roundtrip_and_full_load_results_keep_native_schema() {
        let mut c = super::tests::controller();
        let settings: AgentSettings = decode(&json!({"providers":[{"id":"fixture","kind":"openai","baseUrl":"http://127.0.0.1:9/v1","model":"fixture"}],"activeProviderId":"fixture","temperature":0.2,"self_improvement_enabled":false})).unwrap();
        let op = c.dispatch("save_settings", json!({"settings":settings}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.model.results.contains_key(&op) {
            c.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(c.model.settings.temperature, 0.2);
        let data = c.model.current.clone().unwrap();
        let id = data.session.id.clone();
        c.runtime.block_on(storage::save_session(data)).unwrap();
        c.new_session();
        let op = c.dispatch("load_session", json!({"sessionId":id}));
        while !c.model.results.contains_key(&op) {
            c.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            c.model.results[&op].value.as_ref().unwrap()["session"]["id"],
            id
        );
        assert!(!c.model.window.as_ref().unwrap().revision.is_empty());
    }
}

#[cfg(test)]
mod lifecycle_hardening_tests {
    use super::*;
    fn configured() -> Controller {
        let mut c = super::tests::controller();
        c.model.settings = decode(&json!({"providers":[{"id":"local","baseUrl":"http://127.0.0.1:9/v1","model":"unused"}],"activeProviderId":"local","self_improvement_enabled":false})).unwrap();
        c
    }
    fn drain(c: &mut Controller) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.pending.is_empty() {
            c.poll();
            assert!(
                Instant::now() < deadline,
                "pending operation timed out: {:?}",
                c.model.error
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn dispatched_mutation_blocks_send_before_ack() {
        for command in [
            "compact_session",
            "delete_session",
            "toggle_pin_session",
            "rename_session",
            "rename_session_summary",
        ] {
            let mut c = configured();
            let id = c.selected.clone().unwrap();
            let data = c.live[&id].data.clone();
            c.runtime.block_on(storage::save_session(data)).unwrap();
            c.dispatch(command, json!({"sessionId":id,"title":"renamed"}));
            assert!(
                c.send("must wait".into()).is_err(),
                "{command} did not reserve its session"
            );
            drain(&mut c);
        }
    }
    #[test]
    fn inactive_cache_is_capped_at_six() {
        let mut c = configured();
        for _ in 0..12 {
            c.new_session();
        }
        assert!(
            c.live.len() <= 7,
            "only six inactive sessions plus selection may be retained"
        );
    }
    #[test]
    fn retry_save_preserves_dirty_turn_and_unblocks_only_after_ack() {
        let mut c = configured();
        let id = c.selected.clone().unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages = vec![
            decode(&json!({"id":"u","role":"user","content":"keep user","timestamp":1})).unwrap(),
            decode(
                &json!({"id":"a","role":"assistant","content":"","timestamp":2,"isStreaming":true}),
            )
            .unwrap(),
        ];
        live.changed();
        live.failure_phase = Some("initial_save".into());
        assert!(c.live[&id].dirty);
        assert!(c.send("blocked".into()).is_err());
        c.retry_save().unwrap();
        assert!(c.live[&id].dirty, "retry is not a save acknowledgment");
        assert!(c.live[&id].persistence_busy);
        drain(&mut c);
        assert!(!c.live[&id].dirty);
        assert_eq!(c.live[&id].data.messages.len(), 2);
        assert_eq!(c.live[&id].data.messages[1].is_streaming, Some(false));
        assert!(c.send("next turn".into()).is_ok());
        c.abort().unwrap();
        drain(&mut c);
    }
    #[test]
    fn shutdown_cancels_preparing_run_and_saves_before_ready() {
        let mut c = configured();
        let id = c.selected.clone().unwrap();
        c.send("shutdown turn".into()).unwrap();
        c.begin_shutdown();
        assert!(c.is_shutting_down());
        assert!(c.send("forbidden".into()).is_err());
        assert!(!c.shutdown_ready());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.shutdown_ready() {
            c.poll();
            assert!(
                Instant::now() < deadline,
                "shutdown stalled: {:?}",
                c.model.error
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let saved = c.runtime.block_on(storage::load_session(&id)).unwrap();
        assert_eq!(saved.messages[1].run_status.as_deref(), Some("cancelled"));
    }
    fn initial_failure(c: &mut Controller, phase: &str) -> String {
        let id = c.selected.clone().unwrap();
        let previous = c.live[&id].data.clone();
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages = vec![decode(&json!({"id":"u","role":"user","content":"restore prompt","timestamp":1})).unwrap(), decode(&json!({"id":"a","role":"assistant","content":"","timestamp":2,"runId":"r","isStreaming":true})).unwrap()];
        live.run_id = Some("r".into());
        live.changed();
        live.persistence_busy = true;
        let version = live.version;
        c.next_operation += 1;
        let opid = c.next_operation;
        c.pending.insert(
            opid,
            Pending {
                args: Value::Null,
                kind: Operation::Initial {
                    requests: vec![],
                    previous,
                    draft: "restore prompt".into(),
                    attachments: vec![],
                    run_id: "r".into(),
                    version,
                },
            },
        );
        c.complete(Completion {
            opid,
            command: "controller:prepare".into(),
            session_id: Some(id.clone()),
            generation: c.generation,
            result: Err("fixture failure".into()),
            failure_phase: Some(phase.into()),
        });
        c.publish();
        id
    }
    #[test]
    fn only_proven_prewrite_failure_restores_draft() {
        let mut c = configured();
        let id = initial_failure(&mut c, "attachment_preparation");
        assert!(c.live[&id].data.messages.is_empty());
        assert!(!c.live[&id].dirty);
        assert_eq!(c.model.draft, "restore prompt");
        assert_eq!(
            c.model.failure_phase.as_deref(),
            Some("attachment_preparation")
        );
        let mut c = configured();
        let id = initial_failure(&mut c, "initial_save");
        assert_eq!(c.live[&id].data.messages.len(), 2);
        assert!(c.model.retry_save_available);
        assert!(c.live[&id].dirty);
        assert!(!c.schedule(), "failed saves need explicit retry");
        c.retry_save().unwrap();
        drain(&mut c);
        assert!(!c.live[&id].dirty);
        assert!(c.send("new user turn".into()).is_ok());
        c.abort().unwrap();
        drain(&mut c);
    }
    #[test]
    fn compaction_ack_after_navigation_cannot_touch_new_selection_or_newer_origin() {
        for newer in [false, true] {
            let mut c = configured();
            let id = c.selected.clone().unwrap();
            let data = c.live[&id].data.clone();
            let live = c.live.get_mut(&id).unwrap();
            let origin = Some((live.identity.clone(), live.version, live.run_id.clone()));
            live.mutation_busy = Some(99);
            c.mutations.insert(id.clone(), 99);
            c.pending.insert(
                99,
                Pending {
                    args: json!({"sessionId":id}),
                    kind: Operation::Mutation {
                        origin,
                        external_page: false,
                    },
                },
            );
            c.new_session();
            let selected = c.selected.clone();
            let current = json!(c.model.current);
            if newer {
                c.live.get_mut(&id).unwrap().version += 1;
            }
            c.queue.emit("agent-event", json!({"type":"chunk","data":{"sessionId":id,"runId":"foreign","text":"ignored"}})).unwrap();
            c.complete(Completion {
                opid: 99,
                command: "compact_session".into(),
                session_id: Some(id.clone()),
                generation: 0,
                result: Ok(json!(data)),
                failure_phase: None,
            });
            c.poll();
            assert_eq!(c.selected, selected);
            assert_eq!(json!(c.model.current), current);
            assert_eq!(
                c.live.contains_key(&id),
                newer,
                "never remove a newer origin"
            );
        }
    }
    #[test]
    fn byte_budget_counts_nested_payloads_and_committed_copy_but_skips_hot_runs() {
        let nested = json!({"map":{"strings":["a\nb", "unicode α"]}});
        assert_eq!(
            counted_bytes(&nested),
            serde_json::to_vec(&nested).unwrap().len()
        );
        let mut c = configured();
        let id = c.selected.clone().unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages = vec![decode(&json!({"id":"large","role":"assistant","content":"x".repeat(17 * 1024 * 1024),"timestamp":1})).unwrap()];
        live.committed = Some(live.data.clone());
        live.run_id = Some("hot".into());
        c.new_session();
        assert!(c.live.contains_key(&id));
        assert!(
            c.live[&id].bytes_version.is_none(),
            "hot data must never be scanned"
        );
        c.live.get_mut(&id).unwrap().run_id = None;
        c.evict();
        assert!(
            !c.live.contains_key(&id),
            "two 17MiB snapshots exceed the 32MiB budget"
        );
        let id = c.selected.clone().unwrap();
        c.new_session();
        let measured = c.live[&id].bytes_version;
        assert!(measured.is_some());
        c.live.get_mut(&id).unwrap().bytes_estimate = 123;
        c.evict();
        assert_eq!(
            c.live[&id].bytes_estimate, 123,
            "unchanged versions must reuse measurement"
        );
    }
    #[test]
    fn deadline_fallback_checkpoints_without_claiming_early_success() {
        let mut c = configured();
        let id = c.selected.clone().unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages = vec![decode(&json!({"id":"a","role":"assistant","content":"partial","timestamp":1,"isStreaming":true,"runId":"orphan"})).unwrap()];
        live.run_id = Some("orphan".into());
        live.changed();
        c.begin_shutdown();
        c.shutdown_started = Some(Instant::now() - Duration::from_secs(6));
        assert!(!c.shutdown_ready());
        assert!(c.shutdown_timed_out());
        c.poll();
        assert!(c.shutdown_fallback);
        drain(&mut c);
        assert!(c.shutdown_ready());
        let saved = c.runtime.block_on(storage::load_session(&id)).unwrap();
        assert_eq!(saved.messages[0].content, "partial");
        assert_eq!(saved.messages[0].is_streaming, Some(false));
    }
    #[test]
    fn async_context_drop_does_not_panic_or_wait_on_runtime() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let c = configured();
            let host = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            host.block_on(async move {
                drop(c);
            });
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok());
    }
    #[test]
    fn actual_image_preparation_failure_restores_only_unsaved_draft() {
        let mut c = configured();
        let id = c.selected.clone().unwrap();
        let path = std::env::temp_dir().join(format!("absent-{}.png", uuid::Uuid::new_v4()));
        c.model.attachments = vec![decode(
            &json!({"id":"missing-image","name":"missing.png","path":path.to_string_lossy(),"isDir":false}),
        )
        .unwrap()];
        c.send("restore image prompt".into()).unwrap();
        drain(&mut c);
        assert_eq!(
            c.model.failure_phase.as_deref(),
            Some("attachment_preparation")
        );
        assert_eq!(c.model.draft, "restore image prompt");
        assert_eq!(c.model.attachments.len(), 1);
        assert!(c.live[&id].data.messages.is_empty());
        assert!(!c.live[&id].dirty);
        assert!(c.runtime.block_on(storage::load_session(&id)).is_err());
    }
    #[test]
    fn postcanonical_initial_failure_keeps_turn_for_retry_without_duplicate_user() {
        let mut c = configured();
        let id = initial_failure(&mut c, "initial_save");
        // The canonical write succeeded but cursor/read acknowledgment failed.
        c.runtime
            .block_on(storage::save_session(c.live[&id].data.clone()))
            .unwrap();
        assert!(c.live[&id].dirty);
        assert_eq!(c.live[&id].data.messages.len(), 2);
        c.retry_save().unwrap();
        drain(&mut c);
        let saved = c.runtime.block_on(storage::load_session(&id)).unwrap();
        assert_eq!(
            saved.messages.iter().filter(|m| m.role == "user").count(),
            1
        );
        assert_eq!(saved.messages[1].is_streaming, Some(false));
    }
    #[test]
    fn save_failure_without_status_requires_explicit_retry() {
        let mut c = configured();
        let id = c.selected.clone().unwrap();
        let data = c.live[&id].data.clone();
        let mut window = c
            .runtime
            .block_on(save_snapshot(&c.backend, &data, None))
            .unwrap();
        let valid = window.clone();
        window.revision = "stale-revision".into();
        let live = c.live.get_mut(&id).unwrap();
        live.window = Some(window);
        live.committed = Some(data);
        live.changed();
        c.schedule();
        drain(&mut c);
        assert_eq!(c.model.failure_phase.as_deref(), Some("final_save"));
        assert!(c.live[&id].dirty);
        assert!(!c.schedule(), "failure must not busy-loop writes");
        c.live.get_mut(&id).unwrap().window = Some(valid);
        c.retry_save().unwrap();
        assert!(c.live[&id].dirty);
        drain(&mut c);
        assert!(!c.live[&id].dirty);
    }
}

#[cfg(test)]
mod history_completion_tests {
    use super::*;
    #[test]
    fn known_postcanonical_failure_keeps_trimmed_dirty_data_without_fake_success() {
        let mut c = super::tests::controller();
        let id = c.selected.clone().unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages =
            vec![
                decode(&json!({"id":"u","role":"user","content":"original","timestamp":1}))
                    .unwrap(),
            ];
        live.committed = Some(live.data.clone());
        live.persistence_busy = true;
        let mut trimmed = live.data.clone();
        trimmed.messages.clear();
        let outcome = json!({"data":trimmed,"window":null,"replay":null,"resend":false,"canonicalCommitted":true,"acknowledgmentError":"History canonical commit succeeded; acknowledgment failed: projection unavailable","identity":live.identity,"version":live.version,"runId":null});
        let result = CommandResult {
            id: 99,
            command: crate::history::HISTORY_COMMAND.into(),
            session_id: Some(id.clone()),
            generation: c.generation,
            value: Some(outcome.clone()),
            error: None,
            failure_phase: None,
        };
        c.model.results.insert(99, result.clone());
        c.model.last_command = Some(result);
        c.complete_history(99, Some(&id), c.generation, Ok(outcome));
        c.poll();
        assert!(c.live[&id].dirty);
        assert!(c.live[&id].data.messages.is_empty());
        assert_eq!(c.live[&id].committed.as_ref().unwrap().messages.len(), 1);
        assert_eq!(c.live[&id].failure_phase.as_deref(), Some("final_save"));
        assert!(!c.live[&id].persistence_busy);
        assert!(
            !c.schedule(),
            "post-write failures require explicit recovery"
        );
        assert!(c.model.results[&99].error.is_some());
        assert!(c.model.results[&99].value.is_none());
        assert!(c.model.last_command.as_ref().unwrap().error.is_some());
    }
    #[test]
    fn replay_images_require_matching_actual_attachments_and_merge_new_images() {
        let directory = tempfile::tempdir().unwrap();
        let fresh = directory.path().join("fresh.png");
        std::fs::write(&fresh, b"\x89PNG\r\n\x1a\nfresh").unwrap();
        let attachments: Vec<AttachedContext> = decode(&json!([
            {"id":"old","name":"deleted.png","path":"deleted.png","isDir":false},
            {"id":"fresh","name":"fresh.png","path":fresh.to_string_lossy(),"isDir":false}
        ]))
        .unwrap();
        let replay: Vec<ImageInput> = decode(&json!([
            {"sourcePath":"deleted.png","mediaType":"image/png","data":"iVBORw0KGgo="},
            {"sourcePath":"not-attached.png","mediaType":"image/png","data":"iVBORw0KGgo="}
        ]))
        .unwrap();
        let prepared = prepare_replay_images(&attachments, &replay).unwrap();
        assert_eq!(prepared.len(), 2);
        assert_eq!(prepared[0].source_path, "deleted.png");
        assert!(prepared.iter().all(|i| i.source_path != "not-attached.png"));
        assert_eq!(prepared[1].source_path, fresh.to_string_lossy());
    }
}

#[cfg(test)]
mod shutdown_decision_tests {
    use super::*;
    fn expired(c: &mut Controller) {
        c.shutdown_started = Some(Instant::now() - Duration::from_secs(6));
        c.shutdown_fallback = true;
    }
    fn drain(c: &mut Controller) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.pending.is_empty() {
            c.poll();
            assert!(
                Instant::now() < deadline,
                "save did not acknowledge: {:?}",
                c.model.error
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn shutdown_retry_scans_inactive_dirty_sessions_and_waits_for_ack() {
        let mut c = super::tests::controller();
        assert!(c.retry_shutdown_saves().is_err());
        let first = c.selected.clone().unwrap();
        for text in ["inactive unsaved", "selected unsaved"] {
            let id = c.selected.clone().unwrap();
            let live = c.live.get_mut(&id).unwrap();
            live.data.messages =
                vec![decode(&json!({"id":id,"role":"user","content":text,"timestamp":1})).unwrap()];
            live.changed();
            live.failure_phase = Some("final_save".into());
            if text == "inactive unsaved" {
                c.new_session();
            }
        }
        let second = c.selected.clone().unwrap();
        expired(&mut c);
        c.retry_shutdown_save().unwrap();
        assert!(!c.shutdown_fallback);
        assert!(!c.shutdown_timed_out());
        for id in [&first, &second] {
            assert!(c.live[id].dirty);
            assert!(c.live[id].persistence_busy);
            assert!(c.live[id].failure_phase.is_none());
        }
        assert!(!c.shutdown_ready());
        assert!(c.retry_shutdown_saves().is_err());
        assert!(
            !c.cancel_shutdown(),
            "cannot resume through an active commit"
        );
        drain(&mut c);
        assert!(c.shutdown_ready());
        for (id, text) in [(&first, "inactive unsaved"), (&second, "selected unsaved")] {
            assert_eq!(
                c.runtime
                    .block_on(storage::load_session(id))
                    .unwrap()
                    .messages[0]
                    .content,
                text
            );
            assert!(!c.live[id].dirty);
        }
    }
    #[test]
    fn keep_open_never_schedules_new_work_or_discards_dirty_data() {
        let mut c = super::tests::controller();
        let id = c.selected.clone().unwrap();
        let live = c.live.get_mut(&id).unwrap();
        live.changed();
        live.failure_phase = Some("final_save".into());
        live.cancelled.store(true, Ordering::Release);
        expired(&mut c);
        c.live.get_mut(&id).unwrap().persistence_busy = true;
        assert!(!c.resume_after_shutdown_failure());
        assert!(c.retry_shutdown_saves().is_err());
        c.live.get_mut(&id).unwrap().persistence_busy = false;
        c.model.draft = "keep draft".into();
        assert!(c.resume_after_shutdown_failure());
        assert!(!c.is_shutting_down());
        assert!(!c.shutdown_fallback);
        assert!(c.pending.is_empty());
        assert!(c.live[&id].dirty);
        assert_eq!(c.live[&id].failure_phase.as_deref(), Some("final_save"));
        assert_eq!(c.model.draft, "keep draft");
        assert!(!c.live[&id].cancelled.load(Ordering::Acquire));
    }
    #[test]
    fn shutdown_retry_never_rebases_a_conflicting_canonical_cursor() {
        let mut c = super::tests::controller();
        let id = c.selected.clone().unwrap();
        let mut data = c.live[&id].data.clone();
        data.messages =
            vec![
                decode(&json!({"id":"u","role":"user","content":"baseline","timestamp":1}))
                    .unwrap(),
            ];
        let window = c
            .runtime
            .block_on(save_snapshot(&c.backend, &data, None))
            .unwrap();
        let original_revision = window.revision.clone();
        let live = c.live.get_mut(&id).unwrap();
        live.data = data.clone();
        live.committed = Some(data.clone());
        live.window = Some(window);
        live.data.messages[0].content = "my unsaved edit".into();
        live.changed();
        live.failure_phase = Some("final_save".into());
        data.messages[0].content = "external canonical edit".into();
        c.runtime.block_on(storage::save_session(data)).unwrap();
        expired(&mut c);
        c.retry_shutdown_saves().unwrap();
        drain(&mut c);
        assert!(!c.shutdown_ready());
        assert!(c.live[&id].dirty);
        assert_eq!(
            c.live[&id].window.as_ref().unwrap().revision,
            original_revision
        );
        assert_eq!(c.live[&id].data.messages[0].content, "my unsaved edit");
        assert_eq!(
            c.live[&id].committed.as_ref().unwrap().messages[0].content,
            "baseline"
        );
        assert_eq!(c.live[&id].failure_phase.as_deref(), Some("final_save"));
        assert_eq!(
            c.runtime
                .block_on(storage::load_session(&id))
                .unwrap()
                .messages[0]
                .content,
            "external canonical edit"
        );
    }
}
#[cfg(test)]
mod generic_lifecycle_guard_tests {
    use super::*;

    fn idle_saved() -> (Controller, String, SessionData) {
        let mut c = super::tests::controller();
        let id = c.selected.clone().unwrap();
        let mut data = c.live[&id].data.clone();
        data.messages = vec![decode(&json!({"id":"baseline","role":"assistant","content":"committed answer","timestamp":1,"runId":"finished-run","isStreaming":false})).unwrap()];
        data.session.message_count = 1;
        // Direct backend clients must retain the native lifecycle commands.
        c.runtime
            .block_on(c.backend.dispatch("save_session", json!({"data":data})))
            .unwrap();
        c.live
            .insert(id.clone(), LiveSession::new(data.clone(), None));
        c.published = None;
        c.publish();
        (c, id, data)
    }

    fn native(c: &Controller, id: &str) -> Value {
        c.runtime
            .block_on(c.backend.dispatch("load_session", json!({"sessionId":id})))
            .unwrap()
    }

    fn assert_rejected(c: &mut Controller, id: &str, command: &str, args: Value) {
        let before = native(c, id);
        let state = json!({"data":c.live[id].data,"committed":c.live[id].committed,"window":c.live[id].window});
        let version = c.live[id].version;
        let generation = c.generation;
        let op = c.dispatch(command, args);
        // Check synchronously, before poll can consume an asynchronous completion.
        assert!(
            !c.mutations.contains_key(id),
            "{command} reserved an idle session before rejection"
        );
        assert!(
            c.live[id].mutation_busy.is_none(),
            "{command} reserved the live cache"
        );
        assert!(
            !c.live[id].persistence_busy,
            "{command} started persistence"
        );
        assert!(
            matches!(c.pending[&op].kind, Operation::Generic),
            "{command} queued backend lifecycle work"
        );
        assert_eq!(
            native(c, id),
            before,
            "{command} changed native history before poll"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.model.results.contains_key(&op) {
            c.poll();
            assert!(Instant::now() < deadline, "{command} completion timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        let result = &c.model.results[&op];
        let error = result
            .error
            .as_deref()
            .expect("raw lifecycle command must fail");
        assert!(
            error.contains("use typed Controller methods"),
            "{command}: {error}"
        );
        assert!(
            error.contains(command),
            "rejection must identify {command}: {error}"
        );
        assert!(
            result.value.is_none(),
            "rejection must not report fake success"
        );
        assert_eq!(
            native(c, id),
            before,
            "{command} changed native history after rejection"
        );
        assert_eq!(
            json!({"data":c.live[id].data,"committed":c.live[id].committed,"window":c.live[id].window}),
            state
        );
        assert_eq!(c.live[id].version, version);
        assert_eq!(c.generation, generation);
        assert!(!c.live[id].protected());
    }

    #[test]
    fn raw_save_rejects_new_messages_in_selected_and_inactive_idle_cache() {
        for inactive in [false, true] {
            let (mut c, id, mut data) = idle_saved();
            if inactive {
                c.new_session();
            }
            data.messages.push(decode(&json!({"id":"injected","role":"user","content":"must not persist","timestamp":2})).unwrap());
            data.session.message_count = 2;
            assert_rejected(&mut c, &id, "save_session", json!({"data":data}));
        }
    }

    fn assert_stale_checkpoint_rejected(command: &str) {
        let (mut c, id, mut data) = idle_saved();
        data.messages[0].content = "stale run overwrite".into();
        data.messages[0].is_streaming = Some(true);
        let page = c
            .runtime
            .block_on(
                c.backend
                    .dispatch("load_session_page", json!({"sessionId":id,"limit":50})),
            )
            .unwrap();
        let args = if command == "checkpoint_session" {
            json!({"data":data})
        } else {
            json!({"sessionId":id,"request":{"runId":"finished-run","baseRevision":page["window"]["revision"],"sequence":1,"messages":data.messages,"artifacts":[]}})
        };
        assert_rejected(&mut c, &id, command, args);
    }

    #[test]
    fn raw_checkpoint_cannot_write_a_stale_run() {
        assert_stale_checkpoint_rejected("checkpoint_session");
    }

    #[test]
    fn raw_delta_checkpoint_cannot_write_a_stale_run() {
        assert_stale_checkpoint_rejected("checkpoint_session_delta");
    }

    #[test]
    fn raw_lifecycle_names_reject_before_argument_validation() {
        let (mut c, id, data) = idle_saved();
        for command in [
            "save_session",
            "checkpoint_session",
            "checkpoint_session_delta",
            "refresh_session_window",
            "replace_session_checked",
        ] {
            assert_rejected(&mut c, &id, command, json!({}));
            assert_rejected(
                &mut c,
                &id,
                command,
                json!({"sessionId":"mismatched","data":data}),
            );
        }
    }

    #[test]
    fn raw_refresh_cannot_blindly_advance_the_committed_cursor() {
        let (mut c, id, data) = idle_saved();
        let page: SessionPage = decode(
            &c.runtime
                .block_on(
                    c.backend
                        .dispatch("load_session_page", json!({"sessionId":id,"limit":50})),
                )
                .unwrap(),
        )
        .unwrap();
        c.live.insert(
            id.clone(),
            LiveSession::new(data.clone(), Some(page.window.clone())),
        );
        let mut external = data.clone();
        external.session.title = "externally renamed".into();
        c.runtime
            .block_on(c.backend.dispatch("save_session", json!({"data":external})))
            .unwrap();
        assert_rejected(
            &mut c,
            &id,
            "refresh_session_window",
            json!({"sessionId":id,"data":data,"window":page.window,"allowMetadata":true}),
        );
    }

    #[test]
    fn raw_checked_replacement_requires_typed_methods_even_without_a_session_id() {
        let (mut c, id, data) = idle_saved();
        assert_rejected(&mut c, &id, "replace_session_checked", json!({"data":data}));
        assert_rejected(&mut c, &id, "replace_session_checked", json!({}));
    }

    #[test]
    fn typed_send_initial_save_and_internal_checkpoint_still_work() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let n = socket.read(&mut buffer).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buffer[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"typed answer\"}}]}\n\n").unwrap();
            socket.flush().unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(10));
            let _ = socket.write_all(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n");
        });
        let mut c = super::tests::controller();
        c.model.settings = decode(&json!({"providers":[{"id":"local","kind":"openai","baseUrl":format!("http://{address}/v1"),"model":"fixture"}],"activeProviderId":"local","self_improvement_enabled":false})).unwrap();
        let id = c.selected.clone().unwrap();
        c.send_prompt("typed prompt".into()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while c.live[&id].data.messages.last().unwrap().content != "typed answer" {
            c.poll();
            assert!(
                Instant::now() < deadline,
                "typed stream failed: {:?}",
                c.model.error
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            c.live[&id].window.is_some(),
            "typed initial save must acknowledge a native cursor"
        );
        let baseline = json!(c.live[&id].committed);
        assert_eq!(native(&c, &id)["messages"][0]["content"], "typed prompt");
        c.live.get_mut(&id).unwrap().checkpoint_at = Instant::now() - Duration::from_secs(3);
        assert!(c.schedule());
        assert!(c
            .pending
            .values()
            .any(|p| matches!(p.kind, Operation::Checkpoint { .. })));
        while c.live[&id].persistence_busy {
            c.poll();
            assert!(Instant::now() < deadline, "typed checkpoint timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(c.live[&id].checkpoint_sequence, 1);
        assert_eq!(c.live[&id].checkpoint_version, c.live[&id].version);
        assert_eq!(json!(c.live[&id].committed), baseline);
        assert!(c.live[&id].error.is_none(), "{:?}", c.live[&id].error);
        release.send(()).unwrap();
        while c.live[&id].run_id.is_some() || !c.pending.is_empty() {
            c.poll();
            assert!(
                Instant::now() < deadline,
                "typed final save timed out: {:?}",
                c.model.error
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        server.join().unwrap();
        assert_eq!(native(&c, &id)["messages"][1]["content"], "typed answer");
        assert!(!c.live[&id].dirty);
    }
}

#[cfg(test)]
mod committed_cursor_actual_fault_tests {
    use super::*;
    fn poll_until(c: &mut Controller, predicate: impl Fn(&Controller) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            c.poll();
            if predicate(c) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out: {:?}", c.model.error);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn fixture(count: usize) -> (Controller, String, std::path::PathBuf) {
        let mut c = super::tests::controller();
        let mut data = c.model.current.clone().unwrap();
        data.session.id = format!("sess_{}", uuid::Uuid::new_v4());
        let id = data.session.id.clone();
        data.messages = (0..count).map(|n| decode(&json!({"id":format!("m{n}"),"role":if n%2==0 {"user"} else {"assistant"},"content":format!("content-{n}"),"timestamp":n})).unwrap()).collect();
        data.artifacts = vec![decode(&json!({"id":"hidden","type":"code","title":"prefix artifact","content":"preserve","timestamp":1})).unwrap()];
        c.runtime.block_on(storage::save_session(data)).unwrap();
        c.open_session(&id);
        poll_until(&mut c, |c| !c.model.loading && c.model.window.is_some());
        let path = storage::config_dir()
            .join("sessions")
            .join(format!("{id}.json"));
        (c, id, path)
    }
    fn fault(path: &std::path::Path) -> (rusqlite::Connection, String) {
        let conn =
            rusqlite::Connection::open(storage::config_dir().join("session-pages.sqlite")).unwrap();
        conn.busy_timeout(Duration::from_secs(2)).unwrap();
        let table = format!("fix3_state_fault_{}", uuid::Uuid::new_v4().simple());
        let key = path.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!(
            "CREATE TABLE {table}(enabled INTEGER); INSERT INTO {table} VALUES(1);
            CREATE TRIGGER {table}_trigger BEFORE INSERT ON page_sessions
            WHEN NEW.path='{key}' AND (SELECT enabled FROM {table})=1
            BEGIN SELECT RAISE(ABORT,'actual fix3 state SQLite fault'); END;"
        ))
        .unwrap();
        (conn, table)
    }

    #[test]
    fn actual_initial_postcommit_fault_recovers_before_terminal_save_without_provider_replay() {
        let mut c = super::tests::controller();
        c.model.settings = decode(&json!({"providers":[{"id":"local","kind":"openai","baseUrl":"http://127.0.0.1:1/v1","model":"fixture"}],"activeProviderId":"local","self_improvement_enabled":false})).unwrap();
        let id = c.selected.clone().unwrap();
        // Initialize only the projection schema using an unrelated fixture.
        let mut schema = c.model.current.clone().unwrap();
        schema.session.id = format!("schema_{}", uuid::Uuid::new_v4());
        c.runtime.block_on(storage::save_session(schema)).unwrap();
        let path = storage::config_dir()
            .join("sessions")
            .join(format!("{id}.json"));
        let (conn, table) = fault(&path);
        c.send_prompt("keep exactly one initial user".into())
            .unwrap();
        poll_until(&mut c, |c| {
            c.live[&id].recovery.is_some() && !c.live[&id].persistence_busy
        });
        assert_eq!(c.model.failure_phase.as_deref(), Some("initial_save"));
        assert!(c.live[&id].committed.is_none());
        assert!(c.live[&id].window.is_none());
        assert!(c.live[&id].dirty);
        let written: SessionData = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written.messages.len(), 2);
        assert_eq!(written.messages[1].is_streaming, Some(true));
        // Preparing failed before runner dispatch. Retry saves the locally closed
        // assistant after acknowledging the already-written canonical snapshot.
        conn.execute(&format!("UPDATE {table} SET enabled=0"), [])
            .unwrap();
        c.retry_save().unwrap();
        poll_until(&mut c, |c| {
            !c.live[&id].dirty && !c.live[&id].persistence_busy
        });
        let saved: SessionData = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            saved.messages.iter().filter(|m| m.role == "user").count(),
            1
        );
        assert_eq!(saved.messages[1].is_streaming, Some(false));
        assert!(c
            .model
            .results
            .values()
            .all(|r| r.command != "send_message"));
        let window = c.live[&id].window.clone().unwrap();
        c.runtime
            .block_on(storage::load_session_page(
                &id,
                Some(0),
                1,
                Some(window.revision),
            ))
            .unwrap();
    }
    #[test]
    fn every_history_action_recovers_only_the_committed_cas_snapshot() {
        use crate::history::HistoryAction;
        for action in [
            HistoryAction::Clear,
            HistoryAction::Rollback {
                message_id: "m3".into(),
            },
            HistoryAction::Retry {
                message_id: "m5".into(),
            },
            HistoryAction::Edit {
                message_id: "m4".into(),
                text: "replacement prompt".into(),
            },
            HistoryAction::UpdateContent {
                message_id: "m3".into(),
                expected: "content-3".into(),
                new: "edited answer".into(),
            },
        ] {
            let (mut c, id, path) = fixture(8);
            let (conn, table) = fault(&path);
            let op = c.history(action).unwrap();
            poll_until(&mut c, |c| c.model.results.contains_key(&op));
            assert!(c.model.results[&op].error.is_some());
            assert!(c.live[&id].recovery.is_some());
            let written = std::fs::read(&path).unwrap();
            let expected: SessionData = serde_json::from_slice(&written).unwrap();
            conn.execute(&format!("UPDATE {table} SET enabled=0"), [])
                .unwrap();
            c.retry_save().unwrap();
            poll_until(&mut c, |c| {
                !c.live[&id].dirty && !c.live[&id].persistence_busy
            });
            assert_eq!(std::fs::read(&path).unwrap(), written);
            assert_eq!(
                json!(c.live[&id].committed.as_ref().unwrap()),
                json!(expected)
            );
            assert_eq!(c.live[&id].window.as_ref().unwrap().offset, 0);
            assert!(c
                .model
                .results
                .values()
                .all(|r| r.command != "send_message"));
        }
    }
    #[test]
    fn paged_save_retries_real_cursor_before_newer_live_version_without_clobber() {
        let (mut c, id, path) = fixture(120);
        let old = c.live[&id].window.clone().unwrap();
        assert_eq!(old.offset, 70);
        let old_baseline = json!(c.live[&id].committed);
        let (conn, table) = fault(&path);
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages.last_mut().unwrap().content = "first commit".into();
        live.changed();
        c.schedule();
        poll_until(&mut c, |c| {
            c.live[&id].recovery.is_some() && !c.live[&id].persistence_busy
        });
        assert!(c.live[&id].dirty);
        assert_eq!(json!(c.live[&id].committed), old_baseline);
        assert_eq!(c.live[&id].window.as_ref().unwrap().revision, old.revision);
        assert!(c.model.results.values().any(|r| r
            .error
            .as_ref()
            .is_some_and(|e| storage::CommittedSaveError::parse(e).is_some())));
        let first = std::fs::read(&path).unwrap();
        let first_data: SessionData = serde_json::from_slice(&first).unwrap();
        assert_eq!(first_data.messages.last().unwrap().content, "first commit");
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages.last_mut().unwrap().content = "newer live text".into();
        live.changed();
        // Retry while the real fault remains: no fake ack, no busy loop.
        c.retry_save().unwrap();
        poll_until(&mut c, |c| {
            !c.live[&id].persistence_busy && c.live[&id].failure_phase.is_some()
        });
        assert!(c.live[&id].recovery.is_some());
        assert_eq!(std::fs::read(&path).unwrap(), first);
        conn.execute(&format!("UPDATE {table} SET enabled=0"), [])
            .unwrap();
        c.retry_save().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        // Inspect the actual recovery completion before schedule queues next save.
        while c.live[&id].recovery.is_some() {
            let event = c.receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            if event.topic == "controller:result" {
                c.complete(decode::<Completion>(&event.payload).unwrap());
            }
            assert!(Instant::now() < deadline);
        }
        assert_eq!(std::fs::read(&path).unwrap(), first);
        assert_eq!(
            c.live[&id]
                .committed
                .as_ref()
                .unwrap()
                .messages
                .last()
                .unwrap()
                .content,
            "first commit"
        );
        assert_eq!(
            c.live[&id].data.messages.last().unwrap().content,
            "newer live text"
        );
        assert!(
            c.live[&id].dirty,
            "recovery cannot acknowledge a newer version"
        );
        let revision = c.live[&id].window.as_ref().unwrap().revision.clone();
        c.runtime
            .block_on(storage::load_session_page(&id, Some(0), 1, Some(revision)))
            .unwrap();
        c.schedule();
        poll_until(&mut c, |c| {
            !c.live[&id].dirty && !c.live[&id].persistence_busy
        });
        let saved: SessionData = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.messages.len(), 120);
        assert_eq!(saved.messages[0].content, "content-0");
        assert_eq!(saved.messages.last().unwrap().content, "newer live text");
        assert_eq!(saved.artifacts[0].content, "preserve");
        assert_eq!(c.live[&id].window.as_ref().unwrap().offset, 70);
    }
    #[test]
    fn history_commit_fault_recovers_full_cursor_without_duplicate_replay() {
        let (mut c, id, path) = fixture(122);
        let (conn, table) = fault(&path);
        let op = c.undo().unwrap();
        poll_until(&mut c, |c| c.model.results.get(&op).is_some());
        assert!(c.model.results[&op].error.is_some());
        assert!(c.model.results[&op].value.is_none());
        assert!(c.live[&id].dirty);
        assert!(c.live[&id].recovery.is_some());
        assert_eq!(c.live[&id].data.messages.len(), 120);
        assert_eq!(c.model.draft, "content-120");
        let canonical = std::fs::read(&path).unwrap();
        let full: SessionData = serde_json::from_slice(&canonical).unwrap();
        assert_eq!(full.messages.len(), 120);
        conn.execute(&format!("UPDATE {table} SET enabled=0"), [])
            .unwrap();
        c.retry_save().unwrap();
        poll_until(&mut c, |c| {
            !c.live[&id].dirty && !c.live[&id].persistence_busy
        });
        assert_eq!(std::fs::read(&path).unwrap(), canonical);
        assert_eq!(c.live[&id].window.as_ref().unwrap().offset, 0);
        assert_eq!(c.live[&id].committed.as_ref().unwrap().messages.len(), 120);
        assert_eq!(c.model.draft, "content-120");
        let window = c.live[&id].window.clone().unwrap();
        c.runtime
            .block_on(storage::load_session_page(
                &id,
                Some(0),
                1,
                Some(window.revision),
            ))
            .unwrap();
    }
    #[test]
    fn same_id_external_prefix_edit_blocks_receipt_recovery_and_preserves_dirty_tail() {
        let (mut c, id, path) = fixture(120);
        let (conn, table) = fault(&path);
        let live = c.live.get_mut(&id).unwrap();
        live.data.messages.last_mut().unwrap().content = "my committed tail".into();
        live.changed();
        c.schedule();
        poll_until(&mut c, |c| c.live[&id].recovery.is_some());
        let live_before = json!(c.live[&id].data);
        let baseline = json!(c.live[&id].committed);
        let mut external: SessionData =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        external.messages[0].content = "external same-ID hidden prefix".into();
        let external = serde_json::to_vec_pretty(&external).unwrap();
        std::fs::write(&path, &external).unwrap();
        conn.execute(&format!("UPDATE {table} SET enabled=0"), [])
            .unwrap();
        c.retry_save().unwrap();
        poll_until(&mut c, |c| {
            !c.live[&id].persistence_busy && c.live[&id].failure_phase.is_some()
        });
        assert!(c.live[&id]
            .error
            .as_ref()
            .unwrap()
            .contains("Canonical changed after commit"));
        assert!(c.live[&id].dirty);
        assert!(c.live[&id].recovery.is_some());
        assert_eq!(json!(c.live[&id].data), live_before);
        assert_eq!(json!(c.live[&id].committed), baseline);
        assert_eq!(std::fs::read(&path).unwrap(), external);
        assert!(!c.schedule(), "conflict needs explicit action");
    }
}
