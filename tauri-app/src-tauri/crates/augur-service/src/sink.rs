//! The event seam between the application service layer and its frontends.
//!
//! The service layer never knows whether it lives inside a Tauri process or an
//! HTTP server. It only emits named, serialized events to an [`EventSink`];
//! each frontend decides where those events go. The event names and payload
//! shapes are shared through [`crate::events`], so the webview protocol is
//! byte-identical for both frontends.

use serde::Serialize;

/// Where an event should be delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventTarget {
    /// Every connected client.
    All,
    /// The primary window only. The desktop frontend maps this to the main
    /// webview window; the server frontend treats it like [`EventTarget::All`].
    Main,
}

/// A receiver of application events.
pub trait EventSink: Send + Sync {
    /// Emit one event. Failures are logged, never propagated: an event is a
    /// notification, not a result.
    fn emit(&self, target: EventTarget, event: &str, payload: serde_json::Value);
}

/// Emit a serializable payload through a sink.
pub fn emit<S: Serialize>(
    sink: &dyn EventSink,
    target: EventTarget,
    event: &str,
    payload: &S,
) {
    match serde_json::to_value(payload) {
        Ok(value) => sink.emit(target, event, value),
        Err(error) => log::error!("[events] failed to serialize {event}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Sample {
        value: u32,
    }

    struct CapturingSink(std::sync::Mutex<Vec<(EventTarget, String, serde_json::Value)>>);

    impl EventSink for CapturingSink {
        fn emit(&self, target: EventTarget, event: &str, payload: serde_json::Value) {
            self.0.lock().unwrap().push((target, event.to_string(), payload));
        }
    }

    #[test]
    fn emit_serializes_the_payload_for_the_sink() {
        let sink = CapturingSink(std::sync::Mutex::new(Vec::new()));
        emit(&sink, EventTarget::All, "augur://test", &Sample { value: 3 });
        let captured = sink.0.lock().unwrap();
        assert_eq!(captured[0].1, "augur://test");
        assert_eq!(captured[0].2["value"], 3);
    }
}
