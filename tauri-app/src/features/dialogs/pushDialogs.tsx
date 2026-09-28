/**
 * Dialogs that guard the two pushes that can lose work.
 *
 * A force push replaces the remote history, and a branch with no upstream
 * cannot be pushed at all until one is set. Both state the consequence and
 * the branch they apply to before anything is sent.
 */

import { Icon } from '../../components/Icon';
import { DialogCard } from '../../components/controls';
import { useStore } from '../../app/store';
import { t, ta } from '../../i18n/strings';
import { useActiveRepoId } from './useActiveRepo';

export function ForcePushDialog() {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="force-push-dialog"
      title={
        <>
          <Icon name="triangle-alert" size={16} /> {t(translate, 'push-force-title')}
        </>
      }
      onBackdrop={closeOverlay}
      body={<div className="muted">{t(translate, 'push-force-warning')}</div>}
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="force-push-cancel"
          >
            {t(translate, 'push-force-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--danger"
            data-testid="force-push-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, { action: 'pushForce' });
              }
            }}
          >
            {t(translate, 'push-force-confirm')}
          </button>
        </>
      }
    />
  );
}

export function PushUpstreamDialog({ branch, remote }: { branch: string; remote: string }) {
  const translate = useStore((state) => state.t);
  const repoId = useActiveRepoId();
  const closeOverlay = useStore((state) => state.closeOverlay);
  const runAction = useStore((state) => state.runAction);

  return (
    <DialogCard
      testId="push-upstream-dialog"
      title={
        <>
          <Icon name="chevron-down" size={16} /> {t(translate, 'push-upstream-title')}
        </>
      }
      onBackdrop={closeOverlay}
      body={
        <div className="muted">{ta(translate, 'push-upstream-warning', { branch, remote })}</div>
      }
      footer={
        <>
          <button
            type="button"
            className="tool-button"
            onClick={closeOverlay}
            data-testid="push-upstream-cancel"
          >
            {t(translate, 'push-upstream-cancel')}
          </button>
          <button
            type="button"
            className="tool-button tool-button--primary"
            data-testid="push-upstream-confirm"
            onClick={() => {
              if (repoId) {
                closeOverlay();
                void runAction(repoId, {
                  action: 'pushSetUpstream',
                  remote,
                  branch
                });
              }
            }}
          >
            {t(translate, 'push-upstream-confirm')}
          </button>
        </>
      }
    />
  );
}
