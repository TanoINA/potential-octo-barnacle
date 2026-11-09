use super::storage::config_dir;
use super::types::SessionData;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSearchResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "sessionTitle")]
    pub session_title: String,
    #[serde(rename = "sessionUpdatedAt")]
    pub session_updated_at: i64,
    pub snippet: String,
    #[serde(rename = "matchCount")]
    pub match_count: usize,
    pub score: f64,
    /// Highest-ranked matching message; title/artifact-only hits have no jump.
    #[serde(rename = "matchMessageId", skip_serializing_if = "Option::is_none")]
    pub match_message_id: Option<String>,
}

static DB_LOCK: Mutex<()> = Mutex::new(());

fn db_path() -> PathBuf {
    config_dir().join("search_index.db")
}

pub fn open_connection() -> Result<Connection, String> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn =
        Connection::open(&path).map_err(|e| format!("Could not open search database: {e}"))?;
    init_db(&conn)?;
    Ok(conn)
}

pub fn init_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS fts_content USING fts5(
            session_id UNINDEXED,
            session_title UNINDEXED,
            session_updated_at UNINDEXED,
            content_type UNINDEXED,
            source_id UNINDEXED,
            title,
            body,
            tokenize = 'porter unicode61'
        );",
    )
    .map_err(|e| format!("Could not initialize FTS5 table: {e}"))?;
    Ok(())
}

/// Truncate to at most `max_bytes`, backing up to a UTF-8 char boundary so
/// multibyte content can never panic the indexer (release aborts on panic).
fn bounded_str(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub fn index_session(data: &SessionData) -> Result<(), String> {
    let _guard = DB_LOCK
        .lock()
        .map_err(|_| "Search index lock unavailable".to_string())?;
    let mut conn = open_connection()?;
    index_session_into(&mut conn, data)
}

fn index_session_into(conn: &mut Connection, data: &SessionData) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    tx.execute(
        "DELETE FROM fts_content WHERE session_id = ?1",
        params![data.session.id],
    )
    .map_err(|e| e.to_string())?;

    let s_id = &data.session.id;
    let s_title = &data.session.title;
    let s_updated = data.session.updated_at;

    // Index session title
    tx.execute(
        "INSERT INTO fts_content (session_id, session_title, session_updated_at, content_type, source_id, title, body)
         VALUES (?1, ?2, ?3, 'session_title', ?1, ?2, ?2)",
        params![s_id, s_title, s_updated],
    )
    .map_err(|e| e.to_string())?;

    // Index messages
    for msg in &data.messages {
        let mut full_body = msg.content.clone();
        if let Some(preamble) = &msg.preamble {
            full_body = format!("{}\n\n{}", preamble, full_body);
        }
        if let Some(tool_calls) = &msg.tool_calls {
            for call in tool_calls {
                full_body.push(' ');
                full_body.push_str(&call.name);
                if let Some(res) = &call.result {
                    full_body.push(' ');
                    full_body.push_str(bounded_str(res, 4000));
                }
            }
        }
        tx.execute(
            "INSERT INTO fts_content (session_id, session_title, session_updated_at, content_type, source_id, title, body)
             VALUES (?1, ?2, ?3, 'message', ?4, ?2, ?5)",
            params![s_id, s_title, s_updated, msg.id, full_body],
        )
        .map_err(|e| e.to_string())?;
    }

    // Index artifacts
    for art in &data.artifacts {
        let snippet_body = bounded_str(&art.content, 8000);
        tx.execute(
            "INSERT INTO fts_content (session_id, session_title, session_updated_at, content_type, source_id, title, body)
             VALUES (?1, ?2, ?3, 'artifact', ?4, ?5, ?6)",
            params![s_id, s_title, s_updated, art.id, art.title, snippet_body],
        )
        .map_err(|e| e.to_string())?;
    }

    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_session_index(session_id: &str) -> Result<(), String> {
    let _guard = DB_LOCK
        .lock()
        .map_err(|_| "Search index lock unavailable".to_string())?;
    let conn = open_connection()?;
    conn.execute(
        "DELETE FROM fts_content WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn search_query(query: &str, limit: usize) -> Result<Vec<DeepSearchResult>, String> {
    let _guard = DB_LOCK
        .lock()
        .map_err(|_| "Search index lock unavailable".to_string())?;
    let conn = open_connection()?;
    search_query_with(&conn, query, limit)
}

/// Query an already-open connection. Split out so tests can exercise the real
/// ranking and aggregation logic against an in-memory index.
pub(crate) fn search_query_with(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<DeepSearchResult>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let safe_words: Vec<String> = words
        .iter()
        .map(|w| {
            let clean: String = w
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if clean.is_empty() {
                String::new()
            } else {
                format!("\"{}\"*", clean)
            }
        })
        .filter(|w| !w.is_empty())
        .collect();

    if safe_words.is_empty() {
        return Ok(Vec::new());
    }

    let fts_query = safe_words.join(" AND ");

    let mut stmt = conn
        .prepare(
            "SELECT session_id, session_title, session_updated_at,
                    snippet(fts_content, 6, '<mark>', '</mark>', '...', 18) as snip,
                    bm25(fts_content) as score, content_type, source_id
             FROM fts_content
             WHERE fts_content MATCH ?1
             ORDER BY score ASC, rowid ASC
             LIMIT 10000",
        )
        .map_err(|e| e.to_string())?;

    struct RawMatch {
        session_id: String,
        session_title: String,
        session_updated_at: i64,
        snippet: String,
        score: f64,
        message_id: Option<String>,
    }

    let rows = stmt
        .query_map(params![fts_query], |row| {
            Ok(RawMatch {
                session_id: row.get(0)?,
                session_title: row.get(1)?,
                session_updated_at: row.get(2)?,
                snippet: row.get(3)?,
                score: row.get(4)?,
                message_id: if row.get::<_, String>(5)? == "message" {
                    Some(row.get(6)?)
                } else {
                    None
                },
            })
        })
        .map_err(|e| e.to_string())?;

    use std::collections::BTreeMap;
    let mut session_map: BTreeMap<String, DeepSearchResult> = BTreeMap::new();

    for r in rows.flatten() {
        let entry = session_map
            .entry(r.session_id.clone())
            .or_insert_with(|| DeepSearchResult {
                session_id: r.session_id,
                session_title: r.session_title,
                session_updated_at: r.session_updated_at,
                snippet: r.snippet.clone(),
                match_count: 0,
                score: r.score,
                match_message_id: r.message_id.clone(),
            });
        entry.match_count += 1;
        // Rows are sorted by rank then insertion ordinal. Keep the first message
        // even when the best overall hit is a title or artifact.
        if entry.match_message_id.is_none() && r.message_id.is_some() {
            entry.match_message_id = r.message_id.clone();
            entry.snippet = r.snippet.clone();
        }
        if r.score < entry.score {
            entry.score = r.score;
            entry.snippet = r.snippet;
        }
    }

    let mut results: Vec<DeepSearchResult> = session_map.into_values().collect();
    results.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);
    Ok(results)
}

#[cfg(test)]
mod tests {
    #[test]
    fn message_jump_search_preserves_exact_id_and_artifact_only_fallback() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let mut data = sample_session("jump", "Ordinary title", "uniquejump body");
        data.messages[0].id = "exact-native-id".into();
        index_session_into(&mut conn, &data).unwrap();
        let first = search_query_with(&conn, "uniquejump", 10).unwrap();
        assert_eq!(
            first[0].match_message_id.as_deref(),
            Some("exact-native-id")
        );
        data.messages[0].content = "No match now".into();
        data.artifacts[0].content = "artifactneedle".into();
        index_session_into(&mut conn, &data).unwrap();
        assert!(search_query_with(&conn, "uniquejump", 10)
            .unwrap()
            .is_empty());
        let artifact = search_query_with(&conn, "artifactneedle", 10).unwrap();
        assert_eq!(artifact[0].session_id, "jump");
        assert!(artifact[0].match_message_id.is_none());
    }

    use super::*;
    use crate::agent::types::{Artifact, ChatMessage, Session};

    fn sample_session(id: &str, title: &str, msg: &str) -> SessionData {
        SessionData {
            session: Session {
                id: id.into(),
                title: title.into(),
                created_at: 100,
                updated_at: 200,
                message_count: 1,
                workspace_cwd: None,
                preview: None,
                pinned: false,
                pinned_at: None,
            },
            messages: vec![ChatMessage {
                selected_skills: Vec::new(),
                checkpoint_epoch: None,
                id: format!("msg_{id}"),
                role: "assistant".into(),
                content: msg.into(),
                timestamp: 150,
                provider_messages: None,
                has_image_inputs: None,
                has_provider_messages: None,
                reasoning: None,
                tool_calls: None,
                is_streaming: None,
                attachments: None,
                image_inputs: None,
                council_turn: None,
                run_id: None,
                run_status: None,
                self_improvement: None,
                preamble: None,
                segments: None,
            }],
            artifacts: vec![Artifact {
                stream_revision: None,
                id: format!("art_{id}"),
                title: format!("artifact_{title}"),
                artifact_type: "code".into(),
                language: Some("rust".into()),
                content: format!("fn calculate_{id}() {{ println!(\"quantum compute\"); }}"),
                file_path: Some(format!("src/{id}.rs")),
                timestamp: 160,
            }],
            model_override: None,
        }
    }

    #[test]
    fn fts_indexes_and_matches_messages_with_snippets() {
        let memory_conn = Connection::open_in_memory().unwrap();
        init_db(&memory_conn).unwrap();

        let s1 = sample_session(
            "s1",
            "Rust Optimization",
            "We need to optimize the micro-compaction algorithm for speed.",
        );
        let s2 = sample_session(
            "s2",
            "UI Polish",
            "Let's align the buttons in the navigation drawer.",
        );

        // Insert into memory table
        for s in &[&s1, &s2] {
            memory_conn.execute(
                "INSERT INTO fts_content (session_id, session_title, session_updated_at, content_type, source_id, title, body)
                 VALUES (?1, ?2, ?3, 'message', ?4, ?2, ?5)",
                params![s.session.id, s.session.title, s.session.updated_at, s.messages[0].id, s.messages[0].content],
            ).unwrap();
        }

        // Query for "optimization"
        let mut stmt = memory_conn.prepare(
            "SELECT session_id, session_title, snippet(fts_content, 6, '<mark>', '</mark>', '...', 18)
             FROM fts_content
             WHERE fts_content MATCH '\"optim*\"'",
        ).unwrap();

        let matches: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "s1");
        assert!(matches[0].1.contains("<mark>"));
    }

    #[test]
    fn fts_handles_special_query_characters_gracefully() {
        // Test query sanitization with strange punctuation or unfinished syntax
        let res = search_query("((( AND OR NOT *** ???", 10);
        assert!(res.is_ok());
    }

    #[test]
    fn indexing_multibyte_tool_output_and_artifacts_truncates_on_char_boundaries() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        // 3999 ASCII bytes plus one 2-byte character: byte 4000 is mid-character,
        // exactly where the tool-result truncation used to slice blindly.
        let straddling_result = format!("{}{}", "a".repeat(3999), "é");
        // 7999 ASCII bytes plus one 2-byte character: byte 8000 is mid-character,
        // exactly where the artifact truncation used to slice blindly.
        let straddling_artifact = format!("{}{}", "b".repeat(7999), "é");

        let mut data = sample_session("s-multibyte", "Multibyte Session", "message body");
        data.messages[0].tool_calls = Some(vec![crate::agent::types::ToolCallItem {
            id: "call-1".into(),
            name: "read_file".into(),
            args: serde_json::json!({"path": "huge.txt"}),
            status: "done".into(),
            result: Some(straddling_result),
            ..Default::default()
        }]);
        data.artifacts[0].content = straddling_artifact;

        index_session_into(&mut conn, &data)
            .expect("indexing must never panic on multibyte content");

        let message_body: String = conn
            .query_row(
                "SELECT body FROM fts_content WHERE content_type = 'message'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            message_body.ends_with(&"a".repeat(3999)),
            "tool result must be truncated on a char boundary"
        );
        assert!(!message_body.contains('é'));

        let artifact_body: String = conn
            .query_row(
                "SELECT body FROM fts_content WHERE content_type = 'artifact'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            artifact_body.ends_with(&"b".repeat(7999)),
            "artifact content must be truncated on a char boundary"
        );
        assert!(!artifact_body.contains('é'));
    }

    #[test]
    fn deep_search_does_not_let_one_session_crowd_out_other_sessions() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        // Session A matches "zebra" in 150 short messages. bm25 ranks short
        // documents above the one long matching message in session B, so a
        // naive row limit drops session B from the results entirely.
        let mut busy = sample_session("busy", "Busy session", "zebra");
        busy.messages = (0..150)
            .map(|i| ChatMessage {
                selected_skills: Vec::new(),
                checkpoint_epoch: None,
                id: format!("m{i}"),
                role: "assistant".into(),
                content: "zebra".into(),
                timestamp: 150,
                provider_messages: None,
                has_image_inputs: None,
                has_provider_messages: None,
                reasoning: None,
                tool_calls: None,
                is_streaming: None,
                attachments: None,
                image_inputs: None,
                council_turn: None,
                run_id: None,
                run_status: None,
                self_improvement: None,
                preamble: None,
                segments: None,
            })
            .collect();
        index_session_into(&mut conn, &busy).unwrap();

        let mut rare = sample_session("rare", "Rare session", "zebra");
        rare.messages[0].content = format!("zebra {}", "filler ".repeat(200));
        index_session_into(&mut conn, &rare).unwrap();

        let results = search_query_with(&conn, "zebra", 20).unwrap();
        let ids: Vec<&str> = results.iter().map(|r| r.session_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["busy", "rare"],
            "both sessions must survive ranking"
        );
        let busy_result = results.iter().find(|r| r.session_id == "busy").unwrap();
        assert_eq!(busy_result.match_count, 150);
        let rare_result = results.iter().find(|r| r.session_id == "rare").unwrap();
        assert_eq!(rare_result.match_count, 1);
    }

    #[test]
    fn search_query_supports_prefix_wildcard_matching() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        let s1 = sample_session("s1", "Factorial Algorithm", "Computing factorial recursion");
        index_session_into(&mut conn, &s1).unwrap();

        // Search using partial prefix "fact"
        let results = search_query_with(&conn, "fact", 10).unwrap();
        assert_eq!(
            results.len(),
            1,
            "prefix query 'fact' must match 'factorial'"
        );
        assert_eq!(results[0].session_id, "s1");

        // Search using full word "factorial"
        let results_full = search_query_with(&conn, "factorial", 10).unwrap();
        assert_eq!(results_full.len(), 1, "full query 'factorial' must match");
        assert_eq!(results_full[0].session_id, "s1");
    }
}
