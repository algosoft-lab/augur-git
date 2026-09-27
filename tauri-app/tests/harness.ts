/**
 * Test entry point.
 *
 * Each test navigates to the document with the stub runtime already installed,
 * so the application boots exactly as it does inside the real window.
 */

import { test as base, expect, type Page } from "@playwright/test";

import {
  DEFAULT_OPTIONS,
  fixtureRepo,
  secondFixtureRepo,
  stubSource,
  type StubOptions,
  type StubRepo,
} from "./fixtures/stubBackend";

export interface StubApi {
  /** Every command the interface invoked, in order. */
  commands(): { cmd: string; args: Record<string, unknown> }[];
  /** Command names the interface invoked, in order, deduplicated. */
  commandNames(): string[];
  /** Push a backend event into the running application. */
  emit(event: string, payload: unknown): Promise<void>;
  /** Re-announce a repository's status, refs, and log page. */
  announce(repoId: number): Promise<void>;
}

declare global {
  interface Window {
    __STUB__: {
      log: { cmd: string; args: Record<string, unknown> }[];
      emit(name: string, payload: unknown): void;
      announce(repoId: number): void;
    };
  }
}

/** Options for a test, with the fixture repository already available. */
export interface BootOptions {
  /** Repositories the bootstrap reports as open. */
  open?: StubRepo[];
  /** Milliseconds the comparison's per-file diffs take to arrive. */
  compareDelay?: number;
  /**
   * Commands the backend refuses, with the key and detail it refuses them with.
   */
  refusals?: Record<string, { key: string; detail: string }>;
  /** Make every working-tree file diff fail with this detail. */
  workingDiffFailure?: string;
  /** How long a working-tree file diff takes to arrive. */
  workingDiffDelay?: number;
  /**
   * Report the browser as Windows, so the platform-only entry points render.
   *
   * `navigator.platform` is the only thing the interface asks, so overriding it
   * is enough to reach a Windows-only surface from a Chromium test on macOS.
   */
  windows?: boolean;
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
  /** Window role, from the same query parameter the backend uses. */
  window?: "main" | "compare" | "about";
  /** Repository id passed to a compare window. */
  repoId?: number;
}

function optionsFor(options: BootOptions): StubOptions {
  return {
    open: options.open ?? [],
    compareDelay: options.compareDelay,
    refusals: options.refusals,
    workingDiffFailure: options.workingDiffFailure,
    workingDiffDelay: options.workingDiffDelay,
    pendingPaths: options.pendingPaths,
    savedTabs: options.savedTabs,
    available: options.available ?? [fixtureRepo(), secondFixtureRepo()],
    openFailure: options.openFailure,
    failingActions: options.failingActions,
    probeMerge: options.probeMerge,
    probeRebase: options.probeRebase,
  };
}

/** Load the interface in a page with the stub runtime installed. */
export async function boot(
  page: Page,
  options: BootOptions = {},
): Promise<StubApi> {
  const role = options.window ?? "main";
  const params = new URLSearchParams({ window: role });
  if (role === "compare" && options.repoId !== undefined) {
    params.set("repo", String(options.repoId));
  }
  if (options.windows) {
    await page.addInitScript(() => {
      Object.defineProperty(navigator, "platform", {
        value: "Win32",
        configurable: true,
      });
    });
  }
  await page.addInitScript(stubSource(optionsFor(options)));
  await page.goto(`/?${params.toString()}`);
  // Each window surfaces a different root, so the wait matches the role: seeing
  // it means the store has finished booting.
  const root =
    role === "compare"
      ? '[data-testid="compare-window"], .empty-state'
      : role === "about"
        ? '[data-testid="about"]'
        : '[data-testid="welcome"], [data-testid="graph"]';
  await page.waitForSelector(root);
  return {
    async commands() {
      return page.evaluate(() => window.__STUB__.log);
    },
    async commandNames() {
      const names = await page.evaluate(() =>
        window.__STUB__.log.map((entry) => entry.cmd),
      );
      return [...new Set(names)];
    },
    async emit(event, payload) {
      await page.evaluate(
        ([name, body]) => window.__STUB__.emit(name as string, body),
        [event, payload] as const,
      );
    },
    async announce(repoId) {
      await page.evaluate((id) => window.__STUB__.announce(id), repoId);
    },
  };
}

/**
 * Right-click an element.
 *
 * Playwright's `click({ button: "right" })` does not always make Chromium
 * synthesize `contextmenu`, so the mouse is driven directly at the element's
 * centre, which is what a person does.
 */
export async function rightClick(
  page: Page,
  selector: string,
  index = 0,
): Promise<void> {
  const box = await page.locator(selector).nth(index).boundingBox();
  if (!box) {
    throw new Error(`no box for ${selector}`);
  }
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2, {
    button: "right",
  });
}

/** The default boot, which is most tests. */
export const test = base.extend<{ stub: StubApi }>({
  stub: async ({ page }, use) => {
    await use(await boot(page));
  },
});

export { expect, DEFAULT_OPTIONS, fixtureRepo, secondFixtureRepo };
