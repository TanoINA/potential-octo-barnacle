use aether_core::agent::EventSink;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{
    mpsc::{sync_channel, Receiver, SyncSender, TrySendError},
    Arc, Mutex,
};

pub const EVENT_CAPACITY: usize = 1024;
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The native AgentEvent serde envelope is preserved verbatim in `payload`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendEvent {
    pub topic: String,
    pub payload: Value,
}

/// Lossless, bounded transport. Full queues apply producer backpressure instead
/// of retaining an unbounded overflow/chunk buffer or dropping terminal events.
#[derive(Clone)]
pub struct EventQueue {
    tx: SyncSender<BackendEvent>,
    waker: Arc<Mutex<Option<Waker>>>,
}
impl EventQueue {
    pub fn new() -> (Self, Receiver<BackendEvent>) {
        let (tx, rx) = sync_channel(EVENT_CAPACITY);
        (
            Self {
                tx,
                waker: Arc::new(Mutex::new(None)),
            },
            rx,
        )
    }
    pub fn set_waker(&self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(waker);
        self.wake();
    }
    pub(crate) fn clear_waker(&self) {
        *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    pub fn wake(&self) {
        let callback = self.waker.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(callback) = callback {
            callback();
        }
    }
}
impl EventSink for EventQueue {
    fn emit(&self, topic: &str, payload: Value) -> Result<(), String> {
        let event = BackendEvent {
            topic: topic.into(),
            payload,
        };
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(event)) => {
                self.wake();
                self.tx
                    .send(event)
                    .map_err(|_| "Controller disconnected".to_string())?;
            }
            Err(TrySendError::Disconnected(_)) => return Err("Controller disconnected".into()),
        }
        self.wake();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_core::agent::AgentHost;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn bounded_transport_preserves_every_event_and_wakes_on_delivery() {
        let (transport, rx) = EventQueue::new();
        let wakes = Arc::new(AtomicUsize::new(0));
        let count = wakes.clone();
        transport.set_waker(Arc::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
        }));
        let host = AgentHost::new(transport);
        let producer = std::thread::spawn(move || {
            for n in 0..2048 {
                host.emit("agent-event", json!({"n":n})).unwrap();
            }
            host.emit("agent-event", json!({"type":"done"})).unwrap();
        });
        for n in 0..2048 {
            assert_eq!(rx.recv().unwrap().payload["n"], n);
        }
        assert_eq!(rx.recv().unwrap().payload["type"], "done");
        producer.join().unwrap();
        assert!(wakes.load(Ordering::Relaxed) >= 2049);
    }
}
