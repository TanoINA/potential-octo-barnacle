//! Renderer-neutral backend shared by desktop and headless Aether hosts.
pub mod agent;
pub mod service;
pub mod updater;

#[cfg(test)]
mod tests {
    use super::agent::{
        create_hidden_command, runner::AgentRunner, AgentEvent, AgentHost, AgentRunRequest,
        EventSink,
    };
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use tokio_util::sync::CancellationToken;

    struct Collector(Arc<Mutex<Vec<(String, Value)>>>);

    impl EventSink for Collector {
        fn emit(&self, topic: &str, payload: Value) -> Result<(), String> {
            self.0.lock().unwrap().push((topic.into(), payload));
            Ok(())
        }
    }

    fn invalid_image_request() -> AgentRunRequest {
        serde_json::from_value(json!({
            "runId": "headless-run",
            "sessionId": "headless-session",
            "provider": {},
            "messages": [{
                "id": "image", "role": "user", "content": "describe", "timestamp": 1,
                "imageInputs": [{"sourcePath": "missing.png", "mediaType": "image/png", "data": "not base64"}]
            }]
        }))
        .unwrap()
    }

    fn assert_rejected_image_events(events: &[(String, Value)]) {
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "agent-event");
        assert_eq!(events[0].1["type"], "error");
        assert!(events[0].1["data"]["message"].as_str().is_some());
        assert_eq!(
            events[1],
            (
                "agent-event".into(),
                json!({"type": "done", "data": {
                    "runId": "headless-run", "sessionId": "headless-session",
                    "cancelled": false, "successful": false
                }})
            )
        );
    }

    #[tokio::test]
    async fn shared_runner_emits_image_preflight_failure_without_a_renderer() {
        let events = Arc::new(Mutex::new(Vec::new()));
        AgentRunner::new()
            .run(
                AgentHost::new(Collector(events.clone())),
                String::new(),
                invalid_image_request(),
                CancellationToken::new(),
            )
            .await;
        assert_rejected_image_events(&events.lock().unwrap());
    }

    #[tokio::test]
    async fn shared_council_emits_image_preflight_failure_without_a_renderer() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let request = invalid_image_request();
        AgentRunner::new()
            .run_council(
                AgentHost::new(Collector(events.clone())),
                String::new(),
                request.clone(),
                request,
                CancellationToken::new(),
            )
            .await;
        assert_rejected_image_events(&events.lock().unwrap());
    }

    #[test]
    fn shared_api_exports_event_types_and_hidden_command_builder() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AgentHost>();
        assert_send_sync::<AgentRunner>();
        let _: Option<AgentEvent> = None;
        let command = create_hidden_command("aether-test-command");
        assert_eq!(command.as_std().get_program(), "aether-test-command");
    }
}
