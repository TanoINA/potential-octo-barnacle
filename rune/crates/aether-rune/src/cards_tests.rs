use super::*;
use rune::{Env, Runtime};
use std::sync::Once;

#[test]
fn trace_follows_parent_phase_even_when_last_tool_failed() {
    let mut expansion = BTreeMap::new();
    assert!(trace_open(&expansion, "m", true));
    assert!(!trace_open(&expansion, "m", false));
    expansion.insert("m".into(), (true, true));
    assert!(trace_open(&expansion, "m", true));
    assert!(!trace_open(&expansion, "m", false));
    expansion.insert("m".into(), (false, true));
    assert!(trace_open(&expansion, "m", false));
    assert!(trace_open(&expansion, "m", true));
    expansion.insert("m".into(), (true, false));
    assert!(!trace_open(&expansion, "m", true));
    assert!(!trace_open(&expansion, "m", false));
    assert!(!trace_open(&expansion, "other", false));
}

#[test]
fn plan_parser_uses_whole_source_utf8_offsets_and_protects_other_fences() {
    let source = "π $x$\n\n````text\n```plan\n- [x] literal\n```\n````\n\n```plan\r\n- [ ] α\r\n2. [~] β\r\n* [X] γ\r\n```\n\n```rust\nlet x = 1;\n```";
    let plans = plan_blocks(source);
    assert_eq!(plans.len(), 1);
    let p = &plans[0];
    assert_eq!(p.items.len(), 3);
    assert_eq!(p.completed(), 1);
    assert_eq!(p.items[0].status, PlanStatus::Pending);
    assert_eq!(p.items[1].status, PlanStatus::InProgress);
    assert_eq!(p.items[2].status, PlanStatus::Done);
    for item in &p.items {
        assert_eq!(&source[item.marker_offset..item.marker_offset + 1], "[");
    }
    assert!(source[p.range.clone()].starts_with("```plan\r\n"));
}

#[test]
fn plan_changes_are_source_derived_and_reject_stale_stream_snapshots() {
    let source = "Before $x$\n\n```plan\n- [ ] repeat\n- [~] repeat\n- [x] final\n```\nAfter";
    let plan = plan_blocks(source).remove(0);
    let mut updated = source.to_string();
    let change = PlanChange {
        source: source.into(),
        marker_offset: plan.items[0].marker_offset,
        status: PlanStatus::InProgress,
    };
    assert!(apply_plan_change(&mut updated, &change));
    assert!(!apply_plan_change(&mut updated, &change));
    let next = plan_blocks(&updated).remove(0);
    assert_eq!(next.items[0].status, PlanStatus::InProgress);
    assert_eq!(next.items[1].status, PlanStatus::InProgress);
    let streamed = updated.replace("- [~] repeat", "- [x] repeat");
    assert_eq!(plan_blocks(&streamed)[0].completed(), 3);
    assert!(!apply_plan_change(&mut streamed.clone(), &change));
    assert_eq!(PlanStatus::Pending.next(), PlanStatus::InProgress);
    assert_eq!(PlanStatus::InProgress.next(), PlanStatus::Done);
    assert_eq!(PlanStatus::Done.next(), PlanStatus::Pending);
}

#[test]
fn chunk_ranges_retain_every_byte_including_very_long_utf8_lines() {
    let source = format!("{}\r\nlast\n", "α".repeat(100_000));
    let ranges = source_chunks(&source);
    assert!(ranges.len() > 50);
    assert!(ranges.iter().all(|r| r.len() <= CHUNK_BYTES));
    assert_eq!(
        ranges
            .iter()
            .map(|r| &source[r.clone()])
            .collect::<String>(),
        source
    );
}

fn state() -> AppState {
    static PROFILE: Once = Once::new();
    PROFILE.call_once(|| {
        // Unit-test modules share the process-wide profile. Reuse the disposable
        // root installed by neighboring shell tests; never silently ignore any
        // other setup error. Standalone card tests still install their own root.
        if let Err(error) = aether_core::agent::profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-native-cards-{}", uuid::Uuid::new_v4())),
        ) {
            assert_eq!(error, "Storage root already configured");
        }
    });
    let mut state = AppState::new();
    state.controller.new_session();
    let current = state.controller.model_mut().current.as_mut().unwrap();
    current.messages = serde_json::from_value(serde_json::json!([{
        "id":"cards-message", "role":"assistant", "timestamp":1,
        "content":"Ordinary **Markdown**.\n\n```plan\n- [ ] Pending\n- [~] Active\n- [x] Done\n```\n\n```rust\nlet native = true;\n```",
        "reasoning":"Private collapsed reasoning", "isStreaming":true,
        "toolCalls":[{"id":"tool-1","name":"read","args":{"path":"test"},"status":"error","result":"result retained","terminalOutput":"result retained"}]
    }])).unwrap();
    state
}
fn click(rt: &mut Runtime<AppState>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.016);
}
fn texts(rt: &Runtime<AppState>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.text)
        .collect()
}
#[test]
fn actual_main_view_mounts_plan_trace_and_independently_collapsed_reasoning() {
    let state = state();
    let session = state
        .controller
        .model()
        .current
        .as_ref()
        .unwrap()
        .session
        .id
        .clone();
    let trace = trace_key(&session, "cards-message");
    let mut rt = Runtime::new(
        state,
        crate::view::view,
        (1280.0, 1800.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    assert!(rt.elements().iter().any(|e| e.role == "progressbar"));
    assert!(texts(&rt).iter().any(|t| t.contains("let native = true;")));
    assert!(
        !texts(&rt)
            .iter()
            .any(|t| t.contains("Private collapsed reasoning"))
    );
    assert_eq!(
        texts(&rt)
            .iter()
            .filter(|t| t.as_str() == "result retained")
            .count(),
        1
    );
    let body = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(format!("{trace}-body").as_str()))
        .unwrap();
    assert!(
        body.rect[3] > 0.0 && body.rect[3] <= 320.0,
        "{:?}",
        body.rect
    );
    rt.update(|s, _| {
        s.controller.model_mut().current.as_mut().unwrap().messages[0].is_streaming = Some(false)
    });
    rt.frame(0.016);
    assert!(!texts(&rt).iter().any(|t| t == "result retained"));
    assert!(texts(&rt).iter().any(|t| t.contains("1 failed")));
    click(&mut rt, &format!("{trace}-header"));
    assert!(texts(&rt).iter().any(|t| t == "result retained"));
    rt.invalidate();
    rt.frame(0.016);
    assert!(texts(&rt).iter().any(|t| t == "result retained"));
}
#[test]
fn main_view_plan_clicks_persist_whole_source_only_after_native_ack_and_disable_while_busy() {
    let mut state = state();
    let mut data = state.controller.model().current.clone().unwrap();
    data.session.id = format!("sess_{}", uuid::Uuid::new_v4());
    data.messages[0].is_streaming = Some(false);
    data.messages[0].content =
        "Before $x$.\n\n```plan\n- [ ] Native task\n- [~] Active\n```\n\n- [ ] Ordinary task\n"
            .into();
    let original = data.messages[0].content.clone();
    let session = data.session.id.clone();
    state
        .controller
        .runtime
        .block_on(aether_core::agent::storage::save_session(data))
        .unwrap();
    state.controller.new_session();
    let op = state.controller.open_session(&session);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        state.controller.poll();
        if let Some(result) = state.controller.take_result(op) {
            assert!(result.error.is_none(), "{:?}", result.error);
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let mut rt = Runtime::new(
        state,
        crate::view::view,
        (1280.0, 1800.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    let copy_key = rt
        .elements()
        .into_iter()
        .find(|e| e.label.as_deref() == Some("Copy plan source"))
        .unwrap()
        .key
        .unwrap();
    click(&mut rt, &copy_key);
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some("```plan\n- [ ] Native task\n- [~] Active\n```")
    );
    let task = rt
        .elements()
        .into_iter()
        .find(|e| {
            e.role == "checkbox"
                && e.label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("Native task:"))
        })
        .unwrap()
        .key
        .unwrap();
    click(&mut rt, &task);
    assert!(rt.state.controller.model().busy);
    assert_eq!(
        rt.state
            .controller
            .model()
            .current
            .as_ref()
            .unwrap()
            .messages[0]
            .content,
        original
    );
    click(&mut rt, &task); // Disabled until the first native acknowledgment, not an optimistic second write.
    while rt.state.controller.model().busy {
        rt.state.controller.poll();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        rt.state.controller.model().error.is_none(),
        "{:?}",
        rt.state.controller.model().error
    );
    rt.invalidate();
    rt.frame(0.016);
    let expected = original.replacen("[ ]", "[~]", 1);
    assert_eq!(
        rt.state
            .controller
            .model()
            .current
            .as_ref()
            .unwrap()
            .messages[0]
            .content,
        expected
    );
    let saved = rt
        .state
        .controller
        .runtime
        .block_on(aether_core::agent::storage::load_session(&session))
        .unwrap();
    assert_eq!(saved.messages[0].content, expected);
    // Ordinary Markdown task AFTER the removed fence retains the whole-message byte offset.
    let ordinary = rt
        .elements()
        .into_iter()
        .find(|e| e.role == "checkbox" && e.label.as_deref() == Some("Ordinary task"))
        .unwrap();
    let [x, y, w, h] = ordinary.rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.016);
    assert!(rt.state.controller.model().busy);
    while rt.state.controller.model().busy {
        rt.state.controller.poll();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let saved = rt
        .state
        .controller
        .runtime
        .block_on(aether_core::agent::storage::load_session(&session))
        .unwrap();
    assert_eq!(
        saved.messages[0].content,
        expected.replace("- [ ] Ordinary", "- [x] Ordinary")
    );
}

fn source_fixture(s: &AppState, _: &mut rune::Cx<AppState>) -> El {
    let source = &s.controller.model().current.as_ref().unwrap().messages[0].content;
    source_view(s, source, "large-output", &Kit::neutral(true))
}
#[test]
fn large_source_is_lazy_full_copy_and_last_row_remain_accessible() {
    let mut state = state();
    let original = format!("{}LAST-ROW-SENTINEL", "output line\n".repeat(1000));
    state
        .controller
        .model_mut()
        .current
        .as_mut()
        .unwrap()
        .messages[0]
        .content = original.clone();
    let mut rt = Runtime::new(state, source_fixture, (700.0, 600.0), Env::new(true, 1.0));
    rt.frame(0.0);
    assert!(
        rt.elements().len() < 200,
        "virtual rows must avoid building the full output"
    );
    assert!(!texts(&rt).iter().any(|t| t.contains("LAST-ROW-SENTINEL")));
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    click(&mut rt, "large-output-copy");
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some(original.as_str())
    );
    click(&mut rt, "large-output-expand");
    rt.update(|_, tx| {
        tx.instant();
        tx.scroll_into_view("chunk-1000");
    });
    for _ in 0..30 {
        rt.frame(0.016);
    }
    assert!(
        texts(&rt).iter().any(|t| t.contains("LAST-ROW-SENTINEL")),
        "{:?}",
        texts(&rt)
    );
    assert!(rt.elements().len() < 200);
}

fn agents_fixture(s: &AppState, _: &mut rune::Cx<AppState>) -> El {
    subagents(s, &Kit::neutral(true)).unwrap()
}
#[test]
fn agents_are_role_grouped_without_invented_parent_edges() {
    let mut state = state();
    state.controller.model_mut().subagents = serde_json::from_value(serde_json::json!([
        {"id":"worker-1","role":"Research","task":"Read sources","status":"running","startedAt":1},
        {"id":"worker-2","role":"Research","task":"Verify sources","status":"error","startedAt":1,"summary":"failed source"},
        {"id":"worker-3","role":"Review","task":"Review output","status":"done","startedAt":1}
    ])).unwrap();
    let mut rt = Runtime::new(state, agents_fixture, (700.0, 600.0), Env::new(true, 1.0));
    rt.frame(0.0);
    assert!(texts(&rt).iter().any(|t| t.contains("Research — 2 agents")));
    assert!(
        texts(&rt)
            .iter()
            .any(|t| t.contains("parent metadata is not provided"))
    );
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("1 failed") && e.text_color == Some(red()))
    );
    let failed = rt
        .elements()
        .into_iter()
        .find(|e| {
            e.role == "button"
                && e.label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("worker-2 — error"))
        })
        .unwrap()
        .key
        .unwrap();
    click(&mut rt, &failed);
    assert!(texts(&rt).iter().any(|t| t == "failed source"));
}

fn trace_fixture(s: &AppState, _: &mut rune::Cx<AppState>) -> El {
    let data = s.controller.model().current.as_ref().unwrap();
    execution_trace(s, &data.session.id, &data.messages[0], &Kit::neutral(true)).unwrap()
}
#[test]
fn one_hundred_seventy_seven_steps_are_counted_bounded_and_lazily_reachable() {
    let mut state = state();
    let data = state.controller.model_mut().current.as_mut().unwrap();
    let session = data.session.id.clone();
    data.messages[0].tool_calls = Some(
        (0..177)
            .map(|n| ToolCallItem {
                id: format!("step-{n}"),
                name: format!("native-step-{n}"),
                args: serde_json::json!({"step":n}),
                status: if n == 176 {
                    "error".into()
                } else {
                    "success".into()
                },
                result: Some(format!("retained-result-{n}")),
                ..Default::default()
            })
            .collect(),
    );
    let key = trace_key(&session, "cards-message");
    let mut rt = Runtime::new(state, trace_fixture, (700.0, 600.0), Env::new(true, 1.0));
    rt.frame(0.0);
    assert!(texts(&rt).iter().any(|t| t.contains("177 steps")));
    let body = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(format!("{key}-body").as_str()))
        .unwrap();
    assert!(
        body.rect[3] <= 320.0 && body.rect[3] >= 319.0,
        "{:?}",
        body.rect
    );
    assert!(
        rt.elements().len() < 600,
        "tool details should be index-built"
    );
    rt.update(|_, tx| {
        tx.instant();
        tx.scroll_into_view("tool-step-176-176");
    });
    for _ in 0..30 {
        rt.frame(0.016);
    }
    assert!(
        texts(&rt).iter().any(|t| t == "retained-result-176"),
        "{:?}",
        texts(&rt)
    );
    rt.update(|s, tx| {
        tx.instant();
        s.controller.model_mut().current.as_mut().unwrap().messages[0].is_streaming = Some(false);
    });
    rt.frame(0.016);
    assert!(!texts(&rt).iter().any(|t| t == "retained-result-176"));
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("1 failed") && e.text_color == Some(red()))
    );
    click(&mut rt, &format!("{key}-header"));
    assert!(trace_open(&rt.state.shell.trace_expansion, &key, false));
}
