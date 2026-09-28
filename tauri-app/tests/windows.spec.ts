import { expect, test } from "@playwright/test";

import { boot, fixtureRepo } from "./harness";

/**
 * The standalone comparison window and the settings surface.
 *
 * The comparison window is a separate document that talks to the same backend,
 * so it is booted with the same window role the backend puts on the URL.
 */

test.describe("comparison window", () => {
  test("names the panel in its own header", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });
    await expect(page.getByTestId("compare-window")).toBeVisible();
    await expect(page.getByTestId("compare-title")).toHaveText(
      "Revision comparison",
    );
  });

  test("keeps custom window controls above the compare inputs on Windows", async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: "compare",
      repoId: 7,
      windows: true,
    });
    const titlebar = await page.locator(".window-titlebar").boundingBox();
    const controls = await page.getByTestId("window-controls").boundingBox();
    const header = await page.locator(".compare__header").boundingBox();
    const base = await page.getByTestId("compare-input-Base").boundingBox();
    expect(titlebar).not.toBeNull();
    expect(controls).not.toBeNull();
    expect(header).not.toBeNull();
    expect(base).not.toBeNull();
    expect(controls!.y + controls!.height).toBeLessThanOrEqual(header!.y);
    expect(base!.y).toBeGreaterThanOrEqual(header!.y);
    await expect(page.getByTestId("window-close")).toBeVisible();
  });

  test("uses native-menu spacing on macOS and keeps tabs in the title bar", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], macos: true });
    await expect(page.getByTestId("title-bar")).toHaveClass(/title-bar--macos/);
    await expect(page.getByTestId("tab-bar")).toBeVisible();
    await expect(page.getByTestId("menu-file-trigger")).toHaveCount(0);
    await expect(page.getByTestId("window-controls")).toHaveCount(0);
    const titlebar = await page.getByTestId("title-bar").boundingBox();
    const tabs = await page.getByTestId("tab-bar").boundingBox();
    expect(tabs!.y).toBe(titlebar!.y);
    expect(tabs!.height).toBe(titlebar!.height - 1);
  });

  test("compares two revisions and lists the files", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });
    await expect(page.getByTestId("compare-window")).toBeVisible();

    // The endpoints start empty and the revisions the backend offered are
    // reachable from the picker.
    await page.getByTestId("compare-toggle-Base").click();
    await expect(page.getByTestId("compare-option-local-refs/heads/master")).toBeVisible();
    await page.getByTestId("compare-option-local-refs/heads/master").click();

    await page.getByTestId("compare-toggle-Target").click();
    await expect(page.getByTestId("compare-option-local-refs/heads/feature/tauri")).toBeVisible();
    await page.getByTestId("compare-option-local-refs/heads/feature/tauri").click();

    // The comparison arrives as its own event, and the window reports the
    // files it found.
    await expect(page.getByTestId("compare-file-src/lib.rs")).toBeVisible();
    await expect(page.getByTestId("compare-file-src/new.rs")).toBeVisible();
  });

  test("replays comparison events emitted before the command reply", async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: "compare",
      repoId: 7,
      compareReplyDelay: 90,
    });

    // The fixture emits its file list, per-file diffs, and finished event while
    // start_compare is still waiting to return the request id.
    await expect(page.getByTestId("compare-file-src/lib.rs")).toBeVisible();
    await expect(page.getByTestId("diff-file-header")).toHaveCount(2);
    await expect(page.getByTestId("compare-progress")).toHaveCount(0);
    await expect(page.getByTestId("compare-diff")).toContainText("count += 2");
  });

  test("reports a comparison that failed as a whole, not as no changes", async ({
    page,
  }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: "compare",
      repoId: 7,
      failCompare: "fatal: bad revision 'refs/heads/gone'",
    });

    // The sentence names the failure wherever the window would otherwise have
    // claimed success or invited another choice, and Git's words are not lost.
    await expect(page.getByTestId("compare-request-error")).toHaveText(
      "Unable to load revision comparison",
    );
    await expect(page.getByTestId("compare-request-error")).toHaveAttribute(
      "title",
      "fatal: bad revision 'refs/heads/gone'",
    );
    await expect(page.getByTestId("compare-files-empty")).toContainText(
      "Unable to load revision comparison",
    );
    await expect(page.getByText("The selected revisions have no file changes")).toHaveCount(0);
  });

  test("shows every changed file at once, and one file on request", async ({
    page,
  }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });

    await page.getByTestId("compare-toggle-Base").click();
    await page.getByTestId("compare-option-local-refs/heads/master").click();
    await page.getByTestId("compare-toggle-Target").click();
    await page.getByTestId("compare-option-local-refs/heads/feature/tauri").click();
    await expect(page.getByTestId("compare-file-src/lib.rs")).toBeVisible();

    // The aggregate row is chosen by default and both files appear as their own
    // documents, each under its own path header.
    await expect(page.getByTestId("compare-all-files")).toHaveClass(/is-selected/);
    const headers = page.getByTestId("diff-file-header");
    await expect(headers).toHaveCount(2);
    await expect(headers.nth(0)).toContainText("src/lib.rs");
    await expect(headers.nth(1)).toContainText("src/new.rs");

    // Choosing a file narrows the same list to that one.
    await page.getByTestId("compare-file-src/new.rs").click();
    await expect(page.getByTestId("compare-all-files")).not.toHaveClass(/is-selected/);
    await expect(page.getByTestId("compare-file-src/lib.rs")).toBeVisible();
    await expect(page.getByTestId("diff-file-header")).toHaveCount(1);
    await expect(page.getByTestId("diff-file-header")).toContainText("src/new.rs");
    const starts = await page.evaluate(
      () => (window as any).__STUB__.log.filter((entry: any) => entry.cmd === "start_compare").length,
    );
    // Selecting a file only narrows the current result; it must not restart Git.
    await page.getByTestId("compare-file-src/lib.rs").click();
    await expect(page.getByTestId("diff-file-header")).toContainText("src/lib.rs");
    expect(
      await page.evaluate(
        () => (window as any).__STUB__.log.filter((entry: any) => entry.cmd === "start_compare").length,
      ),
    ).toBe(starts);
  });

  test("groups the offered revisions by kind, each named by its kind", async ({
    page,
  }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });

    await page.getByTestId("compare-input-Base").fill("");
    await page.getByTestId("compare-toggle-Base").click();
    const picker = page.locator(".compare__picker-options");
    await expect(picker).toContainText("Branches");
    await expect(picker).toContainText("remote");
    await expect(picker).toContainText("Tags");

    // Every entry says what kind of thing it is, so a remote branch and a local
    // one are distinguishable and a commit carries its subject.
    const options = page.locator(".compare__picker-option");
    await expect(options.nth(0)).toHaveText("local · master");
    await expect(options.nth(2)).toHaveText("remote · origin/master");
    await expect(options.nth(3)).toHaveText("tag · v1.1.0");
    await expect(options.nth(4)).toContainText(
      "commit · 13c6ef3 · Add the Tauri command surface",
    );
  });

  test("counts the documents as a comparison streams in", async ({ page }) => {
    // Slow the diffs down so the in-flight state is observable at all: the real
    // worker streams them one file at a time.
    await boot(page, {
      open: [fixtureRepo()],
      window: "compare",
      repoId: 7,
      compareDelay: 400,
    });

    // Progress, because a window that looks finished and is not is worse than
    // one that admits it is working. The shape is asserted rather than the exact
    // intermediate value, because how fast a file arrives is the worker's
    // business and not the interface's.
    const progress = page.getByTestId("compare-progress");
    await expect(progress).toBeVisible();
    await expect(progress).toHaveText(/^\d+ \/ 2$/);
    // It goes away once everything has arrived.
    await expect(progress).toHaveCount(0);
    await expect(page.getByTestId("compare-file-src/lib.rs")).toBeVisible();
  });

  test("exports a comparison between a ref and a typed object id", async ({
    page,
  }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });

    // A typed object id is a revision like any other, so the export is offered
    // for it; what cannot be exported is an endpoint that names nothing.
    await page.getByTestId("compare-input-Target").fill(
      "0123456789abcdef0123456789abcdef01234567",
    );
    await expect(page.getByTestId("compare-export")).toBeEnabled();
    await page.getByTestId("compare-input-Target").fill("not a revision");
    await expect(page.getByTestId("compare-export")).toBeDisabled();
  });

  test("suppresses the suggestions when free-form entry is on", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });

    // A list that keeps changing under a typed object id is not a list.
    await page.getByTestId("compare-toggle-Base").click();
    await expect(page.locator(".compare__picker-options")).toBeVisible();
    await page.getByTestId("compare-manual-Base").click();
    await expect(page.locator(".compare__picker-options")).toHaveCount(0);
    // The field still takes the text.
    await page.getByTestId("compare-input-Base").fill("release/1.2");
    await expect(page.getByTestId("compare-input-Base")).toHaveValue("release/1.2");
  });

  test("picks a revision with the keyboard alone", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });

    const input = page.getByTestId("compare-input-Base");
    await input.fill("");

    // An arrow opens the list and moves the highlight, so nothing needs the
    // pointer to reach a revision.
    await input.press("ArrowDown");
    await expect(page.locator(".compare__picker-options")).toBeVisible();
    await expect(
      page.getByTestId("compare-option-local-refs/heads/feature/tauri"),
    ).toHaveClass(/is-highlighted/);
    await input.press("ArrowDown");
    await expect(
      page.getByTestId("compare-option-remote-refs/remotes/origin/master"),
    ).toHaveClass(/is-highlighted/);
    await input.press("ArrowUp");
    await expect(
      page.getByTestId("compare-option-local-refs/heads/feature/tauri"),
    ).toHaveClass(/is-highlighted/);
    await input.press("ArrowUp");
    await expect(
      page.getByTestId("compare-option-local-refs/heads/master"),
    ).toHaveClass(/is-highlighted/);

    // Up from the first entry wraps to the last, and down from the last wraps
    // back, so there is no dead end in either direction.
    const options = page.locator(".compare__picker-option");
    const last = await options.count();
    await input.press("ArrowUp");
    await expect(options.nth(last - 1)).toHaveClass(/is-highlighted/);
    await input.press("ArrowDown");
    await expect(options.nth(0)).toHaveClass(/is-highlighted/);

    // Enter takes the highlighted entry, and the list closes behind it.
    await input.press("Enter");
    await expect(page.locator(".compare__picker-options")).toHaveCount(0);
    await expect(input).toHaveValue("master");
  });

  test("filters the offered revisions as the user types", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });

    await page.getByTestId("compare-input-Base").fill("v1.1");
    await page.getByTestId("compare-toggle-Base").click();

    const picker = page.locator(".compare__picker-options");
    await expect(picker).toContainText("v1.1.0");
    await expect(picker).not.toContainText("origin/master");
  });

  test("closes the settings surface on a click outside", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId("toolbar-settings").click();
    await expect(page.getByTestId("settings-overlay")).toBeVisible();
    // A click on the page behind it closes the surface: the settings are a
    // detour, not a place to dwell.
    await page.getByTestId("settings-overlay").click({ position: { x: 5, y: 5 } });
    await expect(page.getByTestId("settings-overlay")).toHaveCount(0);

    // A click inside it does not.
    await page.getByTestId("toolbar-settings").click();
    await expect(page.getByTestId("settings-overlay")).toBeVisible();
    await page.getByTestId("settings-close").click({ trial: true });
    await expect(page.getByTestId("settings-overlay")).toBeVisible();
  });

  test("accepts a typed object id", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });

    const input = page.getByTestId("compare-input-Base");
    // Too short to be an object id, so the picker says so rather than guessing.
    await input.fill("abc");
    await page.getByTestId("compare-toggle-Base").click();
    await expect(page.locator(".compare__picker-error")).toBeVisible();
    await page.keyboard.press("Escape");

    // A full-length hexadecimal id is offered as a commit.
    await input.fill("0123456789abcdef0123456789abcdef01234567");
    await page.getByTestId("compare-toggle-Base").click();
    await expect(page.getByTestId("compare-use-commit")).toBeVisible();
  });

  test("supersedes an in-flight comparison when a new pair is chosen", async ({
    page,
  }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });
    // The window runs one comparison as soon as it opens, so the count is taken
    // from there rather than from zero.
    const countComparisons = () =>
      page.evaluate(
        () =>
          (window as any).__STUB__.log.filter(
            (entry: any) => entry.cmd === "start_compare",
          ).length,
      );
    await expect(page.getByTestId("compare-file-src/lib.rs")).toBeVisible();
    const before = await countComparisons();

    await page.getByTestId("compare-toggle-Base").click();
    await page.getByTestId("compare-option-local-refs/heads/master").click();
    await page.getByTestId("compare-toggle-Target").click();
    await page.getByTestId("compare-option-local-refs/heads/feature/tauri").click();
    await expect(page.getByTestId("compare-file-src/lib.rs")).toBeVisible();

    // Choosing a different target starts a second comparison. The backend
    // treats the new request id as the current generation, so the first one's
    // remaining answers are dropped rather than appended. The picker filters by
    // what is already chosen, so it is cleared first.
    await page.getByTestId("compare-input-Target").fill("");
    await page.getByTestId("compare-toggle-Target").click();
    await page.getByTestId("compare-option-remote-refs/remotes/origin/master").click();

    // The file list is replaced, not appended to, and the three picks after the
    // automatic one each started exactly one comparison.
    await expect(page.getByTestId("compare-file-src/lib.rs")).toHaveCount(1);
    expect(await countComparisons()).toBe(before + 3);
  });

  test("reports a repository that is no longer open", async ({ page }) => {
    await boot(page, { window: "compare", repoId: 404 });
    await expect(page.getByTestId("compare-title")).toBeVisible();
    await expect(page.locator(".compare")).toContainText("This repository tab is no longer open.");
  });
});

test.describe("settings", () => {
  test("marks the current choice in a mode menu", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    // Commit or amend: opening the menu and reading it has to say which one the
    // button will act on.
    // The menu wraps its trigger, so the wrapper and the button share the id.
    const trigger = page.locator("button[data-testid='commit-mode-trigger']");
    await trigger.click();
    const mode = page.getByTestId("commit-mode");
    await expect(mode.getByTestId("commit-mode-commit").locator(".menu__check")).toBeVisible();
    await expect(
      mode.getByTestId("commit-mode-amend").locator(".menu__check"),
    ).toHaveCount(0);
    await page.getByTestId("commit-mode-amend").click();
    await trigger.click();
    await expect(mode.getByTestId("commit-mode-amend").locator(".menu__check")).toBeVisible();
    await expect(
      mode.getByTestId("commit-mode-commit").locator(".menu__check"),
    ).toHaveCount(0);
  });

  test("shows the shipped shortcut binding next to an override", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId("toolbar-settings").click();
    await page.getByTestId("settings-nav-shortcuts").click();

    // The default is shown so an override reads as a choice rather than a guess
    // at what it replaced.
    await expect(page.getByTestId("shortcut-default-app.quit")).toContainText(
      "Default: CmdOrCtrl+Q",
    );
  });

  test("changes the theme and the fonts", async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId("toolbar-settings").click();
    await expect(page.getByTestId("settings-overlay")).toBeVisible();
    await page.getByTestId("settings-nav-appearance").click();

    await page.getByTestId("settings-theme").click();
    await page.getByTestId("select-option-github-dark").click();

    // The preference is written straight through, so it survives a crash.
    const commands = await stub.commands();
    const setTheme = commands.filter((entry) => entry.cmd === "set_theme");
    expect(setTheme).toHaveLength(1);
    expect((setTheme[0]!.args as any).theme).toBe("github-dark");

    await page.getByTestId("settings-ui-font").click();
    await page.getByTestId("select-option-menlo").click();
    const setType = (await stub.commands()).filter((e) => e.cmd === "set_typography");
    expect(setType).toHaveLength(1);
    expect((setType[0]!.args as any).typography.ui_font_family).toBe("Menlo");
  });

  test("changes the diff layout and the history scope", async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId("toolbar-settings").click();
    await page.getByTestId("settings-nav-layout").click();

    await page.getByTestId("settings-diff-layout").click();
    await page.getByTestId("select-option-inline").click();
    await page.getByTestId("settings-graph-history").click();
    await page.getByTestId("select-option-current-branch-and-upstream").click();

    const commands = await stub.commands();
    expect(
      commands.some(
        (entry) =>
          entry.cmd === "set_diff_layout" &&
          (entry.args as any).layout === "inline",
      ),
    ).toBe(true);
    expect(
      commands.some(
        (entry) =>
          entry.cmd === "set_view" &&
          (entry.args as any).view.graph_history === "current-branch",
      ),
    ).toBe(true);
  });

  test("rejects an empty shortcut and accepts a real one", async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId("toolbar-settings").click();
    await page.getByTestId("settings-nav-shortcuts").click();

    await page.getByTestId("shortcut-app.quit").fill("");
    await page.getByTestId("shortcut-app.quit").press("Enter");
    await expect(page.getByTestId("shortcut-error")).toBeVisible();
    // Nothing was written, because the combination was refused.
    expect((await stub.commands()).filter((e) => e.cmd === "set_shortcut")).toHaveLength(0);

    await page.getByTestId("shortcut-app.quit").fill("CmdOrCtrl+Shift+Q");
    await page.getByTestId("shortcut-app.quit").press("Enter");
    const written = (await stub.commands()).filter((e) => e.cmd === "set_shortcut");
    expect(written).toHaveLength(1);
    expect((written[0]!.args as any).keys).toEqual([
      "CmdOrCtrl",
      "Shift",
      "Q",
    ]);
  });

  test("shows where the settings are stored", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });
    await page.getByTestId("toolbar-settings").click();
    await expect(page.getByTestId("settings-general")).toContainText(
      "com.augur.git.tauri",
    );
  });

  test("closes without saving anything else", async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await page.getByTestId("toolbar-settings").click();
    await page.getByTestId("settings-close").click();
    await expect(page.getByTestId("settings-overlay")).toHaveCount(0);
    // Opening and closing is not a change.
    expect((await stub.commands()).filter((e) => e.cmd === "set_theme")).toHaveLength(0);
  });
});

test.describe("the About window", () => {
  test("reports the identity of this application", async ({ page }) => {
    await boot(page, { window: "about" });

    await expect(page.getByTestId("about")).toBeVisible();
    await expect(page.getByTestId("about-version")).toHaveText("0.1.0");
    await expect(page.getByTestId("about-commit")).toHaveText("abc1234");
    // The identifier is what keeps the two products from sharing a data
    // directory, so it is on the page.
    await expect(page.getByTestId("about-identifier")).toHaveText(
      "com.augur.git.tauri",
    );
    await expect(page.getByTestId("about-cli")).toHaveText("augurgit-tauri");
    await expect(page.getByTestId("about")).toContainText(
      "com.augur.git.tauri/settings.json",
    );
  });
});

test.describe("the in-window menu", () => {
  test("opens a repository from the menu", async ({ page }) => {
    const stub = await boot(page, { windows: true });
    await page.getByTestId("menu-file-trigger").click();
    await page.getByTestId("menu-file-file").click();
    await page.getByTestId("menu-file-open-repository").click();
    await expect(page.getByTestId("repo-7")).toBeVisible();
    expect((await stub.commandNames()).filter((c) => c === "open_repository")).toHaveLength(1);
  });

  test("reports an install per file, with the reason for a failure", async ({
    page,
  }) => {
    await boot(page, { windows: true });
    await page.getByTestId("menu-file-trigger").click();
    await page.getByTestId("menu-file-file").click();
    await page.getByTestId("menu-file-install-cli").click();

    const report = page.getByTestId("cli-report-dialog");
    await expect(report).toBeVisible();
    await expect(report).toContainText("~/.zshrc");
    await expect(report).toContainText("Added the augurgit command to");
    await expect(report).toContainText("Already installed and up to date in");
    // A failure is only useful with its reason.
    await expect(report).toContainText("Permission denied");
    // The shell needs restarting before the change takes effect.
    await expect(report).toContainText("Open a new terminal (or reload your shell configuration) to use it.");
  });

  test("reports a removal as removed, not added", async ({ page }) => {
    await boot(page, { windows: true });
    await page.getByTestId("menu-file-trigger").click();
    await page.getByTestId("menu-file-file").click();
    await page.getByTestId("menu-file-remove-cli").click();

    // Reusing the install wording after a removal would tell the person the
    // command was added when the opposite happened.
    const report = page.getByTestId("cli-report-dialog");
    await expect(report).toContainText("Removed the augurgit command from");
    await expect(report).not.toContainText("Added the augurgit command to");
    await expect(report).toContainText("Not installed in");
  });

  test("lists the recent repositories", async ({ page }) => {
    await boot(page, { windows: true });
    await page.getByTestId("menu-file-trigger").click();
    await page.getByTestId("menu-file-file").click();
    await page.getByTestId("menu-file-recent-repositories").click();
    const recent = page.getByTestId("menu-file-recent-repositories-submenu");
    await expect(recent).toContainText("augur-git");
    await expect(recent).toContainText("other-app");
  });

  test("keeps the branch and settings actions beside the tabs", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], windows: true });
    await expect(page.getByTestId("tab-bar")).toBeVisible();
    await expect(page.getByTestId("title-branch")).toHaveText("master");
    await page.getByTestId("title-settings").click();
    await expect(page.getByTestId("settings-overlay")).toBeVisible();
    await expect(page.getByTestId("window-controls")).toBeVisible();
  });
});

test.describe("the native menu bridge", () => {
  test("routes a native menu activation to the same handler", async ({ page }) => {
    const stub = await boot(page);

    // The native menu dispatches a DOM event so both surfaces run one handler.
    await page.evaluate(() => {
      (window as any).__STUB__.emit("augur://menu", { id: "menu.open-repository" });
    });
    await expect(page.getByTestId("repo-7")).toBeVisible();

    await page.evaluate(() => {
      (window as any).__STUB__.emit("augur://menu", { id: "menu.about" });
    });
    expect((await stub.commandNames())).toContain("open_about_window");
  });

  test("opens paths handed over by a second launch", async ({ page }) => {
    await boot(page);

    await page.evaluate(() => {
      (window as any).__STUB__.emit("augur://open-paths", {
        paths: ["/Users/dev/projects/augur-git"],
      });
    });
    await expect(page.getByTestId("repo-7")).toBeVisible();
  });

  test("opens a dropped folder", async ({ page }) => {
    await boot(page);
    await page.evaluate(() => {
      (window as any).__STUB__.emit("augur://drop-paths", {
        paths: ["/Users/dev/projects/other-app"],
      });
    });
    await expect(page.getByTestId("repo-9")).toBeVisible();
  });

  test("shows a notice pushed by the backend", async ({ page }) => {
    await boot(page);
    await page.evaluate(() => {
      (window as any).__STUB__.emit("augur://app-event", {
        type: "notice",
        level: "warning",
        message: "The index needs rebuilding",
      });
    });
    await expect(page.getByTestId("notice")).toContainText(
      "The index needs rebuilding",
    );
    await page.getByTestId("notice").getByRole("button").click();
    await expect(page.getByTestId("notice")).toHaveCount(0);
  });
});
