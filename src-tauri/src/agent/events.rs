//! Renderer-neutral event boundary shared by desktop and headless hosts.
use serde::Serialize;
use std::sync::Arc;

pub trait EventSink: Send + Sync {
    fn emit(&self, topic: &str, payload: serde_json::Value) -> Result<(), String>;
}

#[derive(Clone)]
pub struct AgentHost(Arc<dyn EventSink>);

impl AgentHost {
    pub fn new(sink: impl EventSink + 'static) -> Self {
        Self(Arc::new(sink))
    }

    /// Serialize once at the host boundary and propagate serialization or delivery failures.
    pub fn emit<T: Serialize>(&self, topic: &str, payload: T) -> Result<(), String> {
        let payload = serde_json::to_value(payload).map_err(|error| error.to_string())?;
        self.0.emit(topic, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::Mutex;

    struct Collector(Arc<Mutex<Vec<(String, Value)>>>);
    impl EventSink for Collector {
        fn emit(&self, topic: &str, payload: Value) -> Result<(), String> {
            self.0.lock().unwrap().push((topic.into(), payload));
            Ok(())
        }
    }

    #[test]
    fn sink_errors_are_returned_unchanged() {
        struct FailingSink;
        impl EventSink for FailingSink {
            fn emit(&self, topic: &str, payload: Value) -> Result<(), String> {
                assert_eq!(topic, "agent-event");
                assert_eq!(payload, json!({"text": "hello"}));
                Err("renderer disconnected".into())
            }
        }

        assert_eq!(
            AgentHost::new(FailingSink).emit("agent-event", json!({"text": "hello"})),
            Err("renderer disconnected".into())
        );
    }

    #[test]
    fn serialization_errors_are_returned_without_calling_the_sink() {
        struct InvalidPayload;
        impl Serialize for InvalidPayload {
            fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("payload cannot be serialized"))
            }
        }

        let events = Arc::new(Mutex::new(Vec::new()));
        let host = AgentHost::new(Collector(events.clone()));
        assert_eq!(
            host.emit("agent-event", InvalidPayload),
            Err("payload cannot be serialized".into())
        );
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn cloned_hosts_forward_topics_and_serialized_payloads_in_order() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let host = AgentHost::new(Collector(events.clone()));
        host.emit(
            "agent-event",
            json!({"type":"chunk","runId":"r1","text":"hello"}),
        )
        .unwrap();
        host.clone()
            .emit("agent:micro-compaction", json!({"removed":2}))
            .unwrap();
        host.emit("agent:self-improvement", json!({"phase":"done"}))
            .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                (
                    "agent-event".into(),
                    json!({"type":"chunk","runId":"r1","text":"hello"})
                ),
                ("agent:micro-compaction".into(), json!({"removed":2})),
                ("agent:self-improvement".into(), json!({"phase":"done"})),
            ]
        );
    }
}
