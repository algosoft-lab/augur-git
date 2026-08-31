//! Editable revision picker shared by both endpoints of the comparison view.

use gpui::prelude::*;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::list::{List, ListDelegate, ListEvent, ListState};
use gpui_component::popover::Popover;
use gpui_component::searchable_list::SearchableListItemElement;
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Sizable, StyledExt, switch::Switch,
    v_flex,
};

use crate::core::git::{CompareRevision, CompareRevisionKind};
use crate::core::i18n::{self, Locale};

use super::revision_picker_logic::{
    grouped_options, has_exact_option, section_for_kind,
};
use super::shared;

const SECTION_COUNT: usize = 3;

/// Complete value used when swapping the two endpoints.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RevisionPickerValue {
    pub input: String,
    pub selected: Option<CompareRevision>,
    pub manual_input: bool,
}

pub(crate) use super::revision_picker_logic::{
    RevisionPickerInput, RevisionPickerOption, classify_input,
};

struct RevisionPickerDelegate {
    locale: Locale,
    groups: [Vec<RevisionPickerOption>; SECTION_COUNT],
    filtered: [Vec<RevisionPickerOption>; SECTION_COUNT],
    selected_index: Option<IndexPath>,
}

impl RevisionPickerDelegate {
    fn new(locale: Locale) -> Self {
        let groups = std::array::from_fn(|_| Vec::new());
        Self {
            locale,
            filtered: groups.clone(),
            groups,
            selected_index: None,
        }
    }

    fn replace_options(&mut self, options: Vec<RevisionPickerOption>) {
        let groups = grouped_options(options);
        self.groups = groups;
        self.filtered = self.groups.clone();
        self.selected_index = self.first_index();
    }

    fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    fn all_options(&self) -> Vec<RevisionPickerOption> {
        self.groups.iter().flatten().cloned().collect()
    }

    fn first_index(&self) -> Option<IndexPath> {
        self.filtered
            .iter()
            .enumerate()
            .find_map(|(section, items)| {
                (!items.is_empty())
                    .then_some(IndexPath::default().section(section))
            })
    }

    fn item(&self, index: IndexPath) -> Option<&RevisionPickerOption> {
        self.filtered
            .get(index.section)
            .and_then(|items| items.get(index.row))
    }

    fn next_index(
        &self,
        current: Option<IndexPath>,
        forward: bool,
    ) -> Option<IndexPath> {
        let indices = self
            .filtered
            .iter()
            .enumerate()
            .flat_map(|(section, items)| {
                (0..items.len()).map(move |row| {
                    IndexPath::default().section(section).row(row)
                })
            })
            .collect::<Vec<_>>();
        if indices.is_empty() {
            return None;
        }
        let Some(current) = current else {
            return indices.first().copied();
        };
        let Some(position) = indices.iter().position(|index| *index == current)
        else {
            return indices.first().copied();
        };
        let next = if forward {
            (position + 1) % indices.len()
        } else {
            position.checked_sub(1).unwrap_or(indices.len() - 1)
        };
        indices.get(next).copied()
    }

    fn filter(&mut self, query: &str) {
        let query = query.trim();
        self.filtered = if query.is_empty() {
            self.groups.clone()
        } else {
            std::array::from_fn(|section| {
                self.groups[section]
                    .iter()
                    .filter(|option| option.matches(query))
                    .cloned()
                    .collect()
            })
        };

        if let Some(revision) = CompareRevision::from_commit_id(query) {
            let options = self.all_options();
            if !has_exact_option(query, &options) {
                self.filtered[section_for_kind(CompareRevisionKind::Commit)]
                    .push(RevisionPickerOption::new(
                        revision,
                        i18n::text_args(
                            self.locale,
                            "branch-compare-use-commit",
                            &[("sha", query)],
                        ),
                    ));
            }
        }
        self.selected_index = self.first_index();
    }
}

impl ListDelegate for RevisionPickerDelegate {
    type Item = SearchableListItemElement;

    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.filter(query);
        Task::ready(())
    }

    fn sections_count(&self, _cx: &App) -> usize {
        SECTION_COUNT
    }

    fn items_count(&self, section: usize, _cx: &App) -> usize {
        self.filtered.get(section).map_or(0, Vec::len)
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let option = self.item(ix)?;
        Some(
            SearchableListItemElement::new(ix.section * 100_000 + ix.row)
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .child(shared(option.label.to_string())),
                ),
        )
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let key = match section {
            0 => "branch-compare-branches",
            1 => "branch-compare-tags",
            _ => "branch-compare-commits",
        };
        Some(
            div()
                .px_2()
                .py_1()
                .text_size(crate::theme::scaled_text_size(10.))
                .text_color(_cx.theme().muted_foreground)
                .child(shared(i18n::text(self.locale, key))),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .w_full()
            .py_3()
            .px_2()
            .text_size(crate::theme::scaled_text_size(11.))
            .text_color(_cx.theme().muted_foreground)
            .child(shared(i18n::text(self.locale, "branch-compare-no-matches")))
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }

    fn cancel(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
    }
}

/// A single editable endpoint control with grouped revision suggestions.
pub(crate) struct RevisionPicker {
    id: SharedString,
    locale: Locale,
    input_state: Entity<InputState>,
    list_state: Entity<ListState<RevisionPickerDelegate>>,
    catalog: Vec<RevisionPickerOption>,
    selected: Option<CompareRevision>,
    unavailable: bool,
    input: String,
    dirty: bool,
    open: bool,
    manual_input: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RevisionPickerEvent> for RevisionPicker {}

/// Emitted when the endpoint text or selected revision changes.
#[derive(Clone, Debug)]
pub(crate) enum RevisionPickerEvent {
    Changed,
}

impl RevisionPicker {
    pub(crate) fn new(
        id: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
        locale: Locale,
    ) -> Self {
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .submit_on_enter(true)
                .placeholder(i18n::text(
                    locale,
                    "branch-compare-revision-placeholder",
                ))
        });
        let list_state = cx.new(|cx| {
            ListState::new(RevisionPickerDelegate::new(locale), window, cx)
        });

        let mut picker = Self {
            id: id.into(),
            locale,
            input_state,
            list_state,
            catalog: Vec::new(),
            selected: None,
            unavailable: false,
            input: String::new(),
            dirty: false,
            open: false,
            manual_input: false,
            _subscriptions: Vec::new(),
        };
        picker.install_subscriptions(window, cx);
        picker
    }

    /// Recreate the window-bound input and list state for the window that now renders this picker.
    ///
    /// InputState installs focus and activation observers for its construction
    /// window, so moving the same picker entity requires fresh state objects in
    /// addition to fresh `subscribe_in` registrations.
    pub(crate) fn attach_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscriptions.clear();

        let input_value = self.input.clone();
        let catalog = self.catalog.clone();
        let query = if self.manual_input || self.selected.is_some() {
            String::new()
        } else {
            input_value.trim().to_string()
        };
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .submit_on_enter(true)
                .placeholder(i18n::text(
                    self.locale,
                    "branch-compare-revision-placeholder",
                ))
        });
        input_state.update(cx, |input, cx| {
            input.set_value(input_value, window, cx);
        });
        let list_state = cx.new(|cx| {
            ListState::new(RevisionPickerDelegate::new(self.locale), window, cx)
        });
        list_state.update(cx, |list, cx| {
            list.delegate_mut().replace_options(catalog);
            list.set_query(&query, window, cx);
            let first = list.delegate().first_index();
            list.set_selected_index(first, window, cx);
        });
        self.input_state = input_state;
        self.list_state = list_state;
        self.install_subscriptions(window, cx);
    }

    fn install_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input_state = self.input_state.clone();
        let list_state = self.list_state.clone();
        let input_for_sub = input_state.clone();
        let list_for_input = list_state.clone();
        let mut subscriptions = vec![cx.subscribe_in(
            &input_state,
            window,
            move |picker, _input, event, window, cx| match event {
                InputEvent::Change => {
                    let text = input_for_sub.read(cx).value().to_string();
                    picker.input = text.clone();
                    picker.dirty = true;
                    picker.open = !picker.manual_input;
                    if !picker.manual_input {
                        list_for_input.update(cx, |list, cx| {
                            list.set_query(text.trim(), window, cx);
                            let first = list.delegate().first_index();
                            list.set_selected_index(first, window, cx);
                        });
                    }
                    cx.emit(RevisionPickerEvent::Changed);
                    cx.notify();
                }
                InputEvent::Focus => {
                    if picker.manual_input {
                        picker.open = false;
                    } else {
                        picker.open = true;
                        if !picker.dirty {
                            list_for_input.update(cx, |list, cx| {
                                list.set_query("", window, cx);
                                let first = list.delegate().first_index();
                                list.set_selected_index(first, window, cx);
                            });
                        }
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    if picker.manual_input {
                        picker.open = false;
                    } else if picker.open {
                        picker.commit_selected(window, cx);
                    } else {
                        picker.open = true;
                        cx.notify();
                    }
                }
                InputEvent::Blur => {}
            },
        )];

        subscriptions.push(cx.subscribe_in(
            &list_state,
            window,
            move |picker, _list, event, window, cx| match event {
                ListEvent::Confirm(index) => {
                    picker.commit_index(*index, window, cx);
                }
                ListEvent::Cancel => {
                    picker.open = false;
                    cx.notify();
                }
                ListEvent::Select(_) => {}
            },
        ));
        self._subscriptions = subscriptions;
    }

    pub(crate) fn selected(&self) -> Option<CompareRevision> {
        self.selected.clone()
    }

    pub(crate) fn value(&self) -> RevisionPickerValue {
        RevisionPickerValue {
            input: self.input.clone(),
            selected: self.selected.clone(),
            manual_input: self.manual_input,
        }
    }

    pub(crate) fn candidate(&self) -> RevisionPickerInput {
        classify_input(&self.input, self.selected.as_ref(), &self.catalog)
    }

    pub(crate) fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.locale == locale {
            return;
        }
        self.locale = locale;
        self.input_state.update(cx, |input, cx| {
            input.set_placeholder(
                i18n::text(locale, "branch-compare-revision-placeholder"),
                window,
                cx,
            );
        });
        self.list_state.update(cx, |list, _| {
            list.delegate_mut().set_locale(locale);
        });
        cx.notify();
    }

    pub(crate) fn set_options(
        &mut self,
        options: Vec<RevisionPickerOption>,
        default: Option<CompareRevision>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.catalog = options.clone();
        self.list_state.update(cx, |list, cx| {
            list.delegate_mut().replace_options(options);
            let query = if self.dirty { self.input.trim() } else { "" };
            list.set_query(query, window, cx);
            let first = list.delegate().first_index();
            list.set_selected_index(first, window, cx);
        });

        if self.dirty {
            self.unavailable = self.selected.as_ref().is_some_and(|value| {
                value.kind != CompareRevisionKind::Commit
                    && !self.catalog.iter().any(|option| option.value == *value)
            });
            return;
        }

        // Keep an existing selection even if its ref disappeared from the
        // latest snapshot. The comparison worker will surface a useful Git
        // error if the ref can no longer be resolved.
        let selected = self.selected.clone().or_else(|| {
            default.and_then(|value| {
                self.catalog
                    .iter()
                    .find(|option| option.value == value)
                    .map(|option| option.value.clone())
            })
        });
        self.selected = selected;
        self.unavailable = self.selected.as_ref().is_some_and(|value| {
            value.kind != CompareRevisionKind::Commit
                && !self.catalog.iter().any(|option| option.value == *value)
        });
        self.input = self
            .selected
            .as_ref()
            .map(|value| value.name.clone())
            .unwrap_or_default();
        self.input_state.update(cx, |input, cx| {
            if input.value().as_str() != self.input {
                input.set_value(self.input.clone(), window, cx);
            }
        });
    }

    pub(crate) fn set_value(
        &mut self,
        value: RevisionPickerValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = value.selected;
        self.input = value.input;
        self.unavailable = false;
        self.manual_input = value.manual_input;
        self.dirty = match self.selected.as_ref() {
            Some(selected) => {
                let input = self.input.trim();
                !selected.name.eq_ignore_ascii_case(input)
                    && !selected.full_name.eq_ignore_ascii_case(input)
            }
            None => !self.input.trim().is_empty(),
        };
        self.open = false;
        self.input_state.update(cx, |input, cx| {
            if input.value().as_str() != self.input {
                input.set_value(self.input.clone(), window, cx);
            }
        });
        self.list_state.update(cx, |list, cx| {
            let query = if self.manual_input || self.selected.is_some() {
                ""
            } else {
                self.input.trim()
            };
            list.set_query(query, window, cx);
            let first = list.delegate().first_index();
            list.set_selected_index(first, window, cx);
        });
        cx.notify();
    }

    fn set_manual_input(
        &mut self,
        manual_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.manual_input == manual_input {
            return;
        }
        self.manual_input = manual_input;
        self.open = false;
        self.list_state.update(cx, |list, cx| {
            let query = if manual_input { "" } else { self.input.trim() };
            list.set_query(query, window, cx);
            let first = list.delegate().first_index();
            list.set_selected_index(first, window, cx);
        });
        cx.notify();
    }

    fn commit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let index = self.list_state.read(cx).selected_index();
        if let Some(index) = index {
            self.commit_index(index, window, cx);
        } else if self.candidate().is_valid() {
            self.open = false;
            cx.emit(RevisionPickerEvent::Changed);
            cx.notify();
        }
    }

    fn commit_index(
        &mut self,
        index: IndexPath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(option) =
            self.list_state.read(cx).delegate().item(index).cloned()
        else {
            return;
        };
        self.selected = if option.value.kind == CompareRevisionKind::Commit
            && option.value.name == option.value.full_name
            && !self
                .catalog
                .iter()
                .any(|candidate| candidate.value == option.value)
        {
            None
        } else {
            Some(option.value.clone())
        };
        self.input = if self.selected.is_some() {
            option.value.name.clone()
        } else {
            option.value.full_name.clone()
        };
        self.dirty = self.selected.is_none();
        self.input_state.update(cx, |input, cx| {
            input.set_value(self.input.clone(), window, cx);
        });
        self.open = false;
        cx.emit(RevisionPickerEvent::Changed);
        cx.notify();
    }

    fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.manual_input {
            if event.keystroke.key.eq_ignore_ascii_case("escape") {
                self.open = false;
                cx.notify();
            }
            return;
        }
        let key = event.keystroke.key.to_ascii_lowercase();
        match key.as_str() {
            "up" | "arrowup" => {
                if self.open {
                    let current = self.list_state.read(cx).selected_index();
                    let next = self
                        .list_state
                        .read(cx)
                        .delegate()
                        .next_index(current, false);
                    self.list_state.update(cx, |list, cx| {
                        list.set_selected_index(next, window, cx);
                        if let Some(next) = next {
                            list.scroll_to_item(
                                next,
                                ScrollStrategy::Nearest,
                                window,
                                cx,
                            );
                        }
                    });
                } else {
                    self.open = true;
                }
                cx.notify();
            }
            "down" | "arrowdown" => {
                if self.open {
                    let current = self.list_state.read(cx).selected_index();
                    let next = self
                        .list_state
                        .read(cx)
                        .delegate()
                        .next_index(current, true);
                    self.list_state.update(cx, |list, cx| {
                        list.set_selected_index(next, window, cx);
                        if let Some(next) = next {
                            list.scroll_to_item(
                                next,
                                ScrollStrategy::Nearest,
                                window,
                                cx,
                            );
                        }
                    });
                } else {
                    self.open = true;
                }
                cx.notify();
            }
            "escape" => {
                if self.open {
                    self.open = false;
                    cx.notify();
                }
            }
            _ => {}
        }
    }
}

impl Render for RevisionPicker {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let this = cx.entity();
        let input_state = self.input_state.clone();
        let list_state = self.list_state.clone();
        let open = self.open && !self.manual_input;
        let manual_input = self.manual_input;
        let validation = match self.candidate() {
            RevisionPickerInput::Invalid(_) => {
                Some(i18n::text(self.locale, "branch-compare-invalid-revision"))
            }
            RevisionPickerInput::Selected(revision)
                if self.unavailable
                    && self.selected.as_ref() == Some(&revision) =>
            {
                Some(i18n::text(
                    self.locale,
                    "branch-compare-revision-unavailable",
                ))
            }
            _ => None,
        };
        let input_focus = input_state.read(cx).focus_handle(cx);
        let popover = Popover::new(SharedString::from(format!(
            "revision-picker-popover:{}",
            self.id
        )))
        .anchor(Anchor::BottomLeft)
        .open(open)
        .on_open_change({
            let this = this.clone();
            move |open, _window, cx| {
                this.update(cx, |picker, cx| {
                    picker.open = *open && !picker.manual_input;
                    cx.notify();
                });
            }
        })
        .track_focus(&input_focus)
        .trigger(
            Input::new(&input_state)
                .w(px(230.))
                .h(px(26.))
                .cleanable(true)
                .when(!manual_input, |input| {
                    input.suffix(Icon::new(IconName::ChevronDown).size(px(13.)))
                }),
        )
        .content(move |_state, _window, _cx| {
            List::new(&list_state)
                .w(px(360.))
                .max_h(rems(20.))
                .paddings(Edges::all(px(4.)))
        });

        v_flex()
            .id(SharedString::from(format!(
                "revision-picker-field:{}",
                self.id
            )))
            .w_full()
            .min_w_0()
            .gap_0p5()
            .capture_key_down({
                let this = this.clone();
                move |event, window, cx| {
                    this.update(cx, |picker, cx| {
                        picker.handle_key(event, window, cx);
                    });
                    let key = event.keystroke.key.to_ascii_lowercase();
                    if matches!(
                        key.as_str(),
                        "up" | "arrowup" | "down" | "arrowdown" | "escape"
                    ) {
                        cx.stop_propagation();
                    }
                }
            })
            .child(
                v_flex().w_full().min_w_0().gap_1().child(popover).child(
                    Switch::new(SharedString::from(format!(
                        "revision-picker-manual:{}",
                        self.id
                    )))
                    .label(i18n::text(
                        self.locale,
                        "branch-compare-manual-input",
                    ))
                    .small()
                    .flex_shrink_0()
                    .checked(manual_input)
                    .on_click({
                        let this = this.clone();
                        move |checked, window, cx| {
                            this.update(cx, |picker, cx| {
                                picker.set_manual_input(*checked, window, cx);
                            });
                        }
                    }),
                ),
            )
            .when_some(validation, |view, message| {
                view.child(
                    div()
                        .text_size(crate::theme::scaled_text_size(10.))
                        .text_color(cx.theme().colors.red)
                        .child(shared(message)),
                )
            })
    }
}
