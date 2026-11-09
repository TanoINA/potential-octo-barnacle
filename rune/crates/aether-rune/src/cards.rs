//! Native trace, reasoning, role-grouped agents and source-backed fenced plans.
//! Expansion is presentation-only; task status always comes from canonical text.
use crate::{
    app::AppState,
    panels::Panel,
    widgets::{button, caption},
};
use aether_core::agent::{ChatMessage, ToolCallItem};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use rune::{Color, El, GrowthSpec, ScrollSpec, TextStyle, col, row, text};
use rune_kit::{Kit, Progress};
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    ops::Range,
    sync::Arc,
};

const MAX_BODY: f32 = 320.0;
const CHUNK_BYTES: usize = 2048;
const PREVIEW_ROWS: usize = 100;

fn scoped_key(kind: &str, scope: &str, id: &str) -> String {
    let mut hash = DefaultHasher::new();
    (scope, id).hash(&mut hash);
    format!("{kind}-{:016x}", hash.finish())
}
pub fn trace_key(session: &str, message: &str) -> String {
    scoped_key("trace", session, message)
}
pub fn trace_open(expansion: &BTreeMap<String, (bool, bool)>, key: &str, running: bool) -> bool {
    expansion
        .get(key)
        .filter(|(phase, _)| *phase == running)
        .map_or(running, |(_, open)| *open)
}
fn failure(status: &str) -> bool {
    matches!(status, "error" | "failed" | "failure")
}
fn running(status: &str) -> bool {
    matches!(status, "running" | "pending" | "started")
}
fn red() -> Color {
    rune::color::hex("#f85149").expect("valid failure color")
}
fn frame(key: &str, kit: &Kit) -> El {
    col()
        .key(key)
        .w_full()
        .min_w(0.0)
        .gap(6.0)
        .padding(8.0)
        .fill(kit.c.layer_01)
        .border(1.0, kit.c.border_muted)
        .radius(8.0)
}
fn header(key: &str, title: &str, failed: usize, open: bool, phase: bool, kit: &Kit) -> El {
    let owned = key.to_string();
    let mut trigger = row()
        .key(format!("{key}-header"))
        .role("button")
        .label(if failed > 0 {
            format!("{title}, {failed} failed")
        } else {
            title.to_string()
        })
        .expanded(open)
        .focusable(true)
        .w_full()
        .min_w(0.0)
        .padding(6.0)
        .gap(8.0)
        .focus_ring(2.0, -2.0, kit.c.border_focus)
        .hover(kit.c.layer_02)
        .on_press_tx(move |s: &mut AppState, tx| {
            let open = trace_open(&s.shell.trace_expansion, &owned, phase);
            s.shell
                .trace_expansion
                .insert(owned.clone(), (phase, !open));
            tx.instant();
        })
        .child(caption(if open { "v" } else { ">" }, kit).a11y_hidden())
        .child(
            text(title)
                .font(TextStyle::new(13.0, 400.0))
                .color(kit.c.text_base)
                .grow(1.0)
                .min_w(0.0),
        );
    if failed > 0 {
        trigger = trigger.child(
            text(format!("{failed} failed"))
                .font(TextStyle::new(12.0, 400.0))
                .color(red()),
        );
    }
    trigger
}
fn viewport(key: &str, height: f32, body: El) -> El {
    // Rune requires an explicitly bounded viewport and a growth stack as FIRST child.
    // A small trace gets its own content budget, not a forced 320-point blank card.
    col()
        .key(format!("{key}-body"))
        .role("region")
        .label("Card details")
        .w_full()
        .min_w(0.0)
        .h(height.clamp(24.0, MAX_BODY))
        .min_h(0.0)
        .max_h(MAX_BODY)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(body)
}
fn visible_message<'a>(s: &'a AppState, session: &str, id: &str) -> Option<&'a ChatMessage> {
    let model = s.controller.model();
    let data = model
        .preview
        .as_ref()
        .map(|p| &p.data)
        .or(model.current.as_ref())?;
    (data.session.id == session)
        .then(|| data.messages.iter().find(|m| m.id == id))
        .flatten()
}
fn artifact(id: &str, key: &str, kit: &Kit) -> El {
    let id = id.to_string();
    button(
        &format!("{key}-artifact"),
        "Open artifact",
        kit,
        move |s, _| {
            s.controller.model_mut().selected_artifact_id = Some(id.clone());
            s.ui.open(Panel::Artifacts, &mut s.controller);
        },
    )
}

/// Split all source bytes into independently shaped rows. A megabyte single line
/// cannot become one enormous text shape; Unicode boundaries and literal copy survive.
fn source_chunks(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut at = 0;
    while at < source.len() {
        let mut end = at.saturating_add(CHUNK_BYTES).min(source.len());
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        if let Some(newline) = source[at..end].find('\n') {
            end = at + newline + 1;
        }
        ranges.push(at..end);
        at = end;
    }
    ranges
}
fn source_view(s: &AppState, source: &str, key: &str, kit: &Kit) -> El {
    let original: Arc<str> = source.into();
    let copy = original.clone();
    let mut root = col()
        .key(key)
        .w_full()
        .min_w(0.0)
        .gap(4.0)
        .selection_group(true);
    root = root.child(button(
        &format!("{key}-copy"),
        "Copy source",
        kit,
        move |_, tx| {
            tx.copy(copy.to_string());
        },
    ));
    if source.len() <= CHUNK_BYTES && source.lines().count() <= 12 {
        return root.child(
            text(source)
                .font(TextStyle::new(12.0, 400.0))
                .selectable(true)
                .w_full(),
        );
    }
    let ranges = Arc::new(source_chunks(source));
    let long = ranges.len() > PREVIEW_ROWS;
    let expanded = trace_open(&s.shell.trace_expansion, key, false);
    let count = if expanded {
        ranges.len()
    } else {
        ranges.len().min(PREVIEW_ROWS)
    };
    let chunks = ranges.clone();
    let version = scoped_key("source", "", source);
    let rows = col()
        .key(format!("{key}-rows"))
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            count,
            18.0,
            |_: &AppState, i| format!("chunk-{i}").into(),
            move |_: &AppState, i| {
                text(original[chunks[i].clone()].trim_end_matches(['\r', '\n']))
                    .font(TextStyle::new(12.0, 400.0))
                    .selectable(true)
                    .w_full()
            },
        )
        .virtual_rows_version(version);
    root = root.child(viewport(
        &format!("{key}-source"),
        (count.min(10) * 18) as f32,
        rows,
    ));
    if long {
        let key_owned = key.to_string();
        root = root
            .child(caption(
                if expanded {
                    format!("All {} source rows available; scroll to read", ranges.len())
                } else {
                    format!(
                        "Preview: first {PREVIEW_ROWS} of {} source rows; expand to read all",
                        ranges.len()
                    )
                },
                kit,
            ))
            .child(button(
                &format!("{key}-expand"),
                if expanded {
                    "Collapse source preview"
                } else {
                    "Expand full source"
                },
                kit,
                move |s, tx| {
                    let open = trace_open(&s.shell.trace_expansion, &key_owned, false);
                    s.shell
                        .trace_expansion
                        .insert(key_owned.clone(), (false, !open));
                    tx.instant();
                },
            ));
    }
    root
}
fn tool_details(s: &AppState, tool: &ToolCallItem, key: &str, kit: &Kit) -> El {
    let args = tool
        .arguments_raw
        .clone()
        .unwrap_or_else(|| tool.args.to_string());
    let mut body = col()
        .key(key)
        .w_full()
        .min_w(0.0)
        .gap(6.0)
        .child(caption(format!("{} — {}", tool.name, tool.status), kit))
        .child(caption("Arguments", kit))
        .child(source_view(s, &args, &format!("{key}-args"), kit));
    // Terminal events often replay the same final result. Show one retained copy
    // when one includes the other; keep genuinely distinct result/terminal data.
    let result = tool.result.as_deref().filter(|r| !r.is_empty());
    let terminal = tool.terminal_output.as_deref().filter(|r| !r.is_empty());
    if let Some(result) =
        result.filter(|r| terminal.is_none_or(|t| !t.contains(*r) || r.contains(t)))
    {
        body = body.child(caption("Result", kit)).child(source_view(
            s,
            result,
            &format!("{key}-result"),
            kit,
        ));
    }
    if let Some(terminal) = terminal.filter(|t| result.is_none_or(|r| !r.contains(*t))) {
        body = body
            .child(caption("Terminal output", kit))
            .child(source_view(s, terminal, &format!("{key}-terminal"), kit));
    }
    if let Some(exit) = tool.exit_code {
        body = body.child(caption(format!("Exit code: {exit}"), kit));
    }
    if let Some(duration) = tool.duration_ms {
        body = body.child(caption(format!("Duration: {duration} ms"), kit));
    }
    if let Some(id) = &tool.artifact_id {
        body = body.child(artifact(id, key, kit));
    }
    body
}
pub fn execution_trace(
    s: &AppState,
    session: &str,
    message: &ChatMessage,
    kit: &Kit,
) -> Option<El> {
    let tools = message.tool_calls.as_ref().filter(|t| !t.is_empty())?;
    let key = trace_key(session, &message.id);
    // Parent phase, NOT last-tool status: failed terminal traces collapse too.
    let phase = message.is_streaming.unwrap_or(false);
    let open = trace_open(&s.shell.trace_expansion, &key, phase);
    let failed = tools.iter().filter(|t| failure(&t.status)).count();
    let mut card = frame(&key, kit).child(header(
        &key,
        &format!("Execution trace — {} steps", tools.len()),
        failed,
        open,
        phase,
        kit,
    ));
    if open {
        let session_key = session.to_string();
        let message_key = message.id.clone();
        let session_build = session_key.clone();
        let message_build = message_key.clone();
        let prefix = key.clone();
        let body = col()
            .key(format!("{key}-rows"))
            .w_full()
            .gap(8.0)
            .growth(GrowthSpec::default())
            .virtual_rows(
                tools.len(),
                100.0,
                move |s: &AppState, i| {
                    visible_message(s, &session_key, &message_key)
                        .and_then(|m| m.tool_calls.as_ref())
                        .and_then(|t| t.get(i))
                        .map_or_else(
                            || format!("missing-{i}").into(),
                            |t| format!("tool-{}-{i}", t.id).into(),
                        )
                },
                move |s: &AppState, i| {
                    let kit = Kit::neutral(true);
                    visible_message(s, &session_build, &message_build)
                        .and_then(|m| m.tool_calls.as_ref())
                        .and_then(|t| t.get(i))
                        .map_or_else(col, |t| {
                            tool_details(s, t, &format!("{prefix}-tool-{}-{i}", t.id), &kit)
                        })
                },
            );
        let height = if tools.len() >= 8 {
            MAX_BODY
        } else {
            tools
                .iter()
                .map(|t| {
                    let lines = t.result.as_deref().unwrap_or("").lines().take(12).count()
                        + t.terminal_output
                            .as_deref()
                            .unwrap_or("")
                            .lines()
                            .take(12)
                            .count();
                    112.0 + lines as f32 * 18.0
                })
                .sum::<f32>()
                .min(MAX_BODY)
        };
        card = card.child(viewport(&key, height, body));
    }
    Some(card)
}
pub fn reasoning(s: &AppState, session: &str, message: &ChatMessage, kit: &Kit) -> Option<El> {
    let source = message.reasoning.as_deref().filter(|r| !r.is_empty())?;
    let key = scoped_key("reasoning", session, &message.id);
    // Reasoning stays independently collapsed even while the parent streams.
    let open = trace_open(&s.shell.trace_expansion, &key, false);
    let mut card = frame(&key, kit).child(header(&key, "Reasoning", 0, open, false, kit));
    if open {
        let content = if source.len() > 16 * 1024 {
            source_view(s, source, &format!("{key}-source"), kit)
        } else {
            s.rich
                .borrow_mut()
                .render(source, &format!("{key}-rich"), kit)
        };
        card = card.child(viewport(
            &key,
            (source.lines().count().max(1) * 22 + 20) as f32,
            col().w_full().growth(GrowthSpec::default()).child(content),
        ));
    }
    Some(card)
}

pub fn subagents(s: &AppState, kit: &Kit) -> Option<El> {
    let model = s.controller.model();
    if model.subagents.is_empty() {
        return None;
    }
    let scope = model
        .run_status
        .as_ref()
        .map(|r| r.run_id.as_str())
        .or_else(|| model.current.as_ref().map(|d| d.session.id.as_str()))
        .unwrap_or("agents");
    let mut groups = BTreeMap::new();
    for agent in &model.subagents {
        groups
            .entry(agent.role.as_str())
            .or_insert_with(Vec::new)
            .push(agent);
    }
    let phase = model.subagents.iter().any(|a| running(&a.status));
    let key = scoped_key("subagents", scope, "all");
    let open = trace_open(&s.shell.trace_expansion, &key, phase);
    let failed = model
        .subagents
        .iter()
        .filter(|a| failure(&a.status))
        .count();
    let mut card = frame(&key, kit).child(header(
        &key,
        &format!("Subagents — {} agents", model.subagents.len()),
        failed,
        open,
        phase,
        kit,
    ));
    if open {
        let mut body = col()
            .key(format!("{key}-roles"))
            .w_full()
            .gap(6.0)
            .growth(GrowthSpec::default())
            .virtualize(80.0)
            .child(
                caption(
                    "Flat role groups — parent metadata is not provided by the native backend",
                    kit,
                )
                .key(format!("{key}-flat-note")),
            );
        for (role, agents) in groups {
            let role_key = scoped_key("agent-role", scope, role);
            let role_phase = agents.iter().any(|a| running(&a.status));
            let role_open = trace_open(&s.shell.trace_expansion, &role_key, role_phase);
            let mut group = col().key(&role_key).w_full().gap(4.0).child(header(
                &role_key,
                &format!("{role} — {} agents", agents.len()),
                agents.iter().filter(|a| failure(&a.status)).count(),
                role_open,
                role_phase,
                kit,
            ));
            if role_open {
                for agent in agents {
                    let agent_key = scoped_key("agent", scope, &agent.id);
                    let agent_phase = running(&agent.status);
                    let agent_open = trace_open(&s.shell.trace_expansion, &agent_key, agent_phase);
                    let mut entry = col().key(&agent_key).w_full().gap(4.0).child(header(
                        &agent_key,
                        &format!("{} — {}", agent.id, agent.status),
                        usize::from(failure(&agent.status)),
                        agent_open,
                        agent_phase,
                        kit,
                    ));
                    if agent_open {
                        entry = entry.child(source_view(
                            s,
                            &agent.task,
                            &format!("{agent_key}-task"),
                            kit,
                        ));
                        if let Some(summary) = &agent.summary {
                            entry = entry.child(source_view(
                                s,
                                summary,
                                &format!("{agent_key}-summary"),
                                kit,
                            ));
                        }
                        if let Some(id) = &agent.artifact_id {
                            entry = entry.child(artifact(id, &agent_key, kit));
                        }
                    }
                    group = group.child(entry);
                }
            }
            body = body.child(group);
        }
        card = card.child(viewport(
            &key,
            (model.subagents.len() * 80 + 24) as f32,
            body,
        ));
    }
    Some(card)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanStatus {
    Pending,
    InProgress,
    Done,
}
impl PlanStatus {
    fn next(self) -> Self {
        match self {
            Self::Pending => Self::InProgress,
            Self::InProgress => Self::Done,
            Self::Done => Self::Pending,
        }
    }
    fn marker(self) -> &'static str {
        match self {
            Self::Pending => " ",
            Self::InProgress => "~",
            Self::Done => "x",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::InProgress => "In progress",
            Self::Done => "Done",
        }
    }
}
#[derive(Debug)]
pub struct PlanItem {
    pub text: String,
    pub marker_offset: usize,
    pub status: PlanStatus,
}
#[derive(Debug)]
enum PlanLine {
    Note(String),
    Item(usize),
}
#[derive(Debug)]
pub struct PlanBlock {
    pub range: Range<usize>,
    pub items: Vec<PlanItem>,
    lines: Vec<PlanLine>,
}
impl PlanBlock {
    fn completed(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == PlanStatus::Done)
            .count()
    }
}
fn plan_item(line: &str, offset: usize) -> Option<PlanItem> {
    let trimmed = line.trim_start();
    let prefix = if matches!(trimmed.as_bytes().first(), Some(b'-' | b'*' | b'+')) {
        1
    } else {
        let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || !matches!(trimmed.as_bytes().get(digits), Some(b'.' | b')')) {
            return None;
        }
        digits + 1
    };
    if !trimmed[prefix..].starts_with(char::is_whitespace) {
        return None;
    }
    let rest = trimmed[prefix..].trim_start();
    let status = match rest.as_bytes().get(..3)? {
        b"[ ]" => PlanStatus::Pending,
        b"[~]" | b"[-]" => PlanStatus::InProgress,
        b"[x]" | b"[X]" => PlanStatus::Done,
        _ => return None,
    };
    Some(PlanItem {
        text: rest[3..].trim().into(),
        marker_offset: offset + line.len() - rest.len(),
        status,
    })
}
pub fn plan_blocks(source: &str) -> Vec<PlanBlock> {
    let mut plans = Vec::new();
    let mut active: Option<PlanBlock> = None;
    // Parser offsets protect nested non-plan fences, indented code and literal plan examples.
    for (event, range) in Parser::new(source).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(language)))
                if language.trim().eq_ignore_ascii_case("plan") =>
            {
                active = Some(PlanBlock {
                    range,
                    items: Vec::new(),
                    lines: Vec::new(),
                });
            }
            Event::Text(_) if active.is_some() => {
                let plan = active.as_mut().unwrap();
                let mut offset = range.start;
                for line in source[range].split_inclusive('\n') {
                    if let Some(item) = plan_item(line, offset) {
                        plan.lines.push(PlanLine::Item(plan.items.len()));
                        plan.items.push(item);
                    } else if !line.trim().is_empty() {
                        plan.lines.push(PlanLine::Note(line.trim_end().into()));
                    }
                    offset += line.len();
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(plan) = active.take() {
                    plans.push(plan);
                }
            }
            _ => {}
        }
    }
    plans
}
#[derive(Clone, Debug)]
pub struct PlanChange {
    pub source: Arc<str>,
    pub marker_offset: usize,
    pub status: PlanStatus,
}
pub fn apply_plan_change(source: &mut String, change: &PlanChange) -> bool {
    if source.as_str() != change.source.as_ref() {
        return false;
    }
    let at = change.marker_offset;
    if !matches!(
        source.as_bytes().get(at..at.saturating_add(3)),
        Some(b"[ ]" | b"[~]" | b"[-]" | b"[x]" | b"[X]")
    ) {
        return false;
    }
    source.replace_range(at + 1..at + 2, change.status.marker());
    true
}
fn editable(s: &AppState, streaming: bool) -> bool {
    let m = s.controller.model();
    !streaming
        && !m.busy
        && !m.loading
        && m.preview.is_none()
        && !m.retry_save_available
        && s.shell.lifecycle == crate::lifecycle::Lifecycle::Open
}
fn persist(s: &mut AppState, session: &str, id: &str, expected: &str, updated: String) {
    if !editable(s, false) {
        s.set_error("Wait for the session operation to finish before editing tasks");
        return;
    }
    if s.controller
        .model()
        .current
        .as_ref()
        .map(|d| d.session.id.as_str())
        != Some(session)
    {
        s.set_error("Task belongs to a different session");
        return;
    }
    if let Err(error) = s.controller.update_message_content(id, expected, updated) {
        s.set_error(error);
    }
}
fn markdown_piece(
    s: &AppState,
    source: Arc<str>,
    range: Range<usize>,
    key: &str,
    session: &str,
    message: &ChatMessage,
    kit: &Kit,
) -> El {
    if !editable(s, message.is_streaming.unwrap_or(false)) {
        return s.rich.borrow_mut().render(&source[range], key, kit);
    }
    let start = range.start;
    let expected = source.clone();
    let id = message.id.clone();
    let session = session.to_string();
    s.rich.borrow_mut().render_with_tasks(
        &source[range],
        key,
        kit,
        move |s: &mut AppState, change| {
            let mut updated = expected.to_string();
            let change = crate::rich_content::TaskChange {
                source: expected.clone(),
                marker_offset: start + change.marker_offset,
                ..change
            };
            if !crate::rich_content::apply_task_change(&mut updated, &change) {
                s.set_error("Task source changed; reload before editing");
                return;
            }
            persist(s, &session, &id, &expected, updated);
        },
    )
}
fn plan_card(
    s: &AppState,
    source: Arc<str>,
    plan: &PlanBlock,
    key: &str,
    session: &str,
    message: &ChatMessage,
    kit: &Kit,
) -> El {
    let done = plan.completed();
    let mut card = frame(key, kit).child(caption("Execution plan", kit)).child(
        Progress::new(Some(if plan.items.is_empty() {
            0.0
        } else {
            done as f32 / plan.items.len() as f32
        }))
        .label(format!(
            "Plan progress: {done}/{} completed",
            plan.items.len()
        ))
        .show_value()
        .build(kit)
        .key(format!("{key}-progress")),
    );
    let copy = source[plan.range.clone()].to_string();
    card = card.child(button(
        &format!("{key}-copy"),
        "Copy plan source",
        kit,
        move |_, tx| tx.copy(copy.clone()),
    ));
    let disabled = !editable(s, message.is_streaming.unwrap_or(false));
    for line in &plan.lines {
        match line {
            PlanLine::Note(note) => {
                card = card.child(
                    text(note)
                        .selectable(true)
                        .font(TextStyle::new(12.0, 400.0)),
                )
            }
            PlanLine::Item(index) => {
                let item = &plan.items[*index];
                let change = PlanChange {
                    source: source.clone(),
                    marker_offset: item.marker_offset,
                    status: item.status.next(),
                };
                let id = message.id.clone();
                let session = session.to_string();
                let mut control = button(
                    &format!("{key}-task-{}", item.marker_offset),
                    &format!(
                        "[{}] {} — {}",
                        item.status.marker(),
                        item.text,
                        item.status.label()
                    ),
                    kit,
                    move |s, _| {
                        let mut updated = change.source.to_string();
                        if !apply_plan_change(&mut updated, &change) {
                            s.set_error("Plan source changed; reload before editing");
                            return;
                        }
                        persist(s, &session, &id, &change.source, updated);
                    },
                )
                .role("checkbox")
                .label(format!(
                    "{}: {}. Activate for {}",
                    item.text,
                    item.status.label(),
                    item.status.next().label()
                ))
                .checked(item.status == PlanStatus::Done)
                .disabled(disabled)
                .w_full();
                if item.status == PlanStatus::InProgress {
                    control = control.checked_mixed();
                }
                card = card.child(control);
            }
        }
    }
    card
}
/// Wrap the existing rich renderer only at protected plan-fence boundaries. Math,
/// ordinary code, source offset mapping and the setup waker remain owned by RichContent.
pub fn render_content(s: &AppState, session: &str, message: &ChatMessage, kit: &Kit) -> El {
    let source: Arc<str> = message.content.as_str().into();
    let plans = plan_blocks(&source);
    let key = format!("rich-{}", message.id);
    if plans.is_empty() {
        return markdown_piece(
            s,
            source.clone(),
            0..source.len(),
            &key,
            session,
            message,
            kit,
        );
    }
    let mut body = col().key(&key).w_full().gap(8.0);
    let mut at = 0;
    for plan in plans {
        if at < plan.range.start {
            body = body.child(markdown_piece(
                s,
                source.clone(),
                at..plan.range.start,
                &format!("{key}-markdown-{at}"),
                session,
                message,
                kit,
            ));
        }
        body = body.child(plan_card(
            s,
            source.clone(),
            &plan,
            &format!("{key}-plan-{}", plan.range.start),
            session,
            message,
            kit,
        ));
        at = plan.range.end;
    }
    if at < source.len() {
        body = body.child(markdown_piece(
            s,
            source.clone(),
            at..source.len(),
            &format!("{key}-markdown-{at}"),
            session,
            message,
            kit,
        ));
    }
    body
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
