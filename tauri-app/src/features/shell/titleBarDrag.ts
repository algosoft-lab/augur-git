import type { MouseEvent } from "react";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { IS_MACOS } from "./WindowControls";

const NON_DRAG_SELECTOR =
  "button, input, textarea, select, a, [role=tab], .menu, .compare__picker";

export function handleTitleBarMouseDown(event: MouseEvent<HTMLElement>): void {
  if (IS_MACOS || event.button !== 0) {
    return;
  }

  const target = event.target;
  if (!(target instanceof Element) || target.closest(NON_DRAG_SELECTOR)) {
    return;
  }

  const window = getCurrentWindow();
  if (event.detail === 2) {
    event.preventDefault();
    void window.toggleMaximize().catch((error: unknown) => {
      console.error("[window] failed to toggle maximize", error);
    });
    return;
  }

  void window.startDragging().catch((error: unknown) => {
    console.error("[window] failed to start dragging", error);
  });
}
