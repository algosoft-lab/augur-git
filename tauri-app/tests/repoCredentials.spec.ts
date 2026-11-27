import { expect, test } from '@playwright/test';

import { boot, fixtureRepo } from './harness';

test('per-repository credential modal manages repository-scoped vault accounts from More menu', async ({
  page
}) => {
  const stub = await boot(page, { open: [fixtureRepo()] });
  await expect(page.getByTestId('repo-7')).toBeVisible();

  await page.getByTestId('toolbar-more').click();
  await page.getByTestId('more-menu-repo-credentials').click();

  const dialog = page.getByTestId('repo-credentials-dialog');
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId('repo-creds-remote-url')).toContainText(
    'https://example.com/augur-git.git'
  );
  await expect(page.getByTestId('repo-creds-conflict-banner')).toBeVisible();
  await expect(page.getByTestId('repo-creds-vault-empty')).toBeVisible();

  // Add a repository-scoped vault account pre-filled from origin.
  await page.getByTestId('repo-creds-vault-add').click();
  await expect(page.getByTestId('repo-creds-vault-host')).toHaveValue('example.com');
  await expect(page.getByTestId('repo-creds-vault-path')).toHaveValue('augur-git.git');
  await page.getByTestId('repo-creds-vault-username').fill('special_user');
  await page.getByTestId('repo-creds-vault-password').fill('s3cret-token');
  await page.getByTestId('repo-creds-vault-save').click();

  await expect(page.getByTestId('repo-creds-vault-list')).toContainText('special_user');
  await expect(page.getByTestId('repo-creds-status-badge')).toContainText('special_user');
  await expect(page.getByTestId('repo-creds-conflict-banner')).toHaveCount(0);

  // Edit and delete the repository-scoped entry.
  await page.getByTestId('repo-creds-vault-edit-cred-1').click();
  await page.getByTestId('repo-creds-vault-username').fill('special_user_2');
  await page.getByTestId('repo-creds-vault-save').click();
  await expect(page.getByTestId('repo-creds-vault-list')).toContainText('special_user_2');

  await page.getByTestId('repo-creds-vault-delete-cred-1').click();
  await page.getByTestId('repo-creds-vault-delete-confirm').click();
  await expect(page.getByTestId('repo-creds-vault-empty')).toBeVisible();

  expect(await stub.commandNames()).toEqual(
    expect.arrayContaining(['repo_credential_snapshot', 'save_credential', 'delete_credential'])
  );
});

test('per-repository credential modal configures local .git/config strategies and runs verification', async ({
  page
}) => {
  const stub = await boot(page, { open: [fixtureRepo()] });
  await expect(page.getByTestId('repo-7')).toBeVisible();

  await page.getByTestId('toolbar-more').click();
  await page.getByTestId('more-menu-repo-credentials').click();
  await expect(page.getByTestId('repo-credentials-dialog')).toBeVisible();

  await page.getByTestId('repo-creds-tab-local').click();
  await expect(page.getByTestId('repo-creds-helpers-list')).toContainText('!gh auth git-credential');

  // Switch to Strategy 2 (isolated-store) and configure local identity + remote URL username.
  await page.getByTestId('repo-creds-mode').click();
  await page.getByTestId('select-option-strategy-2:-reset-global-helpers-+-dedicated-store-file').click();

  await page.getByTestId('repo-creds-local-username').fill('special_user');
  await page.getByTestId('repo-creds-store-file').fill('/home/dev/.git-credentials-special');
  await page.getByTestId('repo-creds-store-password').fill('token-123');
  await page.getByTestId('repo-creds-remote-url-action').click();
  await page
    .getByTestId('select-option-write-username-into-remote-url-(https://user@host/…)')
    .click();
  await expect(page.getByTestId('repo-creds-remote-url-preview')).toContainText(
    'https://special_user@example.com/augur-git.git'
  );
  await page.getByTestId('repo-creds-user-name').fill('Special Bot');
  await page.getByTestId('repo-creds-user-email').fill('bot@example.com');
  await page.getByTestId('repo-creds-local-save').click();

  await expect(page.getByTestId('repo-creds-notice')).toBeVisible();
  await expect(page.getByTestId('repo-creds-helpers-list')).toContainText(
    'store --file=/home/dev/.git-credentials-special'
  );
  await expect(page.getByTestId('repo-creds-remote-url')).toContainText(
    'https://special_user@example.com/augur-git.git'
  );

  // Verify credential resolution and test remote connectivity.
  await page.getByTestId('repo-creds-verify').click();
  await expect(page.getByTestId('repo-creds-verify-result')).toContainText('special_user');
  await expect(page.getByTestId('repo-creds-verify-result')).toContainText(
    'Global helpers isolated'
  );

  await page.getByTestId('repo-creds-test-remote').click();
  await expect(page.getByTestId('repo-creds-test-result')).toContainText(
    'The remote accepted this credential.'
  );

  const savedCalls = (await stub.commands()).filter(
    (entry) => entry.cmd === 'save_repo_credential_config'
  );
  expect(savedCalls).toHaveLength(1);
  expect(savedCalls[0]?.args).toMatchObject({
    repoId: 7,
    input: {
      mode: 'isolated-store',
      useHttpPath: true,
      username: 'special_user',
      storeFile: '/home/dev/.git-credentials-special',
      storePassword: 'token-123',
      remoteUrlUsernameAction: 'embed',
      localUserName: 'Special Bot',
      localUserEmail: 'bot@example.com'
    }
  });
});
