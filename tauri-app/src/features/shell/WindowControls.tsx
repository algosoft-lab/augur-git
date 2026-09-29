import { useEffect, useState } from 'react';

import { getCurrentWindow } from '@tauri-apps/api/window';

import { Icon } from '../../components/Icon';
import { t } from '../../i18n/strings';
import { useStore } from '../../app/store';

export const IS_MACOS = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);

export function WindowControls({
  flushBeforeClose = false,
  maximize = true,
  macosStyle = false
}: {
  flushBeforeClose?: boolean;
  /** Hide the maximize button on a window the backend keeps at a fixed size. */
  maximize?: boolean;
  /** Render macOS traffic-light controls for the undecorated main window. */
  macosStyle?: boolean;
}) {
  const translate = useStore((state) => state.t);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!macosStyle || !maximize) {
      return;
    }

    const window = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const update = () => {
      void window
        .isMaximized()
        .then((value) => {
          if (!disposed) {
            setMaximized(value);
          }
        })
        .catch((error: unknown) => {
          console.warn('[window] failed to read maximize state', error);
        });
    };

    update();
    void window
      .onResized(update)
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch((error: unknown) => {
        console.warn('[window] failed to listen for resize events', error);
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [macosStyle, maximize]);

  if (IS_MACOS && !macosStyle) {
    return null;
  }

  const closeWindow = async () => {
    if (flushBeforeClose) {
      await import('../../bridge/ipc').then((ipc) => ipc.flushState());
    }
    await getCurrentWindow().close();
  };

  const toggleMaximize = async () => {
    const window = getCurrentWindow();
    await window.toggleMaximize();
    setMaximized(await window.isMaximized());
  };

  if (macosStyle) {
    return (
      <div className="window-controls window-controls--macos" data-testid="window-controls">
        <button
          type="button"
          className="window-controls__button window-controls__button--macos window-controls__button--macos-close"
          title={t(translate, 'window-close')}
          aria-label={t(translate, 'window-close')}
          data-testid="window-close"
          onClick={() => void closeWindow()}
        >
          <Icon name="x" size={8} />
        </button>
        <button
          type="button"
          className="window-controls__button window-controls__button--macos window-controls__button--macos-minimize"
          title={t(translate, 'window-minimize')}
          aria-label={t(translate, 'window-minimize')}
          data-testid="window-minimize"
          onClick={() => void getCurrentWindow().minimize()}
        >
          <Icon name="minus" size={8} />
        </button>
        <button
          type="button"
          className="window-controls__button window-controls__button--macos window-controls__button--macos-maximize"
          title={t(translate, maximized ? 'window-restore' : 'window-maximize')}
          aria-label={t(translate, maximized ? 'window-restore' : 'window-maximize')}
          data-testid="window-toggle-maximize"
          onClick={() => void toggleMaximize()}
        >
          <Icon name={maximized ? 'copy' : 'square'} size={7} />
        </button>
      </div>
    );
  }

  return (
    <div className="window-controls" data-testid="window-controls">
      <button
        type="button"
        className="window-controls__button"
        title={t(translate, 'window-minimize')}
        aria-label={t(translate, 'window-minimize')}
        data-testid="window-minimize"
        onClick={() => void getCurrentWindow().minimize()}
      >
        <Icon name="minus" size={12} />
      </button>
      {maximize ? (
        <button
          type="button"
          className="window-controls__button"
          title={t(translate, maximized ? 'window-restore' : 'window-maximize')}
          aria-label={t(translate, maximized ? 'window-restore' : 'window-maximize')}
          data-testid="window-toggle-maximize"
          onClick={() => void toggleMaximize()}
        >
          <Icon name={maximized ? 'copy' : 'square'} size={12} />
        </button>
      ) : null}
      <button
        type="button"
        className="window-controls__button window-controls__button--close"
        title={t(translate, 'window-close')}
        aria-label={t(translate, 'window-close')}
        data-testid="window-close"
        onClick={() => void closeWindow()}
      >
        <Icon name="x" size={13} />
      </button>
    </div>
  );
}
