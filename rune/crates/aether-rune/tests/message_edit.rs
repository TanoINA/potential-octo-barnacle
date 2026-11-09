//! Real main-transcript input routing plus real native storage acknowledgments.
use aether_core::agent::{profile, storage};
use aether_rune::{AppState, view::view};
use rune::{Env, Runtime};
use serde_json::json;
use std::{
    sync::Once,
    time::{Duration, Instant},
};
fn runtime() -> (Runtime<AppState>, String) {
    // This integration binary owns a separate disposable profile.
    static PROFILE: Once = Once::new();
    PROFILE.call_once(|| {
        profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-view-tasks-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
    let mut s = AppState::new();
    let mut data = s.controller.model().current.clone().unwrap();
    data.session.id = format!("sess_{}", uuid::Uuid::new_v4());
    data.messages = serde_json::from_value(json!([
        {"id":"task-user","role":"user","content":"A plan, please","timestamp":1},
        {"id":"task-answer","role":"assistant","content":"- [ ] Native task","timestamp":2}
    ]))
    .unwrap();
    let id = data.session.id.clone();
    s.controller
        .runtime
        .block_on(storage::save_session(data))
        .unwrap();
    s.controller.new_session();
    let op = s.controller.open_session(&id);
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        s.controller.poll();
        if let Some(r) = s.controller.take_result(op) {
            assert!(r.error.is_none(), "{:?}", r.error);
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut rt = Runtime::new(s, view, (1280.0, 1000.0), Env::new(true, 1.0));
    rt.frame(0.0);
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    (rt, id)
}
#[test]
fn main_transcript_secondary_press_opens_native_menu() {
    let (mut rt, _) = runtime();
    let (x, y) = rt
        .find("message-body-task-user")
        .expect("actual transcript row");
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }

    assert!(rt.find("message-context-task-user-menu").is_some());
    assert!(
        rt.find("message-task-user").is_some(),
        "stable virtualized row key"
    );
}
#[test]
fn main_transcript_checkbox_saves_only_after_native_ack() {
    let (mut rt, id) = runtime();
    let checkbox = rt
        .elements()
        .into_iter()
        .find(|e| e.role == "checkbox")
        .expect("task checkbox in main transcript");
    let [x, y, w, h] = checkbox.rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.016);
    assert!(rt.state.controller.model().busy);
    assert_eq!(
        rt.state
            .controller
            .model()
            .current
            .as_ref()
            .unwrap()
            .messages[1]
            .content,
        "- [ ] Native task"
    );
    let until = Instant::now() + Duration::from_secs(10);
    while rt.state.controller.model().busy {
        rt.state.controller.poll();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        rt.state.controller.model().error.is_none(),
        "{:?}",
        rt.state.controller.model().error
    );
    assert_eq!(
        rt.state
            .controller
            .model()
            .current
            .as_ref()
            .unwrap()
            .messages[1]
            .content,
        "- [x] Native task"
    );
    let saved = rt
        .state
        .controller
        .runtime
        .block_on(storage::load_session(&id))
        .unwrap();
    assert_eq!(saved.messages[0].content, "A plan, please");
    assert_eq!(saved.messages[1].content, "- [x] Native task");
}
