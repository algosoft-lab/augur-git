import { expect, test } from '@playwright/test';
import { boot, fixtureRepo } from './harness';

test.describe('remote management', () => {
  test('adds, edits, and removes remotes from the More menu', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('toolbar-more').click();
    await page.getByTestId('more-menu-manage-remotes').click();

    await expect(page.getByTestId('manage-remotes-dialog')).toContainText('origin');
    await expect(page.getByTestId('manage-remotes-dialog')).toContainText(
      'https://example.com/augur-git.git'
    );

    await page.getByTestId('remote-name-input').fill('upstream');
    await page.getByTestId('remote-url-input').fill('https://example.com/upstream.git');
    await page.getByTestId('remote-submit').click();

    // The add is reported as done and the new remote joins the list. The form
    // is empty again, so the name that was just added is not read as a
    // duplicate of itself.
    await expect(page.getByTestId('remotes-notice')).toContainText('upstream');
    await expect(page.getByTestId('remotes-list')).toContainText('upstream');
    await expect(page.getByTestId('remote-name-input')).toHaveValue('');
    await expect(page.getByTestId('remote-name-error')).toHaveCount(0);

    await page.getByTestId('remote-edit-origin').click();
    await page.getByTestId('remote-url-input').fill('git@example.com:augur-git.git');
    await page.getByTestId('remote-submit').click();
    await expect(page.getByTestId('remotes-notice')).toContainText('origin');
    await expect(page.getByTestId('remotes-list')).toContainText('git@example.com:augur-git.git');

    await page.getByTestId('remote-remove-origin').click();
    await expect(page.getByTestId('remote-remove-confirm')).toBeVisible();
    await page.getByTestId('remote-remove-confirm').click();
    await expect(page.getByTestId('manage-remotes-dialog')).toBeVisible();
    await expect(page.getByTestId('remotes-list')).not.toContainText('origin');

    const actions = (await stub.commands())
      .filter((entry) => entry.cmd === 'run_action')
      .map((entry) => (entry.args as any).action);
    expect(actions).toEqual([
      { action: 'remoteAdd', name: 'upstream', url: 'https://example.com/upstream.git' },
      { action: 'remoteSetUrl', name: 'origin', url: 'git@example.com:augur-git.git' },
      { action: 'remoteRemove', name: 'origin' }
    ]);
  });

  test('requires a unique valid name and a non-empty URL before adding', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await page.getByTestId('toolbar-more').click();
    await page.getByTestId('more-menu-manage-remotes').click();

    await page.getByTestId('remote-name-input').fill('origin');
    await expect(page.getByTestId('remote-name-error')).toContainText('already exists');
    await expect(page.getByTestId('remote-submit')).toBeDisabled();

    await page.getByTestId('remote-name-input').fill('upstream');
    await page.getByTestId('remote-url-input').fill('   ');
    await expect(page.getByTestId('remote-url-error')).toBeVisible();
    await expect(page.getByTestId('remote-submit')).toBeDisabled();

    await page.getByTestId('remote-name-input').fill('-invalid');
    await expect(page.getByTestId('remote-name-error')).toBeVisible();
  });

  test('reports a failed add instead of claiming success', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], failingActions: ['remoteAdd'] });
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await page.getByTestId('toolbar-more').click();
    await page.getByTestId('more-menu-manage-remotes').click();

    await page.getByTestId('remote-name-input').fill('upstream');
    await page.getByTestId('remote-url-input').fill('https://example.com/upstream.git');
    await page.getByTestId('remote-submit').click();

    await expect(page.getByTestId('remotes-notice')).toContainText('could not read from remote');
    await expect(page.getByTestId('remote-name-input')).toHaveValue('upstream');
  });

  test('lists an empty repository as having no remotes', async ({ page }) => {
    const repo = fixtureRepo();
    repo.refs.remotes = [];
    repo.refs.remote_urls = [];
    await boot(page, { open: [repo] });
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await page.getByTestId('toolbar-more').click();
    await page.getByTestId('more-menu-manage-remotes').click();

    await expect(page.getByTestId('remotes-empty')).toBeVisible();
    await expect(page.getByTestId('remote-name-input')).toBeVisible();
  });

  test('reads the same in a light theme', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], theme: 'github-light-default' });
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await page.getByTestId('toolbar-more').click();
    await page.getByTestId('more-menu-manage-remotes').click();

    await expect(page.locator('html')).toHaveAttribute('data-mode', 'light');
    await expect(page.getByTestId('remotes-list')).toContainText('origin');
    await expect(page.getByTestId('remote-name-input')).toBeVisible();
  });
});
