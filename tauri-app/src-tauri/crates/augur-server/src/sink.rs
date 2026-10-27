//! The server frontend's event sink: fans service events out to every
//! connected browser over a broadcast channel.

use augur_service::sink::{EventSink, EventTarget};

/// One event as it travels to the websocket tasks.
#[derive(Clone, Debug)]
pub struct BroadcastEvent {
    pub name: String,
    pub payload: serde_json::Value,
}

/// A sink backed by a tokio broadcast channel. `Main`-scoped events are
/// delivered to every client: the server has one primary page, and the
/// browser tabs collectively own the tab list.
#[derive(Clone)]
pub struct BroadcastSink {
    sender: tokio::sync::broadcast::Sender<BroadcastEvent>,
}

impl BroadcastSink {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(capacity);
        Self { sender }
    }

    /// Subscribe to every future event.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<BroadcastEvent> {
        self.sender.subscribe()
    }
}

impl EventSink for BroadcastSink {
    fn emit(&self, _target: EventTarget, event: &str, payload: serde_json::Value) {
        // A send fails only when no client is connected, which is not an error.
        let _ = self.sender.send(BroadcastEvent {
            name: event.to_string(),
            payload,
        });
    }
}
