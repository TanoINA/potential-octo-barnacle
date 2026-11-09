//! Actual Rune editor/Controller/Backend integration; isolated profile, no provider.
use aether_rune::{AppState, app, completion, native};
use rune::{Env, Key, KeyEvent, Mods, Runtime, TextEdit, Tx};
use std::{sync::Once, time::{Duration, Instant}};
static PROFILE: Once = Once::new();
fn runtime() -> Runtime<AppState> {
    PROFILE.call_once(|| { aether_core::agent::profile::configure_storage_root(&std::env::temp_dir().join(format!("aether-completion-{}",uuid::Uuid::new_v4()))).unwrap(); });
    let mut state = AppState::new();
    let end=Instant::now()+Duration::from_secs(5);
    while !state.controller.model().pending_commands.is_empty() {state.controller.poll();assert!(Instant::now()<end);std::thread::sleep(Duration::from_millis(5));}
    state.controller.model_mut().settings.providers.clear();
    let mut rt=Runtime::new(state,aether_rune::view::view,(900.0,600.0),Env::new(true,1.0));
    rt.on_key=app::spec().on_key;rt.frame(0.0);
    let (x,y)=rt.find("composer").unwrap();rt.pointer_down(x,y);rt.pointer_up(x,y);rt.frame(0.016);rt
}
fn key(rt:&mut Runtime<AppState>,key:Key) {rt.key(KeyEvent{key,mods:Mods::default()});rt.frame(0.016);}
fn draft(rt:&mut Runtime<AppState>,text:&str) {rt.state.shell.composer=TextEdit::new(text);rt.state.shell.composer.anchor=text.len();rt.state.shell.composer.focus=text.len();rt.state.shell.composer.focused=true;completion::input_changed(&mut rt.state,&mut Tx::default());rt.invalidate();rt.frame(0.016);}
fn pump(rt:&mut Runtime<AppState>) {(app::spec().poll.unwrap())(&mut rt.state,0.0,&mut Tx::default());rt.invalidate();rt.frame(0.016);}
fn wait(rt:&mut Runtime<AppState>,pred:impl Fn(&AppState)->bool) {let end=Instant::now()+Duration::from_secs(5);while !pred(&rt.state){pump(rt);assert!(Instant::now()<end,"timed out: {:?}",rt.state.shell.completion.status);std::thread::sleep(Duration::from_millis(5));}}
#[test]
fn cursor_unicode_and_commonmark_protection() {
    let source="雪🙂 read @src/main.rs trailing";let cursor=source.find(".rs").unwrap();let t=completion::token(source,cursor).unwrap();assert_eq!(t.query,"src/main");assert_eq!(&source[t.range],"@src/main");
    assert!(completion::token(source,1).is_none());
    for source in ["`@file`", "``a @file b``", "~~~rust\n@file\n~~~", "```rust\n@file", "    @file", "mail@file", "text /mod", "/model arg"] {let cursor=source.find("@file").map_or(source.len(),|n|n+5);assert!(completion::token(source,cursor).is_none(),"{source:?}");}
    assert_eq!(completion::token("  /MOD",6).unwrap().query,"mod");
}
#[test]
fn slash_keyboard_inserts_never_executes_and_escape_waits_for_edit() {
    let mut rt=runtime();key(&mut rt,Key::Text("/".into()));assert_eq!(rt.state.shell.completion.items.len(),8);assert!(rt.find("completion-list").is_some());
    key(&mut rt,Key::Down);assert_eq!(rt.state.shell.completion.selected,1);key(&mut rt,Key::Up);key(&mut rt,Key::Tab);
    assert_eq!(rt.state.shell.composer.text,"/new ");assert!(rt.state.controller.model().error.is_none());assert!(!rt.state.controller.model().busy);
    draft(&mut rt,"/mod");key(&mut rt,Key::Enter);assert_eq!(rt.state.shell.composer.text,"/model ");assert!(rt.state.ui.panel.is_none());assert!(rt.state.controller.model().ui_actions.is_empty());
    draft(&mut rt,"/se");key(&mut rt,Key::Escape);for _ in 0..3 {pump(&mut rt);}assert!(rt.find("completion-list").is_none());
    key(&mut rt,Key::Text("t".into()));assert!(rt.find("completion-list").is_some());
    assert!(rt.state.controller.model().pending_commands.values().all(|c|!c.contains("send")));assert!(rt.state.shell.error.is_none());
}
#[test]
fn real_workspace_search_debounce_click_inspect_and_quote_spaces() {
    let root=tempfile::tempdir().unwrap();std::fs::write(root.path().join("alpha file.txt"),"fixture").unwrap();std::fs::write(root.path().join("beta.txt"),"fixture").unwrap();
    let mut rt=runtime();rt.state.controller=aether_state::Controller::new_with_workspace(root.path().to_string_lossy().into()).unwrap();native::sync(&mut rt.state);
    draft(&mut rt,"雪🙂 @alp tail");let end=rt.state.shell.composer.text.find(" tail").unwrap();rt.state.shell.composer.anchor=end;rt.state.shell.composer.focus=end;completion::input_changed(&mut rt.state,&mut Tx::default());
    pump(&mut rt);assert!(!rt.state.controller.model().pending_commands.values().any(|c|c=="list_workspace_files"));
    wait(&mut rt,|s|!s.shell.completion.items.is_empty());assert_eq!(rt.state.shell.completion.items.len(),1);assert_eq!(rt.state.shell.completion.items[0].label,"alpha file.txt");
    let (x,y)=rt.find("completion-item-0").unwrap();rt.pointer_down(x,y);rt.pointer_up(x,y);rt.frame(0.016);
    assert_eq!(rt.state.shell.composer.text,"雪🙂 @alp tail");assert!(rt.state.shell.native.preparing());assert!(rt.state.controller.model().attachments.is_empty());
    wait(&mut rt,|s|!s.controller.model().attachments.is_empty());assert_eq!(rt.state.shell.composer.text,"雪🙂 @\"alpha file.txt\" tail");
    assert_eq!(std::fs::canonicalize(&rt.state.controller.model().attachments[0].path).unwrap(),std::fs::canonicalize(root.path().join("alpha file.txt")).unwrap());assert!(!rt.state.controller.model().busy);
}
#[test]
fn stale_navigation_aba_and_changed_draft_never_attach_or_insert() {
    let root=tempfile::tempdir().unwrap();std::fs::write(root.path().join("alpha.txt"),"fixture").unwrap();
    let mut rt=runtime();rt.state.controller=aether_state::Controller::new_with_workspace(root.path().to_string_lossy().into()).unwrap();native::sync(&mut rt.state);draft(&mut rt,"@alp");std::thread::sleep(Duration::from_millis(155));completion::sync(&mut rt.state,&mut Tx::default());
    let old=rt.state.controller.model().current.clone().unwrap();let generation=rt.state.controller.navigation_generation();let a=old.session.id.clone();rt.state.controller.runtime.block_on(aether_core::agent::storage::save_session(old)).unwrap();rt.state.controller.new_session();rt.state.controller.open_session(&a);assert_ne!(rt.state.controller.navigation_generation(),generation);
    pump(&mut rt);assert!(rt.state.shell.completion.items.is_empty());
    wait(&mut rt,|s|!s.shell.completion.items.is_empty());key(&mut rt,Key::Enter);assert!(rt.state.shell.native.preparing());draft(&mut rt,"changed @alp");
    for _ in 0..20 {pump(&mut rt);std::thread::sleep(Duration::from_millis(5));}assert!(rt.state.controller.model().attachments.is_empty());assert_eq!(rt.state.shell.composer.text,"changed @alp");
}
#[test]
fn ime_modal_queue_editor_and_preview_do_not_complete() {
    let mut rt=runtime();draft(&mut rt,"/mod");rt.state.shell.composer.preedit=Some(("に".into(),None));
    assert!(!completion::on_key(&mut rt.state,&KeyEvent{key:Key::Enter,mods:Mods::default()},&mut Tx::default()));pump(&mut rt);assert!(rt.state.shell.completion.items.is_empty());assert_eq!(rt.state.shell.composer.text,"/mod");
    rt.state.shell.composer.preedit=None;rt.state.ui.panel=Some(aether_rune::panels::Panel::Settings);pump(&mut rt);assert!(rt.state.shell.completion.items.is_empty());rt.state.ui.panel=None;
    rt.state.shell.queue.edit=Some(aether_rune::statequeue::QueueEdit{session:"fixture".into(),id:"item".into(),text:TextEdit::new("/mod")});pump(&mut rt);assert!(rt.state.shell.completion.items.is_empty());
}
#[test]
fn ctrl_enter_keeps_existing_local_send_route() {
    let mut rt=runtime();draft(&mut rt,"/model");rt.key(KeyEvent{key:Key::Enter,mods:Mods{ctrl:true,..Mods::default()}});rt.frame(0.016);
    assert_eq!(rt.state.controller.model().ui_actions.len(),1,"intent missing; draft={:?}, editor={:?}, error={:?}",rt.state.controller.model().draft,rt.state.shell.composer.text,rt.state.shell.error);assert!(rt.state.shell.composer.text.is_empty(),"accepted intent left editor={:?}; model draft={:?}",rt.state.shell.composer.text,rt.state.controller.model().draft);assert!(!rt.state.controller.model().busy);
}
#[test]
fn selection_cursor_and_workspace_changes_invalidate_visible_results() {
    let root=tempfile::tempdir().unwrap();std::fs::write(root.path().join("alpha.txt"),"fixture").unwrap();
    let mut rt=runtime();rt.state.controller=aether_state::Controller::new_with_workspace(root.path().to_string_lossy().into()).unwrap();native::sync(&mut rt.state);draft(&mut rt,"@alp");
    wait(&mut rt,|s|!s.shell.completion.items.is_empty());
    rt.state.shell.composer.anchor=0;pump(&mut rt);assert!(rt.state.shell.completion.items.is_empty());
    rt.state.shell.composer.anchor=rt.state.shell.composer.focus;pump(&mut rt);wait(&mut rt,|s|!s.shell.completion.items.is_empty());
    rt.state.controller.model_mut().workspace.clear();pump(&mut rt);assert!(rt.state.shell.completion.items.is_empty());assert!(rt.state.controller.model().attachments.is_empty());
}
#[test]
fn popup_geometry_and_keyboard_scroll_stay_with_the_composer() {
    let mut rt=runtime();draft(&mut rt,"/");
    let popup=rt.elements().into_iter().find(|e|e.key.as_deref()==Some("completion-list")).unwrap();
    let [x,y,w,h]=popup.rect;assert!(x>=0.0 && y>=0.0 && x+w<=901.0 && y+h<=601.0 && h<=161.0,"{:?}",popup.rect);
    for _ in 0..7 {key(&mut rt,Key::Down);}assert_eq!(rt.state.shell.completion.selected,7);
    key(&mut rt,Key::Tab);assert_eq!(rt.state.shell.composer.text,"/review ");assert!(!rt.state.controller.model().busy);assert!(rt.state.shell.error.is_none());
}
#[test]
fn failed_inspection_preserves_draft_and_does_not_attach() {
    let root=tempfile::tempdir().unwrap();let path=root.path().join("alpha.txt");std::fs::write(&path,"fixture").unwrap();
    let mut rt=runtime();rt.state.controller=aether_state::Controller::new_with_workspace(root.path().to_string_lossy().into()).unwrap();native::sync(&mut rt.state);draft(&mut rt,"@alp");wait(&mut rt,|s|!s.shell.completion.items.is_empty());
    // A missing file is simulated by selecting a file that has become a directory.
    // Rename preserves the fixture bytes; no user file is deleted.
    std::fs::rename(&path,root.path().join("retained.txt")).unwrap();std::fs::create_dir(&path).unwrap();key(&mut rt,Key::Tab);
    wait(&mut rt,|s|!s.shell.native.preparing());assert_eq!(rt.state.shell.composer.text,"@alp");assert!(rt.state.controller.model().attachments.is_empty());assert!(rt.state.shell.completion.status.as_deref().unwrap_or("").contains("inspection failed"));
}
#[test]
fn ctrl_enter_rejected_send_retains_editor_and_model_draft() {
    let mut rt=runtime(); draft(&mut rt,"retain rejected prompt");
    rt.key(KeyEvent{key:Key::Enter,mods:Mods{ctrl:true,..Mods::default()}});rt.frame(0.016);
    assert_eq!(rt.state.shell.composer.text,"retain rejected prompt");
    assert_eq!(rt.state.controller.model().draft,"retain rejected prompt");
    assert!(rt.state.shell.error.is_some());assert!(!rt.state.controller.model().busy);
}