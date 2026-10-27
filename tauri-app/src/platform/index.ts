/**
 * Platform seam for the capabilities a browser cannot reach directly.
 *
 * The desktop implementations call the Tauri plugins; the web implementations
 * fall back to browser APIs or to server-backed equivalents (the directory
 * picker walks the server filesystem, for example). Feature code calls only
 * these functions and never a plugin import, so one bundle serves both.
 */

import { isDesktop, isWeb } from '../bridge/transport';

/** Copy text to the clipboard. */
export async function copyText(text: string): Promise<void> {
  if (isDesktop) {
    const { writeText } = await import('@tauri-apps/plugin-clipboard-manager');
    await writeText(text);
    return;
  }
  await navigator.clipboard.writeText(text);
}

/** Open a URL in the user's browser. */
export async function openExternal(url: string): Promise<void> {
  if (isDesktop) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
    return;
  }
  window.open(url, '_blank', 'noopener');
}

/** Ask the user to choose a directory. Resolves null on cancel. */
export async function pickDirectory(title: string): Promise<string | null> {
  if (isDesktop) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({ directory: true, title });
    return typeof selected === 'string' ? selected : null;
  }
  return requestServerDirectory(title);
}

/** Ask the user where to save a file; returns the chosen path or null. */
export async function pickSavePath(title: string, defaultName: string): Promise<string | null> {
  if (isDesktop) {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const selected = await save({ title, defaultPath: defaultName });
    return typeof selected === 'string' ? selected : null;
  }
  // The browser cannot name server files; downloads are the web equivalent.
  return null;
}

/**
 * Open an auxiliary surface (about, settings, compare) that the desktop shows
 * in its own window. On the web the same page role is navigated to in this
 * tab; the browser's back button returns to the main view.
 */
export function openWindowSurface(surface: 'about' | 'settings' | `compare-${number}`): void {
  if (isDesktop) {
    void import('../bridge/ipc').then((ipc) => {
      if (surface === 'about') {
        void ipc.openAboutWindow();
      } else if (surface === 'settings') {
        void ipc.openSettingsWindow();
      } else {
        void ipc.openCompareWindow(Number(surface.split('-')[1]));
      }
    });
    return;
  }
  window.location.href = `/index.html?window=${surface.replace('compare-', 'compare&repo=')}`;
}

/** Whether the update checker applies to this runtime. */
export const updatesSupported = isDesktop;

/** Whether the CLI installation applies to this runtime. */
export const cliSupported = isDesktop;

/** Whether WSL repositories can be reached from this runtime. */
export const wslSupported = isDesktop;

/** Whether the window has native chrome the app must draw itself. */
export const hasNativeWindow = isDesktop;

// ===== Server-backed directory picker =====

const PICK_EVENT = 'augur:pick-directory';

interface PickRequest {
  title: string;
  resolve: (path: string | null) => void;
}

function requestServerDirectory(title: string): Promise<string | null> {
  return new Promise((resolve) => {
    window.dispatchEvent(new CustomEvent<PickRequest>(PICK_EVENT, { detail: { title, resolve } }));
  });
}

/** The event name the web directory-picker dialog listens for. */
export const PICK_DIRECTORY_EVENT = PICK_EVENT;

export interface DirectoryListing {
  path: string;
  root: string;
  parent: string | null;
  entries: { name: string; path: string }[];
}

/** List one directory of the server filesystem (web only). */
export async function browseServerDirectory(path: string): Promise<DirectoryListing> {
  const token = sessionStorage.getItem('augur-web-token');
  const response = await fetch('/api/browse', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {})
    },
    body: JSON.stringify({ path })
  });
  if (!response.ok) {
    throw new Error(`directory listing failed: HTTP ${response.status}`);
  }
  return (await response.json()) as DirectoryListing;
}

/** Report the browser's focus so the auto-refresh monitor stays honest. */
export function reportFocus(focused: boolean): void {
  if (isWeb) {
    void import('../bridge/ipc').then((ipc) =>
      ipc
        .setMainWindowFocused(focused)
        .catch(() => {
          // Focus reporting is best-effort; a restart may have dropped it.
        })
    );
  }
}
