import { boot, expect, fixtureRepo, test } from "./harness";

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
    await expect(page.getByTestId("status-bar")).toContainText("No repository open");
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

    // A second tab must not reuse the first one's status.
    await page.getByTestId("tab-new").click();
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

  test("restores the tabs the workspace was saved with", async ({ page }) => {
    const repo = fixtureRepo();
    await boot(page, { open: [repo] });

    // The window adopts the saved tab list rather than showing the welcome page.
    await expect(page.getByTestId("repo-7")).toBeVisible();
    await expect(page.getByTestId("tab-bar")).toContainText("augur-git");
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
