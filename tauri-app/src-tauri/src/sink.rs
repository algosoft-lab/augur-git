//! The desktop frontend's event sink: forwards service events to the Tauri
//! webview windows.

use augur_service::sink::{EventSink, EventTarget};
use tauri::{AppHandle, Emitter, Manager};

/// Emit every service event to the webviews. `Main` targets the primary
/// window only, so CLI paths and window-scoped notices never disturb
/// auxiliary windows.
pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EventSink for TauriSink {
    fn emit(&self, target: EventTarget, event: &str, payload: serde_json::Value) {
        let result = match target {
            EventTarget::All => self.app.emit(event, payload),
            EventTarget::Main => match self.app.get_webview_window("main") {
                Some(window) => window.emit(event, payload),
                None => {
                    log::warn!("[events] no main window to emit {event} to");
                    return;
                }
            },
        };
        if let Err(error) = result {
            log::warn!("[events] failed to emit {event}: {error}");
        }
    }
}
