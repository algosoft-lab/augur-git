/** Temporary full-window prompt shown while a repository folder is dragged. */

import { Icon } from '../../components/Icon';
import { useStore } from '../../app/store';
import { t } from '../../i18n/strings';

export function DropOverlay() {
  const translate = useStore((state) => state.t);

  return (
    <div className="drop-overlay" data-testid="drop-overlay" role="status" aria-live="polite">
      <div className="drop-overlay__content">
        <Icon name="download" size={36} />
        <div className="drop-overlay__title">{t(translate, 'drop-overlay-title')}</div>
        <div className="drop-overlay__hint">{t(translate, 'drop-overlay-hint')}</div>
      </div>
    </div>
  );
}
