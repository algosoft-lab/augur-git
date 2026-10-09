import { boot, expect, fixtureRepo, test } from './harness';

test.describe('command palette', () => {
  test('opens from an input, fuzzy-filters commands, and runs the selected action', async ({
    page
  }) => {
    const repo = fixtureRepo();
    repo.status.files = repo.status.files.filter((file) => file.path !== 'src/conflict.rs');
    await boot(page, { open: [repo] });

    const commitMessage = page.getByTestId('commit-message');
    await commitMessage.focus();
    await page.keyboard.press('Control+p');

    const palette = page.getByTestId('palette');
    const input = page.getByTestId('palette-input');
    await expect(palette).toBeVisible();
    await expect(input).toBeFocused();
    await input.fill('new brnch');
    const newBranch = page.getByTestId('palette-item').filter({ hasText: 'New Branch' });
    await expect(newBranch).toBeVisible();
    await expect(newBranch).toBeEnabled();
    await page.keyboard.press('Enter');

    const branchDialog = page.getByTestId('branch-dialog');
    await expect(branchDialog).toBeVisible();
    await expect(palette).toBeHidden();
    await page.keyboard.press('Control+p');
    await expect(branchDialog).toBeVisible();
    await expect(palette).toBeHidden();
  });

  test('shows unavailable commands disabled and closes with Escape', async ({ page }) => {
    await boot(page);
    await page.keyboard.press('Control+p');

    const palette = page.getByTestId('palette');
    await expect(palette).toBeVisible();
    const input = page.getByTestId('palette-input');
    await input.fill('pull');
    const pull = page.getByTestId('palette-item').filter({ hasText: 'Pull' });
    await expect(pull).toBeVisible();
    await expect(pull).toBeDisabled();
    await page.keyboard.press('Enter');
    await expect(palette).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(palette).toBeHidden();
  });
});
