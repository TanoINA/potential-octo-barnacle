use aether_core::agent::{profile, storage, SessionData};
use aether_state::Controller;
use serde_json::{json, Value};
use std::{
    sync::Once,
    time::{Duration, Instant},
};
fn fixture() -> (Controller, String, SessionData) {
    static ROOT: Once = Once::new();
    ROOT.call_once(|| {
        profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-message-edit-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
    let mut c = Controller::new().unwrap();
    let mut data = c.model().current.clone().unwrap();
    data.session.id = format!("sess_{}", uuid::Uuid::new_v4());
    data.messages=(0..122).map(|n|serde_json::from_value(json!({"id":format!("m{n}"),"role":"assistant","content":if n==121 {"- [ ] Native task".into()} else {format!("prefix-{n}")},"timestamp":n})).unwrap()).collect();
    data.messages[121].provider_messages = Some(vec![
        json!({"role":"assistant","content":"- [ ] Native task","opaque":{"keep":true}}),
    ]);
    data.artifacts = serde_json::from_value(
        json!([{"id":"hidden","type":"code","title":"keep","content":"payload","timestamp":1}]),
    )
    .unwrap();
    let id = data.session.id.clone();
    c.runtime
        .block_on(storage::save_session(data.clone()))
        .unwrap();
    c.new_session();
    let op = c.open_session(&id);
    wait(&mut c, op).unwrap();
    (c, id, data)
}
fn wait(c: &mut Controller, op: u64) -> Result<Value, String> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        c.poll();
        if let Some(r) = c.take_result(op) {
            return r.error.map_or_else(|| Ok(r.value.unwrap()), Err);
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn content_edit_acks_full_cas_preserves_prefix_protocol_artifacts_and_composer() {
    let (mut c, id, before) = fixture();
    assert!(c.model().window.as_ref().unwrap().offset > 0);
    c.model_mut().draft = "keep composer".into();
    c.model_mut().attachments = serde_json::from_value(
        json!([{"id":"draft","name":"draft.txt","path":"draft.txt","isDir":false}]),
    )
    .unwrap();
    let op = c
        .update_message_content("m121", "- [ ] Native task", "- [x] Native task")
        .unwrap();
    assert!(c.model().busy);
    assert_eq!(
        c.model()
            .current
            .as_ref()
            .unwrap()
            .messages
            .last()
            .unwrap()
            .content,
        "- [ ] Native task"
    );
    assert!(c
        .update_message_content("m121", "- [ ] Native task", "bad")
        .is_err());
    assert!(c.send("must wait".into()).is_err());
    let result = wait(&mut c, op).unwrap();
    assert_eq!(result["canonicalCommitted"], true);
    assert_eq!(result["resend"], false);
    assert!(result["replay"].is_null());
    let after = c.runtime.block_on(storage::load_session(&id)).unwrap();
    assert_eq!(after.messages.len(), 122);
    assert_eq!(
        json!(&after.messages[..121]),
        json!(&before.messages[..121])
    );
    assert_eq!(json!(after.artifacts), json!(before.artifacts));
    let mut expected = before.messages[121].clone();
    expected.content = "- [x] Native task".into();
    assert_eq!(json!(after.messages[121]), json!(expected));
    assert_eq!(c.model().draft, "keep composer");
    assert_eq!(c.model().attachments.len(), 1);
    assert!(!c.model().busy);
    assert_eq!(c.model().window.as_ref().unwrap().offset, 0);
    assert!(c
        .update_message_content("m121", "- [ ] Native task", "wrong")
        .is_err());
}
#[test]
fn external_same_id_edit_rejects_stale_revision_without_rewrite() {
    let (mut c, id, mut data) = fixture();
    data.messages[121].content = "external".into();
    c.runtime.block_on(storage::save_session(data)).unwrap();
    let op = c
        .update_message_content("m121", "- [ ] Native task", "- [x] Native task")
        .unwrap();
    assert!(wait(&mut c, op).is_err());
    assert_eq!(
        c.runtime
            .block_on(storage::load_session(&id))
            .unwrap()
            .messages[121]
            .content,
        "external"
    );
    assert_eq!(
        c.model()
            .current
            .as_ref()
            .unwrap()
            .messages
            .last()
            .unwrap()
            .content,
        "- [ ] Native task"
    );
}
#[test]
fn foreign_and_preview_sources_are_read_only() {
    let (mut c, _, _) = fixture();
    assert!(c.update_message_content("m0", "prefix-0", "bad").is_err());
    c.model_mut().busy = true;
    assert!(c
        .update_message_content("m121", "- [ ] Native task", "bad")
        .is_err());
    c.model_mut().busy = false;
    c.model_mut().preview = Some(
        serde_json::from_value(json!({"data":c.model().current,"window":c.model().window}))
            .unwrap(),
    );
    assert!(c
        .update_message_content("m121", "- [ ] Native task", "bad")
        .is_err());
}
#[test]
fn edit_ack_after_navigation_updates_only_origin_cache() {
    let (mut c, id, _) = fixture();
    let op = c
        .update_message_content("m121", "- [ ] Native task", "- [x] Native task")
        .unwrap();
    c.new_session();
    let selected = c.model().current.as_ref().unwrap().session.id.clone();
    c.model_mut().draft = "another conversation".into();
    wait(&mut c, op).unwrap();
    assert_eq!(c.model().current.as_ref().unwrap().session.id, selected);
    assert_eq!(c.model().draft, "another conversation");
    assert_eq!(c.open_session(&id), 0);
    assert_eq!(
        c.model().current.as_ref().unwrap().messages[121].content,
        "- [x] Native task"
    );
}
#[test]
fn dirty_failed_save_is_not_rewritten_by_task_edit() {
    let (mut c, _, mut data) = fixture();
    data.session.id = format!("sess_{}", uuid::Uuid::new_v4());
    data.messages[121].council_turn = Some(
        serde_json::from_value(json!({
            "candidateA":{"providerId":"a","model":"a","content":"- [ ] Native task"},
            "candidateB":{"providerId":"b","model":"b","content":"alternative"}
        }))
        .unwrap(),
    );
    let id = data.session.id.clone();
    c.runtime
        .block_on(storage::save_session(data.clone()))
        .unwrap();
    c.new_session();
    let open = c.open_session(&id);
    wait(&mut c, open).unwrap();
    data.messages[0].content = "native revision changed".into();
    c.runtime.block_on(storage::save_session(data)).unwrap();
    c.choose_council("m121", "A").unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while c.model().busy {
        c.poll();
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        c.model().retry_save_available,
        "failed CAS leaves live data dirty"
    );
    assert!(c
        .update_message_content("m121", "- [ ] Native task", "- [x] Native task")
        .is_err());
    let saved = c.runtime.block_on(storage::load_session(&id)).unwrap();
    assert_eq!(saved.messages[0].content, "native revision changed");
    assert_eq!(saved.messages[121].content, "- [ ] Native task");
}
