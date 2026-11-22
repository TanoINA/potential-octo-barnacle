//! Derived, disposable SQLite projection of canonical session JSON.
//!
//! The first read of an uncached/externally changed file parses the whole file;
//! warm reads only stat that file and decode bounded, indexed SQLite rows.
//! Callers hold storage's keyed session lock across canonical writes and
//! `cache_session`. Catalog reads acquire the same lock per file before SQLite;
//! page callers acquire it before entering this module. No async storage calls.
//! Page artifacts are deliberately partial (at most 80 referenced + 20 recent
//! unlinked artifacts); never use page data as an authoritative full snapshot.

use super::storage::{config_dir, parse_session};
use super::types::{Artifact, ChatMessage, Session, SessionData, SessionPage, SessionWindow};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

static DATABASES: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<Connection>>>>> = OnceLock::new();

fn db_path() -> PathBuf {
    config_dir().join("session-pages.sqlite")
}

fn db_error(error: impl std::fmt::Display) -> String {
    format!("Session paging projection: {error}")
}

fn path_key(path: &Path) -> Result<String, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(db_error)?.join(path)
    };
    absolute
        .into_os_string()
        .into_string()
        .map_err(|_| "Session path is not UTF-8".into())
}

fn with_db<T>(
    db: &Path,
    action: impl FnOnce(&mut Connection) -> Result<T, String>,
) -> Result<T, String> {
    let mut databases = DATABASES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| db_error("database mutex poisoned"))?;
    if !databases.contains_key(db) {
        if let Some(parent) = db.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(db_error)?;
        }
        let connection = Connection::open(db).map_err(db_error)?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(db_error)?;
        let _ = connection.pragma_update(None, "journal_mode", "WAL");
        let _ = connection.pragma_update(None, "synchronous", "NORMAL");
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS page_sessions (
               path TEXT PRIMARY KEY, session_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
               revision TEXT NOT NULL, total INTEGER NOT NULL, header TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS page_messages (
               path TEXT NOT NULL REFERENCES page_sessions(path) ON DELETE CASCADE,
               ordinal INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY(path, ordinal));
             CREATE TABLE IF NOT EXISTS page_artifacts (
               path TEXT NOT NULL REFERENCES page_sessions(path) ON DELETE CASCADE,
               artifact_id TEXT NOT NULL, timestamp INTEGER NOT NULL, linked INTEGER NOT NULL,
               json TEXT NOT NULL, PRIMARY KEY(path, artifact_id));
             CREATE TABLE IF NOT EXISTS checkpoint_message_ids (
               path TEXT NOT NULL REFERENCES page_sessions(path) ON DELETE CASCADE,
               message_id TEXT NOT NULL, ordinal INTEGER NOT NULL, PRIMARY KEY(path,message_id));
             CREATE TABLE IF NOT EXISTS catalog_metadata(path TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,header TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS page_recent_artifacts
               ON page_artifacts(path, linked, timestamp DESC, artifact_id);",
            )
            .map_err(db_error)?;
        databases.insert(db.to_path_buf(), Arc::new(Mutex::new(connection)));
    }
    let database = Arc::clone(databases.get(db).expect("database inserted"));
    drop(databases);
    let mut connection = database
        .lock()
        .map_err(|_| db_error("connection mutex poisoned"))?;
    action(&mut connection)
}

pub(super) fn fingerprint(path: &Path) -> Result<String, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("Could not stat {}: {e}; original preserved", path.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "{} is not a session file; original preserved",
            path.display()
        ));
    }
    let modified = metadata.modified().map_err(db_error)?;
    let time = match modified.duration_since(UNIX_EPOCH) {
        Ok(value) => format!("{}:{}", value.as_secs(), value.subsec_nanos()),
        Err(error) => format!(
            "-{}:{}",
            error.duration().as_secs(),
            error.duration().subsec_nanos()
        ),
    };
    Ok(format!("{}:{time}", metadata.len()))
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        return Err("Invalid session ID; original preserved".into());
    }
    Ok(())
}

fn validate_identity(path: &Path, id: &str, expected: &str) -> Result<(), String> {
    validate_id(id)?;
    validate_id(expected)?;
    if id != expected {
        return Err(format!(
            "{}: session ID does not match filename; original preserved",
            path.display()
        ));
    }
    Ok(())
}

fn references(messages: &[ChatMessage]) -> BTreeSet<&str> {
    let mut ids = BTreeSet::new();
    for message in messages {
        for call in message.tool_calls.iter().flatten().chain(
            message
                .segments
                .iter()
                .flatten()
                .flat_map(|s| s.tool_calls.iter().flatten()),
        ) {
            if let Some(id) = call.artifact_id.as_deref() {
                ids.insert(id);
            }
        }
    }
    ids
}

// Header serialization intentionally borrows only metadata, never clones messages
// (which may contain large image inputs/provider transcripts) or artifacts.
#[derive(serde::Serialize)]
struct Header<'a> {
    session: &'a Session,
    #[serde(rename = "modelOverride")]
    model_override: &'a Option<super::types::SessionModelOverride>,
}

fn replace_projection(
    conn: &mut Connection,
    key: &str,
    stamp: &str,
    data: &SessionData,
) -> Result<(), String> {
    let mut session = data.session.clone();
    session.message_count = data.messages.len();
    let header = serde_json::to_string(&Header {
        session: &session,
        model_override: &data.model_override,
    })
    .map_err(db_error)?;
    let revision = uuid::Uuid::new_v4().to_string();
    let total = i64::try_from(data.messages.len()).map_err(db_error)?;
    let transaction = conn.transaction().map_err(db_error)?;
    transaction.execute(
        "INSERT INTO page_sessions(path,session_id,fingerprint,revision,total,header) VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(path) DO UPDATE SET session_id=excluded.session_id,fingerprint=excluded.fingerprint,
           revision=excluded.revision,total=excluded.total,header=excluded.header",
        params![key, data.session.id, stamp, revision, total, header]
    ).map_err(db_error)?;
    transaction
        .execute(
            "DELETE FROM page_messages WHERE path=?1 AND ordinal>=?2",
            params![key, total],
        )
        .map_err(db_error)?;
    // Remove only obsolete artifact IDs. Bound SQL parameters independently of
    // library size, and borrow canonical IDs without cloning artifact payloads.
    let ids: BTreeSet<_> = data.artifacts.iter().map(|a| a.id.as_str()).collect();
    let stale: Vec<String> = {
        let mut query = transaction
            .prepare("SELECT artifact_id FROM page_artifacts WHERE path=?1")
            .map_err(db_error)?;
        let rows = query
            .query_map([key], |r| r.get::<_, String>(0))
            .map_err(db_error)?;
        let mut stale = Vec::new();
        for row in rows {
            let id = row.map_err(db_error)?;
            if !ids.contains(id.as_str()) {
                stale.push(id);
            }
        }
        stale
    };
    for id in stale {
        transaction
            .execute(
                "DELETE FROM page_artifacts WHERE path=?1 AND artifact_id=?2",
                params![key, id],
            )
            .map_err(db_error)?;
    }
    {
        let mut insert = transaction
            .prepare("INSERT INTO page_messages(path,ordinal,json) VALUES (?1,?2,?3) ON CONFLICT(path,ordinal) DO UPDATE SET json=excluded.json WHERE page_messages.json != excluded.json")
            .map_err(db_error)?;
        let mut upsert_id = transaction
            .prepare("INSERT INTO checkpoint_message_ids(path,message_id,ordinal) VALUES (?1,?2,?3) ON CONFLICT(path,message_id) DO UPDATE SET ordinal=excluded.ordinal WHERE checkpoint_message_ids.ordinal != excluded.ordinal")
            .map_err(db_error)?;
        let mut unique_ids = std::collections::HashSet::new();
        for (ordinal, message) in data.messages.iter().enumerate() {
            if message.id.is_empty() || !unique_ids.insert(message.id.as_str()) {
                return Err("Duplicate or empty message ID; original preserved".into());
            }
            upsert_id
                .execute(params![key, message.id, ordinal as i64])
                .map_err(db_error)?;
            insert
                .execute(params![
                    key,
                    ordinal as i64,
                    serde_json::to_string(message).map_err(db_error)?
                ])
                .map_err(db_error)?;
        }
        // Pilih ID usang tanpa daftar parameter SQL yang membesar bersama riwayat.
        let stale: Vec<String> = {
            let mut query = transaction
                .prepare("SELECT message_id FROM checkpoint_message_ids WHERE path=?1")
                .map_err(db_error)?;
            let rows = query
                .query_map([key], |row| row.get::<_, String>(0))
                .map_err(db_error)?;
            let mut stale = Vec::new();
            for row in rows {
                let id = row.map_err(db_error)?;
                if !unique_ids.contains(id.as_str()) {
                    stale.push(id);
                }
            }
            stale
        };
        let mut delete_id = transaction
            .prepare("DELETE FROM checkpoint_message_ids WHERE path=?1 AND message_id=?2")
            .map_err(db_error)?;
        for id in stale {
            delete_id.execute(params![key, id]).map_err(db_error)?;
        }
    }
    {
        let linked = references(&data.messages);
        let mut insert = transaction.prepare("INSERT INTO page_artifacts(path,artifact_id,timestamp,linked,json) VALUES (?1,?2,?3,?4,?5)
            ON CONFLICT(path,artifact_id) DO UPDATE SET timestamp=excluded.timestamp,linked=excluded.linked,json=excluded.json WHERE page_artifacts.json != excluded.json OR page_artifacts.linked != excluded.linked OR page_artifacts.timestamp != excluded.timestamp").map_err(db_error)?;
        for artifact in &data.artifacts {
            insert
                .execute(params![
                    key,
                    artifact.id,
                    artifact.timestamp,
                    linked.contains(artifact.id.as_str()),
                    serde_json::to_string(artifact).map_err(db_error)?
                ])
                .map_err(db_error)?;
        }
    }
    transaction.commit().map_err(db_error)
}

struct CachedHeader {
    revision: String,
    total: usize,
    header: String,
}

fn ensure_fresh(conn: &mut Connection, path: &Path, id: &str) -> Result<CachedHeader, String> {
    validate_id(id)?;
    let key = path_key(path)?;
    let stamp = fingerprint(path)?;
    let cached = conn
        .query_row(
            "SELECT session_id,fingerprint,revision,total,header FROM page_sessions WHERE path=?1",
            [&key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .map_err(db_error)?;
    if let Some((stored_id, stored_stamp, revision, total, header)) = cached {
        if stored_stamp == stamp {
            validate_identity(path, &stored_id, id)?;
            return Ok(CachedHeader {
                revision,
                total: usize::try_from(total).map_err(db_error)?,
                header,
            });
        }
    }
    // A second stat prevents associating a projection with a different atomic
    // replacement than the bytes parsed. Constant external churn fails visibly.
    let bytes = std::fs::read(path)
        .map_err(|e| format!("Could not read {}: {e}; original preserved", path.display()))?;
    let value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("{}: invalid JSON; original preserved: {e}", path.display()))?;
    let data =
        parse_session(value).map_err(|e| format!("{}: {e}; original preserved", path.display()))?;
    validate_identity(path, &data.session.id, id)?;
    if fingerprint(path)? != stamp {
        return Err(format!(
            "{} changed during paging; retry the read; original preserved",
            path.display()
        ));
    }
    replace_projection(conn, &key, &stamp, &data)?;
    conn.query_row(
        "SELECT revision,total,header FROM page_sessions WHERE path=?1",
        [&key],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )
    .map_err(db_error)
    .and_then(|(revision, total, header)| {
        Ok(CachedHeader {
            revision,
            total: usize::try_from(total).map_err(db_error)?,
            header,
        })
    })
}

/// Refresh the projection after the canonical atomic write, under storage's lock.
pub fn cache_session(path: &Path, data: &SessionData) -> Result<(), String> {
    cache_session_at(&db_path(), path, data)
}

fn cache_session_at(db: &Path, path: &Path, data: &SessionData) -> Result<(), String> {
    validate_identity(path, &data.session.id, &data.session.id)?;
    with_db(db, |conn| {
        replace_projection(conn, &path_key(path)?, &fingerprint(path)?, data)
    })
}

/// `before` is an exclusive ordinal boundary. Returned rows are chronological;
/// use window.offset as the next boundary, with the persisted revision token.
pub fn load_page(
    path: &Path,
    session_id: &str,
    before: Option<usize>,
    limit: usize,
    revision: Option<&str>,
) -> Result<SessionPage, String> {
    load_page_at(&db_path(), path, session_id, before, limit, revision)
}

fn load_page_at(
    db: &Path,
    path: &Path,
    session_id: &str,
    before: Option<usize>,
    limit: usize,
    revision: Option<&str>,
) -> Result<SessionPage, String> {
    with_db(db, |conn| {
        let cached = ensure_fresh(conn, path, session_id)?;
        if revision.is_some_and(|r| r != cached.revision) {
            return Err("Session page cursor is stale; reload the latest page".into());
        }
        let end = before.unwrap_or(cached.total);
        if end > cached.total {
            return Err("Session page boundary exceeds message total".into());
        }
        let limit = limit.clamp(1, 100);
        let offset = end.saturating_sub(limit);
        read_window(conn, path, cached, offset, end)
    })
}

/// Indexed exact-ID lookup; only the selected window is decoded on warm reads.
pub fn load_around(
    path: &Path,
    session_id: &str,
    message_id: &str,
    limit: usize,
) -> Result<SessionPage, String> {
    load_around_at(&db_path(), path, session_id, message_id, limit)
}

fn load_around_at(
    db: &Path,
    path: &Path,
    session_id: &str,
    message_id: &str,
    limit: usize,
) -> Result<SessionPage, String> {
    if message_id.is_empty() || message_id.len() > 1024 {
        return Err("Invalid message ID".into());
    }
    with_db(db, |conn| {
        let cached = ensure_fresh(conn, path, session_id)?;
        let key = path_key(path)?;
        ensure_checkpoint_message_ids(conn, &key, cached.total)?;
        let ordinal: usize = conn
            .query_row(
                "SELECT ordinal FROM checkpoint_message_ids WHERE path=?1 AND message_id=?2",
                params![key, message_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| "Message was not found; refresh search results".to_string())?;
        let limit = limit.clamp(1, 100);
        let offset = ordinal
            .saturating_sub(limit / 2)
            .min(cached.total.saturating_sub(limit));
        let end = (offset + limit).min(cached.total);
        let mut page = read_window(conn, path, cached, offset, end)?;
        page.window.end = Some(end);
        Ok(page)
    })
}

/// Thin only viewport messages; canonical and indexed replay payloads stay intact.
pub(super) fn thin_message(message: &mut ChatMessage) {
    // Project only the returned view; SQLite retains canonical replay bytes.
    message.has_image_inputs = message
        .image_inputs
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|_| true);
    message.has_provider_messages = message
        .provider_messages
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|_| true);
    message.image_inputs = None;
    message.provider_messages = None;
}

fn read_window(
    conn: &Connection,
    path: &Path,
    cached: CachedHeader,
    offset: usize,
    end: usize,
) -> Result<SessionPage, String> {
    let key = path_key(path)?;
    let limit = end - offset;
    let mut data: SessionData = serde_json::from_str(&cached.header).map_err(db_error)?;
    data.session.message_count = cached.total;
    {
        let mut query = conn.prepare("SELECT json FROM page_messages WHERE path=?1 AND ordinal>=?2 AND ordinal<?3 ORDER BY ordinal ASC LIMIT ?4").map_err(db_error)?;
        let rows = query
            .query_map(
                params![key, offset as i64, end as i64, limit as i64],
                |row| row.get::<_, String>(0),
            )
            .map_err(db_error)?;
        for row in rows {
            let mut message: ChatMessage =
                serde_json::from_str(&row.map_err(db_error)?).map_err(db_error)?;
            thin_message(&mut message);
            data.messages.push(message);
        }
    }
    if data.messages.len() != end - offset {
        return Err(db_error("missing selected message rows"));
    }
    // At most 80 selected-message references plus 20 recent globally-unlinked
    // artifacts. Linked artifacts elsewhere in history are fetched lazily.
    let ids: Vec<String> = references(&data.messages)
        .into_iter()
        .take(80)
        .map(str::to_owned)
        .collect();
    if !ids.is_empty() {
        let mut art_stmt = conn
            .prepare_cached("SELECT json FROM page_artifacts WHERE path=?1 AND artifact_id=?2")
            .map_err(db_error)?;
        for id in ids {
            if let Some(json) = art_stmt
                .query_row(params![&key, id], |r| r.get::<_, String>(0))
                .optional()
                .map_err(db_error)?
            {
                data.artifacts
                    .push(serde_json::from_str(&json).map_err(db_error)?);
            }
        }
    }
    let mut query = conn.prepare("SELECT json FROM page_artifacts WHERE path=?1 AND linked=0 ORDER BY timestamp DESC,artifact_id ASC LIMIT 20").map_err(db_error)?;
    let rows = query
        .query_map([&key], |r| r.get::<_, String>(0))
        .map_err(db_error)?;
    for row in rows {
        data.artifacts
            .push(serde_json::from_str(&row.map_err(db_error)?).map_err(db_error)?);
    }
    let first_message_id = data.messages.first().map(|m| m.id.clone());
    Ok(SessionPage {
        data,
        window: SessionWindow {
            end: None,
            offset,
            total: cached.total,
            revision: cached.revision,
            first_message_id,
        },
    })
}

pub fn read_message_image_inputs(
    path: &Path,
    session_id: &str,
    message_id: &str,
) -> Result<Vec<super::types::ImageInput>, String> {
    read_message_image_inputs_at(&db_path(), path, session_id, message_id)
}

fn read_message_image_inputs_at(
    db: &Path,
    path: &Path,
    session_id: &str,
    message_id: &str,
) -> Result<Vec<super::types::ImageInput>, String> {
    if message_id.is_empty() || message_id.len() > 1024 {
        return Err("Invalid message ID".into());
    }
    with_db(db, |conn| {
        let cached = ensure_fresh(conn, path, session_id)?;
        let key = path_key(path)?;
        ensure_checkpoint_message_ids(conn, &key, cached.total)?;
        let json: String = conn.query_row(
            "SELECT m.json FROM checkpoint_message_ids i JOIN page_messages m ON m.path=i.path AND m.ordinal=i.ordinal WHERE i.path=?1 AND i.message_id=?2",
            params![key, message_id], |row| row.get(0),
        ).optional().map_err(db_error)?.ok_or_else(|| format!("Message {message_id} was not found in session {session_id}"))?;
        let message: ChatMessage = serde_json::from_str(&json).map_err(db_error)?;
        Ok(message.image_inputs.unwrap_or_default())
    })
}

pub fn read_artifact(path: &Path, session_id: &str, artifact_id: &str) -> Result<Artifact, String> {
    read_artifact_at(&db_path(), path, session_id, artifact_id)
}

fn read_artifact_at(
    db: &Path,
    path: &Path,
    session_id: &str,
    artifact_id: &str,
) -> Result<Artifact, String> {
    with_db(db, |conn| {
        ensure_fresh(conn, path, session_id)?;
        let json = conn
            .query_row(
                "SELECT json FROM page_artifacts WHERE path=?1 AND artifact_id=?2",
                params![path_key(path)?, artifact_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| {
                format!("Artifact {artifact_id} was not found in session {session_id}")
            })?;
        serde_json::from_str(&json).map_err(db_error)
    })
}

// A metadata scan still reads every canonical JSON byte on a cold/stale file,
// but IgnoredAny skips messages/artifacts without constructing their object graphs.
struct MessageCount(usize);
impl<'de> serde::Deserialize<'de> for MessageCount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Count;
        impl<'de> serde::de::Visitor<'de> for Count {
            type Value = MessageCount;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a message array")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut count = 0;
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    count += 1;
                }
                Ok(MessageCount(count))
            }
        }
        deserializer.deserialize_seq(Count)
    }
}
struct Metadata(Session);
impl<'de> serde::Deserialize<'de> for Metadata {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Scan;
        impl<'de> serde::de::Visitor<'de> for Scan {
            type Value = Metadata;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a session object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut session: Option<Session> = None;
                let mut count = 0;
                let mut legacy = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "session" => {
                            if session.is_some() {
                                return Err(serde::de::Error::custom("duplicate session"));
                            }
                            session = Some(map.next_value()?);
                        }
                        "messages" => count = map.next_value::<MessageCount>()?.0,
                        "id" | "title" | "createdAt" | "updatedAt" | "workspaceCwd" | "preview"
                        | "pinned" | "pinnedAt" => {
                            legacy.insert(key, map.next_value()?);
                        }
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                let mut session = match session {
                    Some(session) => session,
                    None => {
                        parse_session(serde_json::Value::Object(legacy))
                            .map_err(serde::de::Error::custom)?
                            .session
                    }
                };
                session.message_count = count;
                Ok(Metadata(session))
            }
        }
        deserializer.deserialize_map(Scan)
    }
}

fn metadata_at(db: &Path, path: &Path, id: &str) -> Result<Session, String> {
    validate_id(id)?;
    let stamp = fingerprint(path)?;
    let key = path_key(path)?;
    let cached: Option<String> = with_db(db, |conn| {
        let indexed = conn
            .query_row(
                "SELECT header FROM page_sessions WHERE path=?1 AND fingerprint=?2",
                params![key, stamp],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?;
        if indexed.is_some() {
            return Ok(indexed);
        }
        conn.query_row(
            "SELECT header FROM catalog_metadata WHERE path=?1 AND fingerprint=?2",
            params![key, stamp],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)
    })?;
    if let Some(header) = cached {
        #[derive(serde::Deserialize)]
        struct Summary {
            session: Session,
        }
        let summary: Summary = serde_json::from_str(&header).map_err(db_error)?;
        validate_identity(path, &summary.session.id, id)?;
        return Ok(summary.session);
    }
    let file = std::fs::File::open(path).map_err(db_error)?;
    let mut decoder = serde_json::Deserializer::from_reader(std::io::BufReader::new(file));
    let Metadata(session) =
        <Metadata as serde::Deserialize>::deserialize(&mut decoder).map_err(|e| {
            format!(
                "{}: invalid session metadata; original preserved: {e}",
                path.display()
            )
        })?;
    decoder
        .end()
        .map_err(|e| format!("{}: invalid JSON; original preserved: {e}", path.display()))?;
    validate_identity(path, &session.id, id)?;
    if fingerprint(path)? != stamp {
        return Err("Session changed during metadata scan; retry; original preserved".into());
    }
    let header =
        serde_json::to_string(&serde_json::json!({"session":session})).map_err(db_error)?;
    with_db(db, |conn| {
        conn.execute("INSERT INTO catalog_metadata(path,fingerprint,header) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET fingerprint=excluded.fingerprint,header=excluded.header",params![key,stamp,header]).map_err(db_error)?;
        Ok(())
    })?;
    Ok(session)
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmIndexResult {
    pub processed: usize,
    pub remaining: usize,
    pub errors: usize,
}

pub fn warm_index(root: &Path, limit: usize, budget_ms: u64) -> Result<WarmIndexResult, String> {
    warm_index_at(&db_path(), root, limit, budget_ms)
}
fn warm_index_at(
    db: &Path,
    root: &Path,
    limit: usize,
    budget_ms: u64,
) -> Result<WarmIndexResult, String> {
    let started = std::time::Instant::now();
    warm_index_with_elapsed(db, root, limit, budget_ms, || started.elapsed())
}

// The real worker uses wall time. Tests control only elapsed time so parallel
// scheduling/SQLite initialization cannot turn semantic assertions into skips.
fn warm_index_with_elapsed(
    db: &Path,
    root: &Path,
    limit: usize,
    budget_ms: u64,
    elapsed: impl Fn() -> Duration,
) -> Result<WarmIndexResult, String> {
    let mut result = WarmIndexResult::default();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(e) => return Err(db_error(e)),
    };
    for entry in entries {
        let path = match entry {
            Ok(e) => e.path(),
            Err(_) => {
                result.errors += 1;
                continue;
            }
        };
        if path.extension().and_then(|s| s.to_str()) != Some("json")
            || path.with_extension("deleted").exists()
        {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let pending = (|| {
            let stamp = fingerprint(&path)?;
            with_db(db, |conn| {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM page_sessions WHERE path=?1 AND fingerprint=?2)",
                    params![path_key(&path)?, stamp],
                    |r| r.get::<_, bool>(0),
                )
                .map(|fresh| !fresh)
                .map_err(db_error)
            })
        })();
        match pending {
            Ok(false) => continue,
            Err(_) => {
                result.errors += 1;
                continue;
            }
            Ok(true) => {}
        }
        if result.processed + result.errors >= limit.min(3)
            || elapsed() >= Duration::from_millis(budget_ms.min(75))
        {
            result.remaining += 1;
            continue;
        }
        let Ok(_guard) = super::storage::session_io_lock(id).try_lock_owned() else {
            result.remaining += 1;
            continue;
        };
        if path.with_extension("deleted").exists() {
            continue;
        }
        match with_db(db, |conn| ensure_fresh(conn, &path, id)) {
            Ok(_) => result.processed += 1,
            Err(_) => result.errors += 1,
        }
    }
    Ok(result)
}

/// Warm reads decode cached headers; cold reads stream metadata without indexing.
/// Syntax/metadata corruption is reported; message semantic validation is deferred
/// until a page read or idle index build. Canonical files are never modified here.
/// Warm catalog reads decode headers only, never message or artifact rows.
/// All unreadable entries are reported together rather than silently omitted.
pub fn list_catalog(root: &Path) -> Result<Vec<Session>, String> {
    list_catalog_at(&db_path(), root)
}

fn list_catalog_at(db: &Path, root: &Path) -> Result<Vec<Session>, String> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Could not list {}: {e}", root.display())),
    };
    let mut sessions = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(e) => {
                diagnostics.push(format!("{}: {e}", root.display()));
                continue;
            }
        };
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        // Same lock order as writers: session first, then SQLite. Release
        // both after each file so a cold catalog never locks the library.
        let _guard = super::storage::session_io_lock(id).blocking_lock_owned();
        if path
            .with_extension("deleted")
            .try_exists()
            .map_err(db_error)?
        {
            continue;
        }
        match metadata_at(db, &path, id) {
            Ok(session) => sessions.push(session),
            Err(e) => diagnostics.push(format!("{}: {e}", path.display())),
        }
    }
    if !diagnostics.is_empty() {
        return Err(format!(
            "Session catalog has unreadable entries ({} valid sessions preserved):\n{}",
            sessions.len(),
            diagnostics.join("\n")
        ));
    }
    sessions.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| {
                let a_time = if a.pinned {
                    a.pinned_at.unwrap_or(a.updated_at)
                } else {
                    a.updated_at
                };
                let b_time = if b.pinned {
                    b.pinned_at.unwrap_or(b.updated_at)
                } else {
                    b.updated_at
                };
                b_time.cmp(&a_time)
            })
            .then_with(|| b.created_at.cmp(&a.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(sessions)
}

/// Optional eager invalidation. Reads always stat canonical JSON, so a deleted
/// file is rejected even if this function was not called or fails.
pub fn invalidate(path: &Path) -> Result<(), String> {
    invalidate_at(&db_path(), path)
}

fn ensure_checkpoint_message_ids(conn: &Connection, key: &str, total: usize) -> Result<(), String> {
    if total == 0 {
        return Ok(());
    }
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM checkpoint_message_ids WHERE path=?1",
            [key],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if count == 0 {
        // Migration reads IDs only inside SQLite once, not canonical JSON.
        conn.execute(
            "INSERT OR REPLACE INTO checkpoint_message_ids SELECT path,json_extract(json,'$.id'),ordinal FROM page_messages WHERE path=?1",
            [key],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

fn invalidate_at(db: &Path, path: &Path) -> Result<(), String> {
    let key = path_key(path)?;
    with_db(db, |conn| {
        let tx = conn.transaction().map_err(db_error)?;
        tx.execute("DELETE FROM checkpoint_message_ids WHERE path=?1", [&key])
            .map_err(db_error)?;
        tx.execute("DELETE FROM page_sessions WHERE path=?1", [&key])
            .map_err(db_error)?;
        tx.execute("DELETE FROM catalog_metadata WHERE path=?1", [&key])
            .map_err(db_error)?;
        tx.commit().map_err(db_error)
    })
}

// Exact-ID bounded row decoding for the first checkpoint; no historical image
// rows are selected. Old projections lazily populate the new identity index.
pub(super) fn checkpoint_baseline(
    path: &Path,
    id: &str,
    ids: &[&str],
) -> Result<(String, Vec<ChatMessage>), String> {
    with_db(&db_path(), |conn| {
        let header = ensure_fresh(conn, path, id)?;
        let key = path_key(path)?;
        ensure_checkpoint_message_ids(conn, &key, header.total)?;
        let mut messages = Vec::new();
        for id in ids {
            let json: String=conn.query_row("SELECT m.json FROM checkpoint_message_ids i JOIN page_messages m ON m.path=i.path AND m.ordinal=i.ordinal WHERE i.path=?1 AND i.message_id=?2",params![key,id],|r|r.get(0)).map_err(db_error)?;
            messages.push(serde_json::from_str(&json).map_err(db_error)?);
        }
        Ok((header.revision, messages))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn lightweight_pages_strip_multimegabyte_payloads_but_exact_reads_retain_bytes() {
        let f = Fixture::new(2);
        let mut canonical = f.data(2);
        let bytes = "QUJD".repeat(524288); // 2 MiB image payload
        canonical.messages[0].image_inputs = Some(vec![super::super::types::ImageInput {
            source_path: "deleted.png".into(),
            media_type: "image/png".into(),
            data: bytes.clone(),
        }]);
        canonical.messages[1].provider_messages = Some(vec![json!({"signature":bytes})]);
        f.write(&canonical);
        for page in [
            f.page(None, 2, None).unwrap(),
            load_around_at(&f.db, &f.path, &f.id, "m0", 2).unwrap(),
        ] {
            assert!(serde_json::to_vec(&page).unwrap().len() < 10_000);
            assert!(page
                .data
                .messages
                .iter()
                .all(|m| m.image_inputs.is_none() && m.provider_messages.is_none()));
            assert_eq!(page.data.messages[0].has_image_inputs, Some(true));
            assert_eq!(page.data.messages[1].has_provider_messages, Some(true));
        }
        let hydrated = read_message_image_inputs_at(&f.db, &f.path, &f.id, "m0").unwrap();
        assert_eq!(hydrated[0].data, bytes);
        assert!(read_message_image_inputs_at(&f.db, &f.path, &f.id, "missing").is_err());
        assert!(read_message_image_inputs_at(&f.db, &f.path, "foreign", "m0").is_err());
        let full = parse_session(serde_json::from_slice(&std::fs::read(&f.path).unwrap()).unwrap())
            .unwrap();
        assert_eq!(
            full.messages[0].image_inputs.as_ref().unwrap()[0].data,
            hydrated[0].data
        );
        assert_eq!(
            full.messages[1].provider_messages,
            canonical.messages[1].provider_messages
        );
    }

    #[test]
    fn around_exact_bounded_warm_read_unknown_id_and_cold_corrupt() {
        let f = Fixture::new(250);
        let first = load_around_at(&f.db, &f.path, &f.id, "m120", 10).unwrap();
        assert_eq!(
            (
                first.window.offset,
                first.window.end,
                first.data.messages.len()
            ),
            (115, Some(125), 10)
        );
        assert_eq!(
            first
                .data
                .messages
                .iter()
                .map(|m| m.id.clone())
                .collect::<Vec<_>>(),
            (115..125).map(|n| format!("m{n}")).collect::<Vec<_>>()
        );
        let conn = Connection::open(&f.db).unwrap();
        conn.execute(
            "UPDATE page_messages SET json='{broken' WHERE ordinal=0",
            [],
        )
        .unwrap();
        conn.execute_batch("CREATE TRIGGER around_no_update BEFORE UPDATE ON page_sessions BEGIN SELECT RAISE(ABORT,'warm write'); END;").unwrap();
        let warm = load_around_at(&f.db, &f.path, &f.id, "m120", 10).unwrap();
        assert_eq!(first.window.revision, warm.window.revision);
        assert!(load_around_at(&f.db, &f.path, &f.id, "unknown", 10)
            .unwrap_err()
            .contains("not found"));
        assert!(load_around_at(&f.db, &f.path, &f.id, "m0", 1).is_err());
        assert_eq!(
            load_around_at(&f.db, &f.path, &f.id, "m249", 999)
                .unwrap()
                .data
                .messages
                .len(),
            100
        );
        let corrupt = Fixture::new(2);
        std::fs::write(&corrupt.path, "{broken").unwrap();
        let before = std::fs::read(&corrupt.path).unwrap();
        assert!(load_around_at(&corrupt.db, &corrupt.path, &corrupt.id, "m0", 10).is_err());
        assert_eq!(before, std::fs::read(&corrupt.path).unwrap());
    }
    #[test]
    fn around_duplicate_ids_are_rejected_instead_of_jumping_to_an_arbitrary_hit() {
        let f = Fixture::new(4);
        let mut data = f.data(4);
        data.messages[3].id = data.messages[0].id.clone();
        f.write(&data);
        let before = std::fs::read(&f.path).unwrap();
        assert!(load_around_at(&f.db, &f.path, &f.id, "m0", 1).is_err());
        assert_eq!(before, std::fs::read(&f.path).unwrap());
    }

    #[test]
    fn contention_projection_duplicate_or_empty_ids_roll_back_mapping_and_rows() {
        let f = Fixture::new(4);
        let original = f.data(4);
        f.page(None, 4, None).unwrap();
        let conn = Connection::open(&f.db).unwrap();
        let snapshot = || {
            [
                "SELECT json_array(path,session_id,fingerprint,revision,total,header) FROM page_sessions ORDER BY path",
                "SELECT json_array(path,ordinal,json) FROM page_messages ORDER BY path,ordinal",
                "SELECT json_array(path,message_id,ordinal) FROM checkpoint_message_ids ORDER BY path,message_id",
            ].map(|sql| {
                conn.prepare(sql)
                    .unwrap()
                    .query_map([], |row| row.get::<_, String>(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            })
        };
        let before = snapshot();
        for duplicate in [true, false] {
            let mut bad = f.data(3);
            bad.session.title = "must roll back".into();
            bad.messages[0].id = "new-id".into();
            bad.messages[0].content = "must roll back".into();
            bad.messages[1].id = if duplicate {
                "new-id".into()
            } else {
                String::new()
            };
            f.write(&bad);
            let error = cache_session_at(&f.db, &f.path, &bad).unwrap_err();
            assert!(error.contains("Duplicate or empty message ID"), "{error}");
            assert_eq!(snapshot(), before);
            assert_eq!(
                std::fs::read(&f.path).unwrap(),
                serde_json::to_vec(&bad).unwrap()
            );
        }
        f.write(&original);
        for ordinal in 0..4 {
            let id = format!("m{ordinal}");
            let page = load_around_at(&f.db, &f.path, &f.id, &id, 1).unwrap();
            assert_eq!(page.window.offset, ordinal);
            assert_eq!(page.data.messages[0].id, id);
        }
    }

    #[test]
    fn idle_metadata_defers_message_semantics_but_detects_syntax_and_legacy_counts() {
        let f = Fixture::new(2);
        let mut raw = serde_json::to_value(f.data(2)).unwrap();
        raw["messages"] = json!([{"invalid":"not a ChatMessage"}]);
        std::fs::write(&f.path, serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_eq!(list_catalog_at(&f.db, &f.root).unwrap()[0].message_count, 1);
        assert!(f
            .page(None, 2, None)
            .unwrap_err()
            .contains("schema is invalid"));
        let warm = warm_fixture_index(&f.db, &f.root, 3, 75).unwrap();
        assert_eq!((warm.processed, warm.errors), (0, 1));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&f.path).unwrap()).unwrap(),
            raw
        );
        std::fs::write(
            &f.path,
            serde_json::to_vec(&json!({"id":f.id,"title":"Legacy","messages":[1,2,3]})).unwrap(),
        )
        .unwrap();
        assert_eq!(list_catalog_at(&f.db, &f.root).unwrap()[0].message_count, 3);
        let cached = list_catalog_at(&f.db, &f.root).unwrap();
        assert_eq!(
            (
                &cached[0].id,
                cached[0].title.as_str(),
                cached[0].message_count
            ),
            (&f.id, "Legacy", 3)
        );
        std::fs::write(&f.path, format!(r#"{{"id":"{}","messages":[1,2,]}}"#, f.id)).unwrap();
        assert!(list_catalog_at(&f.db, &f.root)
            .unwrap_err()
            .contains("original preserved"));
    }
    #[test]
    fn idle_worker_limit_and_zero_budget_leave_unindexed_files_pending() {
        let f = Fixture::new(3);
        for id in [
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
        ] {
            let mut data = f.data(3);
            data.session.id = id.clone();
            std::fs::write(
                f.root.join(format!("{id}.json")),
                serde_json::to_vec(&data).unwrap(),
            )
            .unwrap();
        }
        let zero = super::warm_index_at(&f.db, &f.root, 3, 0).unwrap();
        assert_eq!((zero.processed, zero.remaining), (0, 3));
        let one = warm_fixture_index(&f.db, &f.root, 1, 75).unwrap();
        assert_eq!((one.processed, one.remaining), (1, 2));
        let rest = warm_fixture_index(&f.db, &f.root, 3, 75).unwrap();
        assert_eq!((rest.processed, rest.remaining), (2, 0));
    }

    #[test]
    fn idle_warm_worker_is_incremental_and_skips_locked_sessions() {
        let f = Fixture::new(12);
        let guard = super::super::storage::session_io_lock(&f.id).blocking_lock_owned();
        let skipped = warm_fixture_index(&f.db, &f.root, 3, 75).unwrap();
        assert_eq!((skipped.processed, skipped.remaining), (0, 1));
        drop(guard);
        let warmed = warm_fixture_index(&f.db, &f.root, 1, 75).unwrap();
        assert_eq!(
            (warmed.processed, warmed.remaining, warmed.errors),
            (1, 0, 0)
        );
        assert_eq!(
            warm_fixture_index(&f.db, &f.root, 3, 75).unwrap().processed,
            0
        );
    }

    #[test]
    fn idle_metadata_catalog_does_not_build_message_projection() {
        let f = Fixture::new(300);
        let catalog = list_catalog_at(&f.db, &f.root).unwrap();
        assert_eq!(catalog[0].message_count, 300);
        let conn = Connection::open(&f.db).unwrap();
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM page_messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "cold listing must not rebuild message rows");
    }

    #[test]
    fn contention_database_action_releases_registry_and_allows_another_database() {
        let first = Fixture::new(1);
        let second = Fixture::new(1);
        with_db(&first.db, |_conn| {
            let first_db = first.db.clone();
            let second_db = second.db.clone();
            let (ready, received) = std::sync::mpsc::channel();
            let probe = std::thread::spawn(move || {
                // A blocking registry acquisition tolerates unrelated parallel
                // readers, but cannot succeed if this action still owns it.
                let first_connection = DATABASES
                    .get()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .get(&first_db)
                    .cloned()
                    .unwrap();
                // Probe the exact connection mutex while the first action is
                // waiting, not an elapsed-time guess about SQL concurrency.
                let first_is_locked = matches!(
                    first_connection.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                );
                let answer = with_db(&second_db, |conn| {
                    conn.query_row("SELECT 42", [], |r| r.get::<_, i64>(0))
                        .map_err(db_error)
                });
                let _ = ready.send((first_is_locked, answer));
            });
            let (first_is_locked, answer) = received
                .recv_timeout(Duration::from_secs(5))
                .expect("registry probe and independent database must complete while the first action is locked");
            probe.join().unwrap();
            assert!(first_is_locked, "probe must overlap the first SQL action");
            assert_eq!(answer?, 42);
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn contention_catalog_ignores_tombstoned_json_left_after_failed_removal() {
        let f = Fixture::new(300);
        f.page(None, 1, None).unwrap();
        std::fs::write(f.path.with_extension("deleted"), "deleted").unwrap();
        std::fs::write(&f.path, "{unreadable leftover").unwrap();
        assert!(list_catalog_at(&f.db, &f.root).unwrap().is_empty());
    }
    #[test]
    fn contention_projection_streaming_updates_only_changed_rows_and_prunes_stale_rows() {
        let f = Fixture::new(300);
        let mut data = f.data(300);
        data.messages[1].image_inputs = Some(vec![super::super::types::ImageInput {
            source_path: "lookup.png".into(),
            media_type: "image/png".into(),
            data: "bG9va3Vw".into(),
        }]);
        data.artifacts = (0..300).map(|n| serde_json::from_value(json!({"id":format!("a{n}"),"title":"output","type":"code","content":"same","timestamp":n})).unwrap()).collect();
        f.write(&data);
        cache_session_at(&f.db, &f.path, &data).unwrap();
        let conn = Connection::open(&f.db).unwrap();
        conn.execute_batch("CREATE TABLE mutations(kind TEXT);
            CREATE TRIGGER mi AFTER INSERT ON page_messages BEGIN INSERT INTO mutations VALUES('mi'); END;
            CREATE TRIGGER mu AFTER UPDATE ON page_messages BEGIN INSERT INTO mutations VALUES('mu'); END;
            CREATE TRIGGER md AFTER DELETE ON page_messages BEGIN INSERT INTO mutations VALUES('md'); END;
            CREATE TRIGGER ai AFTER INSERT ON page_artifacts BEGIN INSERT INTO mutations VALUES('ai'); END;
            CREATE TRIGGER au AFTER UPDATE ON page_artifacts BEGIN INSERT INTO mutations VALUES('au'); END;
            CREATE TRIGGER ad AFTER DELETE ON page_artifacts BEGIN INSERT INTO mutations VALUES('ad'); END;
            CREATE TRIGGER ii AFTER INSERT ON checkpoint_message_ids BEGIN INSERT INTO mutations VALUES('ii'); END;
            CREATE TRIGGER iu AFTER UPDATE ON checkpoint_message_ids BEGIN INSERT INTO mutations VALUES('iu'); END;
            CREATE TRIGGER id_delete AFTER DELETE ON checkpoint_message_ids BEGIN INSERT INTO mutations VALUES('id'); END;").unwrap();
        let old = f.page(None, 1, None).unwrap().window.revision;
        data.messages[299].content = "streaming token".into();
        f.write(&data);
        cache_session_at(&f.db, &f.path, &data).unwrap();
        let count = |kind: &str| -> i64 {
            conn.query_row(
                "SELECT count(*) FROM mutations WHERE kind=?1",
                [kind],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(
            (
                count("mi"),
                count("mu"),
                count("md"),
                count("ai"),
                count("au"),
                count("ad")
            ),
            (0, 1, 0, 0, 0, 0)
        );
        assert_eq!(
            (count("ii"), count("iu"), count("id")),
            (0, 0, 0),
            "content-only updates must not mutate the message-ID mapping"
        );
        assert_ne!(old, f.page(None, 1, None).unwrap().window.revision);
        data.messages.truncate(298);
        data.artifacts.remove(150);
        data.artifacts[0].content = "changed".into();
        data.messages[0].tool_calls = Some(vec![serde_json::from_value(
            json!({"id":"call","artifactId":"a1"}),
        )
        .unwrap()]);
        f.write(&data);
        cache_session_at(&f.db, &f.path, &data).unwrap();
        assert_eq!(
            (
                count("mi"),
                count("mu"),
                count("md"),
                count("ai"),
                count("au"),
                count("ad")
            ),
            (0, 2, 2, 0, 2, 1)
        );
        assert!(read_artifact_at(&f.db, &f.path, &f.id, "a150").is_err());
        assert_eq!(f.page(None, 1, None).unwrap().window.total, 298);
        assert_eq!(
            (count("ii"), count("iu"), count("id")),
            (0, 0, 2),
            "pruning must remove only the two obsolete IDs"
        );
        for id in ["m298", "m299"] {
            assert!(load_around_at(&f.db, &f.path, &f.id, id, 1).is_err());
            assert!(read_message_image_inputs_at(&f.db, &f.path, &f.id, id).is_err());
        }

        data.messages.push(f.data(301).messages.pop().unwrap());
        f.write(&data);
        cache_session_at(&f.db, &f.path, &data).unwrap();
        assert_eq!(
            (count("ii"), count("iu"), count("id")),
            (1, 0, 2),
            "appending must insert exactly one new ID"
        );
        let appended = load_around_at(&f.db, &f.path, &f.id, "m300", 1).unwrap();
        assert_eq!(appended.window.offset, 298);
        assert_eq!(appended.data.messages[0].id, "m300");

        data.messages.swap(1, 297);
        f.write(&data);
        cache_session_at(&f.db, &f.path, &data).unwrap();
        assert_eq!(
            (count("ii"), count("iu"), count("id")),
            (1, 2, 2),
            "reordering must update only the moved ordinals"
        );
        for (id, ordinal) in [("m297", 1), ("m1", 297), ("m300", 298)] {
            let page = load_around_at(&f.db, &f.path, &f.id, id, 1).unwrap();
            assert_eq!(page.window.offset, ordinal);
            assert_eq!(page.data.messages[0].id, id);
        }
        assert_eq!(
            read_message_image_inputs_at(&f.db, &f.path, &f.id, "m1").unwrap()[0].data,
            "bG9va3Vw"
        );
        assert!(read_message_image_inputs_at(&f.db, &f.path, &f.id, "m297")
            .unwrap()
            .is_empty());

        data.messages.remove(298);
        data.messages.remove(1);
        f.write(&data);
        cache_session_at(&f.db, &f.path, &data).unwrap();
        assert_eq!(
            (count("ii"), count("iu"), count("id")),
            (1, 298, 4),
            "pruning reordered IDs must delete only stale IDs and shift surviving ordinals"
        );
        for id in ["m297", "m300"] {
            assert!(load_around_at(&f.db, &f.path, &f.id, id, 1).is_err());
        }
        let moved = load_around_at(&f.db, &f.path, &f.id, "m1", 1).unwrap();
        assert_eq!(moved.window.offset, 296);
        assert_eq!(moved.data.messages[0].id, "m1");
        assert_eq!(
            read_message_image_inputs_at(&f.db, &f.path, &f.id, "m1").unwrap()[0].data,
            "bG9va3Vw"
        );
    }
    use super::*;
    use serde_json::json;

    fn warm_fixture_index(
        db: &Path,
        root: &Path,
        limit: usize,
        budget_ms: u64,
    ) -> Result<WarmIndexResult, String> {
        super::warm_index_with_elapsed(db, root, limit, budget_ms, || Duration::ZERO)
    }

    #[test]
    fn idle_worker_uses_only_supplied_fixture_root_and_database() {
        let first = Fixture::new(2);
        let second = Fixture::new(3);
        assert_ne!(first.id, second.id);
        assert!(!Arc::ptr_eq(
            &super::super::storage::session_io_lock(&first.id),
            &super::super::storage::session_io_lock(&second.id),
        ));
        let held = super::super::storage::session_io_lock(&second.id).blocking_lock_owned();
        let warmed = warm_fixture_index(&first.db, &first.root, 3, 75).unwrap();
        assert_eq!(
            (warmed.processed, warmed.remaining, warmed.errors),
            (1, 0, 0)
        );
        with_db(&first.db, |conn| {
            let id: String = conn
                .query_row("SELECT session_id FROM page_sessions", [], |r| r.get(0))
                .map_err(db_error)?;
            assert_eq!(id, first.id);
            Ok(())
        })
        .unwrap();
        assert!(
            !second.db.exists(),
            "another fixture's DB must remain untouched"
        );
        drop(held);
    }

    #[test]
    fn idle_worker_elapsed_budget_leaves_work_pending_without_projection() {
        let f = Fixture::new(2);
        let expired =
            super::warm_index_with_elapsed(&f.db, &f.root, 3, 75, || Duration::from_millis(75))
                .unwrap();
        assert_eq!(
            (expired.processed, expired.remaining, expired.errors),
            (0, 1, 0)
        );
        with_db(&f.db, |conn| {
            let rows: i64 = conn
                .query_row("SELECT count(*) FROM page_sessions", [], |r| r.get(0))
                .map_err(db_error)?;
            assert_eq!(rows, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn idle_worker_locked_session_returns_without_waiter_or_frame() {
        let f = Fixture::new(2);
        let before = std::fs::read(&f.path).unwrap();
        let lock = super::super::storage::session_io_lock(&f.id);
        let guard = lock.clone().blocking_lock_owned();
        let (send, receive) = std::sync::mpsc::channel();
        let db = f.db.clone();
        let root = f.root.clone();
        let worker = std::thread::spawn(move || {
            send.send(warm_fixture_index(&db, &root, 3, 75)).unwrap();
        });
        // Deadlock watchdog, not a frame-duration/performance assertion: the
        // worker must return while the session remains locked by this thread.
        let skipped = receive
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        assert_eq!(
            (skipped.processed, skipped.remaining, skipped.errors),
            (0, 1, 0)
        );
        assert_eq!(
            Arc::strong_count(&lock),
            2,
            "worker must retain no lock/waiter"
        );
        drop(guard);
        assert!(
            lock.try_lock().is_ok(),
            "worker must not acquire the released lock later"
        );
        assert_eq!(std::fs::read(&f.path).unwrap(), before);
        assert!(
            !f.path.with_extension("checkpoint").exists(),
            "idle maintenance must not write a frame"
        );
        with_db(&f.db, |conn| {
            let rows: i64 = conn
                .query_row("SELECT count(*) FROM page_sessions", [], |r| r.get(0))
                .map_err(db_error)?;
            assert_eq!(rows, 0);
            Ok(())
        })
        .unwrap();
    }

    struct Fixture {
        root: std::path::PathBuf,
        db: std::path::PathBuf,
        path: std::path::PathBuf,
        id: String,
    }
    impl Fixture {
        fn new(count: usize) -> Self {
            let root = std::env::temp_dir().join(format!("aether-pages-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            // Locks are keyed by ID globally, not by path: a UUID root alone
            // does not isolate fixtures that reuse a fixed session ID.
            let id = root.file_name().unwrap().to_str().unwrap().to_owned();
            let fixture = Self {
                db: root.join("pages.sqlite"),
                path: root.join(format!("{id}.json")),
                id,
                root,
            };
            fixture.write(&fixture.data(count));
            fixture
        }
        fn data(&self, count: usize) -> SessionData {
            serde_json::from_value(json!({
                "session":{"id":self.id,"title":"History","createdAt":1,"updatedAt":2,"messageCount":999},
                "messages":(0..count).map(|n| json!({"id":format!("m{n}"),"role":"assistant","content":format!("text {n}"),"timestamp":n})).collect::<Vec<_>>(),
                "modelOverride":{"providerId":"provider","model":"chosen","thinking":true}
            })).unwrap()
        }
        fn write(&self, data: &SessionData) {
            std::fs::write(&self.path, serde_json::to_vec(data).unwrap()).unwrap();
        }
        fn page(
            &self,
            before: Option<usize>,
            limit: usize,
            revision: Option<&str>,
        ) -> Result<SessionPage, String> {
            load_page_at(&self.db, &self.path, &self.id, before, limit, revision)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(databases) = DATABASES.get() {
                if let Ok(mut databases) = databases.lock() {
                    databases.remove(&self.db);
                }
            }
            // Only this fixture's UUID-named temporary directory is removed.
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn warm_page_never_decodes_unselected_message_rows_or_rewrites_projection() {
        let f = Fixture::new(250);
        let first = f.page(None, 10, None).unwrap();
        let conn = rusqlite::Connection::open(&f.db).unwrap();
        conn.execute(
            "UPDATE page_messages SET json = '{broken' WHERE ordinal = 0",
            [],
        )
        .unwrap();
        conn.execute_batch("CREATE TRIGGER reject_updates BEFORE UPDATE ON page_sessions BEGIN SELECT RAISE(ABORT, 'warm write'); END; CREATE TRIGGER reject_inserts BEFORE INSERT ON page_sessions BEGIN SELECT RAISE(ABORT, 'warm write'); END;").unwrap();
        let warm = f.page(None, 10, None).unwrap();
        assert_eq!(warm.window.revision, first.window.revision);
        assert_eq!(warm.data.messages.len(), 10);
        assert!(f.page(Some(1), 1, None).unwrap_err().contains("projection"));
    }

    #[test]
    fn warm_read_does_not_parse_canonical_json_and_revision_survives_reopen() {
        let f = Fixture::new(12);
        let page = f.page(None, 3, None).unwrap();
        let metadata = std::fs::metadata(&f.path).unwrap();
        let broken = vec![b'!'; metadata.len() as usize];
        std::fs::write(&f.path, broken).unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&f.path)
            .unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
            .unwrap();
        // Drop just the test's connection from the registry, simulating a new
        // process; a fresh connection must reuse the persisted revision/rows.
        DATABASES.get().unwrap().lock().unwrap().remove(&f.db);
        let warm = f.page(None, 3, Some(&page.window.revision)).unwrap();
        assert_eq!(warm.window.revision, page.window.revision);
        assert_eq!(warm.data.messages[0].id, "m9");
        // Missing canonical JSON must not be resurrected from the sidecar.
        let moved = f.root.join("moved");
        std::fs::rename(&f.path, &moved).unwrap();
        assert!(f.page(None, 3, None).is_err());
        std::fs::rename(&moved, &f.path).unwrap();
    }

    #[test]
    fn projection_replacement_rolls_back_header_and_rows_on_failure() {
        let f = Fixture::new(2);
        let page = f.page(None, 2, None).unwrap();
        let conn = rusqlite::Connection::open(&f.db).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_messages BEFORE INSERT ON page_messages BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
        let mut changed = f.data(3);
        changed.session.title = "New title".into();
        f.write(&changed);
        assert!(cache_session_at(&f.db, &f.path, &changed).is_err());
        let (revision, total, count): (String, i64, i64) = conn
            .query_row(
                "SELECT revision,total,(SELECT COUNT(*) FROM page_messages) FROM page_sessions",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(revision, page.window.revision);
        assert_eq!(total, 2);
        assert_eq!(count, 2);
    }

    #[test]
    fn fingerprint_changes_rebuild_and_reject_stale_cursor() {
        let f = Fixture::new(5);
        let first = f.page(None, 2, None).unwrap();
        let mut changed = f.data(6);
        changed.session.title = "External edit".into();
        f.write(&changed);
        assert!(f
            .page(Some(3), 2, Some(&first.window.revision))
            .unwrap_err()
            .contains("stale"));
        let fresh = f.page(None, 2, None).unwrap();
        assert_ne!(fresh.window.revision, first.window.revision);
        assert_eq!(fresh.window.total, 6);
        assert_eq!(fresh.data.session.title, "External edit");
        // Same length, different mtime must also invalidate.
        changed.session.title = "External EDIT".into();
        std::thread::sleep(std::time::Duration::from_millis(25));
        f.write(&changed);
        assert_ne!(
            f.page(None, 2, None).unwrap().window.revision,
            fresh.window.revision
        );
    }

    #[test]
    fn limits_empty_boundaries_and_identity_are_validated() {
        let f = Fixture::new(130);
        assert_eq!(
            f.page(None, usize::MAX, None).unwrap().data.messages.len(),
            100
        );
        assert_eq!(f.page(None, 0, None).unwrap().data.messages.len(), 1);
        let empty = f.page(Some(0), 20, None).unwrap();
        assert!(empty.data.messages.is_empty());
        assert_eq!(empty.window.first_message_id, None);
        assert!(f.page(Some(131), 20, None).is_err());
        assert!(load_page_at(&f.db, &f.path, "wrong", None, 20, None).is_err());
        assert!(load_page_at(&f.db, &f.path, "../bad", None, 20, None).is_err());
        let zero = Fixture::new(0).page(None, 20, None).unwrap();
        assert_eq!(zero.window.total, 0);
    }

    #[test]
    fn artifacts_are_separate_bounded_and_can_be_loaded_lazily() {
        let f = Fixture::new(2);
        let mut data = f.data(2);
        data.artifacts = (0..150).map(|n| serde_json::from_value(json!({"id":format!("a{n}"),"title":"output","type":"code","content":format!("artifact {n}"),"timestamp":n})).unwrap()).collect();
        data.messages[1].tool_calls = Some(vec![serde_json::from_value(
            json!({"id":"call","artifactId":"a0"}),
        )
        .unwrap()]);
        data.messages[1].segments = Some(vec![serde_json::from_value(
            json!({"id":"segment","toolCalls":[{"id":"segment-call","artifactId":"a1"}]}),
        )
        .unwrap()]);
        f.write(&data);
        cache_session_at(&f.db, &f.path, &data).unwrap();
        let page = f.page(None, 1, None).unwrap();
        assert_eq!(page.data.artifacts.len(), 22);
        assert!(page.data.artifacts.iter().any(|a| a.id == "a0"));
        assert!(page.data.artifacts.iter().any(|a| a.id == "a1"));
        assert!(page.data.artifacts.iter().any(|a| a.id == "a149"));
        assert_eq!(
            read_artifact_at(&f.db, &f.path, &f.id, "a50")
                .unwrap()
                .content,
            "artifact 50"
        );
        assert!(read_artifact_at(&f.db, &f.path, &f.id, "missing").is_err());
        assert_eq!(
            serde_json::from_slice::<SessionData>(&std::fs::read(&f.path).unwrap())
                .unwrap()
                .artifacts
                .len(),
            150
        );
        let conn = rusqlite::Connection::open(&f.db).unwrap();
        let header: String = conn
            .query_row("SELECT header FROM page_sessions", [], |r| r.get(0))
            .unwrap();
        assert!(!header.contains("artifact 149"));
    }

    #[test]
    fn corruption_is_reported_without_modifying_original_or_hiding_catalog_errors() {
        let f = Fixture::new(2);
        f.page(None, 2, None).unwrap();
        for raw in ["{broken", "{}", "{\"session\":{\"id\":\"session\"}}"] {
            std::fs::write(&f.path, raw).unwrap();
            let error = f.page(None, 2, None).unwrap_err();
            assert!(error.contains("original preserved"), "{error}");
            assert_eq!(std::fs::read_to_string(&f.path).unwrap(), raw);
        }
        std::fs::write(f.root.join("other.json"), "{bad").unwrap();
        let error = list_catalog_at(&f.db, &f.root).unwrap_err();
        assert!(
            error.contains(f.path.file_name().unwrap().to_str().unwrap())
                && error.contains("other.json"),
            "{error}"
        );
    }

    #[test]
    fn legacy_catalog_is_metadata_only_when_warm_and_missing_files_are_not_served() {
        let f = Fixture::new(2);
        let legacy = json!({"id":f.id,"title":"Legacy","messages":[{"id":"old","role":"user","content":"hello","timestamp":1}],"modelOverride":{"model":"legacy-model"}});
        std::fs::write(&f.path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let page = f.page(None, 5, None).unwrap();
        assert_eq!(page.window.total, 1);
        assert_eq!(
            page.data.model_override.as_ref().unwrap().model.as_deref(),
            Some("legacy-model")
        );
        let conn = rusqlite::Connection::open(&f.db).unwrap();
        conn.execute("UPDATE page_messages SET json = '{bad'", [])
            .unwrap();
        assert_eq!(list_catalog_at(&f.db, &f.root).unwrap()[0].message_count, 1);
        invalidate_at(&f.db, &f.path).unwrap();
        assert!(f.page(None, 5, None).is_ok());
        let missing = f.root.join("missing.json");
        assert!(load_page_at(&f.db, &missing, "missing", None, 10, None).is_err());
        assert!(list_catalog_at(&f.db, &f.root.join("absent"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn cold_then_warm_pages_are_bounded_and_chronological() {
        let f = Fixture::new(250);
        let last = f.page(None, 50, None).unwrap();
        assert_eq!(last.window.offset, 200);
        assert_eq!(last.window.total, 250);
        assert_eq!(last.window.first_message_id.as_deref(), Some("m200"));
        assert_eq!(last.data.session.message_count, 250);
        assert_eq!(
            last.data.model_override.as_ref().unwrap().model.as_deref(),
            Some("chosen")
        );
        assert_eq!(last.data.messages.first().unwrap().id, "m200");
        assert_eq!(last.data.messages.last().unwrap().id, "m249");
        let previous = f
            .page(Some(last.window.offset), 50, Some(&last.window.revision))
            .unwrap();
        assert_eq!(previous.window.offset, 150);
        assert_eq!(previous.data.messages.last().unwrap().id, "m199");
        assert_eq!(previous.window.revision, last.window.revision);
    }
}
