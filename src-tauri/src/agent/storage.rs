use super::types::{
    AgentRunRequest, AgentSettings, Artifact, AttachedContext, ChatMessage, GlobalArtifactItem,
    McpServerConfig, ProviderConfig, ProviderKind, Session, SessionData, SessionPage,
    SessionWindow,
};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;

// Weak entries avoid retaining one lock forever for every historical session ID.
static SESSION_IO_LOCKS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
> = std::sync::OnceLock::new();
pub(super) fn session_io_lock(session_id: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    let mut locks = SESSION_IO_LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(lock) = locks.get(session_id).and_then(std::sync::Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(session_id.to_owned(), std::sync::Arc::downgrade(&lock));
    lock
}
pub fn config_dir() -> PathBuf {
    #[cfg(feature = "shared-profile")]
    if let Some(root) = super::profile::root() {
        return root;
    }
    #[cfg(feature = "diagnostics")]
    if let Some(root) = super::native_smoke::profile() {
        return root.clone();
    }
    #[cfg(test)]
    {
        static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        ROOT.get_or_init(|| {
            if let Some(root) = std::env::var_os("AETHER_JOURNAL_TEST_ROOT") {
                return PathBuf::from(root);
            }
            std::env::temp_dir().join(format!("aether-native-tests-{}", Uuid::new_v4()))
        })
        .clone()
    }
    #[cfg(not(test))]
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aether-agent")
}

// Journal fixtures use explicit per-session roots without mutating process-global
// environment/configuration or changing routing in production builds.
#[cfg(test)]
static SESSION_TEST_DIRECTORIES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, PathBuf>>,
> = std::sync::OnceLock::new();
#[cfg(test)]
pub(super) fn register_session_test_directory(id: &str, root: PathBuf) {
    SESSION_TEST_DIRECTORIES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.to_owned(), root);
}

fn sessions_dir() -> PathBuf {
    config_dir().join("sessions")
}
fn settings_path() -> PathBuf {
    config_dir().join("config.json")
}
fn workspace_path() -> PathBuf {
    config_dir().join("workspace.json")
}

fn provider(
    id: &str,
    name: &str,
    kind: ProviderKind,
    base_url: &str,
    model: &str,
) -> ProviderConfig {
    ProviderConfig {
        id: id.into(),
        name: name.into(),
        kind,
        base_url: base_url.into(),
        model: model.into(),
        ..ProviderConfig::default()
    }
}

pub fn default_settings() -> AgentSettings {
    let providers = vec![
        provider(
            "gutsai",
            "GutsAI",
            ProviderKind::Openai,
            "https://api.gutsai.id/v1",
            "kimi-k2-thinking",
        ),
        provider(
            "anthropic",
            "Anthropic",
            ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "claude-sonnet-4-5",
        ),
        provider(
            "deepseek",
            "DeepSeek",
            ProviderKind::Deepseek,
            "https://api.deepseek.com/v1",
            "deepseek-chat",
        ),
        provider(
            "ollama",
            "Ollama (Local)",
            ProviderKind::Ollama,
            "http://localhost:11434/v1",
            "llama3.2",
        ),
    ];
    AgentSettings {
        active_provider_id: "gutsai".into(),
        providers,
        ..AgentSettings::default()
    }
}

pub async fn load_settings() -> Result<AgentSettings, String> {
    let defaults = default_settings();
    let content = match fs::read_to_string(settings_path()).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            save_settings(&defaults).await?;
            return Ok(defaults);
        }
        Err(error) => return Err(format!("Could not read settings: {error}")),
    };
    let mut value = serde_json::from_str::<serde_json::Value>(&content).map_err(|error| {
        format!("Settings JSON is invalid; the original file was preserved: {error}")
    })?;
    migrate_provider_kinds(&mut value);
    let mut settings = serde_json::from_value::<AgentSettings>(value).map_err(|error| {
        format!("Settings schema is invalid; the original file was preserved: {error}")
    })?;
    if settings.providers.is_empty() {
        settings.providers = defaults.providers;
    }
    if !settings
        .providers
        .iter()
        .any(|provider| provider.id == settings.active_provider_id)
    {
        settings.active_provider_id = settings.providers[0].id.clone();
    }
    settings.max_turns = settings.max_turns.clamp(1, 100);
    if !settings.temperature.is_finite() {
        settings.temperature = 0.7;
    }
    Ok(settings)
}

fn migrate_provider_kinds(value: &mut serde_json::Value) {
    let Some(providers) = value
        .get_mut("providers")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };
    for provider in providers {
        if provider.get("kind").is_some() {
            continue;
        }
        let id = provider
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let base = provider
            .get("baseUrl")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let kind = if id.contains("ollama") || base.contains("11434") {
            "ollama"
        } else if id.contains("deepseek") || base.contains("deepseek") {
            "deepseek"
        } else if id.contains("anthropic") || base.contains("anthropic") {
            "anthropic"
        } else {
            "openai"
        };
        provider["kind"] = kind.into();
    }
}

pub async fn save_settings(settings: &AgentSettings) -> Result<(), String> {
    if settings.providers.is_empty() {
        return Err("At least one provider is required".into());
    }
    if !settings.temperature.is_finite() || !(0.0..=2.0).contains(&settings.temperature) {
        return Err("Temperature must be between 0 and 2".into());
    }
    if !(1..=100).contains(&settings.max_turns) {
        return Err("Maximum agent turns must be between 1 and 100".into());
    }
    if settings.max_tokens == Some(0) {
        return Err("Max tokens must be at least 1 or left unset".into());
    }
    let mut ids = std::collections::HashSet::new();
    for provider in &settings.providers {
        validate_safe_id(&provider.id)?;
        if !ids.insert(&provider.id) {
            return Err(format!("Duplicate provider ID: {}", provider.id));
        }
        let url = Url::parse(&provider.base_url)
            .map_err(|error| format!("Invalid base URL for {}: {error}", provider.name))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(format!(
                "Provider {} requires an HTTP(S) base URL with a host",
                provider.name
            ));
        }
    }
    for server in &settings.mcp_servers {
        validate_mcp_server(server)?;
    }
    let content = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
    atomic_write(&settings_path(), &content).await
}

fn validate_safe_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(
            "IDs must contain only ASCII letters, numbers, underscores, or hyphens (max 128 chars)"
                .into(),
        );
    }
    Ok(())
}

fn session_path(id: &str) -> Result<PathBuf, String> {
    validate_safe_id(id)?;
    #[cfg(test)]
    if let Some(root) = SESSION_TEST_DIRECTORIES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .cloned()
    {
        return Ok(root.join(format!("{id}.json")));
    }
    Ok(sessions_dir().join(format!("{id}.json")))
}

pub async fn list_sessions() -> Result<Vec<Session>, String> {
    list_sessions_in_dir(&sessions_dir()).await
}

// Keep recovery, projection reads and live overlays scoped to the same root.
pub(super) async fn list_sessions_in_dir(root: &Path) -> Result<Vec<Session>, String> {
    recover_session_journals(root).await?;
    let catalog_root = root.to_path_buf();
    let mut sessions = tokio::task::spawn_blocking(move || {
        repair_dirty_projections()?;
        super::session_pages::list_catalog(&catalog_root)
    })
    .await
    .map_err(|e| e.to_string())??;
    for session in &mut sessions {
        validate_safe_id(&session.id)?;
        let path = root.join(format!("{}.json", session.id));
        let _guard = session_io_lock(&session.id).lock_owned().await;
        if super::session_journal::is_active(&path) {
            super::session_journal::overlay_active_catalog(&path, session).await?;
        }
    }
    Ok(sessions)
}
#[cfg(not(test))]
type DirectorySessionData = SessionData;
#[cfg(test)]
type DirectorySessionData = tests::TrackedDirectorySession;

// Gagal secara jelas, bukan mengembalikan katalog parsial yang menyesatkan.
// Pemanggil mengekstrak hasil per berkas sebelum riwayat berikutnya dibaca.
async fn visit_session_directory(
    root: &Path,
    mut visit: impl FnMut(DirectorySessionData),
) -> Result<(), String> {
    let mut entries = match fs::read_dir(root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Could not list {}: {error}", root.display())),
    };
    let mut valid_sessions = 0;
    let mut diagnostics = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("Could not enumerate {}: {error}", root.display()))?
    {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        // Penanda penghapusan permanen mengalahkan sisa JSON, termasuk yang rusak.
        if fs::try_exists(path.with_extension("deleted"))
            .await
            .map_err(|error| error.to_string())?
        {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let _guard = session_io_lock(id).lock_owned().await;
        match read_session_file(&path, id).await {
            Ok(Some(session)) => {
                #[cfg(test)]
                let session = tests::TrackedDirectorySession::new(&path, session);
                valid_sessions += 1;
                visit(session);
            }
            Ok(None) => diagnostics.push(format!("{} disappeared during listing", path.display())),
            Err(error) => diagnostics.push(error),
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Session catalog has unreadable entries ({} valid sessions preserved):\n{}",
            valid_sessions,
            diagnostics.join("\n")
        ))
    }
}

#[cfg(test)]
async fn read_session_directory(root: &Path) -> Result<Vec<DirectorySessionData>, String> {
    let mut data = Vec::new();
    visit_session_directory(root, |session| data.push(session)).await?;
    Ok(data)
}

pub async fn list_all_artifacts() -> Result<Vec<GlobalArtifactItem>, String> {
    list_all_artifacts_in_dir(&sessions_dir()).await
}

async fn list_all_artifacts_in_dir(root: &Path) -> Result<Vec<GlobalArtifactItem>, String> {
    let mut items = Vec::new();
    visit_session_directory(root, |data| {
        #[cfg(test)]
        let (data, _snapshot) = data.into_parts();
        let SessionData {
            session,
            artifacts,
            messages,
            model_override,
        } = data;
        // Lepaskan seluruh riwayat beserta base64 sebelum membaca sesi berikutnya.
        drop(messages);
        drop(model_override);
        for art in artifacts {
            items.push(GlobalArtifactItem {
                session_id: session.id.clone(),
                session_title: session.title.clone(),
                session_updated_at: session.updated_at,
                artifact: art,
            });
        }
    })
    .await?;
    items.sort_by_key(|a| std::cmp::Reverse(a.artifact.timestamp));
    Ok(items)
}

pub async fn load_session(session_id: &str) -> Result<SessionData, String> {
    let _guard = session_io_lock(session_id).lock_owned().await;
    load_session_unlocked(session_id).await
}

async fn load_session_unlocked(session_id: &str) -> Result<SessionData, String> {
    let path = session_path(session_id)?;
    read_session_file(&path, session_id)
        .await?
        .ok_or_else(|| format!("Session {session_id} was not found"))
}

async fn read_session_file(path: &Path, expected_id: &str) -> Result<Option<SessionData>, String> {
    let mut data = read_session_file_raw(path, expected_id).await?;
    if let Some(data) = &mut data {
        let recover = super::session_journal::needs_recovery(path).await?;
        if super::session_journal::overlay(path, data, recover).await? {
            data.session.message_count = data.messages.len();
            data.session.preview = session_preview(&data.messages);
            if recover {
                data.session.updated_at = chrono::Utc::now().timestamp_millis();
                write_session_atomic(path, data).await?;
            }
        }
    }
    Ok(data)
}
async fn read_session_file_raw(
    path: &Path,
    expected_id: &str,
) -> Result<Option<SessionData>, String> {
    if fs::try_exists(path.with_extension("deleted"))
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("Session was deleted".into());
    }
    let content = match fs::read_to_string(path).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
    };
    let value = serde_json::from_str(&content).map_err(|error| {
        format!(
            "{}: invalid JSON; original preserved: {error}",
            path.display()
        )
    })?;
    let data = parse_session(value)
        .map_err(|error| format!("{}: {error}; original preserved", path.display()))?;
    validate_safe_id(&data.session.id)?;
    if data.session.id != expected_id {
        return Err(format!(
            "{}: session ID does not match filename; original preserved",
            path.display()
        ));
    }
    Ok(Some(data))
}

pub(super) fn parse_session(value: serde_json::Value) -> Result<SessionData, String> {
    if !value.is_object() {
        return Err("Session must be a JSON object".into());
    }
    if value.get("session").is_some() {
        return serde_json::from_value(value)
            .map_err(|error| format!("Session schema is invalid: {error}"));
    }
    let id = value
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or("Legacy session requires an ID")?;
    validate_safe_id(id)?;
    let session = Session {
        id: value
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        title: value
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or("Untitled")
            .to_string(),
        created_at: value
            .get("createdAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
        updated_at: value
            .get("updatedAt")
            .and_then(|value| value.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
        message_count: value
            .get("messages")
            .and_then(|value| value.as_array())
            .map(|messages| messages.len())
            .unwrap_or(0),
        workspace_cwd: value
            .get("workspaceCwd")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        preview: value
            .get("preview")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        pinned: value
            .get("pinned")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        pinned_at: value.get("pinnedAt").and_then(|value| value.as_i64()),
    };
    let messages = value
        .get("messages")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let artifacts = value
        .get("artifacts")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let model_override = value
        .get("modelOverride")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(None);
    Ok(SessionData {
        session,
        messages,
        artifacts,
        model_override,
    })
}

/// Stable wire contract for a canonical commit whose cursor is not acknowledged.
/// The proof binds ALL bytes, including unloaded prefix, artifacts and replay data.
/// It is a compare-only recovery receipt, never permission to rebase or overwrite.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommittedSaveProof {
    pub version: u8,
    pub session_id: String,
    pub canonical_sha256: String,
    pub offset: usize,
    pub total: usize,
    pub first_message_id: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommittedSaveError {
    pub proof: CommittedSaveProof,
    pub cause: String,
}
impl CommittedSaveError {
    const PREFIX: &'static str = "[canonical-committed]";
    pub fn parse(error: &str) -> Option<Self> {
        serde_json::from_str(error.strip_prefix(Self::PREFIX)?).ok()
    }
    fn wire(&self) -> String {
        format!(
            "{}{}",
            Self::PREFIX,
            serde_json::to_string(self).expect("receipt serializes")
        )
    }
    fn for_data(data: &SessionData, offset: usize, cause: String) -> String {
        let bytes = serde_json::to_vec_pretty(data).expect("canonical data already serialized");
        Self {
            proof: CommittedSaveProof {
                version: 1,
                session_id: data.session.id.clone(),
                canonical_sha256: canonical_sha256(&bytes),
                offset,
                total: data.messages.len(),
                first_message_id: data.messages.get(offset).map(|m| m.id.clone()),
            },
            cause,
        }
        .wire()
    }
    fn at_offset(error: String, offset: usize, first_message_id: Option<String>) -> String {
        match Self::parse(&error) {
            Some(mut receipt) => {
                receipt.proof.offset = offset;
                receipt.proof.first_message_id = first_message_id;
                receipt.wire()
            }
            None => error,
        }
    }
}
fn canonical_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// Recover only the exact canonical commit named by a failed save receipt.
/// No incoming snapshot is merged and canonical JSON is NEVER rewritten here.
/// Full replay payloads are returned for the original writable suffix so the
/// caller can establish an exact committed baseline without replacing newer UI text.
pub async fn recover_save_ack(
    session_id: &str,
    proof: CommittedSaveProof,
) -> Result<SessionPage, String> {
    let _guard = session_io_lock(session_id).lock_owned().await;
    let path = session_path(session_id)?;
    recover_save_ack_file(&path, session_id, proof).await
}
async fn recover_save_ack_file(
    path: &Path,
    session_id: &str,
    proof: CommittedSaveProof,
) -> Result<SessionPage, String> {
    if proof.version != 1 || proof.session_id != session_id {
        return Err("Invalid committed save proof; snapshot preserved".into());
    }
    if path.with_extension("deleted").exists() {
        return Err("Session was deleted; committed save recovery refused".into());
    }
    let raw = fs::read(path).await.map_err(|e| e.to_string())?;
    if canonical_sha256(&raw) != proof.canonical_sha256 {
        return Err("Canonical changed after commit; recovery conflict, snapshot preserved".into());
    }
    let data = parse_session(serde_json::from_slice(&raw).map_err(|e| e.to_string())?)?;
    if data.session.id != session_id
        || data.messages.len() != proof.total
        || proof.offset > proof.total
        || data.messages.get(proof.offset).map(|m| m.id.clone()) != proof.first_message_id
    {
        return Err("Committed save boundary mismatch; snapshot preserved".into());
    }
    // Rebuild from hash-verified bytes, not a size/mtime cache hit. A real
    // projection transaction and read must succeed before returning ANY cursor.
    super::session_pages::cache_session(path, &data)?;
    let page = super::session_pages::load_page(path, session_id, Some(0), 1, None)?;
    if path.with_extension("deleted").exists()
        || fs::read(path).await.map_err(|e| e.to_string())? != raw
    {
        let _ = super::session_pages::invalidate(path);
        return Err("Canonical changed during recovery; snapshot preserved".into());
    }
    super::session_journal::acknowledge_committed(path, &raw).await?;
    dirty_projections()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(path);
    let window = SessionWindow {
        end: None,
        offset: proof.offset,
        total: data.messages.len(),
        revision: page.window.revision,
        first_message_id: proof.first_message_id,
    };
    let mut data = data;
    data.messages = data.messages.split_off(proof.offset);
    Ok(SessionPage { data, window })
}

pub async fn save_session(data: SessionData) -> Result<(), String> {
    let _guard = session_io_lock(&data.session.id).lock_owned().await;
    save_session_unlocked(data).await
}

/// Explicit full-history replacement. A present cursor must describe the entire
/// hydrated canonical baseline. None is create-only, never an unchecked overwrite.
/// The returned data is intentionally FULL (including opaque replay bytes), not
/// a viewport projection: it is the exact committed controller baseline.
pub async fn replace_session_checked(
    data: SessionData,
    expected_window: Option<SessionWindow>,
) -> Result<SessionPage, String> {
    let _guard = session_io_lock(&data.session.id).lock_owned().await;
    let path = session_path(&data.session.id)?;
    replace_session_checked_file(&path, data, expected_window).await
}
async fn replace_session_checked_file(
    path: &Path,
    data: SessionData,
    expected_window: Option<SessionWindow>,
) -> Result<SessionPage, String> {
    recover_session_journal_if_needed(path, &data.session.id).await?;
    match expected_window {
        Some(expected) => {
            if expected.end.is_some() || expected.offset != 0 || expected.revision.is_empty() {
                return Err("History replacement requires a full canonical cursor".into());
            }
            let current = page_at(
                path,
                &data.session.id,
                Some(expected.total.min(1)),
                1,
                Some(&expected.revision),
            )?;
            if current.window.total != expected.total
                || current.data.messages.first().map(|m| &m.id)
                    != expected.first_message_id.as_ref()
            {
                return Err("History replacement baseline changed; reload explicitly".into());
            }
        }
        None => {
            if fs::try_exists(&path).await.map_err(|e| e.to_string())? {
                return Err("Existing history requires an expected revision".into());
            }
        }
    }
    let mut ids = std::collections::HashSet::new();
    for message in &data.messages {
        if message.id.is_empty() || !ids.insert(&message.id) {
            return Err("History replacement requires unique message IDs".into());
        }
        if message.has_image_inputs == Some(true) && message.image_inputs.is_none()
            || message.has_provider_messages == Some(true) && message.provider_messages.is_none()
        {
            return Err("History replacement must hydrate omitted replay payloads".into());
        }
    }
    // Keep all existing tombstone, metadata, native-review and journal safeguards.
    // These permit truncation of idle, explicitly hydrated history.
    let stored = save_session_file(path, data, false).await?;
    let _ = super::search::index_session(&stored);
    // Never load/deserialize canonical again after writing, nor fabricate a
    // revision on projection failure. Report the known post-commit condition.
    let page = page_at(path, &stored.session.id, Some(0), 1, None)
        .map_err(|e| CommittedSaveError::for_data(&stored, 0, e))?;
    let window = SessionWindow {
        end: None,
        offset: 0,
        total: stored.messages.len(),
        revision: page.window.revision,
        first_message_id: stored.messages.first().map(|m| m.id.clone()),
    };
    Ok(SessionPage {
        data: stored,
        window,
    })
}

/// Mid-run snapshots use the same atomic, synced replacement and tombstone /
/// metadata protections as final saves, without rebuilding FTS on every tick.
/// The completed save reconciles the index when the run ends.
pub async fn checkpoint_session(data: SessionData) -> Result<(), String> {
    let _guard = session_io_lock(&data.session.id).lock_owned().await;
    let path = session_path(&data.session.id)?;
    save_session_file(&path, data, false).await?;
    Ok(())
}

#[derive(Hash)]
struct ReviewKey<'a>(&'a str, Option<&'a str>);
impl PartialEq for ReviewKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        #[cfg(test)]
        REVIEW_COMPARISONS.with(|count| count.set(count.get() + 1));
        self.0 == other.0 && self.1 == other.1
    }
}
impl Eq for ReviewKey<'_> {}
#[cfg(test)]
thread_local! { static REVIEW_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
fn review_key(message: &ChatMessage) -> ReviewKey<'_> {
    ReviewKey(&message.id, message.run_id.as_deref())
}
fn merge_self_improvements(data: &mut SessionData, previous: &SessionData) {
    let mut stored_by_key = std::collections::HashMap::with_capacity(previous.messages.len());
    for stored in &previous.messages {
        // Preserve find's first-match semantics, including entries without a review.
        stored_by_key.entry(review_key(stored)).or_insert(stored);
    }
    for message in &mut data.messages {
        if let Some(stored) = stored_by_key.get(&review_key(message)) {
            // Native review results win over stale frontend snapshots of the same turn.
            if stored.self_improvement.is_some() {
                message.self_improvement = stored.self_improvement.clone();
            }
        }
    }
}

/// Merge retries into native history without resurrecting resolved proposals.
fn merge_review_events(
    current: &super::types::SelfImprovementEvent,
    incoming: &super::types::SelfImprovementEvent,
) -> super::types::SelfImprovementEvent {
    use super::types::SelfImprovementAction as Action;
    if current.run_id != incoming.run_id || current.session_id != incoming.session_id {
        return incoming.clone();
    }
    // Scalar-only legacy reviews have no proposal identity to reconcile. A new
    // manual conclusion may replace the previous conclusion for this turn.
    if current.changes.is_empty() && incoming.changes.is_empty() {
        return incoming.clone();
    }
    let mut merged = current.clone();
    for change in &incoming.changes {
        if let Some(stored) = change.pending_id.as_ref().and_then(|id| {
            merged
                .changes
                .iter_mut()
                .find(|c| c.pending_id.as_ref() == Some(id))
        }) {
            // Once native history resolves an ID (including rejection), it wins
            // against both stale staging and conflicting duplicate resolutions.
            if stored.action == Action::SkillStaged && change.action != Action::SkillStaged {
                *stored = change.clone();
            }
        } else if !merged.changes.contains(change) {
            // Applied manual edits have no pending ID. Only exact duplicates
            // collapse; two edits to the same path/action can be distinct work.
            merged.changes.push(change.clone());
        }
    }
    if merged.changes == current.changes {
        return current.clone();
    }
    let active = merged.changes.iter().find(|c| c.action != Action::NoAction);
    merged.action = active.map(|c| c.action.clone()).unwrap_or(Action::NoAction);
    merged.skill_name = active.map(|c| c.skill_name.clone());
    merged.summary = incoming.summary.clone().or_else(|| current.summary.clone());
    merged.workspace_cwd = current
        .workspace_cwd
        .clone()
        .or_else(|| incoming.workspace_cwd.clone());
    merged
}
pub async fn record_self_improvement(
    event: &super::types::SelfImprovementEvent,
) -> Result<(), String> {
    let _guard = session_io_lock(&event.session_id).lock_owned().await;
    record_self_improvement_file(&session_path(&event.session_id)?, event).await
}

async fn record_self_improvement_file(
    path: &Path,
    event: &super::types::SelfImprovementEvent,
) -> Result<(), String> {
    let mut data = read_session_file(path, &event.session_id)
        .await?
        .ok_or_else(|| format!("Session {} was not found", event.session_id))?;
    let message = data
        .messages
        .iter_mut()
        .find(|message| {
            message.role == "assistant"
                && (message.run_id.as_deref() == Some(event.run_id.as_str())
                    || message.id == event.run_id)
        })
        .ok_or("The reviewed turn no longer exists in this session")?;
    let merged = match &message.self_improvement {
        Some(current) => merge_review_events(current, event),
        None => event.clone(),
    };
    // Replays must not invalidate a frontend window that already saw this commit.
    if message.self_improvement.as_ref() == Some(&merged) {
        return Ok(());
    }
    message.self_improvement = Some(merged);
    write_session_atomic(path, &data).await
}

/// Recover a missing foreground review link from native staging authority only.
/// The root is supplied by native code, never by an IPC caller.
pub async fn attach_pending_link(root: &Path, id: &str, session_id: &str) -> Result<(), String> {
    let write = load_pending_link(root, id, session_id).await?;
    let _guard = session_io_lock(session_id).lock_owned().await;
    attach_pending_link_file(&session_path(session_id)?, &write).await
}

async fn load_pending_link(
    root: &Path,
    id: &str,
    session_id: &str,
) -> Result<super::review_pending::PendingWrite, String> {
    let public = super::review_pending::pending_diff(root, id, session_id).await?;
    let authority = root.join(".authority");
    let sealed = super::review_pending::pending_diff(&authority, id, session_id).await?;
    if public != sealed {
        return Err("Pending staging snapshot mismatch".into());
    }
    // pending_diff checks the UUID, bounded file and every path component.
    let bytes = fs::read(authority.join(format!("{id}.json")))
        .await
        .map_err(|e| e.to_string())?;
    if bytes.len() > 256 * 1024 * 16 {
        return Err("Pending staging snapshot too large".into());
    }
    let write: super::review_pending::PendingWrite =
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let public_bytes = fs::read(root.join(format!("{id}.json")))
        .await
        .map_err(|e| e.to_string())?;
    let public_write: super::review_pending::PendingWrite =
        serde_json::from_slice(&public_bytes).map_err(|e| e.to_string())?;
    if write.id != id
        || write.session_id != session_id
        || serde_json::to_vec(&public_write).map_err(|e| e.to_string())? != bytes
    {
        return Err("Pending staging snapshot mismatch".into());
    }
    Ok(write)
}

async fn attach_pending_link_file(
    path: &Path,
    write: &super::review_pending::PendingWrite,
) -> Result<(), String> {
    use super::types::{
        SelfImprovementAction as Action, SelfImprovementChange, SelfImprovementEvent,
    };
    let mut data = read_session_file(path, &write.session_id)
        .await?
        .ok_or("Session not found")?;
    let workspace_cwd = data.session.workspace_cwd.clone();
    let message = data
        .messages
        .iter_mut()
        .find(|m| {
            m.role == "assistant"
                && !write.run_id.is_empty()
                && (m.run_id.as_deref() == Some(write.run_id.as_str())
                    || m.id == write.run_id
                    || m.self_improvement.as_ref().is_some_and(|e| {
                        e.session_id == write.session_id && e.run_id == write.run_id
                    }))
        })
        .ok_or(
            "The originating assistant turn is not saved; wait for the run to finish and retry",
        )?;
    if let Some(event) = &message.self_improvement {
        if event.session_id != write.session_id || event.run_id != write.run_id {
            return Err("Review identity does not match the pending proposal".into());
        }
        if let Some(link) = event
            .changes
            .iter()
            .find(|c| c.pending_id.as_deref() == Some(write.id.as_str()))
        {
            if link.skill_name != write.skill_name
                || link.file_path.as_deref() != Some(write.target_path.as_str())
                || link.skill_path.as_deref() != Some(write.skill_root.as_str())
            {
                return Err("Pending proposal does not match its review link".into());
            }
            return Ok(()); // Never resurrect an already resolved change.
        }
    }
    let event = message
        .self_improvement
        .get_or_insert_with(|| SelfImprovementEvent {
            changes: Vec::new(),
            run_id: write.run_id.clone(),
            session_id: write.session_id.clone(),
            action: Action::SkillStaged,
            skill_name: Some(write.skill_name.clone()),
            summary: None,
            workspace_cwd,
        });
    event.changes.push(SelfImprovementChange {
        action: Action::SkillStaged,
        skill_name: write.skill_name.clone(),
        file_path: Some(write.target_path.clone()),
        skill_path: Some(write.skill_root.clone()),
        pending_id: Some(write.id.clone()),
        summary: Some("Awaiting review approval".into()),
    });
    if event.action == Action::NoAction {
        event.action = Action::SkillStaged;
        event.skill_name = Some(write.skill_name.clone());
    }
    write_session_atomic(path, &data).await
}
/// Resolve a staged change in native history under the same lock as all session
/// saves. Do not use save_session_file: its stale-snapshot merge intentionally
/// preserves the old native event and would undo this authoritative update.
pub async fn resolve_review_proposal(
    session_id: &str,
    pending_id: &str,
    change: Option<super::types::SelfImprovementChange>,
) -> Result<(), String> {
    let _guard = session_io_lock(session_id).lock_owned().await;
    resolve_review_proposal_file(&session_path(session_id)?, session_id, pending_id, change).await
}

async fn resolve_review_proposal_file(
    path: &Path,
    session_id: &str,
    pending_id: &str,
    change: Option<super::types::SelfImprovementChange>,
) -> Result<(), String> {
    use super::types::SelfImprovementAction;
    let mut data = read_session_file(path, session_id)
        .await?
        .ok_or("Session not found")?;
    let mut matched = false;
    for event in data
        .messages
        .iter_mut()
        .filter_map(|m| m.self_improvement.as_mut())
    {
        if event.session_id != session_id {
            continue;
        }
        let mut affected = false;
        for stored in &mut event.changes {
            if stored.pending_id.as_deref() != Some(pending_id) {
                continue;
            }
            if let Some(approved) = &change {
                // Identity belongs to the original linked proposal, not to a
                // caller's replacement object. The pending ID remains for UI reload.
                if approved.skill_name != stored.skill_name
                    || approved.file_path != stored.file_path
                    || approved.skill_path != stored.skill_path
                {
                    return Err("Approved change does not match the staged proposal".into());
                }
                *stored = approved.clone();
                stored.pending_id = Some(pending_id.into());
            } else {
                stored.action = SelfImprovementAction::NoAction;
                stored.summary = Some("Rejected review skill proposal".into());
            }
            affected = true;
            matched = true;
        }
        if affected {
            let active = event
                .changes
                .iter()
                .find(|c| c.action != SelfImprovementAction::NoAction);
            event.action = active
                .map(|c| c.action.clone())
                .unwrap_or(SelfImprovementAction::NoAction);
            event.skill_name = active.map(|c| c.skill_name.clone());
            // Preserve legacy event summary, run/session identity and workspace.
        }
    }
    if !matched {
        return Err("Pending proposal is not attached to a review in this session".into());
    }
    write_session_atomic(path, &data).await
}

/// Merge pin state dari sesi yang tersimpan di disk ke `data`.
/// `pin_is_authoritative == true` berarti pemanggil (mis. `toggle_pin_session`)
/// sudah menentukan state pin secara eksplisit — disk tidak boleh menimpanya,
/// sehingga pin yang baru dilepas tidak "hidup" lagi pada save berikutnya.
fn merge_pin_state(data: &mut Session, previous: &Session, pin_is_authoritative: bool) {
    if pin_is_authoritative {
        return;
    }
    data.pinned = previous.pinned;
    data.pinned_at = previous.pinned_at;
}

async fn save_session_unlocked(data: SessionData) -> Result<(), String> {
    save_session_unlocked_with_pin_policy(data, false).await
}

async fn save_session_unlocked_with_pin_policy(
    data: SessionData,
    pin_is_authoritative: bool,
) -> Result<(), String> {
    let path = session_path(&data.session.id)?;
    let stored = save_session_file(&path, data, pin_is_authoritative).await?;
    let _ = super::search::index_session(&stored);
    Ok(())
}

async fn save_session_file(
    path: &Path,
    mut data: SessionData,
    metadata_is_authoritative: bool,
) -> Result<SessionData, String> {
    let mut previous = read_session_file_raw(path, &data.session.id).await?;
    if let Some(previous) = &mut previous {
        super::session_journal::overlay(path, previous, false).await?;
        // Hydrate from canonical plus journal before validating and writing.
        // Journal-only replay bytes must not disappear with a thin final save.
        retain_history_payload(&mut data.messages, previous);
    }
    super::session_journal::validate_snapshot(path, &data).await?;
    save_session_file_with_previous(path, data, metadata_is_authoritative, previous.as_ref()).await
}

async fn save_session_file_with_previous(
    path: &Path,
    mut data: SessionData,
    metadata_is_authoritative: bool,
    previous: Option<&SessionData>,
) -> Result<SessionData, String> {
    if fs::try_exists(path.with_extension("deleted"))
        .await
        .map_err(|error| error.to_string())?
    {
        return Err(
            "Session was deleted; stale snapshots cannot recreate it. Create a new session ID."
                .into(),
        );
    }
    // Full append snapshots may also originate from lightweight loaded history.
    // Absence means omitted, not deletion; explicit hydrated destructive edits
    // still control which message IDs remain in the snapshot.
    if let Some(previous) = previous {
        retain_history_payload(&mut data.messages, previous);
    }
    for message in &mut data.messages {
        message.has_image_inputs = None;
        message.has_provider_messages = None;
    }
    let now = chrono::Utc::now().timestamp_millis();
    let mut is_substantive_change = true;
    if let Some(previous) = previous {
        super::session_journal::validate_canonical_progress(previous, &data)?;
        data.session.created_at = previous.session.created_at;
        // Native metadata wins over queued snapshots once a conversation exists.
        // Empty sessions may still acquire the first automatically generated title.
        if !metadata_is_authoritative
            && !(previous.messages.is_empty() && previous.session.title == "New Chat")
        {
            data.session.title = previous.session.title.clone();
        }
        merge_pin_state(
            &mut data.session,
            &previous.session,
            metadata_is_authoritative,
        );
        merge_self_improvements(&mut data, previous);

        // Preserve previous updated_at if session content is unmodified,
        // preventing sidebar jump/swap on simple navigation or read.
        if previous.session.updated_at > 0
            && data.session.title == previous.session.title
            && data.session.workspace_cwd == previous.session.workspace_cwd
            && data.session.pinned == previous.session.pinned
            && !substantive_changed(&data, previous)?
        {
            is_substantive_change = false;
            data.session.updated_at = previous.session.updated_at;
        }
    }
    if data.session.created_at <= 0 {
        data.session.created_at = now;
    }
    if is_substantive_change {
        data.session.updated_at = now;
    }
    data.session.message_count = data.messages.len();
    data.session.title = bounded_title(&data.session.title);
    data.session.preview = session_preview(&data.messages);
    #[cfg(target_os = "windows")]
    if let Some(cwd) = &data.session.workspace_cwd {
        let clean = cwd.strip_prefix(r"\\?\").unwrap_or(cwd).to_string();
        data.session.workspace_cwd = Some(clean);
    }
    write_session_atomic(path, &data).await?;
    Ok(data)
}

fn substantive_changed(data: &SessionData, previous: &SessionData) -> Result<bool, String> {
    data_changed_with(data, previous, || {
        Ok(
            serde_json::to_value((&data.messages, &data.artifacts, &data.model_override))
                .map_err(|e| e.to_string())?
                != serde_json::to_value((
                    &previous.messages,
                    &previous.artifacts,
                    &previous.model_override,
                ))
                .map_err(|e| e.to_string())?,
        )
    })
}

fn data_changed_with(
    data: &SessionData,
    previous: &SessionData,
    exact_changed: impl FnOnce() -> Result<bool, String>,
) -> Result<bool, String> {
    // Guard ini hanya membuktikan ketidaksamaan lewat field pinjaman yang selalu
    // diserialisasi; jangan menyimpulkan seluruh sesi sama dari subset ini.
    // Tipe sesi memakai derive Serialize dan Value (tanpa serializer kustom atau
    // key peta non-string), sehingga guard tidak melewati error serialisasi data.
    if data.messages.len() != previous.messages.len()
        || data.artifacts.len() != previous.artifacts.len()
        || data
            .messages
            .iter()
            .zip(&previous.messages)
            .any(|(a, b)| a.content != b.content || a.reasoning != b.reasoning)
        || data
            .artifacts
            .iter()
            .zip(&previous.artifacts)
            .any(|(a, b)| a.content != b.content)
    {
        return Ok(true);
    }
    // Gambar, providerMessages, segments, Council, override, serta field lainnya
    // tetap memakai pembandingan JSON persis, termasuk propagasi error-nya.
    // Optimasi ini hanya menghindari alokasi pembandingan saat perubahan pasti;
    // payload IPC penuh, pembacaan sesi, dan penulisan disk penuh tetap ada.
    exact_changed()
}
fn bounded_title(title: &str) -> String {
    let trimmed = title.trim();
    let value = if trimmed.is_empty() {
        "New Chat"
    } else {
        trimmed
    };
    value.chars().take(200).collect()
}

fn session_preview(messages: &[ChatMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| {
            matches!(message.role.as_str(), "user" | "assistant")
                && !message.content.trim().is_empty()
        })
        .map(|message| {
            message
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(240)
                .collect()
        })
}

pub async fn rename_session(session_id: &str, title: &str) -> Result<SessionData, String> {
    if title.trim().is_empty() {
        return Err("Session title must not be empty".into());
    }
    let _guard = session_io_lock(session_id).lock_owned().await;
    let mut data = load_session_unlocked(session_id).await?;
    data.session.title = bounded_title(title);
    save_session_unlocked_with_pin_policy(data, true).await?;
    load_session_unlocked(session_id).await
}

pub async fn toggle_pin_session(session_id: &str) -> Result<Session, String> {
    let _guard = session_io_lock(session_id).lock_owned().await;
    let mut data = load_session_unlocked(session_id).await?;
    data.session.pinned = !data.session.pinned;
    if data.session.pinned {
        data.session.pinned_at = Some(chrono::Utc::now().timestamp_millis());
    } else {
        data.session.pinned_at = None;
    }
    save_session_unlocked_with_pin_policy(data, true).await?;
    Ok(load_session_unlocked(session_id).await?.session)
}

pub async fn delete_session(session_id: &str) -> Result<(), String> {
    let _guard = session_io_lock(session_id).lock_owned().await;
    let path = session_path(session_id)?;
    // A durable marker prevents late snapshots from recreating a deleted ID.
    atomic_write(&path.with_extension("deleted"), b"deleted\n").await?;
    super::session_journal::clear(&path).await?;
    let _ = super::session_pages::invalidate(&path);
    match fs::remove_file(&path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Could not delete session: {error}")),
    }
    let _ = super::search::delete_session_index(session_id);
    Ok(())
}

fn bounded_summary_text(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut text: String = normalized.chars().take(max_chars).collect();
    if normalized.chars().count() > max_chars {
        text.push('…');
    }
    text
}

fn tool_call_path(call: &super::types::ToolCallItem) -> Option<String> {
    let from_value = |value: &serde_json::Value| {
        value
            .get("path")
            .or_else(|| value.get("filePath"))
            .or_else(|| value.get("file_path"))
            .and_then(|path| path.as_str())
            .map(str::to_string)
    };
    match &call.args {
        serde_json::Value::String(raw) => serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|value| from_value(&value)),
        value => from_value(value),
    }
}

#[derive(Debug)]
pub(crate) enum TranscriptPart<'a> {
    Text(&'a str),
    Tool(&'a super::types::ToolCallItem),
}

pub(crate) fn ordered_transcript(message: &ChatMessage) -> Vec<TranscriptPart<'_>> {
    let mut parts = Vec::new();
    let mut seen_tools = std::collections::HashSet::new();
    if let Some(segments) = message
        .segments
        .as_ref()
        .filter(|segments| !segments.is_empty())
    {
        // Segments are authoritative: preamble/content are flattened UI mirrors.
        for segment in segments {
            if let Some(text) = segment.text.as_deref().filter(|text| !text.is_empty()) {
                parts.push(TranscriptPart::Text(text));
            }
            for call in segment.tool_calls.iter().flatten() {
                if seen_tools.insert(call.id.as_str()) {
                    let latest = message
                        .tool_calls
                        .iter()
                        .flatten()
                        .find(|current| current.id == call.id)
                        .unwrap_or(call);
                    parts.push(TranscriptPart::Tool(latest));
                }
            }
        }
        // Frontend content concatenates all post-preamble chunks, not just the
        // final segment. Compare the joined narrative, retaining only a truly
        // absent tail from older snapshots with incomplete segment persistence.
        let narrative: String = segments
            .iter()
            .filter_map(|segment| segment.text.as_deref())
            .collect();
        let aggregate_narrative = message
            .preamble
            .as_deref()
            .and_then(|preamble| narrative.strip_prefix(preamble))
            .unwrap_or(&narrative);
        let mut offset = 0;
        let mirrored = segments.iter().any(|segment| {
            let matches = narrative[offset..] == message.content;
            offset += segment.text.as_deref().unwrap_or_default().len();
            matches
        });
        let missing = if mirrored {
            ""
        } else {
            message
                .content
                .strip_prefix(aggregate_narrative)
                .unwrap_or(&message.content)
        };
        if !missing.is_empty() {
            parts.push(TranscriptPart::Text(missing));
        }
    } else {
        if let Some(preamble) = message.preamble.as_deref().filter(|text| !text.is_empty()) {
            parts.push(TranscriptPart::Text(preamble));
        }
        for call in message.tool_calls.iter().flatten() {
            if seen_tools.insert(call.id.as_str()) {
                parts.push(TranscriptPart::Tool(call));
            }
        }
        if !message.content.is_empty()
            && message.preamble.as_deref() != Some(message.content.as_str())
        {
            parts.push(TranscriptPart::Text(&message.content));
        }
    }
    for call in message.tool_calls.iter().flatten() {
        if seen_tools.insert(call.id.as_str()) {
            parts.push(TranscriptPart::Tool(call));
        }
    }
    parts
}

fn transcript_text(message: &ChatMessage) -> String {
    ordered_transcript(message)
        .into_iter()
        .map(|part| match part {
            TranscriptPart::Text(text) => text.to_string(),
            TranscriptPart::Tool(call) => format!(
                "Tool {} [{}] (id: {})\nArguments: {}\nResult: {}",
                call.name,
                call.status,
                call.id,
                call.args,
                call.result
                    .as_deref()
                    .or(call.terminal_output.as_deref())
                    .unwrap_or("(no result)")
            ),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn deterministic_summary(messages: &[ChatMessage]) -> String {
    let mut goals = Vec::new();
    let mut checkpoints = Vec::new();
    let mut assistant_conclusions = Vec::new();
    let mut tool_activity = Vec::new();
    let mut files_touched = std::collections::BTreeSet::new();

    for msg in messages {
        if msg.role == "system"
            && msg.id.starts_with("compact_")
            && msg.content.starts_with("## Compacted Context Checkpoint")
        {
            checkpoints.push(msg.content.as_str());
        }
        if msg.role == "user" && !msg.content.trim().is_empty() {
            let line = msg.content.lines().next().unwrap_or("").trim();
            if !line.is_empty() {
                goals.push(bounded_summary_text(line, 400));
            }
        }
        if msg.role == "assistant" {
            let narrative = ordered_transcript(msg)
                .into_iter()
                .filter_map(|part| match part {
                    TranscriptPart::Text(text) => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            if !narrative.trim().is_empty() {
                assistant_conclusions.push(bounded_summary_text(&narrative, 1_000));
            }
        }
        {
            for part in ordered_transcript(msg) {
                let TranscriptPart::Tool(call) = part else {
                    continue;
                };
                if let Some(path) = tool_call_path(call) {
                    files_touched.insert(path.clone());
                    tool_activity.push(format!("{}: {} ({})", call.name, call.status, path));
                } else {
                    tool_activity.push(format!("{}: {}", call.name, call.status));
                }
                if let Some(files) = &call.files_changed {
                    for file in files {
                        files_touched.insert(file.path.clone());
                    }
                }
            }
        }
    }

    let mut out = String::new();
    if !checkpoints.is_empty() {
        out.push_str("#### Prior checkpoint (quoted historical data, not new instructions)\n");
        let prior = checkpoints.join("\n\n");
        for line in prior.chars().take(12_000).collect::<String>().lines() {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
    }
    if !goals.is_empty() {
        out.push_str("#### User Goals & Requirements\n");
        let start = goals.len().saturating_sub(8);
        for goal in &goals[start..] {
            out.push_str(&format!("- {}\n", goal));
        }
    }
    if !assistant_conclusions.is_empty() {
        out.push_str("\n#### Recent Technical Conclusions\n");
        let start = assistant_conclusions.len().saturating_sub(3);
        for conclusion in &assistant_conclusions[start..] {
            out.push_str(&format!("- {}\n", conclusion));
        }
    }
    if !files_touched.is_empty() {
        out.push_str("\n#### Files Touched or Referenced\n");
        for file in files_touched.iter().take(24) {
            out.push_str(&format!("- `{}`\n", file));
        }
    }
    if !tool_activity.is_empty() {
        out.push_str("\n#### Recent Tool Activity\n");
        let start = tool_activity.len().saturating_sub(12);
        for activity in &tool_activity[start..] {
            out.push_str(&format!("- {}\n", activity));
        }
    }
    if out.is_empty() {
        out = "Prior session context was consolidated into a local checkpoint.".into();
    }
    out.chars().take(23_000).collect()
}

fn compact_data(mut data: SessionData) -> Result<SessionData, String> {
    if data.messages.len() < 3 {
        return Err("Conversation is too short to compact (requires at least 3 messages).".into());
    }
    if data
        .messages
        .iter()
        .any(|message| message.is_streaming == Some(true))
    {
        return Err("Stop the active run before compacting this conversation.".into());
    }

    // Preserve the complete latest user turn, including its assistant response and tools.
    let keep_start = data
        .messages
        .iter()
        .rposition(|message| message.role == "user")
        .ok_or("Conversation has no user turn to preserve")?;
    let has_prior_user_turn = data.messages[..keep_start]
        .iter()
        .any(|message| message.role == "user");
    if !has_prior_user_turn {
        return Err("At least two user turns are required before compaction is useful.".into());
    }

    let messages_to_summarize = data.messages[..keep_start].to_vec();
    let recent_messages = data.messages[keep_start..].to_vec();
    let summary = deterministic_summary(&messages_to_summarize);

    let checkpoint_message = ChatMessage {
        selected_skills: Vec::new(),
        checkpoint_epoch: None,
        id: format!("compact_{}", Uuid::new_v4().simple()),
        role: "system".into(),
        content: format!(
            "## Compacted Context Checkpoint\n\n*Compacted {} previous messages using local deterministic history.*\n\n{}",
            messages_to_summarize.len(),
            summary
        ),
        timestamp: chrono::Utc::now().timestamp_millis(),
        reasoning: None,
        tool_calls: None,
        is_streaming: Some(false),
        attachments: None,
        image_inputs: None,
        council_turn: None,
        run_id: None,
        run_status: None,
        self_improvement: None,
        provider_messages: None,
        has_image_inputs: None,
        has_provider_messages: None,
        preamble: None,
        segments: None,
    };

    data.messages = std::iter::once(checkpoint_message)
        .chain(recent_messages)
        .collect();
    Ok(data)
}

pub async fn compact_session(session_id: &str) -> Result<SessionData, String> {
    let _guard = session_io_lock(session_id).lock_owned().await;
    let data = load_session_unlocked(session_id).await?;
    let compacted = compact_data(data)?;
    save_session_unlocked(compacted).await?;
    load_session_unlocked(session_id).await
}

pub async fn export_session(
    session_id: &str,
    destination: &str,
    format: &str,
) -> Result<(), String> {
    let _guard = session_io_lock(session_id).lock_owned().await;
    let data = load_session_unlocked(session_id).await?;
    if destination.trim().is_empty() || destination.contains('\0') {
        return Err("An export destination is required".into());
    }
    let bytes = match format {
        "json" => serde_json::to_vec_pretty(&data).map_err(|error| error.to_string())?,
        "markdown" | "md" => render_markdown(&data).into_bytes(),
        "html" => render_standalone_html(&data).into_bytes(),
        _ => return Err("Export format must be 'json', 'markdown', or 'html'".into()),
    };
    let destination = Path::new(destination);
    let resolved = resolve_export_path(destination)?;
    let source = session_path(session_id)?;
    for root in [config_dir(), source.parent().ok_or("Session has no directory")?.to_path_buf()] {
        if export_path_key(&resolved).starts_with(export_path_key(&resolve_export_path(&root)?)) {
            return Err("Export destination is inside internal state".into());
        }
    }
    // Write exactly the path checked above: Win32 can normalize junction/..
    // in the original spelling differently and redirect it into internal state.
    atomic_write(&resolved, &bytes).await
}

// Resolve each existing ancestor before processing `..`: lexical normalization
// first would hide traversal through a symlink/junction. Missing suffixes are
// retained so newly created directories receive the same boundary check.
fn resolve_export_path(path: &Path) -> Result<PathBuf, String> {
    use std::path::Component;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(|error| error.to_string())?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => resolved.push(component.as_os_str()),
            Component::CurDir => {},
            Component::ParentDir => { resolved.pop(); },
            Component::Normal(name) => {
                // Win32 accepts trailing-dot/space aliases even though canonical
                // paths use the verbatim namespace. Normalize before appending
                // to a canonical ancestor so that these aliases cannot hide it.
                #[cfg(windows)]
                let name = std::ffi::OsString::from(name.to_string_lossy().trim_end_matches(['.', ' ']));
                resolved.push(name);
                match std::fs::canonicalize(&resolved) {
                    Ok(canonical) => resolved = canonical,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                    Err(error) => return Err(format!("Cannot resolve export path {}: {error}", resolved.display())),
                }
            }
        }
    }
    Ok(resolved)
}

fn export_path_key(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        // Canonicalization returns verbatim disk/UNC paths. Fold case for
        // missing components too; Path::starts_with retains component bounds.
        let text = path.to_string_lossy().replace('/', "\\").to_lowercase();
        if let Some(unc) = text.strip_prefix(r"\\?\unc\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
    }
    #[cfg(not(windows))]
    path.to_path_buf()
}

fn render_markdown(data: &SessionData) -> String {
    let mut output = format!(
        "# {}\n\nExported: {}\n\n",
        data.session.title,
        chrono::Utc::now().to_rfc3339()
    );
    for message in &data.messages {
        output.push_str(&format!(
            "## {}\n\n{}\n\n",
            message.role,
            transcript_text(message)
        ));
        if let Some(reasoning) = &message.reasoning {
            output.push_str(&format!(
                "<details><summary>Reasoning</summary>\n\n{}\n\n</details>\n\n",
                reasoning
            ));
        }
    }
    if !data.artifacts.is_empty() {
        output.push_str("# Artifacts\n\n");
        for artifact in &data.artifacts {
            output.push_str(&format!(
                "## {}\n\n```{}\n{}\n```\n\n",
                artifact.title,
                artifact.language.clone().unwrap_or_default(),
                artifact.content
            ));
        }
    }
    output
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_standalone_html(data: &SessionData) -> String {
    let title = html_escape(&data.session.title);
    let date = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();

    let mut body = String::new();
    for msg in &data.messages {
        let role_class = if msg.role == "user" {
            "user"
        } else {
            "assistant"
        };
        let role_label = if msg.role == "user" {
            "User"
        } else {
            "Assistant"
        };
        let content_html = html_escape(&transcript_text(msg));

        let reasoning_html = if let Some(r) = &msg.reasoning {
            if !r.trim().is_empty() {
                format!(
                    "<details class=\"reasoning\"><summary>Reasoning trace</summary><pre>{}</pre></details>",
                    html_escape(r)
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        body.push_str(&format!(
            r#"<div class="message {role_class}">
  <span class="badge {role_class}">{role_label}</span>
  {reasoning_html}
  <div class="content">{content_html}</div>
</div>
"#
        ));
    }

    let mut artifacts_html = String::new();
    if !data.artifacts.is_empty() {
        artifacts_html.push_str(r#"<div class="artifacts-section"><h2>Session Artifacts</h2>"#);
        for art in &data.artifacts {
            let art_title = html_escape(&art.title);
            let art_lang = html_escape(&art.language.clone().unwrap_or_else(|| "text".to_string()));
            let art_content = html_escape(&art.content);
            artifacts_html.push_str(&format!(
                r#"<div class="artifact-card">
  <h3>{art_title} <span class="lang">({art_lang})</span></h3>
  <pre><code>{art_content}</code></pre>
</div>
"#
            ));
        }
        artifacts_html.push_str("</div>");
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - AetherAgent Export</title>
<style>
  body {{
    background-color: #0d1117;
    color: #e6edf3;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
    line-height: 1.6;
    margin: 0;
    padding: 32px 16px;
  }}
  .container {{ max-width: 840px; margin: 0 auto; }}
  h1 {{ font-size: 24px; color: #f0f6fc; margin-bottom: 4px; }}
  .meta {{ color: #8b949e; font-size: 12px; margin-bottom: 24px; border-bottom: 1px solid #30363d; padding-bottom: 12px; }}
  .message {{
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 12px;
    padding: 16px 20px;
    margin-bottom: 18px;
    white-space: pre-wrap;
    word-break: break-word;
  }}
  .message.user {{
    background: #161f2e;
    border-color: #388bfd50;
  }}
  .badge {{
    display: inline-block;
    padding: 2px 8px;
    border-radius: 6px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    margin-bottom: 10px;
    letter-spacing: 0.05em;
  }}
  .badge.user {{ background: #1f6feb30; color: #58a6ff; }}
  .badge.assistant {{ background: #23863630; color: #3fb950; }}
  pre {{
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 8px;
    padding: 12px;
    overflow-x: auto;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 1.45;
  }}
  code {{ font-family: inherit; }}
  details.reasoning {{
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 8px;
    padding: 8px 12px;
    margin-bottom: 12px;
    font-size: 12px;
    color: #8b949e;
  }}
  details.reasoning summary {{ cursor: pointer; color: #f0883e; font-weight: 600; outline: none; }}
  .artifacts-section {{ margin-top: 40px; border-top: 2px solid #30363d; padding-top: 20px; }}
  .artifact-card {{ margin-bottom: 24px; }}
  .artifact-card h3 {{ font-size: 15px; color: #58a6ff; margin-bottom: 8px; }}
  .artifact-card .lang {{ font-size: 11px; color: #8b949e; font-weight: normal; }}
  @media print {{
    body {{ background: #ffffff !important; color: #111111 !important; padding: 0 !important; }}
    .message {{ background: #ffffff !important; border: 1px solid #dddddd !important; page-break-inside: avoid; }}
    .message.user {{ border-color: #999999 !important; }}
    pre {{ background: #f6f8fa !important; border: 1px solid #e1e4e8 !important; color: #24292e !important; }}
    .badge.user {{ background: #eeeeee !important; color: #000000 !important; }}
    .badge.assistant {{ background: #eeeeee !important; color: #000000 !important; }}
  }}
</style>
</head>
<body>
<div class="container">
  <h1>{title}</h1>
  <div class="meta">Exported from AetherAgent on {date} &bull; Print to PDF ready (Ctrl+P)</div>
  {body}
  {artifacts_html}
</div>
</body>
</html>"#
    )
}

pub(crate) async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Target has no parent directory")?;
    if parent.as_os_str().is_empty() {
        return Err("Export destination must include a parent directory".into());
    }
    fs::create_dir_all(parent)
        .await
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let temp = parent.join(format!(".aether-{}.tmp", Uuid::new_v4()));
    let write_result = async {
        let mut file = fs::File::create(&temp).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        Ok::<(), std::io::Error>(())
    }
    .await;
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp).await;
        return Err(format!("Could not write temporary data: {error}"));
    }
    if let Err(error) = fs::rename(&temp, path).await {
        let _ = fs::remove_file(&temp).await;
        return Err(format!("Could not replace {}: {error}", path.display()));
    }
    Ok(())
}

pub fn load_workspace_sync() -> String {
    if let Ok(content) = std::fs::read_to_string(workspace_path()) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(cwd) = value.get("cwd").and_then(|value| value.as_str()) {
                if Path::new(cwd).is_dir() {
                    return cwd.to_string();
                }
            }
        }
    }
    String::new()
}

pub async fn clear_workspace() -> Result<(), String> {
    clear_workspace_file(&workspace_path()).await
}

async fn clear_workspace_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Could not clear workspace {}: {error}",
            path.display()
        )),
    }
}

// Shared by workspace persistence and IPC request binding; never reads config.
pub fn canonical_workspace(cwd: &str) -> Result<String, String> {
    if cwd.trim().is_empty() {
        return Ok(String::new());
    }
    let canonical = std::fs::canonicalize(cwd.trim())
        .map_err(|error| format!("Workspace is unavailable: {error}"))?;
    if !canonical.is_dir() {
        return Err("Workspace must be an existing directory".into());
    }
    let value = canonical.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    let value = value.strip_prefix(r"\\?\").unwrap_or(&value).to_string();
    Ok(value)
}

pub async fn save_workspace(cwd: &str) -> Result<(), String> {
    let clean_cwd = canonical_workspace(cwd)?;
    if clean_cwd.is_empty() {
        return clear_workspace().await;
    }
    let content = serde_json::to_vec_pretty(&serde_json::json!({"cwd": clean_cwd}))
        .map_err(|error| error.to_string())?;
    atomic_write(&workspace_path(), &content).await
}

pub fn validate_mcp_server(server: &McpServerConfig) -> Result<(), String> {
    validate_safe_id(&server.id)?;
    if server.enabled && server.command.trim().is_empty() {
        return Err("Enabled MCP servers require a command".into());
    }
    Ok(())
}

pub fn inspect_paths(paths: Vec<String>) -> Vec<AttachedContext> {
    paths
        .into_iter()
        .filter_map(|p| {
            let trimmed = p.trim().to_string();
            if trimmed.is_empty() {
                return None;
            }
            let std_path = Path::new(&trimmed);
            let name = std_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| trimmed.clone());
            let is_dir = std_path.is_dir();
            let id = format!(
                "att-{}-{}",
                chrono::Utc::now().timestamp_millis(),
                &Uuid::new_v4().simple().to_string()[..6]
            );
            Some(AttachedContext {
                id,
                path: trimmed,
                name,
                is_dir,
            })
        })
        .collect()
}

pub async fn save_attachment(
    workspace_cwd: Option<&str>,
    name: &str,
    bytes: &[u8],
) -> Result<AttachedContext, String> {
    if bytes.is_empty() {
        return Err("Attachment data cannot be empty".into());
    }
    const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024; // 25 MiB
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err("Attachment exceeds the 25 MiB limit".into());
    }

    let root = match workspace_cwd {
        Some(cwd) if !cwd.trim().is_empty() => {
            let canonical = canonical_workspace(cwd)?;
            PathBuf::from(canonical)
        }
        _ => {
            let root = config_dir();
            fs::create_dir_all(&root).await.map_err(|e| e.to_string())?;
            root
        }
    };
    let relative_dir = if workspace_cwd.is_some_and(|cwd| !cwd.trim().is_empty()) {
        ".aether-agent/attachments"
    } else {
        "attachments"
    };
    let root_text = root.to_string_lossy();
    let target_dir = super::tools::resolve_workspace_path(&root_text, relative_dir)?;
    fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| format!("Failed to create attachments directory: {e}"))?;
    let target_dir = super::tools::resolve_workspace_path(&root_text, relative_dir)?;

    let clean_name = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "attachment.png".into());

    let stem = Path::new(&clean_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "attachment".into());
    let ext = Path::new(&clean_name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    let suffix = &Uuid::new_v4().simple().to_string()[..8];
    let unique_filename = format!("{stem}_{suffix}{ext}");
    let relative_file = format!("{relative_dir}/{unique_filename}");
    let file_path = super::tools::resolve_workspace_path(&root_text, &relative_file)?;
    debug_assert!(file_path.starts_with(&target_dir));

    atomic_write(&file_path, bytes).await?;

    let id = format!(
        "att-{}-{}",
        chrono::Utc::now().timestamp_millis(),
        &Uuid::new_v4().simple().to_string()[..6]
    );

    Ok(AttachedContext {
        id,
        path: file_path.to_string_lossy().to_string(),
        name: clean_name,
        is_dir: false,
    })
}

// A failed disposable projection must never make a committed canonical save fail.
// Remember failed invalidations and refuse stale reads until invalidation succeeds.
static DIRTY_PROJECTIONS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<PathBuf>>,
> = std::sync::OnceLock::new();
fn dirty_projections() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    DIRTY_PROJECTIONS.get_or_init(Default::default)
}
fn repair_dirty_projections() -> Result<(), String> {
    let mut dirty = dirty_projections()
        .lock()
        .map_err(|_| "Projection invalidation lock poisoned")?;
    let paths: Vec<_> = dirty.iter().cloned().collect();
    for path in paths {
        super::session_pages::invalidate(&path)?;
        dirty.remove(&path);
    }
    Ok(())
}

async fn write_session_atomic(path: &Path, data: &SessionData) -> Result<(), String> {
    // All canonical writers, including native review updates, honor deletion.
    if fs::try_exists(path.with_extension("deleted"))
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("Session was deleted".into());
    }
    let bytes = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
    super::session_journal::prepare_commit(path, data, &bytes).await?;
    atomic_write(path, &bytes).await?;
    // cache_session always replaces the revision, even for equal size/mtime.
    // Keep this synchronous under the per-session I/O lock: dropping an IPC future must
    // not release the writer lock while a detached projection write is running.
    if let Err(error) = super::session_pages::cache_session(path, data) {
        eprintln!("Canonical session saved; invalidating failed page projection: {error}");
        dirty_projections()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_path_buf());
        let _ = repair_dirty_projections();
        // Keep the sealed target sidecar until a load repairs the projection.
        return Ok(());
    }
    super::session_journal::clear(path)
        .await
        .map_err(|e| CommittedSaveError::for_data(data, 0, e))?;
    Ok(())
}

fn page_at(
    path: &Path,
    id: &str,
    before: Option<usize>,
    limit: usize,
    revision: Option<&str>,
) -> Result<SessionPage, String> {
    repair_dirty_projections()?;
    if path.with_extension("deleted").exists() {
        let _ = super::session_pages::invalidate(path);
        return Err("Session was deleted".into());
    }
    super::session_pages::load_page(path, id, before, limit, revision)
}

// A live run owns its sidecar until a successful canonical commit or restart.
// Only crash leftovers/sealed interrupted commits take the full recovery path.
async fn recover_session_journal_if_needed(path: &Path, id: &str) -> Result<(), String> {
    if super::session_journal::needs_recovery(path).await? {
        read_session_file(path, id)
            .await?
            .ok_or("Checkpoint canonical session missing; checkpoint preserved")?;
    }
    Ok(())
}

pub async fn load_session_page(
    session_id: &str,
    before: Option<usize>,
    limit: usize,
    revision: Option<String>,
) -> Result<SessionPage, String> {
    let path = session_path(session_id)?;
    let _guard = session_io_lock(session_id).lock_owned().await;
    recover_session_journal_if_needed(&path, session_id).await?;
    let id = session_id.to_owned();
    let page_path = path.clone();
    let (mut page, _guard) = tokio::task::spawn_blocking(move || {
        let page = page_at(&page_path, &id, before, limit, revision.as_deref());
        page.map(|page| (page, _guard))
    })
    .await
    .map_err(|e| e.to_string())??;
    super::session_journal::overlay_active_page(&path, &mut page.data).await?;
    Ok(page)
}

/// Read-only detached viewport, never a partial-save suffix.
pub async fn load_session_around(
    session_id: &str,
    message_id: &str,
    limit: usize,
) -> Result<SessionPage, String> {
    let path = session_path(session_id)?;
    let _guard = session_io_lock(session_id).lock_owned().await;
    recover_session_journal_if_needed(&path, session_id).await?;
    let id = session_id.to_owned();
    let message_id = message_id.to_owned();
    let page_path = path.clone();
    let (mut page, _guard) = tokio::task::spawn_blocking(move || {
        repair_dirty_projections()?;
        if page_path.with_extension("deleted").exists() {
            return Err("Session was deleted".into());
        }
        let page = super::session_pages::load_around(&page_path, &id, &message_id, limit);
        page.map(|page| (page, _guard))
    })
    .await
    .map_err(|e| e.to_string())??;
    super::session_journal::overlay_active_page(&path, &mut page.data).await?;
    Ok(page)
}

/// Image hydration stays exact-ID and canonical; checkpoint assistants do not
/// replace historical user attachments.
pub async fn read_message_image_inputs(
    session_id: &str,
    message_id: &str,
) -> Result<Vec<super::types::ImageInput>, String> {
    let path = session_path(session_id)?;
    let _guard = session_io_lock(session_id).lock_owned().await;
    recover_session_journal_if_needed(&path, session_id).await?;
    let id = session_id.to_owned();
    let message_id = message_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let _guard = _guard;
        repair_dirty_projections()?;
        if path.with_extension("deleted").exists() {
            return Err("Session was deleted".into());
        }
        super::session_pages::read_message_image_inputs(&path, &id, &message_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn read_session_artifact(
    session_id: &str,
    artifact_id: &str,
) -> Result<Artifact, String> {
    let path = session_path(session_id)?;
    let _guard = session_io_lock(session_id).lock_owned().await;
    if artifact_id.is_empty() || artifact_id.len() > 1024 {
        return Err("Invalid artifact ID".into());
    }
    recover_session_journal_if_needed(&path, session_id).await?;
    if let Some(artifact) =
        super::session_journal::active_artifact(&path, session_id, artifact_id).await?
    {
        return Ok(artifact);
    }
    let id = session_id.to_owned();
    let artifact_id = artifact_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let _guard = _guard;
        repair_dirty_projections()?;
        if path.with_extension("deleted").exists() {
            return Err("Session was deleted".into());
        }
        super::session_pages::read_artifact(&path, &id, &artifact_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn validate_window(
    full: &SessionData,
    messages: &[ChatMessage],
    window: &SessionWindow,
    revision: &str,
) -> Result<(), String> {
    if window.end.is_some_and(|end| end != window.total) {
        return Err(
            "History preview is read-only; return to latest before saving; original preserved"
                .into(),
        );
    }
    if window.revision != revision {
        return Err("Session page cursor is stale; reload the latest page".into());
    }
    if window.offset > window.total
        || window.total > full.messages.len()
        || window.offset > full.messages.len()
    {
        return Err("Session page boundary exceeds canonical history; original preserved".into());
    }
    match window.first_message_id.as_deref() {
        Some(id)
            if full.messages.get(window.offset).map(|m| m.id.as_str()) == Some(id)
                && messages.first().map(|m| m.id.as_str()) == Some(id) => {}
        None if window.total == window.offset
            && full.messages.get(window.offset).is_some_and(|stored| {
                messages.first().is_some_and(|first| first.id == stored.id)
            }) => {}
        None if window.offset == full.messages.len() && window.total == window.offset => {}
        _ => return Err("Session page first-message boundary changed; original preserved".into()),
    }
    let mut ids = std::collections::HashSet::new();
    for message in full.messages[..window.offset].iter().chain(messages) {
        if message.id.is_empty() || !ids.insert(message.id.as_str()) {
            return Err(
                "Duplicate or empty message ID across session page boundary; original preserved"
                    .into(),
            );
        }
    }
    Ok(())
}

// Missing means omitted, regardless of transient hints. Never substitute an
// explicitly supplied payload: growth validation must see and reject regressions.
pub(super) fn retain_message_payload(message: &mut ChatMessage, stored: &ChatMessage) {
    if message.image_inputs.is_none() {
        message.image_inputs = stored.image_inputs.clone();
    }
    if message.provider_messages.is_none() {
        message.provider_messages = stored.provider_messages.clone();
    }
}
fn retain_history_payload(messages: &mut [ChatMessage], full: &SessionData) {
    // Full append saves use this too: avoid a quadratic scan of long history.
    let mut stored_by_id = std::collections::HashMap::with_capacity(full.messages.len());
    for stored in &full.messages {
        stored_by_id.entry(stored.id.as_str()).or_insert(stored);
    }
    for message in messages {
        if let Some(stored) = stored_by_id.get(message.id.as_str()) {
            retain_message_payload(message, stored);
        }
    }
}

fn merge_page_data(
    full: SessionData,
    mut data: SessionData,
    window: &SessionWindow,
    revision: &str,
) -> Result<SessionData, String> {
    if full.session.id != data.session.id {
        return Err("Session identity mismatch; original preserved".into());
    }
    validate_window(&full, &data.messages, window, revision)?;
    // Page writes are append/update snapshots, not destructive full hydration.
    // Requiring every persisted suffix ID in order also rejects accidentally
    // saving an older bounded page as though it were the complete tail.
    let suffix = &full.messages[window.offset..];
    if data.messages.len() < suffix.len()
        || suffix
            .iter()
            .zip(&data.messages)
            .any(|(stored, incoming)| stored.id != incoming.id)
    {
        return Err("Partial snapshot is missing or reorders canonical tail messages; fully load history before destructive edits; original preserved".into());
    }
    retain_history_payload(&mut data.messages, &full);
    let mut messages = full.messages[..window.offset].to_vec();
    messages.append(&mut data.messages);
    data.messages = messages;
    let mut artifacts = full.artifacts;
    let mut ids = std::collections::HashSet::new();
    for artifact in data.artifacts {
        if !ids.insert(artifact.id.clone()) {
            return Err("Duplicate artifact ID; original preserved".into());
        }
        if let Some(stored) = artifacts.iter_mut().find(|a| a.id == artifact.id) {
            *stored = artifact;
        } else {
            artifacts.push(artifact);
        }
    }
    data.artifacts = artifacts;
    Ok(data)
}

async fn save_session_page_file(
    path: &Path,
    mut data: SessionData,
    window: SessionWindow,
    checkpoint: bool,
) -> Result<SessionWindow, String> {
    let mut full = read_session_file_raw(path, &data.session.id)
        .await?
        .ok_or("Session not found; partial saves cannot create sessions")?;
    super::session_journal::overlay(path, &mut full, false).await?;
    retain_history_payload(&mut data.messages, &full);
    super::session_journal::validate_snapshot(path, &data).await?;
    let current = page_at(path, &data.session.id, Some(0), 1, Some(&window.revision))?;
    let merged = merge_page_data(full.clone(), data, &window, &current.window.revision)?;
    let first_message_id = merged.messages.get(window.offset).map(|m| m.id.clone());
    let stored = save_session_file_with_previous(path, merged, false, Some(&full))
        .await
        .map_err(|e| CommittedSaveError::at_offset(e, window.offset, first_message_id))?;
    if !checkpoint {
        let _ = super::search::index_session(&stored);
    }
    // Canonical bytes alone are not a cursor acknowledgment. Never invent a
    // revision: keep callers dirty until hash-bound recovery reads the real one.
    let revision = page_at(path, &stored.session.id, Some(0), 1, None)
        .map(|p| p.window.revision)
        .map_err(|e| CommittedSaveError::for_data(&stored, window.offset, e))?;
    Ok(SessionWindow {
        end: None,
        offset: window.offset,
        total: stored.messages.len(),
        revision,
        first_message_id: stored.messages.get(window.offset).map(|m| m.id.clone()),
    })
}

pub async fn save_session_page(
    data: SessionData,
    window: SessionWindow,
) -> Result<SessionWindow, String> {
    let _guard = session_io_lock(&data.session.id).lock_owned().await;
    let path = session_path(&data.session.id)?;
    save_session_page_file(&path, data, window, false).await
}

// Mirror useAgent.restoreSession only for comparison. Never persist this view:
// generated segments, cancelled tools and reasoning must not replace native data.
fn normalize_for_page(message: &ChatMessage) -> ChatMessage {
    let mut restored = message.clone();
    let incomplete = message.role == "assistant"
        && (message.is_streaming == Some(true)
            || (message.run_id.is_some()
                && message.run_status.as_deref().unwrap_or("").is_empty()));
    restored.is_streaming = Some(false);
    if incomplete {
        restored.run_status = Some("cancelled".into());
        let cancel = |tools: &mut Option<Vec<super::types::ToolCallItem>>| {
            for tool in tools.iter_mut().flatten() {
                if matches!(tool.status.as_str(), "running" | "pending") {
                    tool.status = "cancelled".into();
                }
            }
        };
        cancel(&mut restored.tool_calls);
        for segment in restored.segments.iter_mut().flatten() {
            cancel(&mut segment.tool_calls);
        }
        if let Some(council) = &mut restored.council_turn {
            council.candidate_a.is_streaming = Some(false);
            council.candidate_b.is_streaming = Some(false);
        }
    }
    let has_tools = restored
        .tool_calls
        .as_ref()
        .is_some_and(|calls| !calls.is_empty());
    if restored.preamble.as_deref().unwrap_or("").is_empty()
        && has_tools
        && !restored.content.is_empty()
    {
        static TRANSITION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let pattern = TRANSITION.get_or_init(|| regex::Regex::new(
            "(?i)(Dua subagent selesai|Subagent selesai|Hasil tes|Semua pengujian selesai|Deliverables|All tasks completed|Investigation complete|Test results)"
        ).expect("constant legacy preamble regex"));
        if let Some(found) = pattern.find(&restored.content) {
            let mut split = found.start();
            if restored.content[..split].encode_utf16().count() > 30 {
                if let Some(dot) = restored.content[..split].rfind('.') {
                    if restored.content[..dot].encode_utf16().count() > 30
                        && restored.content[dot..split].encode_utf16().count() <= 2
                    {
                        split = dot + 1;
                    }
                }
                let preamble = restored.content[..split].trim();
                let rest = restored.content[split..].trim();
                if preamble.encode_utf16().count() > 20 && rest.encode_utf16().count() > 20 {
                    restored.preamble = Some(preamble.to_owned());
                    restored.content = rest.to_owned();
                }
            }
        }
    }
    if restored.segments.is_none() && restored.role == "assistant" {
        let segment = |id: &str, text: String, tools| super::types::MessageSegment {
            id: id.into(),
            text: Some(text),
            tool_calls: Some(tools),
        };
        if has_tools {
            restored.segments = Some(vec![
                segment(
                    "seg_0",
                    restored.preamble.clone().unwrap_or_default(),
                    restored.tool_calls.clone().unwrap_or_default(),
                ),
                segment("seg_1", restored.content.clone(), vec![]),
            ]);
        } else if !restored.content.is_empty() {
            restored.segments = Some(vec![segment("seg_0", restored.content.clone(), vec![])]);
        }
    }
    restored
}

/// Reconcile only a known native metadata/review mutation, never a save retry.
/// Validate the entire loaded suffix before acknowledging the current revision.
/// The canonical JSON is read-only; page data may contain only some artifacts.
pub async fn refresh_session_window(
    session_id: &str,
    data: SessionData,
    window: SessionWindow,
    allow_metadata: bool,
    review_run_id: Option<&str>,
) -> Result<SessionWindow, String> {
    let path = session_path(session_id)?;
    let _guard = session_io_lock(session_id).lock_owned().await;
    let full = load_session_unlocked(session_id).await?;
    let conflict = || {
        "Session changed beyond the authorized native mutation; reload the latest page; original preserved".to_string()
    };
    if data.session.id != session_id
        || window.total != full.messages.len()
        || window.offset > window.total
        || data.messages.len() != window.total - window.offset
        || window.first_message_id.as_deref()
            != full.messages.get(window.offset).map(|m| m.id.as_str())
    {
        return Err(conflict());
    }
    // Reuse identity/duplicate validation, deliberately not the old revision.
    validate_window(&full, &data.messages, &window, &window.revision)?;
    let mut metadata = data.session.clone();
    metadata.updated_at = full.session.updated_at;
    metadata.message_count = full.session.message_count;
    metadata.preview = full.session.preview.clone();
    if allow_metadata {
        metadata.title = full.session.title.clone();
        metadata.pinned = full.session.pinned;
        metadata.pinned_at = full.session.pinned_at;
    }
    let value = |item| serde_json::to_value(item).map_err(|e| e.to_string());
    if value(&metadata)? != value(&full.session)?
        || serde_json::to_value(&data.model_override).map_err(|e| e.to_string())?
            != serde_json::to_value(&full.model_override).map_err(|e| e.to_string())?
    {
        return Err(conflict());
    }
    for (incoming, stored) in data.messages.iter().zip(&full.messages[window.offset..]) {
        let mut incoming = incoming.clone();
        let mut canonical = stored.clone();
        // Missing immutable payloads and derived hints are viewport differences,
        // not canonical edits. Present payloads must still compare exactly.
        if incoming.image_inputs.is_none() {
            incoming.image_inputs = stored.image_inputs.clone();
        }
        if incoming.provider_messages.is_none() {
            incoming.provider_messages = stored.provider_messages.clone();
        }
        incoming.has_image_inputs = None;
        incoming.has_provider_messages = None;
        canonical.has_image_inputs = None;
        canonical.has_provider_messages = None;
        if review_run_id.is_some_and(|run| {
            !run.is_empty()
                && incoming.run_id.as_deref() == Some(run)
                && stored.run_id.as_deref() == Some(run)
        }) {
            incoming.self_improvement = None;
            canonical.self_improvement = None;
        }
        let incoming = serde_json::to_value(&incoming).map_err(|e| e.to_string())?;
        if incoming != serde_json::to_value(&canonical).map_err(|e| e.to_string())?
            && incoming
                != serde_json::to_value(normalize_for_page(&canonical))
                    .map_err(|e| e.to_string())?
        {
            return Err(conflict());
        }
    }
    let artifacts: std::collections::HashMap<_, _> =
        full.artifacts.iter().map(|a| (a.id.as_str(), a)).collect();
    let mut seen = std::collections::HashSet::new();
    for artifact in &data.artifacts {
        let Some(stored) = artifacts.get(artifact.id.as_str()) else {
            return Err(conflict());
        };
        if !seen.insert(&artifact.id)
            || serde_json::to_value(artifact).map_err(|e| e.to_string())?
                != serde_json::to_value(stored).map_err(|e| e.to_string())?
        {
            return Err(conflict());
        }
    }
    // Read one boundary row for the projection's current revision, not the full
    // suffix again. An empty suffix has no boundary row (including empty chat).
    let before = if window.offset < full.messages.len() {
        window.offset + 1
    } else {
        window.offset
    };
    let current = page_at(&path, session_id, Some(before), 1, None)?;
    if current.window.total != full.messages.len() {
        return Err(conflict());
    }
    Ok(SessionWindow {
        end: None,
        offset: window.offset,
        total: full.messages.len(),
        revision: current.window.revision,
        first_message_id: full.messages.get(window.offset).map(|m| m.id.clone()),
    })
}
pub async fn checkpoint_session_page(
    data: SessionData,
    window: SessionWindow,
) -> Result<SessionWindow, String> {
    let _guard = session_io_lock(&data.session.id).lock_owned().await;
    let path = session_path(&data.session.id)?;
    save_session_page_file(&path, data, window, true).await
}

async fn hydrate_run_history_file(
    path: &Path,
    request: &mut AgentRunRequest,
) -> Result<(), String> {
    let Some(window) = request.history_window.clone() else {
        return Ok(());
    };
    validate_safe_id(&request.session_id)?;
    let current = page_at(
        path,
        &request.session_id,
        Some(0),
        1,
        Some(&window.revision),
    )?;
    let full = read_session_file(path, &request.session_id)
        .await?
        .ok_or("Session not found")?;
    validate_window(&full, &request.messages, &window, &current.window.revision)?;
    let prompt = request
        .messages
        .last()
        .filter(|message| message.role == "user")
        .ok_or("Run history must end with the current user message")?;
    // The frontend persists the user and a streaming assistant before invoking.
    // A caller without a placeholder is safe only when canonical history itself
    // ends at the current user; never discard a completed or foreign-run turn.
    let end = match full.messages.last() {
        Some(last)
            if last.role == "assistant"
                && last.run_id.as_deref() == Some(request.run_id.as_str())
                && last.is_streaming == Some(true) =>
        {
            full.messages.len() - 1
        }
        Some(last) if last.role == "user" => full.messages.len(),
        _ => {
            return Err(
                "Canonical history has no current user or same-run streaming placeholder".into(),
            )
        }
    };
    let current_user = full
        .messages
        .get(..end)
        .and_then(|messages| messages.last())
        .filter(|message| message.role == "user" && message.id == prompt.id)
        .ok_or("Run current user does not match canonical history")?;
    let suffix = full
        .messages
        .get(window.offset..end)
        .ok_or("Run history boundary exceeds the canonical current user")?;
    if suffix.len() != request.messages.len()
        || suffix
            .iter()
            .zip(&request.messages)
            .any(|(stored, incoming)| stored.id != incoming.id)
    {
        return Err("Run history is missing or reorders canonical tail messages".into());
    }
    // Only the current API prompt may differ (expanded attachments or /plan).
    // Clone all other fields, including provider transcripts and images, from
    // canonical storage rather than accepting client substitutions.
    let mut history = full.messages[..end].to_vec();
    let mut current_user = current_user.clone();
    current_user.content = prompt.content.clone();
    history[end - 1] = current_user;
    request.messages = history;
    request.history_window = None;
    Ok(())
}

pub async fn hydrate_run_history(request: &mut AgentRunRequest) -> Result<(), String> {
    if request.history_window.is_none() {
        return Ok(());
    }
    let _guard = session_io_lock(&request.session_id).lock_owned().await;
    let path = session_path(&request.session_id)?;
    hydrate_run_history_file(&path, request).await
}

pub async fn rename_session_summary(session_id: &str, title: &str) -> Result<Session, String> {
    if title.trim().is_empty() {
        return Err("Session title must not be empty".into());
    }
    let _guard = session_io_lock(session_id).lock_owned().await;
    let path = session_path(session_id)?;
    let mut data = load_session_unlocked(session_id).await?;
    data.session.title = bounded_title(title);
    let stored = save_session_file(&path, data, true).await?;
    let _ = super::search::index_session(&stored);
    Ok(stored.session)
}

pub async fn checkpoint_session_delta(
    session_id: &str,
    request: super::types::CheckpointDelta,
) -> Result<super::types::CheckpointAck, String> {
    let _guard = session_io_lock(session_id).lock_owned().await;
    let path = session_path(session_id)?;
    repair_dirty_projections()?;
    super::session_journal::checkpoint(&path, session_id, request).await
}

async fn recover_session_journals(root: &Path) -> Result<(), String> {
    let mut entries = match fs::read_dir(root).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("checkpoint") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Invalid checkpoint filename")?;
        validate_safe_id(id)?;
        let _guard = session_io_lock(id).lock_owned().await;
        if path.with_extension("deleted").exists() {
            super::session_journal::clear(&path.with_extension("json")).await?;
            continue;
        }
        recover_session_journal_if_needed(&path.with_extension("json"), id).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[derive(Debug, Default)]
    struct DirectorySnapshotCounts {
        live: usize,
        peak: usize,
        loaded: usize,
    }

    tokio::task_local! {
        static DIRECTORY_SNAPSHOT_PROBE: (
            PathBuf,
            std::sync::Arc<std::sync::Mutex<DirectorySnapshotCounts>>,
        );
    }

    #[derive(Debug)]
    pub(super) struct DirectorySnapshotGuard(
        std::sync::Arc<std::sync::Mutex<DirectorySnapshotCounts>>,
    );

    impl Drop for DirectorySnapshotGuard {
        fn drop(&mut self) {
            self.0.lock().unwrap().live -= 1;
        }
    }

    // Penjaga ikut pemilik snapshot, termasuk saat ditahan dalam Vec atau iterator.
    // Task-local membatasi pengukuran ke fixture tanpa mengubah profil pengguna.
    #[derive(Debug)]
    pub(super) struct TrackedDirectorySession {
        data: SessionData,
        snapshot: Option<DirectorySnapshotGuard>,
    }

    impl TrackedDirectorySession {
        pub(super) fn new(path: &Path, data: SessionData) -> Self {
            let snapshot = DIRECTORY_SNAPSHOT_PROBE
                .try_with(|(root, counts)| {
                    if path.parent() != Some(root.as_path()) {
                        return None;
                    }
                    let mut state = counts.lock().unwrap();
                    state.live += 1;
                    state.loaded += 1;
                    state.peak = state.peak.max(state.live);
                    Some(DirectorySnapshotGuard(counts.clone()))
                })
                .ok()
                .flatten();
            Self { data, snapshot }
        }

        pub(super) fn into_parts(self) -> (SessionData, Option<DirectorySnapshotGuard>) {
            (self.data, self.snapshot)
        }
    }

    impl std::ops::Deref for TrackedDirectorySession {
        type Target = SessionData;

        fn deref(&self) -> &Self::Target {
            &self.data
        }
    }

    fn artifact_memory_fixture(root: &Path, index: usize, timestamps: &[i64]) -> SessionData {
        let mut data = data_with_messages(vec![message("user", "image history")]);
        data.session.id = format!("artifact-memory-{index}");
        data.session.title = format!("Session {index} — hasil");
        data.session.updated_at = 1000 + index as i64;
        data.messages[0].image_inputs = Some(vec![super::super::types::ImageInput {
            source_path: "missing-fixture.png".into(),
            media_type: "image/png".into(),
            data: "QUJD".repeat(64 * 1024),
        }]);
        data.messages[0].provider_messages = Some(vec![serde_json::json!({
            "role":"assistant", "signature":"opaque", "data":"REVG".repeat(1024)
        })]);
        data.artifacts = timestamps
            .iter()
            .enumerate()
            .map(|(offset, timestamp)| {
                serde_json::from_value(serde_json::json!({
                    "id":format!("artifact-{index}-{offset}"),
                    "title":format!("Result {index}/{offset}"), "type":"code",
                    "content":format!("exact content {index}/{offset}\n<>"),
                    "language":"rust", "filePath":format!("src/result-{index}-{offset}.rs"),
                    "timestamp":timestamp, "streamRevision":7
                }))
                .unwrap()
            })
            .collect();
        std::fs::write(
            root.join(format!("{}.json", data.session.id)),
            serde_json::to_vec(&data).unwrap(),
        )
        .unwrap();
        data
    }

    #[tokio::test]
    async fn artifact_memory_listing_retains_at_most_one_full_snapshot() {
        let root = fixture_dir();
        for index in 0..6 {
            artifact_memory_fixture(&root, index, &[index as i64]);
        }
        let counts = std::sync::Arc::new(std::sync::Mutex::new(DirectorySnapshotCounts::default()));
        let items = DIRECTORY_SNAPSHOT_PROBE
            .scope(
                (root.clone(), counts.clone()),
                list_all_artifacts_in_dir(&root),
            )
            .await
            .unwrap();
        assert_eq!(items.len(), 6);
        let counts = counts.lock().unwrap();
        assert_eq!(counts.loaded, 6, "jalur listing harus membaca semua fixture");
        assert_eq!(counts.live, 0, "semua snapshot harus dilepas setelah listing");
        assert_eq!(
            counts.peak, 1,
            "listing artefak tidak boleh menahan beberapa riwayat sesi penuh: {counts:?}"
        );
    }

    #[tokio::test]
    async fn artifact_memory_listing_preserves_artifact_metadata_and_stable_order() {
        let root = fixture_dir();
        artifact_memory_fixture(&root, 0, &[10, 50, 50, i64::MIN]);
        artifact_memory_fixture(&root, 1, &[i64::MAX, 50, 20]);
        artifact_memory_fixture(&root, 2, &[]);
        let legacy = artifact_memory_fixture(&root, 3, &[50, 30]);
        let legacy_path = root.join(format!("{}.json", legacy.session.id));
        fs::write(
            &legacy_path,
            serde_json::to_vec(&serde_json::json!({
                "id":legacy.session.id, "title":legacy.session.title,
                "createdAt":legacy.session.created_at, "updatedAt":legacy.session.updated_at,
                "messages":legacy.messages, "artifacts":legacy.artifacts
            }))
            .unwrap(),
        )
        .await
        .unwrap();
        fs::write(root.join("ignored.tmp"), "{broken").await.unwrap();
        let before: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let bytes = std::fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        // Urutan referensi memakai enumerasi yang sama dengan implementasi lama.
        let mut expected = Vec::new();
        for tracked in read_session_directory(&root).await.unwrap() {
            let (data, _snapshot) = tracked.into_parts();
            for artifact in data.artifacts {
                expected.push(GlobalArtifactItem {
                    session_id: data.session.id.clone(),
                    session_title: data.session.title.clone(),
                    session_updated_at: data.session.updated_at,
                    artifact,
                });
            }
        }
        expected.sort_by_key(|item| std::cmp::Reverse(item.artifact.timestamp));
        let actual = list_all_artifacts_in_dir(&root).await.unwrap();
        assert_eq!(actual.len(), 9);
        assert_eq!(
            actual.iter().map(|item| item.artifact.timestamp).collect::<Vec<_>>(),
            vec![i64::MAX, 50, 50, 50, 50, 30, 20, 10, i64::MIN]
        );
        assert_eq!(actual[0].session_id, "artifact-memory-1");
        assert_eq!(actual[0].session_title, "Session 1 — hasil");
        assert_eq!(actual[0].session_updated_at, 1001);
        assert_eq!(actual[0].artifact.content, "exact content 1/0\n<>");
        assert_eq!(actual[0].artifact.language.as_deref(), Some("rust"));
        assert_eq!(actual[0].artifact.file_path.as_deref(), Some("src/result-1-0.rs"));
        assert_eq!(actual[0].artifact.stream_revision, Some(7));
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "metadata, isi, dan urutan timestamp sama harus tetap persis"
        );
        for (path, bytes) in before {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[tokio::test]
    async fn artifact_memory_listing_preserves_corruption_diagnostics_and_tombstone_wins() {
        let root = fixture_dir();
        artifact_memory_fixture(&root, 0, &[1, 2]);
        artifact_memory_fixture(&root, 1, &[]);
        let deleted = artifact_memory_fixture(&root, 2, &[999]);
        let deleted_path = root.join(format!("{}.json", deleted.session.id));
        fs::write(deleted_path.with_extension("deleted"), "deleted").await.unwrap();
        let corrupt_path = root.join("corrupt.json");
        let mismatch_path = root.join("mismatch.json");
        fs::write(&corrupt_path, "{broken").await.unwrap();
        fs::write(&mismatch_path, serde_json::to_vec(&deleted).unwrap()).await.unwrap();
        for leftover in [serde_json::to_vec(&deleted).unwrap(), b"{broken".to_vec()] {
            fs::write(&deleted_path, &leftover).await.unwrap();
            let expected = read_session_directory(&root).await.unwrap_err();
            let counts = std::sync::Arc::new(std::sync::Mutex::new(DirectorySnapshotCounts::default()));
            let actual = DIRECTORY_SNAPSHOT_PROBE
                .scope((root.clone(), counts.clone()), list_all_artifacts_in_dir(&root))
                .await
                .unwrap_err();
            assert_eq!(actual, expected, "pesan serta urutan diagnostik tidak berubah");
            assert!(actual.contains("2 valid sessions preserved"), "{actual}");
            assert!(actual.contains("corrupt.json: invalid JSON; original preserved"), "{actual}");
            assert!(actual.contains("mismatch.json: session ID does not match filename; original preserved"), "{actual}");
            assert!(!actual.contains("artifact-memory-2.json"), "{actual}");
            assert_eq!(counts.lock().unwrap().live, 0);
            assert_eq!(fs::read(&deleted_path).await.unwrap(), leftover);
            assert_eq!(fs::read(&corrupt_path).await.unwrap(), b"{broken");
            assert_eq!(fs::read(&mismatch_path).await.unwrap(), serde_json::to_vec(&deleted).unwrap());
        }
        let isolated = fixture_dir();
        assert!(list_all_artifacts_in_dir(&isolated.join("missing")).await.unwrap().is_empty());
        assert!(list_all_artifacts_in_dir(&isolated).await.unwrap().is_empty());
        let not_directory = isolated.join("not-directory");
        fs::write(&not_directory, "x").await.unwrap();
        assert_eq!(
            list_all_artifacts_in_dir(&not_directory).await.unwrap_err(),
            read_session_directory(&not_directory).await.unwrap_err()
        );
    }

    /// Real SQLite transaction failure, scoped to this fixture's canonical path.
    /// No test-only save/ack hook and no fabricated completion or revision.
    fn projection_fault(path: &Path) -> (rusqlite::Connection, String) {
        let conn =
            rusqlite::Connection::open(super::config_dir().join("session-pages.sqlite")).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let table = format!("fix3_fault_{}", Uuid::new_v4().simple());
        let key = path.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!(
            "CREATE TABLE {table}(enabled INTEGER); INSERT INTO {table} VALUES(1);
            CREATE TRIGGER {table}_trigger BEFORE INSERT ON page_sessions
            WHEN NEW.path='{key}' AND (SELECT enabled FROM {table})=1
            BEGIN SELECT RAISE(ABORT,'actual fix3 SQLite projection fault'); END;"
        ))
        .unwrap();
        (conn, table)
    }
    #[tokio::test]
    async fn committed_page_fault_recovers_actual_cursor_without_canonical_rewrite() {
        let path = fixture_dir().join("test-session.json");
        let mut data = data_with_messages(vec![
            message("user", "hidden prefix"),
            message("assistant", "tail"),
        ]);
        data.messages[0].provider_messages = Some(vec![serde_json::json!({"opaque":"hidden"})]);
        super::save_session_file(&path, data, false).await.unwrap();
        let mut page = super::page_at(&path, "test-session", None, 1, None).unwrap();
        let old_revision = page.window.revision.clone();
        page.data.messages[0].content = "committed tail".into();
        let (conn, fault) = projection_fault(&path);
        let error = super::save_session_page_file(&path, page.data, page.window, false)
            .await
            .unwrap_err();
        let receipt =
            super::CommittedSaveError::parse(&error).expect("typed canonical-committed error");
        assert!(receipt
            .cause
            .contains("actual fix3 SQLite projection fault"));
        assert_eq!(receipt.proof.offset, 1);
        let canonical = fs::read(&path).await.unwrap();
        assert_eq!(
            receipt.proof.canonical_sha256,
            super::canonical_sha256(&canonical)
        );
        assert!(
            super::recover_save_ack_file(&path, "test-session", receipt.proof.clone())
                .await
                .is_err()
        );
        assert_eq!(fs::read(&path).await.unwrap(), canonical);
        conn.execute(&format!("UPDATE {fault} SET enabled=0"), [])
            .unwrap();
        let ack = super::recover_save_ack_file(&path, "test-session", receipt.proof)
            .await
            .unwrap();
        assert_eq!(ack.window.offset, 1);
        assert_eq!(ack.window.total, 2);
        assert_ne!(ack.window.revision, old_revision);
        assert_eq!(ack.data.messages[0].content, "committed tail");
        let actual =
            super::page_at(&path, "test-session", None, 1, Some(&ack.window.revision)).unwrap();
        assert_eq!(actual.window.revision, ack.window.revision);
        let full: SessionData = serde_json::from_slice(&canonical).unwrap();
        assert_eq!(
            full.messages[0].provider_messages.as_ref().unwrap()[0]["opaque"],
            "hidden"
        );
        assert_eq!(
            fs::read(&path).await.unwrap(),
            canonical,
            "recovery must not rewrite canonical"
        );
        super::save_session_page_file(&path, ack.data, ack.window, false)
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn committed_replacement_fault_rejects_same_id_hidden_prefix_changes() {
        let path = fixture_dir().join("test-session.json");
        let data = data_with_messages(vec![
            message("user", "prefix"),
            message("assistant", "tail"),
            message("user", "remove"),
        ]);
        super::save_session_file(&path, data.clone(), false)
            .await
            .unwrap();
        let expected = super::page_at(&path, "test-session", None, 3, None)
            .unwrap()
            .window;
        let (conn, fault) = projection_fault(&path);
        let mut trimmed = data;
        trimmed.messages.truncate(2);
        let error = super::replace_session_checked_file(&path, trimmed, Some(expected))
            .await
            .unwrap_err();
        let proof = super::CommittedSaveError::parse(&error).unwrap().proof;
        assert_eq!(proof.offset, 0);
        let mut canonical: SessionData =
            serde_json::from_slice(&fs::read(&path).await.unwrap()).unwrap();
        assert_eq!(
            canonical.messages.len(),
            2,
            "replacement really committed before the error"
        );
        canonical.messages[0].content = "external same-ID prefix edit".into();
        let external = serde_json::to_vec_pretty(&canonical).unwrap();
        fs::write(&path, &external).await.unwrap();
        conn.execute(&format!("UPDATE {fault} SET enabled=0"), [])
            .unwrap();
        let error = super::recover_save_ack_file(&path, "test-session", proof)
            .await
            .unwrap_err();
        assert!(error.contains("Canonical changed after commit"), "{error}");
        assert_eq!(fs::read(&path).await.unwrap(), external);
    }
    #[tokio::test]
    async fn committed_empty_replacement_fault_recovers_zero_offset_and_real_revision() {
        let path = fixture_dir().join("test-session.json");
        let data = data_with_messages(vec![message("user", "clear me")]);
        super::save_session_file(&path, data.clone(), false)
            .await
            .unwrap();
        let expected = super::page_at(&path, "test-session", None, 1, None)
            .unwrap()
            .window;
        let (conn, fault) = projection_fault(&path);
        let mut empty = data;
        empty.messages.clear();
        let error = super::replace_session_checked_file(&path, empty, Some(expected))
            .await
            .unwrap_err();
        let receipt = super::CommittedSaveError::parse(&error).unwrap();
        let canonical = fs::read(&path).await.unwrap();
        conn.execute(&format!("UPDATE {fault} SET enabled=0"), [])
            .unwrap();
        let ack = super::recover_save_ack_file(&path, "test-session", receipt.proof)
            .await
            .unwrap();
        assert_eq!(ack.window.total, 0);
        assert_eq!(ack.window.offset, 0);
        assert_eq!(ack.window.first_message_id, None);
        super::page_at(&path, "test-session", None, 1, Some(&ack.window.revision)).unwrap();
        assert_eq!(fs::read(&path).await.unwrap(), canonical);
    }

    #[tokio::test]
    async fn committed_recovery_rejects_foreign_identity_boundary_and_tombstone() {
        let path = fixture_dir().join("test-session.json");
        let data = data_with_messages(vec![message("user", "immutable")]);
        super::save_session_file(&path, data.clone(), false)
            .await
            .unwrap();
        let receipt = super::CommittedSaveError::parse(&super::CommittedSaveError::for_data(
            &super::read_session_file_raw(&path, "test-session")
                .await
                .unwrap()
                .unwrap(),
            0,
            "fixture".into(),
        ))
        .unwrap();
        let mut foreign = receipt.proof.clone();
        foreign.session_id = "different".into();
        assert!(super::recover_save_ack_file(&path, "test-session", foreign)
            .await
            .unwrap_err()
            .contains("Invalid committed save proof"));
        let mut boundary = receipt.proof.clone();
        boundary.first_message_id = Some("wrong-boundary".into());
        assert!(
            super::recover_save_ack_file(&path, "test-session", boundary)
                .await
                .unwrap_err()
                .contains("boundary mismatch")
        );
        let before = fs::read(&path).await.unwrap();
        fs::write(path.with_extension("deleted"), b"deleted")
            .await
            .unwrap();
        assert!(
            super::recover_save_ack_file(&path, "test-session", receipt.proof)
                .await
                .unwrap_err()
                .contains("deleted")
        );
        assert_eq!(fs::read(&path).await.unwrap(), before);
    }
    #[test]
    fn committed_error_contract_does_not_parse_arbitrary_strings() {
        assert!(super::CommittedSaveError::parse(
            "History canonical commit succeeded; acknowledgment failed: fake"
        )
        .is_none());
        assert!(super::CommittedSaveError::parse("[canonical-committed]not json").is_none());
        assert!(
            super::CommittedSaveError::parse("[canonical-committed]{\"proof\":null}").is_none()
        );
    }

    #[tokio::test]
    async fn lightweight_page_and_full_append_save_preserve_canonical_replay_bytes() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let mut full = data_with_messages(vec![
            message("user", "image"),
            message("assistant", "signed"),
            message("user", "follow up"),
        ]);
        full.messages[0].image_inputs = Some(vec![super::super::types::ImageInput {
            source_path: "deleted.png".into(),
            media_type: "image/png".into(),
            data: "QUJD".repeat(524288),
        }]);
        full.messages[1].provider_messages =
            Some(vec![serde_json::json!({"signature":"opaque replay"})]);
        save_session_file(&path, full.clone(), false).await.unwrap();
        let page = page_at(&path, "test-session", None, 3, None).unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() < 10_000);
        let window = save_session_page_file(&path, page.data.clone(), page.window, false)
            .await
            .unwrap();
        let mut request: AgentRunRequest = serde_json::from_value(serde_json::json!({"sessionId":"test-session","runId":"run","provider":{},"messages":page.data.messages,"historyWindow":window})).unwrap();
        hydrate_run_history_file(&path, &mut request).await.unwrap();
        assert_eq!(
            request.messages[0].image_inputs.as_ref().unwrap()[0].data,
            full.messages[0].image_inputs.as_ref().unwrap()[0].data
        );
        assert_eq!(
            request.messages[1].provider_messages,
            full.messages[1].provider_messages
        );
        let mut append = page_at(&path, "test-session", None, 3, None).unwrap().data;
        append.messages.push(message("assistant", "new"));
        save_session_file(&path, append, false).await.unwrap();
        let stored = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.messages[0].image_inputs.as_ref().unwrap()[0].data,
            full.messages[0].image_inputs.as_ref().unwrap()[0].data
        );
        assert_eq!(
            stored.messages[1].provider_messages,
            full.messages[1].provider_messages
        );
        let disk = fs::read_to_string(&path).await.unwrap();
        assert!(!disk.contains("hasImageInputs"));
        assert!(!disk.contains("hasProviderMessages"));
    }

    #[tokio::test]
    async fn preview_guard_rejects_save_checkpoint_and_model_hydration_without_changing_canonical()
    {
        let (path, full, request) = native_history_fixture().await;
        let before = fs::read(&path).await.unwrap();
        let mut window = request.history_window.clone().unwrap();
        window.end = Some(window.total - 1);
        // Supply the complete valid suffix: only the detached-window marker should reject it.
        let mut data = full.clone();
        data.messages = full.messages[window.offset..].to_vec();
        for checkpoint in [false, true] {
            assert!(
                save_session_page_file(&path, data.clone(), window.clone(), checkpoint)
                    .await
                    .unwrap_err()
                    .contains("read-only")
            );
        }
        let mut incoming = request.clone();
        incoming.history_window = Some(window);
        assert!(hydrate_run_history_file(&path, &mut incoming)
            .await
            .unwrap_err()
            .contains("read-only"));
        assert_eq!(before, fs::read(&path).await.unwrap());
    }

    #[tokio::test]
    async fn record_review_duplicate_preserves_projection_revision() {
        let mut data = data_with_messages(vec![message("assistant", "Done")]);
        data.session.id = format!("record-review-{}", Uuid::new_v4());
        data.messages[0].run_id = Some("run".into());
        let root = fixture_dir();
        let path = root.join("session.json");
        let mut event = review_merge_event(serde_json::json!([
            {"action":"skill_staged","skill_name":"one","pending_id":"proposal"}
        ]));
        event.session_id = data.session.id.clone();
        write_session_atomic(&path, &data).await.unwrap();
        let initial = page_at(&path, &data.session.id, None, 1, None).unwrap();
        record_self_improvement_file(&path, &event).await.unwrap();
        let committed = page_at(&path, &data.session.id, None, 1, None).unwrap();
        assert_ne!(initial.window.revision, committed.window.revision);
        assert_eq!(
            committed.data.messages[0].self_improvement.as_ref(),
            Some(&event)
        );
        let bytes = fs::read(&path).await.unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        for _ in 0..3 {
            record_self_improvement_file(&path, &event).await.unwrap();
            let duplicate = page_at(&path, &data.session.id, None, 1, None).unwrap();
            assert_eq!(
                committed.window.revision, duplicate.window.revision,
                "an identical review must not write another projection revision"
            );
            assert_eq!(bytes, fs::read(&path).await.unwrap());
            assert_eq!(
                modified,
                std::fs::metadata(&path).unwrap().modified().unwrap()
            );
        }
        event.changes[0].action = super::super::types::SelfImprovementAction::SkillCreated;
        record_self_improvement_file(&path, &event).await.unwrap();
        let resolved = page_at(&path, &data.session.id, None, 1, None).unwrap();
        assert_ne!(committed.window.revision, resolved.window.revision);
        record_self_improvement_file(&path, &event).await.unwrap();
        assert_eq!(
            resolved.window.revision,
            page_at(&path, &data.session.id, None, 1, None)
                .unwrap()
                .window
                .revision
        );
        // A stale staged replay also merges to the same committed resolution.
        event.changes[0].action = super::super::types::SelfImprovementAction::SkillStaged;
        record_self_improvement_file(&path, &event).await.unwrap();
        assert_eq!(
            resolved.window.revision,
            page_at(&path, &data.session.id, None, 1, None)
                .unwrap()
                .window
                .revision
        );
        fs::remove_dir_all(root).await.unwrap();
    }
    // Exercise the public command against real canonical JSON and its projection.
    #[tokio::test]
    async fn refresh_window_authorizes_only_metadata_or_run_scoped_review_without_writing() {
        let mut full = data_with_messages(vec![
            message("user", "hidden prefix"),
            message("assistant", "answer"),
            message("assistant", "other answer"),
        ]);
        full.session.id = format!("refresh-{}", Uuid::new_v4());
        full.messages[1].run_id = Some("review-run".into());
        full.messages[1].run_status = Some("completed".into());
        full.messages[1].reasoning = Some("private reasoning".into());
        full.messages[1].image_inputs = Some(vec![serde_json::from_value(serde_json::json!({
            "sourcePath":"image.png", "mediaType":"image/png", "data":"QUJD"
        }))
        .unwrap()]);
        full.messages[1].provider_messages = Some(vec![serde_json::json!({"signature":"signed"})]);
        full.messages[2].run_id = Some("unrelated-run".into());
        full.messages[2].run_status = Some("completed".into());
        full.artifacts = vec![serde_json::from_value(serde_json::json!({
            "id":"artifact", "title":"Result", "type":"text", "content":"original", "timestamp":1
        }))
        .unwrap()];
        let path = session_path(&full.session.id).unwrap();
        write_session_atomic(&path, &full).await.unwrap();
        let initial = load_session_page(&full.session.id, None, 2, None)
            .await
            .unwrap();
        let mut baseline = initial.data;
        // Hand-built frontend restored shape, not the implementation under test.
        for message in &mut baseline.messages {
            message.is_streaming = Some(false);
            message.segments = Some(vec![super::super::types::MessageSegment {
                id: "seg_0".into(),
                text: Some(message.content.clone()),
                tool_calls: Some(vec![]),
            }]);
        }
        baseline.session.updated_at = -1;
        baseline.session.message_count = 0;
        baseline.session.preview = Some("stale preview".into());
        full.session.title = "Renamed".into();
        full.session.pinned = true;
        full.session.pinned_at = Some(42);
        write_session_atomic(&path, &full).await.unwrap();
        let bytes = fs::read(&path).await.unwrap();
        let fresh = refresh_session_window(
            &full.session.id,
            baseline.clone(),
            initial.window.clone(),
            true,
            None,
        )
        .await
        .unwrap();
        assert_ne!(fresh.revision, initial.window.revision);
        assert_eq!(fresh.offset, 1);
        assert_eq!(fresh.total, 3);
        assert_eq!(fresh.first_message_id, initial.window.first_message_id);
        assert_eq!(bytes, fs::read(&path).await.unwrap());
        assert!(refresh_session_window(
            &full.session.id,
            baseline.clone(),
            initial.window.clone(),
            false,
            None
        )
        .await
        .is_err());
        // The thin baseline above authorizes metadata refresh without payload IPC.
        // Explicitly hydrated payloads must still detect substitutions and native
        // same-ID payload edits; omitted payloads have no client-side byte copy.
        baseline.messages[0].image_inputs = full.messages[1].image_inputs.clone();
        baseline.messages[0].provider_messages = full.messages[1].provider_messages.clone();
        // Same IDs cannot conceal changes anywhere in the loaded suffix.
        let original = serde_json::to_value(&baseline).unwrap();
        for (pointer, replacement) in [
            ("/messages/0/content", serde_json::json!("edited")),
            ("/messages/1/content", serde_json::json!("edited later")),
            (
                "/messages/0/reasoning",
                serde_json::json!("changed reasoning"),
            ),
            ("/messages/0/imageInputs/0/data", serde_json::json!("REVG")),
            (
                "/messages/0/providerMessages",
                serde_json::json!([{"signature":"forged"}]),
            ),
            (
                "/messages/0/segments/0/text",
                serde_json::json!("changed segment"),
            ),
            (
                "/artifacts/0/content",
                serde_json::json!("changed artifact"),
            ),
            ("/modelOverride", serde_json::json!({"model":"other"})),
            (
                "/session/workspaceCwd",
                serde_json::json!("other workspace"),
            ),
            ("/session/createdAt", serde_json::json!(99)),
        ] {
            let mut value = original.clone();
            *value.pointer_mut(pointer).unwrap() = replacement;
            let changed = serde_json::from_value(value).unwrap();
            assert!(
                refresh_session_window(
                    &full.session.id,
                    changed,
                    initial.window.clone(),
                    true,
                    None
                )
                .await
                .is_err(),
                "accepted {pointer}"
            );
            assert_eq!(bytes, fs::read(&path).await.unwrap());
        }
        // Native same-ID edits, not just client edits, must also invalidate refresh.
        for case in 0..5 {
            let mut changed = full.clone();
            match case {
                0 => changed.messages[1].content.push_str(" externally changed"),
                1 => changed.messages[1].image_inputs.as_mut().unwrap()[0].data = "REVG".into(),
                2 => {
                    changed.messages[1].provider_messages =
                        Some(vec![serde_json::json!({"signature":"new"})])
                }
                3 => changed.artifacts[0].content = "external artifact".into(),
                4 => {
                    changed.model_override =
                        Some(serde_json::from_value(serde_json::json!({"model":"new"})).unwrap())
                }
                _ => unreachable!(),
            }
            write_session_atomic(&path, &changed).await.unwrap();
            let external_bytes = fs::read(&path).await.unwrap();
            assert!(
                refresh_session_window(
                    &full.session.id,
                    baseline.clone(),
                    initial.window.clone(),
                    true,
                    None
                )
                .await
                .is_err(),
                "accepted external change {case}"
            );
            assert_eq!(external_bytes, fs::read(&path).await.unwrap());
        }
        // Review exception is confined to the explicitly named message run.
        baseline.session = full.session.clone();
        full.messages[1].self_improvement = Some(
            serde_json::from_value(serde_json::json!({
                "run_id":"review-run", "session_id":full.session.id, "action":"no_action"
            }))
            .unwrap(),
        );
        write_session_atomic(&path, &full).await.unwrap();
        let bytes = fs::read(&path).await.unwrap();
        assert!(refresh_session_window(
            &full.session.id,
            baseline.clone(),
            initial.window.clone(),
            false,
            Some("review-run")
        )
        .await
        .is_ok());
        assert!(refresh_session_window(
            &full.session.id,
            baseline.clone(),
            initial.window.clone(),
            false,
            Some("unrelated-run")
        )
        .await
        .is_err());
        assert!(refresh_session_window(
            &full.session.id,
            baseline.clone(),
            initial.window.clone(),
            false,
            None
        )
        .await
        .is_err());
        let mut unrelated = baseline.clone();
        unrelated.messages[1].self_improvement = full.messages[1].self_improvement.clone();
        assert!(refresh_session_window(
            &full.session.id,
            unrelated,
            initial.window.clone(),
            false,
            Some("review-run")
        )
        .await
        .is_err());
        for case in 0..7 {
            let mut window = initial.window.clone();
            let mut data = baseline.clone();
            match case {
                0 => window.offset = 0,
                1 => window.total += 1,
                2 => window.first_message_id = Some("wrong".into()),
                3 => {
                    data.messages.pop();
                }
                4 => data.messages.push(message("user", "not yet persisted")),
                5 => data.session.id = "wrong-session".into(),
                6 => data.artifacts.push(data.artifacts[0].clone()),
                _ => unreachable!(),
            }
            assert!(
                refresh_session_window(&full.session.id, data, window, false, Some("review-run"))
                    .await
                    .is_err(),
                "accepted boundary case {case}"
            );
        }
        assert_eq!(bytes, fs::read(&path).await.unwrap());
    }
    #[tokio::test]
    async fn contention_session_locks_serialize_one_id_without_blocking_other_ids() {
        let a = session_io_lock("contention-a");
        let held = a.clone().lock_owned().await;
        let same = session_io_lock("contention-a");
        assert!(same.try_lock().is_err(), "same session must serialize");
        let b = session_io_lock("contention-b");
        assert!(b.try_lock().is_ok(), "unrelated session must not wait");
        drop(held);
        assert!(same.try_lock().is_ok());
        let again = session_io_lock("contention-a");
        assert!(std::sync::Arc::ptr_eq(&same, &again));
    }

    #[tokio::test]
    async fn refresh_window_restored_cancelled_legacy_messages_preserve_all_native_payloads() {
        let call = serde_json::json!({"id":"tool", "name":"read_file", "args":{"path":"file"}, "status":"running", "result":"partial"});
        let preamble = "A sufficiently long opening narrative about the investigation.";
        let answer = "Investigation complete with a sufficiently long final answer.";
        let original_content = format!("{preamble} {answer}");
        let mut full = data_with_messages(vec![serde_json::from_value(serde_json::json!({
            "id":"assistant", "role":"assistant", "timestamp":1,
            "content":original_content, "reasoning":"keep every reasoning token",
            "runId":"unfinished", "isStreaming":true, "toolCalls":[call],
            "providerMessages":[{"signature":"native signature"}],
            "councilTurn":{
                "candidateA":{"content":"candidate A", "reasoning":"reason A", "isStreaming":true},
                "candidateB":{"content":"candidate B", "reasoning":"reason B", "isStreaming":true}
            }
        }))
        .unwrap()]);
        full.session.id = format!("refresh-restore-{}", Uuid::new_v4());
        let path = session_path(&full.session.id).unwrap();
        write_session_atomic(&path, &full).await.unwrap();
        let page = load_session_page(&full.session.id, None, 10, None)
            .await
            .unwrap();
        let mut restored = page.data;
        let msg = &mut restored.messages[0];
        msg.content = answer.into();
        msg.preamble = Some(preamble.into());
        msg.is_streaming = Some(false);
        msg.run_status = Some("cancelled".into());
        msg.tool_calls.as_mut().unwrap()[0].status = "cancelled".into();
        msg.council_turn.as_mut().unwrap().candidate_a.is_streaming = Some(false);
        msg.council_turn.as_mut().unwrap().candidate_b.is_streaming = Some(false);
        msg.segments = Some(vec![
            super::super::types::MessageSegment {
                id: "seg_0".into(),
                text: Some(preamble.into()),
                tool_calls: msg.tool_calls.clone(),
            },
            super::super::types::MessageSegment {
                id: "seg_1".into(),
                text: Some(answer.into()),
                tool_calls: Some(vec![]),
            },
        ]);
        let bytes = fs::read(&path).await.unwrap();
        let fresh = refresh_session_window(
            &full.session.id,
            restored.clone(),
            page.window.clone(),
            false,
            None,
        )
        .await
        .unwrap();
        assert_eq!(fresh.revision, page.window.revision);
        // Never normalize both sides: that could hide edits to native tool state.
        for case in 0..5 {
            let mut changed = restored.clone();
            match case {
                0 => changed.messages[0].reasoning = None,
                1 => {
                    changed.messages[0].tool_calls.as_mut().unwrap()[0].args =
                        serde_json::json!({"path":"other"})
                }
                2 => {
                    changed.messages[0].segments.as_mut().unwrap()[0]
                        .tool_calls
                        .as_mut()
                        .unwrap()[0]
                        .result = Some("changed".into())
                }
                3 => {
                    changed.messages[0]
                        .council_turn
                        .as_mut()
                        .unwrap()
                        .candidate_a
                        .content = "edited".into()
                }
                4 => changed.messages[0]
                    .content
                    .push_str(" uncheckpointed progress"),
                _ => unreachable!(),
            }
            assert!(
                refresh_session_window(
                    &full.session.id,
                    changed,
                    page.window.clone(),
                    false,
                    Some("unfinished")
                )
                .await
                .is_err(),
                "accepted restored edit {case}"
            );
        }
        assert_eq!(bytes, fs::read(&path).await.unwrap());
        let reread = load_session(&full.session.id).await.unwrap();
        assert_eq!(reread.messages[0].content, original_content);
        assert!(reread.messages[0].segments.is_none());
        assert_eq!(
            reread.messages[0].reasoning.as_deref(),
            Some("keep every reasoning token")
        );
        assert_eq!(
            reread.messages[0].tool_calls.as_ref().unwrap()[0].status,
            "running"
        );
        // Empty sessions and an exhausted suffix have no first-message boundary.
        for count in [0, 1] {
            let mut empty_tail =
                data_with_messages((0..count).map(|_| message("user", "prefix")).collect());
            empty_tail.session.id = format!("refresh-empty-{}", Uuid::new_v4());
            let path = session_path(&empty_tail.session.id).unwrap();
            write_session_atomic(&path, &empty_tail).await.unwrap();
            let page = load_session_page(&empty_tail.session.id, None, 1, None)
                .await
                .unwrap();
            let mut window = page.window;
            window.offset = count;
            window.first_message_id = None;
            empty_tail.messages.clear();
            let bytes = fs::read(&path).await.unwrap();
            let fresh = refresh_session_window(
                &empty_tail.session.id.clone(),
                empty_tail,
                window,
                false,
                None,
            )
            .await
            .unwrap();
            assert_eq!(fresh.offset, count);
            assert_eq!(fresh.total, count);
            assert!(fresh.first_message_id.is_none());
            assert_eq!(bytes, fs::read(&path).await.unwrap());
        }
    }
    async fn native_history_fixture() -> (PathBuf, SessionData, AgentRunRequest) {
        let path = fixture_dir().join("test-session.json");
        let mut full = data_with_messages(vec![
            message("user", "prefix"),
            message("assistant", "canonical signed answer"),
            message("user", "historical question"),
            message("assistant", "historical answer"),
            message("user", "/plan current question"),
            message("assistant", ""),
        ]);
        full.messages[5].run_id = Some("run".into());
        full.messages[5].is_streaming = Some(true);
        for index in [0, 1, 4] {
            full.messages[index].image_inputs = Some(vec![super::super::types::ImageInput {
                source_path: format!("image-{index}.png"),
                media_type: "image/png".into(),
                data: "QUJD".into(),
            }]);
            full.messages[index].provider_messages = Some(vec![serde_json::json!({
                "role":"assistant", "content":[{"type":"thinking", "thinking":"native", "signature":"opaque"}]
            })]);
        }
        save_session_file(&path, full.clone(), false).await.unwrap();
        let window = page_at(&path, "test-session", None, 5, None)
            .unwrap()
            .window;
        let request = serde_json::from_value(serde_json::json!({
            "sessionId":"test-session", "runId":"run", "provider":{},
            "messages":full.messages[1..5], "historyWindow":window
        }))
        .unwrap();
        (path, full, request)
    }

    #[tokio::test]
    async fn native_history_rejects_missing_reordered_invented_or_non_user_tail() {
        let (path, full, request) = native_history_fixture().await;
        let before = fs::read(&path).await.unwrap();
        for case in 0..11 {
            let mut invalid = request.clone();
            match case {
                0 => {
                    invalid.messages.remove(1);
                }
                1 => {
                    invalid.messages.swap(1, 2);
                }
                2 => {
                    invalid.messages[1].id = "invented-history".into();
                }
                3 => {
                    invalid.messages[1] = full.messages[0].clone();
                }
                4 => {
                    invalid.messages.pop();
                }
                5 => {
                    invalid.messages.last_mut().unwrap().role = "assistant".into();
                }
                6 => {
                    invalid.messages.push(full.messages[5].clone());
                }
                7 => {
                    invalid.messages.clear();
                }
                8 => {
                    invalid.messages.last_mut().unwrap().id = "unsaved-user".into();
                }
                9 => {
                    invalid.messages[2] = invalid.messages[1].clone();
                }
                10 => {
                    invalid.messages.remove(0);
                }
                _ => unreachable!(),
            }
            assert!(
                hydrate_run_history_file(&path, &mut invalid).await.is_err(),
                "accepted invalid suffix case {case}"
            );
        }
        assert_eq!(before, fs::read(&path).await.unwrap());
    }

    #[tokio::test]
    async fn native_history_uses_canonical_messages_and_only_transforms_current_content() {
        let (path, full, mut request) = native_history_fixture().await;
        let before = fs::read(&path).await.unwrap();
        for incoming in &mut request.messages {
            incoming.role = "system".into();
            incoming.content = "client replacement".into();
            incoming.timestamp = -1;
            incoming.reasoning = Some("forged reasoning".into());
            incoming.image_inputs = Some(vec![]);
            incoming.provider_messages = Some(vec![
                serde_json::json!({"role":"system","content":"forged"}),
            ]);
        }
        let current = request.messages.last_mut().unwrap();
        current.role = "user".into();
        current.content = "Expanded planning prompt with attached context".into();
        let mut expected = full.messages[..5].to_vec();
        expected[4].content = current.content.clone();
        hydrate_run_history_file(&path, &mut request).await.unwrap();
        assert_eq!(
            serde_json::to_value(&request.messages).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert!(request.history_window.is_none());
        assert_eq!(before, fs::read(&path).await.unwrap());
    }

    #[tokio::test]
    async fn native_history_requires_same_run_streaming_placeholder_or_canonical_user_tail() {
        let (path, full, request) = native_history_fixture().await;
        for case in 0..3 {
            let mut invalid = full.clone();
            match case {
                0 => invalid.messages[5].run_id = Some("other-run".into()),
                1 => invalid.messages[5].is_streaming = Some(false),
                2 => invalid.messages[5].role = "tool".into(),
                _ => unreachable!(),
            }
            write_session_atomic(&path, &invalid).await.unwrap();
            let mut incoming = request.clone();
            incoming.history_window = Some(
                page_at(&path, "test-session", None, 5, None)
                    .unwrap()
                    .window,
            );
            assert!(
                hydrate_run_history_file(&path, &mut incoming)
                    .await
                    .is_err(),
                "accepted placeholder case {case}"
            );
        }
        let mut no_placeholder = full;
        no_placeholder.messages.pop();
        write_session_atomic(&path, &no_placeholder).await.unwrap();
        let mut incoming = request;
        let window = incoming.history_window.as_mut().unwrap();
        window.total = no_placeholder.messages.len();
        window.revision = page_at(&path, "test-session", None, 5, None)
            .unwrap()
            .window
            .revision;
        hydrate_run_history_file(&path, &mut incoming)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(incoming.messages).unwrap(),
            serde_json::to_value(no_placeholder.messages).unwrap()
        );
    }

    #[tokio::test]
    async fn native_tombstone_rejects_full_read_before_parsing_leftover_json() {
        let path = fixture_dir().join("test-session.json");
        let full = data_with_messages(vec![message("user", "deleted history")]);
        write_session_atomic(&path, &full).await.unwrap();
        fs::write(path.with_extension("deleted"), "deleted")
            .await
            .unwrap();
        for content in [serde_json::to_string(&full).unwrap(), "{broken".into()] {
            fs::write(&path, &content).await.unwrap();
            let error = read_session_file(&path, "test-session").await.unwrap_err();
            assert!(error.contains("deleted"), "{error}");
            assert_eq!(fs::read_to_string(&path).await.unwrap(), content);
        }
    }

    #[tokio::test]
    async fn native_tombstone_directory_skips_deleted_leftovers() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let full = data_with_messages(vec![message("user", "live history")]);
        write_session_atomic(&path, &full).await.unwrap();
        fs::write(root.join("deleted-session.json"), "{broken")
            .await
            .unwrap();
        fs::write(root.join("deleted-session.deleted"), "deleted")
            .await
            .unwrap();
        let listed = read_session_directory(&root).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session.id, full.session.id);
    }

    #[tokio::test]
    async fn native_tombstone_atomic_writer_cannot_recreate_or_replace_deleted_history() {
        let path = fixture_dir().join("test-session.json");
        let full = data_with_messages(vec![message("user", "deleted history")]);
        write_session_atomic(&path, &full).await.unwrap();
        let before = fs::read(&path).await.unwrap();
        fs::write(path.with_extension("deleted"), "deleted")
            .await
            .unwrap();
        let mut stale = full;
        stale.messages[0].content = "stale review write".into();
        assert!(write_session_atomic(&path, &stale)
            .await
            .unwrap_err()
            .contains("deleted"));
        assert_eq!(before, fs::read(&path).await.unwrap());
        let missing = path.with_file_name("missing.json");
        fs::write(missing.with_extension("deleted"), "deleted")
            .await
            .unwrap();
        assert!(write_session_atomic(&missing, &stale).await.is_err());
        assert!(!missing.exists());
    }

    #[test]
    fn contention_review_merge_1000_real_messages_is_linear_and_first_duplicate_wins() {
        let mut previous = data_with_messages(
            (0..1000)
                .map(|n| {
                    let mut m = message("assistant", "answer");
                    m.id = format!("m{n}");
                    m.run_id = Some(format!("r{n}"));
                    m.self_improvement = Some(review_merge_event(serde_json::json!([])));
                    m
                })
                .collect(),
        );
        let mut incoming = previous.clone();
        for m in &mut incoming.messages {
            m.self_improvement = None;
        }
        REVIEW_COMPARISONS.with(|count| count.set(0));
        merge_self_improvements(&mut incoming, &previous);
        let comparisons = REVIEW_COMPARISONS.with(|count| count.get());
        assert!(incoming
            .messages
            .iter()
            .all(|m| m.self_improvement.is_some()));
        assert!(
            comparisons <= 4000,
            "1000 real lookups used {comparisons} key comparisons"
        );
        let mut duplicate = previous.messages[0].clone();
        previous.messages[0].self_improvement = None;
        duplicate.self_improvement.as_mut().unwrap().summary = Some("later duplicate".into());
        previous.messages.push(duplicate);
        incoming.messages[0].self_improvement = None;
        merge_self_improvements(&mut incoming, &previous);
        assert!(
            incoming.messages[0].self_improvement.is_none(),
            "first duplicate, even without a review, must win"
        );
        incoming.messages[1].run_id = Some("different run".into());
        incoming.messages[1].self_improvement = None;
        merge_self_improvements(&mut incoming, &previous);
        assert!(incoming.messages[1].self_improvement.is_none());
    }
    #[tokio::test]
    async fn page_queued_empty_origin_snapshot_accepts_refreshed_revision_and_canonical_first_id() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        save_session_file(&path, data_with_messages(vec![]), false)
            .await
            .unwrap();
        let mut window = page_at(&path, "test-session", None, 10, None)
            .unwrap()
            .window;
        let mut data = data_with_messages(vec![
            message("user", "new"),
            message("assistant", "partial"),
        ]);
        let saved = save_session_page_file(&path, data.clone(), window.clone(), true)
            .await
            .unwrap();
        window.revision = saved.revision;
        data.messages[1].content = "later checkpoint".into();
        let result = save_session_page_file(&path, data, window, true)
            .await
            .unwrap();
        assert_eq!(result.total, 2);
        assert!(result.first_message_id.is_some());
    }
    #[tokio::test]
    async fn page_save_rejects_a_non_latest_page_instead_of_truncating_unloaded_suffix() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let full = data_with_messages(vec![
            message("user", "one"),
            message("assistant", "two"),
            message("user", "three"),
        ]);
        save_session_file(&path, full, false).await.unwrap();
        let older = page_at(&path, "test-session", Some(2), 1, None).unwrap();
        let bytes = fs::read(&path).await.unwrap();
        assert!(
            save_session_page_file(&path, older.data, older.window, true)
                .await
                .is_err()
        );
        assert_eq!(bytes, fs::read(&path).await.unwrap());
    }
    #[tokio::test]
    async fn page_checkpoint_preserves_hidden_payload_metadata_and_rejects_conflicts_before_write()
    {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let mut full = data_with_messages(vec![
            message("user", "prefix"),
            message("assistant", "tail"),
        ]);
        full.session.pinned = true;
        full.session.pinned_at = Some(7);
        full.messages[0].image_inputs = Some(vec![super::super::types::ImageInput {
            source_path: "old.png".into(),
            media_type: "image/png".into(),
            data: "QUJD".into(),
        }]);
        full.messages[1].provider_messages = Some(vec![
            serde_json::json!({"role":"assistant","content":[{"type":"thinking","signature":"opaque","thinking":"reason"}]}),
        ]);
        full.artifacts = (0..120).map(|i| serde_json::from_value(serde_json::json!({"id":format!("a{i}"),"title":"artifact","type":"code","content":"hidden","timestamp":i})).unwrap()).collect();
        save_session_file(&path, full.clone(), false).await.unwrap();
        let page = page_at(&path, "test-session", None, 1, None).unwrap();
        assert!(page.data.artifacts.len() < full.artifacts.len());
        let mut tail = page.data.clone();
        tail.session.title = "stale".into();
        tail.session.pinned = false;
        tail.messages[0].provider_messages = None;
        tail.messages[0].content = "checkpoint".into();
        let saved = save_session_page_file(&path, tail.clone(), page.window.clone(), true)
            .await
            .unwrap();
        assert_ne!(saved.revision, page.window.revision);
        let stored = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.messages[0].id, full.messages[0].id);
        assert_eq!(
            stored.messages[0].image_inputs.as_ref().unwrap()[0].data,
            "QUJD"
        );
        assert_eq!(
            stored.messages[1].provider_messages,
            full.messages[1].provider_messages
        );
        assert_eq!(stored.artifacts.len(), 120);
        assert_eq!(stored.session.title, "Test");
        assert!(stored.session.pinned);
        let bytes = fs::read(&path).await.unwrap();
        assert!(
            save_session_page_file(&path, tail.clone(), page.window, true)
                .await
                .is_err()
        );
        let mut invalid = saved.clone();
        invalid.offset = 0;
        assert!(save_session_page_file(&path, tail.clone(), invalid, true)
            .await
            .is_err());
        tail.messages.push(full.messages[0].clone());
        assert!(save_session_page_file(&path, tail, saved, true)
            .await
            .is_err());
        assert_eq!(bytes, fs::read(&path).await.unwrap());
    }

    #[tokio::test]
    async fn page_hydration_replays_prefix_images_and_native_protocol_without_saved_placeholder() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let mut full = data_with_messages(vec![
            message("user", "image"),
            message("assistant", "signed turn"),
            message("user", "new"),
            message("assistant", "placeholder"),
        ]);
        full.messages[3].run_id = Some("run".into());
        full.messages[3].is_streaming = Some(true);
        full.messages[0].image_inputs = Some(vec![super::super::types::ImageInput {
            source_path: "old.png".into(),
            media_type: "image/png".into(),
            data: "QUJD".into(),
        }]);
        full.messages[1].provider_messages = Some(vec![
            serde_json::json!({"role":"assistant","content":[{"type":"thinking","signature":"opaque","thinking":"thought"}]}),
        ]);
        save_session_file(&path, full.clone(), false).await.unwrap();
        let cursor = page_at(&path, "test-session", None, 2, None)
            .unwrap()
            .window;
        let mut request: AgentRunRequest = serde_json::from_value(serde_json::json!({"sessionId":"test-session","runId":"run","provider":{},"messages":[full.messages[2].clone()],"historyWindow":cursor})).unwrap();
        hydrate_run_history_file(&path, &mut request).await.unwrap();
        assert_eq!(request.messages.len(), 3);
        assert_eq!(
            request.messages[0].image_inputs.as_ref().unwrap()[0].data,
            "QUJD"
        );
        assert_eq!(
            request.messages[1].provider_messages,
            full.messages[1].provider_messages
        );
        assert!(request.history_window.is_none());
        let mut wrong = request.clone();
        wrong.session_id = "foreign".into();
        wrong.history_window = Some(cursor.clone());
        assert!(hydrate_run_history_file(&path, &mut wrong).await.is_err());
        let mut wrong = request.clone();
        wrong.messages = vec![full.messages[3].clone()];
        wrong.history_window = Some(cursor);
        assert!(hydrate_run_history_file(&path, &mut wrong).await.is_err());
        let mut no_window = request.clone();
        let before = serde_json::to_value(&no_window).unwrap();
        hydrate_run_history_file(&root.join("missing.json"), &mut no_window)
            .await
            .unwrap();
        assert_eq!(before, serde_json::to_value(no_window).unwrap());
    }

    #[tokio::test]
    async fn page_failed_projection_is_disposable_after_successful_canonical_save() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let full = data_with_messages(vec![message("user", "old")]);
        save_session_file(&path, full.clone(), false).await.unwrap();
        let first = page_at(&path, "test-session", None, 5, None).unwrap();
        let conn = rusqlite::Connection::open(config_dir().join("session-pages.sqlite")).unwrap();
        // Target just this fixture; other parallel storage tests keep working.
        let key = path.to_string_lossy().replace('\'', "''");
        let trigger = format!("reject_{}", Uuid::new_v4().simple());
        conn.execute_batch(&format!("CREATE TRIGGER {trigger} BEFORE INSERT ON page_messages WHEN NEW.path='{key}' BEGIN SELECT RAISE(ABORT,'injected'); END;")).unwrap();
        let mut changed = full;
        changed.messages[0].content = "new".into();
        save_session_file(&path, changed, false).await.unwrap();
        assert_eq!(
            read_session_file(&path, "test-session")
                .await
                .unwrap()
                .unwrap()
                .messages[0]
                .content,
            "new"
        );
        let stale: i64 = conn
            .query_row(
                "SELECT count(*) FROM page_sessions WHERE path=?1",
                [path.to_string_lossy().as_ref()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale, 0);
        conn.execute_batch(&format!("DROP TRIGGER {trigger}"))
            .unwrap();
        let fresh = page_at(&path, "test-session", None, 5, None).unwrap();
        assert_ne!(first.window.revision, fresh.window.revision);
        assert_eq!(fresh.data.messages[0].content, "new");
        fs::write(path.with_extension("deleted"), "deleted")
            .await
            .unwrap();
        assert!(page_at(&path, "test-session", None, 5, None).is_err());
    }

    #[tokio::test]
    async fn page_warm_catalog_does_not_decode_history_and_reports_corrupt_files() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        save_session_file(
            &path,
            data_with_messages(vec![message("user", "history")]),
            false,
        )
        .await
        .unwrap();
        let conn = rusqlite::Connection::open(config_dir().join("session-pages.sqlite")).unwrap();
        conn.execute(
            "UPDATE page_messages SET json='{broken' WHERE path=?1",
            [path.to_string_lossy().as_ref()],
        )
        .unwrap();
        let catalog_root = root.clone();
        let catalog = tokio::task::spawn_blocking(move || {
            super::super::session_pages::list_catalog(&catalog_root)
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(catalog[0].message_count, 1);
        fs::write(root.join("corrupt.json"), "{broken")
            .await
            .unwrap();
        let catalog_root = root.clone();
        let error = tokio::task::spawn_blocking(move || {
            super::super::session_pages::list_catalog(&catalog_root)
        })
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.contains("corrupt.json") && error.contains("1 valid sessions preserved"));
        assert_eq!(
            fs::read_to_string(root.join("corrupt.json")).await.unwrap(),
            "{broken"
        );
    }
    #[tokio::test]
    async fn page_empty_origin_hydrates_saved_user_once_without_placeholder() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let full = data_with_messages(vec![]);
        save_session_file(&path, full.clone(), false).await.unwrap();
        let initial = page_at(&path, "test-session", None, 50, None).unwrap();
        let mut updated = full;
        updated.messages = vec![message("user", "new"), message("assistant", "placeholder")];
        updated.messages[1].run_id = Some("run".into());
        updated.messages[1].is_streaming = Some(true);
        let saved = save_session_page_file(&path, updated.clone(), initial.window.clone(), true)
            .await
            .unwrap();
        let mut request: AgentRunRequest = serde_json::from_value(serde_json::json!({
            "runId":"run", "sessionId":"test-session", "provider":{},
            "messages":[updated.messages[0].clone()],
            "historyWindow":{ "offset":0,"total":0,"firstMessageId":null,"revision":saved.revision }
        }))
        .unwrap();
        hydrate_run_history_file(&path, &mut request).await.unwrap();
        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].content, "new");
        assert!(request.history_window.is_none());
    }
    #[test]
    fn page_merge_preserves_prefix_and_hidden_artifacts() {
        let mut full =
            data_with_messages(vec![message("user", "old"), message("assistant", "tail")]);
        full.artifacts.push(serde_json::from_value(serde_json::json!({"id":"hidden","title":"hidden","type":"code","content":"keep","timestamp":1})).unwrap());
        let window = super::super::types::SessionWindow {
            end: None,
            offset: 1,
            total: 2,
            revision: "revision".into(),
            first_message_id: Some(full.messages[1].id.clone()),
        };
        let mut tail = full.clone();
        tail.messages.remove(0);
        tail.messages[0].content = "updated".into();
        tail.artifacts.clear();
        let merged = merge_page_data(full.clone(), tail, &window, "revision").unwrap();
        assert_eq!(merged.messages[0].id, full.messages[0].id);
        assert_eq!(merged.messages[1].content, "updated");
        assert_eq!(merged.artifacts.len(), 1);
        assert!(merge_page_data(full.clone(), merged.clone(), &window, "stale").is_err());
        let mut invalid = window.clone();
        invalid.offset = 3;
        assert!(merge_page_data(full, merged, &invalid, "revision").is_err());
    }
    use super::*;

    fn image_history() -> SessionData {
        let mut data = data_with_messages(vec![
            message("user", "describe image"),
            message("assistant", "partial"),
        ]);
        data.messages[0].image_inputs = Some(vec![super::super::types::ImageInput {
            source_path: "history.png".into(),
            media_type: "image/png".into(),
            data: "QUJD".repeat(512 * 1024),
        }]);
        data.messages[1].reasoning = Some("thinking".into());
        data.messages[1].is_streaming = Some(true);
        data.artifacts.push(
            serde_json::from_value(serde_json::json!({
                "id":"artifact", "title":"output", "type":"code",
                "content":"partial", "timestamp":1
            }))
            .unwrap(),
        );
        data
    }

    #[test]
    fn substantive_guard_streaming_changes_skip_exact_comparison() {
        let previous = image_history();
        let mut calls = 0;
        for case in 0..6 {
            let mut incoming = previous.clone();
            match case {
                0 => incoming.messages[1].content = "changed".into(),
                1 => incoming.messages[1].reasoning = Some("changed!".into()),
                2 => incoming.artifacts[0].content = "changed".into(),
                3 => incoming.messages.push(message("assistant", "new")),
                4 => incoming.artifacts.clear(),
                5 => incoming.messages[1].reasoning = None,
                _ => unreachable!(),
            }
            assert!(data_changed_with(&incoming, &previous, || {
                calls += 1;
                Ok(true)
            })
            .unwrap());
        }
        assert_eq!(
            calls, 0,
            "perubahan streaming tidak boleh menyalin JSON gambar"
        );
    }
    #[test]
    fn substantive_guard_falls_back_once_for_identical_and_non_guard_fields() {
        let previous = image_history();
        let original = serde_json::to_value(&previous).unwrap();
        let cases = [
            ("/messages/0/imageInputs/0/data", serde_json::json!("REVG")),
            (
                "/messages/0/imageInputs/0/sourcePath",
                serde_json::json!("other.png"),
            ),
            (
                "/messages/0/imageInputs/0/mediaType",
                serde_json::json!("image/jpeg"),
            ),
            (
                "/messages/1/providerMessages",
                serde_json::json!([
                    {"role":"assistant","content":[{"type":"image","data":"REVG"}]}
                ]),
            ),
            (
                "/messages/1/segments",
                serde_json::json!([{"id":"s1","text":"segment"}]),
            ),
            (
                "/messages/1/councilTurn",
                serde_json::json!({
                    "candidateA":{"content":"candidate A"}, "candidateB":{}, "chosenWinner":"a"
                }),
            ),
            (
                "/messages/1/toolCalls",
                serde_json::json!([
                    {"id":"tool","name":"read_file","args":{"path":"test"},"status":"done"}
                ]),
            ),
            (
                "/messages/1/attachments",
                serde_json::json!([
                    {"id":"attachment","path":"test.txt","name":"test.txt","isDir":false}
                ]),
            ),
            ("/messages/1/preamble", serde_json::json!("preamble")),
            ("/messages/1/isStreaming", serde_json::json!(false)),
            ("/messages/1/runId", serde_json::json!("run")),
            ("/messages/1/runStatus", serde_json::json!("completed")),
            (
                "/messages/1/selfImprovement",
                serde_json::json!({
                    "run_id":"run", "session_id":"test-session", "action":"no_action"
                }),
            ),
            ("/messages/1/id", serde_json::json!("other-id")),
            ("/messages/1/role", serde_json::json!("system")),
            ("/messages/1/timestamp", serde_json::json!(2)),
            ("/artifacts/0/title", serde_json::json!("renamed")),
            ("/artifacts/0/type", serde_json::json!("text")),
            ("/artifacts/0/language", serde_json::json!("rust")),
            ("/artifacts/0/filePath", serde_json::json!("test.rs")),
            ("/artifacts/0/id", serde_json::json!("other-artifact")),
            ("/artifacts/0/timestamp", serde_json::json!(2)),
            (
                "/modelOverride",
                serde_json::json!({
                    "providerId":"provider", "model":"model", "thinking":true, "reasoningEffort":"high"
                }),
            ),
        ];
        for (pointer, value) in std::iter::once(("", original.clone())).chain(cases) {
            let mut changed = original.clone();
            if pointer.is_empty() {
                changed = value;
            } else {
                // Field opsional yang None dapat dihilangkan oleh serde.
                let (parent, key) = pointer.rsplit_once('/').unwrap();
                changed.pointer_mut(parent).unwrap()[key] = value;
            }
            let incoming: SessionData = serde_json::from_value(changed).unwrap();
            let mut calls = 0;
            let result = data_changed_with(&incoming, &previous, || {
                calls += 1;
                substantive_changed(&incoming, &previous)
            })
            .unwrap();
            assert_eq!(calls, 1, "fallback: {pointer}");
            assert_eq!(result, !pointer.is_empty(), "hasil: {pointer}");
        }
    }

    #[test]
    fn substantive_guard_preserves_exact_comparator_error() {
        let data = image_history();
        let mut calls = 0;
        let result = data_changed_with(&data, &data, || {
            calls += 1;
            Err("serialization failed".into())
        });
        assert_eq!(calls, 1);
        assert_eq!(result.unwrap_err(), "serialization failed");
    }

    #[tokio::test]
    async fn substantive_guard_checkpoint_preserves_metadata_timestamp_and_image_bytes() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let mut previous = image_history();
        previous.session.title = "Native title".into();
        previous.session.created_at = 123;
        previous.session.updated_at = 456;
        previous.session.pinned = true;
        previous.session.pinned_at = Some(234);
        previous.messages[1].provider_messages = Some(vec![serde_json::json!({
            "role":"assistant", "content":[{"type":"image","data":"R0hJ"}]
        })]);
        atomic_write(&path, &serde_json::to_vec_pretty(&previous).unwrap())
            .await
            .unwrap();
        let mut stale = previous.clone();
        stale.session.title = "Stale title".into();
        stale.session.created_at = 999;
        stale.session.updated_at = 999;
        stale.session.pinned = false;
        stale.session.pinned_at = None;
        let identical = save_session_file(&path, stale, false).await.unwrap();
        assert_eq!(identical.session.updated_at, 456);
        assert_eq!(identical.session.created_at, 123);
        assert_eq!(identical.session.title, "Native title");
        assert!(identical.session.pinned);
        assert_eq!(identical.session.pinned_at, Some(234));

        let expected_images = serde_json::to_value(&previous.messages[0].image_inputs).unwrap();
        let mut streaming = identical;
        streaming.messages[1].content.push_str(" checkpoint");
        save_session_file(&path, streaming, false).await.unwrap();
        let mut recovered = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        assert!(recovered.session.updated_at > 456);
        assert_eq!(recovered.messages[1].content, "partial checkpoint");
        assert_eq!(
            serde_json::to_value(&recovered.messages[0].image_inputs).unwrap(),
            expected_images
        );
        assert_eq!(
            recovered.messages[1].provider_messages,
            previous.messages[1].provider_messages
        );
        assert_eq!(recovered.session.title, "Native title");
        assert_eq!(recovered.session.created_at, 123);
        assert!(recovered.session.pinned);
        assert_eq!(recovered.session.pinned_at, Some(234));

        // Timestamp lama membuat perubahan gambar saja teruji tanpa sleep.
        recovered.session.updated_at = 456;
        atomic_write(&path, &serde_json::to_vec_pretty(&recovered).unwrap())
            .await
            .unwrap();
        recovered.messages[0].image_inputs.as_mut().unwrap()[0].data = "REVG".into();
        save_session_file(&path, recovered, false).await.unwrap();
        let recovered = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        assert!(recovered.session.updated_at > 456);
        assert_eq!(
            recovered.messages[0].image_inputs.as_ref().unwrap()[0]
                .data
                .as_bytes(),
            b"REVG"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).await.unwrap();
    }

    fn fixture_dir() -> PathBuf {
        let root = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("aether-storage-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn export_fixture() -> (PathBuf, SessionData, PathBuf) {
        let root = fixture_dir();
        let sessions = root.join("custom-sessions");
        std::fs::create_dir(&sessions).unwrap();
        let mut data = data_with_messages(vec![message("user", "Keep canonical history")]);
        data.session.id = format!("export-{}", Uuid::new_v4());
        register_session_test_directory(&data.session.id, sessions.clone());
        let canonical = sessions.join(format!("{}.json", data.session.id));
        std::fs::write(&canonical, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
        (root, data, canonical)
    }

    #[tokio::test]
    async fn export_all_formats_create_and_replace_legitimate_outside_files() {
        let (root, mut data, canonical) = export_fixture();
        data.session.title = "Export <safe> & résumé".into();
        data.messages[0].content.push_str("\nRésumé <script> & export");
        let before = serde_json::to_vec_pretty(&data).unwrap();
        fs::write(&canonical, &before).await.unwrap();
        // A shared string prefix is not an internal path component.
        let outside = root.join("custom-sessions-exports");
        for (format, extension) in [
            ("json", "json"),
            ("markdown", "md"),
            ("md", "md"),
            ("html", "html"),
        ] {
            let destination = outside.join(format).join("nested").join(format!("session.{extension}"));
            assert!(!destination.parent().unwrap().exists());
            for replace in [false, true] {
                if replace {
                    fs::write(&destination, b"old outside export").await.unwrap();
                }
                export_session(&data.session.id, &destination.to_string_lossy(), format)
                    .await
                    .unwrap();
                let bytes = fs::read(&destination).await.unwrap();
                if format == "json" {
                    assert_eq!(bytes, before, "JSON export must preserve the full session");
                } else {
                    let text = String::from_utf8(bytes).unwrap();
                    assert!(text.contains("Keep canonical history"));
                    if format == "html" {
                        assert!(text.starts_with("<!DOCTYPE html>"));
                        assert!(text.contains("Export &lt;safe&gt; &amp; résumé"));
                        assert!(text.contains("Résumé &lt;script&gt; &amp; export"));
                    } else {
                        assert!(text.starts_with("# Export <safe> & résumé\n"));
                        assert!(text.contains("Résumé <script> & export"));
                    }
                }
                assert_eq!(fs::read(&canonical).await.unwrap(), before);
                assert_eq!(std::fs::read_dir(destination.parent().unwrap()).unwrap().count(), 1);
            }
        }
    }

    #[tokio::test]
    async fn export_rejects_internal_config_and_checkpoint_without_changing_bytes() {
        let (_root, data, canonical) = export_fixture();
        // Use a unique config subtree and another session's sidecar so this test
        // neither mutates shared settings nor triggers source journal recovery.
        let profile = config_dir();
        let state = profile.join(format!("export-protected-{}", Uuid::new_v4()));
        fs::create_dir_all(&state).await.unwrap();
        let protected = [
            (state.join("config.json"), b"{\"nativeConfig\":\"preserve\"}\n".to_vec()),
            (canonical.with_file_name("other-session.checkpoint"), b"checkpoint\0\xff\n".to_vec()),
            (canonical.with_file_name("other-session.deleted"), b"deleted\n".to_vec()),
        ];
        for (path, bytes) in &protected {
            fs::write(path, bytes).await.unwrap();
        }
        let before = fs::read(&canonical).await.unwrap();
        for format in ["json", "markdown", "md", "html"] {
            for (destination, _) in &protected {
                let error = export_session(&data.session.id, &destination.to_string_lossy(), format)
                    .await
                    .unwrap_err();
                assert!(error.contains("internal state"), "{error}");
                for (path, bytes) in &protected {
                    assert_eq!(fs::read(path).await.unwrap(), *bytes, "changed {}", path.display());
                }
                assert_eq!(fs::read(&canonical).await.unwrap(), before);
            }
        }
        for destination in [profile.as_path(), state.as_path(), canonical.parent().unwrap()] {
            assert!(export_session(&data.session.id, &destination.to_string_lossy(), "html")
                .await
                .unwrap_err()
                .contains("internal state"));
        }
        assert_eq!(std::fs::read_dir(&state).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(canonical.parent().unwrap()).unwrap().count(), 3);
    }

    #[tokio::test]
    async fn export_rejects_canonical_session_without_changing_bytes() {
        let (_root, data, canonical) = export_fixture();
        let before = fs::read(&canonical).await.unwrap();
        let result = export_session(&data.session.id, &canonical.to_string_lossy(), "markdown").await;
        assert_eq!(
            fs::read(&canonical).await.unwrap(), before,
            "export must preserve canonical bytes; result: {result:?}"
        );
        assert!(result.unwrap_err().contains("internal state"));
    }

    #[tokio::test]
    async fn export_rejects_missing_internal_children_and_parent_traversal() {
        let (root, data, canonical) = export_fixture();
        let before = fs::read(&canonical).await.unwrap();
        let state = config_dir();
        assert!(state.starts_with(std::env::temp_dir()), "test profile must be isolated");
        let missing = format!("export-missing-{}", Uuid::new_v4());
        for destination in [
            canonical.parent().unwrap().join(&missing).join("child/export.json"),
            root.join("absent/../custom-sessions").join(&missing).join("export.md"),
            state.join(&missing).join("child/export.json"),
        ] {
            let result = export_session(&data.session.id, &destination.to_string_lossy(), "json").await;
            assert!(
                result.as_ref().is_err_and(|error| error.contains("internal state")),
                "must reject missing internal destination {}: {result:?}", destination.display()
            );
            assert!(!destination.exists());
            assert_eq!(fs::read(&canonical).await.unwrap(), before);
        }
        assert!(!canonical.parent().unwrap().join(&missing).exists());
        assert!(!root.join("absent").exists());
        assert!(!state.join(&missing).exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn export_rejects_windows_path_aliases_without_changing_bytes() {
        let (_root, data, canonical) = export_fixture();
        let before = fs::read(&canonical).await.unwrap();
        let sessions = canonical.parent().unwrap();
        let aliases = [
            canonical.to_string_lossy().to_uppercase(),
            format!(r"\\?\{}", canonical.display()),
            canonical.to_string_lossy().replace('\\', "/"),
            format!(r"{}. \{}", sessions.display(), canonical.file_name().unwrap().to_string_lossy()),
            format!(r"{}.\missing\export.md", sessions.display()),
            format!(r"{} \missing\export.md", sessions.display()),
            config_dir().join("export-missing/case/export.md").to_string_lossy().to_uppercase(),
        ];
        for destination in aliases {
            let result = export_session(&data.session.id, &destination, "markdown").await;
            assert_eq!(fs::read(&canonical).await.unwrap(), before, "alias overwrote canonical: {destination}");
            assert!(
                result.as_ref().is_err_and(|error| error.contains("internal state")),
                "must reject internal alias {destination}: {result:?}"
            );
        }
    }

    #[cfg(any(unix, windows))]
    fn export_directory_link(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .unwrap()
            .success(), "could not create isolated export junction");
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn export_rejects_links_into_internal_roots_without_changing_bytes() {
        let (root, data, canonical) = export_fixture();
        let state = config_dir().join(format!("export-linked-{}", Uuid::new_v4()));
        fs::create_dir_all(&state).await.unwrap();
        let config = state.join("config.json");
        let config_bytes = b"{\"keep\":\"config bytes\"}\n";
        fs::write(&config, config_bytes).await.unwrap();
        let before = fs::read(&canonical).await.unwrap();
        for (name, target, protected) in [
            ("sessions-link", canonical.parent().unwrap(), canonical.as_path()),
            ("state-link", state.as_path(), config.as_path()),
        ] {
            let link = root.join(name);
            export_directory_link(target, &link);
            for destination in [
                link.join(protected.file_name().unwrap()),
                link.join("missing/nested/export.html"),
            ] {
                let error = export_session(&data.session.id, &destination.to_string_lossy(), "html")
                    .await
                    .unwrap_err();
                assert!(error.contains("internal state"), "{error}");
                assert_eq!(fs::read(&canonical).await.unwrap(), before);
                assert_eq!(fs::read(&config).await.unwrap(), config_bytes);
            }
            assert!(!target.join("missing").exists());
        }
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn export_link_parent_traversal_never_overwrites_canonical() {
        let (root, data, canonical) = export_fixture();
        let outside = root.join("outside");
        fs::create_dir(&outside).await.unwrap();
        let link = canonical.parent().unwrap().join("outside-link");
        export_directory_link(&outside, &link);
        let before = fs::read(&canonical).await.unwrap();
        let destination = link.join("..").join(canonical.file_name().unwrap());
        let result = export_session(&data.session.id, &destination.to_string_lossy(), "markdown").await;
        assert_eq!(fs::read(&canonical).await.unwrap(), before, "link/.. bypass: {result:?}");
        result.expect("the validated outside destination must remain exportable");
        let exported = fs::read_to_string(root.join(canonical.file_name().unwrap())).await.unwrap();
        assert!(exported.starts_with("# Test\n"));
        assert!(exported.contains("Keep canonical history"));
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn regression_attachment_rejects_external_link_and_missing_workspace() {
        let root = fixture_dir();
        let workspace = root.join("workspace");
        let outside = root.join("outside");
        fs::create_dir_all(workspace.join(".aether-agent"))
            .await
            .unwrap();
        fs::create_dir(&outside).await.unwrap();
        let link = workspace.join(".aether-agent").join("attachments");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        {
            // Fixed command, generated fixture paths only; creates a junction
            // without Developer Mode/admin privileges. Never launches payloads.
            let status = std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link.to_string_lossy().replace('/', "\\"))
                .arg(outside.to_string_lossy().replace('/', "\\"))
                .status()
                .unwrap();
            assert!(
                status.success(),
                "Could not create isolated junction fixture"
            );
        }
        assert!(
            save_attachment(Some(&workspace.to_string_lossy()), "image.png", b"fixture")
                .await
                .is_err()
        );
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        assert!(save_attachment(
            Some(&root.join("missing").to_string_lossy()),
            "image.png",
            b"fixture"
        )
        .await
        .is_err());
        #[cfg(windows)]
        fs::remove_dir(&link).await.unwrap();
        #[cfg(unix)]
        fs::remove_file(&link).await.unwrap();
        fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn regression_repeated_compaction_retains_first_checkpoint_and_preamble() {
        let mut answer = message("assistant", "parser complete");
        answer.preamble = Some("Unique pre-tool conclusion".into());
        let first = compact_data(data_with_messages(vec![
            message("user", "Unique original parser requirement"),
            answer,
            message("user", "second turn"),
            message("assistant", "second result"),
        ]))
        .unwrap();
        let mut next = first;
        next.messages.extend([
            message("user", "third turn"),
            message("assistant", "third result"),
        ]);
        let second = compact_data(next).unwrap();
        assert!(second.messages[0]
            .content
            .contains("Unique original parser requirement"));
        assert!(second.messages[0]
            .content
            .contains("Unique pre-tool conclusion"));
        assert!(second.messages[0].content.chars().count() <= 24_000);
    }

    #[test]
    fn regression_transcript_frontend_multitool_aggregate_is_not_duplicated() {
        let first = serde_json::json!({"id":"tool-1","name":"read_file","args":{},"status":"done","result":"first result"});
        let second = serde_json::json!({"id":"tool-2","name":"read_file","args":{},"status":"done","result":"second result"});
        // flushPending appends post-first-tool chunks both to segments and
        // content; only the opening text is moved out to preamble.
        let msg: ChatMessage = serde_json::from_value(serde_json::json!({
            "id":"assistant-1","role":"assistant","timestamp":1,
            "content":"intermediate narrative final narrative",
            "preamble":"opening narrative",
            "toolCalls":[first.clone(), second.clone()],
            "segments":[
                {"id":"s0","text":"opening narrative","toolCalls":[first]},
                {"id":"s1","text":"intermediate narrative ","toolCalls":[second]},
                {"id":"s2","text":"final narrative"}
            ]
        }))
        .unwrap();
        let data = data_with_messages(vec![msg]);
        let roundtrip: SessionData =
            serde_json::from_slice(&serde_json::to_vec(&data).unwrap()).unwrap();
        for rendered in [
            render_markdown(&roundtrip),
            render_standalone_html(&roundtrip),
            deterministic_summary(&roundtrip.messages),
        ] {
            for text in [
                "opening narrative",
                "intermediate narrative",
                "final narrative",
            ] {
                assert_eq!(
                    rendered.matches(text).count(),
                    1,
                    "missing/duplicate {text}: {rendered}"
                );
            }
            assert!(rendered.find("opening narrative") < rendered.find("intermediate narrative"));
            assert!(rendered.find("intermediate narrative") < rendered.find("final narrative"));
        }
        let text = transcript_text(&roundtrip.messages[0]);
        for (before, after) in [
            ("opening narrative", "first result"),
            ("first result", "intermediate narrative"),
            ("intermediate narrative", "second result"),
            ("second result", "final narrative"),
        ] {
            assert!(text.find(before) < text.find(after), "{text}");
        }
    }

    #[test]
    fn regression_transcript_preserves_only_absent_legacy_tail() {
        for (content, expected) in [
            ("middle final", vec!["opening", "middle final"]),
            (
                "middle final plus tail",
                vec!["opening", "middle final", " plus tail"],
            ),
            (
                "absent final",
                vec!["opening", "middle final", "absent final"],
            ),
            ("final", vec!["opening", "middle final", "final"]),
        ] {
            let msg: ChatMessage = serde_json::from_value(serde_json::json!({
                "id":"assistant-1","role":"assistant","timestamp":1,
                "content":content,"preamble":"opening",
                "segments":[{"id":"s0","text":"opening"},{"id":"s1","text":"middle final"}]
            }))
            .unwrap();
            let text: Vec<_> = ordered_transcript(&msg)
                .into_iter()
                .filter_map(|part| match part {
                    TranscriptPart::Text(text) => Some(text),
                    _ => None,
                })
                .collect();
            assert_eq!(text, expected);
        }
    }

    #[test]
    fn regression_exports_ordered_segments_and_results_once() {
        let call = serde_json::json!({"id":"tool-1","name":"read_file","args":{"path":"file.txt"},"status":"done","result":"unique result <safe>"});
        let msg: ChatMessage = serde_json::from_value(serde_json::json!({
            "id":"assistant-1","role":"assistant","timestamp":1,"content":"final narrative",
            "preamble":"before tool","toolCalls":[call.clone()],
            "segments":[{"id":"s0","text":"before tool","toolCalls":[call]},
                {"id":"s1","text":"between tools"},{"id":"s2","text":"final narrative"}]
        }))
        .unwrap();
        let data = data_with_messages(vec![msg]);
        let roundtrip: SessionData =
            serde_json::from_slice(&serde_json::to_vec(&data).unwrap()).unwrap();
        for rendered in [
            render_markdown(&roundtrip),
            render_standalone_html(&roundtrip),
        ] {
            for needle in [
                "before tool",
                "unique result",
                "between tools",
                "final narrative",
            ] {
                assert_eq!(
                    rendered.matches(needle).count(),
                    1,
                    "missing/duplicate {needle}"
                );
            }
            assert!(rendered.find("before tool") < rendered.find("unique result"));
            assert!(rendered.find("unique result") < rendered.find("between tools"));
            assert!(rendered.find("between tools") < rendered.find("final narrative"));
            assert!(rendered.contains("file.txt"));
        }
        assert!(render_standalone_html(&roundtrip).contains("&lt;safe&gt;"));
    }

    #[tokio::test]
    async fn streaming_checkpoint_roundtrips_partial_text_and_final_save_replaces_it() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let mut partial = message("assistant", "partial answer");
        partial.is_streaming = Some(true);
        partial.run_id = Some("run-checkpoint".into());
        let data = data_with_messages(vec![partial]);
        save_session_file(&path, data.clone(), false).await.unwrap();
        let recovered = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recovered.messages[0].content, "partial answer");
        assert_eq!(recovered.messages[0].is_streaming, Some(true));
        assert_eq!(
            recovered.messages[0].run_id.as_deref(),
            Some("run-checkpoint")
        );
        let mut completed = data;
        completed.messages[0].content.push_str(" completed");
        completed.messages[0].is_streaming = Some(false);
        save_session_file(&path, completed, false).await.unwrap();
        let recovered = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recovered.messages[0].content, "partial answer completed");
        assert_eq!(recovered.messages[0].is_streaming, Some(false));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn regression_corrupt_session_is_never_overwritten_and_list_reports_path() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        for corrupt in ["{broken", "{}", "{\"session\":{\"id\":\"test-session\"}}"] {
            fs::write(&path, corrupt).await.unwrap();
            assert!(save_session_file(&path, data_with_messages(vec![]), false)
                .await
                .is_err());
            assert_eq!(fs::read_to_string(&path).await.unwrap(), corrupt);
            let error = read_session_directory(&root).await.unwrap_err();
            assert!(error.contains("test-session.json"), "{error}");
        }
        fs::remove_file(&path).await.unwrap();
        save_session_file(&path, data_with_messages(vec![]), false)
            .await
            .unwrap();
        assert_eq!(read_session_directory(&root).await.unwrap().len(), 1);
        let invalid_dir = root.join("not-directory");
        fs::write(&invalid_dir, "x").await.unwrap();
        assert!(read_session_directory(&invalid_dir).await.is_err());
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn regression_stale_save_cannot_restore_title_pin_or_deleted_session() {
        let root = fixture_dir();
        let path = root.join("test-session.json");
        let mut original = data_with_messages(vec![message("user", "hello")]);
        original.session.pinned = true;
        original.session.pinned_at = Some(10);
        save_session_file(&path, original.clone(), false)
            .await
            .unwrap();
        let mut renamed = original.clone();
        renamed.session.title = "Renamed explicitly".into();
        renamed.session.pinned = false;
        renamed.session.pinned_at = None;
        save_session_file(&path, renamed, true).await.unwrap();
        save_session_file(&path, original.clone(), false)
            .await
            .unwrap();
        let stored = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.session.title, "Renamed explicitly");
        assert!(!stored.session.pinned);
        fs::write(path.with_extension("deleted"), "deleted")
            .await
            .unwrap();
        fs::remove_file(&path).await.unwrap();
        assert!(save_session_file(&path, original, false).await.is_err());
        assert!(!path.exists());
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn regression_clear_workspace_propagates_remove_errors() {
        let root = fixture_dir();
        let path = root.join("workspace.json");
        fs::create_dir(&path).await.unwrap();
        assert!(clear_workspace_file(&path).await.is_err());
        fs::remove_dir(&path).await.unwrap();
        assert!(clear_workspace_file(&path).await.is_ok());
        fs::write(&path, "workspace").await.unwrap();
        clear_workspace_file(&path).await.unwrap();
        assert!(!path.exists());
        fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn reflection_survives_stale_session_saves_without_resurrecting_deleted_turns() {
        let mut stored = data_with_messages(vec![message("assistant", "Fixed")]);
        stored.messages[0].run_id = Some("run-1".into());
        stored.messages[0].self_improvement = Some(super::super::types::SelfImprovementEvent {
            changes: Vec::new(),
            run_id: "run-1".into(),
            session_id: "test-session".into(),
            action: super::super::types::SelfImprovementAction::SkillCreated,
            skill_name: Some("test-recipe".into()),
            summary: None,
            workspace_cwd: None,
        });
        let mut stale = stored.clone();
        stale.messages[0].self_improvement = None;
        merge_self_improvements(&mut stale, &stored);
        assert_eq!(
            stale.messages[0].self_improvement,
            stored.messages[0].self_improvement
        );
        stale.messages.clear();
        merge_self_improvements(&mut stale, &stored);
        assert!(stale.messages.is_empty());
    }

    #[tokio::test]
    async fn review_proposal_fallback_preserves_existing_review_and_requires_assistant() {
        let path = fixture_dir().join("session.json");
        let mut data = data_with_messages(vec![message("assistant", "Done")]);
        data.messages[0].run_id = Some("run".into());
        data.messages[0].self_improvement = Some(serde_json::from_value(serde_json::json!({
            "run_id":"run", "session_id":"test-session", "action":"skill_updated", "skill_name":"existing",
            "summary":"original summary", "workspace_cwd":"original workspace",
            "changes":[{"action":"skill_updated","skill_name":"existing"}]
        })).unwrap());
        let original = data.messages[0].self_improvement.clone().unwrap();
        save_session_file(&path, data, false).await.unwrap();
        let write = super::super::review_pending::PendingWrite {
            id: "new".into(),
            session_id: "test-session".into(),
            run_id: "run".into(),
            skill_name: "new".into(),
            target_path: "/skills/new/SKILL.md".into(),
            skill_root: "/skills/new".into(),
            original: None,
            proposed: Some("content".into()),
        };
        attach_pending_link_file(&path, &write).await.unwrap();
        resolve_review_proposal_file(&path, "test-session", "new", None)
            .await
            .unwrap();
        let mut saved = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        let event = saved.messages[0].self_improvement.as_ref().unwrap();
        assert_eq!(event.changes[0], original.changes[0]);
        assert_eq!(event.action, original.action);
        assert_eq!(event.skill_name, original.skill_name);
        assert_eq!(event.summary, original.summary);
        assert_eq!(event.workspace_cwd, original.workspace_cwd);
        saved.messages[0].role = "user".into();
        let bytes = serde_json::to_vec_pretty(&saved).unwrap();
        atomic_write(&path, &bytes).await.unwrap();
        assert!(attach_pending_link_file(&path, &write).await.is_err());
        assert_eq!(bytes, fs::read(&path).await.unwrap());
    }

    #[tokio::test]
    async fn review_proposal_fallback_requires_sealed_pending_authority() {
        let root = fixture_dir().join("pending");
        let skill = fixture_dir().join("skill");
        let write = super::super::review_pending::PendingWrite {
            id: Uuid::new_v4().to_string(),
            session_id: "test-session".into(),
            run_id: "run".into(),
            skill_name: "safe".into(),
            target_path: skill.join("SKILL.md").to_string_lossy().into_owned(),
            skill_root: skill.to_string_lossy().into_owned(),
            original: None,
            proposed: Some("content".into()),
        };
        let id = super::super::review_pending::stage(&root, write)
            .await
            .unwrap();
        let sealed = load_pending_link(&root, &id, "test-session").await.unwrap();
        assert_eq!(sealed.skill_name, "safe");
        assert!(load_pending_link(&root, &id, "foreign").await.is_err());
        assert!(load_pending_link(&root, "../outside", "test-session")
            .await
            .is_err());
        let public = root.join(format!("{id}.json"));
        let bytes = fs::read(&public).await.unwrap();
        let mut tampered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        tampered["run_id"] = serde_json::json!("foreign-run");
        fs::write(&public, serde_json::to_vec(&tampered).unwrap())
            .await
            .unwrap();
        assert!(load_pending_link(&root, &id, "test-session").await.is_err());
        // skill_root is deliberately absent from pending_diff's view: the
        // complete staging snapshot must still prevent a wider destination.
        tampered = serde_json::from_slice(&bytes).unwrap();
        tampered["skill_root"] = serde_json::json!("/outside");
        fs::write(&public, serde_json::to_vec(&tampered).unwrap())
            .await
            .unwrap();
        assert!(load_pending_link(&root, &id, "test-session").await.is_err());
    }
    #[tokio::test]
    async fn review_proposal_fallback_attachment_merges_and_is_idempotent() {
        use super::super::types::{
            SelfImprovementAction as Action, SelfImprovementChange as Change,
        };
        let path = fixture_dir().join("session.json");
        let mut data = data_with_messages(vec![message("assistant", "Done")]);
        data.messages[0].run_id = Some("run".into());
        save_session_file(&path, data, false).await.unwrap();
        let write = super::super::review_pending::PendingWrite {
            id: "pending".into(),
            session_id: "test-session".into(),
            run_id: "run".into(),
            skill_name: "one".into(),
            target_path: "/skills/one/SKILL.md".into(),
            skill_root: "/skills/one".into(),
            original: None,
            proposed: Some("new".into()),
        };
        attach_pending_link_file(&path, &write).await.unwrap();
        let first = fs::read(&path).await.unwrap();
        attach_pending_link_file(&path, &write).await.unwrap();
        assert_eq!(first, fs::read(&path).await.unwrap());
        let mut second = write.clone();
        second.id = "other".into();
        second.skill_name = "two".into();
        second.target_path = "/skills/two/SKILL.md".into();
        second.skill_root = "/skills/two".into();
        attach_pending_link_file(&path, &second).await.unwrap();
        let mut approved = Change {
            action: Action::SkillCreated,
            skill_name: write.skill_name.clone(),
            file_path: Some(write.target_path.clone()),
            skill_path: Some(write.skill_root.clone()),
            pending_id: None,
            summary: Some("Approved".into()),
        };
        resolve_review_proposal_file(&path, "test-session", "pending", Some(approved.clone()))
            .await
            .unwrap();
        attach_pending_link_file(&path, &write).await.unwrap();
        resolve_review_proposal_file(&path, "test-session", "other", None)
            .await
            .unwrap();
        let saved = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        let event = saved.messages[0].self_improvement.as_ref().unwrap();
        approved.pending_id = Some("pending".into());
        assert_eq!(
            event.changes,
            vec![
                approved,
                Change {
                    action: Action::NoAction,
                    skill_name: "two".into(),
                    file_path: Some(second.target_path),
                    skill_path: Some(second.skill_root),
                    pending_id: Some("other".into()),
                    summary: Some("Rejected review skill proposal".into())
                }
            ]
        );
        assert_eq!(event.action, Action::SkillCreated);
        let before = fs::read(&path).await.unwrap();
        let mut invalid = write.clone();
        invalid.run_id = "unsaved".into();
        assert!(attach_pending_link_file(&path, &invalid)
            .await
            .unwrap_err()
            .contains("wait"));
        invalid = write.clone();
        invalid.skill_name = "tampered".into();
        assert!(attach_pending_link_file(&path, &invalid).await.is_err());
        invalid = write;
        invalid.session_id = "foreign".into();
        assert!(attach_pending_link_file(&path, &invalid).await.is_err());
        assert_eq!(before, fs::read(&path).await.unwrap());
    }
    #[tokio::test]
    async fn review_proposal_resolution_persists_and_survives_stale_saves() {
        use super::super::types::{
            SelfImprovementAction as Action, SelfImprovementChange as Change, SelfImprovementEvent,
        };
        let root = fixture_dir();
        let path = root.join("session.json");
        let mut data = data_with_messages(vec![message("assistant", "Reviewed")]);
        let staged = |id: &str, name: &str| Change {
            action: Action::SkillStaged,
            skill_name: name.into(),
            file_path: Some(format!("/skills/{name}/SKILL.md")),
            skill_path: Some(format!("/skills/{name}")),
            pending_id: Some(id.into()),
            summary: None,
        };
        data.messages[0].self_improvement = Some(SelfImprovementEvent {
            run_id: "run-1".into(),
            session_id: "test-session".into(),
            changes: vec![staged("first", "one"), staged("second", "two")],
            action: Action::SkillStaged,
            skill_name: Some("one".into()),
            summary: Some("Legacy summary".into()),
            workspace_cwd: Some("workspace".into()),
        });
        save_session_file(&path, data.clone(), false).await.unwrap();
        let mut approved = staged("first", "one");
        approved.action = Action::SkillCreated;
        approved.pending_id = None;
        resolve_review_proposal_file(&path, "test-session", "first", Some(approved))
            .await
            .unwrap();
        resolve_review_proposal_file(&path, "test-session", "second", None)
            .await
            .unwrap();
        // A stale frontend checkpoint must not resurrect either staged state.
        save_session_file(&path, data.clone(), false).await.unwrap();
        let stored = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        let event = stored.messages[0].self_improvement.as_ref().unwrap();
        assert_eq!(event.action, Action::SkillCreated);
        assert_eq!(event.skill_name.as_deref(), Some("one"));
        assert_eq!(event.changes[0].pending_id.as_deref(), Some("first"));
        assert_eq!(event.changes[1].action, Action::NoAction);
        assert_eq!(event.changes[1].pending_id.as_deref(), Some("second"));
        assert_eq!(event.summary.as_deref(), Some("Legacy summary"));
        assert_eq!(event.workspace_cwd.as_deref(), Some("workspace"));
        let bytes = fs::read(&path).await.unwrap();
        assert!(
            resolve_review_proposal_file(&path, "foreign", "first", None)
                .await
                .is_err()
        );
        assert!(
            resolve_review_proposal_file(&path, "test-session", "missing", None)
                .await
                .is_err()
        );
        assert_eq!(fs::read(&path).await.unwrap(), bytes);
        resolve_review_proposal_file(&path, "test-session", "first", None)
            .await
            .unwrap();
        let stored = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        let event = stored.messages[0].self_improvement.as_ref().unwrap();
        assert_eq!(event.action, Action::NoAction);
        assert_eq!(event.skill_name, None);
    }

    #[tokio::test]
    async fn review_proposal_rejection_preserves_other_active_change() {
        use super::super::types::{
            SelfImprovementAction as Action, SelfImprovementChange as Change,
        };
        let root = fixture_dir();
        let path = root.join("session.json");
        let mut data = data_with_messages(vec![message("assistant", "Reviewed")]);
        data.messages[0].self_improvement = Some(serde_json::from_value(serde_json::json!({
            "run_id":"run", "session_id":"test-session", "action":"skill_staged", "skill_name":"one",
            "changes":[
                {"action":"skill_staged","skill_name":"one","pending_id":"pending"},
                {"action":"skill_updated","skill_name":"two"}
            ]
        })).unwrap());
        save_session_file(&path, data, false).await.unwrap();
        let invalid = Change {
            action: Action::SkillUpdated,
            skill_name: "foreign".into(),
            file_path: None,
            skill_path: None,
            pending_id: None,
            summary: None,
        };
        assert!(
            resolve_review_proposal_file(&path, "test-session", "pending", Some(invalid))
                .await
                .is_err()
        );
        resolve_review_proposal_file(&path, "test-session", "pending", None)
            .await
            .unwrap();
        let stored = read_session_file(&path, "test-session")
            .await
            .unwrap()
            .unwrap();
        let event = stored.messages[0].self_improvement.as_ref().unwrap();
        assert_eq!(event.action, Action::SkillUpdated);
        assert_eq!(event.skill_name.as_deref(), Some("two"));
    }

    fn review_merge_event(changes: serde_json::Value) -> super::super::types::SelfImprovementEvent {
        serde_json::from_value(serde_json::json!({
            "run_id":"run", "session_id":"test-session", "action":"skill_staged",
            "skill_name":"one", "summary":"Original review", "changes":changes
        }))
        .unwrap()
    }

    #[test]
    fn review_event_merge_preserves_resolutions_and_unrelated_changes() {
        let mut current = review_merge_event(serde_json::json!([
            {"action":"skill_created","skill_name":"one","pending_id":"approved"},
            {"action":"no_action","skill_name":"two","pending_id":"rejected"},
            {"action":"skill_updated","skill_name":"manual","file_path":"/manual"}
        ]));
        current.action = super::super::types::SelfImprovementAction::SkillCreated;
        let incoming = review_merge_event(serde_json::json!([
            {"action":"skill_staged","skill_name":"one","pending_id":"approved"},
            {"action":"skill_staged","skill_name":"two","pending_id":"rejected"}
        ]));
        assert_eq!(merge_review_events(&current, &incoming), current);
    }

    #[test]
    fn review_event_merge_accepts_new_manual_changes_without_replacing_pending() {
        let current = review_merge_event(serde_json::json!([
            {"action":"skill_staged","skill_name":"one","pending_id":"pending","file_path":"/one"}
        ]));
        let mut incoming = review_merge_event(serde_json::json!([
            {"action":"skill_updated","skill_name":"one","file_path":"/one","summary":"First edit"},
            {"action":"skill_updated","skill_name":"one","file_path":"/one","summary":"Second edit"}
        ]));
        incoming.summary = Some("New manual review".into());
        let merged = merge_review_events(&current, &incoming);
        assert_eq!(merged.changes.len(), 3);
        assert_eq!(merged.changes[0], current.changes[0]);
        assert_eq!(merged.changes[1..], incoming.changes);
        assert_eq!(merged.summary, incoming.summary);
        assert_eq!(merge_review_events(&merged, &incoming), merged);
    }

    #[test]
    fn review_event_merge_accepts_resolution_and_keeps_native_resolution_on_conflict() {
        let staged = review_merge_event(serde_json::json!([
            {"action":"skill_staged","skill_name":"one","pending_id":"pending"}
        ]));
        let resolved = review_merge_event(serde_json::json!([
            {"action":"skill_created","skill_name":"one","pending_id":"pending"}
        ]));
        let merged = merge_review_events(&staged, &resolved);
        assert_eq!(merged.changes, resolved.changes);
        assert_eq!(
            merged.action,
            super::super::types::SelfImprovementAction::SkillCreated
        );
        let rejected = review_merge_event(serde_json::json!([
            {"action":"no_action","skill_name":"one","pending_id":"pending"}
        ]));
        assert_eq!(merge_review_events(&merged, &rejected), merged);
    }

    #[test]
    fn review_event_merge_legacy_conclusions_remain_replaceable() {
        let current = review_merge_event(serde_json::json!([]));
        assert_eq!(merge_review_events(&current, &current), current);
        let mut incoming = current.clone();
        incoming.action = super::super::types::SelfImprovementAction::NoAction;
        incoming.skill_name = None;
        incoming.summary = Some("New manual conclusion".into());
        assert_eq!(merge_review_events(&current, &incoming), incoming);
        incoming.run_id = "another-run".into();
        assert_eq!(merge_review_events(&current, &incoming), incoming);
    }
    #[test]
    fn review_event_merge_appends_distinct_pending_ids_even_for_the_same_file() {
        let current = review_merge_event(serde_json::json!([
            {"action":"skill_staged","skill_name":"one","pending_id":"first","file_path":"/one"}
        ]));
        let incoming = review_merge_event(serde_json::json!([
            {"action":"skill_staged","skill_name":"one","pending_id":"second","file_path":"/one"}
        ]));
        let merged = merge_review_events(&current, &incoming);
        assert_eq!(merged.changes.len(), 2);
        assert_eq!(merged.changes[0], current.changes[0]);
        assert_eq!(merged.changes[1], incoming.changes[0]);
        assert_eq!(merge_review_events(&merged, &incoming), merged);
        let empty_retry = review_merge_event(serde_json::json!([]));
        assert_eq!(merge_review_events(&merged, &empty_retry), merged);
    }
    fn message(role: &str, content: &str) -> ChatMessage {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "role": role,
            "content": content,
            "timestamp": 1
        }))
        .unwrap()
    }

    fn data_with_messages(messages: Vec<ChatMessage>) -> SessionData {
        SessionData {
            session: Session {
                id: "test-session".into(),
                title: "Test".into(),
                created_at: 1,
                updated_at: 1,
                message_count: messages.len(),
                workspace_cwd: None,
                preview: None,
                pinned: false,
                pinned_at: None,
            },
            messages,
            artifacts: Vec::new(),
            model_override: None,
        }
    }

    #[test]
    fn unpinning_a_session_is_not_resurrected_by_stale_disk_state() {
        let mut stored = data_with_messages(vec![message("user", "hi")]);
        stored.session.pinned = true;
        stored.session.pinned_at = Some(10);

        // Save biasa dari payload tanpa info pin: pin dilindungi dari state default.
        let mut default_payload = data_with_messages(vec![message("user", "hi")]);
        merge_pin_state(&mut default_payload.session, &stored.session, false);
        assert!(
            default_payload.session.pinned,
            "save biasa harus mempertahankan pin yang ada di disk"
        );

        // Payload hasil toggle_pin_session (pin dilepas eksplisit): harus menang.
        let mut unpinned = data_with_messages(vec![message("user", "hi")]);
        unpinned.session.pinned = false;
        unpinned.session.pinned_at = None;
        merge_pin_state(&mut unpinned.session, &stored.session, true);
        assert!(
            !unpinned.session.pinned,
            "pin yang sudah dilepas tidak boleh dihidupkan kembali oleh save"
        );
        assert_eq!(unpinned.session.pinned_at, None);
    }

    #[test]
    fn migrates_legacy_session_shape() {
        let value = serde_json::json!({"id":"sess-1","title":"Legacy","createdAt":1,"updatedAt":2,"messages":[],"artifacts":[]});
        let data = parse_session(value).unwrap();
        assert_eq!(data.session.id, "sess-1");
        assert_eq!(data.session.title, "Legacy");
    }

    #[test]
    fn rejects_path_shaped_session_ids() {
        assert!(session_path("../escape").is_err());
        assert!(session_path("valid-session_1").is_ok());
    }

    #[test]
    fn creates_compact_preview() {
        let data = vec![ChatMessage {
            selected_skills: Vec::new(),
            checkpoint_epoch: None,
            id: "1".into(),
            role: "user".into(),
            content: "  hello\n world ".into(),
            timestamp: 1,
            reasoning: None,
            tool_calls: None,
            is_streaming: None,
            attachments: None,
            image_inputs: None,
            council_turn: None,
            run_id: None,
            run_status: None,
            self_improvement: None,
            provider_messages: None,
            has_image_inputs: None,
            has_provider_messages: None,
            preamble: None,
            segments: None,
        }];
        assert_eq!(session_preview(&data).as_deref(), Some("hello world"));
    }

    #[test]
    fn compaction_is_local_turn_aware_and_reads_object_arguments() {
        let mut first_answer = message("assistant", "Implemented the parser");
        first_answer.tool_calls = Some(vec![super::super::types::ToolCallItem {
            id: "call-1".into(),
            name: "edit_file".into(),
            args: serde_json::json!({"path": "src/main.rs"}),
            status: "done".into(),
            ..Default::default()
        }]);
        let data = data_with_messages(vec![
            message("user", "Implement a parser"),
            first_answer,
            message("user", "Add focused tests"),
            message("assistant", "Tests added"),
        ]);
        let compacted = compact_data(data).unwrap();
        assert_eq!(compacted.messages.len(), 3);
        assert_eq!(compacted.messages[0].role, "system");
        assert!(compacted.messages[0].content.contains("src/main.rs"));
        assert!(compacted.messages[0]
            .content
            .contains("Implemented the parser"));
        assert_eq!(compacted.messages[1].content, "Add focused tests");
        assert_eq!(compacted.messages[2].content, "Tests added");
    }

    #[test]
    fn compaction_rejects_in_progress_or_single_turn_histories() {
        let mut streaming = data_with_messages(vec![
            message("user", "one"),
            message("assistant", "answer"),
            message("user", "two"),
            message("assistant", "pending"),
        ]);
        streaming.messages[3].is_streaming = Some(true);
        assert!(compact_data(streaming).is_err());

        let single_turn =
            data_with_messages(vec![message("user", "one"), message("assistant", "answer")]);
        assert!(compact_data(single_turn).is_err());
    }

    #[test]
    fn session_deserialization_defaults_pinned_to_false() {
        let raw = serde_json::json!({
            "id": "sess-legacy",
            "title": "Legacy Session",
            "createdAt": 1000,
            "updatedAt": 2000,
            "messageCount": 0
        });
        let session: Session = serde_json::from_value(raw).unwrap();
        assert!(!session.pinned);
        assert_eq!(session.pinned_at, None);
    }

    #[test]
    fn pinned_sessions_sort_ahead_of_recent_unpinned_sessions() {
        let s1 = Session {
            id: "s1".into(),
            title: "Old Pinned".into(),
            created_at: 100,
            updated_at: 100,
            message_count: 0,
            workspace_cwd: None,
            preview: None,
            pinned: true,
            pinned_at: Some(500),
        };
        let s2 = Session {
            id: "s2".into(),
            title: "Newer Unpinned".into(),
            created_at: 1000,
            updated_at: 2000,
            message_count: 0,
            workspace_cwd: None,
            preview: None,
            pinned: false,
            pinned_at: None,
        };
        let s3 = Session {
            id: "s3".into(),
            title: "Newly Pinned".into(),
            created_at: 100,
            updated_at: 100,
            message_count: 0,
            workspace_cwd: None,
            preview: None,
            pinned: true,
            pinned_at: Some(900),
        };

        let mut list = [s1, s2, s3];
        list.sort_by(|a, b| match (a.pinned, b.pinned) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (true, true) => {
                let a_pin = a.pinned_at.unwrap_or(a.updated_at);
                let b_pin = b.pinned_at.unwrap_or(b.updated_at);
                b_pin.cmp(&a_pin)
            }
            (false, false) => b.updated_at.cmp(&a.updated_at),
        });

        assert_eq!(list[0].id, "s3");
        assert_eq!(list[1].id, "s1");
        assert_eq!(list[2].id, "s2");
    }

    #[test]
    fn global_artifact_item_round_trips_json() {
        let item = GlobalArtifactItem {
            session_id: "sess-1".into(),
            session_title: "Coding Session".into(),
            session_updated_at: 12345678,
            artifact: super::super::types::Artifact {
                stream_revision: None,
                id: "art-1".into(),
                title: "index.ts".into(),
                artifact_type: "code".into(),
                language: Some("typescript".into()),
                content: "console.log('hi');".into(),
                file_path: Some("src/index.ts".into()),
                timestamp: 12345600,
            },
        };
        let val = serde_json::to_value(&item).unwrap();
        assert_eq!(val["sessionId"], "sess-1");
        assert_eq!(val["sessionTitle"], "Coding Session");
        assert_eq!(val["sessionUpdatedAt"], 12345678);
        assert_eq!(val["artifact"]["title"], "index.ts");
        assert_eq!(val["artifact"]["type"], "code");
    }

    #[test]
    fn inspect_paths_distinguishes_files_and_directories() {
        let temp_dir = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let test_file = temp_dir.join(format!("aether_test_{}.txt", Uuid::new_v4().simple()));
        std::fs::write(&test_file, b"sample text").unwrap();

        let inspected = inspect_paths(vec![
            temp_dir.to_string_lossy().to_string(),
            test_file.to_string_lossy().to_string(),
            "".to_string(),
        ]);

        let _ = std::fs::remove_file(&test_file);

        assert_eq!(inspected.len(), 2);
        assert!(inspected[0].is_dir);
        assert!(!inspected[1].is_dir);
        assert!(inspected[1].name.ends_with(".txt"));
    }

    #[tokio::test]
    async fn save_attachment_persists_bytes_and_rejects_empty_data() {
        let empty_err = save_attachment(None, "test.png", b"").await;
        assert!(empty_err.is_err());

        let temp_workspace = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("aether_ws_{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&temp_workspace).unwrap();

        let att = save_attachment(
            Some(&temp_workspace.to_string_lossy()),
            "screenshot.png",
            b"fake-image-bytes",
        )
        .await
        .unwrap();

        assert_eq!(att.name, "screenshot.png");
        assert!(!att.is_dir);
        assert!(att.path.contains(".aether-agent"));
        assert!(std::path::Path::new(&att.path).exists());

        let read_back = std::fs::read(&att.path).unwrap();
        assert_eq!(read_back, b"fake-image-bytes");

        let _ = std::fs::remove_dir_all(&temp_workspace);
    }
}
