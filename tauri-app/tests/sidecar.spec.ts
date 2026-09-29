import { expect, test } from '@playwright/test';

import { boot, fixtureRepo, longFixtureRepo, secondFixtureRepo } from './harness';

test.describe('Sidecar mode', () => {
  test('keeps the graph compact, opens full-page diffs, and returns to the source page', async ({
    page
  }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await page.setViewportSize({ width: 360, height: 480 });
    await page.getByTestId('title-sidecar-toggle').click();
    await expect(page.getByTestId('sidecar-window')).toBeVisible();
    await expect(page.getByTestId('sidecar-page-changes')).toBeVisible();

    const expectNoOverflow = async (width: number) => {
      const overflow = await page.evaluate(() => {
        const app = document.querySelector<HTMLElement>('.app')!;
        const toolbar = document.querySelector<HTMLElement>('.toolbar--sidecar');
        return {
          root: document.documentElement.scrollWidth > window.innerWidth,
          app: app.scrollWidth > app.clientWidth,
          toolbar: toolbar ? toolbar.scrollWidth > toolbar.clientWidth : false
        };
      });
      expect(overflow, `horizontal overflow at ${width}px`).toEqual({
        root: false,
        app: false,
        toolbar: false
      });
    };

    for (const width of [360, 420, 520]) {
      await page.setViewportSize({ width, height: 480 });
      await expectNoOverflow(width);

      await page.getByTestId('sidecar-nav-history').click();
      await expect(page.getByTestId('sidecar-page-history')).toBeVisible();
      await expectNoOverflow(width);
      await page.getByTestId('sidecar-graph-scope-trigger').click();
      await expect(page.getByText('Current branch and upstream')).toBeVisible();
      await page.getByText('Current branch and upstream').click();

      const firstCommit = page.locator('.graph-row').first();
      await firstCommit.hover();
      const preview = page.getByTestId('commit-preview');
      await expect(preview).toBeVisible();
      const previewFits = await preview.evaluate((element) => {
        const bounds = element.getBoundingClientRect();
        return (
          bounds.left >= 0 &&
          bounds.top >= 0 &&
          bounds.right <= window.innerWidth &&
          bounds.bottom <= window.innerHeight
        );
      });
      expect(previewFits).toBe(true);
      await page.mouse.move(320, 450);
      await firstCommit.click();
      await expect(page.getByTestId('sidecar-page-diff')).toBeVisible();
      await expect(page.getByTestId('sidecar-diff-file-select')).toHaveValue('__all__');
      await expect(page.getByTestId('diff-view')).toBeVisible();
      await expectNoOverflow(width);
      await page.getByTestId('sidecar-diff-file-select').selectOption('src/lib.rs');
      await expect(page.getByTestId('sidecar-diff-file-select')).toHaveValue('src/lib.rs');
      await page.getByTestId('sidecar-diff-back').click();
      await expect(page.getByTestId('sidecar-page-history')).toBeVisible();

      await page.getByTestId('sidecar-nav-branches').click();
      await expect(page.getByTestId('sidecar-page-branches')).toBeVisible();
      await expectNoOverflow(width);
      await page.getByTestId('sidecar-nav-changes').click();
    }

    await page.setViewportSize({ width: 360, height: 480 });
    await page.getByTestId('changes-file-src/git/graph.rs').click();
    await expect(page.getByTestId('sidecar-page-diff')).toBeVisible();
    await expect(page.getByTestId('sidecar-diff-file-select')).toHaveValue(
      'changes:src/git/graph.rs'
    );
    await page.getByTestId('sidecar-diff-file-select').selectOption('changes:notes.md');
    await expect(page.getByTestId('sidecar-diff-file-select')).toHaveValue('changes:notes.md');
    await page.getByTestId('sidecar-diff-back').click();
    await expect(page.getByTestId('sidecar-page-changes')).toBeVisible();

    await page.getByTestId('sidecar-more').click();
    await page.getByText('Push (Force)', { exact: true }).click();
    const forceDialog = page.getByTestId('force-push-dialog');
    await expect(forceDialog).toBeVisible();
    const dialogFits = await forceDialog.evaluate((element) => {
      const bounds = element.getBoundingClientRect();
      return (
        bounds.left >= 0 &&
        bounds.top >= 0 &&
        bounds.right <= window.innerWidth &&
        bounds.bottom <= window.innerHeight
      );
    });
    expect(dialogFits).toBe(true);
    await page.getByTestId('force-push-cancel').click();

    await page.getByTestId('sidecar-mode-toggle').click();
    await expect(page.getByTestId('repo-7')).toBeVisible();
    expect((await stub.commands()).some((entry) => entry.cmd === 'set_window_mode')).toBe(true);
    await page.getByTestId('title-sidecar-toggle').click();
    await page.reload();
    await expect(page.getByTestId('sidecar-window')).toBeVisible();
  });

  test('keeps drafts per repository and retains the draft after a failed commit', async ({
    page
  }) => {
    await boot(page, {
      open: [fixtureRepo(), secondFixtureRepo()],
      failingActions: ['commit']
    });
    await page.setViewportSize({ width: 420, height: 480 });
    await page.getByTestId('title-sidecar-toggle').click();

    const repoPicker = page.getByTestId('sidecar-repository-select');
    const firstRepoKey = await repoPicker.evaluate(
      (element) => (element as HTMLSelectElement).value
    );
    const firstDraft = page.getByTestId('commit-message');
    await firstDraft.fill('First repository draft');
    await page.getByTestId('sidecar-nav-branches').click();
    await page.getByTestId('sidecar-nav-changes').click();
    await expect(page.getByTestId('commit-message')).toHaveValue('First repository draft');

    const secondRepoKey = await repoPicker.locator('option').nth(1).getAttribute('value');
    await repoPicker.selectOption(secondRepoKey!);
    await expect(page.getByTestId('commit-message')).toHaveValue('');
    await page.getByTestId('commit-message').fill('Second repository draft');
    await repoPicker.selectOption(firstRepoKey);
    await expect(page.getByTestId('commit-message')).toHaveValue('First repository draft');

    await page.getByTestId('commit-submit').click();
    await expect(page.locator('.sidecar__repo-message.is-error')).toBeVisible();
    await expect(page.getByTestId('commit-message')).toHaveValue('First repository draft');
  });

  test('shows commit details to keyboard users and restores history search and scroll after Diff', async ({
    page
  }) => {
    await boot(page, { open: [longFixtureRepo(120)] });
    await page.setViewportSize({ width: 360, height: 480 });
    await page.getByTestId('title-sidecar-toggle').click();
    await page.getByTestId('sidecar-nav-history').click();

    const graph = page.getByTestId('graph-list');
    await graph.focus();
    await page.keyboard.press('j');
    const preview = page.getByTestId('commit-preview');
    await expect(preview).toBeVisible();
    await expect(preview.locator('.commit-preview__meta')).toBeVisible();

    await graph.evaluate((element) => {
      element.scrollTop = 40 * 22;
    });
    await expect.poll(() => graph.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
    const savedScrollTop = await graph.evaluate((element) => element.scrollTop);
    const visibleRow = page.locator('.graph-row').first();
    await visibleRow.click();
    await expect(page.getByTestId('sidecar-page-diff')).toBeVisible();
    await page.getByTestId('sidecar-diff-back').click();
    await expect(page.getByTestId('sidecar-page-history')).toBeVisible();
    await expect
      .poll(() => page.getByTestId('graph-list').evaluate((element) => element.scrollTop))
      .toBe(savedScrollTop);

    const query = 'Long history commit 74';
    await page.getByTestId('commit-search').fill(query);
    await expect(page.locator('.graph-row')).toHaveCount(1);
    await page.locator('.graph-row').click();
    await expect(page.getByTestId('sidecar-page-diff')).toBeVisible();
    await page.getByTestId('sidecar-diff-back').click();
    await expect(page.getByTestId('commit-search')).toHaveValue(query);
    await expect(page.locator('.graph-row')).toHaveCount(1);
  });

  test('previews commits on hover, retires the preview for the context menu, and anchors it after a deep scroll', async ({
    page
  }) => {
    await boot(page, { open: [longFixtureRepo()] });
    await page.setViewportSize({ width: 360, height: 480 });
    await page.getByTestId('title-sidecar-toggle').click();
    await page.getByTestId('sidecar-nav-history').click();

    const row = page.locator('.graph-row').first();
    await row.hover();
    const preview = page.getByTestId('commit-preview');
    await expect(preview).toBeVisible();
    await expect(preview.locator('.commit-preview__meta')).toBeVisible();

    // The menu opens at the same cursor anchor as the preview, so the preview
    // retires rather than the two stacking over each other.
    const box = (await row.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2, {
      button: 'right'
    });
    const menu = page.locator('.context-menu');
    await expect(menu).toBeVisible();
    await expect(preview).toHaveCount(0);

    // Rows crossed while the menu is up show no preview behind it either. The
    // pointer is driven directly because the menu covers the row's centre and
    // a locator hover would refuse to move over it.
    const below = (await page.locator('.graph-row').nth(1).boundingBox())!;
    await page.mouse.move(below.x + below.width / 2, below.y + below.height / 2);
    await expect(preview).toHaveCount(0);

    // Closing the menu leaves the preview gone; the next hover brings it back,
    // the way a native tooltip behaves.
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await row.hover();
    await expect(preview).toBeVisible();

    // A deep scroll used to fling the preview far below the hovered row: it
    // was absolutely positioned against the virtual list's transformed window
    // instead of the viewport. The pointer leaves the list first, so no stale
    // hover survives the scroll. The scroll puts the target row near the top
    // of the list, away from the bottom edge where the flip-up would move the
    // preview off the cursor.
    await page.getByTestId('commit-search').hover();
    const list = page.getByTestId('graph-list');
    await list.evaluate((element) => {
      element.scrollTop = 70 * 22;
    });
    await expect.poll(() => list.evaluate((element) => element.scrollTop)).toBe(70 * 22);

    const deepRow = page.locator('.graph-row', { hasText: 'Long history commit 74' });
    await expect(deepRow).toBeVisible();
    const deepBox = (await deepRow.boundingBox())!;
    const cursor = { x: deepBox.x + deepBox.width / 2, y: deepBox.y + deepBox.height / 2 };
    await page.mouse.move(cursor.x, cursor.y);

    await expect(preview).toBeVisible();
    // The narrow window clamps the preview's left edge to stay inside the
    // viewport, so only the vertical anchor is expected at the cursor: the old
    // displacement bug flung the preview below the hovered row.
    const previewBox = (await preview.boundingBox())!;
    expect(Math.abs(previewBox.y - cursor.y)).toBeLessThanOrEqual(2);
    const previewFits = await preview.evaluate((element) => {
      const bounds = element.getBoundingClientRect();
      return (
        bounds.left >= 0 &&
        bounds.top >= 0 &&
        bounds.right <= window.innerWidth &&
        bounds.bottom <= window.innerHeight
      );
    });
    expect(previewFits).toBe(true);
  });

  test('clears a commit draft only after a successful commit', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });
    await page.setViewportSize({ width: 520, height: 480 });
    await page.getByTestId('title-sidecar-toggle').click();
    await page.getByTestId('commit-message').fill('Successful sidecar commit');
    await page.getByTestId('commit-submit').click();
    await expect(page.getByTestId('commit-message')).toHaveValue('');
  });

  test('keeps the mode switch available when no repository is open', async ({ page }) => {
    await boot(page);
    await page.getByTestId('title-sidecar-toggle').click();
    await expect(page.getByTestId('title-sidecar-toggle')).toHaveAttribute(
      'aria-label',
      'Switch to Desktop mode'
    );
    await page.getByTestId('title-sidecar-toggle').click();
    await expect(page.getByTestId('window-welcome')).toBeVisible();
  });
});
