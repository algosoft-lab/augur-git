/**
 * Test entry point.
 *
 * Each test navigates to the document with the stub runtime already installed,
 * so the application boots exactly as it does inside the real window.
 */

import type { CliStatus } from '../src/bridge/types';

import { test as base, expect, type Page } from '@playwright/test';

import {
  DEFAULT_OPTIONS,
  fixtureRepo,
  longFixtureRepo,
  secondFixtureRepo,
  stubSource,
  type StubOptions,
  type StubRepo
} from './fixtures/stubBackend';

export interface StubApi {
  /** Every command the interface invoked, in order. */
  commands(): { cmd: string; args: Record<string, unknown> }[];
  /** Command names the interface invoked, in order, deduplicated. */
  commandNames(): string[];
  /** Push a backend event into the running application. */
  emit(event: string, payload: unknown): Promise<void>;
  /** Re-announce a repository's status, refs, and log page. */
  announce(repoId: number): Promise<void>;
  /** Change the stubbed window maximize state and emit a native resize event. */
  setMaximized(value: boolean): Promise<void>;
  /** Read the last text the app wrote to the clipboard. */
  clipboard(): Promise<string>;
}

declare global {
  interface Window {
    __STUB__: {
      log: { cmd: string; args: Record<string, unknown> }[];
      clipboard: string;
      emit(name: string, payload: unknown): void;
      setMaximized(value: boolean): void;
      announce(repoId: number): void;
    };
  }
}

/** Options for a test, with the fixture repository already available. */
export interface BootOptions {
  cliStatus?: CliStatus;
  /** Repositories the bootstrap reports as open. */
  open?: StubRepo[];
  /** Milliseconds the comparison's per-file diffs take to arrive. */
  compareDelay?: number;
  /** Hold `start_compare` replies while streaming the matching events first. */
  compareReplyDelay?: number;
  /** Make every comparison fail as a whole with this detail. */
  failCompare?: string;
  /**
   * Commands the backend refuses, with the key and detail it refuses them with.
   */
  refusals?: Record<string, { key: string; detail: string }>;
  /** Make every working-tree file diff fail with this detail. */
  workingDiffFailure?: string;
  /** How long a working-tree file diff takes to arrive. */
  workingDiffDelay?: number;
  /** Fail working-tree diff requests after the first with this detail. */
  workingDiffFailureAfterFirst?: string;
  /** Fail only the first working-tree diff request with this detail. */
  workingDiffFirstFailure?: string;
  /** Leave working-tree diff requests unanswered to verify timeout handling. */
  workingDiffNeverResponds?: boolean;
  /** Fail commit file metadata requests with this detail. */
  commitFilesFailure?: string;
  /** Fail every commit file diff request with this detail. */
  commitDiffFailure?: string;
  /** Fail commit diff requests for these paths. */
  commitDiffFailurePaths?: string[];
  /** Delay commit file diff events by this many milliseconds. */
  commitDiffDelay?: number;
  /** Leave commit file diff requests unanswered to verify timeout handling. */
  commitDiffNeverResponds?: boolean;
  /** Add representative image changes to commit and comparison fixtures. */
  includeImageFixtures?: boolean;
  /** Delay or fail lazy image preview commands. */
  imagePreviewDelay?: number;
  imagePreviewFailure?: string;
  /**
   * Report the browser as Windows, so the platform-only entry points render.
   *
   * `navigator.platform` is the only thing the interface asks, so overriding it
   * is enough to reach a Windows-only surface from a Chromium test on macOS.
   */
  windows?: boolean;
  /** Report macOS so the native-menu title bar can be verified. */
  macos?: boolean;
  /** Initial persisted pane geometry for restart and resize checks. */
  layout?: Partial<{
    sidebar_width: number;
    right_panel_width: number;
    diff_height: number | null;
    file_list_ratio: number;
  }>;
  /** System font families returned by the appearance settings. */
  fontFamilies?: string[];
  /** Initial persisted font preferences. */
  typography?: StubOptions['typography'];
  /** Strategy the toolbar Pull button uses. */
  pullAction?: StubOptions['pullAction'];
  diffLayout?: StubOptions['diffLayout'];
  diffSoftWrap?: StubOptions['diffSoftWrap'];
  showUntracked?: boolean;
  /**
   * Paths the backend is holding because the window was not listening when they
   * arrived, which is the state a launch with a path argument produces.
   */
  pendingPaths?: string[];
  /**
   * Paths the saved workspace lists as open with no repository behind them, so
   * this window has to open them.
   */
  savedTabs?: string[];
  /**
   * The key the saved workspace records as its active tab.
   *
   * Defaults to the first saved tab, as a launch that opened its first
   * repository last would have left it.
   */
  savedActiveTab?: string;
  /** The pool `open_repository` hands out. */
  available?: StubRepo[];
  /** Reject `open_repository` with this error. */
  openFailure?: { key: string; detail: string };
  /** Reject these `run_action` operations, so the failure path is exercised. */
  failingActions?: string[];
  /** Overrides for the merge preflight probe. */
  probeMerge?: Record<string, unknown>;
  /** Overrides for the rebase preflight probe. */
  probeRebase?: Record<string, unknown>;
  /** How long an action stays in progress, so the progress state is observable. */
  actionDelay?: number;
  /** How long `open_repository` takes, and its snapshot 1.5s after it. */
  openDelay?: number;
  /** How long the WSL distribution list takes to arrive. */
  wslDelay?: number;
  /** Window role, from the same query parameter the backend uses. */
  window?: 'main' | 'compare' | 'about' | 'settings';
  /** Initial settings section requested by a direct settings-window entry. */
  settingsSection?: 'general' | 'appearance' | 'layout' | 'shortcuts';
  /** Repository id passed to a compare window. */
  repoId?: number;
}

function optionsFor(options: BootOptions): StubOptions {
  return {
    open: options.open ?? [],
    cliStatus: options.cliStatus,
    compareDelay: options.compareDelay,
    compareReplyDelay: options.compareReplyDelay,
    failCompare: options.failCompare,
    refusals: options.refusals,
    workingDiffFailure: options.workingDiffFailure,
    workingDiffFirstFailure: options.workingDiffFirstFailure,
    workingDiffFailureAfterFirst: options.workingDiffFailureAfterFirst,
    workingDiffDelay: options.workingDiffDelay,
    workingDiffNeverResponds: options.workingDiffNeverResponds,
    commitFilesFailure: options.commitFilesFailure,
    commitDiffFailure: options.commitDiffFailure,
    commitDiffFailurePaths: options.commitDiffFailurePaths,
    commitDiffDelay: options.commitDiffDelay,
    commitDiffNeverResponds: options.commitDiffNeverResponds,
    includeImageFixtures: options.includeImageFixtures,
    imagePreviewDelay: options.imagePreviewDelay,
    imagePreviewFailure: options.imagePreviewFailure,
    pendingPaths: options.pendingPaths,
    savedTabs: options.savedTabs,
    savedActiveTab: options.savedActiveTab,
    available: options.available ?? [fixtureRepo(), secondFixtureRepo()],
    openFailure: options.openFailure,
    failingActions: options.failingActions,
    probeMerge: options.probeMerge,
    probeRebase: options.probeRebase,
    actionDelay: options.actionDelay,
    openDelay: options.openDelay,
    wslDelay: options.wslDelay,
    layout: options.layout,
    fontFamilies: options.fontFamilies,
    typography: options.typography,
    pullAction: options.pullAction,
    diffLayout: options.diffLayout,
    diffSoftWrap: options.diffSoftWrap,
    showUntracked: options.showUntracked
  };
}

/** Load the interface in a page with the stub runtime installed. */
export async function boot(page: Page, options: BootOptions = {}): Promise<StubApi> {
  const role = options.window ?? 'main';
  const params = new URLSearchParams({ window: role });
  if (role === 'compare' && options.repoId !== undefined) {
    params.set('repo', String(options.repoId));
  }
  if (role === 'settings' && options.settingsSection) {
    params.set('section', options.settingsSection);
  }
  if (options.windows || options.macos) {
    await page.addInitScript(
      (platform) => {
        Object.defineProperty(navigator, 'platform', { value: platform, configurable: true });
      },
      options.macos ? 'MacIntel' : 'Win32'
    );
  }
  await page.addInitScript(stubSource(optionsFor(options)));
  await page.goto(`/?${params.toString()}`);
  // Each window surfaces a different root, so the wait matches the role: seeing
  // it means the store has finished booting.
  const root =
    role === 'compare'
      ? '[data-testid="compare-window"], .empty-state'
      : role === 'about'
        ? '[data-testid="about"]'
        : role === 'settings'
          ? '[data-testid="settings-window"]'
          : '[data-testid="welcome"], [data-testid="graph"]';
  await page.waitForSelector(root);
  return {
    async commands() {
      return page.evaluate(() => window.__STUB__.log);
    },
    async commandNames() {
      const names = await page.evaluate(() => window.__STUB__.log.map((entry) => entry.cmd));
      return [...new Set(names)];
    },
    async emit(event, payload) {
      await page.evaluate(([name, body]) => window.__STUB__.emit(name as string, body), [
        event,
        payload
      ] as const);
    },
    async announce(repoId) {
      await page.evaluate((id) => window.__STUB__.announce(id), repoId);
    },
    async setMaximized(value) {
      await page.evaluate((maximized) => {
        window.__STUB__.setMaximized(maximized);
        window.__STUB__.emit('tauri://resize', { width: 1280, height: 800 });
      }, value);
    },
    async clipboard() {
      return page.evaluate(() => window.__STUB__.clipboard);
    }
  };
}

/**
 * Right-click an element.
 *
 * Playwright's `click({ button: "right" })` does not always make Chromium
 * synthesize `contextmenu`, so the mouse is driven directly at the element's
 * centre, which is what a person does.
 */
export async function rightClick(page: Page, selector: string, index = 0): Promise<void> {
  const box = await page.locator(selector).nth(index).boundingBox();
  if (!box) {
    throw new Error(`no box for ${selector}`);
  }
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2, {
    button: 'right'
  });
}

/** The default boot, which is most tests. */
export const test = base.extend<{ stub: StubApi }>({
  stub: async ({ page }, use) => {
    await use(await boot(page));
  }
});

export { expect, DEFAULT_OPTIONS, fixtureRepo, longFixtureRepo, secondFixtureRepo };
