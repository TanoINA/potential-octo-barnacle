use super::*;
use serde_json::json;

#[test]
fn p04_deepseek_helper_replays_reasoning_only_for_deepseek() {
    let mut accumulator = StreamAccumulator::default();
    process_openai_delta(
        &json!({"reasoning_content":"inspect\nthen act", "content":"Checking.", "tool_calls":[{"id":"call-1","function":{"name":"read_file","arguments":"{\"path\":\"a.rs\"}"}}]}),
        &mut accumulator,
        &mut |_| {},
        &mut |_| {},
        &mut |_| {},
    );
    let result = accumulator.finish();
    let message = result.assistant_message(ProviderKind::Deepseek);
    assert_eq!(message["reasoning_content"], "inspect\nthen act");
    assert_eq!(
        message["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"a.rs\"}"
    );
    assert!(message["tool_calls"][0].get("stream_index").is_none());
    for kind in [
        ProviderKind::Openai,
        ProviderKind::Ollama,
        ProviderKind::Anthropic,
    ] {
        assert!(result
            .assistant_message(kind)
            .get("reasoning_content")
            .is_none());
    }
}

#[test]
fn p04_nonstream_preserves_opaque_blocks_and_does_not_duplicate_tools() {
    let content = json!([
        {"type":"thinking","thinking":"original\n","signature":"opaque+/==", "provider_extension":{"key":"untouched"}},
        {"type":"redacted_thinking","data":"opaque encrypted data"},
        {"type":"tool_use","id":"call-1","name":"read_file","input":{}},
    ]);
    let mut accumulator = StreamAccumulator::default();
    process_anthropic_message(
        &json!({"content":content,"stop_reason":"tool_use"}),
        &mut accumulator,
        &mut |_| {},
        &mut |_| {},
        &mut |_| {},
    );
    let result = accumulator.finish();
    assert_eq!(json!(result.assistant_content), content);
    let mut assistant = result.assistant_message(ProviderKind::Anthropic);
    assistant["tool_calls"] = json!(result.tool_calls);
    let (_, converted) = convert_anthropic_messages(&json!([assistant])).unwrap();
    assert_eq!(converted[0]["content"], content);
}

#[test]
fn p04_provider_messages_survive_chat_message_serialization_and_event_scope() {
    use super::super::types::{AgentEvent, ChatMessage, ProviderStateEventData};
    let native = json!([
        {"role":"assistant","content":[{"type":"thinking","thinking":"inspect","signature":"opaque=="},{"type":"tool_use","id":"t1","name":"read_file","input":{}}]},
        {"role":"tool","tool_call_id":"t1","content":"result"},
        {"role":"assistant","content":"done","reasoning_content":"finish"},
    ]);
    let message: ChatMessage = serde_json::from_value(json!({
        "id":"a1", "role":"assistant", "content":"display", "timestamp":1,
        "providerMessages":native,
    }))
    .unwrap();
    let stored = serde_json::to_string(&message).unwrap();
    let loaded: ChatMessage = serde_json::from_str(&stored).unwrap();
    assert_eq!(json!(loaded.provider_messages.unwrap()), native);
    let event = AgentEvent::ProviderState(ProviderStateEventData {
        run_id: "run-1".into(),
        session_id: "session-1".into(),
        provider_messages: native.as_array().unwrap().clone(),
    });
    let wire = serde_json::to_value(event).unwrap();
    assert_eq!(
        wire,
        json!({"type":"provider_state","data":{
            "runId":"run-1","sessionId":"session-1","providerMessages":native,
        }})
    );
    let legacy: ChatMessage =
        serde_json::from_value(json!({"id":"a0","role":"assistant","content":"old","timestamp":0}))
            .unwrap();
    assert!(legacy.provider_messages.is_none());
}

#[test]
fn p04_anthropic_signed_stream_survives_tool_roundtrip() {
    let events = vec![
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Inspect first.\n"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque+/"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"==\n"}}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"encrypted+/=="}}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"text","text":"Checking "}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"now."}}),
        json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"tool-1","name":"read_file","input":{}}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"\"src/main.rs\"}"}}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
    ];
    let mut accumulator = StreamAccumulator::default();
    for event in events {
        process_anthropic_event(
            &event,
            &mut accumulator,
            &mut |_| {},
            &mut |_| {},
            &mut |_| {},
        );
    }
    let result = accumulator.finish();
    let expected = json!([
        {"type":"thinking","thinking":"Inspect first.\n","signature":"opaque+/==\n"},
        {"type":"redacted_thinking","data":"encrypted+/=="},
        {"type":"text","text":"Checking now."},
        {"type":"tool_use","id":"tool-1","name":"read_file","input":{"path":"src/main.rs"}},
    ]);
    assert_eq!(json!(result.assistant_content), expected);
    let assistant = result.assistant_message(ProviderKind::Anthropic);
    let (_, converted) = convert_anthropic_messages(&json!([
        {"role":"system","content":"Instructions"},
        {"role":"user","content":"Inspect"},
        assistant,
        {"role":"tool","tool_call_id":"tool-1","content":"file contents"},
    ]))
    .unwrap();
    assert_eq!(converted[1]["content"], expected);
    assert_eq!(converted[2]["content"][0]["tool_use_id"], "tool-1");
}
