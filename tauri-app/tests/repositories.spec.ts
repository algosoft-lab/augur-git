import { boot, expect, fixtureRepo, rightClick, secondFixtureRepo, test } from './harness';

/**
 * Opening and closing repositories.
 *
 * These cover the tab lifecycle end to end: the welcome page, the folder
 * picker, the tab that appears, the panes that fill in from backend events, and
 * closing a tab again.
 */

test.describe('repositories', () => {
  test('scrolls a long working-tree list while keeping its header fixed', async ({ page }) => {
    const repo = fixtureRepo();
    repo.status.files = Array.from({ length: 80 }, (_, index) => ({
      index: ' ',
      worktree: 'M',
      path: `src/generated-${index}.ts`,
      old_path: null
    }));
    await boot(page, { open: [repo] });

    const list = page.getByTestId('changes-scroll');
    const header = page.getByTestId('changes-header');
    const headerYBefore = (await header.boundingBox())!.y;
    const lastFile = page.getByTestId('changes-file-src/generated-79.ts');

    const scrollMetrics = await list.evaluate((element) => {
      const scrollable = element as HTMLElement;
      return {
        clientHeight: scrollable.clientHeight,
        scrollHeight: scrollable.scrollHeight
      };
    });
    expect(scrollMetrics.scrollHeight).toBeGreaterThan(scrollMetrics.clientHeight);

    await list.evaluate((element) => {
      const scrollable = element as HTMLElement;
      scrollable.scrollTop = scrollable.scrollHeight;
    });
    await expect(lastFile).toBeInViewport();
    await expect.poll(() => list.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
    expect((await header.boundingBox())!.y).toBe(headerYBefore);
  });

  test('shows the welcome page when nothing is open', async ({ page }) => {
    await boot(page);
    await expect(page.getByTestId('welcome')).toBeVisible();
    await expect(page.getByTestId('welcome-open')).toBeVisible();
    await expect(page.getByTestId('status-bar')).toContainText('No repository selected');
    // The status bar reports the absence of a repository rather than leaving the
    // row empty.
    await expect(page.getByTestId('tab-new')).toBeVisible();
  });

  test('offers the recent repositories it was given', async ({ page }) => {
    await boot(page);
    const recent = page.locator('.welcome__recent-item');
    await expect(recent).toHaveCount(2);
    await expect(recent.first()).toContainText('augur-git');
  });

  test('opens a repository and fills every pane from events', async ({ page }) => {
    const stub = await boot(page);

    await page.getByTestId('welcome-open').click();

    // A tab appears and the welcome page gives way to the three-pane layout.
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await expect(page.getByTestId('welcome')).toHaveCount(0);

    // Sidebar: the checked-out branch is marked and the ref sections are present.
    await expect(page.getByTestId('branch-master')).toBeVisible();
    await expect(page.getByTestId('branch-master')).toHaveClass(/is-head/);
    await expect(page.getByTestId('branch-feature/tauri')).toBeVisible();
    await expect(page.getByTestId('remote-branch-origin/master')).toBeVisible();
    await expect(page.getByTestId('tag-row-v1.0.0')).toBeVisible();
    await expect(page.getByTestId('stash-row-stash@{0}')).toBeVisible();

    // Toolbar: the branch and the ahead/behind counters.
    await expect(page.getByTestId('toolbar-more')).toBeVisible();
    await expect(page.getByTestId('toolbar-fetch')).toBeEnabled();
    await expect(page.getByTestId('toolbar')).toContainText('2');
    await expect(page.getByTestId('toolbar')).toContainText('1');

    // Commit graph: rows, ref labels, and the hash column. The first row is the
    // HEAD commit and carries its ref labels.
    const firstRow = page.locator('.graph-row').first();
    await expect(firstRow).toBeVisible();
    await expect(firstRow).toHaveAttribute('data-testid', /^graph-row-[0-9a-f]{7}$/);
    await expect(page.locator('.graph-row')).toHaveCount(8);
    await expect(page.getByTestId('graph')).toContainText('HEAD');
    await expect(page.getByTestId('graph')).toContainText('feature/tauri');

    // Right panel: the commit editor and the grouped working-tree files.
    await expect(page.getByTestId('commit-message')).toBeVisible();
    await expect(page.getByTestId('changes-file-src/main.rs')).toBeVisible();
    await expect(page.getByTestId('changes-file-notes.md')).toBeVisible();
    // Only two groups, matching the reference: conflicts are reported inside
    // the changes group by their status character, not as a third group.
    await expect(page.getByTestId('changes-toggle-staged')).toBeVisible();
    await expect(page.getByTestId('changes-toggle-changes')).toBeVisible();
    await expect(page.getByTestId('changes-file-src/conflict.rs')).toBeVisible();
    await expect(page.getByTestId('changes-toggle-staged')).toContainText('Staged');
    await expect(page.getByTestId('changes-toggle-changes')).toContainText('Changes');
    // While conflicts exist, a group-wide restore would destroy the work the
    // merge is waiting on, so the row is offered no discard at all.
    await expect(page.getByTestId('changes-discard-all')).toBeDisabled();

    // Status bar: the path.
    await expect(page.getByTestId('status-bar')).toContainText('/Users/dev/projects/augur-git');

    // The interface asked the backend to open exactly the path it was given.
    const commands = await stub.commandNames();
    expect(commands).toContain('open_repository');
  });

  test('shows group totals and refreshes them with the working-tree snapshot', async ({ page }) => {
    const repo = fixtureRepo();
    const stub = await boot(page, { open: [repo] });

    await expect(page.getByTestId('changes-stats-files-staged')).toHaveText('4');
    await expect(page.getByTestId('changes-stats-added-staged')).toHaveText('+12');
    await expect(page.getByTestId('changes-stats-deleted-staged')).toHaveText('−3');
    await expect(page.getByTestId('changes-stats-files-changes')).toHaveText('5');
    await expect(page.getByTestId('changes-stats-added-changes')).toHaveText('+13');
    await expect(page.getByTestId('changes-stats-deleted-changes')).toHaveText('−4');

    await page.getByTestId('changes-toggle-changes').click();
    await expect(page.getByTestId('changes-stats-changes')).toBeVisible();

    repo.status.files = repo.status.files.map((file) =>
      file.path === 'src/git/graph.rs' ? { ...file, index: 'M', worktree: ' ' } : file
    );
    repo.status.diff_stats = {
      staged: { added: 20, deleted: 5 },
      unstaged: { added: 1, deleted: 2 },
      untracked: { added: 5, deleted: 0 }
    };
    await stub.emit('augur://repo-event', { repoId: repo.id, type: 'status', ...repo.status });

    await expect(page.getByTestId('changes-stats-files-staged')).toHaveText('5');
    await expect(page.getByTestId('changes-stats-added-staged')).toHaveText('+20');
    await expect(page.getByTestId('changes-stats-files-changes')).toHaveText('4');
    await expect(page.getByTestId('changes-stats-added-changes')).toHaveText('+6');
    await expect(page.getByTestId('changes-stats-deleted-changes')).toHaveText('−2');

    repo.status.diff_stats = {
      staged: null,
      unstaged: null,
      untracked: { added: 0, deleted: 0 }
    };
    await stub.emit('augur://repo-event', { repoId: repo.id, type: 'status', ...repo.status });
    await expect(page.getByTestId('changes-stats-staged')).toContainText('—');
    await expect(page.getByTestId('changes-stats-changes')).toContainText('—');

    repo.status.diff_stats = {
      staged: { added: 0, deleted: 0 },
      unstaged: { added: 0, deleted: 0 },
      untracked: { added: 0, deleted: 0 }
    };
    await stub.emit('augur://repo-event', { repoId: repo.id, type: 'status', ...repo.status });
    await expect(page.getByTestId('changes-stats-added-staged')).toHaveText('+0');
    await expect(page.getByTestId('changes-stats-deleted-changes')).toHaveText('−0');
  });

  test('keeps Changes totals in sync when untracked files are hidden', async ({ page }) => {
    const repo = fixtureRepo();
    await boot(page, { open: [repo], showUntracked: false });

    await expect(page.getByTestId('changes-file-notes.md')).toHaveCount(0);
    await expect(page.getByTestId('changes-stats-files-changes')).toHaveText('4');
    await expect(page.getByTestId('changes-stats-added-changes')).toHaveText('+8');
    await expect(page.getByTestId('changes-stats-deleted-changes')).toHaveText('−4');
  });

  test('reports a repository that cannot be opened', async ({ page }) => {
    await boot(page, {
      openFailure: { key: 'err-not-a-repo', detail: '/tmp/empty' }
    });

    await page.getByTestId('welcome-open').click();

    // The failure is a notice rather than a tab, because there is no repository
    // to put in one.
    const notice = page.getByTestId('notice');
    await expect(notice).toBeVisible();
    await expect(notice).toContainText('Not a Git repository');
    await expect(notice).toContainText('/tmp/empty');
    await expect(page.getByTestId('welcome')).toBeVisible();
  });

  test("keeps one repository's state per tab", async ({ page }) => {
    await boot(page);

    await page.getByTestId('welcome-open').click();
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await expect(page.getByTestId('branch-master')).toBeVisible();

    // A second tab comes from the start page, which offers the next repository.
    await page.getByTestId('tab-new').click();
    await expect(page.getByTestId('start-page')).toBeVisible();
    await page.getByTestId('welcome-open').click();
    await expect(page.getByTestId('repo-9')).toBeVisible();
    await expect(page.getByTestId('branch-trunk')).toBeVisible();
    await expect(page.getByTestId('branch-master')).toHaveCount(0);

    // Switching back restores the first tab's repository unchanged.
    await page.locator('.tab').first().click();
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await expect(page.getByTestId('branch-master')).toBeVisible();
  });

  test('sends only the selected repository to the automatic refresh monitor', async ({ page }) => {
    const first = fixtureRepo();
    const second = secondFixtureRepo();
    await boot(page, {
      open: [first, second],
      savedTabs: [first.path, second.path],
      savedActiveTab: first.path
    });

    await expect(page.locator('.tab')).toHaveCount(2);

    await expect
      .poll(async () =>
        page.evaluate(() => {
          const entries = (window as any).__STUB__.log.filter(
            (entry: any) => entry.cmd === 'set_auto_refresh_target'
          );
          return entries.at(-1)?.args.repoId;
        })
      )
      .toBe(first.id);

    await page.locator('.tab').nth(1).click();
    await expect
      .poll(async () =>
        page.evaluate(() => {
          const entries = (window as any).__STUB__.log.filter(
            (entry: any) => entry.cmd === 'set_auto_refresh_target'
          );
          return entries.at(-1)?.args.repoId;
        })
      )
      .toBe(second.id);
  });

  test('closing the last tab returns to the welcome page', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await expect(page.getByTestId('repo-7')).toBeVisible();
    await page.getByTestId('tab-close-/Users/dev/projects/augur-git').click();

    await expect(page.getByTestId('welcome')).toBeVisible();
    // The backend is told to release the repository worker, not just to hide
    // the tab.
    const commands = await stub.commands();
    expect(
      commands.some((entry) => entry.cmd === 'close_repository' && entry.args.repoId === 7)
    ).toBe(true);
  });

  test('hints what the tab close button does', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    const close = page.getByTestId('tab-close-/Users/dev/projects/augur-git');
    await expect(close).toHaveAttribute('title', 'Close this repository tab');
    await expect(close).toHaveAttribute('aria-label', 'Close tab');
  });

  test('names the repository it is scanning while the first snapshot is in flight', async ({
    page
  }) => {
    // Opening from the welcome page with a slow backend separates the command's
    // reply from the first snapshot, which is the window in which the interface
    // can only say it is scanning.
    await boot(page, {
      available: [secondFixtureRepo()],
      openDelay: 60
    });

    await page.getByTestId('welcome-open').click();
    await expect(page.getByTestId('repo-9')).toBeVisible();
    await expect(page.getByTestId('status-bar')).toContainText('Scanning @ other-app');

    // The snapshot ends the scanning state.
    await expect(page.getByTestId('branch-trunk')).toBeVisible();
    await expect(page.getByTestId('status-bar')).not.toContainText('Scanning @');
  });

  test('restores the tabs the workspace was saved with', async ({ page }) => {
    const repo = fixtureRepo();
    await boot(page, { open: [repo] });

    // The window adopts the saved tab list rather than showing the welcome page.
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await expect(page.getByTestId('tab-bar')).toContainText('augur-git');
  });

  test('reorders tabs on drop, saves their order, and keeps the selected tab active', async ({
    page
  }) => {
    const first = fixtureRepo();
    const second = secondFixtureRepo();
    const stub = await boot(page, { open: [first, second] });
    const firstTab = page.getByTestId(`tab-${first.path}`);
    const secondTab = page.getByTestId(`tab-${second.path}`);

    const firstBounds = await firstTab.boundingBox();
    const secondBounds = await secondTab.boundingBox();
    expect(firstBounds).not.toBeNull();
    expect(secondBounds).not.toBeNull();
    const secondStart = {
      x: secondBounds!.x + secondBounds!.width / 2,
      y: secondBounds!.y + secondBounds!.height / 2
    };
    await page.mouse.move(secondStart.x, secondStart.y);
    await page.mouse.down();
    await page.mouse.move(secondStart.x - 12, secondStart.y, { steps: 2 });
    await page.mouse.move(firstBounds!.x + 8, secondStart.y);
    await expect(firstTab).toHaveClass(/is-drop-before/);
    await page.mouse.up();

    await expect(page.locator('.tab__label')).toHaveText(['other-app', 'augur-git']);
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await expect
      .poll(async () => {
        const saves = (await stub.commands()).filter((entry) => entry.cmd === 'set_workspace_tabs');
        return saves.at(-1)?.args;
      })
      .toMatchObject({
        tabs: [{ path: second.path }, { path: first.path }],
        active: first.path
      });

    const reorderedFirst = await secondTab.boundingBox();
    const reorderedSecond = await firstTab.boundingBox();
    expect(reorderedFirst).not.toBeNull();
    expect(reorderedSecond).not.toBeNull();
    const returnStart = {
      x: reorderedFirst!.x + reorderedFirst!.width / 2,
      y: reorderedFirst!.y + reorderedFirst!.height / 2
    };
    await page.mouse.move(returnStart.x, returnStart.y);
    await page.mouse.down();
    await page.mouse.move(returnStart.x + 12, returnStart.y, { steps: 2 });
    await page.mouse.move(reorderedSecond!.x + reorderedSecond!.width * 0.7, returnStart.y);
    await expect(firstTab).toHaveClass(/is-drop-after/);
    await page.mouse.up();

    await expect(page.locator('.tab__label')).toHaveText(['augur-git', 'other-app']);
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await expect
      .poll(async () => {
        const saves = (await stub.commands()).filter((entry) => entry.cmd === 'set_workspace_tabs');
        return saves.at(-1)?.args;
      })
      .toMatchObject({
        tabs: [{ path: first.path }, { path: second.path }],
        active: first.path
      });

    const saves = (await stub.commands()).filter((entry) => entry.cmd === 'set_workspace_tabs');
    const saved = saves.at(-1)?.args as { tabs: { path: string }[] };
    const restoredPage = await page.context().newPage();
    await boot(restoredPage, {
      savedTabs: saved.tabs.map((tab) => tab.path),
      savedActiveTab: first.path,
      available: [first, second]
    });
    await expect(restoredPage.locator('.tab__label')).toHaveText(['augur-git', 'other-app']);
  });

  test('cancels a tab drag outside the bar or with Escape', async ({ page }) => {
    const first = fixtureRepo();
    const second = secondFixtureRepo();
    const stub = await boot(page, { open: [first, second] });
    const firstTab = page.getByTestId(`tab-${first.path}`);
    const secondTab = page.getByTestId(`tab-${second.path}`);
    const saveCount = async () =>
      (await stub.commands()).filter((entry) => entry.cmd === 'set_workspace_tabs').length;
    const initialSaveCount = await saveCount();

    const secondBounds = await secondTab.boundingBox();
    const barBounds = await page.getByTestId('tab-bar').boundingBox();
    expect(secondBounds).not.toBeNull();
    expect(barBounds).not.toBeNull();
    const start = {
      x: secondBounds!.x + secondBounds!.width / 2,
      y: secondBounds!.y + secondBounds!.height / 2
    };
    await page.mouse.move(start.x, start.y);
    await page.mouse.down();
    await page.mouse.move(start.x - 12, start.y, { steps: 2 });
    await page.mouse.move(barBounds!.x + 8, barBounds!.y + barBounds!.height + 80);
    await page.mouse.up();
    await expect(page.locator('.tab__label')).toHaveText(['augur-git', 'other-app']);
    await expect(page.getByTestId('repo-7')).toBeVisible();
    expect(await saveCount()).toBe(initialSaveCount);

    await page.mouse.move(start.x, start.y);
    await page.mouse.down();
    await page.mouse.move(start.x - 12, start.y, { steps: 2 });
    await page.mouse.move((await firstTab.boundingBox())!.x + 8, start.y);
    await expect(firstTab).toHaveClass(/is-drop-before/);
    await page.keyboard.press('Escape');
    await page.mouse.up();
    await expect(page.locator('.tab__label')).toHaveText(['augur-git', 'other-app']);
    await expect(page.getByTestId('repo-7')).toBeVisible();
    expect(await saveCount()).toBe(initialSaveCount);
  });

  test('allows reordering a start page without saving it as a repository tab', async ({ page }) => {
    const first = fixtureRepo();
    const second = secondFixtureRepo();
    const stub = await boot(page, { open: [first, second] });
    await page.getByTestId('tab-new').click();
    const startTab = page.locator('.tab').last();
    const firstTab = page.getByTestId(`tab-${first.path}`);
    const startBounds = await startTab.boundingBox();
    const firstBounds = await firstTab.boundingBox();
    expect(startBounds).not.toBeNull();
    expect(firstBounds).not.toBeNull();

    const startX = startBounds!.x + startBounds!.width / 2;
    const startY = startBounds!.y + startBounds!.height / 2;
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX - 12, startY, { steps: 2 });
    await page.mouse.move(firstBounds!.x + 8, startY);
    await expect(firstTab).toHaveClass(/is-drop-before/);
    await page.mouse.up();

    await expect(page.locator('.tab__label')).toHaveText(['New Tab', 'augur-git', 'other-app']);
    await expect(page.getByTestId('start-page')).toBeVisible();
    await expect
      .poll(async () => {
        const saves = (await stub.commands()).filter((entry) => entry.cmd === 'set_workspace_tabs');
        const args = saves.at(-1)?.args as { tabs: { path: string }[] } | undefined;
        return args?.tabs.map((tab) => tab.path);
      })
      .toEqual([first.path, second.path]);
  });

  test('auto-scrolls an overflowing tab bar while dragging', async ({ page }) => {
    const repositories = Array.from({ length: 14 }, (_, index) => ({
      ...fixtureRepo(),
      id: 20 + index,
      path: `/Users/dev/projects/repo-${index}`
    }));
    await boot(page, { open: repositories });
    const firstTab = page.getByTestId(`tab-${repositories[0]!.path}`);
    const lastTab = page.getByTestId(`tab-${repositories.at(-1)!.path}`);
    const bar = page.getByTestId('tab-bar');
    const firstBounds = await firstTab.boundingBox();
    const barBounds = await bar.boundingBox();
    expect(firstBounds).not.toBeNull();
    expect(barBounds).not.toBeNull();
    expect(await bar.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true);

    const start = {
      x: firstBounds!.x + firstBounds!.width / 2,
      y: firstBounds!.y + firstBounds!.height / 2
    };
    await page.mouse.move(start.x, start.y);
    await page.mouse.down();
    await page.mouse.move(start.x + 12, start.y, { steps: 2 });
    await page.mouse.move(barBounds!.x + barBounds!.width - 8, start.y);
    await expect(lastTab).toHaveClass(/is-drop-after/, { timeout: 5000 });
    await page.mouse.up();

    await expect(page.locator('.tab__label').last()).toHaveText('repo-0');
  });

  test("keeps a conflicted file's actions, disabled and explained", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    // The row is present but its actions are not: a conflict has to be resolved
    // before a file can be staged or discarded, and the row says so rather than
    // leaving the person to wonder why the buttons are gone.
    const conflicted = page.getByTestId('changes-file-src/conflict.rs');
    await expect(conflicted).toBeVisible();
    const toggle = page.getByTestId('changes-toggle-src/conflict.rs');
    await expect(toggle).toBeDisabled();
    await expect(toggle).toHaveAttribute('title', 'Unavailable for conflicted files');

    // The same in the context menu, which keeps the entries and disables them.
    await rightClick(page, '[data-testid="changes-row-src/conflict.rs"]');
    await expect(page.getByTestId('context-toggle-stage')).toBeDisabled();
    await expect(page.getByTestId('context-discard')).toBeDisabled();
  });

  test('lists a partially staged file in both groups', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    // A file with a staged change and an unstaged one has two different diffs.
    // Showing it only under Staged would make the unstaged half unreachable.
    const staged = page.locator('[data-testid="changes-toggle-staged"]');
    const changes = page.locator('[data-testid="changes-toggle-changes"]');
    await expect(staged).toBeVisible();
    await expect(changes).toBeVisible();

    // The row keys include the group, so the two entries are distinct and both
    // are addressable.
    await expect(page.getByTestId('changes-file-src/partial.rs')).toHaveCount(2);

    // Selecting the unstaged half asks the backend for the working tree, and
    // selecting the staged half asks for the index.
    await page.getByTestId('changes-file-src/partial.rs').nth(1).click();
    await expect(page.getByTestId('bottom-panel')).toContainText('Changes');
    await page.getByTestId('changes-file-src/partial.rs').nth(0).click();
    await expect(page.getByTestId('bottom-panel')).toContainText('Staged');
  });

  test('refreshes an open working diff only while its repository tab is selected', async ({
    page
  }) => {
    const first = fixtureRepo();
    const second = secondFixtureRepo();
    const stub = await boot(page, {
      open: [first, second],
      savedTabs: [first.path, second.path],
      savedActiveTab: first.path
    });

    await page.getByTestId('changes-file-src/main.rs').click();
    await expect(page.getByTestId('bottom-panel')).toBeVisible();
    const workingDiffCalls = async () =>
      (await stub.commands()).filter((entry) => entry.cmd === 'load_working_tree_diff').length;
    await expect.poll(workingDiffCalls).toBe(1);

    await stub.emit('augur://repo-event', { repoId: first.id, type: 'status', ...first.status });
    await expect.poll(workingDiffCalls).toBe(2);

    await page.locator('.tab').nth(1).click();
    await expect(page.getByTestId('repo-9')).toBeVisible();
    await stub.emit('augur://repo-event', { repoId: first.id, type: 'status', ...first.status });
    await page.waitForTimeout(100);
    expect(await workingDiffCalls()).toBe(2);
  });

  test('opens a new tab as a start page that a repository replaces', async ({ page }) => {
    await boot(page);

    // The start page is a tab, not a folder dialog.
    await page.getByTestId('tab-new').click();
    await expect(page.locator('.tab')).toHaveCount(1);
    await expect(page.locator('.tab__label')).toHaveText(['New Tab']);
    await expect(page.getByTestId('start-page')).toBeVisible();

    // Opening a repository into it fills the slot rather than pushing a second
    // tab, so the tab count does not grow.
    await page.getByTestId('welcome-open').click();
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await expect(page.locator('.tab')).toHaveCount(1);
    await expect(page.locator('.tab__label')).toHaveText(['augur-git']);

    // A start page is not written to the saved workspace, so the reload that
    // follows does not bring it back.
    const commands = await page.evaluate(() =>
      (window as any).__STUB__.log.filter((entry: any) => entry.cmd === 'set_workspace_tabs')
    );
    const last = (commands as { args: { tabs: { path: string }[] }[] }[]).at(-1);
    expect(last?.args.tabs.map((tab) => tab.path)).not.toContain('');
  });

  test('keeps the active branch in the sidebar without a title-bar badge', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await expect(page.getByTestId('title-branch')).toHaveCount(0);
    await expect(page.getByTestId('branch-master')).toBeVisible();

    await page.getByTestId('sidebar-toggle-branches').click();
    await expect(page.getByTestId('branch-master')).toHaveCount(0);
    await page.getByTestId('sidebar-toggle-branches').click();
    await expect(page.getByTestId('branch-master')).toBeVisible();

    await page.getByTestId('tab-new').click();
    await page.getByTestId('welcome-open').click();
    await expect(page.getByTestId('repo-9')).toBeVisible();
    await expect(page.getByTestId('branch-trunk')).toBeVisible();
  });

  test('marks a tab that failed to open', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    // A status error after the fact is reported in the graph area and the
    // status bar, and the tab's indicator turns to the error colour.
    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'statusError',
      error: { key: 'err-git-status', detail: 'index.lock exists' }
    });
    await expect(page.getByTestId('graph-error')).toContainText('index.lock exists');
    await expect(page.getByTestId('status-bar')).toContainText('index.lock exists');
    await expect(page.locator('.tab__dot--error')).toHaveCount(1);
  });

  test('collects a path handed over before the window was listening', async ({ page }) => {
    // Launching the application with a path argument is the case the backend has
    // to hold: the window does not exist when the path arrives, so there is
    // nothing to emit it to. Before this the bootstrap always said no and the
    // queue was never drained, so the argument did nothing at all.
    await boot(page, { pendingPaths: ['/Users/dev/projects/from-the-command-line'] });

    // Exactly one tab: the collection is drained, so the strict-mode second
    // initialisation finds nothing rather than opening a second repository.
    await expect(page.locator('.tab')).toHaveCount(1);
    await expect(page.locator('.tab__label')).toHaveText(['from-the-command-line']);
    await expect(page.getByTestId('repo-7')).toBeVisible();

    // And it was collected rather than delivered, which is the only route that
    // survives a window that did not exist yet.
    expect(
      await page.evaluate(
        () =>
          (window as any).__STUB__.log.filter((entry: any) => entry.cmd === 'take_pending_paths')
            .length
      )
    ).toBeGreaterThan(0);
  });

  test('completes a restored tab that has no repository behind it yet', async ({ page }) => {
    // The saved tab list is adopted before the window knows which repositories
    // the backend has open, so every restored tab starts as a claim. A claim
    // that is short-circuited instead of completed leaves the tab in its
    // loading state forever, with the welcome page showing behind it.
    await boot(page, { savedTabs: ['/Users/dev/projects/augur-git'] });

    await expect(page.getByTestId('repo-7')).toBeVisible();
    await expect(page.getByTestId('branch-master')).toBeVisible();
    await expect(page.locator('.tab')).toHaveCount(1);
    await expect(page.getByTestId('welcome')).toHaveCount(0);
  });

  test('comes back on the tab the workspace was saved with', async ({ page }) => {
    // Three tabs are saved and the active one is not the first of them. The
    // window has to come back on the tab the person left, and it has to fill
    // that one in before the tabs behind it, because everything visible while
    // it waits belongs to the tab that is on screen.
    await boot(page, {
      savedTabs: [
        '/Users/dev/projects/augur-git',
        '/Users/dev/projects/other-app',
        '/Users/dev/projects/third-app'
      ],
      savedActiveTab: '/Users/dev/projects/other-app'
    });

    // No tab is clicked, and the selected tab is already filled in.
    await expect(page.getByTestId('repo-9')).toBeVisible();
    await expect(page.getByTestId('branch-trunk')).toBeVisible();
    await expect(page.getByTestId('welcome')).toHaveCount(0);
    await expect(page.locator('.tab')).toHaveCount(3);
    await expect(page.locator('.tab__label')).toHaveText(['augur-git', 'other-app', 'third-app']);
    await expect(page.locator('.tab.is-active .tab__label')).toHaveText('other-app');

    // The tab on screen is opened first, rather than last: restoring the rest of
    // the list behind it is what used to leave the window showing an empty tab.
    expect(
      await page.evaluate(() =>
        (window as any).__STUB__.log
          .filter((entry: any) => entry.cmd === 'open_repository')
          .map((entry: any) => entry.args.path)
      )
    ).toEqual([
      '/Users/dev/projects/other-app',
      '/Users/dev/projects/augur-git',
      '/Users/dev/projects/third-app'
    ]);

    // And it is asked for a snapshot once the list is in place, so a first
    // snapshot lost to the subscription arriving late is recovered rather than
    // waiting for a click.
    expect(
      await page.evaluate(() =>
        (window as any).__STUB__.log
          .filter((entry: any) => entry.cmd === 'refresh_repository')
          .map((entry: any) => entry.args.repoId)
      )
    ).toContain(9);
  });

  test('opens one tab when the same folder arrives twice', async ({ page }) => {
    // Two drops in quick succession, or a drop racing a menu item, both reach
    // the tab list before either has finished opening. The claim is taken
    // before the request, so the second one finds the tab already there.
    await boot(page);
    await page.evaluate(() => {
      const stub = (window as any).__STUB__;
      stub.emit('augur://open-paths', { paths: ['/Users/dev/projects/other-app'] });
      stub.emit('augur://drop-paths', { paths: ['/Users/dev/projects/other-app'] });
    });
    await expect(page.getByTestId('repo-9')).toBeVisible();
    await expect(page.locator('.tab')).toHaveCount(1);
    const opens = await page.evaluate(
      () =>
        (window as any).__STUB__.log.filter((entry: any) => entry.cmd === 'open_repository').length
    );
    expect(opens).toBe(1);
  });

  test('keeps a tab opened alongside the saved tab list', async ({ page }) => {
    // The command line hands its paths to the window that owns the tab list, so
    // one can arrive before the bootstrap response. Adopting the saved list
    // must not discard it, and the two repositories stay separate tabs.
    await boot(page, { open: [fixtureRepo()] });
    await page.evaluate(() => {
      (window as any).__STUB__.emit('augur://open-paths', {
        paths: ['/Users/dev/projects/other-app']
      });
    });
    await expect(page.getByTestId('repo-9')).toBeVisible();
    await expect(page.locator('.tab')).toHaveCount(2);
    await expect(page.locator('.tab__label')).toHaveText(['augur-git', 'other-app']);
  });

  test('keeps the first snapshot that arrives before the command reply', async ({ page }) => {
    // The backend starts a worker thread inside `open_repository`, so its first
    // status can reach the webview before the command's own reply. Those events
    // are buffered and folded in, so the repository is never left blank.
    await boot(page);

    await page.getByTestId('welcome-open').click();

    // If the buffer were missing, every one of these would be empty.
    await expect(page.getByTestId('branch-master')).toBeVisible();
    await expect(page.locator('.graph-row')).toHaveCount(8);
    await expect(page.getByTestId('changes-file-src/main.rs')).toBeVisible();
  });
});
