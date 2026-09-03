//! Per-extension detail card rendering for the extensions panel.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    switch::Switch,
    v_flex,
};

use crate::core::extension::{
    EventTrigger, ExtensionRunTrigger, ExtensionSettings, ExtensionSource,
    RepositoryRunResult, SettingValue,
};
use crate::core::i18n::{self, Locale};

use super::{
    ExtensionRow, ExtensionsPanel, ExtensionsPanelEvent, capabilities_summary,
    settings,
};

/// Renders the detail card of the selected extension: title, action buttons,
/// metadata, trust warnings, event subscriptions, settings, and run history.
pub(super) fn detail_card(
    panel: &ExtensionsPanel,
    entity: &Entity<ExtensionsPanel>,
    row: &ExtensionRow,
    cx: &Context<ExtensionsPanel>,
) -> AnyElement {
    let colors = cx.theme().colors.clone();
    let locale = panel.locale;
    let run_once_label = i18n::text(locale, "extensions-run-once");
    let cancel_label = i18n::text(locale, "extensions-cancel");
    let save_settings_label = i18n::text(locale, "extensions-save-settings");
    let trust_label_text = i18n::text(locale, "extensions-trust");
    let trusted_label = i18n::text(locale, "extensions-trusted");
    let subscribe_label = i18n::text(locale, "extensions-subscribe");
    let subscribed_label = i18n::text(locale, "extensions-subscribed");
    let events_title = i18n::text(locale, "extensions-event-subscriptions");
    let next_run_label = i18n::text(locale, "extensions-next-run");
    let settings_title = i18n::text(locale, "extensions-settings");
    let history_title = i18n::text(locale, "extensions-recent-runs");
    let manual_capability = i18n::text(locale, "extensions-manual");
    let events_capability = i18n::text(locale, "extensions-events");
    let source_label = i18n::text(locale, "extensions-source");
    let fingerprint_label = i18n::text(locale, "extensions-fingerprint");
    let version_label = i18n::text(locale, "extensions-version");
    let author_label = i18n::text(locale, "extensions-author");
    let path_label = i18n::text(locale, "extensions-path");
    let bundled_label = i18n::text(locale, "extensions-bundled");
    let local_source_label = i18n::text(locale, "extensions-local-directory");
    let uninstall_label = i18n::text(locale, "extensions-uninstall");
    let untrusted_warning = i18n::text(locale, "extensions-untrusted-warning");

    let id = row.definition.package.manifest.id.clone();
    let id_for_run = id.clone();
    let id_for_trust = id.clone();
    let panel_for_trust = entity.clone();
    let panel_for_run = entity.clone();
    let panel_for_cancel = entity.clone();
    let panel_for_save = entity.clone();
    let panel_for_uninstall = entity.clone();
    let id_for_uninstall = id.clone();
    let id_for_cancel = id.clone();
    let id_for_save = id.clone();
    let trust_label = if row.settings.trusted {
        trusted_label.clone()
    } else {
        trust_label_text.clone()
    };
    let source_name = match row.definition.package.source {
        ExtensionSource::Bundled => bundled_label.clone(),
        ExtensionSource::LocalDirectory => local_source_label.clone(),
    };
    let source = format!(
        "{source_label}: {source_name} · {fingerprint_label}: {}",
        row.definition.package.fingerprint
    );
    let path = row
        .definition
        .package
        .root
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| bundled_label.clone());
    let author = row
        .definition
        .package
        .manifest
        .author
        .clone()
        .unwrap_or_else(|| "—".to_string());
    let version = row.definition.package.manifest.version.clone();
    let status = panel.statuses.get(&id).cloned();
    let capabilities =
        capabilities_summary(row, &manual_capability, &events_capability);
    let history = row
        .history
        .iter()
        .rev()
        .take(3)
        .map(|record| {
            let trigger = trigger_display(locale, &record.trigger);
            let repository_summary = record
                .repositories
                .iter()
                .map(|repository| {
                    let result = match &repository.result {
                        RepositoryRunResult::Success { summary } => {
                            i18n::text_args(
                                locale,
                                "extensions-history-ok",
                                &[("summary", summary)],
                            )
                        }
                        RepositoryRunResult::Failed { code, summary } => {
                            i18n::text_args(
                                locale,
                                "extensions-history-failed",
                                &[("code", code), ("summary", summary)],
                            )
                        }
                    };
                    format!("{} — {result}", repository.display_name)
                })
                .collect::<Vec<_>>()
                .join("; ");
            div()
                .text_color(colors.muted_foreground)
                .text_size(crate::theme::scaled_text_size(10.))
                .child(SharedString::from(format!(
                    "{} · {}",
                    i18n::text_args(
                        locale,
                        "extensions-history-run",
                        &[
                            ("run_id", &record.run_id.to_string()),
                            ("trigger", &trigger),
                            ("summary", &record.summary),
                        ],
                    ),
                    repository_summary
                )))
        })
        .collect::<Vec<_>>();
    let can_uninstall = !row.definition.package.bundled;
    let has_settings = !row.definition.package.manifest.settings.is_empty();
    let settings = row
        .definition
        .package
        .manifest
        .settings
        .iter()
        .map(|(key, definition)| {
            settings::render_setting(panel, row, key, definition, &colors, cx)
        })
        .collect::<Vec<_>>();
    let events = row
        .definition
        .package
        .manifest
        .event_triggers()
        .into_iter()
        .map(|trigger| {
            let trigger_id = trigger.id.clone();
            let extension_id = id.clone();
            let subscribed = row.settings.is_subscribed(&trigger_id);
            let panel = entity.clone();
            let subscription_label = if subscribed {
                subscribed_label.clone()
            } else {
                subscribe_label.clone()
            };
            let mut label =
                format!("{} · {}", trigger.label(), trigger.event_type);
            if let Some(description) = trigger.description.as_deref() {
                label.push_str(" — ");
                label.push_str(description);
            }
            if let Some(schedule) =
                next_event_hint(locale, &trigger, &row.settings)
            {
                label.push_str(" · ");
                label.push_str(&next_run_label);
                label.push_str(": ");
                label.push_str(&schedule);
            }
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .text_color(colors.foreground)
                        .text_size(crate::theme::scaled_text_size(11.))
                        .child(SharedString::from(label)),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Switch::new(SharedString::from(format!(
                                "extension-event-{extension_id}-{trigger_id}"
                            )))
                            .small()
                            .checked(subscribed)
                            .on_click(move |_event, _window, cx| {
                                panel.update(cx, |_panel, cx| {
                                    cx.emit(ExtensionsPanelEvent::SubscriptionChanged {
                                        extension_id: extension_id.clone(),
                                        trigger_id: trigger_id.clone(),
                                        subscribed: !subscribed,
                                    });
                                });
                            }),
                        )
                        .child(
                            div()
                                .text_color(colors.muted_foreground)
                                .text_size(crate::theme::scaled_text_size(10.))
                                .child(SharedString::from(subscription_label)),
                        ),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();
    v_flex()
        .w_full()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::BOLD)
                        .text_color(colors.foreground)
                        .child(SharedString::from(
                            row.definition.package.manifest.name.clone(),
                        )),
                )
                .child(
                    div()
                        .text_color(colors.muted_foreground)
                        .text_size(crate::theme::scaled_text_size(10.))
                        .child(SharedString::from(capabilities)),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    Button::new(SharedString::from(format!(
                        "extension-trust-{id}"
                    )))
                    .label(trust_label)
                    .ghost()
                    .small()
                    .on_click(
                        move |_event, _window, cx| {
                            panel_for_trust.update(cx, |panel, cx| {
                                panel.handle_trust_click(
                                    id_for_trust.clone(),
                                    cx,
                                )
                            });
                        },
                    ),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "extension-run-{id}"
                    )))
                    .label(run_once_label.clone())
                    .primary()
                    .small()
                    .on_click(
                        move |_event, _window, cx| {
                            panel_for_run.update(cx, |_panel, cx| {
                                cx.emit(ExtensionsPanelEvent::RunNow(
                                    id_for_run.clone(),
                                ))
                            });
                        },
                    ),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "extension-cancel-{id}"
                    )))
                    .label(cancel_label.clone())
                    .ghost()
                    .small()
                    .on_click(
                        move |_event, _window, cx| {
                            panel_for_cancel.update(cx, |_panel, cx| {
                                cx.emit(ExtensionsPanelEvent::Cancel(
                                    id_for_cancel.clone(),
                                ))
                            });
                        },
                    ),
                )
                .when(has_settings, |element| {
                    element.child(
                        Button::new(SharedString::from(format!(
                            "extension-save-{id}"
                        )))
                        .label(save_settings_label.clone())
                        .ghost()
                        .small()
                        .on_click(
                            move |_event, _window, cx| {
                                panel_for_save.update(cx, |_panel, cx| {
                                    cx.emit(ExtensionsPanelEvent::SaveSettings(
                                        id_for_save.clone(),
                                    ))
                                });
                            },
                        ),
                    )
                })
                .when(can_uninstall, |element| {
                    element.child(
                        Button::new(SharedString::from(format!(
                            "extension-uninstall-{id}"
                        )))
                        .label(uninstall_label.clone())
                        .ghost()
                        .small()
                        .on_click(
                            move |_event, _window, cx| {
                                panel_for_uninstall.update(cx, |_panel, cx| {
                                    cx.emit(ExtensionsPanelEvent::Uninstall(
                                        id_for_uninstall.clone(),
                                    ))
                                });
                            },
                        ),
                    )
                }),
        )
        .child(
            div()
                .text_color(colors.muted_foreground)
                .text_size(crate::theme::scaled_text_size(11.))
                .child(SharedString::from(
                    row.definition.package.manifest.description.clone(),
                )),
        )
        .child(
            div()
                .text_color(colors.muted_foreground)
                .text_size(crate::theme::scaled_text_size(10.))
                .child(SharedString::from(source)),
        )
        .child(
            div()
                .text_color(colors.muted_foreground)
                .text_size(crate::theme::scaled_text_size(10.))
                .child(SharedString::from(format!(
                    "{version_label}: {version} · {author_label}: {author}"
                ))),
        )
        .child(
            div()
                .text_color(colors.muted_foreground)
                .text_size(crate::theme::scaled_text_size(10.))
                .child(SharedString::from(format!("{path_label}: {path}"))),
        )
        .when(!row.settings.trusted, |element| {
            element
                .child(
                    div()
                        .text_color(colors.warning)
                        .text_size(crate::theme::scaled_text_size(11.))
                        .child(SharedString::from(untrusted_warning.clone())),
                )
                .when(panel.trust_confirmations.contains(&id), |element| {
                    element.child(
                        div()
                            .text_color(colors.warning)
                            .text_size(crate::theme::scaled_text_size(11.))
                            .child(SharedString::from(i18n::text(
                                locale,
                                "extensions-permission-confirm",
                            ))),
                    )
                })
        })
        .when(!events.is_empty(), |element| {
            element
                .child(
                    div()
                        .text_color(colors.foreground)
                        .text_size(crate::theme::scaled_text_size(11.))
                        .child(SharedString::from(events_title.clone())),
                )
                .children(events)
        })
        .when(!settings.is_empty(), |element| {
            element
                .child(
                    div()
                        .text_color(colors.foreground)
                        .text_size(crate::theme::scaled_text_size(11.))
                        .child(SharedString::from(settings_title.clone())),
                )
                .children(settings)
        })
        .when(!history.is_empty(), |element| {
            element
                .child(
                    div()
                        .text_color(colors.foreground)
                        .text_size(crate::theme::scaled_text_size(11.))
                        .child(SharedString::from(history_title.clone())),
                )
                .children(history)
        })
        .when_some(status, |element, status| {
            element.child(
                div()
                    .text_color(colors.blue)
                    .text_size(crate::theme::scaled_text_size(11.))
                    .child(SharedString::from(status)),
            )
        })
        .into_any_element()
}

fn trigger_display(locale: Locale, trigger: &ExtensionRunTrigger) -> String {
    match trigger {
        ExtensionRunTrigger::Manual => {
            i18n::text(locale, "extensions-history-trigger-manual")
        }
        ExtensionRunTrigger::Schedule {
            trigger_id,
            event_type,
        }
        | ExtensionRunTrigger::Repository {
            trigger_id,
            event_type,
        } => i18n::text_args(
            locale,
            "extensions-history-trigger-event",
            &[("event_type", event_type), ("trigger_id", trigger_id)],
        ),
    }
}

fn next_event_hint(
    locale: Locale,
    trigger: &EventTrigger,
    settings: &ExtensionSettings,
) -> Option<String> {
    match trigger.event_type.as_str() {
        "schedule.daily" => trigger
            .time_setting
            .as_deref()
            .and_then(|key| settings.values.get(key))
            .and_then(|value| match value {
                SettingValue::Time(time) => Some(i18n::text_args(
                    locale,
                    "extensions-daily-at",
                    &[("time", time.as_str())],
                )),
                _ => None,
            }),
        "schedule.interval" => trigger
            .interval_setting
            .as_deref()
            .and_then(|key| settings.values.get(key))
            .and_then(|value| match value {
                SettingValue::Integer(minutes) => Some(i18n::text_args(
                    locale,
                    "extensions-every-minutes",
                    &[("minutes", &minutes.to_string())],
                )),
                _ => None,
            }),
        _ => Some(i18n::text(locale, "extensions-on-event")),
    }
}
