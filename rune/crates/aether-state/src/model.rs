use aether_core::agent::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunPhase {
    Preparing,
    WaitingProvider,
    ReceivingAnswer,
    RunningTool,
    Saving,
    Completed,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub run_id: String,
    pub session_id: String,
    pub phase: RunPhase,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub save_failed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub id: u64,
    pub command: String,
    pub session_id: Option<String>,
    pub generation: u64,
    pub value: Option<Value>,
    pub error: Option<String>,
    /// attachment_preparation, initial_save, or final_save; dirty turns are never discarded.
    #[serde(default)]
    pub failure_phase: Option<String>,
}

/// UI projection only. Canonical state and offscreen active runs belong to the
/// controller. `draft`, `attachments` and artifact selection may be edited by UI.
/// A successful provider run whose final canonical snapshot was acknowledged.
/// No prompt, answer, title or path leaves the controller through this channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedRun {
    pub session_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Model {
    pub settings: AgentSettings,
    pub sessions: Vec<Session>,
    pub current: Option<SessionData>,
    pub window: Option<SessionWindow>,
    /// Detached, read-only search window; never replaces the writable suffix.
    pub preview: Option<SessionPage>,
    pub draft: String,
    pub attachments: Vec<AttachedContext>,
    /// Canonical replay bytes, reused only for matching explicit attachment paths.
    pub replay_image_inputs: Vec<ImageInput>,
    pub busy: bool,
    pub loading: bool,
    pub loading_older: bool,
    pub error: Option<String>,
    /// attachment_preparation, initial_save, or final_save; dirty turns are never discarded.
    #[serde(default)]
    pub failure_phase: Option<String>,
    pub retry_save_available: bool,
    pub recoverable_draft: Option<String>,
    pub recoverable_attachments: Vec<AttachedContext>,
    pub workspace: String,
    pub selected_artifact_id: Option<String>,
    pub run_status: Option<RunStatus>,
    pub notice: Option<NoticeEventData>,
    pub subagents: Vec<SubagentProgress>,
    pub active_skills: Vec<SkillInfo>,
    pub pending_commands: BTreeMap<u64, String>,
    pub results: BTreeMap<u64, CommandResult>,
    pub last_command: Option<CommandResult>,
    /// Bounded non-chat event inbox (review/micro-compaction etc.).
    pub outbound: Vec<crate::BackendEvent>,
    /// Bounded native UI intents; hosts drain with mem::take on each poll.
    pub ui_actions: Vec<crate::commands::UiAction>,
    /// Bounded host inbox (32), including offscreen sessions. Hosts drain each poll.
    pub completed_runs: Vec<CompletedRun>,
}
/// Unacknowledged canonical bytes are distinct from a stale/conflicting cursor.
/// Only the native receipt can authorize compare-only acknowledgment recovery.
#[derive(Clone)]
pub(crate) enum SaveRecovery {
    CanonicalCommitted {
        proof: storage::CommittedSaveProof,
        version: u64,
    },
}
pub(crate) struct LiveSession {
    pub data: SessionData,
    /// Never changed by a stream or journal ack.
    pub committed: Option<SessionData>,
    pub recovery: Option<SaveRecovery>,
    pub window: Option<SessionWindow>,
    pub run_id: Option<String>,
    pub message_id: Option<String>,
    pub status: Option<RunStatus>,
    pub dirty: bool,
    pub persistence_busy: bool,
    /// Reservation owned by one dispatched mutation until its acknowledgment.
    pub mutation_busy: Option<u64>,
    pub identity: String,
    pub recovery_draft: Option<(String, Vec<AttachedContext>)>,
    pub bytes_version: Option<u64>,
    pub bytes_estimate: usize,
    pub loading_older: bool,
    pub failed: bool,
    pub done: bool,
    /// Retained across failed saves/recovery; consumed only by an acknowledged clean save.
    pub completion_pending: Option<String>,
    pub cancelled: Arc<AtomicBool>,
    pub error: Option<String>,
    /// attachment_preparation, initial_save, or final_save; dirty turns are never discarded.
    pub failure_phase: Option<String>,
    pub notice: Option<NoticeEventData>,
    pub subagents: Vec<SubagentProgress>,
    pub skills: Vec<SkillInfo>,
    pub selected_artifact: Option<String>,
    pub modified_artifacts: HashSet<String>,
    pub checkpoint_artifacts: HashMap<String, Value>,
    pub checkpoint_sequence: u64,
    pub checkpoint_at: Instant,
    pub version: u64,
    pub checkpoint_version: u64,
    pub touched: u64,
}
impl LiveSession {
    pub fn new(data: SessionData, window: Option<SessionWindow>) -> Self {
        Self {
            committed: window.as_ref().map(|_| data.clone()),
            recovery: None,
            data,
            window,
            run_id: None,
            message_id: None,
            status: None,
            dirty: false,
            persistence_busy: false,
            mutation_busy: None,
            identity: uuid::Uuid::new_v4().to_string(),
            recovery_draft: None,
            bytes_version: None,
            bytes_estimate: 0,
            loading_older: false,
            failed: false,
            done: false,
            completion_pending: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            error: None,
            failure_phase: None,
            notice: None,
            subagents: vec![],
            skills: vec![],
            selected_artifact: None,
            modified_artifacts: HashSet::new(),
            checkpoint_artifacts: HashMap::new(),
            checkpoint_sequence: 0,
            checkpoint_at: Instant::now(),
            version: 0,
            checkpoint_version: 0,
            touched: 0,
        }
    }
    pub fn take_completion(&mut self) -> Option<CompletedRun> {
        if self.dirty || self.persistence_busy || self.recovery.is_some() || !self.done
            || self.failed || self.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        self.completion_pending.take().map(|run_id| CompletedRun {
            session_id: self.data.session.id.clone(), run_id,
        })
    }
    pub fn changed(&mut self) {
        self.version += 1;
        self.dirty = true;
    }
    pub fn protected(&self) -> bool {
        self.run_id.is_some()
            || self.dirty
            || self.recovery.is_some()
            || self.persistence_busy
            || self.loading_older
            || self.mutation_busy.is_some()
    }
}

pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn new_data(workspace: &str) -> SessionData {
    let timestamp = now();
    SessionData {
        session: Session {
            id: format!("sess_{}", uuid::Uuid::new_v4()),
            title: "New Chat".into(),
            created_at: timestamp,
            updated_at: timestamp,
            message_count: 0,
            workspace_cwd: Some(workspace.into()),
            preview: None,
            pinned: false,
            pinned_at: None,
        },
        messages: vec![],
        artifacts: vec![],
        model_override: None,
    }
}

impl Model {
    pub(crate) fn record_completion(&mut self, completed: Option<CompletedRun>) {
        if let Some(completed) = completed {
            self.completed_runs.push(completed);
            if self.completed_runs.len() > 32 { self.completed_runs.remove(0); }
        }
    }
}