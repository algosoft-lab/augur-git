//! Dropdown menu positioning shared by the workspace and Git controls.

use std::{cell::Cell, rc::Rc};

use gpui::{
    Anchor, App, Bounds, Context, DismissEvent, ElementId, Entity, Focusable,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point,
    RenderOnce, Role, SharedString, StatefulInteractiveElement, Window,
    anchored, deferred, div, point, px,
};
use gpui_component::{Selectable, menu::PopupMenu, popover::PopoverState};

/// Paint priority for dropdowns hosted by a gpui-component dialog.
pub(crate) const DIALOG_DROPDOWN_PRIORITY: usize = 20;

/// A dropdown menu whose top edge is aligned with the bottom edge of its
/// trigger. This avoids the inverted `Bottom*` corner calculation in the
/// current `gpui-component` popup implementation.
pub(crate) trait DropdownMenuExt:
    Selectable + InteractiveElement + IntoElement + 'static
{
    fn dropdown_menu_below(
        self,
        builder: impl Fn(
            PopupMenu,
            &mut Window,
            &mut Context<PopupMenu>,
        ) -> PopupMenu
        + 'static,
    ) -> DropdownMenuBelow<Self> {
        self.dropdown_menu_below_with_key("static".into(), builder)
    }

    /// Build a dropdown whose cached popup is rebuilt when `content_key` changes.
    fn dropdown_menu_below_with_key(
        mut self,
        content_key: SharedString,
        builder: impl Fn(
            PopupMenu,
            &mut Window,
            &mut Context<PopupMenu>,
        ) -> PopupMenu
        + 'static,
    ) -> DropdownMenuBelow<Self> {
        let id = self.interactivity().element_id.clone().unwrap_or(0.into());
        DropdownMenuBelow::new(id, self, content_key, builder)
    }
}

impl DropdownMenuExt for gpui_component::button::Button {}

#[derive(Default)]
struct DropdownMenuState {
    menu: Option<Entity<PopupMenu>>,
    menu_content_key: Option<SharedString>,
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
    content_key: SharedString,
    deferred_priority: usize,
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
        content_key: SharedString,
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
            content_key,
            deferred_priority: 1,
            builder: Rc::new(builder),
        }
    }

    /// Set the deferred paint priority for the popup content.
    pub(crate) fn deferred_priority(mut self, priority: usize) -> Self {
        self.deferred_priority = priority;
        self
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
            Some(menu) => {
                let content_changed =
                    menu_state.read(cx).menu_content_key.as_ref()
                        != Some(&self.content_key);
                if content_changed {
                    let builder = self.builder.clone();
                    let content_key = self.content_key.clone();
                    menu.update(cx, |menu, cx| {
                        menu.rebuild(window, cx, move |menu, window, cx| {
                            builder(menu, window, cx)
                        });
                    });
                    menu_state.update(cx, |state, _| {
                        state.menu_content_key = Some(content_key);
                    });
                    log::debug!(
                        "[dropdown] rebuilt menu content: id={:?}",
                        self.id
                    );
                }
                menu
            }
            None => {
                let builder = self.builder.clone();
                let menu =
                    PopupMenu::build(window, cx, move |menu, window, cx| {
                        builder(menu, window, cx)
                    });
                menu_state.update(cx, |state, _| {
                    state.menu = Some(menu.clone());
                    state.menu_content_key = Some(self.content_key.clone());
                });
                log::debug!(
                    "[dropdown] opened menu: id={:?}, priority={}",
                    self.id,
                    self.deferred_priority
                );
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
            // PopupMenu owns outside-click handling for the complete menu
            // hierarchy. Its parent-aware dismiss logic keeps a deferred
            // submenu alive until a menu item receives MouseUp and can fire
            // its click/action handler.
            .child(menu);
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
            .with_priority(self.deferred_priority),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;
    use gpui::{AppContext as _, Context, Render, Styled as _, canvas};
    use gpui_component::{
        Root, WindowExt, button::Button, h_flex, menu::PopupMenuItem, v_flex,
    };

    #[test]
    fn dropdown_anchor_starts_at_trigger_bottom_left() {
        let bounds = Bounds::new(
            point(px(100.), px(40.)),
            gpui::size(px(180.), px(32.)),
        );

        assert_eq!(dropdown_anchor_point(bounds), point(px(100.), px(72.)),);
    }

    struct DropdownHarness {
        clicked: Rc<Cell<bool>>,
    }

    impl Render for DropdownHarness {
        fn render(
            &mut self,
            _: &mut Window,
            _: &mut Context<Self>,
        ) -> impl IntoElement {
            let clicked = self.clicked.clone();
            v_flex().size_full().child(div().h(px(100.))).child(
                Button::new("dropdown-test-trigger")
                    .label("Open")
                    .debug_selector(|| "dropdown-test-button".into())
                    .dropdown_menu_below(move |menu, window, cx| {
                        let clicked_for_submenu = clicked.clone();
                        let submenu =
                            PopupMenu::build(window, cx, move |menu, _, _| {
                                let clicked = clicked_for_submenu.clone();
                                menu.item(
                                    PopupMenuItem::new("Install").on_click(
                                        move |_, _, _| clicked.set(true),
                                    ),
                                )
                            });
                        menu.item(PopupMenuItem::submenu("File", submenu))
                    }),
            )
        }
    }

    struct DynamicDropdownHarness {
        items: Vec<String>,
        revision: u64,
        clicked: Rc<Cell<Option<usize>>>,
    }

    struct DialogHost;

    impl Render for DialogHost {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> impl IntoElement {
            div()
                .size_full()
                .children(Root::render_dialog_layer(window, cx))
        }
    }

    fn paint_probe(
        order: Rc<RefCell<Vec<&'static str>>>,
        label: &'static str,
    ) -> impl IntoElement {
        canvas(
            |_, _, _| (),
            move |_, _, _, _| order.borrow_mut().push(label),
        )
        .size(px(1.))
    }

    impl Render for DynamicDropdownHarness {
        fn render(
            &mut self,
            _: &mut Window,
            _: &mut Context<Self>,
        ) -> impl IntoElement {
            let items = self.items.clone();
            let revision = self.revision;
            let clicked = self.clicked.clone();
            v_flex().size_full().child(
                Button::new("dynamic-dropdown-test-trigger")
                    .label("Open")
                    .debug_selector(|| "dynamic-dropdown-test-button".into())
                    .dropdown_menu_below_with_key(
                        SharedString::from(format!("dynamic:{revision}")),
                        move |menu, _, _| {
                            items.iter().enumerate().fold(
                                menu,
                                |menu, (index, label)| {
                                    let clicked = clicked.clone();
                                    menu.item(
                                        PopupMenuItem::new(label.clone())
                                            .on_click(move |_, _, _| {
                                                clicked.set(Some(index))
                                            }),
                                    )
                                },
                            )
                        },
                    ),
            )
        }
    }

    #[gpui::test]
    fn dropdown_menu_renders_below_trigger(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|_, _| DropdownHarness {
            clicked: Rc::new(Cell::new(false)),
        });
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

    #[gpui::test]
    fn dynamic_dropdown_rebuilds_and_clicks_new_item(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let clicked = Rc::new(Cell::new(None));
        let clicked_for_view = clicked.clone();
        let (view, cx) =
            cx.add_window_view(move |_, _| DynamicDropdownHarness {
                items: vec!["Ubuntu".into()],
                revision: 1,
                clicked: clicked_for_view.clone(),
            });
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let button = cx
            .debug_bounds("dynamic-dropdown-test-button")
            .expect("dynamic dropdown button should be rendered");
        cx.simulate_click(button.center(), Default::default());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        let initial_menu = cx
            .debug_bounds("dropdown-menu-content")
            .expect("dynamic dropdown menu should be rendered");

        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.items.push("archlinux".into());
                view.revision = 2;
                cx.notify();
            });
        });
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        let updated_menu = cx
            .debug_bounds("dropdown-menu-content")
            .expect("updated dropdown menu should be rendered");
        assert!(
            updated_menu.size.height > initial_menu.size.height,
            "menu should grow after adding an item: initial={initial_menu:?}, updated={updated_menu:?}"
        );

        let second_item =
            point(updated_menu.center().x, updated_menu.bottom() - px(8.));
        cx.simulate_click(second_item, Default::default());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        assert_eq!(clicked.get(), Some(1));
        assert!(
            cx.debug_bounds("dropdown-menu-content").is_none(),
            "menu should dismiss after clicking the new item"
        );
    }

    #[gpui::test]
    fn dialog_dropdown_paints_above_dialog_and_clicks_second_item(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);

        let paint_order = Rc::new(RefCell::new(Vec::new()));
        let clicked = Rc::new(Cell::new(None));
        let (_, cx) = cx.add_window_view(|window, cx| {
            let content = cx.new(|_| DialogHost);
            Root::new(content, window, cx)
        });

        let paint_order_for_dialog = paint_order.clone();
        let clicked_for_dialog = clicked.clone();
        cx.update(|window, cx| {
            window.open_dialog(cx, move |dialog, _, _| {
                let paint_order = paint_order_for_dialog.clone();
                let clicked = clicked_for_dialog.clone();
                dialog.width(px(360.)).child(
                    v_flex()
                        .gap_2()
                        .child(paint_probe(paint_order.clone(), "dialog"))
                        .child(
                            Button::new("dialog-dropdown-test-trigger")
                                .label("Ubuntu")
                                .debug_selector(|| {
                                    "dialog-dropdown-test-button".into()
                                })
                                .dropdown_menu_below_with_key(
                                    SharedString::from("dialog-test"),
                                    move |menu, _, _| {
                                        let paint_order = paint_order.clone();
                                        let clicked = clicked.clone();
                                        menu.item(PopupMenuItem::new("Ubuntu"))
                                            .item(
                                                PopupMenuItem::element(
                                                    move |_, _| {
                                                        h_flex()
                                                            .child("archlinux")
                                                            .child(paint_probe(
                                                                paint_order
                                                                    .clone(),
                                                                "menu",
                                                            ))
                                                    },
                                                )
                                                .on_click(move |_, _, _| {
                                                    clicked.set(Some(1));
                                                }),
                                            )
                                    },
                                )
                                .deferred_priority(DIALOG_DROPDOWN_PRIORITY),
                        ),
                )
            });
        });
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        let button = cx
            .debug_bounds("dialog-dropdown-test-button")
            .expect("dialog dropdown button should be rendered");
        paint_order.borrow_mut().clear();
        cx.simulate_click(button.center(), Default::default());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        let order = paint_order.borrow();
        let dialog_index = order
            .iter()
            .rposition(|label| *label == "dialog")
            .expect("dialog paint probe should run");
        let menu_index = order
            .iter()
            .rposition(|label| *label == "menu")
            .expect("menu paint probe should run");
        assert!(
            dialog_index < menu_index,
            "dialog content must paint before dropdown content: {order:?}"
        );
        drop(order);

        let menu = cx
            .debug_bounds("dropdown-menu-content")
            .expect("dialog dropdown menu should be rendered");
        let second_item = point(menu.center().x, menu.bottom() - px(8.));
        cx.simulate_click(second_item, Default::default());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        assert_eq!(clicked.get(), Some(1));
        assert!(
            cx.debug_bounds("dropdown-menu-content").is_none(),
            "menu should dismiss after clicking archlinux"
        );
    }

    #[gpui::test]
    fn dropdown_submenu_click_reaches_the_item_handler(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let clicked = Rc::new(Cell::new(false));
        let clicked_for_view = clicked.clone();
        let (_, cx) = cx.add_window_view(move |_, _| DropdownHarness {
            clicked: clicked_for_view.clone(),
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));

        let button = cx
            .debug_bounds("dropdown-test-button")
            .expect("dropdown button should be rendered");
        cx.simulate_click(button.center(), Default::default());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        let menu = cx
            .debug_bounds("dropdown-menu-content")
            .expect("dropdown menu should be rendered");
        cx.simulate_mouse_move(menu.center(), None, Default::default());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        // PopupMenu anchors a submenu to the right edge of its parent row.
        let submenu_item =
            point(menu.right() + px(32.), menu.origin.y + px(13.));
        cx.simulate_click(submenu_item, Default::default());
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }

        assert!(clicked.get(), "submenu item handler should be called");
    }
}
