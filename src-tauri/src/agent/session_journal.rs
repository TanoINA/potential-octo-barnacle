//! One cumulative, checksummed sidecar per session/run. Callers hold the session I/O lock.
//! The canonical stamp uses size + nanosecond mtime, matching session_pages; edits
//! deliberately preserving both are outside this guard (not a cryptographic seal).
use super::{
    session_pages, storage,
    types::{Artifact, ChatMessage, CheckpointAck, CheckpointDelta, SessionData},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    version: u32,
    session_id: String,
    run_id: String,
    base_revision: String,
    base_stamp: String,
    sequence: u64,
    messages: Vec<ChatMessage>,
    artifacts: Vec<Artifact>,
    request_hash: String,
    target_hash: Option<String>,
    checksum: String,
}
#[derive(Clone)]
struct Baseline {
    stamp: String,
    revision: String,
    messages: HashMap<String, ChatMessage>,
}
// Unlike the bounded baseline cache, pending-run ownership must never be evicted.
// Disk reads do not register ownership: after a restart, sidecars are recoverable.
static ACTIVE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
fn active() -> &'static Mutex<HashSet<PathBuf>> {
    ACTIVE.get_or_init(Default::default)
}
pub(super) fn is_active(path: &Path) -> bool {
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(path)
}
fn register_active(path: &Path) {
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_path_buf());
}
static BASES: OnceLock<Mutex<HashMap<PathBuf, Baseline>>> = OnceLock::new();
fn bases() -> &'static Mutex<HashMap<PathBuf, Baseline>> {
    BASES.get_or_init(Default::default)
}
pub(super) fn path_for(path: &Path) -> PathBuf {
    path.with_extension("checkpoint")
}
fn hash(bytes: &[u8]) -> String {
    // Stable FNV-1a detects accidental corruption; not an authentication mechanism.
    let mut value = 0xcbf29ce484222325u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    format!("{value:016x}")
}
fn bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|e| e.to_string())
}
async fn read(path: &Path) -> Result<Option<Journal>, String> {
    let raw = match tokio::fs::read(path_for(path)).await {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut record: Journal =
        serde_json::from_slice(&raw).map_err(|e| format!("Checkpoint corrupt; preserved: {e}"))?;
    let checksum = std::mem::take(&mut record.checksum);
    if record.version != 1 || hash(&bytes(&record)?) != checksum {
        return Err("Checkpoint checksum mismatch; preserved".into());
    }
    record.checksum = checksum;
    Ok(Some(record))
}
async fn write(path: &Path, record: &mut Journal) -> Result<(), String> {
    record.checksum.clear();
    record.checksum = hash(&bytes(record)?);
    storage::atomic_write(&path_for(path), &bytes(record)?).await
}
pub(super) async fn clear(path: &Path) -> Result<(), String> {
    bases()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(path);
    match tokio::fs::remove_file(path_for(path)).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(path);
    Ok(())
}
/// Compare-only completion of a sealed canonical commit. Never discard a newer
/// live checkpoint merely because the caller has an older canonical receipt.
pub(super) async fn acknowledge_committed(path: &Path, canonical: &[u8]) -> Result<(), String> {
    if let Some(record) = read(path).await? {
        if record.target_hash.as_deref() != Some(hash(canonical).as_str()) {
            return Err(
                "Checkpoint changed after commit; recovery conflict, checkpoint preserved".into(),
            );
        }
    }
    clear(path).await
}
async fn guard(path: &Path, record: &Journal) -> Result<bool, String> {
    if path.with_extension("deleted").exists() {
        return Err("Session was deleted".into());
    }
    if session_pages::fingerprint(path)? == record.base_stamp {
        return Ok(false);
    }
    if let Some(target) = &record.target_hash {
        let raw = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
        if &hash(&raw) == target {
            return Ok(true);
        } // interrupted successful canonical commit
    }
    Err("Checkpoint base changed; conflict, original and checkpoint preserved".into())
}
fn narrative(message: &ChatMessage) -> String {
    if let Some(segments) = message.segments.as_ref().filter(|s| !s.is_empty()) {
        format!(
            "{}{}",
            message.preamble.as_deref().unwrap_or(""),
            segments
                .iter()
                .filter_map(|s| s.text.as_deref())
                .collect::<String>()
        )
    } else {
        format!(
            "{}{}",
            message.preamble.as_deref().unwrap_or(""),
            message.content
        )
    }
}
fn append_only(old: Option<&str>, new: Option<&str>) -> bool {
    old.is_none_or(|old| new.is_some_and(|new| new.starts_with(old)))
}
fn tool_grows(old: &super::types::ToolCallItem, new: &super::types::ToolCallItem) -> bool {
    if old.id != new.id {
        return false;
    }
    // Native streaming placeholders have no parsed args until the call resolves.
    let pending = old.status == "pending";
    let terminal = matches!(old.status.as_str(), "done" | "error" | "cancelled");
    if terminal {
        return serde_json::to_value(old).ok() == serde_json::to_value(new).ok();
    }
    if !matches!(
        (old.status.as_str(), new.status.as_str()),
        (
            "pending",
            "pending" | "running" | "done" | "error" | "cancelled"
        ) | ("running", "running" | "done" | "error" | "cancelled")
    ) {
        return false;
    }
    if old.name != new.name && !(pending && old.name == "tool" && !new.name.is_empty()) {
        return false;
    }
    if old.args != new.args && !(pending && old.args.as_object().is_some_and(|a| a.is_empty())) {
        return false;
    }
    if !append_only(old.arguments_raw.as_deref(), new.arguments_raw.as_deref()) {
        return false;
    }
    // The runner emits a fresh startedAt at preparation and again at execution.
    // Changes are allowed only at a forward lifecycle boundary, never on a tick.
    if old.started_at.is_some()
        && old.started_at != new.started_at
        && !(old.status != new.status
            && new
                .started_at
                .zip(old.started_at)
                .is_some_and(|(n, o)| n >= o))
    {
        return false;
    }
    let Ok(mut a) = serde_json::to_value(old) else {
        return false;
    };
    let Ok(mut b) = serde_json::to_value(new) else {
        return false;
    };
    for value in [&mut a, &mut b] {
        for key in ["id", "name", "args", "status", "argumentsRaw", "startedAt"] {
            value.as_object_mut().unwrap().remove(key);
        }
    }
    // Result/exit metadata can be introduced at completion; already observed
    // output and metadata cannot subsequently be rewritten.
    a.as_object()
        .unwrap()
        .iter()
        .all(|(key, value)| value.is_null() || b.get(key) == Some(value))
}
fn tools_grow(old: &[super::types::ToolCallItem], new: &[super::types::ToolCallItem]) -> bool {
    let mut ids = std::collections::HashSet::new();
    if new
        .iter()
        .any(|tool| tool.id.is_empty() || !ids.insert(&tool.id))
    {
        return false;
    }
    old.iter().all(|old| {
        new.iter()
            .find(|new| new.id == old.id)
            .is_some_and(|new| tool_grows(old, new))
    })
}
fn grows(old: &serde_json::Value, new: &serde_json::Value) -> bool {
    use serde_json::Value::*;
    match (old, new) {
        (Null, _) => true,
        (String(a), String(b)) => b.starts_with(a),
        (Array(a), Array(b)) => a.len() <= b.len() && a.iter().zip(b).all(|(a, b)| grows(a, b)),
        (Object(a), Object(b)) => a.iter().all(|(key, value)| {
            if key == "toolCalls" && !value.is_null() {
                let Ok(old) =
                    serde_json::from_value::<Vec<super::types::ToolCallItem>>(value.clone())
                else {
                    return false;
                };
                let Some(next) = b.get(key) else {
                    return false;
                };
                let Ok(new) =
                    serde_json::from_value::<Vec<super::types::ToolCallItem>>(next.clone())
                else {
                    return false;
                };
                return tools_grow(&old, &new);
            }
            matches!(
                key.as_str(),
                "isStreaming" | "runStatus" | "selfImprovement"
            ) || b
                .get(key)
                .map_or(value.is_null(), |next| grows(value, next))
        }),
        _ => old == new,
    }
}
fn checkpoint_counter<T: Serialize>(value: &T, key: &str) -> Result<u64, String> {
    let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    match value.get(key) {
        None | Some(serde_json::Value::Null) => Ok(0),
        Some(value) => value
            .as_u64()
            .filter(|n| *n <= 9_007_199_254_740_991)
            .ok_or_else(|| format!("Invalid {key}: expected a nonnegative safe integer")),
    }
}
fn message_grows(old: &ChatMessage, new: &ChatMessage) -> Result<bool, String> {
    message_grows_at_epochs(
        old,
        new,
        checkpoint_counter(old, "checkpointEpoch")?,
        checkpoint_counter(new, "checkpointEpoch")?,
    )
}
fn message_grows_at_epochs(
    old: &ChatMessage,
    new: &ChatMessage,
    old_epoch: u64,
    new_epoch: u64,
) -> Result<bool, String> {
    if new_epoch < old_epoch {
        return Ok(false);
    }
    if old.id != new.id
        || old.role != new.role
        || old.run_id != new.run_id
        || old.timestamp != new.timestamp
        || !narrative(new).starts_with(&narrative(old))
        || (!old.content.is_empty()
            && !new.content.starts_with(&old.content)
            && !new
                .preamble
                .as_deref()
                .is_some_and(|p| p.starts_with(&old.content)))
    {
        return Ok(false);
    }
    let mut a = serde_json::to_value(old).map_err(|e| e.to_string())?;
    let mut b = serde_json::to_value(new).map_err(|e| e.to_string())?;
    // Only an explicit forward retry epoch can reset provider reasoning. Visible
    // narrative, tools, provider transcript and identity remain lower bounds.
    if new_epoch > old_epoch {
        a.as_object_mut().unwrap().remove("reasoning");
        b.as_object_mut().unwrap().remove("reasoning");
    }
    for value in [&mut a, &mut b] {
        value.as_object_mut().unwrap().remove("checkpointEpoch");
        // Notice text and optional provider attempt metadata are ephemeral UI state,
        // not permission to reset narrative or tool progress.
        value.as_object_mut().unwrap().remove("notice");
        // Viewport payload hints are projections, never semantic progress.
        value.as_object_mut().unwrap().remove("hasImageInputs");
        value.as_object_mut().unwrap().remove("hasProviderMessages");
    }
    // content is a frontend mirror; before the first tool it moves into preamble.
    for value in [&mut a, &mut b] {
        value.as_object_mut().unwrap().remove("content");
        value.as_object_mut().unwrap().remove("preamble");
    }
    Ok(grows(&a, &b))
}
// Only final/canonical snapshot validation treats missing payload as omission.
// Checkpoint validation stays strict: accepting None there would write None into
// the sidecar. Borrow normal full snapshots; clone only when hydration is needed.
fn snapshot_message_grows(old: &ChatMessage, next: &ChatMessage) -> Result<bool, String> {
    if (next.image_inputs.is_none() && old.image_inputs.is_some())
        || (next.provider_messages.is_none() && old.provider_messages.is_some())
    {
        let mut hydrated = next.clone();
        super::storage::retain_message_payload(&mut hydrated, old);
        message_grows(old, &hydrated)
    } else {
        message_grows(old, next)
    }
}
fn validate_messages(run: &str, messages: &[ChatMessage]) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for message in messages {
        if message.id.is_empty()
            || !ids.insert(&message.id)
            || message.role != "assistant"
            || message.run_id.as_deref() != Some(run)
        {
            return Err("Checkpoint requires unique existing same-run assistant messages".into());
        }
    }
    Ok(())
}
fn artifact_grows(old: &Artifact, new: &Artifact) -> Result<bool, String> {
    Ok(artifact_grows_at_revisions(
        old,
        new,
        checkpoint_counter(old, "streamRevision")?,
        checkpoint_counter(new, "streamRevision")?,
    ))
}
fn artifact_grows_at_revisions(
    old: &Artifact,
    new: &Artifact,
    old_revision: u64,
    new_revision: u64,
) -> bool {
    // Event revisions authorize content replacement, never identity replacement.
    // terminal_output appends between artifact events without a revision bump.
    old.id == new.id
        && old.title == new.title
        && old.artifact_type == new.artifact_type
        && old.language == new.language
        && old.file_path == new.file_path
        && old.timestamp == new.timestamp
        && new_revision >= old_revision
        && (new_revision > old_revision || new.content.starts_with(&old.content))
}
fn validate_artifacts(artifacts: &[Artifact]) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    if artifacts
        .iter()
        .any(|a| a.id.is_empty() || !ids.insert(&a.id))
    {
        return Err("Duplicate or empty checkpoint artifact ID".into());
    }
    Ok(())
}
pub(super) async fn checkpoint(
    path: &Path,
    id: &str,
    request: CheckpointDelta,
) -> Result<CheckpointAck, String> {
    if path.with_extension("deleted").exists() {
        return Err("Session was deleted".into());
    }
    if request.run_id.is_empty()
        || request.sequence == 0
        || request.sequence > 9_007_199_254_740_991
        || request.messages.is_empty()
    {
        return Err(
            "Checkpoint requires a run, assistant snapshot and positive safe sequence".into(),
        );
    }
    validate_messages(&request.run_id, &request.messages)?;
    validate_artifacts(&request.artifacts)?;
    let stamp = session_pages::fingerprint(path)?;
    let request_hash = hash(&bytes(&request)?);
    let previous = read(path).await?;
    let mut baseline = bases()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(path)
        .cloned();
    if baseline.as_ref().is_none_or(|b| {
        b.stamp != stamp
            || b.revision != request.base_revision
            || request
                .messages
                .iter()
                .any(|m| !b.messages.contains_key(&m.id))
    }) {
        let ids: Vec<_> = request.messages.iter().map(|m| m.id.as_str()).collect();
        let (revision, messages) = session_pages::checkpoint_baseline(path, id, &ids)?;
        baseline = Some(Baseline {
            stamp: stamp.clone(),
            revision,
            messages: messages.into_iter().map(|m| (m.id.clone(), m)).collect(),
        });
    }
    let baseline = baseline.unwrap();
    if baseline.revision != request.base_revision {
        return Err("Checkpoint base revision is stale; reload/reconcile explicitly".into());
    }
    for message in &request.messages {
        let original = baseline
            .messages
            .get(&message.id)
            .ok_or("Checkpoint assistant is not persisted")?;
        if original.role != "assistant"
            || original.run_id.as_deref() != Some(&request.run_id)
            || original.is_streaming != Some(true)
            || !message_grows(original, message)?
        {
            return Err("Checkpoint assistant identity or baseline progress conflict".into());
        }
    }
    let mut record = if let Some(record) = previous {
        let committed = guard(path, &record).await?;
        if record.session_id != id
            || record.run_id != request.run_id
            || record.base_revision != request.base_revision
            || record.base_stamp != stamp
            || committed
        {
            return Err("Checkpoint run/base conflict; reload explicitly".into());
        }
        if record.target_hash.is_none()
            && request.sequence == record.sequence
            && request_hash == record.request_hash
        {
            register_active(path);
            return Ok(CheckpointAck {
                revision: record.base_revision,
                sequence: record.sequence,
            });
        }
        if request.sequence <= record.sequence {
            return Err("Checkpoint sequence is stale".into());
        }
        record
    } else {
        Journal {
            version: 1,
            session_id: id.into(),
            run_id: request.run_id.clone(),
            base_revision: request.base_revision.clone(),
            base_stamp: stamp,
            sequence: 0,
            messages: vec![],
            artifacts: vec![],
            request_hash: String::new(),
            target_hash: None,
            checksum: String::new(),
        }
    };
    for message in request.messages {
        if let Some(old) = record.messages.iter_mut().find(|m| m.id == message.id) {
            if !message_grows(old, &message)? {
                return Err("Checkpoint would regress assistant progress".into());
            }
            *old = message;
        } else {
            record.messages.push(message);
        }
    }
    for artifact in request.artifacts {
        if let Some(old) = record.artifacts.iter_mut().find(|a| a.id == artifact.id) {
            if !artifact_grows(old, &artifact)? {
                return Err("Checkpoint would regress artifact progress".into());
            }
            *old = artifact;
        } else {
            record.artifacts.push(artifact);
        }
    }
    // Segel yang belum terkomit baru dilepas setelah seluruh validasi kemajuan lolos.
    record.target_hash = None;
    record.sequence = request.sequence;
    record.request_hash = request_hash;
    write(path, &mut record).await?;
    // Keep ownership through runner completion and failed final saves. Releasing
    // it on a done event races the frontend's asynchronous final save.
    register_active(path);
    let mut cache = bases().lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= 32 {
        cache.clear();
    }
    cache.insert(path.to_path_buf(), baseline);
    Ok(CheckpointAck {
        revision: record.base_revision,
        sequence: record.sequence,
    })
}
/// Only abandoned sidecars (or an already-replaced sealed target) need folding.
/// Caller holds the session lock. Checking the guard here also rejects external
/// edits before a live projected read can return an unrelated canonical view.
pub(super) async fn needs_recovery(path: &Path) -> Result<bool, String> {
    let Some(record) = read(path).await? else {
        return Ok(false);
    };
    let committed = guard(path, &record).await?;
    Ok(!is_active(path) || committed)
}

async fn active_record(path: &Path, id: &str) -> Result<Option<Journal>, String> {
    if !is_active(path) {
        return Ok(None);
    }
    let Some(record) = read(path).await? else {
        return Ok(None);
    };
    if record.session_id != id {
        return Err("Checkpoint session identity mismatch".into());
    }
    if guard(path, &record).await? {
        return Ok(None);
    }
    validate_messages(&record.run_id, &record.messages)?;
    Ok(Some(record))
}

/// Overlay only selected message IDs. The projection, cursor and total remain
/// canonical; active progress never enters the reusable SQLite projection.
pub(super) async fn overlay_active_page(path: &Path, data: &mut SessionData) -> Result<(), String> {
    let Some(record) = active_record(path, &data.session.id).await? else {
        return Ok(());
    };
    let mut messages: HashMap<_, _> = record
        .messages
        .into_iter()
        .map(|m| (m.id.clone(), m))
        .collect();
    for old in &mut data.messages {
        if let Some(message) = messages.remove(&old.id) {
            if old.role != "assistant" || old.run_id != message.run_id {
                return Err("Checkpoint message identity mismatch".into());
            }
            let review = old.self_improvement.clone();
            *old = message;
            session_pages::thin_message(old);
            if review.is_some() {
                old.self_improvement = review;
            }
        }
    }
    let references: std::collections::BTreeSet<_> = data
        .messages
        .iter()
        .flat_map(|message| {
            message
                .tool_calls
                .iter()
                .flatten()
                .chain(
                    message
                        .segments
                        .iter()
                        .flatten()
                        .flat_map(|segment| segment.tool_calls.iter().flatten()),
                )
                .filter_map(|call| call.artifact_id.as_deref())
        })
        .take(80)
        .collect();
    // Bound additional artifact payloads as on canonical pages. Existing selected
    // artifacts always get their latest content; other artifacts remain lazy.
    let mut recent: Vec<_> = record.artifacts.iter().collect();
    recent.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| a.id.cmp(&b.id)));
    let recent: HashSet<_> = recent.into_iter().take(20).map(|a| a.id.as_str()).collect();
    for artifact in &record.artifacts {
        if let Some(old) = data.artifacts.iter_mut().find(|a| a.id == artifact.id) {
            *old = artifact.clone();
        } else if references.contains(artifact.id.as_str()) || recent.contains(artifact.id.as_str())
        {
            data.artifacts.push(artifact.clone());
        }
    }
    Ok(())
}

/// Warm catalogs read sidecar messages only, never canonical historical rows.
pub(super) async fn overlay_active_catalog(
    path: &Path,
    session: &mut super::types::Session,
) -> Result<(), String> {
    let Some(record) = active_record(path, &session.id).await? else {
        return Ok(());
    };
    if let Some(message) = record
        .messages
        .iter()
        .rev()
        .find(|m| !m.content.trim().is_empty())
    {
        session.preview = Some(
            message
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(240)
                .collect(),
        );
    }
    Ok(())
}

/// Exact-ID lookup includes new journal-only artifacts without loading history.
pub(super) async fn active_artifact(
    path: &Path,
    id: &str,
    artifact_id: &str,
) -> Result<Option<Artifact>, String> {
    Ok(active_record(path, id)
        .await?
        .and_then(|record| record.artifacts.into_iter().find(|a| a.id == artifact_id)))
}

/// Overlay a valid sidecar in memory. Only recovery readers may terminalize
/// abandoned progress; writers must validate snapshots against the live journal.
pub(super) async fn overlay(
    path: &Path,
    data: &mut SessionData,
    recover_abandoned: bool,
) -> Result<bool, String> {
    let Some(record) = read(path).await? else {
        return Ok(false);
    };
    if record.session_id != data.session.id {
        return Err("Checkpoint session identity mismatch".into());
    }
    let committed = guard(path, &record).await?;
    // A committed seal can require projection repair while still owned. That is
    // not abandonment, and must never cancel the live canonical assistant.
    let abandoned = recover_abandoned && !is_active(path);
    if committed && !abandoned {
        return Ok(true);
    }
    validate_messages(&record.run_id, &record.messages)?;
    for message in record.messages {
        let old = data
            .messages
            .iter_mut()
            .find(|m| m.id == message.id)
            .ok_or("Checkpoint message no longer exists")?;
        if old.role != "assistant" || old.run_id != message.run_id {
            return Err("Checkpoint message identity mismatch".into());
        }
        // A replaced canonical target may contain newer terminal data. Recover
        // its lifecycle in place, never overwrite it with the older checkpoint.
        if !committed {
            let review = old.self_improvement.clone();
            *old = message;
            if review.is_some() {
                old.self_improvement = review;
            }
        }
        if abandoned
            && (old.is_streaming == Some(true)
                || old.run_status.as_deref().unwrap_or("").is_empty())
        {
            old.is_streaming = Some(false);
            old.run_status = Some("cancelled".into());
            for tool in old.tool_calls.iter_mut().flatten().chain(
                old.segments
                    .iter_mut()
                    .flatten()
                    .flat_map(|segment| segment.tool_calls.iter_mut().flatten()),
            ) {
                if matches!(tool.status.as_str(), "pending" | "running") {
                    tool.status = "cancelled".into();
                }
            }
            if let Some(council) = &mut old.council_turn {
                council.candidate_a.is_streaming = Some(false);
                council.candidate_b.is_streaming = Some(false);
            }
        }
    }
    if !committed {
        for artifact in record.artifacts {
            if let Some(old) = data.artifacts.iter_mut().find(|a| a.id == artifact.id) {
                if !artifact_grows(old, &artifact)? {
                    return Err("Checkpoint would regress artifact progress".into());
                }
                *old = artifact;
            } else {
                data.artifacts.push(artifact);
            }
        }
    }
    Ok(true)
}
/// A final/legacy save may consume a sidecar only if it includes its complete
/// progress. Never replace completed status with the sidecar's streaming flag.
pub(super) async fn validate_snapshot(path: &Path, data: &SessionData) -> Result<(), String> {
    let Some(record) = read(path).await? else {
        return Ok(());
    };
    guard(path, &record).await?;
    if record.session_id != data.session.id {
        return Err("Checkpoint session mismatch".into());
    }
    for old in &record.messages {
        let next = data
            .messages
            .iter()
            .find(|m| m.id == old.id)
            .ok_or("Snapshot omits checkpoint assistant")?;
        if !snapshot_message_grows(old, next)? {
            return Err("Snapshot is older than durable checkpoint; original preserved".into());
        }
    }
    for old in &record.artifacts {
        let next = data
            .artifacts
            .iter()
            .find(|a| a.id == old.id)
            .ok_or("Snapshot omits checkpoint artifact")?;
        if !artifact_grows(old, next)? {
            return Err("Snapshot regresses checkpoint artifact".into());
        }
    }
    Ok(())
}
// A checkpoint may already have been folded by a metadata/read operation.
// Streaming same-run canonical content remains a lower bound for stale full saves.
pub(super) fn validate_canonical_progress(
    previous: &SessionData,
    next: &SessionData,
) -> Result<(), String> {
    for old in &previous.messages {
        if old.role != "assistant" || old.run_id.is_none() || old.is_streaming != Some(true) {
            continue;
        }
        let message = next
            .messages
            .iter()
            .find(|m| m.id == old.id)
            .ok_or("Snapshot omits active canonical assistant")?;
        if !snapshot_message_grows(old, message)? {
            return Err("Snapshot regresses active canonical checkpoint progress".into());
        }
    }
    if previous
        .messages
        .iter()
        .any(|m| m.role == "assistant" && m.run_id.is_some() && m.is_streaming == Some(true))
    {
        for old in &previous.artifacts {
            let artifact = next
                .artifacts
                .iter()
                .find(|a| a.id == old.id)
                .ok_or("Snapshot omits active canonical artifact")?;
            if !artifact_grows(old, artifact)? {
                return Err("Snapshot regresses active canonical artifact progress".into());
            }
        }
    }
    Ok(())
}
pub(super) async fn prepare_commit(
    path: &Path,
    data: &SessionData,
    canonical: &[u8],
) -> Result<(), String> {
    validate_snapshot(path, data).await?;
    if let Some(mut record) = read(path).await? {
        record.target_hash = Some(hash(canonical));
        write(path, &mut record).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{storage, types::*};
    fn fixture_directory(id: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("aether-journal-fixture-{id}"))
            .join("sessions")
    }
    fn fixture() -> SessionData {
        let data: SessionData = serde_json::from_value(serde_json::json!({
            "session":{"id":format!("journal-{}",uuid::Uuid::new_v4()),"title":"Journal","createdAt":1,"updatedAt":1,"messageCount":2},
            "messages":[{"id":"user","role":"user","content":"question","timestamp":1},
                {"id":"assistant","role":"assistant","content":"","timestamp":2,"runId":"run","isStreaming":true}],"artifacts":[]
        })).unwrap();
        storage::register_session_test_directory(
            &data.session.id,
            fixture_directory(&data.session.id),
        );
        data
    }
    async fn start(mut data: SessionData) -> (SessionData, CheckpointDelta, std::path::PathBuf) {
        storage::save_session(data.clone()).await.unwrap();
        let page = storage::load_session_page(&data.session.id, None, 2, None)
            .await
            .unwrap();
        data = page.data;
        let mut message = data.messages[1].clone();
        message.content = "progress".into();
        let delta = CheckpointDelta {
            run_id: "run".into(),
            base_revision: page.window.revision,
            sequence: 1,
            messages: vec![message],
            artifacts: vec![],
        };
        let path = fixture_directory(&data.session.id).join(format!("{}.json", data.session.id));
        (data, delta, path)
    }
    #[tokio::test]
    async fn journal_artifacts_remain_lower_bounds_for_final_and_folded_saves() {
        let (mut data, mut delta, path) = start(fixture()).await;
        let artifact: Artifact = serde_json::from_value(serde_json::json!({"id":"output","title":"output","type":"text","content":"durable output","timestamp":1})).unwrap();
        delta.artifacts.push(artifact.clone());
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        data.messages[1] = delta.messages[0].clone();
        data.messages[1].is_streaming = Some(false);
        data.artifacts.push(artifact.clone());
        assert!(super::validate_snapshot(&path, &data).await.is_ok());
        data.artifacts[0].content = "durable".into();
        assert!(super::validate_snapshot(&path, &data).await.is_err());
        let mut folded = data.clone();
        folded.messages[1].is_streaming = Some(true);
        folded.artifacts[0] = artifact;
        assert!(super::validate_canonical_progress(&folded, &data).is_err());
        data.artifacts[0].content = "durable output complete".into();
        assert!(super::validate_canonical_progress(&folded, &data).is_ok());
        data.artifacts.clear();
        assert!(super::validate_canonical_progress(&folded, &data).is_err());
    }
    #[test]
    fn journal_tool_error_and_cancellation_complete_but_do_not_reopen_calls() {
        let mut old = fixture().messages[1].clone();
        old.tool_calls = Some(vec![ToolCallItem {
            id: "call".into(),
            name: "run_command".into(),
            status: "running".into(),
            started_at: Some(10),
            ..Default::default()
        }]);
        for status in ["error", "cancelled"] {
            let mut next = old.clone();
            next.tool_calls.as_mut().unwrap()[0].status = status.into();
            next.tool_calls.as_mut().unwrap()[0].result = Some("execution stopped".into());
            next.tool_calls.as_mut().unwrap()[0].started_at = Some(11);
            assert!(super::message_grows(&old, &next).unwrap());
            assert!(!super::message_grows(&next, &old).unwrap());
            assert!(super::message_grows(&next, &next).unwrap());
        }
        let mut changed = old.clone();
        changed.tool_calls.as_mut().unwrap()[0].started_at = Some(11);
        assert!(
            !super::message_grows(&old, &changed).unwrap(),
            "same-phase timestamp rewrite"
        );
    }
    #[test]
    fn journal_artifact_replacement_requires_strictly_newer_revision() {
        let old: Artifact = serde_json::from_value(serde_json::json!({"id":"output","title":"output","type":"terminal","content":"long partial output","timestamp":1})).unwrap();
        let mut next = old.clone();
        next.content = "final".into();
        assert!(super::artifact_grows_at_revisions(&old, &next, 1, 2));
        assert!(!super::artifact_grows_at_revisions(&old, &next, 1, 1));
        assert!(!super::artifact_grows_at_revisions(&old, &next, 2, 1));
        assert!(!super::artifact_grows_at_revisions(&old, &next, 1, 0));
        next = old.clone();
        next.content.push_str(" appended");
        assert!(
            super::artifact_grows_at_revisions(&old, &next, 1, 1),
            "terminal_output appends within an event revision"
        );
        assert!(super::artifact_grows_at_revisions(&old, &next, 0, 0));
        for case in 0..5 {
            let mut bad = next.clone();
            match case {
                0 => bad.id.push('x'),
                1 => bad.title.push('x'),
                2 => bad.artifact_type = "text".into(),
                3 => bad.file_path = Some("elsewhere".into()),
                _ => bad.timestamp += 1,
            }
            assert!(
                !super::artifact_grows_at_revisions(&old, &bad, 1, 2),
                "identity mutation {case}"
            );
        }
    }
    #[test]
    fn journal_first_tool_preserves_text_moved_to_preamble() {
        let mut old = fixture().messages[1].clone();
        old.content = "Visible opening".into();
        let mut next = old.clone();
        next.content.clear();
        next.preamble = Some(old.content.clone());
        next.segments = Some(vec![MessageSegment {
            id: "seg_0".into(),
            text: Some(String::new()),
            tool_calls: None,
        }]);
        assert!(super::message_grows(&old, &next).unwrap());
        next.preamble = Some("Visible".into());
        assert!(!super::message_grows(&old, &next).unwrap());
    }
    #[tokio::test]
    async fn journal_artifact_unversioned_regression_is_rejected_on_delta() {
        let (data, mut delta, path) = start(fixture()).await;
        let artifact: Artifact = serde_json::from_value(serde_json::json!({"id":"output","title":"output","type":"text","content":"durable output","timestamp":1})).unwrap();
        delta.artifacts.push(artifact);
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        let before = tokio::fs::read(super::path_for(&path)).await.unwrap();
        delta.sequence += 1;
        delta.artifacts[0].content = "durable".into();
        assert!(storage::checkpoint_session_delta(&data.session.id, delta)
            .await
            .is_err());
        assert_eq!(
            before,
            tokio::fs::read(super::path_for(&path)).await.unwrap()
        );
    }
    #[test]
    fn journal_retry_epoch_authorizes_only_reasoning_reset() {
        let mut old = fixture().messages[1].clone();
        old.content = "durable visible answer".into();
        old.reasoning = Some("abandoned provider reasoning".into());
        let mut next = old.clone();
        next.reasoning = Some("new attempt reasoning".into());
        assert!(super::message_grows_at_epochs(&old, &next, 0, 1).unwrap());
        assert!(!super::message_grows_at_epochs(&old, &next, 0, 0).unwrap());
        assert!(!super::message_grows_at_epochs(&old, &next, 2, 1).unwrap());
        next.content = "durable".into();
        assert!(!super::message_grows_at_epochs(&old, &next, 0, 1).unwrap());
        next = old.clone();
        next.timestamp += 1;
        assert!(!super::message_grows_at_epochs(&old, &next, 0, 1).unwrap());
        next = old.clone();
        next.tool_calls = Some(vec![ToolCallItem {
            id: "call".into(),
            name: "run_command".into(),
            status: "done".into(),
            ..Default::default()
        }]);
        let mut reset = next.clone();
        reset.tool_calls = None;
        assert!(!super::message_grows_at_epochs(&next, &reset, 0, 1).unwrap());
    }
    #[test]
    fn journal_typed_tool_lifecycle_accepts_native_placeholders_and_freezes_completion() {
        let mut old = fixture().messages[1].clone();
        old.content = "Visible opening".into();
        let pending: ToolCallItem = serde_json::from_value(serde_json::json!({
            "id":"turn_1_stream_0", "name":"tool", "args":{}, "status":"pending",
            "argumentsRaw":"", "startedAt":100
        }))
        .unwrap();
        old.tool_calls = Some(vec![pending.clone()]);
        old.segments = Some(vec![MessageSegment {
            id: "seg_0".into(),
            text: Some("Visible opening".into()),
            tool_calls: Some(vec![pending]),
        }]);
        let mut running = old.clone();
        let tool: ToolCallItem = serde_json::from_value(serde_json::json!({
            "id":"turn_1_stream_0", "name":"run_command", "args":{"command":"pwd"},
            "status":"running", "argumentsRaw":"{\"command\":\"pwd\"}", "startedAt":110
        }))
        .unwrap();
        running.tool_calls = Some(vec![tool.clone()]);
        running.segments.as_mut().unwrap()[0].tool_calls = Some(vec![tool]);
        assert!(
            super::message_grows(&old, &running).unwrap(),
            "native pending placeholder must resolve"
        );
        let mut done = running.clone();
        for tools in [
            &mut done.tool_calls,
            &mut done.segments.as_mut().unwrap()[0].tool_calls,
        ] {
            let tool = &mut tools.as_mut().unwrap()[0];
            tool.status = "done".into();
            tool.started_at = Some(120);
            tool.completed_at = Some(150);
            tool.duration_ms = Some(30);
            tool.result = Some("D:/AetherAgent".into());
            tool.exit_code = Some(0);
        }
        assert!(
            super::message_grows(&running, &done).unwrap(),
            "execution starts after the running announcement"
        );
        assert!(
            !super::message_grows(&done, &running).unwrap(),
            "terminal status cannot regress"
        );
        for case in 0..5 {
            let mut bad = done.clone();
            let tool = &mut bad.tool_calls.as_mut().unwrap()[0];
            match case {
                0 => tool.result = Some("D:/Aether".into()),
                1 => tool.result = Some("D:/AetherAgent rewritten".into()),
                2 => tool.args = serde_json::json!({"command":"ls"}),
                3 => tool.name = "run_command_other".into(),
                _ => tool.status = "error".into(),
            }
            assert!(
                !super::message_grows(&done, &bad).unwrap(),
                "completed tool mutation {case}"
            );
        }
        let mut dropped = done.clone();
        dropped.tool_calls = Some(vec![]);
        assert!(!super::message_grows(&done, &dropped).unwrap());
    }
    #[test]
    fn journal_segments_must_not_hide_a_regressing_content_mirror() {
        let mut old = fixture().messages[1].clone();
        old.content = "latest durable tail".into();
        old.segments = Some(vec![MessageSegment {
            id: "segment".into(),
            text: Some("opening".into()),
            tool_calls: None,
        }]);
        let mut next = old.clone();
        next.content = "latest".into();
        assert!(!super::message_grows(&old, &next).unwrap());
    }
    // Model only this fixture losing its in-process owner at restart. Keep the
    // durable sidecar and baseline intact; never clear another parallel run.
    fn abandon_for_recovery(path: &std::path::Path) {
        assert!(super::active()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(path));
    }
    #[tokio::test]
    async fn journal_sealed_commit_retries_projection_failure_without_losing_progress() {
        let (data, mut delta, path) = start(fixture()).await;
        let artifact: Artifact = serde_json::from_value(serde_json::json!({
            "id":"output","title":"output","type":"text","content":"durable output","timestamp":1
        }))
        .unwrap();
        delta.artifacts.push(artifact.clone());
        storage::checkpoint_session_delta(&data.session.id, delta)
            .await
            .unwrap();
        let original = tokio::fs::read(&path).await.unwrap();
        let pending = super::read(&path).await.unwrap().unwrap();
        assert!(pending.target_hash.is_none());
        assert!(!super::guard(&path, &pending).await.unwrap());
        let conn =
            rusqlite::Connection::open(storage::config_dir().join("session-pages.sqlite")).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let trigger = format!("journal_fail_{}", uuid::Uuid::new_v4().simple());
        let key = path.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("CREATE TRIGGER {trigger} BEFORE INSERT ON page_messages WHEN NEW.path='{key}' BEGIN SELECT RAISE(ABORT,'injected checkpoint projection failure'); END;")).unwrap();
        abandon_for_recovery(&path);
        assert!(super::needs_recovery(&path).await.unwrap());
        let recovered = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(recovered.messages[1].content, "progress");
        assert_eq!(recovered.artifacts[0].content, artifact.content);
        let canonical = tokio::fs::read(&path).await.unwrap();
        assert_ne!(
            canonical, original,
            "recovery must actually commit canonical progress"
        );
        let persisted: SessionData = serde_json::from_slice(&canonical).unwrap();
        assert_eq!(persisted.messages[1].content, "progress");
        assert_eq!(persisted.artifacts[0].content, artifact.content);
        let sealed = super::read(&path).await.unwrap().unwrap();
        assert_eq!(sealed.target_hash, Some(super::hash(&canonical)));
        assert!(
            super::guard(&path, &sealed).await.unwrap(),
            "sealed target, not the old base"
        );
        assert!(super::needs_recovery(&path).await.unwrap());
        assert!(
            super::path_for(&path).exists(),
            "sidecar must survive until projection commit succeeds"
        );
        let projected: i64 = conn
            .query_row(
                "SELECT count(*) FROM page_sessions WHERE path=?1",
                [path.to_string_lossy().as_ref()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            projected, 0,
            "failed projection must be invalidated, not served stale"
        );
        conn.execute_batch(&format!("DROP TRIGGER {trigger}"))
            .unwrap();
        let page = storage::load_session_page(&data.session.id, None, 1, None)
            .await
            .unwrap();
        assert_eq!(page.data.messages[0].content, "progress");
        assert_eq!(page.data.artifacts[0].content, artifact.content);
        assert!(!super::path_for(&path).exists());
        assert!(!super::is_active(&path));
        assert_eq!(
            storage::read_session_artifact(&data.session.id, &artifact.id)
                .await
                .unwrap()
                .content,
            artifact.content
        );
    }
    #[tokio::test]
    async fn journal_active_pages_keep_provider_transcripts_native_through_final_save() {
        let mut original = fixture();
        let transcript = "x".repeat(2 * 1024 * 1024);
        original.messages[1].provider_messages = Some(vec![
            serde_json::json!({"role":"assistant", "content":transcript}),
        ]);
        let canonical_payload = original.messages[1].provider_messages.clone();
        let (data, mut delta, path) = start(original).await;
        let canonical = tokio::fs::read(&path).await.unwrap();
        let persisted: SessionData = serde_json::from_slice(&canonical).unwrap();
        assert_eq!(persisted.messages[1].provider_messages, canonical_payload);
        assert!(canonical.len() > 2 * 1024 * 1024);
        assert!(data.messages[1].provider_messages.is_none());
        assert_eq!(data.messages[1].has_provider_messages, Some(true));
        // Both native stores retain full replay bytes; viewport reads must not copy them.
        let live_payload = canonical_payload.unwrap();
        delta.messages[0].provider_messages = Some(live_payload.clone());
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        let sidecar = tokio::fs::read(super::path_for(&path)).await.unwrap();
        let record = super::read(&path).await.unwrap().unwrap();
        assert_eq!(
            record.messages[0].provider_messages.as_ref(),
            Some(&live_payload)
        );
        assert!(sidecar.len() > 2 * 1024 * 1024);

        let page = storage::load_session_page(
            &data.session.id,
            None,
            1,
            Some(delta.base_revision.clone()),
        )
        .await
        .unwrap();
        let around = storage::load_session_around(&data.session.id, "assistant", 1)
            .await
            .unwrap();
        for response in [&page, &around] {
            assert_eq!(response.data.messages.len(), 1);
            let message = &response.data.messages[0];
            assert_eq!(message.id, "assistant");
            assert_eq!(message.content, "progress");
            assert!(
                message.provider_messages.is_none(),
                "active pages must omit replay bytes"
            );
            assert_eq!(message.has_provider_messages, Some(true));
            assert!(serde_json::to_vec(&response.data).unwrap().len() < 10 * 1024);
            assert!(serde_json::to_vec(response).unwrap().len() < 10 * 1024);
            assert_eq!(response.window.revision, delta.base_revision);
        }
        let full = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(
            full.messages[1].provider_messages.as_ref(),
            Some(&live_payload)
        );
        assert_eq!(canonical, tokio::fs::read(&path).await.unwrap());
        assert_eq!(
            sidecar,
            tokio::fs::read(super::path_for(&path)).await.unwrap()
        );
        assert!(super::is_active(&path));

        // A later checkpoint must still project the latest active generation.
        delta.sequence = 2;
        delta.messages[0].content.push_str(" more");
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        let mut latest = storage::load_session_page(
            &data.session.id,
            None,
            1,
            Some(delta.base_revision.clone()),
        )
        .await
        .unwrap();
        assert_eq!(latest.data.messages[0].content, "progress more");
        assert!(latest.data.messages[0].provider_messages.is_none());
        assert_eq!(latest.data.messages[0].has_provider_messages, Some(true));
        assert!(serde_json::to_vec(&latest).unwrap().len() < 10 * 1024);
        // Save the thin UI response, not a copy carrying native replay bytes.
        latest.data.messages[0].content.push_str(" complete");
        latest.data.messages[0].is_streaming = Some(false);
        storage::save_session_page(latest.data, latest.window)
            .await
            .unwrap();
        let final_data = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(final_data.messages.len(), 2);
        assert_eq!(final_data.messages[0].content, "question");
        assert_eq!(final_data.messages[1].content, "progress more complete");
        assert_eq!(final_data.messages[1].is_streaming, Some(false));
        assert_eq!(
            final_data.messages[1].provider_messages.as_ref(),
            Some(&live_payload)
        );
        let saved: SessionData =
            serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
        assert_eq!(
            saved.messages[1].provider_messages.as_ref(),
            Some(&live_payload)
        );
        assert!(!super::path_for(&path).exists());
        assert!(!super::is_active(&path));
    }
    #[tokio::test]
    async fn journal_only_replay_payload_survives_thin_paged_and_full_final_saves() {
        for paged in [true, false] {
            let (data, mut delta, path) = start(fixture()).await;
            let payload = Some(vec![serde_json::json!({
                "role":"assistant", "content":"p".repeat(2 * 1024 * 1024)
            })]);
            let images = Some(vec![serde_json::from_value(serde_json::json!({
                "sourcePath":"native-image.png", "mediaType":"image/png", "data":"aW1hZ2U="
            }))
            .unwrap()]);
            delta.messages[0].provider_messages = payload.clone();
            delta.messages[0].image_inputs = images.clone();
            storage::checkpoint_session_delta(&data.session.id, delta)
                .await
                .unwrap();
            let canonical = tokio::fs::read(&path).await.unwrap();
            let raw: SessionData = serde_json::from_slice(&canonical).unwrap();
            assert!(raw.messages[1].provider_messages.is_none());
            let sidecar = tokio::fs::read(super::path_for(&path)).await.unwrap();
            let mut page = storage::load_session_page(&data.session.id, None, 2, None)
                .await
                .unwrap();
            assert!(page.data.messages[1].provider_messages.is_none());
            assert!(page.data.messages[1].image_inputs.is_none());
            assert_eq!(page.data.messages[1].has_provider_messages, Some(true));
            page.data.messages[1].content.push_str(" complete");
            page.data.messages[1].is_streaming = Some(false);
            // Hints are not authority: native retention must work even if cleared or false.
            page.data.messages[1].has_provider_messages = if paged { None } else { Some(false) };
            page.data.messages[1].has_image_inputs = if paged { None } else { Some(false) };
            // Exercise thin snapshot validation independently of storage hydration.
            assert!(super::validate_snapshot(&path, &page.data).await.is_ok());
            let mut active = raw.clone();
            super::overlay(&path, &mut active, false).await.unwrap();
            assert!(super::validate_canonical_progress(&active, &page.data).is_ok());
            for incorrect in [
                vec![serde_json::json!({"role":"assistant", "content":"wrong"})],
                vec![],
            ] {
                let mut bad = page.data.clone();
                bad.messages[1].provider_messages = Some(incorrect);
                let rejected = if paged {
                    storage::save_session_page(bad, page.window.clone())
                        .await
                        .map(|_| ())
                } else {
                    storage::save_session(bad).await.map(|_| ())
                };
                assert!(
                    rejected.is_err(),
                    "explicit protocol changes must be rejected"
                );
                assert_eq!(canonical, tokio::fs::read(&path).await.unwrap());
                assert_eq!(
                    sidecar,
                    tokio::fs::read(super::path_for(&path)).await.unwrap()
                );
            }
            if paged {
                storage::save_session_page(page.data, page.window)
                    .await
                    .unwrap();
            } else {
                storage::save_session(page.data).await.unwrap();
            }
            let raw: SessionData =
                serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
            let loaded = storage::load_session(&data.session.id).await.unwrap();
            for saved in [&raw, &loaded] {
                assert_eq!(saved.messages[1].provider_messages, payload);
                assert_eq!(
                    serde_json::to_value(&saved.messages[1].image_inputs).unwrap(),
                    serde_json::to_value(&images).unwrap()
                );
                assert_eq!(saved.messages[1].content, "progress complete");
                assert_eq!(saved.messages[1].is_streaming, Some(false));
                assert_eq!(saved.messages[1].has_provider_messages, None);
                assert_eq!(saved.messages[1].has_image_inputs, None);
            }
            assert!(!super::path_for(&path).exists());
        }
    }
    #[tokio::test]
    async fn journal_active_reads_preserve_revision_through_other_save_and_paged_final() {
        let (data, mut delta, path) = start(fixture()).await;
        // A genuine suffix: the paged final must preserve the unseen user row.
        let initial = storage::load_session_page(&data.session.id, None, 1, None)
            .await
            .unwrap();
        assert_eq!(initial.window.offset, 1);
        assert_eq!(initial.window.total, 2);
        assert_eq!(
            initial.window.first_message_id.as_deref(),
            Some("assistant")
        );
        let revision = initial.window.revision.clone();
        assert_eq!(delta.base_revision, revision);
        let artifact: Artifact = serde_json::from_value(serde_json::json!({
            "id":"live-output","title":"output","type":"text","content":"durable output","timestamp":1
        })).unwrap();
        delta.artifacts.push(artifact.clone());
        let canonical = tokio::fs::read(&path).await.unwrap();
        let stamp = super::super::session_pages::fingerprint(&path).unwrap();
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        let sidecar = tokio::fs::read(super::path_for(&path)).await.unwrap();

        let catalog = storage::list_sessions_in_dir(path.parent().unwrap())
            .await
            .unwrap();
        assert_eq!(
            catalog
                .iter()
                .find(|s| s.id == data.session.id)
                .unwrap()
                .preview
                .as_deref(),
            Some("progress")
        );
        let mut other = fixture();
        assert_ne!(other.session.id, data.session.id);
        other.messages[1].content = "other session".into();
        other.messages[1].is_streaming = Some(false);
        storage::save_session(other).await.unwrap();
        let around = storage::load_session_around(&data.session.id, "assistant", 1)
            .await
            .unwrap();
        assert_eq!(around.window.revision, revision);
        assert_eq!(
            around
                .data
                .messages
                .iter()
                .find(|m| m.id == "assistant")
                .unwrap()
                .content,
            "progress"
        );
        assert_eq!(
            storage::read_session_artifact(&data.session.id, &artifact.id)
                .await
                .unwrap()
                .content,
            artifact.content
        );
        let full = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(full.messages[1].content, "progress");
        let page = storage::load_session_page(&data.session.id, None, 1, Some(revision.clone()))
            .await
            .unwrap();
        assert_eq!(page.window.revision, revision);
        assert_eq!(page.data.messages[0].content, "progress");
        assert_eq!(canonical, tokio::fs::read(&path).await.unwrap());
        assert_eq!(
            stamp,
            super::super::session_pages::fingerprint(&path).unwrap()
        );
        assert_eq!(
            sidecar,
            tokio::fs::read(super::path_for(&path)).await.unwrap()
        );
        assert!(super::is_active(&path));
        assert!(super::read(&path)
            .await
            .unwrap()
            .unwrap()
            .target_hash
            .is_none());

        delta.sequence = 2;
        delta.messages[0].content.push_str(" more");
        delta.artifacts[0].content.push_str(" more");
        let ack = storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        assert_eq!(ack.revision, revision);
        assert_eq!(ack.sequence, 2);
        let latest = storage::load_session_page(&data.session.id, None, 1, Some(revision.clone()))
            .await
            .unwrap();
        assert_eq!(latest.window.revision, revision);
        assert_eq!(latest.data.messages[0].content, "progress more");
        assert_eq!(latest.data.artifacts[0].content, "durable output more");
        let mut final_data = initial.data;
        final_data.messages[0] = delta.messages[0].clone();
        final_data.messages[0].content.push_str(" complete");
        final_data.messages[0].is_streaming = Some(false);
        final_data.artifacts = delta.artifacts;
        let committed = storage::save_session_page(final_data, initial.window)
            .await
            .unwrap();
        assert_ne!(committed.revision, revision);
        assert!(!super::path_for(&path).exists());
        assert!(!super::is_active(&path));
        let final_data = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(final_data.messages.len(), 2);
        assert_eq!(final_data.messages[0].content, "question");
        assert_eq!(final_data.messages[1].content, "progress more complete");
        assert_eq!(final_data.messages[1].is_streaming, Some(false));
        assert_eq!(final_data.artifacts[0].content, "durable output more");
    }
    #[tokio::test]
    async fn journal_folded_progress_cannot_be_lost_to_stale_full_final() {
        let (mut data, delta, _) = start(fixture()).await;
        storage::checkpoint_session_delta(&data.session.id, delta)
            .await
            .unwrap();
        storage::toggle_pin_session(&data.session.id).await.unwrap();
        data.messages[1].is_streaming = Some(false);
        assert!(
            storage::save_session(data.clone()).await.is_err(),
            "stale final must not erase already folded checkpoint"
        );
        data.messages[1].content = "progress complete".into();
        storage::save_session(data.clone()).await.unwrap();
        let final_data = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(final_data.messages[1].content, "progress complete");
        assert!(final_data.session.pinned);
        assert_eq!(final_data.messages[1].is_streaming, Some(false));
    }
    #[tokio::test]
    async fn journal_delta_is_small_and_warm_ticks_do_not_decode_history() {
        let mut data = fixture();
        data.messages[0].image_inputs = Some(vec![ImageInput {
            source_path: "history.png".into(),
            media_type: "image/png".into(),
            data: "QUJD".repeat(512 * 1024),
        }]);
        let (data, mut delta, path) = start(data).await;
        assert!(serde_json::to_vec(&delta).unwrap().len() < 2048);
        let canonical = tokio::fs::read(&path).await.unwrap();
        let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        // Corrupt an unselected derived historical row; warm checkpoints must not decode it.
        let conn =
            rusqlite::Connection::open(storage::config_dir().join("session-pages.sqlite")).unwrap();
        conn.execute(
            "UPDATE page_messages SET json='{broken' WHERE path=?1 AND ordinal=0",
            [path.to_string_lossy().as_ref()],
        )
        .unwrap();
        for seq in 2..5 {
            delta.sequence = seq;
            delta.messages[0].content.push_str(" more");
            storage::checkpoint_session_delta(&data.session.id, delta.clone())
                .await
                .unwrap();
        }
        assert_eq!(canonical, tokio::fs::read(&path).await.unwrap());
        assert_eq!(stamp, std::fs::metadata(&path).unwrap().modified().unwrap());
        assert!(std::fs::metadata(super::path_for(&path)).unwrap().len() < 4096);
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.path() == super::path_for(&path))
                .count(),
            1
        );
        let recovered = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(
            recovered.messages[0].image_inputs.as_ref().unwrap()[0]
                .data
                .len(),
            2 * 1024 * 1024
        );
        assert_eq!(recovered.messages[1].content, delta.messages[0].content);
    }
    #[tokio::test]
    async fn journal_sequences_identity_final_and_pin_are_conflict_safe() {
        let (data, mut delta, path) = start(fixture()).await;
        let ack = storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        assert_eq!(ack.revision, delta.base_revision);
        assert_eq!(ack.sequence, 1);
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        let journal = tokio::fs::read(super::path_for(&path)).await.unwrap();
        let mut bad = delta.clone();
        bad.messages[0].content = "different".into();
        assert!(storage::checkpoint_session_delta(&data.session.id, bad)
            .await
            .is_err());
        for case in 0..6 {
            let mut bad = delta.clone();
            bad.sequence = 2;
            match case {
                0 => bad.messages[0].role = "user".into(),
                1 => bad.messages[0].id = "invented".into(),
                2 => bad.messages[0].run_id = Some("other".into()),
                3 => bad.messages.push(bad.messages[0].clone()),
                4 => bad.base_revision = "stale".into(),
                _ => bad.messages[0].content = "short".into(),
            }
            assert!(
                storage::checkpoint_session_delta(&data.session.id, bad)
                    .await
                    .is_err(),
                "case {case}"
            );
        }
        assert_eq!(
            journal,
            tokio::fs::read(super::path_for(&path)).await.unwrap()
        );
        assert!(storage::save_session(data.clone()).await.is_err());
        storage::toggle_pin_session(&data.session.id).await.unwrap();
        delta.sequence = 2;
        delta.messages[0].content.push_str(" more");
        assert!(
            storage::checkpoint_session_delta(&data.session.id, delta.clone())
                .await
                .is_err()
        );
        let fresh = storage::load_session_page(&data.session.id, None, 2, None)
            .await
            .unwrap();
        assert!(fresh.data.session.pinned);
        assert_eq!(fresh.data.messages[1].content, "progress");
        delta.base_revision = fresh.window.revision.clone();
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        let mut final_data = fresh.data;
        final_data.messages[1] = delta.messages[0].clone();
        final_data.messages[1].content.push_str(" complete");
        final_data.messages[1].is_streaming = Some(false);
        storage::save_session_page(final_data, fresh.window)
            .await
            .unwrap();
        assert!(!super::path_for(&path).exists());
        assert_eq!(
            storage::load_session(&data.session.id)
                .await
                .unwrap()
                .messages[1]
                .is_streaming,
            Some(false)
        );
    }
    #[tokio::test]
    async fn journal_corruption_external_edits_and_tombstones_preserve_authority() {
        for case in 0..3 {
            let (data, delta, path) = start(fixture()).await;
            storage::checkpoint_session_delta(&data.session.id, delta.clone())
                .await
                .unwrap();
            let sidecar = super::path_for(&path);
            if case == 0 {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&tokio::fs::read(&sidecar).await.unwrap()).unwrap();
                value["messages"][0]["content"] = "tampered".into();
                tokio::fs::write(&sidecar, serde_json::to_vec(&value).unwrap())
                    .await
                    .unwrap();
            } else if case == 1 {
                tokio::fs::write(&path, serde_json::to_vec(&data).unwrap())
                    .await
                    .unwrap();
            } else {
                tokio::fs::write(path.with_extension("deleted"), b"deleted")
                    .await
                    .unwrap();
            }
            let canonical = tokio::fs::read(&path).await.unwrap();
            let journal = tokio::fs::read(&sidecar).await.unwrap();
            assert!(storage::load_session(&data.session.id).await.is_err());
            assert!(storage::checkpoint_session_delta(&data.session.id, delta)
                .await
                .is_err());
            assert_eq!(canonical, tokio::fs::read(&path).await.unwrap());
            assert_eq!(journal, tokio::fs::read(&sidecar).await.unwrap());
            storage::delete_session(&data.session.id).await.unwrap();
            assert!(!sidecar.exists());
        }
    }
    #[tokio::test]
    async fn journal_prepared_commit_resumes_only_while_canonical_base_is_unchanged() {
        for committed in [true, false] {
            let (mut data, mut delta, path) = start(fixture()).await;
            delta.artifacts.push(serde_json::from_value(serde_json::json!({
                "id":"output","title":"output","type":"text","content":"durable output","timestamp":1
            })).unwrap());
            storage::checkpoint_session_delta(&data.session.id, delta.clone())
                .await
                .unwrap();
            let base = tokio::fs::read(&path).await.unwrap();
            let stamp = super::super::session_pages::fingerprint(&path).unwrap();
            data.messages[1] = delta.messages[0].clone();
            data.messages[1].is_streaming = Some(false);
            data.artifacts = delta.artifacts.clone();
            let target = serde_json::to_vec_pretty(&data).unwrap();
            super::prepare_commit(&path, &data, &target).await.unwrap();
            let sealed = tokio::fs::read(super::path_for(&path)).await.unwrap();
            let record = super::read(&path).await.unwrap().unwrap();
            assert_eq!(record.target_hash, Some(super::hash(&target)));
            assert_eq!(record.sequence, 1);
            assert!(!super::guard(&path, &record).await.unwrap());
            assert_eq!(
                stamp,
                super::super::session_pages::fingerprint(&path).unwrap()
            );
            assert_eq!(base, tokio::fs::read(&path).await.unwrap());

            if committed {
                storage::atomic_write(&path, &target).await.unwrap();
                assert!(super::guard(&path, &record).await.unwrap());
            } else {
                assert!(
                    storage::checkpoint_session_delta(&data.session.id, delta.clone())
                        .await
                        .is_err(),
                    "a sealed sequence must not be reopened by an identical replay"
                );
                assert_eq!(
                    sealed,
                    tokio::fs::read(super::path_for(&path)).await.unwrap()
                );
                for case in 0..9 {
                    let mut bad = delta.clone();
                    bad.sequence = 2;
                    match case {
                        0 => bad.base_revision.push_str("-stale"),
                        1 => bad.run_id = "foreign".into(),
                        2 => bad.messages[0].id = "foreign".into(),
                        3 => bad.messages[0].timestamp += 1,
                        4 => bad.messages[0].content = "regression".into(),
                        5 => bad.artifacts[0].content = "durable".into(),
                        6 => bad.artifacts[0].title.push_str(" changed"),
                        7 => bad.sequence = 1,
                        _ => bad.messages.push(bad.messages[0].clone()),
                    }
                    assert!(
                        storage::checkpoint_session_delta(&data.session.id, bad)
                            .await
                            .is_err(),
                        "invalid sealed continuation {case}"
                    );
                    assert_eq!(
                        sealed,
                        tokio::fs::read(super::path_for(&path)).await.unwrap()
                    );
                    assert_eq!(base, tokio::fs::read(&path).await.unwrap());
                }
            }

            delta.sequence = 2;
            delta.messages[0].content.push_str(" more");
            delta.artifacts[0].content.push_str(" more");
            let resumed = storage::checkpoint_session_delta(&data.session.id, delta.clone()).await;
            if committed {
                assert!(
                    resumed.is_err(),
                    "a committed seal must reject the stale run"
                );
                assert_eq!(target, tokio::fs::read(&path).await.unwrap());
                assert_eq!(
                    sealed,
                    tokio::fs::read(super::path_for(&path)).await.unwrap()
                );
                assert!(super::read(&path)
                    .await
                    .unwrap()
                    .unwrap()
                    .target_hash
                    .is_some());
                continue;
            }
            let ack = resumed.expect("an uncommitted seal must allow newer validated progress");
            assert_eq!(ack.sequence, 2);
            assert_eq!(ack.revision, delta.base_revision);
            let resumed = super::read(&path).await.unwrap().unwrap();
            assert!(resumed.target_hash.is_none());
            assert_eq!(resumed.sequence, 2);
            assert_eq!(resumed.messages[0].content, "progress more");
            assert_eq!(resumed.artifacts[0].content, "durable output more");
            assert_eq!(base, tokio::fs::read(&path).await.unwrap());
            assert_eq!(
                stamp,
                super::super::session_pages::fingerprint(&path).unwrap()
            );
            assert!(super::is_active(&path));
            assert!(super::acknowledge_committed(&path, &target).await.is_err());
            assert_eq!(super::read(&path).await.unwrap().unwrap().sequence, 2);
        }
    }
    #[tokio::test]
    async fn journal_foreign_prepared_commit_seals_are_preserved() {
        for case in 0..4 {
            let (mut data, mut delta, path) = start(fixture()).await;
            storage::checkpoint_session_delta(&data.session.id, delta.clone())
                .await
                .unwrap();
            data.messages[1] = delta.messages[0].clone();
            let target = serde_json::to_vec_pretty(&data).unwrap();
            super::prepare_commit(&path, &data, &target).await.unwrap();
            let mut foreign = super::read(&path).await.unwrap().unwrap();
            match case {
                0 => foreign.session_id = "foreign-session".into(),
                1 => foreign.run_id = "foreign-run".into(),
                2 => foreign.base_revision = "foreign-revision".into(),
                _ => foreign.base_stamp = "foreign-stamp".into(),
            }
            super::write(&path, &mut foreign).await.unwrap();
            let canonical = tokio::fs::read(&path).await.unwrap();
            let sealed = tokio::fs::read(super::path_for(&path)).await.unwrap();
            delta.sequence = 2;
            delta.messages[0].content.push_str(" more");
            assert!(
                storage::checkpoint_session_delta(&data.session.id, delta)
                    .await
                    .is_err(),
                "foreign seal {case} must reject continuation"
            );
            assert_eq!(canonical, tokio::fs::read(&path).await.unwrap());
            assert_eq!(
                sealed,
                tokio::fs::read(super::path_for(&path)).await.unwrap()
            );
            assert_eq!(
                super::read(&path).await.unwrap().unwrap().target_hash,
                Some(super::hash(&target))
            );
        }
    }
    #[tokio::test]
    async fn journal_interrupted_commit_seal_recovers_after_canonical_replacement() {
        let (data, delta, path) = start(fixture()).await;
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        let mut target = data;
        target.messages[1] = delta.messages[0].clone();
        let canonical = serde_json::to_vec_pretty(&target).unwrap();
        super::prepare_commit(&path, &target, &canonical)
            .await
            .unwrap();
        storage::atomic_write(&path, &canonical).await.unwrap();
        let page = storage::load_session_page(&target.session.id, None, 1, None)
            .await
            .unwrap();
        assert_eq!(page.data.messages[0].content, "progress");
        assert!(!super::path_for(&path).exists());
    }
    #[tokio::test]
    async fn journal_cumulative_artifacts_and_native_review_survive_final_save() {
        let (mut data, mut delta, path) = start(fixture()).await;
        let artifact:Artifact=serde_json::from_value(serde_json::json!({"id":"artifact","title":"output","type":"text","content":"durable","timestamp":1})).unwrap();
        delta.artifacts.push(artifact.clone());
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        delta.sequence = 2;
        delta.artifacts.clear();
        delta.messages[0].content.push_str(" more");
        storage::checkpoint_session_delta(&data.session.id, delta.clone())
            .await
            .unwrap();
        // A final with shorter progress must leave both files unchanged.
        let canonical = tokio::fs::read(&path).await.unwrap();
        let sidecar = tokio::fs::read(super::path_for(&path)).await.unwrap();
        assert!(storage::save_session(data.clone()).await.is_err());
        assert_eq!(canonical, tokio::fs::read(&path).await.unwrap());
        assert_eq!(
            sidecar,
            tokio::fs::read(super::path_for(&path)).await.unwrap()
        );
        let event:SelfImprovementEvent=serde_json::from_value(serde_json::json!({"run_id":"run","session_id":data.session.id,"action":"no_action","summary":"review preserved"})).unwrap();
        storage::record_self_improvement(&event).await.unwrap();
        let recovered = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(recovered.artifacts[0].content, "durable");
        assert_eq!(recovered.messages[1].content, "progress more");
        assert_eq!(
            recovered.messages[1].self_improvement.as_ref(),
            Some(&event)
        );
        data.messages[1] = delta.messages[0].clone();
        data.messages[1].is_streaming = Some(false);
        data.artifacts.push(artifact);
        storage::save_session(data.clone()).await.unwrap();
        let recovered = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(recovered.messages[1].is_streaming, Some(false));
        assert_eq!(
            recovered.messages[1].self_improvement.as_ref(),
            Some(&event)
        );
    }
    #[test]
    fn journal_restart_child() {
        let Ok(id) = std::env::var("AETHER_JOURNAL_RESTART_ID") else {
            return;
        };
        storage::register_session_test_directory(
            &id,
            std::env::var_os("AETHER_JOURNAL_RESTART_DIRECTORY")
                .unwrap()
                .into(),
        );
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            if let Ok(edit) = std::env::var("AETHER_JOURNAL_RESTART_RECOVERY_EDIT") {
                let path = fixture_directory(&id).join(format!("{id}.json"));
                assert!(!super::is_active(&path));
                assert!(super::needs_recovery(&path).await.unwrap());
                let expected: SessionData = serde_json::from_slice(
                    &tokio::fs::read(path.with_extension("expected")).await.unwrap(),
                )
                .unwrap();
                let page = storage::load_session_page(&id, None, 20, None)
                    .await
                    .unwrap();
                let recovered = storage::load_session(&id).await.unwrap();
                let persisted: SessionData =
                    serde_json::from_slice(&tokio::fs::read(&path).await.unwrap()).unwrap();
                assert!(!super::path_for(&path).exists());
                let mut edited = recovered.clone();
                edited.messages.truncate(if edit == "clear" { 0 } else { 1 });
                edited.artifacts.clear();
                let result = storage::save_session(edited.clone()).await;
                assert!(result.is_ok(), "immediate {edit} after abandoned recovery: {result:?}");
                for actual in [&recovered, &persisted] {
                    assert_eq!(
                        serde_json::to_value(&actual.messages).unwrap(),
                        serde_json::to_value(&expected.messages).unwrap(),
                        "only checkpoint-matching interrupted lifecycle fields may change"
                    );
                    assert_eq!(
                        serde_json::to_value(&actual.artifacts).unwrap(),
                        serde_json::to_value(&expected.artifacts).unwrap()
                    );
                }
                assert_eq!(page.data.messages[1].is_streaming, Some(false));
                assert_eq!(page.data.messages[1].run_status.as_deref(), Some("cancelled"));
                assert!(page.data.messages[1].provider_messages.is_none());
                assert_eq!(page.data.messages[1].has_provider_messages, Some(true));
                let saved = storage::load_session(&id).await.unwrap();
                assert_eq!(saved.messages.len(), edited.messages.len());
                assert!(saved.artifacts.is_empty());
                return;
            }
            let page = storage::load_session_page(&id, None, 1, None)
                .await
                .unwrap();
            assert_eq!(page.data.messages[0].content, "progress");
            assert_eq!(
                storage::load_session(&id).await.unwrap().messages[1].content,
                "progress"
            );
        });
    }
    #[tokio::test]
    async fn journal_real_process_restart_loads_disk_sidecar() {
        let (data, delta, path) = start(fixture()).await;
        storage::checkpoint_session_delta(&data.session.id, delta)
            .await
            .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agent::session_journal::tests::journal_restart_child",
                "--nocapture",
            ])
            .env("AETHER_JOURNAL_RESTART_ID", &data.session.id)
            .env("AETHER_JOURNAL_RESTART_DIRECTORY", path.parent().unwrap())
            .env("AETHER_JOURNAL_TEST_ROOT", storage::config_dir())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!super::path_for(&path).exists());
    }
    #[tokio::test]
    async fn journal_owned_recovery_reads_keep_live_and_sealed_progress_protected() {
        for seal in ["none", "prepared", "committed"] {
            let (mut data, mut delta, path) = start(fixture()).await;
            let tool = ToolCallItem {
                id: "live-tool".into(), name: "execute".into(), status: "running".into(),
                args: serde_json::json!({"command":"keep running"}),
                arguments_raw: Some("{\"command\":\"keep running\"}".into()),
                ..Default::default()
            };
            delta.messages[0].tool_calls = Some(vec![tool.clone()]);
            delta.messages[0].segments = Some(vec![MessageSegment {
                id: "live-segment".into(), text: Some("progress".into()), tool_calls: Some(vec![tool])
            }]);
            storage::checkpoint_session_delta(&data.session.id, delta.clone()).await.unwrap();
            data.messages[1] = delta.messages[0].clone();
            if seal != "none" {
                let target = serde_json::to_vec_pretty(&data).unwrap();
                super::prepare_commit(&path, &data, &target).await.unwrap();
                if seal == "committed" {
                    storage::atomic_write(&path, &target).await.unwrap();
                }
            }
            let canonical = tokio::fs::read(&path).await.unwrap();
            let sidecar = tokio::fs::read(super::path_for(&path)).await.unwrap();
            assert!(super::is_active(&path));
            let loaded = storage::load_session(&data.session.id).await.unwrap();
            let page = storage::load_session_page(&data.session.id, None, 1, None).await.unwrap();
            assert_eq!(serde_json::to_value(&loaded.messages[1]).unwrap(),
                serde_json::to_value(&delta.messages[0]).unwrap(), "owned seal={seal}");
            assert_eq!(page.data.messages[0].is_streaming, Some(true));
            assert_eq!(page.data.messages[0].tool_calls.as_ref().unwrap()[0].status, "running");
            let after_read = tokio::fs::read(&path).await.unwrap();
            if seal != "committed" {
                assert_eq!(canonical, after_read);
                assert_eq!(sidecar, tokio::fs::read(super::path_for(&path)).await.unwrap());
                assert!(super::is_active(&path));
            }
            let mut cleared = loaded;
            cleared.messages.clear();
            cleared.artifacts.clear();
            let error = storage::save_session(cleared).await.unwrap_err();
            assert!(error.contains("Snapshot omits"), "{error}");
            assert_eq!(after_read, tokio::fs::read(&path).await.unwrap());
        }
    }
    #[tokio::test]
    async fn journal_abandoned_restart_terminalizes_progress_before_clear_or_undo() {
        for edit in ["clear", "undo"] {
            for seal in ["none", "prepared", "committed"] {
                let mut original = fixture();
                let mut completed = original.messages[1].clone();
                completed.id = "completed".into();
                let mut no_status = original.messages[1].clone();
                no_status.id = "no-status".into();
                let mut unrelated = original.messages[1].clone();
                unrelated.id = "not-in-checkpoint".into();
                unrelated.is_streaming = Some(false);
                unrelated.tool_calls = Some(vec![ToolCallItem {
                    id: "historical-pending".into(), status: "pending".into(),
                    ..Default::default()
                }]);
                original.messages.extend([completed, no_status, unrelated]);
                let (_, mut delta, path) = start(original.clone()).await;
                let mut data = original;
                delta.messages = vec![data.messages[1].clone()];
                delta.messages[0].content = "progress".into();
                let tools: Vec<ToolCallItem> = ["pending", "running", "done", "error", "cancelled"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, status)| serde_json::from_value(serde_json::json!({
                        "id":format!("tool-{i}"), "name":"execute", "status":status,
                        "args":{"command":"echo preserved", "nested":[1, {"key":"value"}]},
                        "argumentsRaw":"{\"command\":\"echo preserved\"", "artifactId":"output",
                        "result":"observed result", "terminalOutput":"observed stdout\n",
                        "startedAt":10, "completedAt":20, "durationMs":10, "exitCode":7,
                        "filesChanged":[{"path":"keep.txt", "additions":2, "deletions":1}]
                    })).unwrap())
                    .collect();
                let message = &mut delta.messages[0];
                message.preamble = Some("opening".into());
                message.reasoning = Some("native reasoning".into());
                message.checkpoint_epoch = Some(2);
                message.tool_calls = Some(tools.clone());
                message.segments = Some(vec![MessageSegment {
                    id: "segment".into(), text: Some("segment narrative".into()),
                    tool_calls: Some(tools),
                }]);
                message.provider_messages = Some(vec![serde_json::json!({
                    "role":"assistant", "content":"native transcript", "opaque":{"signature":"keep"}
                })]);
                message.image_inputs = Some(vec![ImageInput {
                    source_path:"native.png".into(), media_type:"image/png".into(), data:"aW1hZ2U=".into()
                }]);
                message.council_turn = Some(serde_json::from_value(serde_json::json!({
                    "candidateA":{"content":"candidate A", "reasoning":"keep A", "isStreaming":true},
                    "candidateB":{"content":"candidate B", "reasoning":"keep B", "isStreaming":true}
                })).unwrap());
                let mut completed = data.messages[2].clone();
                completed.is_streaming = Some(false);
                completed.run_status = Some("completed".into());
                completed.content = "already completed".into();
                let mut no_status = data.messages[3].clone();
                no_status.is_streaming = Some(false);
                no_status.content = "interrupted before status".into();
                delta.messages.extend([completed, no_status]);
                delta.artifacts.push(serde_json::from_value(serde_json::json!({
                    "id":"output", "title":"output", "type":"text", "content":"durable artifact\n",
                    "timestamp":10, "streamRevision":3, "filePath":"keep.txt"
                })).unwrap());
                storage::checkpoint_session_delta(&data.session.id, delta.clone()).await.unwrap();
                for message in &delta.messages {
                    *data.messages.iter_mut().find(|old| old.id == message.id).unwrap() = message.clone();
                }
                data.artifacts = delta.artifacts.clone();
                if seal != "none" {
                    let target = serde_json::to_vec_pretty(&data).unwrap();
                    super::prepare_commit(&path, &data, &target).await.unwrap();
                    if seal == "committed" {
                        storage::atomic_write(&path, &target).await.unwrap();
                    }
                }
                let mut expected = data;
                for index in [1, 3] {
                    let message = &mut expected.messages[index];
                    message.is_streaming = Some(false);
                    message.run_status = Some("cancelled".into());
                    for tool in message.tool_calls.iter_mut().flatten().chain(
                        message.segments.iter_mut().flatten()
                            .flat_map(|segment| segment.tool_calls.iter_mut().flatten())
                    ) {
                        if matches!(tool.status.as_str(), "pending" | "running") {
                            tool.status = "cancelled".into();
                        }
                    }
                    if let Some(council) = &mut message.council_turn {
                        council.candidate_a.is_streaming = Some(false);
                        council.candidate_b.is_streaming = Some(false);
                    }
                }
                tokio::fs::write(path.with_extension("expected"), serde_json::to_vec(&expected).unwrap())
                    .await.unwrap();
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "agent::session_journal::tests::journal_restart_child", "--nocapture"])
                    .env("AETHER_JOURNAL_RESTART_ID", &expected.session.id)
                    .env("AETHER_JOURNAL_RESTART_DIRECTORY", path.parent().unwrap())
                    .env("AETHER_JOURNAL_TEST_ROOT", storage::config_dir())
                    .env("AETHER_JOURNAL_RESTART_RECOVERY_EDIT", edit)
                    .output().unwrap();
                assert!(output.status.success(), "{edit}, seal={seal}: {}{}", 
                    String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
                assert!(!super::path_for(&path).exists());
            }
        }
    }
    #[tokio::test]
    async fn journal_catalog_fixture_does_not_scan_another_corrupt_sidecar() {
        let (bad, delta, bad_path) = start(fixture()).await;
        storage::checkpoint_session_delta(&bad.session.id, delta)
            .await
            .unwrap();
        let sidecar = super::path_for(&bad_path);
        let mut value: serde_json::Value =
            serde_json::from_slice(&tokio::fs::read(&sidecar).await.unwrap()).unwrap();
        value["messages"][0]["content"] = "tampered".into();
        tokio::fs::write(&sidecar, serde_json::to_vec(&value).unwrap())
            .await
            .unwrap();
        let before = tokio::fs::read(&sidecar).await.unwrap();
        let (good, delta, good_path) = start(fixture()).await;
        storage::checkpoint_session_delta(&good.session.id, delta)
            .await
            .unwrap();
        let sessions = storage::list_sessions_in_dir(good_path.parent().unwrap())
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, good.session.id);
        assert_eq!(sessions[0].preview.as_deref(), Some("progress"));
        assert_ne!(good_path.parent(), bad_path.parent());
        assert_eq!(before, tokio::fs::read(&sidecar).await.unwrap());
        let error = storage::list_sessions_in_dir(bad_path.parent().unwrap())
            .await
            .unwrap_err();
        assert!(error.contains("Checkpoint checksum mismatch"), "{error}");
        assert_eq!(before, tokio::fs::read(&sidecar).await.unwrap());
        storage::delete_session(&bad.session.id).await.unwrap();
    }
    #[tokio::test]
    async fn journal_catalog_folds_latest_preview() {
        let (data, delta, path) = start(fixture()).await;
        storage::checkpoint_session_delta(&data.session.id, delta)
            .await
            .unwrap();
        let sessions = storage::list_sessions_in_dir(path.parent().unwrap())
            .await
            .unwrap();
        assert_eq!(
            sessions
                .iter()
                .find(|s| s.id == data.session.id)
                .unwrap()
                .preview
                .as_deref(),
            Some("progress")
        );
    }
    #[tokio::test]
    async fn journal_recovers_cumulative_progress_from_disk() {
        let data = fixture();
        storage::save_session(data.clone()).await.unwrap();
        let page = storage::load_session_page(&data.session.id, None, 1, None)
            .await
            .unwrap();
        let mut delta = serde_json::json!({"runId":"run","baseRevision":page.window.revision,"sequence":1,"messages":[data.messages[1]],"artifacts":[]});
        delta["messages"][0]["content"] = "durable progress".into();
        storage::checkpoint_session_delta(&data.session.id, serde_json::from_value(delta).unwrap())
            .await
            .unwrap();
        let recovered = storage::load_session(&data.session.id).await.unwrap();
        assert_eq!(recovered.messages[1].content, "durable progress");
    }
}
