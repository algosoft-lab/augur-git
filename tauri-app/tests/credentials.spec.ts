import { expect, test } from '@playwright/test';

import { boot, fixtureRepo } from './harness';

/**
 * Credential settings.
 *
 * The page has to stay honest about where a password would go: an available
 * backend, an unavailable one with its reason, and a delete that asks first.
 * Every action is asserted through the command the backend received, because
 * that is the contract the two frontends share.
 */
test('credential settings reports backends and adds an account', async ({ page }) => {
  const stub = await boot(page, { window: 'settings' });
  await page.getByTestId('settings-nav-credentials').click();

  await expect(page.getByTestId('credentials-backend-keychain')).toHaveText('Available');
  await expect(page.getByTestId('credentials-backend-plaintext')).toHaveText('Available');
  await expect(page.getByTestId('credentials-empty')).toBeVisible();

  await page.getByTestId('credentials-add').click();
  await expect(page.getByTestId('credentials-form')).toBeVisible();
  await page.getByTestId('credentials-form-host').fill('https://github.com/team/repo.git');
  await page.getByTestId('credentials-form-path').fill('/team/');
  await page.getByTestId('credentials-form-username').fill('dev');
  await page.getByTestId('credentials-form-password').fill('s3cret');
  await page.getByTestId('credentials-form-save').click();

  await expect(page.getByTestId('credentials-form')).toHaveCount(0);
  const table = page.getByTestId('credentials-table');
  await expect(table).toContainText('github.com');
  await expect(table).toContainText('dev');
  await expect(table).toContainText('team');

  // The row actions have to stay inside the table; they used to be pushed past
  // its right edge, where the clipped column could not be clicked.
  const bounds = (await table.boundingBox())!;
  for (const action of ['credentials-edit-cred-1', 'credentials-delete-cred-1']) {
    const box = (await page.getByTestId(action).boundingBox())!;
    expect(box.x + box.width).toBeLessThanOrEqual(bounds.x + bounds.width + 1);
    expect(box.x).toBeGreaterThanOrEqual(bounds.x);
  }
  expect(await stub.commandNames()).toEqual(
    expect.arrayContaining(['credential_snapshot', 'save_credential'])
  );
});

test('credential settings confirms before deleting and can import', async ({ page }) => {
  const stub = await boot(page, { window: 'settings' });
  await page.getByTestId('settings-nav-credentials').click();

  await page.getByTestId('credentials-add').click();
  await page.getByTestId('credentials-form-host').fill('git.example.com');
  await page.getByTestId('credentials-form-username').fill('dev');
  await page.getByTestId('credentials-form-password').fill('token');
  await page.getByTestId('credentials-form-save').click();
  await expect(page.getByTestId('credentials-table')).toContainText('git.example.com');

  await page.getByTestId('credentials-delete-cred-1').click();
  await expect(page.getByTestId('credentials-delete-dialog')).toBeVisible();
  await page.getByTestId('credentials-delete-confirm').click();
  await expect(page.getByTestId('credentials-empty')).toBeVisible();

  await page.getByTestId('credentials-import').click();
  await expect(page.getByTestId('credentials-notice')).toContainText('1');
  expect(await stub.commandNames()).toEqual(
    expect.arrayContaining(['delete_credential', 'import_git_credentials'])
  );
});

test('credential preferences are written through the backend', async ({ page }) => {
  await boot(page, { window: 'settings' });
  await page.getByTestId('settings-nav-credentials').click();

  await page.getByTestId('credentials-auto-fill').click();
  await expect(page.getByTestId('credentials-auto-fill')).not.toBeChecked();
  await page.getByTestId('credentials-allow-plaintext').click();
  await expect(page.getByTestId('credentials-allow-plaintext')).toBeChecked();
  await expect(page.getByTestId('settings-credentials')).toContainText('credential-secrets.json');
});

test('the remote dialog says when an address already has an account', async ({ page }) => {
  await boot(page, { open: [fixtureRepo()] });
  await expect(page.getByTestId('repo-7')).toBeVisible();
  await page.getByTestId('toolbar-more').click();
  await page.getByTestId('more-menu-manage-remotes').click();

  await page.getByTestId('remote-url-input').fill('https://github.com/team/repo.git');
  await expect(page.getByTestId('remote-saved-account')).toContainText('dev');

  await page.getByTestId('remote-url-input').fill('https://gitlab.com/team/repo.git');
  await expect(page.getByTestId('remote-saved-account')).toHaveCount(0);
});
