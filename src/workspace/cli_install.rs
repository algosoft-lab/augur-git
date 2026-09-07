//! File-menu entry points for installing and removing the `augurgit`
//! command in user shell configuration files.
//!
//! Both handlers run the filesystem work on a background thread and then
//! show a per-file report dialog, so a failure (read-only home directory,
//! malformed manual edits, ...) is surfaced instead of being swallowed.

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Div, Window, px};
use gpui_component::{WindowExt, v_flex};

use crate::core::i18n::{self, Locale};
use crate::core::shell_install::{self, ChangeReport, Operation, Outcome};

use super::Workspace;
use super::app_menu::{InstallCli, RemoveCli};

impl Workspace {
    pub(super) fn handle_install_cli(
        &mut self,
        _action: &InstallCli,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let locale = self.locale;
        log::info!("[cli_install] install requested from the File menu");
        cx.spawn_in(window, async move |_, cx| {
            let report =
                cx.background_spawn(async { shell_install::install() })
                    .await;
            if let Err(error) =
                cx.update(|window, cx| open_report_dialog(locale, report, window, cx))
            {
                log::warn!(
                    "[cli_install] window closed before the report dialog: {error}"
                );
            }
        })
        .detach();
    }

    pub(super) fn handle_remove_cli(
        &mut self,
        _action: &RemoveCli,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let locale = self.locale;
        log::info!("[cli_install] removal requested from the File menu");
        cx.spawn_in(window, async move |_, cx| {
            let report =
                cx.background_spawn(async { shell_install::uninstall() })
                    .await;
            if let Err(error) =
                cx.update(|window, cx| open_report_dialog(locale, report, window, cx))
            {
                log::warn!(
                    "[cli_install] window closed before the report dialog: {error}"
                );
            }
        })
        .detach();
    }
}

fn open_report_dialog(
    locale: Locale,
    report: ChangeReport,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(i18n::text(locale, "cli-dialog-title"))
            .width(px(480.))
            .child(report_element(locale, report.clone()))
    });
}

fn report_element(locale: Locale, report: ChangeReport) -> AnyElement {
    let mut column = v_flex().gap_2().text_size(px(13.));

    if report.results.is_empty() {
        let none_key = match report.operation {
            Operation::Install => "cli-install-none",
            Operation::Uninstall => "cli-remove-none",
        };
        return column
            .child(i18n::text(locale, none_key))
            .into_any_element();
    }

    let (applied_key, skipped_key) = match report.operation {
        Operation::Install => ("cli-install-updated", "cli-install-unchanged"),
        Operation::Uninstall => {
            ("cli-remove-updated", "cli-remove-notinstalled")
        }
    };
    let applied = collect_entries(
        &report,
        &match report.operation {
            Operation::Install => Outcome::Updated,
            Operation::Uninstall => Outcome::Removed,
        },
    );
    let skipped = collect_entries(
        &report,
        &match report.operation {
            Operation::Install => Outcome::Unchanged,
            Operation::Uninstall => Outcome::NotInstalled,
        },
    );
    let failed: Vec<String> = report
        .results
        .iter()
        .filter_map(|result| match &result.outcome {
            Outcome::Failed(error) => {
                Some(format!("• {}: {error}", display_path(&result.path)))
            }
            _ => None,
        })
        .collect();

    column = with_group(column, i18n::text(locale, applied_key), applied);
    column = with_group(column, i18n::text(locale, skipped_key), skipped);
    column =
        with_group(column, i18n::text(locale, "cli-install-failed"), failed);
    if report.fallback_binary {
        column = column.child(i18n::text(locale, "cli-binary-fallback"));
    }
    if report.results.iter().any(|result| {
        matches!(result.outcome, Outcome::Updated | Outcome::Unchanged)
    }) {
        column = column.child(i18n::text(locale, "cli-install-hint"));
    }
    column.into_any_element()
}

fn collect_entries(report: &ChangeReport, outcome: &Outcome) -> Vec<String> {
    report
        .results
        .iter()
        .filter(|result| &result.outcome == outcome)
        .map(|result| format!("• {}", display_path(&result.path)))
        .collect()
}

fn display_path(path: &std::path::Path) -> String {
    if path.as_os_str().is_empty() {
        "-".to_string()
    } else {
        path.display().to_string()
    }
}

fn with_group(parent: Div, header: String, entries: Vec<String>) -> Div {
    if entries.is_empty() {
        return parent;
    }
    let mut group = v_flex().gap_1().child(header);
    for entry in entries {
        group = group.child(entry);
    }
    parent.child(group)
}
