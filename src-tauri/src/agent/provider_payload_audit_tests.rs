use super::*;
use serde_json::{json, Value};

fn config(kind: ProviderKind, model: &str) -> ProviderConfig {
    ProviderConfig {
        kind,
        model: model.into(),
        base_url: "https://api.openai.com/v1".into(),
        ..ProviderConfig::default()
    }
}

fn options(thinking: bool, effort: ReasoningEffort) -> CompletionOptions {
    CompletionOptions {
        thinking,
        reasoning_effort: effort,
        temperature: 0.7,
        max_tokens: Some(8192),
        stream: true,
    }
}

fn openai(provider: &ProviderConfig, options: &CompletionOptions) -> Value {
    openai_body(
        provider,
        &json!([{"role":"user","content":"Hello"}]),
        &json!([]),
        options,
    )
}

fn anthropic(provider: &ProviderConfig, options: &CompletionOptions) -> Result<Value, String> {
    anthropic_body(
        provider,
        &json!([{"role":"user","content":"Hello"}]),
        &json!([]),
        options,
    )
}

#[test]
fn provider_payload_official_non_reasoning_has_no_vendor_controls() {
    for thinking in [false, true] {
        let body = openai(
            &config(ProviderKind::Openai, "gpt-4o"),
            &options(thinking, ReasoningEffort::ExtraHigh),
        );
        assert!(
            body.get("thinking").is_none(),
            "official OpenAI must not receive vendor extensions: {body}"
        );
        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["temperature"], 0.7);
        assert_eq!(body["max_tokens"], 8192);
        assert!(body.get("max_completion_tokens").is_none());
    }
}

#[test]
fn provider_payload_official_reasoning_uses_supported_efforts_and_completion_cap() {
    let efforts = [
        ReasoningEffort::Minimal,
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
        ReasoningEffort::ExtraHigh,
        ReasoningEffort::Max,
        ReasoningEffort::Ultra,
    ];
    let cases = [
        (
            "o3",
            ["low", "low", "medium", "high", "high", "high", "high"],
            "low",
        ),
        (
            "o4-mini-2025-04-16",
            ["low", "low", "medium", "high", "high", "high", "high"],
            "low",
        ),
        (
            "gpt-5",
            ["minimal", "low", "medium", "high", "high", "high", "high"],
            "minimal",
        ),
        (
            "gpt-5-mini",
            ["minimal", "low", "medium", "high", "high", "high", "high"],
            "minimal",
        ),
        (
            "gpt-5.1",
            ["low", "low", "medium", "high", "high", "high", "high"],
            "none",
        ),
        (
            "gpt-5.2",
            ["low", "low", "medium", "high", "xhigh", "xhigh", "xhigh"],
            "none",
        ),
    ];
    for (model, expected, off) in cases {
        for (effort, expected) in efforts.into_iter().zip(expected) {
            let body = openai(&config(ProviderKind::Openai, model), &options(true, effort));
            assert_eq!(body["reasoning_effort"], expected, "{model}: {body}");
            assert_eq!(body["max_completion_tokens"], 8192, "{model}");
            for forbidden in ["thinking", "think", "max_tokens", "temperature"] {
                assert!(
                    body.get(forbidden).is_none(),
                    "{model} contains {forbidden}: {body}"
                );
            }
        }
        let body = openai(
            &config(ProviderKind::Openai, model),
            &options(false, ReasoningEffort::High),
        );
        assert_eq!(body["reasoning_effort"], off, "{model}: {body}");
        assert_eq!(body.get("temperature").is_some(), off == "none");
    }
    for model in ["o1-mini", "o1-preview"] {
        let body = openai(
            &config(ProviderKind::Openai, model),
            &options(true, ReasoningEffort::High),
        );
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("temperature").is_none());
        assert_eq!(body["max_completion_tokens"], 8192);
    }
}

#[test]
fn provider_payload_ollama_compatibility_controls_and_custom_wire_mapping() {
    for (effort, wire) in [
        (ReasoningEffort::Minimal, "minimal"),
        (ReasoningEffort::Low, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::ExtraHigh, "xhigh"),
        (ReasoningEffort::Max, "max"),
        (ReasoningEffort::Ultra, "ultra"),
    ] {
        for thinking in [false, true] {
            let body = openai(
                &config(ProviderKind::Ollama, "qwen3"),
                &options(thinking, effort),
            );
            assert_eq!(
                body["reasoning_effort"],
                if thinking { wire } else { "none" }
            );
            assert!(body.get("thinking").is_none());
            assert!(body.get("think").is_none());
        }
        for host in [
            "https://gutsai.example/v1",
            "https://api.openai.com.proxy.example/v1",
            "https://api.openai.com@proxy.example/v1",
        ] {
            let mut provider = config(ProviderKind::Openai, "gpt-5.2");
            provider.base_url = host.into();
            let body = openai(&provider, &options(false, effort));
            assert_eq!(body["thinking"]["type"], "disabled");
            assert_eq!(body["reasoning_effort"], wire);
            assert_eq!(body["temperature"], 0.7);
            assert_eq!(body["max_tokens"], 8192);
        }
    }
}

#[test]
fn provider_payload_anthropic_manual_thinking_validates_without_raising_cap() {
    let provider = config(ProviderKind::Anthropic, "claude-sonnet-4-5-20250929");
    let mut opts = options(true, ReasoningEffort::ExtraHigh);
    let body = anthropic(&provider, &opts).unwrap();
    assert!(body.get("temperature").is_none());
    assert!(body.get("output_config").is_none());
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["max_tokens"], 8192);
    assert!(body["thinking"]["budget_tokens"].as_u64().unwrap() < 8192);
    for cap in [0, 1, 1024, 2047] {
        opts.max_tokens = Some(cap);
        let error = anthropic(&provider, &opts).unwrap_err();
        assert!(error.contains("max_tokens"), "{error}");
    }
    opts.max_tokens = Some(2048);
    assert_eq!(anthropic(&provider, &opts).unwrap()["max_tokens"], 2048);
    opts.thinking = false;
    opts.max_tokens = Some(100);
    let body = anthropic(&provider, &opts).unwrap();
    assert_eq!(body["max_tokens"], 100);
    assert_eq!(body["temperature"], 0.7);
    assert_eq!(body["thinking"]["type"], "disabled");
}

#[test]
fn provider_payload_anthropic_capabilities_map_all_efforts() {
    let efforts = [
        ReasoningEffort::Minimal,
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
        ReasoningEffort::ExtraHigh,
        ReasoningEffort::Max,
        ReasoningEffort::Ultra,
    ];
    let cases = [
        (
            "claude-opus-4-5",
            ["low", "low", "medium", "high", "high", "high", "high"],
            "enabled",
        ),
        (
            "claude-opus-4-6",
            ["low", "low", "medium", "high", "high", "max", "max"],
            "adaptive",
        ),
        (
            "claude-sonnet-4-6",
            ["low", "low", "medium", "high", "high", "max", "max"],
            "adaptive",
        ),
        (
            "claude-opus-4-7",
            ["low", "low", "medium", "high", "xhigh", "max", "max"],
            "adaptive",
        ),
        (
            "claude-opus-5",
            ["low", "low", "medium", "high", "xhigh", "max", "max"],
            "adaptive",
        ),
        (
            "claude-opus-5-5",
            ["low", "low", "medium", "high", "xhigh", "max", "max"],
            "adaptive",
        ),
    ];
    for (model, expected, mode) in cases {
        for (effort, expected) in efforts.into_iter().zip(expected) {
            let mut opts = options(true, effort);
            if mode == "adaptive" {
                opts.max_tokens = Some(512);
            }
            let body = anthropic(&config(ProviderKind::Anthropic, model), &opts).unwrap();
            assert_eq!(body["output_config"]["effort"], expected, "{model}: {body}");
            assert_eq!(body["thinking"]["type"], mode, "{model}");
            assert!(body.get("temperature").is_none());
            if mode == "adaptive" {
                assert!(body["thinking"].get("budget_tokens").is_none());
                assert_eq!(body["max_tokens"], 512);
            }
        }
    }
    for model in [
        "claude-sonnet-4-5",
        "claude-haiku-4-5",
        "claude-sonnet-4-50-custom",
    ] {
        let body = anthropic(
            &config(ProviderKind::Anthropic, model),
            &options(false, ReasoningEffort::Max),
        )
        .unwrap();
        assert!(body.get("output_config").is_none(), "{model}: {body}");
    }
}

#[test]
fn provider_payload_anthropic_off_is_explicit_or_rejected_not_faked() {
    for model in [
        "claude-opus-4-6",
        "claude-opus-4-7",
        "claude-sonnet-4-6",
        "claude-sonnet-5",
    ] {
        let body = anthropic(
            &config(ProviderKind::Anthropic, model),
            &options(false, ReasoningEffort::High),
        )
        .unwrap();
        assert_eq!(body["thinking"]["type"], "disabled");
        if model == "claude-opus-4-7" || model == "claude-sonnet-5" {
            assert!(body.get("temperature").is_none());
        }
    }
    for model in [
        "claude-opus-5-5",
        "claude-sonnet-5-5",
        "claude-fable-5-1",
        "claude-mythos-5",
    ] {
        let error = anthropic(
            &config(ProviderKind::Anthropic, model),
            &options(false, ReasoningEffort::High),
        )
        .unwrap_err();
        assert!(
            error.contains("thinking") && error.contains(model),
            "{error}"
        );
    }
    for model in ["claude-opus-5", "claude-haiku-5-5"] {
        assert!(anthropic(
            &config(ProviderKind::Anthropic, model),
            &options(false, ReasoningEffort::High)
        )
        .is_ok());
        assert!(anthropic(
            &config(ProviderKind::Anthropic, model),
            &options(false, ReasoningEffort::Max)
        )
        .is_err());
    }
}

#[test]
fn provider_payload_extra_body_is_last_explicit_override() {
    for kind in [
        ProviderKind::Openai,
        ProviderKind::Ollama,
        ProviderKind::Anthropic,
    ] {
        let model = if kind == ProviderKind::Anthropic {
            "claude-sonnet-4-5"
        } else {
            "gpt-4o"
        };
        let mut provider = config(kind, model);
        provider.extra_body.insert("temperature".into(), json!(0.2));
        provider
            .extra_body
            .insert("reasoning_effort".into(), json!("custom-effort"));
        provider
            .extra_body
            .insert("thinking".into(), json!({"type":"vendor-mode"}));
        let body = if kind == ProviderKind::Anthropic {
            anthropic(&provider, &options(true, ReasoningEffort::Medium)).unwrap()
        } else {
            openai(&provider, &options(true, ReasoningEffort::Medium))
        };
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["reasoning_effort"], "custom-effort");
        assert_eq!(body["thinking"]["type"], "vendor-mode");
    }
}

#[test]
fn provider_payload_thinking_off_capability_for_background_requests() {
    for model in [
        "claude-opus-5-5",
        "claude-sonnet-5-5",
        "claude-fable-5",
        "claude-fable-5-1",
        "claude-mythos-5",
        "claude-mythos-5-1",
        "claude-mythos-preview",
    ] {
        assert!(!supports_thinking_off(
            &config(ProviderKind::Anthropic, model),
            ReasoningEffort::Low
        ));
    }
    for model in ["claude-opus-5", "claude-haiku-5-5"] {
        assert!(supports_thinking_off(
            &config(ProviderKind::Anthropic, model),
            ReasoningEffort::Low
        ));
        assert!(!supports_thinking_off(
            &config(ProviderKind::Anthropic, model),
            ReasoningEffort::ExtraHigh
        ));
    }
    assert!(supports_thinking_off(
        &config(ProviderKind::Openai, "claude-opus-5-5"),
        ReasoningEffort::Low
    ));
    for model in [
        "claude-opus-4-7-20260416",
        "claude-opus-4-8",
        "claude-sonnet-5",
        "claude-haiku-5-5",
    ] {
        let mut opts = options(true, ReasoningEffort::ExtraHigh);
        opts.max_tokens = Some(512);
        let body = anthropic(&config(ProviderKind::Anthropic, model), &opts).unwrap();
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "xhigh");
        opts.max_tokens = Some(0);
        assert!(anthropic(&config(ProviderKind::Anthropic, model), &opts).is_err());
    }
    for model in [
        "claude-opus-4-70-custom",
        "claude-opus-99",
        "claude-opus-4-7-custom",
    ] {
        let body = anthropic(
            &config(ProviderKind::Anthropic, model),
            &options(true, ReasoningEffort::Max),
        )
        .unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert!(body.get("output_config").is_none());
    }
    let body = anthropic(
        &config(ProviderKind::Anthropic, "claude-mythos-preview"),
        &options(true, ReasoningEffort::ExtraHigh),
    )
    .unwrap();
    assert_eq!(body["output_config"]["effort"], "high");
}

#[tokio::test]
async fn provider_payload_responses_only_rejected_before_network() {
    for model in [
        "gpt-5-pro",
        "gpt-5-pro-2025-10-06",
        "gpt-5.2-pro",
        "gpt-5.2-pro-2025-12-11",
    ] {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let error = stream_chat_completion(
            &config(ProviderKind::Openai, model),
            &json!([]),
            &json!([]),
            &options(true, ReasoningEffort::High),
            |_| {},
            |_| {},
            |_| {},
            cancel,
        )
        .await
        .unwrap_err();
        assert!(
            error.contains(model) && error.contains("Responses"),
            "{error}"
        );
    }
    for (model, host) in [
        ("gpt-5-pro", "https://custom.example/v1"),
        ("gpt-5.2-pro-custom", "https://api.openai.com/v1"),
        ("gpt-99", "https://api.openai.com/v1"),
    ] {
        let mut provider = config(ProviderKind::Openai, model);
        provider.base_url = host.into();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            stream_chat_completion(
                &provider,
                &json!([]),
                &json!([]),
                &options(true, ReasoningEffort::High),
                |_| {},
                |_| {},
                |_| {},
                cancel
            )
            .await
            .unwrap()
            .cancelled
        );
    }
}

#[test]
fn provider_payload_switch_from_anthropic_normalizes_openai_replay() {
    let messages = json!([
        {"role":"user","content":"Run the tool"},
        {"role":"assistant","content":[
            {"type":"thinking","thinking":"private","signature":"opaque"},
            {"type":"redacted_thinking","data":"opaque-redacted"},
            {"type":"text","text":"Checking"},
            {"type":"tool_use","id":"call_1","name":"lookup","input":{"q":"rust"}}
        ]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":[{"type":"text","text":"found"}]}]},
        {"role":"assistant","content":"Done","reasoning_content":"deepseek-only"},
        {"role":"tool","tool_call_id":"call_2","content":"existing-result"}
    ]);
    for kind in [
        ProviderKind::Openai,
        ProviderKind::Ollama,
        ProviderKind::Deepseek,
    ] {
        let body = openai_body(
            &config(kind, "custom-model"),
            &messages,
            &json!([]),
            &options(false, ReasoningEffort::High),
        );
        assert_eq!(body["messages"][1]["content"], "Checking", "{body}");
        assert_eq!(
            body["messages"][1]["tool_calls"][0],
            json!({"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"q\":\"rust\"}"}})
        );
        assert_eq!(
            body["messages"][2],
            json!({"role":"tool","tool_call_id":"call_1","content":"found"})
        );
        assert_eq!(body["messages"][4], messages[4]);
        assert_eq!(
            body["messages"][3].get("reasoning_content").is_some(),
            kind == ProviderKind::Deepseek
        );
    }
    let (_, native) = convert_anthropic_messages(&messages).unwrap();
    assert_eq!(
        native[1]["content"], messages[1]["content"],
        "signed blocks must stay unchanged on native replay"
    );
}

#[test]
fn provider_payload_deepseek_preserves_existing_thinking_controls() {
    let provider = config(ProviderKind::Deepseek, "deepseek-chat");
    for thinking in [false, true] {
        let body = openai(&provider, &options(thinking, ReasoningEffort::ExtraHigh));
        assert_eq!(
            body["thinking"]["type"],
            if thinking { "enabled" } else { "disabled" }
        );
        assert_eq!(body["reasoning_effort"], "xhigh");
        assert_eq!(body["max_tokens"], 8192);
    }
}
