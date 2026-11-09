//! Targeted, renderer-free facade tests. The harness works before parent re-exports.
#[allow(unused_imports)]
#[path = "../src/agent.rs"]
mod agent;
#[path = "../src/service.rs"]
mod service;
#[path = "../src/updater.rs"]
mod updater;
use agent::{AgentHost, EventSink};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};

struct Collector(Arc<Mutex<Vec<(String, Value)>>>);
impl EventSink for Collector {
    fn emit(&self, topic: &str, value: Value) -> Result<(), String> {
        self.0.lock().unwrap().push((topic.into(), value));
        Ok(())
    }
}
fn backend() -> service::Backend {
    static PROFILE: OnceLock<()> = OnceLock::new();
    PROFILE.get_or_init(|| {
        #[cfg(feature = "shared-profile")]
        agent::profile::configure_storage_root(
            std::env::temp_dir().join(format!("aether-service-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
    });
    service::Backend::new(AgentHost::new(Collector(Arc::new(Mutex::new(Vec::new())))))
}

#[tokio::test]
async fn service_settings_dispatch_returns_the_authoritative_settings() {
    let result = backend().dispatch("get_settings", json!({})).await;
    assert!(
        result.is_ok(),
        "get_settings must be dispatched, got {result:?}"
    );
    let expected = serde_json::to_value(agent::storage::load_settings().await.unwrap()).unwrap();
    assert_eq!(result.unwrap(), expected);
    assert!(service::Backend::supported_commands().contains(&"get_settings"));
    assert!(agent::storage::config_dir()
        .starts_with(std::fs::canonicalize(std::env::temp_dir()).unwrap()));
}

#[tokio::test]
async fn service_required_arguments_are_validated_before_native_operations() {
    let backend = backend();
    for (command, key) in [
        ("send_message", "request"),
        ("send_council_message", "requestA"),
        ("review_session", "sessionId"),
        ("abort_agent", "runId"),
        ("save_settings", "settings"),
        ("warm_session_index", "limit"),
        ("load_session", "sessionId"),
        ("load_session_page", "sessionId"),
        ("load_session_around", "sessionId"),
        ("save_session_page", "data"),
        ("refresh_session_window", "sessionId"),
        ("checkpoint_session_page", "data"),
        ("read_message_image_inputs", "sessionId"),
        ("read_session_artifact", "sessionId"),
        ("rename_session_summary", "sessionId"),
        ("save_session", "data"),
        ("checkpoint_session_delta", "sessionId"),
        ("checkpoint_session", "data"),
        ("rename_session", "sessionId"),
        ("toggle_pin_session", "sessionId"),
        ("deep_search", "query"),
        ("delete_session", "sessionId"),
        ("export_session", "sessionId"),
        ("compact_session", "sessionId"),
        ("save_workspace_agents_md", "content"),
        ("fetch_provider_models", "provider"),
        ("set_workspace_cwd", "newCwd"),
        ("save_skill", "name"),
        ("list_mcp_tools", "server"),
        ("call_mcp_tool", "server"),
        ("git_commit", "message"),
        ("revert_file_changes", "path"),
        ("stage_file_changes", "path"),
        ("run_shell_command", "command"),
        ("inspect_paths", "paths"),
        ("prepare_image_attachments", "attachments"),
        ("save_pasted_attachment", "name"),
        ("open_path_or_url", "pathOrUrl"),
        ("reveal_in_explorer", "path"),
        ("list_background_tasks", "sessionId"),
        ("kill_background_task", "sessionId"),
        ("get_background_task_output", "sessionId"),
        ("read_reflection_skill", "sessionId"),
        ("list_review_proposals", "sessionId"),
        ("review_proposal_diff", "sessionId"),
        ("approve_review_proposal", "sessionId"),
        ("reject_review_proposal", "sessionId"),
    ] {
        let error = backend.dispatch(command, json!({})).await.unwrap_err();
        assert!(
            error.contains(key),
            "{command}: expected missing {key}, got {error}"
        );
    }
}

#[tokio::test]
async fn service_unknown_commands_and_malformed_arguments_never_succeed() {
    let backend = backend();
    assert!(backend
        .dispatch("not_a_command", json!({}))
        .await
        .unwrap_err()
        .contains("Unknown command"));
    assert!(backend.dispatch("get_settings", json!([])).await.is_err());
    assert!(backend
        .dispatch("load_session", json!({"sessionId": 42}))
        .await
        .is_err());
    assert!(backend
        .dispatch("load_session", json!({"session_id": "../escape"}))
        .await
        .is_err());
    let commands = service::Backend::supported_commands();
    assert!(commands.len() >= 60);
    let mut unique = commands.to_vec();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), commands.len());
    assert!(commands.contains(&"install_release_update"));
}

#[tokio::test]
async fn service_background_tasks_cannot_cross_session_boundaries() {
    let backend = backend();
    let owner = format!("owner-{}", uuid::Uuid::new_v4());
    let command = if cfg!(windows) {
        "Write-Output scoped"
    } else {
        "printf scoped"
    };
    let task = agent::tasks::get_task_manager()
        .spawn_task(&owner, command, "", None, false)
        .await
        .unwrap();
    for command in ["get_background_task_output", "kill_background_task"] {
        assert!(backend
            .dispatch(command, json!({"sessionId":"foreign", "taskId":task.id}))
            .await
            .unwrap_err()
            .contains("not found"));
    }
    assert_eq!(
        backend
            .dispatch("list_background_tasks", json!({"sessionId":"foreign"}))
            .await
            .unwrap(),
        json!([])
    );
    assert!(backend
        .dispatch(
            "get_background_task_output",
            json!({"session_id":owner, "task_id":task.id})
        )
        .await
        .is_ok());
}

#[tokio::test]
async fn service_rendererless_run_emits_model_error_then_done_and_releases_scope() {
    let mut backend = backend();
    let events = Arc::new(Mutex::new(Vec::new()));
    backend.host = AgentHost::new(Collector(events.clone()));
    let id = format!("invalid-model-{}", uuid::Uuid::new_v4());
    backend
        .dispatch(
            "send_message",
            json!({"request":{
                "runId":id, "sessionId":id, "provider":{}, "messages":[],
                "maxTurns":1, "autoCompactionEnabled":false
            }}),
        )
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if events
                .lock()
                .unwrap()
                .iter()
                .any(|(topic, event)| topic == "agent-event" && event["type"] == "done")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    {
        let events = events.lock().unwrap();
        let error = events
            .iter()
            .position(|(_, e)| e["type"] == "error")
            .unwrap();
        let done = events
            .iter()
            .position(|(_, e)| e["type"] == "done")
            .unwrap();
        assert!(error < done);
        assert_eq!(events[done].1["data"]["successful"], false);
        assert_eq!(events[done].1["data"]["sessionId"], id);
    }
    tokio::task::yield_now().await;
    let _cancel = backend
        .runner
        .begin(&id, &id)
        .expect("finished run must release its scope");
    backend.runner.finish(&id);
}

#[tokio::test]
async fn service_council_rejects_foreign_scope_before_starting_runner() {
    let backend = backend();
    let a = json!({"runId":"a", "sessionId":"s", "provider":{}, "messages":[]});
    let mut b = a.clone();
    b["runId"] = json!("b");
    assert!(backend
        .dispatch("send_council_message", json!({"requestA":a,"requestB":b}))
        .await
        .unwrap_err()
        .contains("scope"));
}

#[tokio::test]
async fn service_native_open_rejects_code_remote_files_and_control_characters() {
    let backend = backend();
    for target in [
        "javascript:alert(1)",
        "file://remote/share/readme.md",
        "https://example.com/\n",
        "\\\\host\\share\\readme.md",
    ] {
        assert!(backend
            .dispatch("open_path_or_url", json!({"pathOrUrl":target}))
            .await
            .is_err());
    }
    assert!(backend
        .dispatch("open_terminal", json!({"path":"https://example.com"}))
        .await
        .is_err());
    assert!(backend
        .dispatch(
            "reveal_in_explorer",
            json!({"path":"file://remote/share/x"})
        )
        .await
        .is_err());
}

#[tokio::test]
async fn service_storage_commands_round_trip_pages_metadata_and_explicit_export() {
    let backend = backend();
    let id = format!("facade-{}", uuid::Uuid::new_v4());
    let data = json!({"session":{"id":id,"title":"Facade","createdAt":1,"updatedAt":1,"messageCount":2},
        "messages":[{"id":"user","role":"user","content":"hello","timestamp":1},
        {"id":"assistant","role":"assistant","content":"world","timestamp":2}],"artifacts":[]});
    assert_eq!(
        backend
            .dispatch("save_session", json!({"data":data}))
            .await
            .unwrap(),
        Value::Null
    );
    let sessions = backend.dispatch("list_sessions", json!({})).await.unwrap();
    assert!(sessions.as_array().unwrap().iter().any(|s| s["id"] == id));
    assert_eq!(
        backend
            .dispatch("load_session", json!({"sessionId":id}))
            .await
            .unwrap()["messages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let page = backend
        .dispatch("load_session_page", json!({"sessionId":id,"limit":1}))
        .await
        .unwrap();
    assert_eq!(page["window"]["total"], 2);
    assert_eq!(
        backend
            .dispatch(
                "rename_session_summary",
                json!({"sessionId":id,"title":"Renamed"})
            )
            .await
            .unwrap()["title"],
        "Renamed"
    );
    assert_eq!(
        backend
            .dispatch("toggle_pin_session", json!({"sessionId":id}))
            .await
            .unwrap()["pinned"],
        true
    );
    assert!(backend
        .dispatch("export_session", json!({"sessionId":id,"format":"json"}))
        .await
        .unwrap_err()
        .contains("destination"));
    let destination = agent::storage::config_dir().join(format!("{id}.json"));
    backend
        .dispatch(
            "export_session",
            json!({"sessionId":id,"destination":destination,"format":"json"}),
        )
        .await
        .unwrap();
    assert!(std::fs::read_to_string(destination)
        .unwrap()
        .contains("Renamed"));
    backend
        .dispatch("delete_session", json!({"sessionId":id}))
        .await
        .unwrap();
    assert!(backend
        .dispatch("load_session", json!({"sessionId":id}))
        .await
        .is_err());
}

#[test]
fn service_construction_does_not_initialize_profile_contents() {
    if std::env::var_os("AETHER_SERVICE_CONSTRUCTOR_CHILD").is_some() {
        let backend = backend();
        let root = agent::storage::config_dir();
        assert!(!root.join("config.json").exists());
        assert!(!root.join("sessions").exists());
        assert!(!root.join("tasks").exists());
        assert_eq!(*backend.workspace.lock().unwrap(), "");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "service_construction_does_not_initialize_profile_contents",
            "--nocapture",
        ])
        .env("AETHER_SERVICE_CONSTRUCTOR_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn service_proposals_require_native_session_and_run_association() {
    let backend = backend();
    for command in [
        "read_reflection_skill",
        "approve_review_proposal",
        "reject_review_proposal",
        "review_proposal_diff",
    ] {
        assert!(backend
            .dispatch(
                command,
                json!({"sessionId":"missing-session", "skillName":"x", "id":"missing-proposal"})
            )
            .await
            .is_err());
    }
}

#[tokio::test]
async fn service_workspace_mutations_bind_expected_workspace_before_writing() {
    let backend = backend();
    let root = agent::storage::config_dir().join(format!("workspace-{}", uuid::Uuid::new_v4()));
    let other = root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    *backend.workspace.lock().unwrap() = root.to_string_lossy().into_owned();
    for command in ["save_workspace_agents_md", "save_pasted_attachment"] {
        let error = backend.dispatch(command, json!({"content":"must not write", "name":"test.txt", "bytes":[1,2], "expectedWorkspace":other})).await.unwrap_err();
        assert!(error.contains("Workspace changed"), "{error}");
    }
    assert!(!root.join("AGENTS.md").exists());
    backend
        .dispatch(
            "init_workspace_agents_md",
            json!({"expectedWorkspace":root}),
        )
        .await
        .unwrap();
    assert!(root.join("AGENTS.md").exists());
    backend
        .dispatch(
            "save_workspace_agents_md",
            json!({"expected_workspace":root,"content":"# Workspace"}),
        )
        .await
        .unwrap();
    assert_eq!(
        backend
            .dispatch(
                "read_workspace_agents_md",
                json!({"expectedWorkspace":root})
            )
            .await
            .unwrap(),
        "# Workspace"
    );
}

#[tokio::test]
async fn service_invalid_image_preflight_never_reserves_runner_scope() {
    let backend = backend();
    let id = format!("bad-image-{}", uuid::Uuid::new_v4());
    let request = json!({"runId":id,"sessionId":id,"provider":{},"messages":[{
        "id":"user","role":"user","content":"image","timestamp":1,
        "imageInputs":[{"sourcePath":"missing.png","mediaType":"image/png","data":"invalid base64"}]
    }]});
    assert!(backend
        .dispatch("send_message", json!({"request":request}))
        .await
        .is_err());
    let _cancel = backend.runner.begin(&id, &id).unwrap();
    backend.runner.finish(&id);
    assert!(backend
        .dispatch(
            "send_council_message",
            json!({"requestA":request,"requestB":request})
        )
        .await
        .is_err());
    let _cancel = backend.runner.begin(&id, &id).unwrap();
    backend.runner.finish(&id);
}

#[tokio::test]
async fn service_council_workspace_scope_and_argument_aliases_are_enforced() {
    let backend = backend();
    let root = agent::storage::config_dir().join(format!("council-{}", uuid::Uuid::new_v4()));
    let other = root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let a = json!({"runId":"council", "sessionId":"session", "workspaceCwd":root,"provider":{},"messages":[]});
    let mut b = a.clone();
    b["workspaceCwd"] = json!(other);
    assert!(backend
        .dispatch("send_council_message", json!({"request_a":a,"request_b":b}))
        .await
        .unwrap_err()
        .contains("workspace"));
    assert!(backend
        .dispatch("load_session", json!({"sessionId":"a","session_id":"b"}))
        .await
        .unwrap_err()
        .contains("Ambiguous"));
}

#[tokio::test]
async fn service_release_metadata_does_not_pretend_to_install() {
    let backend = backend();
    let metadata = backend
        .dispatch("get_release_info", json!({}))
        .await
        .unwrap();
    assert_eq!(metadata["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(metadata["platform"], std::env::consts::OS);
    assert!(metadata["signingStatus"]
        .as_str()
        .unwrap()
        .contains("not verified"));
    assert!(backend
        .dispatch(
            "install_release_update",
            json!({"confirmed":true,"expectedVersion":"999.0.0"})
        )
        .await
        .unwrap_err()
        .contains("not configured"));
    if metadata["updatesConfigured"] == false {
        let check = backend
            .dispatch("check_release_update", json!({}))
            .await
            .unwrap();
        assert_eq!(check["configured"], false);
        assert_eq!(check["version"], Value::Null);
    }
}
