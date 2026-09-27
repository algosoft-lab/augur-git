import { boot, expect, fixtureRepo, rightClick, secondFixtureRepo, test } from "./harness";

/**
 * Opening and closing repositories.
 *
 * These cover the tab lifecycle end to end: the welcome page, the folder
 * picker, the tab that appears, the panes that fill in from backend events, and
 * closing a tab again.
 */

test.describe("repositories", () => {
  test("shows the welcome page when nothing is open", async ({ page }) => {
    await boot(page);
    await expect(page.getByTestId("welcome")).toBeVisible();
    await expect(page.getByTestId("welcome-open")).toBeVisible();
    await expect(page.getByTestId("status-bar")).toContainText("No repository selected");
    // The status bar reports the absence of a repository rather than leaving the
    // row empty.
    await expect(page.getByTestId("tab-new")).toBeVisible();
  });

  test("offers the recent repositories it was given", async ({ page }) => {
    await boot(page);
    const recent = page.locator(".welcome__recent-item");
    await expect(recent).toHaveCount(2);
    await expect(recent.first()).toContainText("augur-git");
  });

  test("opens a repository and fills every pane from events", async ({ page }) => {
    const stub = await boot(page);

    await page.getByTestId("welcome-open").click();

    // A tab appears and the welcome page gives way to the three-pane layout.
    await expect(page.getByTestId("repo-7")).toBeVisible();
    await expect(page.getByTestId("welcome")).toHaveCount(0);

    // Sidebar: the checked-out branch is marked and the ref sections are present.
    await expect(page.getByTestId("branch-master")).toBeVisible();
    await expect(page.getByTestId("branch-master")).toHaveClass(/is-head/);
    await expect(page.getByTestId("branch-feature/tauri")).toBeVisible();
    await expect(page.getByTestId("remote-branch-origin/master")).toBeVisible();
    await expect(page.getByTestId("tag-row-v1.0.0")).toBeVisible();
    await expect(page.getByTestId("stash-row-stash@{0}")).toBeVisible();

    // Toolbar: the branch and the ahead/behind counters.
    await expect(page.getByTestId("toolbar-branch")).toBeVisible();
    await expect(page.getByTestId("toolbar-fetch")).toBeEnabled();
    await expect(page.getByTestId("toolbar")).toContainText("2");
    await expect(page.getByTestId("toolbar")).toContainText("1");

    // Commit graph: rows, ref labels, and the hash column. The first row is the
    // HEAD commit and carries its ref labels.
    const firstRow = page.locator(".graph-row").first();
    await expect(firstRow).toBeVisible();
    await expect(firstRow).toHaveAttribute("data-testid", /^graph-row-[0-9a-f]{7}$/);
    await expect(page.locator(".graph-row")).toHaveCount(8);
    await expect(page.getByTestId("graph")).toContainText("HEAD");
    await expect(page.getByTestId("graph")).toContainText("feature/tauri");

    // Right panel: the commit editor and the grouped working-tree files.
    await expect(page.getByTestId("commit-message")).toBeVisible();
    await expect(page.getByTestId("changes-file-src/main.rs")).toBeVisible();
    await expect(page.getByTestId("changes-file-notes.md")).toBeVisible();
    // Only two groups, matching the reference: conflicts are reported inside
    // the changes group by their status character, not as a third group.
    await expect(page.getByTestId("changes-toggle-staged")).toBeVisible();
    await expect(page.getByTestId("changes-toggle-changes")).toBeVisible();
    await expect(page.getByTestId("changes-file-src/conflict.rs")).toBeVisible();
    await expect(page.getByTestId("changes-toggle-staged")).toContainText("Staged");
    await expect(page.getByTestId("changes-toggle-changes")).toContainText("Changes");
    // While conflicts exist, a group-wide restore would destroy the work the
    // merge is waiting on, so the row is offered no discard at all.
    await expect(page.getByTestId("changes-discard-all")).toBeDisabled();

    // Status bar: the path.
    await expect(page.getByTestId("status-bar")).toContainText(
      "/Users/dev/projects/augur-git",
    );

    // The interface asked the backend to open exactly the path it was given.
    const commands = await stub.commandNames();
    expect(commands).toContain("open_repository");
  });

  test("reports a repository that cannot be opened", async ({ page }) => {
    await boot(page, {
      openFailure: { key: "err-not-a-repo", detail: "/tmp/empty" },
    });

    await page.getByTestId("welcome-open").click();

    // The failure is a notice rather than a tab, because there is no repository
    // to put in one.
    const notice = page.getByTestId("notice");
    await expect(notice).toBeVisible();
    await expect(notice).toContainText("Not a Git repository");
    await expect(notice).toContainText("/tmp/empty");
    await expect(page.getByTestId("welcome")).toBeVisible();
  });

  test("keeps one repository's state per tab", async ({ page }) => {
    await boot(page);

    await page.getByTestId("welcome-open").click();
    await expect(page.getByTestId("repo-7")).toBeVisible();
    await expect(page.getByTestId("branch-master")).toBeVisible();

    // A second tab comes from the start page, which offers the next repository.
    await page.getByTestId("tab-new").click();
    await expect(page.getByTestId("start-page")).toBeVisible();
    await page.getByTestId("welcome-open").click();
    await expect(page.getByTestId("repo-9")).toBeVisible();
    await expect(page.getByTestId("branch-trunk")).toBeVisible();
    await expect(page.getByTestId("branch-master")).toHaveCount(0);

    // Switching back restores the first tab's repository unchanged.
    await page.locator(".tab").first().click();
    await expect(page.getByTestId("repo-7")).toBeVisible();
    await expect(page.getByTestId("branch-master")).toBeVisible();
  });

  test("closing the last tab returns to the welcome page", async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await expect(page.getByTestId("repo-7")).toBeVisible();
    await page.getByTestId("tab-close-/Users/dev/projects/augur-git").click();

    await expect(page.getByTestId("welcome")).toBeVisible();
    // The backend is told to release the repository worker, not just to hide
    // the tab.
    const commands = await stub.commands();
    expect(
      commands.some((entry) => entry.cmd === "close_repository" && entry.args.repoId === 7),
    ).toBe(true);
  });

  test("hints what the tab close button does", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    const close = page.getByTestId("tab-close-/Users/dev/projects/augur-git");
    await expect(close).toHaveAttribute("title", "Close this repository tab");
    await expect(close).toHaveAttribute("aria-label", "Close tab");
  });

  test("names the repository it is scanning while the first snapshot is in flight", async ({
    page,
  }) => {
    // Opening from the welcome page with a slow backend separates the command's
    // reply from the first snapshot, which is the window in which the interface
    // can only say it is scanning.
    await boot(page, {
      available: [secondFixtureRepo()],
      openDelay: 60,
    });

    await page.getByTestId("welcome-open").click();
    await expect(page.getByTestId("repo-9")).toBeVisible();
    await expect(page.getByTestId("status-bar")).toContainText(
      "Scanning @ other-app",
    );

    // The snapshot ends the scanning state.
    await expect(page.getByTestId("branch-trunk")).toBeVisible();
    await expect(page.getByTestId("status-bar")).not.toContainText("Scanning @");
  });

  test("restores the tabs the workspace was saved with", async ({ page }) => {
    const repo = fixtureRepo();
    await boot(page, { open: [repo] });

    // The window adopts the saved tab list rather than showing the welcome page.
    await expect(page.getByTestId("repo-7")).toBeVisible();
    await expect(page.getByTestId("tab-bar")).toContainText("augur-git");
  });

  test("keeps a conflicted file's actions, disabled and explained", async ({
    page,
  }) => {
    await boot(page, { open: [fixtureRepo()] });

    // The row is present but its actions are not: a conflict has to be resolved
    // before a file can be staged or discarded, and the row says so rather than
    // leaving the person to wonder why the buttons are gone.
    const conflicted = page.getByTestId("changes-file-src/conflict.rs");
    await expect(conflicted).toBeVisible();
    const toggle = page.getByTestId("changes-toggle-src/conflict.rs");
    await expect(toggle).toBeDisabled();
    await expect(toggle).toHaveAttribute(
      "title",
      "Unavailable for conflicted files",
    );

    // The same in the context menu, which keeps the entries and disables them.
    await rightClick(page, '[data-testid="changes-row-src/conflict.rs"]');
    await expect(page.getByTestId("context-toggle-stage")).toBeDisabled();
    await expect(page.getByTestId("context-discard")).toBeDisabled();
  });

  test("lists a partially staged file in both groups", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    // A file with a staged change and an unstaged one has two different diffs.
    // Showing it only under Staged would make the unstaged half unreachable.
    const staged = page.locator('[data-testid="changes-toggle-staged"]');
    const changes = page.locator('[data-testid="changes-toggle-changes"]');
    await expect(staged).toBeVisible();
    await expect(changes).toBeVisible();

    // The row keys include the group, so the two entries are distinct and both
    // are addressable.
    await expect(
      page.getByTestId("changes-file-src/partial.rs"),
    ).toHaveCount(2);

    // Selecting the unstaged half asks the backend for the working tree, and
    // selecting the staged half asks for the index.
    await page.getByTestId("changes-file-src/partial.rs").nth(1).click();
    await expect(page.getByTestId("bottom-panel")).toContainText("Changes");
    await page.getByTestId("changes-file-src/partial.rs").nth(0).click();
    await expect(page.getByTestId("bottom-panel")).toContainText("Staged");
  });

  test("opens a new tab as a start page that a repository replaces", async ({
    page,
  }) => {
    await boot(page);

    // The start page is a tab, not a folder dialog.
    await page.getByTestId("tab-new").click();
    await expect(page.locator(".tab")).toHaveCount(1);
    await expect(page.locator(".tab__label")).toHaveText(["New Tab"]);
    await expect(page.getByTestId("start-page")).toBeVisible();

    // Opening a repository into it fills the slot rather than pushing a second
    // tab, so the tab count does not grow.
    await page.getByTestId("welcome-open").click();
    await expect(page.getByTestId("repo-7")).toBeVisible();
    await expect(page.locator(".tab")).toHaveCount(1);
    await expect(page.locator(".tab__label")).toHaveText(["augur-git"]);

    // A start page is not written to the saved workspace, so the reload that
    // follows does not bring it back.
    const commands = await page.evaluate(() =>
      (window as any).__STUB__.log.filter(
        (entry: any) => entry.cmd === "set_workspace_tabs",
      ),
    );
    const last = (commands as { args: { tabs: { path: string }[] }[] }[]).at(-1);
    expect(last?.args.tabs.map((tab) => tab.path)).not.toContain("");
  });

  test("shows the active branch in the title bar and reveals it on click", async ({
    page,
  }) => {
    await boot(page, { open: [fixtureRepo()] });

    const badge = page.getByTestId("title-branch");
    await expect(badge).toBeVisible();
    await expect(badge).toHaveText("master");

    // With the section collapsed, the click still has to land somewhere
    // visible, so it expands and highlights the branch list.
    await page.getByTestId("sidebar-toggle-branches").click();
    await expect(page.getByTestId("branch-master")).toHaveCount(0);
    await badge.click();
    await expect(page.getByTestId("branch-master")).toBeVisible();
    await expect(page.getByTestId("sidebar-branches")).toHaveClass(/is-flashing/);

    // A second repository switches which branch is shown.
    await page.getByTestId("tab-new").click();
    await page.getByTestId("welcome-open").click();
    await expect(page.getByTestId("repo-9")).toBeVisible();
    await expect(badge).toHaveText("trunk");
  });

  test("marks a tab that failed to open", async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await expect(page.getByTestId("repo-7")).toBeVisible();

    // A status error after the fact is reported in the graph area and the
    // status bar, and the tab's indicator turns to the error colour.
    await stub.emit("augur://repo-event", {
      repoId: 7,
      type: "statusError",
      error: { key: "err-git-status", detail: "index.lock exists" },
    });
    await expect(page.getByTestId("graph-error")).toContainText("index.lock exists");
    await expect(page.getByTestId("status-bar")).toContainText("index.lock exists");
    await expect(page.locator(".tab__dot--error")).toHaveCount(1);
  });

  test("collects a path handed over before the window was listening", async ({
    page,
  }) => {
    // Launching the application with a path argument is the case the backend has
    // to hold: the window does not exist when the path arrives, so there is
    // nothing to emit it to. Before this the bootstrap always said no and the
    // queue was never drained, so the argument did nothing at all.
    await boot(page, { pendingPaths: ["/Users/dev/projects/from-the-command-line"] });

    // Exactly one tab: the collection is drained, so the strict-mode second
    // initialisation finds nothing rather than opening a second repository.
    await expect(page.locator(".tab")).toHaveCount(1);
    await expect(page.locator(".tab__label")).toHaveText(["from-the-command-line"]);
    await expect(page.getByTestId("repo-7")).toBeVisible();

    // And it was collected rather than delivered, which is the only route that
    // survives a window that did not exist yet.
    expect(
      await page.evaluate(() =>
        (window as any).__STUB__.log.filter(
          (entry: any) => entry.cmd === "take_pending_paths",
        ).length,
      ),
    ).toBeGreaterThan(0);
  });

  test("completes a restored tab that has no repository behind it yet", async ({
    page,
  }) => {
    // The saved tab list is adopted before the window knows which repositories
    // the backend has open, so every restored tab starts as a claim. A claim
    // that is short-circuited instead of completed leaves the tab in its
    // loading state forever, with the welcome page showing behind it.
    await boot(page, { savedTabs: ["/Users/dev/projects/augur-git"] });

    await expect(page.getByTestId("repo-7")).toBeVisible();
    await expect(page.getByTestId("branch-master")).toBeVisible();
    await expect(page.locator(".tab")).toHaveCount(1);
    await expect(page.getByTestId("welcome")).toHaveCount(0);
  });

  test("opens one tab when the same folder arrives twice", async ({ page }) => {
    // Two drops in quick succession, or a drop racing a menu item, both reach
    // the tab list before either has finished opening. The claim is taken
    // before the request, so the second one finds the tab already there.
    await boot(page);
    await page.evaluate(() => {
      const stub = (window as any).__STUB__;
      stub.emit("augur://open-paths", { paths: ["/Users/dev/projects/other-app"] });
      stub.emit("augur://drop-paths", { paths: ["/Users/dev/projects/other-app"] });
    });
    await expect(page.getByTestId("repo-9")).toBeVisible();
    await expect(page.locator(".tab")).toHaveCount(1);
    const opens = await page.evaluate(
      () =>
        (window as any).__STUB__.log.filter(
          (entry: any) => entry.cmd === "open_repository",
        ).length,
    );
    expect(opens).toBe(1);
  });

  test("keeps a tab opened alongside the saved tab list", async ({ page }) => {
    // The command line hands its paths to the window that owns the tab list, so
    // one can arrive before the bootstrap response. Adopting the saved list
    // must not discard it, and the two repositories stay separate tabs.
    await boot(page, { open: [fixtureRepo()] });
    await page.evaluate(() => {
      (window as any).__STUB__.emit("augur://open-paths", {
        paths: ["/Users/dev/projects/other-app"],
      });
    });
    await expect(page.getByTestId("repo-9")).toBeVisible();
    await expect(page.locator(".tab")).toHaveCount(2);
    await expect(page.locator(".tab__label")).toHaveText(["augur-git", "other-app"]);
  });

  test("keeps the first snapshot that arrives before the command reply", async ({
    page,
  }) => {
    // The backend starts a worker thread inside `open_repository`, so its first
    // status can reach the webview before the command's own reply. Those events
    // are buffered and folded in, so the repository is never left blank.
    await boot(page);

    await page.getByTestId("welcome-open").click();

    // If the buffer were missing, every one of these would be empty.
    await expect(page.getByTestId("branch-master")).toBeVisible();
    await expect(page.locator(".graph-row")).toHaveCount(8);
    await expect(page.getByTestId("changes-file-src/main.rs")).toBeVisible();
  });
});
