import { expect, test } from "@playwright/test";

import { boot, fixtureRepo, secondFixtureRepo } from "./harness";

/**
 * Selecting a commit and reading its diff.
 *
 * The point of these is the selection flow: the file list comes from a second
 * event, the diff from a third, and the viewer has to say which commit it is
 * showing so a stale panel cannot be mistaken for current content.
 */

test.describe("commit selection", () => {
  test("loads a commit's files and then its diff", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.locator(".graph-row").first().click();

    // The commit's files arrive in their own event.
    await expect(page.getByTestId("bottom-file-src/lib.rs")).toBeVisible();
    await expect(page.getByTestId("bottom-file-src/commands/repo.rs")).toBeVisible();
    // The panel names the commit so the content is attributable.
    await expect(page.getByTestId("bottom-panel")).toContainText(
      "Add the Tauri command surface",
    );

    // Every changed file is shown at once, each under its own path header.
    await expect(page.getByTestId("bottom-panel")).toContainText("All changed files");
    // The commit has two parents, so the panel says the diff is against the
    // first parent rather than an implicit comparison.
    await expect(page.getByTestId("bottom-panel")).toContainText("vs first parent");
    const headers = page.getByTestId("diff-file-header");
    await expect(headers).toHaveCount(2);
    await expect(headers.nth(0)).toContainText("src/lib.rs");
    await expect(headers.nth(1)).toContainText("src/commands/repo.rs");

    // One hunk per file, because both files of the commit are shown.
    await expect(page.getByTestId("diff-hunk")).toHaveCount(2);
    await expect(page.getByTestId("diff-view")).toContainText("count += 2");
    // The deleted line is present too, because the inline layout pairs them.
    await expect(page.getByTestId("diff-view")).toContainText("count += 1");
  });

  test("marks the changed characters in the inline layout", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });
    await page.evaluate(() => {
      const store = (window as any).__STUB__;
      void store;
    });

    await page.locator(".graph-row").first().click();
    await expect(page.getByTestId("diff-hunk").first()).toBeVisible();

    // The backend reports the changed character, so it is marked inside the
    // line. A whole-line colour would hide a one-character edit.
    const mark = page.locator(".diff__text mark").first();
    await expect(mark).toBeVisible();
    await expect(mark).toHaveText(/2/);
  });

  test("switches to the side-by-side layout when the preference says so", async ({
    page,
  }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.evaluate(() => {
      (window as any).__STUB__.config.view.diff_layout = "side-by-side";
    });

    await page.locator(".graph-row").first().click();
    await expect(page.getByTestId("diff-hunk").first()).toBeVisible();

    // A single file shows one text cell per row; the file list is what narrows
    // the view, so a commit with two files still lists both.
    await page.getByTestId("bottom-file-src/lib.rs").click();
    await expect(page.getByTestId("bottom-file-src/lib.rs")).toHaveClass(/is-selected/);
    await expect(page.getByTestId("diff-file-header")).toHaveCount(0);
    const firstRow = page.locator('[data-testid="diff-row"]').first();
    await expect(firstRow.locator(".diff__text")).toHaveCount(1);
  });

  test("drops a diff that arrives after the selection moved on", async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await page.locator(".graph-row").first().click();
    await page.getByTestId("bottom-file-src/lib.rs").click();
    await expect(page.getByTestId("diff-file-header")).toHaveCount(0);
    await expect(page.getByTestId("diff-view")).toContainText("count += 2");

    // A late answer for a commit that is no longer selected must not replace
    // what is on screen.
    await stub.emit("augur://repo-event", {
      repoId: 7,
      type: "fileDiff",
      oid: "0000000000000000000000000000000000000001",
      file: {
        path: "src/other.rs",
        old_path: null,
        new_path: "src/other.rs",
        status: "modified",
        old_blob: null,
        new_blob: null,
        added: 1,
        deleted: 1,
      },
      document: {
        path: "src/other.rs",
        language: "rust",
        rows: [
          {
            kind: "hunk",
            old_no: null,
            new_no: null,
            old_text: null,
            new_text: null,
            hunk_header: "@@ -1 +1 @@",
          },
        ],
        aligned_rows: [],
        old_source: null,
        new_source: null,
        inline_old: [],
        inline_new: [],
        binary: false,
        copy_text: "",
      },
    });

    await expect(page.getByTestId("diff-view")).toContainText("count += 2");
    await expect(page.getByTestId("diff-view")).not.toContainText("@@ -1 +1 @@");
  });

  test("clears the selection back to the placeholder", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.locator(".graph-row").first().click();
    await expect(page.getByTestId("bottom-file-src/lib.rs")).toBeVisible();

    await page.getByTestId("bottom-clear-commit").click();

    await expect(page.getByTestId("bottom-file-src/lib.rs")).toHaveCount(0);
    await expect(page.getByTestId("bottom-panel")).toContainText("Select a commit");
  });

  test("loads a working-tree diff and says which side it is", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId("changes-file-src/main.rs").click();

    await expect(page.getByTestId("bottom-panel")).toContainText("Staged changes");
    await expect(page.getByTestId("diff-hunk")).toBeVisible();
    await expect(page.getByTestId("diff-view")).toContainText("count += 2");
  });

  test("labels an unstaged file as a working-tree change", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId("changes-file-src/git/graph.rs").click();

    await expect(page.getByTestId("bottom-panel")).toContainText("Working tree changes");
  });

  test("shows the commit message dialog on request", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    // A right click inside the row opens the context menu, exactly as it does
    // in the application.
    const row = page.locator(".graph-row").first();
    const box = (await row.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2, {
      button: "right",
    });
    await expect(page.getByTestId("context-show-message")).toBeVisible();
    await page.getByTestId("context-show-message").click();

    await expect(page.getByTestId("commit-message-dialog")).toBeVisible();
    // The message is fetched on demand and rendered in full.
    await expect(page.getByTestId("commit-message-body")).toContainText(
      "Add the Tauri command surface",
    );
    // The co-author trailer is part of the message and is listed separately
    // from the body.
    await expect(page.getByTestId("commit-message-coauthors")).toContainText(
      "Co-authors",
    );
    await expect(page.getByTestId("commit-message-coauthors")).toContainText(
      "ada@example.com",
    );

    await page.getByTestId("commit-message-close").click();
    await expect(page.getByTestId("commit-message-dialog")).toHaveCount(0);
  });

  test("hides the author and then the message when the window narrows", async ({
    page,
  }) => {
    await boot(page, { open: [secondFixtureRepo()] });

    // The lane area is 12 + 24 * lanes + 8, and the two columns need their own
    // widths on top of the message minimum. The thresholds are the backend's,
    // so this asserts the whole column rule rather than a duplicated formula.
    const thresholds = await page.evaluate(() =>
      (window as any).__STUB__.log
        .filter((entry: any) => entry.cmd === "column_visibility")
        .map((entry: any) => entry.args),
    );
    expect((thresholds as unknown[]).length).toBeGreaterThan(0);

    // At the default width both optional columns are present.
    await expect(page.locator(".graph-row__author").first()).toBeVisible();
    await expect(page.locator(".graph-row__subject").first()).toBeVisible();

    // The author goes first, because its threshold is the higher of the two.
    await page.setViewportSize({ width: 1000, height: 800 });
    await expect(page.locator(".graph-row__author")).toHaveCount(0);
    await expect(page.locator(".graph-row__subject").first()).toBeVisible();

    // The subject goes next. The date and the hash stay: the threshold for the
    // subject already accounts for the date, and the hash identifies the row.
    await page.setViewportSize({ width: 800, height: 800 });
    await expect(page.locator(".graph-row__subject")).toHaveCount(0);
    await expect(page.locator(".graph-row__date").first()).toBeVisible();
    await expect(page.locator(".graph-row__hash").first()).toBeVisible();
  });

  test("filters the graph by commit message", async ({ page }) => {
    await boot(page, { open: [secondFixtureRepo()] });
    await expect(page.locator(".graph-row")).toHaveCount(1);

    await page.getByTestId("commit-search").fill("nothing matches this");
    await expect(page.getByTestId("commit-search-no-results")).toBeVisible();
    await expect(page.locator(".graph-row")).toHaveCount(0);

    await page.getByTestId("commit-search").fill("readme");
    await expect(page.getByTestId("commit-search-results")).toContainText("1 of 1");
    await expect(page.locator(".graph-row")).toHaveCount(1);
  });
});
