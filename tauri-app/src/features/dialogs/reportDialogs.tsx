/**
 * Dialogs that report an outcome rather than ask for a decision.
 *
 * A failed operation shows the label that ran and Git's own first line, so the
 * cause is visible without a log.
 */

import { useStore } from '../../app/store';
import { DialogCard } from '../../components/controls';
import { Icon } from '../../components/Icon';
import { t, ta } from '../../i18n/strings';

export function OperationErrorDialog({
  label,
  detail,
  titleKey
}: {
  label: string;
  detail: string;
  /** Which operation failed, so the title names it rather than "command". */
  titleKey: 'merge-error-title' | 'rebase-error-title';
}) {
  const translate = useStore((state) => state.t);
  const closeOverlay = useStore((state) => state.closeOverlay);
  const firstLine = detail.split('\n').find((line) => line.trim().length > 0) ?? detail;

  return (
    <DialogCard
      testId="operation-error-dialog"
      title={
        <>
          <Icon name="triangle-alert" size={16} /> {ta(translate, titleKey, { label })}
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
          {t(translate, 'rebase-error-close')}
        </button>
      }
    />
  );
}
