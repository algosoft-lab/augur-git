//! Dropdown menu positioning shared by the workspace and Git controls.

use std::{cell::Cell, rc::Rc};

use gpui::{
    Anchor, App, Bounds, Context, DismissEvent, ElementId, Entity, Focusable,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point,
    RenderOnce, Role, SharedString, StatefulInteractiveElement, Window,
    anchored, deferred, div, point, px,
};
use gpui_component::{Selectable, menu::PopupMenu, popover::PopoverState};

/// A dropdown menu whose top edge is aligned with the bottom edge of its
/// trigger. This avoids the inverted `Bottom*` corner calculation in the
/// current `gpui-component` popup implementation.
pub(crate) trait DropdownMenuExt:
    Selectable + InteractiveElement + IntoElement + 'static
{
    fn dropdown_menu_below(
        mut self,
        builder: impl Fn(
            PopupMenu,
            &mut Window,
            &mut Context<PopupMenu>,
        ) -> PopupMenu
        + 'static,
    ) -> DropdownMenuBelow<Self> {
        let id = self.interactivity().element_id.clone().unwrap_or(0.into());
        DropdownMenuBelow::new(id, self, builder)
    }
}

impl DropdownMenuExt for gpui_component::button::Button {}

#[derive(Default)]
struct DropdownMenuState {
    menu: Option<Entity<PopupMenu>>,
    trigger_bounds: Bounds<Pixels>,
    trigger_bounds_captured: bool,
}

fn dropdown_anchor_point(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(bounds.origin.x, bounds.bottom())
}

#[derive(IntoElement)]
pub(crate) struct DropdownMenuBelow<T: Selectable + IntoElement + 'static> {
    id: ElementId,
    trigger: T,
    builder: Rc<
        dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu,
    >,
}

impl<T> DropdownMenuBelow<T>
where
    T: Selectable + IntoElement + 'static,
{
    fn new(
        id: ElementId,
        trigger: T,
        builder: impl Fn(
            PopupMenu,
            &mut Window,
            &mut Context<PopupMenu>,
        ) -> PopupMenu
        + 'static,
    ) -> Self {
        Self {
            id: SharedString::from(format!("dropdown-menu-below:{id:?}"))
                .into(),
            trigger,
            builder: Rc::new(builder),
        }
    }
}

impl<T> RenderOnce for DropdownMenuBelow<T>
where
    T: Selectable + IntoElement + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(
            (self.id.clone(), "popover"),
            cx,
            |_, cx| PopoverState::new(false, cx),
        );
        let menu_state =
            window.use_keyed_state((self.id.clone(), "menu"), cx, |_, _| {
                DropdownMenuState::default()
            });

        let open = state.read(cx).is_open();
        let focus_handle = state.read(cx).focus_handle(cx);
        let trigger_bounds =
            Rc::new(Cell::new(menu_state.read(cx).trigger_bounds));
        let parent_view_id = window.current_view();
        let trigger_selected = self.trigger.is_selected();
        let trigger = self
            .trigger
            .selected(trigger_selected || open)
            .into_any_element();

        let root = div()
            .on_children_prepainted({
                let trigger_bounds = trigger_bounds.clone();
                let menu_state = menu_state.clone();
                move |children_bounds, window, cx| {
                    let Some(bounds) = children_bounds.first().copied() else {
                        return;
                    };
                    trigger_bounds.set(bounds);
                    let bounds_changed = menu_state.update(cx, |state, _| {
                        let changed = !state.trigger_bounds_captured
                            || state.trigger_bounds != bounds;
                        state.trigger_bounds = bounds;
                        state.trigger_bounds_captured = true;
                        changed
                    });
                    if bounds_changed {
                        window.request_animation_frame();
                    }
                }
            })
            .id(self.id.clone())
            .child(trigger)
            .on_mouse_down(MouseButton::Left, {
                let state = state.clone();
                move |_, window, cx| {
                    cx.stop_propagation();
                    state.update(cx, |state, cx| {
                        state.set_open(open, cx);
                        state.toggle_open(window, cx);
                    });
                    cx.notify(parent_view_id);
                }
            });

        if !open || !menu_state.read(cx).trigger_bounds_captured {
            return root;
        }

        let menu = match menu_state.read(cx).menu.clone() {
            Some(menu) => menu,
            None => {
                let builder = self.builder.clone();
                let menu =
                    PopupMenu::build(window, cx, move |menu, window, cx| {
                        builder(menu, window, cx)
                    });
                menu_state.update(cx, |state, _| {
                    state.menu = Some(menu.clone());
                });
                menu.focus_handle(cx).focus(window, cx);

                let popover_state = state.clone();
                let menu_state_for_dismiss = menu_state.clone();
                window
                    .subscribe(
                        &menu,
                        cx,
                        move |_, _: &DismissEvent, window, cx| {
                            popover_state.update(cx, |state, cx| {
                                state.dismiss(window, cx)
                            });
                            menu_state_for_dismiss.update(cx, |state, _| {
                                state.menu = None;
                            });
                        },
                    )
                    .detach();

                menu
            }
        };

        let content = div()
            .id("dropdown-menu-content")
            .role(Role::Dialog)
            .occlude()
            .tab_group()
            .track_focus(&focus_handle)
            .key_context("Popover")
            .on_action(
                window.listener_for(&state, PopoverState::on_action_cancel),
            )
            .child(menu)
            .on_mouse_down_out({
                let state = state.clone();
                move |_, window, cx| {
                    state.update(cx, |state, cx| state.dismiss(window, cx));
                    cx.notify(parent_view_id);
                }
            });
        #[cfg(test)]
        let content = content.debug_selector(|| "dropdown-menu-content".into());

        root.child(
            deferred(
                anchored()
                    .anchor(Anchor::TopLeft)
                    .position(dropdown_anchor_point(trigger_bounds.get()))
                    .snap_to_window_with_margin(px(8.))
                    .child(content),
            )
            .with_priority(1),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, Styled as _};
    use gpui_component::{button::Button, menu::PopupMenuItem, v_flex};

    #[test]
    fn dropdown_anchor_starts_at_trigger_bottom_left() {
        let bounds = Bounds::new(
            point(px(100.), px(40.)),
            gpui::size(px(180.), px(32.)),
        );

        assert_eq!(dropdown_anchor_point(bounds), point(px(100.), px(72.)),);
    }

    struct DropdownHarness;

    impl Render for DropdownHarness {
        fn render(
            &mut self,
            _: &mut Window,
            _: &mut Context<Self>,
        ) -> impl IntoElement {
            v_flex().size_full().child(div().h(px(100.))).child(
                Button::new("dropdown-test-trigger")
                    .label("Open")
                    .debug_selector(|| "dropdown-test-button".into())
                    .dropdown_menu_below(|menu, _, _| {
                        menu.item(PopupMenuItem::label("Item"))
                    }),
            )
        }
    }

    #[gpui::test]
    fn dropdown_menu_renders_below_trigger(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|_, _| DropdownHarness);
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let button = cx
            .debug_bounds("dropdown-test-button")
            .expect("dropdown button should be rendered");
        cx.simulate_click(button.center(), Default::default());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        let button = cx
            .debug_bounds("dropdown-test-button")
            .expect("dropdown button should be rendered");
        let menu = cx
            .debug_bounds("dropdown-menu-content")
            .expect("dropdown menu should be rendered");
        assert!(
            menu.origin.y >= button.bottom(),
            "menu bounds: {menu:?}; button bounds: {button:?}"
        );
        assert_eq!(menu.origin.y, button.bottom());
    }
}
