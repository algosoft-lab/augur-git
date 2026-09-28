import { useState } from "react";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { Icon } from "../../components/Icon";
import { t } from "../../i18n/strings";
import { useStore } from "../../app/store";

export const IS_MACOS =
  typeof navigator !== "undefined" && /Mac/i.test(navigator.platform);

export function WindowControls({
  flushBeforeClose = false,
}: {
  flushBeforeClose?: boolean;
}) {
  const translate = useStore((state) => state.t);
  const [maximized, setMaximized] = useState(false);

  if (IS_MACOS) {
    return null;
  }

  return (
    <div className="window-controls" data-testid="window-controls">
      <button
        type="button"
        className="window-controls__button"
        title={t(translate, "window-minimize")}
        aria-label={t(translate, "window-minimize")}
        data-testid="window-minimize"
        onClick={() => void getCurrentWindow().minimize()}
      >
        <Icon name="minus" size={12} />
      </button>
      <button
        type="button"
        className="window-controls__button"
        title={t(translate, maximized ? "window-restore" : "window-maximize")}
        aria-label={t(translate, maximized ? "window-restore" : "window-maximize")}
        data-testid="window-toggle-maximize"
        onClick={async () => {
          const window = getCurrentWindow();
          await window.toggleMaximize();
          setMaximized(await window.isMaximized());
        }}
      >
        <Icon name={maximized ? "copy" : "square"} size={12} />
      </button>
      <button
        type="button"
        className="window-controls__button window-controls__button--close"
        title={t(translate, "window-close")}
        aria-label={t(translate, "window-close")}
        data-testid="window-close"
        onClick={async () => {
          if (flushBeforeClose) {
            await import("../../bridge/ipc").then((ipc) => ipc.flushState());
          }
          await getCurrentWindow().close();
        }}
      >
        <Icon name="x" size={13} />
      </button>
    </div>
  );
}
