import { expect, test } from "@playwright/test";

import { boot, fixtureRepo } from "./harness";

/**
 * The standalone comparison window and the settings surface.
 *
 * The comparison window is a separate document that talks to the same backend,
 * so it is booted with the same window role the backend puts on the URL.
 */

test.describe("comparison window", () => {
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
  });

  test("groups the offered revisions by kind", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });

    await page.getByTestId("compare-toggle-Base").click();
    const picker = page.locator(".compare__picker-options");
    await expect(picker).toContainText("Branches");
    await expect(picker).toContainText("remote");
    await expect(picker).toContainText("Tags");
  });

  test("filters the offered revisions as the user types", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: "compare", repoId: 7 });

    await page.getByTestId("compare-input-Base").fill("v1.1");
    await page.getByTestId("compare-toggle-Base").click();

    const picker = page.locator(".compare__picker-options");
    await expect(picker).toContainText("v1.1.0");
    await expect(picker).not.toContainText("origin/master");
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

    // The file list is replaced, not appended to.
    await expect(page.getByTestId("compare-file-src/lib.rs")).toHaveCount(1);
    const compares = (await page.evaluate(() =>
      (window as any).__STUB__.log.filter(
        (entry: any) => entry.cmd === "start_compare",
      ).length,
    )) as number;
    expect(compares).toBe(2);
  });

  test("reports a repository that is no longer open", async ({ page }) => {
    await boot(page, { window: "compare", repoId: 404 });
    await expect(page.getByTestId("compare-window")).toHaveCount(0);
  });
});

test.describe("settings", () => {
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
    const stub = await boot(page);
    await page.getByTestId("menu-file-trigger").click();
    await page.getByTestId("menu-file-open-repository").click();
    await expect(page.getByTestId("repo-7")).toBeVisible();
    expect((await stub.commandNames()).filter((c) => c === "open_repository")).toHaveLength(1);
  });

  test("reports an install per file, with the reason for a failure", async ({
    page,
  }) => {
    await boot(page);
    await page.getByTestId("menu-file-trigger").click();
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
    await boot(page);
    await page.getByTestId("menu-file-trigger").click();
    await page.getByTestId("menu-file-remove-cli").click();

    // Reusing the install wording after a removal would tell the person the
    // command was added when the opposite happened.
    const report = page.getByTestId("cli-report-dialog");
    await expect(report).toContainText("Removed the augurgit command from");
    await expect(report).not.toContainText("Added the augurgit command to");
    await expect(report).toContainText("Not installed in");
  });

  test("lists the recent repositories", async ({ page }) => {
    await boot(page);
    await page.getByTestId("menu-file-trigger").click();
    const menu = page.getByTestId("menu-file");
    await expect(menu).toContainText("augur-git");
    await expect(menu).toContainText("other-app");
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
