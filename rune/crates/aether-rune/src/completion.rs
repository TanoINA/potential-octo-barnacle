//! Cursor-scoped native completion. Suggestions never execute commands or provider work.
use crate::{app::AppState, native, widgets::caption};
use pulldown_cmark::{Event, Parser, Tag};
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col};
use rune_kit::Kit;
use std::{collections::BTreeMap, ops::Range, path::{Component, Path, PathBuf}, time::{Duration, Instant}};

const DEBOUNCE: Duration = Duration::from_millis(150);
const COMMANDS: &[(&str, &str)] = &[
    ("/new", "Start a new conversation"), ("/clear", "Clear conversation history"),
    ("/undo", "Restore the latest user turn"), ("/retry", "Retry the latest turn"),
    ("/compact", "Compress conversation context"), ("/compress", "Alias for compact"),
    ("/stop", "Stop the active run"), ("/review", "Review completed work"),
    ("/reflect", "Alias for review"), ("/refine", "Alias for review"),
    ("/title", "Rename this conversation"), ("/pin", "Pin this conversation"),
    ("/export", "Export with confirmation"), ("/save", "Export with confirmation"),
    ("/model", "Choose a model"), ("/settings", "Open settings"),
    ("/history", "Browse conversations"), ("/artifacts", "Browse artifacts"),
    ("/tools", "Inspect tools"), ("/memory", "Workspace instructions"),
    ("/terminal", "Open the native shell"), ("/git", "Workspace Git status"),
    ("/tasks", "Inspect background tasks"), ("/skills", "Browse skills"),
    ("/init", "Initialize workspace instructions"), ("/plan", "Plan a task on send"),
    ("/ask", "Read-only request on send"), ("/agent", "Agent request on send"),
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind { Mention, Slash }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token { pub kind: Kind, pub range: Range<usize>, pub query: String }
/// Byte offsets match the pinned Rune TextEdit::focus/anchor, not character counts.
pub fn token(text: &str, cursor: usize) -> Option<Token> {
    let before = text.get(..cursor)?;
    let start = before.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i,c)| i+c.len_utf8());
    let word = &before[start..];
    let (kind, query) = if let Some(q) = word.strip_prefix('@') {
        if q.contains(['@', '"', '`']) { return None; }
        (Kind::Mention, q.to_owned())
    } else if let Some(q) = word.strip_prefix('/') {
        if !before[..start].trim().is_empty() { return None; }
        (Kind::Slash, q.to_ascii_lowercase())
    } else { return None; };
    // CommonMark's byte ranges protect inline, fenced, and indented code.
    if Parser::new(text).into_offset_iter().any(|(event, range)|
        matches!(event, Event::Code(_) | Event::Start(Tag::CodeBlock(_))) && range.contains(&start)) {
        return None;
    }
    Some(Token { kind, range: start..cursor, query })
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    session: Option<String>, workspace: String, navigation: u64, native_generation: u64,
    text: String, anchor: usize, cursor: usize,
}
impl Snapshot {
    fn capture(s: &AppState) -> Self {
        let m = s.controller.model();
        Self { session: m.current.as_ref().map(|d|d.session.id.clone()), workspace: m.workspace.clone(),
            navigation: s.controller.navigation_generation(), native_generation: s.shell.native.generation,
            text: s.shell.composer.text.clone(), anchor: s.shell.composer.anchor, cursor: s.shell.composer.focus }
    }
    fn current(&self, s: &AppState) -> bool { allowed(s) && *self == Self::capture(s) }
}
#[derive(Clone, Debug)]
pub struct Suggestion { pub label: String, pub description: String, path: Option<PathBuf> }
struct Inspection { id: u64, snapshot: Snapshot, origin: native::Origin, relative: String, path: PathBuf }
#[derive(Default)]
pub struct CompletionState {
    snapshot: Option<Snapshot>, token: Option<Token>, epoch: u64,
    due: Option<Instant>, pending: BTreeMap<u64, (u64, Snapshot)>,
    inspection: Option<Inspection>, dismissed: Option<Snapshot>,
    pub items: Vec<Suggestion>, pub selected: usize, pub status: Option<String>,
}
fn allowed(s: &AppState) -> bool {
    s.shell.lifecycle == crate::lifecycle::Lifecycle::Open && s.ui.panel.is_none()
        && s.shell.export.confirmation.is_none() && s.shell.queue.edit.is_none()
        && s.shell.native.preview.is_none() && s.controller.model().preview.is_none()
        && !s.controller.model().loading && s.shell.composer.preedit.is_none()
        && s.shell.composer.anchor == s.shell.composer.focus
}
fn observe(s: &mut AppState, tx: &mut Tx) -> bool {
    // Navigation is a separate ABA fence from native attachment generation.
    native::origin(s);
    let snapshot = allowed(s).then(|| Snapshot::capture(s));
    if snapshot == s.shell.completion.snapshot { return false; }
    let c = &mut s.shell.completion;
    c.epoch = c.epoch.wrapping_add(1);
    c.items.clear(); c.status = None; c.selected = 0; c.due = None; c.token = None;
    if let Some(i) = c.inspection.take() { s.shell.native.pending.remove(&i.id); s.controller.take_result(i.id); }
    c.snapshot = snapshot.clone();
    if let Some(snapshot) = snapshot {
        let dismissed = c.dismissed.as_ref().is_some_and(|d| d.text == snapshot.text && d.navigation == snapshot.navigation && d.session == snapshot.session && d.workspace == snapshot.workspace);
        if !dismissed {
            c.dismissed = None;
            c.token = token(&snapshot.text, snapshot.cursor);
            if let Some(t) = &c.token {
                match t.kind {
                    Kind::Slash => c.items = COMMANDS.iter().filter(|(name,_)|name[1..].starts_with(&t.query)).take(8)
                        .map(|(name,description)|Suggestion { label: (*name).into(), description: (*description).into(), path: None }).collect(),
                    Kind::Mention if !snapshot.workspace.is_empty() => {
                        c.due = Some(Instant::now() + DEBOUNCE);
                        c.status = Some("Searching workspace files...".into());
                        tx.wake_after(DEBOUNCE);
                    }
                    _ => c.status = Some("Choose a workspace to mention files".into()),
                }
            }
        }
    }
    tx.scroll_to_y("completion-list",0.0);
    true
}
pub fn input_changed(s: &mut AppState, tx: &mut Tx) {
    s.controller.model_mut().draft = s.shell.composer.text.clone();
    observe(s, tx);
}
fn relative_path(workspace: &str, relative: &str) -> Option<PathBuf> {
    let p = Path::new(relative);
    if relative.is_empty() || relative.chars().any(char::is_control) || p.is_absolute()
        || !p.components().all(|c|matches!(c, Component::Normal(_))) { return None; }
    Some(Path::new(workspace).join(p))
}
fn same_path(a: &Path, b: &Path) -> bool {
    let clean = |p: &Path| { let s=p.to_string_lossy().replace('\\', "/"); let s=s.strip_prefix("//?/").unwrap_or(&s).to_owned(); if cfg!(windows) {s.to_lowercase()} else {s} };
    clean(a) == clean(b)
}
fn insert(s: &mut AppState, snapshot: &Snapshot, value: &str) -> bool {
    if !snapshot.current(s) { return false; }
    let Some(t) = token(&snapshot.text, snapshot.cursor) else { return false; };
    let edit = &mut s.shell.composer;
    edit.anchor = t.range.start; edit.focus = t.range.end;
    // The pinned Rune API inserts by replacing its byte-range selection, with undo.
    let value = if snapshot.text[snapshot.cursor..].chars().next().is_some_and(char::is_whitespace) { value.trim_end_matches(' ') } else { value };
    edit.insert(value);
    s.controller.model_mut().draft = edit.text.clone();
    s.shell.completion.items.clear(); s.shell.completion.token = None; s.shell.completion.due = None;
    s.shell.completion.status = None;
    true
}
/// Poll before native::sync: only a valid inspection may mutate the draft or attach.
pub fn sync(s: &mut AppState, tx: &mut Tx) -> bool {
    let mut changed = observe(s, tx);
    if let Some(due) = s.shell.completion.due {
        if Instant::now() >= due {
            s.shell.completion.due = None;
            if let (Some(snapshot), Some(t)) = (s.shell.completion.snapshot.clone(), s.shell.completion.token.clone()) {
                let id = s.controller.dispatch("list_workspace_files", serde_json::json!({"query":t.query,"limit":12}));
                s.shell.completion.pending.insert(id, (s.shell.completion.epoch, snapshot));
            }
            changed = true;
        } else { tx.wake_after(due.saturating_duration_since(Instant::now())); }
    }
    let ready: Vec<_> = s.shell.completion.pending.keys().copied().filter(|id|s.controller.model().results.contains_key(id)).collect();
    for id in ready {
        let (epoch, snapshot) = s.shell.completion.pending.remove(&id).unwrap();
        let result = s.controller.take_result(id).unwrap();
        if epoch != s.shell.completion.epoch || !snapshot.current(s) || s.shell.completion.dismissed.is_some() { continue; }
        changed = true;
        s.shell.completion.items.clear();
        if let Some(error) = result.error { s.shell.completion.status = Some(format!("File search: {error}")); continue; }
        match result.value.and_then(|v|serde_json::from_value::<Vec<String>>(v).ok()) {
            Some(paths) => {
                s.shell.completion.items = paths.into_iter().take(12).filter_map(|relative| {
                    let path = relative_path(&snapshot.workspace, &relative)?;
                    Some(Suggestion { label: relative, description: "Attach workspace file".into(), path: Some(path) })
                }).collect();
                s.shell.completion.status = s.shell.completion.items.is_empty().then(||"No matching files".into());
                tx.scroll_to_y("completion-list",0.0);
            }
            None => s.shell.completion.status = Some("Invalid file search response".into()),
        }
    }
    if s.shell.completion.inspection.as_ref().is_some_and(|i|s.controller.model().results.contains_key(&i.id)) {
        let i = s.shell.completion.inspection.take().unwrap();
        let valid = i.snapshot.current(s) && native::origin(s) == i.origin;
        let result = s.controller.model().results.get(&i.id).unwrap();
        let inspected = result.error.is_none() && result.value.clone().and_then(|v|serde_json::from_value::<Vec<aether_core::agent::AttachedContext>>(v).ok())
            .is_some_and(|files|files.len()==1 && !files[0].is_dir && same_path(Path::new(&files[0].path), &i.path));
        if valid && inspected {
            let display = if i.relative.chars().any(char::is_whitespace) || i.relative.contains('"') { format!("@{} ",serde_json::to_string(&i.relative).unwrap()) } else { format!("@{} ",i.relative) };
            insert(s, &i.snapshot, &display);
            // Native sync consumes this validated reply, including image preparation.
        } else {
            s.shell.native.pending.remove(&i.id); s.controller.take_result(i.id);
            if valid { s.shell.completion.status = Some("File inspection failed; draft retained".into()); }
        }
        changed = true;
    }
    changed
}
fn accept(s: &mut AppState, index: usize, tx: &mut Tx) {
    if !allowed(s) || s.shell.completion.inspection.is_some() { return; }
    let Some(snapshot) = s.shell.completion.snapshot.clone().filter(|v|v.current(s)) else { return; };
    let Some(item) = s.shell.completion.items.get(index).cloned() else { return; };
    if let Some(path) = item.path {
        let origin = native::origin(s);
        let previous = s.shell.native.pending.keys().next_back().copied();
        native::attach_paths(s, std::slice::from_ref(&path));
        if let Some(id) = s.shell.native.pending.keys().next_back().copied().filter(|id|Some(*id)!=previous) {
            s.shell.completion.inspection = Some(Inspection { id, snapshot, origin, relative: item.label, path });
            s.shell.completion.status = Some("Inspecting selected file...".into());
        }
    } else { insert(s, &snapshot, &format!("{} ",item.label)); }
    s.shell.composer.focused = true; tx.wake_after(Duration::ZERO);
}
pub fn on_key(s: &mut AppState, event: &KeyEvent, tx: &mut Tx) -> bool {
    if !allowed(s) || event.mods.cmd || event.mods.ctrl || event.mods.alt || event.mods.shift { return false; }
    observe(s, tx);
    if s.shell.completion.token.is_none() { return false; }
    match event.key {
        Key::Escape => {
            s.shell.completion.dismissed = s.shell.completion.snapshot.clone();
            s.shell.completion.token = None; s.shell.completion.items.clear(); s.shell.completion.status = None; s.shell.completion.due = None;
            if let Some(i)=s.shell.completion.inspection.take() {s.shell.native.pending.remove(&i.id);s.controller.take_result(i.id);}
            true
        }
        Key::Up | Key::Down if !s.shell.completion.items.is_empty() => {
            let c = &mut s.shell.completion;
            c.selected = if event.key == Key::Down {(c.selected+1)%c.items.len()} else {(c.selected+c.items.len()-1)%c.items.len()};
            tx.scroll_into_view_in("completion-list",format!("completion-item-{}",c.selected)); true
        }
        Key::Enter | Key::Tab if !s.shell.completion.items.is_empty() || s.shell.completion.due.is_some() || !s.shell.completion.pending.is_empty() || s.shell.completion.inspection.is_some() || s.shell.completion.status.is_some() => {
            if !tx.key_repeat() { accept(s, s.shell.completion.selected, tx); } true
        }
        _ => false,
    }
}
pub fn render(s: &AppState, kit: &Kit) -> Option<El> {
    let c = &s.shell.completion;
    if !allowed(s) || c.token.is_none() || !c.snapshot.as_ref().is_some_and(|v|v.current(s)) { return None; }
    if c.items.is_empty() && c.status.is_none() {return None;}
    let mut popup = col().key("completion-list").role("listbox").label("Composer suggestions").w_full()
        .h((c.items.len() as f32 * 32.0 + if c.status.is_some(){28.0}else{0.0}).clamp(28.0,160.0))
        .scroll(ScrollSpec {stick_bottom:false}).fill(kit.c.layer_01).border(1.0,kit.c.border_muted).radius(7.0);
    for (index, item) in c.items.iter().enumerate() {
        let key = format!("completion-item-{index}");
        let snapshot = c.snapshot.clone();let epoch=c.epoch;
        popup = popup.child(col().key(key).role("option").label(format!("{} - {}",item.label,item.description))
            .h(32.0).min_h(32.0).w_full().padding(6.0).fill(if index==c.selected {kit.c.layer_02}else{kit.c.layer_01})
            .on_press_tx(move |s: &mut AppState, tx| {if epoch==s.shell.completion.epoch && snapshot == s.shell.completion.snapshot {accept(s,index,tx);}})
            .child(caption(format!("{}  -  {}",item.label,item.description),kit)));
    }
    if let Some(status)=&c.status {popup=popup.child(caption(status,kit).role("status"));}
    Some(popup)
}