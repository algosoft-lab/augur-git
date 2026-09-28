//! GPUI window lifecycle helpers.
//!
//! Window-close observers run while GPUI is unwinding the window update that
//! removed the window. Entity updates from those observers must be deferred so
//! they cannot re-enter an entity that is already leased by the current update.

use gpui::{App, Context, WeakEntity};

#[cfg(target_os = "macos")]
use super::Workspace;

pub(super) fn defer_entity_update<T>(
    entity: WeakEntity<T>,
    cx: &mut App,
    update: impl FnOnce(&mut T, &mut Context<T>) + 'static,
) where
    T: 'static,
{
    cx.defer(move |cx| {
        let _ = entity.update(cx, update);
    });
}

#[cfg(target_os = "macos")]
pub(super) fn install_window_close_observer(cx: &mut Context<Workspace>) {
    let active = cx.entity().downgrade();
    cx.on_window_closed(move |cx, _window_id| {
        defer_entity_update(active.clone(), cx, |workspace, cx| {
            workspace.persist_ui_state(cx);
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{AppContext, EmptyView, TestAppContext};

    use super::defer_entity_update;

    struct Probe {
        updates: Rc<Cell<usize>>,
    }

    #[gpui::test]
    fn window_close_updates_are_deferred_until_the_active_entity_is_released(
        cx: &mut TestAppContext,
    ) {
        cx.skip_drawing();

        let updates = Rc::new(Cell::new(0));
        let probe = cx.new(|_| Probe {
            updates: updates.clone(),
        });
        let weak_probe = probe.downgrade();
        cx.update(|cx| {
            cx.on_window_closed(move |cx, _window_id| {
                defer_entity_update(weak_probe.clone(), cx, |probe, _cx| {
                    probe.updates.set(probe.updates.get() + 1);
                });
            })
            .detach();
        });

        let window = cx.add_window(|_, _| EmptyView);
        probe.update(cx, |_probe, cx| {
            window
                .update(cx, |_, window, _cx| window.remove_window())
                .expect("test window should be removable");
            assert_eq!(updates.get(), 0);
        });

        assert_eq!(updates.get(), 1);
    }
}
