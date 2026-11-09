//! Pure validation shared by native controls and service fixtures.
use serde_json::Value;

pub const SECRET: &str = "<unchanged secret>";

/// Merge onto the complete loaded document, then restore redacted secrets. A
/// partial advanced edit must not silently reset unrelated settings to defaults.
pub fn settings_document(source: &str, original: &Value) -> Result<Value, String> {
    let patch: Value =
        serde_json::from_str(source).map_err(|e| format!("Invalid settings JSON: {e}"))?;
    if !patch.is_object() {
        return Err("Settings must be a JSON object".into());
    }
    let mut full = original.clone();
    merge(&mut full, &patch);
    restore(&mut full, original)?;
    Ok(full)
}

fn merge(target: &mut Value, patch: &Value) {
    if let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) {
        for (key, value) in patch {
            if value.is_object() {
                merge(
                    target
                        .entry(key.clone())
                        .or_insert(Value::Object(Default::default())),
                    value,
                );
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
    } else {
        *target = patch.clone();
    }
}

fn restore(value: &mut Value, original: &Value) -> Result<(), String> {
    match value {
        Value::String(s) if s == SECRET => {
            if original.is_null() {
                return Err("A new secret must be entered in its secure field".into());
            }
            *value = original.clone();
        }
        Value::Object(map) => {
            for (key, item) in map {
                restore(item, original.get(key).unwrap_or(&Value::Null))?;
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                // Stable identity, not position, protects keys when providers reorder.
                let old = item
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| {
                        original
                            .as_array()?
                            .iter()
                            .find(|v| v.get("id").and_then(Value::as_str) == Some(id))
                    })
                    .or_else(|| {
                        if item.get("id").is_none() {
                            original.get(i)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(&Value::Null);
                restore(item, old)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Never print provider keys, MCP environment secrets or authorization headers
/// in summaries, error details, clipboard JSON or the advanced settings editor.
pub fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let lower = k.to_ascii_lowercase();
                    let sensitive = lower.contains("apikey")
                        || lower.contains("api_key")
                        || lower.contains("token")
                            && !lower.contains("max")
                            && !lower.contains("threshold")
                        || lower.contains("password")
                        || lower.contains("secret")
                        || lower == "authorization"
                        || lower == "env"
                        || lower == "extraheaders";
                    (
                        k.clone(),
                        if sensitive && !v.is_null() {
                            Value::String(SECRET.into())
                        } else {
                            redact(v)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact).collect()),
        _ => value.clone(),
    }
}

pub fn object(source: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(source).map_err(|e| format!("Invalid JSON: {e}"))?;
    if !value.is_object() {
        return Err("Arguments must be a JSON object".into());
    }
    Ok(value)
}

pub fn scope(kind: &str, session: &str, item: &str) -> String {
    // JSON tuple encoding is collision-free even when IDs contain colons.
    serde_json::to_string(&(kind, session, item)).expect("string tuple serializes")
}

pub fn code(source: &str, language: &str) -> String {
    let longest = source.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    let language: String = language
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(32)
        .collect();
    format!("{fence}{language}\n{source}\n{fence}")
}

pub fn needs_confirmation(command: &str) -> bool {
    matches!(
        command,
        "install_release_update"
            | "delete_session"
            | "undo_turn"
            | "rollback_turn"
            | "retry_turn"
            | "edit_and_resend"
            | "revert_file_changes"
            | "kill_background_task"
            | "approve_review_proposal"
            | "reject_review_proposal"
            | "compact_session"
            | "git_push"
            | "save_session"
            | "save_session_page"
            | "refresh_session_window"
            | "checkpoint_session"
            | "checkpoint_session_delta"
            | "checkpoint_session_page"
            | "save_pasted_attachment"
            | "save_skill"
            | "save_workspace_agents_md"
            | "init_workspace_agents_md"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn settings_edits_preserve_unedited_configuration() {
        let original = json!({"providers":[{"id":"p", "apiKey":"private"}], "reviewConfig":{"writeApproval":true}, "futureOption":17});
        let edited = settings_document(r#"{"temperature":0.4}"#, &original).unwrap();
        assert_eq!(edited["futureOption"], 17);
        assert_eq!(edited["providers"][0]["apiKey"], "private");
        assert_eq!(edited["reviewConfig"]["writeApproval"], true);
        assert_eq!(edited["temperature"], 0.4);
    }
    #[test]
    fn redacted_reordered_provider_keys_restore_by_id() {
        let original = json!({"providers":[{"id":"a", "apiKey":"A"}, {"id":"b", "apiKey":"B"}], "maxTokens":8192});
        let mut safe = redact(&original);
        safe["providers"].as_array_mut().unwrap().reverse();
        assert!(!safe.to_string().contains("\"A\""));
        let restored = settings_document(&safe.to_string(), &original).unwrap();
        assert_eq!(restored["providers"][0]["apiKey"], "B");
        assert_eq!(safe["maxTokens"], 8192);
    }
    #[test]
    fn validation_rejects_non_objects() {
        assert!(settings_document("[]", &json!({})).is_err());
        assert!(object("null").is_err());
        assert!(object("{bad}").is_err());
    }
    #[test]
    fn scope_cannot_collide_and_code_cannot_escape_fence() {
        assert_ne!(scope("tasks", "a:b", "c"), scope("tasks", "a", "b:c"));
        let rendered = code("```\ndanger", "shell");
        assert!(rendered.starts_with("````shell\n"));
        assert!(rendered.ends_with("\n````"));
        assert!(needs_confirmation("revert_file_changes"));
        assert!(needs_confirmation("delete_session"));
        assert!(!needs_confirmation("git_status"));
    }
}

#[cfg(test)]
mod identity_regression {
    use super::*;
    use serde_json::json;
    #[test]
    fn new_provider_must_not_inherit_another_provider_secret() {
        let original = json!({"providers":[{"id":"old","apiKey":"must-not-copy"}]});
        let patch = json!({"providers":[{"id":"new","apiKey":SECRET}]});
        assert!(settings_document(&patch.to_string(), &original).is_err());
    }
}

/// Plain clipboard form uses the same Markdown parser as the native renderer.
/// Formatting delimiters and link destinations are omitted; code/math remain text.
pub fn plain_text(source: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, TagEnd};
    let mut text = String::new();
    for event in Parser::new_ext(source, Options::all()) {
        match event {
            Event::Text(value)
            | Event::Code(value)
            | Event::InlineMath(value)
            | Event::DisplayMath(value) => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak | Event::Rule => text.push('\n'),
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::CodeBlock
                | TagEnd::TableRow,
            ) => {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
            }
            Event::End(TagEnd::TableCell) => text.push('\t'),
            _ => {}
        }
    }
    text.trim().to_owned()
}
