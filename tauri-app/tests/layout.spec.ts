import { expect, test, type Page } from '@playwright/test';

import { boot, fixtureRepo } from './harness';

async function dragBy(page: Page, selector: string, deltaX: number, deltaY = 0): Promise<void> {
  const handle = page.getByTestId(selector);
  const box = await handle.boundingBox();
  if (!box) {
    throw new Error(`No visible splitter: ${selector}`);
  }
  const startX = box.x + box.width / 2;
  const startY = box.y + box.height / 2;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX + deltaX, startY + deltaY, { steps: 8 });
  await page.mouse.up();
}

test.describe('pane layout', () => {
  test('captures drags beyond the handle and restores the saved width', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    const handle = page.getByTestId('sidebar-splitter');
    const box = await handle.boundingBox();
    expect(box).not.toBeNull();
    const startX = box!.x + box!.width / 2;
    const startY = box!.y + box!.height / 2;

    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX + 90, startY, { steps: 6 });
    await page.mouse.move(startX + 50, startY, { steps: 4 });
    await page.mouse.up();

    await expect(page.getByTestId('repo-sidebar')).toHaveCSS('width', '300px');
    await expect
      .poll(
        async () => (await stub.commands()).filter((entry) => entry.cmd === 'set_layout').length
      )
      .toBe(1);

    await page.mouse.move(startX + 150, startY);
    await expect(page.getByTestId('repo-sidebar')).toHaveCSS('width', '300px');

    await page.reload();
    await expect(page.getByTestId('repo-sidebar')).toHaveCSS('width', '300px');
  });

  test('clamps side and bottom splitters after the window shrinks', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await page.setViewportSize({ width: 860, height: 600 });

    await dragBy(page, 'sidebar-splitter', 400);
    await expect(page.getByTestId('repo-sidebar')).toHaveCSS('width', '260px');

    await dragBy(page, 'right-splitter', 70);
    await expect(page.getByTestId('repo-right')).toHaveCSS('width', '250px');
    const statsFit = await page
      .getByTestId('changes-stats-staged')
      .evaluate((element) => element.scrollWidth <= element.clientWidth);
    expect(statsFit).toBe(true);
    const center = await page.getByTestId('repo-7').locator('.repo__center').boundingBox();
    expect(center!.width).toBeGreaterThanOrEqual(280);

    await page.locator('.graph-row').first().click();
    await expect(page.getByTestId('bottom-panel')).toBeVisible();
    await dragBy(page, 'diff-splitter', 0, 30);
    const diffHeight = Number.parseFloat(
      await page.getByTestId('bottom-panel').evaluate((element) => getComputedStyle(element).height)
    );
    expect(diffHeight).toBeLessThan(300);
    expect(diffHeight).toBeGreaterThan(280);

    await expect(page.getByTestId('bottom-file-src/lib.rs')).toBeVisible();
    const listBefore = await page
      .getByTestId('bottom-file-list')
      .evaluate((element) => element.getBoundingClientRect().width);
    await dragBy(page, 'bottom-file-splitter', 80);
    const listAfter = await page
      .getByTestId('bottom-file-list')
      .evaluate((element) => element.getBoundingClientRect().width);
    expect(listAfter).toBeGreaterThan(listBefore);
    await expect
      .poll(
        async () => (await stub.commands()).filter((entry) => entry.cmd === 'set_layout').length
      )
      .toBe(4);
  });
});
