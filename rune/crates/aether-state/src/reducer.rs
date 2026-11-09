use crate::model::{now, LiveSession, RunPhase};
use aether_core::agent::*;
use serde_json::Value;

fn decode<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("Invalid native event: {e}"))
}

fn upsert(tools: &mut Vec<ToolCallItem>, incoming: &ToolCallItem) {
    if let Some(old) = tools.iter_mut().find(|t| t.id == incoming.id) {
        *old = incoming.clone();
    } else {
        tools.push(incoming.clone());
    }
}

/// Returns false for a stale/foreign run; malformed matching events are errors.
pub(crate) fn apply(live: &mut LiveSession, kind: &str, data: &Value) -> Result<bool, String> {
    if data["sessionId"].as_str() != Some(&live.data.session.id)
        || data["runId"].as_str() != live.run_id.as_deref()
        || live.done
    {
        return Ok(false);
    }
    let index = live
        .data
        .messages
        .iter()
        .position(|m| Some(&m.id) == live.message_id.as_ref())
        .ok_or("Active assistant missing")?;
    let m = &mut live.data.messages[index];
    let mut phase = None;
    match kind {
        "chunk" => {
            let text = data["text"].as_str().ok_or("Chunk text missing")?;
            m.content.push_str(text);
            let segments = m.segments.get_or_insert_with(Vec::new);
            if segments
                .last()
                .is_none_or(|s| s.tool_calls.as_ref().is_some_and(|t| !t.is_empty()))
            {
                segments.push(MessageSegment {
                    id: format!("{}_seg_{}", m.id, segments.len()),
                    text: Some(String::new()),
                    tool_calls: Some(vec![]),
                });
            }
            segments
                .last_mut()
                .unwrap()
                .text
                .get_or_insert_with(String::new)
                .push_str(text);
            live.notice = None;
            phase = Some(RunPhase::ReceivingAnswer);
        }
        "reasoning" => {
            m.reasoning
                .get_or_insert_with(String::new)
                .push_str(data["text"].as_str().ok_or("Reasoning missing")?);
            phase = Some(RunPhase::ReceivingAnswer);
        }
        "provider_state" => m.provider_messages = Some(decode(&data["providerMessages"])?),
        "notice" => {
            let notice: NoticeEventData = decode(data)?;
            if notice.reset_partial {
                m.reasoning = Some(String::new());
                m.checkpoint_epoch = Some(m.checkpoint_epoch.unwrap_or(0) + 1);
            }
            live.notice = Some(notice);
            phase = Some(RunPhase::WaitingProvider);
        }
        "tool_update" => {
            let mut tool: ToolCallItem = decode(&data["tool"])?;
            let tools = m.tool_calls.get_or_insert_with(Vec::new);
            tool.started_at = tool
                .started_at
                .or_else(|| {
                    tools
                        .iter()
                        .find(|t| t.id == tool.id)
                        .and_then(|t| t.started_at)
                })
                .or(Some(now()));
            upsert(tools, &tool);
            if m.preamble.is_none() && !m.content.trim().is_empty() {
                m.preamble = Some(std::mem::take(&mut m.content));
            }
            let segments = m.segments.get_or_insert_with(Vec::new);
            // Update a tool in its original segment, not the segment of a late completion.
            let existing = segments.iter().position(|s| {
                s.tool_calls
                    .as_ref()
                    .is_some_and(|t| t.iter().any(|t| t.id == tool.id))
            });
            if segments.is_empty() {
                segments.push(MessageSegment {
                    id: format!("{}_seg_0", m.id),
                    text: Some(String::new()),
                    tool_calls: Some(vec![]),
                });
            }
            let at = existing.unwrap_or(segments.len() - 1);
            upsert(segments[at].tool_calls.get_or_insert_with(Vec::new), &tool);
            phase = Some(if tools.iter().any(|t| t.status == "running") {
                RunPhase::RunningTool
            } else {
                RunPhase::WaitingProvider
            });
        }
        "artifact" => {
            let incoming: Artifact = decode(&data["artifact"])?;
            let id = incoming.id.clone();
            if let Some(old) = live.data.artifacts.iter_mut().find(|a| a.id == id) {
                old.content = incoming.content;
                old.stream_revision = Some(old.stream_revision.unwrap_or(0) + 1);
            } else {
                let mut incoming = incoming;
                incoming.stream_revision = Some(1);
                live.data.artifacts.push(incoming);
            }
            live.modified_artifacts.insert(id.clone());
            live.selected_artifact = Some(id);
        }
        "terminal_output" => {
            let id = data["artifactId"]
                .as_str()
                .ok_or("Terminal artifactId missing")?;
            let artifact = live
                .data
                .artifacts
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or("Terminal output preceded artifact")?;
            artifact
                .content
                .push_str(data["delta"].as_str().ok_or("Terminal delta missing")?);
            artifact.stream_revision = Some(artifact.stream_revision.unwrap_or(0) + 1);
            live.modified_artifacts.insert(id.into());
        }
        "subagent_update" => {
            let sub: SubagentProgress = decode(&data["subagent"])?;
            if let Some(old) = live.subagents.iter_mut().find(|s| s.id == sub.id) {
                *old = sub;
            } else {
                live.subagents.push(sub);
            }
        }
        "skill_activated" => {
            let skill: SkillInfo = decode(&data["skill"])?;
            if !live
                .skills
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(&skill.name))
            {
                live.skills.push(skill);
            }
        }
        "council_chunk" | "council_done" => {
            let turn = m
                .council_turn
                .as_mut()
                .ok_or("Council event without council turn")?;
            let candidate = match data["candidate"].as_str() {
                Some("A") => &mut turn.candidate_a,
                Some("B") => &mut turn.candidate_b,
                _ => return Err("Invalid council candidate".into()),
            };
            if kind == "council_chunk" {
                let text = data["text"].as_str().ok_or("Council text missing")?;
                if data["kind"] == "reasoning" {
                    candidate
                        .reasoning
                        .get_or_insert_with(String::new)
                        .push_str(text);
                } else {
                    candidate.content.push_str(text);
                }
            } else {
                candidate.is_streaming = Some(false);
                candidate.duration_ms = data["durationMs"].as_i64();
            }
            phase = Some(RunPhase::ReceivingAnswer);
        }
        "error" => {
            live.failed = true;
            live.error = Some(
                data["message"]
                    .as_str()
                    .unwrap_or("Native run failed")
                    .into(),
            );
        }
        "done" => {
            let cancelled = data["cancelled"]
                .as_bool()
                .ok_or("Done cancelled missing")?;
            let successful = data["successful"]
                .as_bool()
                .ok_or("Done successful missing")?;
            live.failed |= !successful && !cancelled;
            live.cancelled
                .store(cancelled, std::sync::atomic::Ordering::Release);
            m.is_streaming = Some(false);
            m.run_status = Some(
                if cancelled {
                    "cancelled"
                } else if live.failed || !successful {
                    "failed"
                } else {
                    "completed"
                }
                .into(),
            );
            for t in m.tool_calls.iter_mut().flatten().chain(
                m.segments
                    .iter_mut()
                    .flatten()
                    .flat_map(|s| s.tool_calls.iter_mut().flatten()),
            ) {
                if matches!(t.status.as_str(), "pending" | "running") {
                    t.status = if cancelled { "cancelled" } else { "error" }.into();
                }
            }
            if let Some(turn) = &mut m.council_turn {
                turn.candidate_a.is_streaming = Some(false);
                turn.candidate_b.is_streaming = Some(false);
            }
            if cancelled {
                for s in &mut live.subagents {
                    if s.status == "running" {
                        s.status = "cancelled".into();
                        s.completed_at = Some(now());
                    }
                }
            }
            live.done = true;
            live.notice = None;
            phase = Some(RunPhase::Saving);
            live.data.session.updated_at = now();
            live.data.session.message_count =
                live.window.as_ref().map_or(0, |w| w.offset) + live.data.messages.len();
            live.data.session.preview = live
                .data
                .messages
                .iter()
                .rev()
                .find(|m| !m.content.is_empty())
                .map(|m| m.content.chars().take(240).collect());
        }
        _ => return Ok(false),
    }
    if let (Some(status), Some(phase)) = (&mut live.status, phase) {
        status.phase = phase;
    }
    live.changed();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn run_scoped_stream_preserves_native_tool_and_artifact_ids() {
        let data = serde_json::from_value(json!({"session":{"id":"s","title":"test","createdAt":0,"updatedAt":0,"messageCount":1},"messages":[{"id":"assistant-r","role":"assistant","content":"","timestamp":0,"runId":"r","isStreaming":true}]})).unwrap();
        let mut live = LiveSession::new(data, None);
        live.run_id = Some("r".into());
        live.message_id = Some("assistant-r".into());
        assert!(!apply(
            &mut live,
            "chunk",
            &json!({"runId":"wrong","sessionId":"s","text":"BAD"})
        )
        .unwrap());
        apply(
            &mut live,
            "chunk",
            &json!({"runId":"r","sessionId":"s","text":"before"}),
        )
        .unwrap();
        apply(&mut live, "tool_update", &json!({"runId":"r","sessionId":"s","tool":{"id":"native-tool","name":"read_file","status":"done"}})).unwrap();
        apply(
            &mut live,
            "chunk",
            &json!({"runId":"r","sessionId":"s","text":"after"}),
        )
        .unwrap();
        apply(&mut live, "artifact", &json!({"runId":"r","sessionId":"s","artifact":{"id":"native-art","title":"shell","type":"terminal","content":"","timestamp":1}})).unwrap();
        apply(
            &mut live,
            "terminal_output",
            &json!({"runId":"r","sessionId":"s","artifactId":"native-art","delta":"out"}),
        )
        .unwrap();
        let m = &live.data.messages[0];
        assert_eq!(m.preamble.as_deref(), Some("before"));
        assert_eq!(m.content, "after");
        assert_eq!(m.tool_calls.as_ref().unwrap()[0].id, "native-tool");
        assert_eq!(m.segments.as_ref().unwrap().len(), 2);
        assert_eq!(live.data.artifacts[0].id, "native-art");
        assert_eq!(live.data.artifacts[0].content, "out");
    }
}
