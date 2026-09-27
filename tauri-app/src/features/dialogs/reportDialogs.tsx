/**
 * Dialogs that report an outcome rather than ask for a decision.
 *
 * A failed operation shows the label that ran and Git's own first line, so the
 * cause is visible without a log. The shell installer groups its per-file
 * results by outcome and picks its wording from the operation, because a report
 * that says "added" after a removal is worse than no report.
 */

import { useStore } from "../../app/store";
import type { ChangeReport, Outcome } from "../../bridge/types";
import { DialogCard } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { t, ta } from "../../i18n/strings";

/** The catalog key naming one outcome of the installer's work on a file. */
function outcomeKey(operation: ChangeReport["operation"], outcome: Outcome): string {
  if ("failed" in outcome) {
    return "cli-install-failed";
  }
  if ("removed" in outcome) {
    return "cli-remove-updated";
  }
  if ("notInstalled" in outcome) {
    return "cli-remove-notinstalled";
  }
  if ("updated" in outcome) {
    // A removal that changed a file is reported as removed above, so `updated`
    // here is always an install.
    return "cli-install-updated";
  }
  // `unchanged` is install-only: a removal with nothing to remove is
  // `notInstalled`.
  return operation === "install" ? "cli-install-unchanged" : "cli-remove-notinstalled";
}

export function OperationErrorDialog({
  label,
  detail,
  titleKey,
}: {
  label: string;
  detail: string;
  /** Which operation failed, so the title names it rather than "command". */
  titleKey: "merge-error-title" | "rebase-error-title";
}) {
  const translate = useStore((state) => state.t);
  const closeOverlay = useStore((state) => state.closeOverlay);
  const firstLine = detail.split("\n").find((line) => line.trim().length > 0) ?? detail;

  return (
    <DialogCard
      testId="operation-error-dialog"
      title={
        <>
          <Icon name="triangle-alert" size={16} />{" "}
          {ta(translate, titleKey, { label })}
        </>
      }
      onBackdrop={closeOverlay}
      body={
        <>
          <div className="mono muted" data-testid="operation-error-label">
            {label}
          </div>
          <pre className="status-conflict" data-testid="operation-error-detail">
            {firstLine}
          </pre>
        </>
      }
      footer={
        <button
          type="button"
          className="tool-button tool-button--primary"
          onClick={closeOverlay}
          data-testid="operation-error-close"
        >
          {t(translate, "rebase-error-close")}
        </button>
      }
    />
  );
}

export function CliReportDialog({ report }: { report: ChangeReport }) {
  const translate = useStore((state) => state.t);
  const closeOverlay = useStore((state) => state.closeOverlay);

  const failed = report.results.filter((entry) => "failed" in entry.outcome);
  const changed = report.results.filter(
    (entry) => "updated" in entry.outcome || "removed" in entry.outcome,
  );
  const skipped = report.results.filter(
    (entry) => "unchanged" in entry.outcome || "notInstalled" in entry.outcome,
  );

  const group = (entries: typeof report.results) => (
    <ul className="stack stack--tight" style={{ margin: 0, paddingLeft: 18 }}>
      {entries.map((entry) => (
        <li key={entry.path}>
          {t(translate, outcomeKey(report.operation, entry.outcome))}{" "}
          <span className="mono">{entry.path}</span>
          {"failed" in entry.outcome ? (
            <span className="status-conflict"> — {entry.outcome.failed}</span>
          ) : null}
        </li>
      ))}
    </ul>
  );

  return (
    <DialogCard
      testId="cli-report-dialog"
      title={t(translate, "cli-dialog-title")}
      onBackdrop={closeOverlay}
      width={520}
      body={
        <>
          {report.results.length === 0 ? (
            <div className="muted" data-testid="cli-report-none">
              {t(
                translate,
                report.operation === "install" ? "cli-install-none" : "cli-remove-none",
              )}
            </div>
          ) : null}
          {changed.length ? group(changed) : null}
          {skipped.length ? group(skipped) : null}
          {/* A failure replaces the path's own line, because the reason is the
              thing worth reading. */}
          {failed.length ? group(failed) : null}
          {report.fallback_binary ? (
            <div className="status-mod">{t(translate, "cli-binary-fallback")}</div>
          ) : null}
          {changed.length ? (
            <div className="muted">{t(translate, "cli-install-hint")}</div>
          ) : null}
        </>
      }
      footer={
        <button
          type="button"
          className="tool-button tool-button--primary"
          onClick={closeOverlay}
          data-testid="cli-report-close"
        >
          {t(translate, "dialog-cancel")}
        </button>
      }
    />
  );
}
