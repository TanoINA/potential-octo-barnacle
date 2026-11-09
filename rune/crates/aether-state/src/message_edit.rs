//! Content-only display edits. Never replay a turn or rewrite provider protocol.
use crate::{history::HistoryAction, Controller};
use aether_core::agent::{ChatMessage, SessionData};

impl Controller {
    /// Reserve the selected clean session and publish only the native CAS acknowledgment.
    /// `expected_source` must be the entire message body rendered by the UI.
    pub fn update_message_content(
        &mut self,
        id: impl Into<String>,
        expected_source: impl Into<String>,
        new_content: impl Into<String>,
    ) -> Result<u64, String> {
        let id = id.into();
        let expected = expected_source.into();
        let new = new_content.into();
        if self.model.busy {
            return Err("Wait for the session operation to finish".into());
        }
        let current = self.model.current.as_ref().ok_or("No session selected")?;
        if self.selected.as_deref() != Some(current.session.id.as_str()) {
            return Err("Visible session does not match the selected session".into());
        }
        replacement(current, &id, &expected, &new)?;
        self.history(HistoryAction::UpdateContent {
            message_id: id,
            expected,
            new,
        })
    }
}

/// Build without mutating the snapshot; both viewport and hydrated canonical data
/// must pass the same exact-source check. Opaque provider/image payloads survive.
pub(crate) fn replacement(
    data: &SessionData,
    id: &str,
    expected: &str,
    new: &str,
) -> Result<(usize, ChatMessage), String> {
    if id.is_empty() {
        return Err("Message ID is required".into());
    }
    let mut found = data.messages.iter().enumerate().filter(|(_, m)| m.id == id);
    let (index, message) = found.next().ok_or("Message is not in this session")?;
    if found.next().is_some() {
        return Err("Ambiguous message ID".into());
    }
    if message.content != expected {
        return Err("Message source changed; reload before editing".into());
    }
    if message.is_streaming == Some(true) {
        return Err("Streaming messages cannot be edited".into());
    }
    let mut next = message.clone();
    // Text segments are ordered around tool traces. Patch the single changed
    // range in place, retaining every segment ID and every tool-call position.
    if let Some(segments) = &mut next.segments {
        if segments.iter().any(|s| s.text.is_some()) {
            let joined: String = segments.iter().filter_map(|s| s.text.as_deref()).collect();
            let prefix = if joined == expected {
                0
            } else if message
                .preamble
                .as_ref()
                .is_some_and(|p| joined.strip_prefix(p.as_str()) == Some(expected))
            {
                message.preamble.as_ref().unwrap().len()
            } else {
                return Err("Message segments do not match the rendered source".into());
            };
            if joined.len() != prefix + expected.len() {
                return Err("Message segments do not match the rendered source".into());
            }
            if expected != new {
                let mut start = expected
                    .bytes()
                    .zip(new.bytes())
                    .take_while(|(a, b)| a == b)
                    .count();
                while !expected.is_char_boundary(start) || !new.is_char_boundary(start) {
                    start -= 1;
                }
                let mut suffix = expected.as_bytes()[start..]
                    .iter()
                    .rev()
                    .zip(new.as_bytes()[start..].iter().rev())
                    .take_while(|(a, b)| a == b)
                    .count();
                while !expected.is_char_boundary(expected.len() - suffix)
                    || !new.is_char_boundary(new.len() - suffix)
                {
                    suffix -= 1;
                }
                let end = prefix + expected.len() - suffix;
                let inserted = &new[start..new.len() - suffix];
                start += prefix;
                let mut offset = 0;
                let mut patched = false;
                for segment in segments {
                    let Some(text) = &mut segment.text else {
                        continue;
                    };
                    let segment_start = offset;
                    offset += text.len();
                    if !patched
                        && start >= segment_start
                        && (start < offset || (start == end && start == offset))
                    {
                        // A task marker is wholly in one source segment. Reject
                        // cross-trace rewrites rather than move text across tools.
                        if start < segment_start || end > offset {
                            return Err("Content edit crosses message segment boundaries".into());
                        }
                        text.replace_range(start - segment_start..end - segment_start, inserted);
                        patched = true;
                    }
                }
                if !patched {
                    return Err("Message segment source is unavailable".into());
                }
            }
        }
    }
    next.content = new.into();
    Ok((index, next))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture_data() -> SessionData {
        let mut data = crate::model::new_data("");
        data.messages = serde_json::from_value(json!([{
            "id":"a", "role":"assistant", "content":"- [ ] Task", "timestamp":1,
            "preamble":"Before工具", "providerMessages":[{"opaque":"keep"}],
            "segments":[
                {"id":"before","text":"Before工具"},
                {"id":"tools","toolCalls":[{"id":"t","name":"read","args":{},"status":"success","result":"keep"}]},
                {"id":"after","text":"- [ ] Task"}
            ]
        }])).unwrap();
        data
    }
    #[test]
    fn patch_preserves_preamble_tool_positions_ids_and_protocol() {
        let data = fixture_data();
        let (_, next) = replacement(&data, "a", "- [ ] Task", "- [x] Task").unwrap();
        let mut expected = data.messages[0].clone();
        expected.content = "- [x] Task".into();
        expected.segments.as_mut().unwrap()[2].text = Some("- [x] Task".into());
        assert_eq!(json!(next), json!(expected));
        assert_eq!(data.messages[0].content, "- [ ] Task");
    }
    #[test]
    fn conflicting_segment_source_and_cross_trace_rewrite_are_rejected() {
        let mut data = fixture_data();
        data.messages[0].segments.as_mut().unwrap()[2].text = Some("different".into());
        assert!(replacement(&data, "a", "- [ ] Task", "- [x] Task").is_err());
        let mut data = fixture_data();
        data.messages[0].preamble = None;
        data.messages[0].content = "Before工具- [ ] Task".into();
        assert!(replacement(&data, "a", "Before工具- [ ] Task", "changed").is_err());
    }
}
