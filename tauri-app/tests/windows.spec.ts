import { expect, test } from '@playwright/test';

import { boot, fixtureRepo, secondFixtureRepo } from './harness';

/**
 * The standalone comparison and settings windows.
 *
 * Each is a separate document that talks to the same backend, so each is booted
 * with the same window role the backend puts on the URL.
 */

test.describe('comparison window', () => {
  test('previews image changes in revision comparison', async ({ page }) => {
    const stub = await boot(page, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7,
      includeImageFixtures: true
    });
    await page.getByTestId('compare-file-assets/changed.png').waitFor();
    await page.getByTestId('compare-file-assets/changed.png').click();
    await expect(page.getByTestId('diff-image-preview')).toBeVisible();
    await expect(page.getByTestId('diff-image-old')).toHaveAttribute(
      'src',
      /^data:image\/png;base64,/
    );
    expect((await stub.commands()).some((entry) => entry.cmd === 'load_image_preview')).toBe(true);
  });

  test('reloads a pending image preview after comparison endpoints change', async ({ page }) => {
    const stub = await boot(page, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7,
      includeImageFixtures: true,
      imagePreviewDelay: 350
    });
    const countChangedImageCalls = async () =>
      (await stub.commands()).filter(
        (entry) =>
          entry.cmd === 'load_image_preview' &&
          (entry.args as any).target?.file?.new_path === 'assets/changed.png'
      ).length;

    await expect.poll(countChangedImageCalls).toBeGreaterThan(0);
    await expect(page.getByTestId('diff-image-new-loading').first()).toBeVisible();

    await page.getByTestId('compare-toggle-Base').click();
    await page.getByTestId('compare-option-remote-refs/remotes/origin/master').click();
    await expect(page.getByTestId('compare-file-assets/changed.png')).toBeVisible();
    await page.getByTestId('compare-file-assets/changed.png').click();
    await expect(page.getByTestId('diff-image-new-loading')).toBeVisible();
    await expect.poll(countChangedImageCalls).toBeGreaterThan(1);
    await expect(page.getByTestId('diff-image-new')).toHaveAttribute(
      'src',
      /^data:image\/png;base64,/
    );
  });

  test('syncs the soft-wrap toggle between the repository and Compare windows', async ({
    page
  }) => {
    await boot(page, { open: [fixtureRepo()] });
    await page.locator('.graph-row').first().click();
    await page.getByTestId('bottom-file-src/lib.rs').waitFor();

    const compare = await page.context().newPage();
    await boot(compare, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7
    });
    await expect(compare.getByTestId('compare-soft-wrap')).toHaveAttribute('aria-pressed', 'false');

    await page.getByTestId('bottom-soft-wrap').click();
    await expect(compare.getByTestId('compare-soft-wrap')).toHaveAttribute('aria-pressed', 'true');

    await compare.getByTestId('compare-soft-wrap').click();
    await expect(page.getByTestId('bottom-soft-wrap')).toHaveAttribute('aria-pressed', 'false');
  });

  test('syncs diff typography with the main window and restores it after reload', async ({
    page
  }) => {
    await boot(page, { open: [fixtureRepo()], macos: true });
    const compare = await page.context().newPage();
    const compareStub = await boot(compare, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7,
      macos: true
    });

    const prevented = await compare.evaluate(() =>
      document.body.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: '+',
          code: 'Equal',
          metaKey: true,
          shiftKey: true,
          bubbles: true,
          cancelable: true
        })
      )
    );
    expect(prevented).toBe(false);
    await expect
      .poll(() =>
        page.evaluate(() => document.documentElement.style.getPropertyValue('--diff-font-size'))
      )
      .toBe('17px');
    await expect
      .poll(() =>
        compare.evaluate(() => document.documentElement.style.getPropertyValue('--diff-font-size'))
      )
      .toBe('17px');
    expect(
      (await compareStub.commands()).filter((entry) => entry.cmd === 'set_typography')
    ).toHaveLength(1);

    await compare.getByTestId('compare-toggle-Base').click();
    await compare.getByTestId('compare-option-local-refs/heads/master').focus();
    const popupKeyWasAllowed = await compare.evaluate(() =>
      document.activeElement!.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: '+',
          code: 'Equal',
          metaKey: true,
          shiftKey: true,
          bubbles: true,
          cancelable: true
        })
      )
    );
    expect(popupKeyWasAllowed).toBe(true);
    await expect
      .poll(() =>
        compare.evaluate(() => document.documentElement.style.getPropertyValue('--diff-font-size'))
      )
      .toBe('17px');

    await compare.reload();
    await compare.waitForSelector('[data-testid="compare-window"]');
    await expect
      .poll(() =>
        compare.evaluate(() => document.documentElement.style.getPropertyValue('--diff-font-size'))
      )
      .toBe('17px');
  });

  test('names the panel in its own header', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });
    await expect(page.getByTestId('compare-window')).toBeVisible();
    await expect(page.getByTestId('compare-title')).toHaveText('Revision comparison');
  });

  test('asks the backend for a standalone window from the toolbar', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('toolbar-compare').click();

    // The comparison lives in its own native window, so the main window only
    // asks the backend to open it and never mounts a surface of its own.
    await expect(page.getByTestId('compare-window')).toHaveCount(0);
    const calls = (await stub.commands()).filter((entry) => entry.cmd === 'open_compare_window');
    expect(calls).toHaveLength(1);
    expect((calls[0]!.args as any).repoId).toBe(7);
  });

  test('keeps custom window controls above the compare inputs on Windows', async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7,
      windows: true
    });
    const titlebar = await page.locator('.window-titlebar').boundingBox();
    const controls = await page.getByTestId('window-controls').boundingBox();
    const header = await page.locator('.compare__header').boundingBox();
    const base = await page.getByTestId('compare-input-Base').boundingBox();
    expect(titlebar).not.toBeNull();
    expect(controls).not.toBeNull();
    expect(header).not.toBeNull();
    expect(base).not.toBeNull();
    expect(controls!.y + controls!.height).toBeLessThanOrEqual(header!.y);
    expect(base!.y).toBeGreaterThanOrEqual(header!.y);
    await expect(page.getByTestId('window-close')).toBeVisible();
  });

  test('drags the compare title text and maximizes on a double click', async ({ page }) => {
    // Dragging is a JS handler on Windows and Linux, and a native drag region
    // on macOS that the browser stub cannot simulate, so pin the platform.
    const stub = await boot(page, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7,
      windows: true
    });
    const title = page.getByTestId('compare-title');

    await title.click();
    await expect
      .poll(async () =>
        (await stub.commands()).some((entry) => entry.cmd === 'plugin:window|start_dragging')
      )
      .toBe(true);
    await title.dblclick();
    await expect
      .poll(async () =>
        (await stub.commands()).some((entry) => entry.cmd === 'plugin:window|toggle_maximize')
      )
      .toBe(true);
  });

  test('reserves title-bar space for native controls on macOS', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], macos: true });
    await expect(page.getByTestId('title-bar')).toHaveClass(/title-bar--macos/);
    await expect(page.getByTestId('tab-bar')).toBeVisible();
    await expect(page.getByTestId('menu-file-trigger')).toHaveCount(0);
    await expect(page.getByTestId('window-controls')).toHaveCount(0);
    const titlebar = await page.getByTestId('title-bar').boundingBox();
    const tabs = await page.getByTestId('tab-bar').boundingBox();
    const brand = await page.locator('.title-bar__brand').boundingBox();
    await expect(page.getByTestId('title-bar')).toHaveCSS('padding-left', '78px');
    expect(brand!.x).toBeGreaterThanOrEqual(titlebar!.x + 78);
    expect(tabs!.y).toBe(titlebar!.y);
    expect(tabs!.height).toBe(titlebar!.height - 1);
  });

  test('reserves title-bar space for native controls on the welcome page', async ({ page }) => {
    await boot(page, { macos: true });
    await expect(page.getByTestId('welcome')).toBeVisible();
    await expect(page.getByTestId('title-bar')).toHaveCSS('padding-left', '78px');
    await expect(page.getByTestId('window-controls')).toHaveCount(0);
  });

  test('compares two revisions and lists the files', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });
    await expect(page.getByTestId('compare-window')).toBeVisible();

    // The endpoints start empty and the revisions the backend offered are
    // reachable from the picker.
    await page.getByTestId('compare-toggle-Base').click();
    await expect(page.getByTestId('compare-option-local-refs/heads/master')).toBeVisible();
    await page.getByTestId('compare-option-local-refs/heads/master').click();

    await page.getByTestId('compare-toggle-Target').click();
    await expect(page.getByTestId('compare-option-local-refs/heads/feature/tauri')).toBeVisible();
    await page.getByTestId('compare-option-local-refs/heads/feature/tauri').click();

    // The comparison arrives as its own event, and the window reports the
    // files it found.
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
    await expect(page.getByTestId('compare-file-src/new.rs')).toBeVisible();
  });

  test('replays comparison events emitted before the command reply', async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7,
      compareReplyDelay: 90
    });

    // The fixture emits its file list, per-file diffs, and finished event while
    // start_compare is still waiting to return the request id.
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
    await expect(page.getByTestId('diff-file-header')).toHaveCount(2);
    await expect(page.getByTestId('compare-progress')).toHaveCount(0);
    await expect(page.getByTestId('compare-diff')).toContainText('count += 2');
  });

  test('reports a comparison that failed as a whole, not as no changes', async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7,
      failCompare: "fatal: bad revision 'refs/heads/gone'"
    });

    // The sentence names the failure wherever the window would otherwise have
    // claimed success or invited another choice, and Git's words are not lost.
    await expect(page.getByTestId('compare-request-error')).toHaveText(
      'Unable to load revision comparison'
    );
    await expect(page.getByTestId('compare-request-error')).toHaveAttribute(
      'title',
      "fatal: bad revision 'refs/heads/gone'"
    );
    await expect(page.getByTestId('compare-files-empty')).toContainText(
      'Unable to load revision comparison'
    );
    await expect(page.getByText('The selected revisions have no file changes')).toHaveCount(0);
  });

  test('shows every changed file at once, and one file on request', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });

    await page.getByTestId('compare-toggle-Base').click();
    await page.getByTestId('compare-option-local-refs/heads/master').click();
    await page.getByTestId('compare-toggle-Target').click();
    await page.getByTestId('compare-option-local-refs/heads/feature/tauri').click();
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();

    // The aggregate row is chosen by default and both files appear as their own
    // documents, each under its own path header.
    await expect(page.getByTestId('compare-all-files')).toHaveClass(/is-selected/);
    const headers = page.getByTestId('diff-file-header');
    await expect(headers).toHaveCount(2);
    await expect(headers.nth(0)).toContainText('src/lib.rs');
    await expect(headers.nth(1)).toContainText('src/new.rs');
    await page.getByTestId('compare-soft-wrap').click();
    await expect(page.getByTestId('compare-soft-wrap')).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('.diff--soft-wrap')).toBeVisible();

    // Choosing a file narrows the same list to that one.
    await page.getByTestId('compare-file-src/new.rs').click();
    await expect(page.getByTestId('compare-all-files')).not.toHaveClass(/is-selected/);
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
    await expect(page.getByTestId('diff-file-header')).toHaveCount(1);
    await expect(page.getByTestId('diff-file-header')).toContainText('src/new.rs');
    const starts = await page.evaluate(
      () =>
        (window as any).__STUB__.log.filter((entry: any) => entry.cmd === 'start_compare').length
    );
    // Selecting a file only narrows the current result; it must not restart Git.
    await page.getByTestId('compare-file-src/lib.rs').click();
    await expect(page.getByTestId('diff-file-header')).toContainText('src/lib.rs');
    expect(
      await page.evaluate(
        () =>
          (window as any).__STUB__.log.filter((entry: any) => entry.cmd === 'start_compare').length
      )
    ).toBe(starts);
  });

  test('groups the offered revisions by kind, each named by its kind', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });

    await page.getByTestId('compare-input-Base').fill('');
    const picker = page.locator('.compare__picker-options');
    await expect(picker).toContainText('Branches');
    await expect(picker).toContainText('remote');
    await expect(picker).toContainText('Tags');

    // The list contains named refs only; commit IDs are entered manually.
    const options = page.locator('.compare__picker-option');
    await expect(options).toHaveCount(4);
    await expect(options.nth(0)).toHaveText('local · master');
    await expect(options.nth(2)).toHaveText('remote · origin/master');
    await expect(options.nth(3)).toHaveText('tag · v1.1.0');
    await expect(page.getByTestId('compare-use-commit')).toHaveCount(0);
  });

  test('keeps the dropdown fully visible in narrow and regular windows', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });

    for (const viewport of [
      { width: 900, height: 560 },
      { width: 1280, height: 800 }
    ]) {
      await page.setViewportSize(viewport);
      await page.getByTestId('compare-toggle-Base').click();
      const menu = page.getByTestId('compare-options-Base');
      await expect(menu).toBeVisible();
      const box = await menu.boundingBox();
      expect(box).not.toBeNull();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.y).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width);
      expect(box!.y + box!.height).toBeLessThanOrEqual(viewport.height);
      await page.getByTestId('compare-option-local-refs/heads/feature/tauri').click();
    }
  });

  test('counts the documents as a comparison streams in', async ({ page }) => {
    // Slow the diffs down so the in-flight state is observable at all: the real
    // worker streams them one file at a time.
    await boot(page, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7,
      compareDelay: 400
    });

    // Progress, because a window that looks finished and is not is worse than
    // one that admits it is working. The shape is asserted rather than the exact
    // intermediate value, because how fast a file arrives is the worker's
    // business and not the interface's.
    const progress = page.getByTestId('compare-progress');
    await expect(progress).toBeVisible();
    await expect(progress).toHaveText(/^\d+ \/ 2$/);
    // It goes away once everything has arrived.
    await expect(progress).toHaveCount(0);
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
  });

  test('exports a comparison between a ref and a typed object id', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });

    await page.getByTestId('compare-manual-Target').click();
    // A valid manual SHA starts a comparison and makes the ref-to-commit patch
    // exportable once its streamed result is complete.
    await page.getByTestId('compare-input-Target').fill('0123456789abcdef0123456789abcdef01234567');
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
    await expect(page.getByTestId('compare-export')).toBeEnabled();
    await page.getByTestId('compare-input-Target').fill('not-a-sha');
    await expect(page.getByTestId('compare-export')).toBeDisabled();
  });

  test('manual mode accepts only a commit SHA', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });

    const countComparisons = () =>
      page.evaluate(
        () =>
          (window as any).__STUB__.log.filter((entry: any) => entry.cmd === 'start_compare').length
      );
    const before = await countComparisons();
    await page.getByTestId('compare-toggle-Base').click();
    await expect(page.locator('.compare__picker-options')).toBeVisible();
    await page.getByTestId('compare-manual-Base').click();
    await expect(page.locator('.compare__picker-options')).toHaveCount(0);
    await page.getByTestId('compare-input-Base').fill('release/1.2');
    await expect(page.getByTestId('compare-input-Base')).toHaveValue('release/1.2');
    await expect(page.getByTestId('compare-manual-error-Base')).toContainText('commit SHA');
    expect(await countComparisons()).toBe(before);

    await page.getByTestId('compare-input-Base').fill('0123456789abcdef0123456789abcdef01234567');
    await expect.poll(countComparisons).toBe(before + 1);
    await expect(page.getByTestId('compare-progress')).toHaveCount(0);
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
  });

  test('picks a revision with the keyboard alone', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });

    const input = page.getByTestId('compare-input-Base');
    await input.fill('');

    // An arrow opens the list and moves the highlight, so nothing needs the
    // pointer to reach a revision.
    await input.press('ArrowDown');
    await expect(page.locator('.compare__picker-options')).toBeVisible();
    await expect(page.getByTestId('compare-option-local-refs/heads/feature/tauri')).toHaveClass(
      /is-highlighted/
    );
    await input.press('ArrowDown');
    await expect(page.getByTestId('compare-option-remote-refs/remotes/origin/master')).toHaveClass(
      /is-highlighted/
    );
    await input.press('ArrowUp');
    await expect(page.getByTestId('compare-option-local-refs/heads/feature/tauri')).toHaveClass(
      /is-highlighted/
    );
    await input.press('ArrowUp');
    await expect(page.getByTestId('compare-option-local-refs/heads/master')).toHaveClass(
      /is-highlighted/
    );

    // Up from the first entry wraps to the last, and down from the last wraps
    // back, so there is no dead end in either direction.
    const options = page.locator('.compare__picker-option');
    const last = await options.count();
    await input.press('ArrowUp');
    await expect(options.nth(last - 1)).toHaveClass(/is-highlighted/);
    await input.press('ArrowDown');
    await expect(options.nth(0)).toHaveClass(/is-highlighted/);

    // Enter takes the highlighted entry, and the list closes behind it.
    await input.press('Enter');
    await expect(page.locator('.compare__picker-options')).toHaveCount(0);
    await expect(input).toHaveValue('master');
  });

  test('fuzzy search preserves the current comparison until a ref is selected', async ({
    page
  }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });

    const countComparisons = () =>
      page.evaluate(
        () =>
          (window as any).__STUB__.log.filter((entry: any) => entry.cmd === 'start_compare').length
      );
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
    const before = await countComparisons();

    await page.getByTestId('compare-input-Base').fill('FTau');

    const picker = page.locator('.compare__picker-options');
    await expect(page.getByTestId('compare-option-local-refs/heads/feature/tauri')).toBeVisible();
    await expect(picker).not.toContainText('origin/master');
    expect(await countComparisons()).toBe(before);

    await page.getByTestId('compare-option-local-refs/heads/feature/tauri').click();
    await expect.poll(countComparisons).toBe(before + 1);
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
  });

  test('opens the standalone settings window from the title-bar gear', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId('title-settings').click();
    // The settings live in their own window, so the main window only asks the
    // backend to open it and never mounts a surface of its own.
    await expect(page.getByTestId('settings-window')).toHaveCount(0);
    expect(await stub.commandNames()).toContain('open_settings_window');
  });

  test('manual SHA input rejects short values and accepts a full object ID', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });

    await page.getByTestId('compare-manual-Base').click();
    const input = page.getByTestId('compare-input-Base');
    await input.fill('abc');
    await expect(page.getByTestId('compare-manual-error-Base')).toBeVisible();
    await expect(page.getByTestId('compare-run')).toBeDisabled();

    await input.fill('0123456789abcdef0123456789abcdef01234567');
    await expect(page.getByTestId('compare-manual-error-Base')).toHaveCount(0);
    await expect(page.getByTestId('compare-run')).toBeEnabled();
  });

  test('supersedes an in-flight comparison when a new pair is chosen', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'compare', repoId: 7 });
    // The window runs one comparison as soon as it opens, so the count is taken
    // from there rather than from zero.
    const countComparisons = () =>
      page.evaluate(
        () =>
          (window as any).__STUB__.log.filter((entry: any) => entry.cmd === 'start_compare').length
      );
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
    const before = await countComparisons();

    await page.getByTestId('compare-toggle-Base').click();
    await page.getByTestId('compare-option-local-refs/heads/master').click();
    await page.getByTestId('compare-toggle-Target').click();
    await page.getByTestId('compare-option-local-refs/heads/feature/tauri').click();
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();

    // Choosing a different target starts a second comparison. The backend
    // treats the new request id as the current generation, so the first one's
    // remaining answers are dropped rather than appended. Clearing the closed
    // field opens the full list for this selection.
    await page.getByTestId('compare-input-Target').fill('');
    await page.getByTestId('compare-option-remote-refs/remotes/origin/master').click();

    // The file list is replaced, not appended to, and the three picks after the
    // automatic one each started exactly one comparison.
    await expect(page.getByTestId('compare-file-src/lib.rs')).toHaveCount(1);
    expect(await countComparisons()).toBe(before + 3);
  });

  test('a newer manual SHA supersedes older in-flight SHA comparisons', async ({ page }) => {
    const stub = await boot(page, {
      open: [fixtureRepo()],
      window: 'compare',
      repoId: 7,
      compareDelay: 120
    });
    const countComparisons = () =>
      page.evaluate(
        () =>
          (window as any).__STUB__.log.filter((entry: any) => entry.cmd === 'start_compare').length
      );
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
    await expect(page.getByTestId('compare-progress')).toHaveCount(0);
    const before = await countComparisons();

    await page.getByTestId('compare-manual-Target').click();
    const input = page.getByTestId('compare-input-Target');
    await input.fill('1111111111111111111111111111111111111111');
    await input.fill('2222222222222222222222222222222222222222');
    await expect.poll(countComparisons).toBe(before + 2);
    await expect(page.getByTestId('compare-progress')).toHaveCount(0);
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();

    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'branchCompareError',
      requestId: before + 1,
      detail: 'stale comparison error'
    });
    await expect(page.getByTestId('compare-request-error')).toHaveCount(0);
    await expect(page.getByTestId('compare-file-src/lib.rs')).toBeVisible();
  });

  test('reports a repository that is no longer open', async ({ page }) => {
    await boot(page, { window: 'compare', repoId: 404 });
    await expect(page.getByTestId('compare-title')).toBeVisible();
    await expect(page.locator('.compare')).toContainText('This repository tab is no longer open.');
  });
});

test.describe('nightly updates', () => {
  test('About exposes automatic checks and manual checks without downloading', async ({ page }) => {
    const stub = await boot(page, { window: 'about', windows: true });

    await expect(page.getByTestId('about')).toBeVisible();
    await expect(page.getByTestId('auto-check-updates')).toBeChecked();
    await page.getByTestId('check-for-updates').click();
    await expect(page.getByTestId('update-status')).toHaveText('You are up to date.');

    await page.getByTestId('auto-check-updates').uncheck();
    await expect
      .poll(async () =>
        (await stub.commands()).filter((entry) => entry.cmd === 'set_auto_check_updates')
      )
      .toHaveLength(1);
    const commands = await stub.commandNames();
    expect(commands).toContain('get_update_snapshot');
    expect(commands).toContain('check_for_updates');
    expect(commands).not.toContain('download_update');
  });

  test('main-window update notice opens About and persists dismissal', async ({ page }) => {
    const stub = await boot(page, { windows: true });
    const commitSha = 'a'.repeat(40);
    await stub.emit('augur://update-event', {
      type: 'status',
      status: {
        state: 'available',
        currentVersion: '0.1.0',
        latestVersion: '0.1.1-nightly.24',
        latestCommitSha: commitSha,
        canInstall: true,
        progress: null,
        error: null,
        installChannel: 'windows-installer'
      }
    });
    await stub.emit('augur://update-event', {
      type: 'notice',
      notice: { commitSha, version: '0.1.1-nightly.24' }
    });

    const notice = page.getByTestId('update-notice');
    await expect(notice).toBeVisible();
    await notice.getByRole('button', { name: 'Review update' }).click();
    await expect
      .poll(async () => (await stub.commands()).some((entry) => entry.cmd === 'open_about_window'))
      .toBe(true);

    await notice.locator('.update-notice__close').click();
    await expect(notice).toHaveCount(0);
    const dismiss = (await stub.commands()).find((entry) => entry.cmd === 'dismiss_update_notice');
    expect(dismiss?.args).toEqual({ commitSha });
  });

  test('About copies the Homebrew upgrade command for a cask installation', async ({ page }) => {
    const stub = await boot(page, { window: 'about', macos: true });
    await stub.emit('augur://update-event', {
      type: 'status',
      status: {
        state: 'available',
        currentVersion: '0.1.0',
        latestVersion: '0.1.1-nightly.24',
        latestCommitSha: 'b'.repeat(40),
        canInstall: false,
        progress: null,
        error: null,
        installChannel: 'homebrew-cask'
      }
    });

    const copy = page.getByTestId('homebrew-copy-upgrade');
    await expect(copy).toBeVisible();
    await copy.click();
    expect(await stub.clipboard()).toBe('brew upgrade --cask augur-git');
    await expect(copy).toHaveAttribute('aria-label', 'Upgrade command copied');
  });

  test('the main-window notice copies the Homebrew upgrade command', async ({ page }) => {
    const stub = await boot(page, { windows: true, macos: true });
    const commitSha = 'c'.repeat(40);
    await stub.emit('augur://update-event', {
      type: 'status',
      status: {
        state: 'available',
        currentVersion: '0.1.0',
        latestVersion: '0.1.1-nightly.24',
        latestCommitSha: commitSha,
        canInstall: false,
        progress: null,
        error: null,
        installChannel: 'homebrew-cask'
      }
    });
    await stub.emit('augur://update-event', {
      type: 'notice',
      notice: { commitSha, version: '0.1.1-nightly.24' }
    });

    const copy = page.getByTestId('homebrew-copy-upgrade');
    await expect(copy).toBeVisible();
    await copy.click();
    expect(await stub.clipboard()).toBe('brew upgrade --cask augur-git');
  });
});

test.describe('custom title bar', () => {
  test('drags from empty tab-bar space while keeping its controls interactive', async ({
    page
  }) => {
    // Dragging is a JS handler on Windows and Linux, and a native drag region
    // on macOS that the browser stub cannot simulate, so pin the platform.
    const stub = await boot(page, {
      open: [fixtureRepo(), secondFixtureRepo()],
      windows: true
    });
    const countCommands = async (cmd: string) =>
      (await stub.commands()).filter((entry) => entry.cmd === cmd).length;

    const tabBar = page.getByTestId('tab-bar');
    const tabBarBox = await tabBar.boundingBox();
    const newTabBox = await page.getByTestId('tab-new').boundingBox();
    expect(tabBarBox).not.toBeNull();
    expect(newTabBox).not.toBeNull();
    const blankX = tabBarBox!.x + tabBarBox!.width - 8;
    const blankY = tabBarBox!.y + tabBarBox!.height / 2;
    expect(blankX).toBeGreaterThan(newTabBox!.x + newTabBox!.width);
    expect(await tabBar.evaluate((element) => getComputedStyle(element).cursor)).toBe('default');

    await page.mouse.click(blankX, blankY);
    await expect.poll(() => countCommands('plugin:window|start_dragging')).toBe(1);

    await page.mouse.dblclick(blankX, blankY);
    await expect.poll(() => countCommands('plugin:window|toggle_maximize')).toBe(1);
    await expect.poll(() => countCommands('plugin:window|start_dragging')).toBe(2);

    const firstTab = page.getByTestId(`tab-${fixtureRepo().path}`);
    const secondTab = page.getByTestId(`tab-${secondFixtureRepo().path}`);
    const firstBounds = await firstTab.boundingBox();
    const secondBounds = await secondTab.boundingBox();
    expect(firstBounds).not.toBeNull();
    expect(secondBounds).not.toBeNull();
    const startX = secondBounds!.x + secondBounds!.width / 2;
    const startY = secondBounds!.y + secondBounds!.height / 2;
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX - 12, startY, { steps: 2 });
    await page.mouse.move(firstBounds!.x + 8, startY);
    await page.mouse.up();
    await expect(page.locator('.tab__label')).toHaveText(['other-app', 'augur-git']);
    await expect.poll(() => countCommands('plugin:window|start_dragging')).toBe(2);

    await page.locator('.tab__label').first().click();
    await page.locator('.tab__close').first().click();
    await page.getByTestId('tab-new').click();
    await expect.poll(() => countCommands('plugin:window|start_dragging')).toBe(2);
  });

  test('marks only the empty tab-bar surface as draggable on macOS', async ({ page }) => {
    const first = fixtureRepo();
    const second = secondFixtureRepo();
    const stub = await boot(page, { open: [first, second], macos: true });
    const tabBar = page.getByTestId('tab-bar');

    await expect(tabBar).toHaveAttribute('data-tauri-drag-region', 'true');
    await expect(page.locator('.tab').first()).not.toHaveAttribute('data-tauri-drag-region');
    await expect(page.locator('.tab').last()).not.toHaveAttribute('data-tauri-drag-region');
    await expect(page.locator('.tab__close').first()).not.toHaveAttribute('data-tauri-drag-region');
    await expect(page.locator('.tab__close').last()).not.toHaveAttribute('data-tauri-drag-region');
    await expect(page.getByTestId('tab-new')).not.toHaveAttribute('data-tauri-drag-region');

    const firstTab = page.getByTestId(`tab-${first.path}`);
    const secondTab = page.getByTestId(`tab-${second.path}`);
    const firstBounds = await firstTab.boundingBox();
    const secondBounds = await secondTab.boundingBox();
    expect(firstBounds).not.toBeNull();
    expect(secondBounds).not.toBeNull();
    const startX = secondBounds!.x + secondBounds!.width / 2;
    const startY = secondBounds!.y + secondBounds!.height / 2;
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX - 12, startY, { steps: 2 });
    await page.mouse.move(firstBounds!.x + 8, startY);
    await page.mouse.up();
    await expect(page.locator('.tab__label')).toHaveText(['other-app', 'augur-git']);
    await expect
      .poll(async () => {
        const saves = (await stub.commands()).filter((entry) => entry.cmd === 'set_workspace_tabs');
        const args = saves.at(-1)?.args as { tabs: { path: string }[] } | undefined;
        return args?.tabs.map((tab) => tab.path);
      })
      .toEqual([second.path, first.path]);
  });

  test('drags the main window from its blank region and leaves controls clickable', async ({
    page
  }) => {
    // Dragging is a JS handler on Windows and Linux, and a native drag region
    // on macOS that the browser stub cannot simulate, so pin the platform.
    const stub = await boot(page, { open: [fixtureRepo()], windows: true });
    const dragCount = async () =>
      (await stub.commands()).filter((entry) => entry.cmd === 'plugin:window|start_dragging')
        .length;

    await page.locator('.title-bar__drag').click();
    await expect.poll(dragCount).toBe(1);
    await page.getByTestId('title-settings').click();
    await expect.poll(dragCount).toBe(1);
  });
});

test.describe('settings window', () => {
  test('opens directly to the theme selector and responds to navigation requests', async ({
    page
  }) => {
    const stub = await boot(page, { window: 'settings', settingsSection: 'appearance' });

    await expect(page.getByTestId('settings-appearance')).toBeVisible();
    await expect(page.getByTestId('settings-theme')).toBeFocused();

    await page.getByTestId('settings-nav-general').click();
    await stub.emit('augur://settings-navigate', { section: 'appearance' });
    await expect(page.getByTestId('settings-appearance')).toBeVisible();
    await expect(page.getByTestId('settings-theme')).toBeFocused();
  });

  test('marks the current choice in a mode menu', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    // Commit or amend: opening the menu and reading it has to say which one the
    // button will act on.
    const trigger = page.getByTestId('commit-mode-trigger');
    await trigger.click();
    const mode = page.getByTestId('commit-mode');
    await expect(mode.getByTestId('commit-mode-commit').locator('.menu__check')).toBeVisible();
    await expect(mode.getByTestId('commit-mode-amend').locator('.menu__check')).toHaveCount(0);
    // The list is end-aligned, so an adjustment bug that reruns the placement
    // would walk it to the window's left edge; pin it to the trigger instead.
    const triggerBox = (await trigger.boundingBox())!;
    const menuBox = (await mode.boundingBox())!;
    expect(menuBox.x + menuBox.width).toBeCloseTo(triggerBox.x + triggerBox.width, 0);
    expect(menuBox.y).toBeGreaterThanOrEqual(triggerBox.y + triggerBox.height);
    expect(menuBox.y - (triggerBox.y + triggerBox.height)).toBeLessThan(6);
    await page.getByTestId('commit-mode-amend').click();
    await trigger.click();
    await expect(mode.getByTestId('commit-mode-amend').locator('.menu__check')).toBeVisible();
    await expect(mode.getByTestId('commit-mode-commit').locator('.menu__check')).toHaveCount(0);
  });

  test('opens under its own title bar without a maximize control', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'settings', windows: true });

    await expect(page.getByTestId('settings-window')).toBeVisible();
    await expect(page.getByTestId('settings-title')).toHaveText('Settings');
    await expect(page.getByTestId('window-controls')).toBeVisible();
    // The backend keeps the window at a fixed size, so there is nothing to
    // maximize; minimize and close remain.
    await expect(page.getByTestId('window-toggle-maximize')).toHaveCount(0);
    await expect(page.getByTestId('window-minimize')).toBeVisible();
    await expect(page.getByTestId('window-close')).toBeVisible();
  });

  test('shows the shipped shortcut binding next to an override', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'settings' });

    await page.getByTestId('settings-nav-shortcuts').click();

    // The default is shown so an override reads as a choice rather than a guess
    // at what it replaced.
    await expect(page.getByTestId('shortcut-default-app.quit')).toContainText(
      'Default: CmdOrCtrl+Q'
    );
  });

  test('searches settings fuzzily and jumps to the selected field', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'settings' });

    const search = page.getByTestId('settings-search');
    await search.fill('uif');
    await expect(page.getByTestId('settings-search-result-settings-field-ui-font')).toBeVisible();
    await search.press('ArrowDown');
    await expect(
      page.getByTestId('settings-search-result-settings-field-ui-font-size')
    ).toHaveAttribute('aria-selected', 'true');
    await search.press('Enter');

    await expect(page.getByTestId('settings-appearance')).toBeVisible();
    await expect(page.getByTestId('settings-field-ui-font-size')).toHaveClass(/is-flash/);
  });

  test('shows an empty state and clears the settings query with Escape', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'settings' });

    const search = page.getByTestId('settings-search');
    await search.fill('no-such-setting');
    await expect(page.getByText('No matching settings found.')).toBeVisible();
    await search.press('Escape');
    await expect(search).toHaveValue('');
    await expect(page.getByText('No matching settings found.')).toHaveCount(0);
  });

  test('fuzzy-filters shortcut rows independently of the global settings search', async ({
    page
  }) => {
    await boot(page, { open: [fixtureRepo()], window: 'settings' });
    await page.getByTestId('settings-nav-shortcuts').click();

    const filter = page.getByTestId('shortcut-filter');
    await filter.fill('pul');
    await expect(page.getByTestId('shortcut-repo.pull')).toBeVisible();
    await expect(page.getByTestId('shortcut-repo.push')).toHaveCount(0);
    await expect(page.getByTestId('shortcut-app.quit')).toHaveCount(0);
  });

  test('changes the theme and preserves complete font family names', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()], window: 'settings' });

    await page.getByTestId('settings-nav-appearance').click();

    await page.getByTestId('settings-theme').click();
    await page.getByTestId('select-option-github-dark').click();

    // The preference is written straight through, so it survives a crash.
    const commands = await stub.commands();
    const setTheme = commands.filter((entry) => entry.cmd === 'set_theme');
    expect(setTheme).toHaveLength(1);
    expect((setTheme[0]!.args as any).theme).toBe('github-dark');

    await page.getByTestId('settings-ui-font').click();
    await page.getByTestId('select-option-source-sans-3').click();
    const setType = (await stub.commands()).filter((e) => e.cmd === 'set_typography');
    expect(setType).toHaveLength(1);
    expect((setType[0]!.args as any).typography.ui_font_family).toBe('Source Sans 3');
    await expect
      .poll(() =>
        page.evaluate(() => document.documentElement.style.getPropertyValue('--ui-font-family'))
      )
      .toBe('"Source Sans 3"');
  });

  test('searches grouped themes and applies a new light theme immediately', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()], window: 'settings' });
    await page.getByTestId('settings-nav-appearance').click();
    await page.getByTestId('settings-theme').click();

    const search = page.getByRole('textbox', { name: 'Search themes…' });
    await search.fill('Tokyo Night');
    await expect(page.getByTestId('select-option-tokyo-night')).toBeVisible();
    await expect(page.getByTestId('select-option-tokyo-night-storm')).toBeVisible();
    await expect(page.getByTestId('select-option-tokyo-night-light')).toBeVisible();
    await expect(page.getByRole('option')).toHaveCount(3);

    await page.getByTestId('select-option-tokyo-night-light').click();
    await expect(page.locator('html')).toHaveAttribute('data-mode', 'light');
    await expect
      .poll(() =>
        page
          .locator('html')
          .evaluate((element) => getComputedStyle(element).getPropertyValue('--background').trim())
      )
      .toBe('#e6e7ed');
    const setTheme = (await stub.commands()).filter((entry) => entry.cmd === 'set_theme');
    expect(setTheme).toHaveLength(1);
    expect((setTheme[0]!.args as any).theme).toBe('tokyo-night-light');
  });

  test('shows a saved font that is not in the discovered system list', async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      window: 'settings',
      typography: { ui_font_family: 'Saved Custom Font' },
      fontFamilies: ['Inter']
    });

    await page.getByTestId('settings-nav-appearance').click();

    await expect(page.getByTestId('settings-ui-font')).toContainText('Saved Custom Font');
    await expect
      .poll(() =>
        page.evaluate(() => document.documentElement.style.getPropertyValue('--ui-font-family'))
      )
      .toBe('"Saved Custom Font"');
  });

  test('accepts an exact typed font family when discovery omits it', async ({ page }) => {
    const stub = await boot(page, {
      open: [fixtureRepo()],
      window: 'settings',
      fontFamilies: ['Inter']
    });

    await page.getByTestId('settings-nav-appearance').click();
    await page.getByTestId('settings-ui-font').click();
    const search = page.locator('.select__search input');
    await search.fill('A Font With Spaces');
    await search.press('Enter');

    await expect(page.getByTestId('settings-ui-font')).toContainText('A Font With Spaces');
    const setType = (await stub.commands()).filter((entry) => entry.cmd === 'set_typography');
    expect((setType.at(-1)!.args as any).typography.ui_font_family).toBe('A Font With Spaces');
  });

  test('changes the diff layout and the history scope', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()], window: 'settings' });

    await page.getByTestId('settings-nav-layout').click();

    await page.getByTestId('settings-diff-layout').click();
    await page.getByTestId('select-option-inline').click();
    await page.getByTestId('settings-graph-history').click();
    await page.getByTestId('select-option-current-branch-and-upstream').click();

    const commands = await stub.commands();
    expect(
      commands.some(
        (entry) => entry.cmd === 'set_diff_layout' && (entry.args as any).layout === 'inline'
      )
    ).toBe(true);
    expect(
      commands.some(
        (entry) =>
          entry.cmd === 'set_view' && (entry.args as any).view.graph_history === 'current-branch'
      )
    ).toBe(true);
  });

  test('changes the pull action the toolbar button performs', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()], window: 'settings' });

    await page.getByTestId('settings-nav-layout').click();
    await page.getByTestId('settings-pull-action').click();
    await page.getByTestId('select-option-rebase').click();

    const commands = await stub.commands();
    const views = commands.filter((entry) => entry.cmd === 'set_view');
    expect(views).toHaveLength(1);
    expect((views[0]!.args as any).view.pull_action).toBe('rebase');
  });

  test('unbinds an empty shortcut and accepts a real one', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()], window: 'settings' });

    await page.getByTestId('settings-nav-shortcuts').click();

    await page.getByTestId('shortcut-app.quit').fill('');
    await page.getByTestId('shortcut-app.quit').press('Enter');
    await expect(page.getByTestId('shortcut-error')).toHaveCount(0);
    const unbound = (await stub.commands()).filter((e) => e.cmd === 'set_shortcut');
    expect(unbound).toHaveLength(1);
    expect((unbound[0]!.args as any).keys).toEqual([]);

    await page.getByTestId('shortcut-app.quit').fill('CmdOrCtrl+Shift+Q');
    await page.getByTestId('shortcut-app.quit').press('Enter');
    const written = (await stub.commands()).filter((e) => e.cmd === 'set_shortcut');
    expect(written).toHaveLength(2);
    expect((written[1]!.args as any).keys).toEqual(['cmdorctrl-shift-q']);
  });

  test('shows where the settings are stored', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], window: 'settings' });
    await expect(page.getByTestId('settings-general')).toContainText('com.augur.git.tauri');
  });

  test('navigating the sections writes nothing on its own', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()], window: 'settings' });
    for (const section of ['appearance', 'layout', 'shortcuts', 'general']) {
      await page.getByTestId(`settings-nav-${section}`).click();
      await expect(page.getByTestId(`settings-${section}`)).toBeVisible();
    }
    // Reading the settings is not changing them.
    expect((await stub.commands()).filter((e) => e.cmd === 'set_theme')).toHaveLength(0);
    expect((await stub.commands()).filter((e) => e.cmd === 'set_typography')).toHaveLength(0);
  });
});

test.describe('the About window', () => {
  test('reports the identity of this application', async ({ page }) => {
    await boot(page, { window: 'about' });

    await expect(page.getByTestId('about')).toBeVisible();
    await expect(page.getByTestId('about-version')).toHaveText('0.1.0');
    await expect(page.getByTestId('about-commit')).toHaveText('abc1234');
    // The identifier is what keeps the two products from sharing a data
    // directory, so it is on the page.
    await expect(page.getByTestId('about-identifier')).toHaveText('com.augur.git.tauri');
    await expect(page.getByTestId('about')).toContainText('com.augur.git.tauri/settings.json');
  });

  test('keeps its title clear of the macOS traffic lights', async ({ page }) => {
    // The backend opens this window with an overlay title bar, so the traffic
    // lights are drawn on top of the webview and the title has to start after
    // them rather than underneath.
    await boot(page, { window: 'about', macos: true });

    const title = page.getByTestId('about-title');
    const bounds = await title.boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.x).toBeGreaterThanOrEqual(78);
    // Dragging stays on the empty part of the bar, so the title text keeps
    // its normal text selection behavior.
    await expect(page.locator('.window-titlebar__drag')).toHaveAttribute(
      'data-tauri-drag-region',
      'true'
    );
  });
});

test.describe('the in-window menu', () => {
  test('opens a repository from the menu', async ({ page }) => {
    const stub = await boot(page, { windows: true });
    await page.getByTestId('menu-file-trigger').click();
    await page.getByTestId('menu-file-file').click();
    await page.getByTestId('menu-file-open-repository').click();
    await expect(page.getByTestId('repo-7')).toBeVisible();
    expect((await stub.commandNames()).filter((c) => c === 'open_repository')).toHaveLength(1);
  });

  test('lists the recent repositories', async ({ page }) => {
    await boot(page, { windows: true });
    await page.getByTestId('menu-file-trigger').click();
    await page.getByTestId('menu-file-file').click();
    await page.getByTestId('menu-file-recent-repositories').click();
    const recent = page.getByTestId('menu-file-recent-repositories-submenu');
    await expect(recent).toContainText('augur-git');
    await expect(recent).toContainText('other-app');
  });

  test('opens settings from the Edit menu on the welcome page', async ({ page }) => {
    const stub = await boot(page, { windows: true });
    await expect(page.getByTestId('title-settings')).toBeVisible();
    await page.getByTestId('menu-file-trigger').click();
    await page.getByTestId('menu-file-edit').click();
    await page.getByTestId('menu-file-settings').click();
    expect(await stub.commandNames()).toContain('open_settings_window');
  });

  test('switches window mode from View and updates the available destination', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], windows: true });

    await page.getByTestId('menu-file-trigger').click();
    await page.getByTestId('menu-file-view').click();
    await page.getByTestId('menu-file-mode-toggle').click();
    await expect(page.getByTestId('sidecar-window')).toBeVisible();

    await page.getByTestId('menu-file-trigger').click();
    await page.getByTestId('menu-file-view').click();
    await expect(page.getByTestId('menu-file-mode-toggle')).toHaveText('Switch to Desktop mode');
    await page.getByTestId('menu-file-mode-toggle').click();
    await expect(page.getByTestId('repo-7')).toBeVisible();
  });

  test('opens the theme settings section from View', async ({ page }) => {
    const stub = await boot(page, { windows: true });

    await page.getByTestId('menu-file-trigger').click();
    await page.getByTestId('menu-file-view').click();
    await page.getByTestId('menu-file-appearance').click();

    const settings = (await stub.commands()).find((entry) => entry.cmd === 'open_settings_window');
    expect(settings?.args.section).toBe('appearance');
  });

  test('adjusts diff font size from View and respects its bounds', async ({ page }) => {
    const stub = await boot(page, {
      open: [fixtureRepo()],
      typography: { diff_font_size: 20 },
      windows: true
    });
    const openView = async () => {
      await page.getByTestId('menu-file-trigger').click();
      await page.getByTestId('menu-file-view').click();
    };

    await openView();
    await page.getByTestId('menu-file-diff-font-increase').click();
    expect((await stub.commands()).filter((entry) => entry.cmd === 'set_typography')).toHaveLength(
      0
    );

    await openView();
    await page.getByTestId('menu-file-diff-font-decrease').click();
    await openView();
    await page.getByTestId('menu-file-diff-font-reset').click();

    const typography = (await stub.commands())
      .filter((entry) => entry.cmd === 'set_typography')
      .map((entry) => (entry.args.typography as { diff_font_size: number }).diff_font_size);
    expect(typography).toEqual([19, 16]);
  });

  test('opens the shared Branch actions from Edit', async ({ page }) => {
    const repo = fixtureRepo();
    repo.status.files = repo.status.files.filter((file) => file.index !== 'U');
    await boot(page, { open: [repo], windows: true });

    await page.getByTestId('menu-file-trigger').click();
    await page.getByTestId('menu-file-edit').click();
    await page.getByTestId('menu-file-branch').click();
    await page.getByTestId('menu-file-branch-new').click();

    await expect(page.getByTestId('branch-dialog')).toBeVisible();
  });

  test('opens About from the Help menu on the welcome page', async ({ page }) => {
    const stub = await boot(page, { windows: true });

    await page.getByTestId('menu-file-trigger').click();
    await page.getByTestId('menu-file-help').click();
    await page.getByTestId('menu-file-about').click();

    expect(await stub.commandNames()).toContain('open_about_window');
  });

  test('opens settings from the title-bar gear on the welcome page', async ({ page }) => {
    const stub = await boot(page, { windows: true });
    await page.getByTestId('title-settings').click();
    expect(await stub.commandNames()).toContain('open_settings_window');
  });

  test('opens About from the title-bar app name', async ({ page }) => {
    const stub = await boot(page, { windows: true });
    const brand = page.getByTestId('title-about');
    await expect(brand).toBeVisible();
    await expect(brand).toHaveAttribute('aria-label', 'About Augur Git');
    await brand.click();
    expect(await stub.commandNames()).toContain('open_about_window');
  });

  test('places the title card before tabs and settings before window controls', async ({
    page
  }) => {
    const stub = await boot(page, { open: [fixtureRepo()], windows: true });
    await expect(page.getByTestId('tab-bar')).toBeVisible();
    await expect(page.getByTestId('title-branch')).toHaveCount(0);
    await expect(page.getByTestId('title-settings')).toBeVisible();
    const brand = page.locator('.title-bar__brand');
    await expect(brand).toHaveText('Augur Git');
    await expect(brand).toHaveCSS('font-weight', '700');
    await expect(brand).toHaveCSS('border-radius', '6px');
    await expect(brand.locator('svg')).toBeVisible();
    await expect(page.getByTestId('title-settings')).toHaveText('');
    await expect(page.getByTestId('title-settings')).toHaveAttribute('aria-label', 'Settings');
    const usesThemeColor = await brand.evaluate((element) => {
      const probe = document.createElement('span');
      probe.style.color = 'var(--primary-foreground)';
      document.body.append(probe);
      const expected = getComputedStyle(probe).color;
      probe.remove();
      return getComputedStyle(element).color === expected;
    });
    expect(usesThemeColor).toBe(true);
    const usesButtonBackground = await brand.evaluate((element) => {
      const probe = document.createElement('span');
      probe.style.backgroundColor = 'var(--primary-background)';
      document.body.append(probe);
      const expected = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return getComputedStyle(element).backgroundColor === expected;
    });
    expect(usesButtonBackground).toBe(true);

    const positions = await page.evaluate(() => {
      const left = (selector: string) =>
        document.querySelector(selector)?.getBoundingClientRect().left ?? -1;
      return {
        brand: left('.title-bar__brand'),
        tabs: left('.tab-bar'),
        settings: left('[data-testid="title-settings"]'),
        controls: left('.window-controls')
      };
    });
    expect(positions.brand).toBeLessThan(positions.tabs);
    expect(positions.settings).toBeLessThan(positions.controls);

    await page.getByTestId('title-settings').click();
    expect((await stub.commandNames()).filter((c) => c === 'open_settings_window')).toHaveLength(1);
    await expect(page.getByTestId('window-controls')).toBeVisible();
  });
});

test.describe('the native menu bridge', () => {
  test('routes a native menu activation to the same handler', async ({ page }) => {
    const stub = await boot(page);

    // The native menu dispatches a DOM event so both surfaces run one handler.
    await page.evaluate(() => {
      (window as any).__STUB__.emit('augur://menu', { id: 'menu.open-repository' });
    });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.evaluate(() => {
      (window as any).__STUB__.emit('augur://menu', { id: 'menu.about' });
    });
    expect(await stub.commandNames()).toContain('open_about_window');

    await page.evaluate(() => {
      (window as any).__STUB__.emit('augur://menu', { id: 'menu.settings' });
    });
    expect(await stub.commandNames()).toContain('open_settings_window');

    await page.evaluate(() => {
      (window as any).__STUB__.emit('augur://menu', { id: 'menu.view.appearance' });
    });
    const settings = (await stub.commands()).filter(
      (entry) => entry.cmd === 'open_settings_window'
    );
    expect(settings.at(-1)?.args.section).toBe('appearance');

    await page.evaluate(() => {
      (window as any).__STUB__.emit('augur://menu', { id: 'menu.view.diff-font-increase' });
    });
    const typography = (await stub.commands()).filter((entry) => entry.cmd === 'set_typography');
    expect((typography.at(-1)?.args.typography as { diff_font_size: number }).diff_font_size).toBe(
      17
    );
  });

  test('opens paths handed over by a second launch', async ({ page }) => {
    await boot(page);

    await page.evaluate(() => {
      (window as any).__STUB__.emit('augur://open-paths', {
        paths: ['/Users/dev/projects/augur-git']
      });
    });
    await expect(page.getByTestId('repo-7')).toBeVisible();
  });

  test('opens a dropped folder', async ({ page }) => {
    await boot(page);
    await page.evaluate(() => {
      (window as any).__STUB__.emit('augur://drop-paths', {
        paths: ['/Users/dev/projects/other-app']
      });
    });
    await expect(page.getByTestId('repo-9')).toBeVisible();
  });

  test('shows a notice pushed by the backend', async ({ page }) => {
    await boot(page);
    await page.evaluate(() => {
      (window as any).__STUB__.emit('augur://app-event', {
        type: 'notice',
        level: 'warning',
        message: 'The index needs rebuilding'
      });
    });
    await expect(page.getByTestId('notice')).toContainText('The index needs rebuilding');
    await page.getByTestId('notice').getByRole('button').click();
    await expect(page.getByTestId('notice')).toHaveCount(0);
  });
});
