//! Translate native window drag-and-drop events into targeted frontend events.

use std::path::Path;

use tauri::{Emitter, EventTarget, Manager, Window, WindowEvent};

use crate::events;

const MAIN_WINDOW_LABEL: &str = "main";

#[derive(Debug, PartialEq, Eq)]
struct OutboundEvent {
    target: EventTarget,
    name: &'static str,
    payload: OutboundPayload,
}

#[derive(Debug, PartialEq, Eq)]
enum OutboundPayload {
    DragState { label: String, active: bool },
    OpenPaths { paths: Vec<String> },
}

fn events_for_window_event(
    label: &str,
    event: &WindowEvent,
    is_directory: impl Fn(&Path) -> bool,
) -> Vec<OutboundEvent> {
    let WindowEvent::DragDrop(event) = event else {
        return Vec::new();
    };

    let state = |active| OutboundEvent {
        target: EventTarget::window(label),
        name: events::DRAG_STATE_EVENT,
        payload: OutboundPayload::DragState {
            label: label.to_string(),
            active,
        },
    };

    match event {
        tauri::DragDropEvent::Enter { paths, .. } => {
            vec![state(paths.iter().any(|path| is_directory(path)))]
        }
        tauri::DragDropEvent::Leave => vec![state(false)],
        tauri::DragDropEvent::Drop { paths, .. } => {
            let mut emitted = vec![state(false)];
            let folders: Vec<String> = paths
                .iter()
                .filter(|path| is_directory(path))
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            if !folders.is_empty() {
                emitted.push(OutboundEvent {
                    target: EventTarget::window(MAIN_WINDOW_LABEL),
                    name: events::DROP_EVENT,
                    payload: OutboundPayload::OpenPaths { paths: folders },
                });
            }
            emitted
        }
        tauri::DragDropEvent::Over { .. } => Vec::new(),
        _ => Vec::new(),
    }
}

/// Handle the drag-and-drop event delivered by Tauri's native window hook.
pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    let WindowEvent::DragDrop(drag) = event else {
        return;
    };

    let label = window.label();
    if matches!(drag, tauri::DragDropEvent::Over { .. }) {
        return;
    }
    let paths = match drag {
        tauri::DragDropEvent::Enter { paths, .. } | tauri::DragDropEvent::Drop { paths, .. } => {
            paths.as_slice()
        }
        tauri::DragDropEvent::Leave | tauri::DragDropEvent::Over { .. } => &[],
        _ => &[],
    };
    let event_name = match drag {
        tauri::DragDropEvent::Enter { .. } => "enter",
        tauri::DragDropEvent::Drop { .. } => "drop",
        tauri::DragDropEvent::Leave => "leave",
        tauri::DragDropEvent::Over { .. } => "over",
        _ => "other",
    };
    let folder_count = paths.iter().filter(|path| path.is_dir()).count();
    log::info!(
        "[drag_drop] window={label} event={event_name} paths={} folders={folder_count}",
        paths.len()
    );

    for outgoing in events_for_window_event(label, event, Path::is_dir) {
        let outgoing_name = outgoing.name;
        let result = match outgoing.payload {
            OutboundPayload::DragState { label, active } => {
                log::debug!("[drag_drop] window={} folder_drag_active={active}", label);
                window.app_handle().emit_to(
                    outgoing.target,
                    outgoing.name,
                    events::DragStatePayload { label, active },
                )
            }
            OutboundPayload::OpenPaths { paths } => {
                log::info!(
                    "[drag_drop] window={label} forwarding {} folder(s) to main",
                    paths.len()
                );
                window.app_handle().emit_to(
                    outgoing.target,
                    outgoing.name,
                    events::OpenPathsPayload { paths },
                )
            }
        };
        if let Err(error) = result {
            log::error!("[drag_drop] window={label} failed to emit {outgoing_name}: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tauri::PhysicalPosition;

    fn position() -> PhysicalPosition<f64> {
        PhysicalPosition::new(0.0, 0.0)
    }

    fn window_event(event: tauri::DragDropEvent) -> WindowEvent {
        WindowEvent::DragDrop(event)
    }

    #[test]
    fn entering_a_folder_shows_the_prompt_in_the_window_under_the_pointer() {
        let event = window_event(tauri::DragDropEvent::Enter {
            paths: vec![PathBuf::from("project")],
            position: position(),
        });

        let outgoing =
            events_for_window_event("compare-7", &event, |path| path == Path::new("project"));

        assert_eq!(
            outgoing,
            [OutboundEvent {
                target: EventTarget::window("compare-7"),
                name: events::DRAG_STATE_EVENT,
                payload: OutboundPayload::DragState {
                    label: "compare-7".to_string(),
                    active: true,
                },
            }]
        );
    }

    #[test]
    fn entering_a_file_clears_a_stale_folder_prompt() {
        let event = window_event(tauri::DragDropEvent::Enter {
            paths: vec![PathBuf::from("file.txt")],
            position: position(),
        });

        let outgoing = events_for_window_event("main", &event, |_| false);

        assert_eq!(
            outgoing,
            [OutboundEvent {
                target: EventTarget::window("main"),
                name: events::DRAG_STATE_EVENT,
                payload: OutboundPayload::DragState {
                    label: "main".to_string(),
                    active: false,
                },
            }]
        );
    }

    #[test]
    fn leaving_clears_the_drag_prompt_in_the_window_under_the_pointer() {
        let event = window_event(tauri::DragDropEvent::Leave);

        assert_eq!(
            events_for_window_event("main", &event, |_| false),
            [OutboundEvent {
                target: EventTarget::window("main"),
                name: events::DRAG_STATE_EVENT,
                payload: OutboundPayload::DragState {
                    label: "main".to_string(),
                    active: false,
                },
            }]
        );
    }

    #[test]
    fn dropping_folders_clears_the_source_prompt_and_forwards_only_folders_to_main() {
        let event = window_event(tauri::DragDropEvent::Drop {
            paths: vec![
                PathBuf::from("project-a"),
                PathBuf::from("notes.txt"),
                PathBuf::from("project-b"),
            ],
            position: position(),
        });

        let outgoing = events_for_window_event("compare-7", &event, |path| {
            matches!(path.to_str(), Some("project-a" | "project-b"))
        });

        assert_eq!(
            outgoing,
            [
                OutboundEvent {
                    target: EventTarget::window("compare-7"),
                    name: events::DRAG_STATE_EVENT,
                    payload: OutboundPayload::DragState {
                        label: "compare-7".to_string(),
                        active: false,
                    },
                },
                OutboundEvent {
                    target: EventTarget::window(MAIN_WINDOW_LABEL),
                    name: events::DROP_EVENT,
                    payload: OutboundPayload::OpenPaths {
                        paths: vec!["project-a".to_string(), "project-b".to_string()],
                    },
                },
            ]
        );
    }

    #[test]
    fn dropping_only_files_never_requests_a_repository_open() {
        let event = window_event(tauri::DragDropEvent::Drop {
            paths: vec![PathBuf::from("file.txt")],
            position: position(),
        });

        let outgoing = events_for_window_event("main", &event, |_| false);

        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].name, events::DRAG_STATE_EVENT);
        assert!(!matches!(
            outgoing[0].payload,
            OutboundPayload::OpenPaths { .. }
        ));
    }

    #[test]
    fn native_window_events_other_than_drag_and_drop_are_ignored() {
        assert!(events_for_window_event("main", &WindowEvent::Focused(true), |_| false).is_empty());
        let over = window_event(tauri::DragDropEvent::Over {
            position: position(),
        });
        assert!(events_for_window_event("main", &over, |_| false).is_empty());
    }
}
