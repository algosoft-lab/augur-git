import { expect, test } from '@playwright/test';

import { boot } from './harness';
import { fixtureRepo } from './fixtures/stubBackend';

test('Git LFS settings show the system client and persist download choices', async ({ page }) => {
  const stub = await boot(page, { window: 'settings', settingsSection: 'lfs' });

  await expect(page.getByTestId('settings-field-lfs-environment')).toBeVisible();
  await expect(page.getByTestId('settings-field-lfs-environment')).toContainText('3.6.1');
  await expect(page.getByTestId('settings-field-lfs-environment')).toContainText('/usr/local/bin/git-lfs');
  await expect(page.getByTestId('settings-lfs-auto-download')).toBeChecked();
  await expect(page.getByTestId('settings-lfs-download-on-open')).not.toBeChecked();

  await page.getByTestId('settings-lfs-auto-download').uncheck();
  await page.getByTestId('settings-lfs-download-on-open').check();

  const preferences = (await stub.commands()).filter(
    (entry) => entry.cmd === 'set_lfs_download_preferences'
  );
  expect(preferences.map((entry) => entry.args)).toEqual([
    { automatic: false, onOpen: false },
    { automatic: false, onOpen: true }
  ]);
});

test('clone keeps the default LFS download choice and exposes partial recovery', async ({ page }) => {
  const stub = await boot(page, { cloneLfsError: 'The LFS server rejected the transfer.' });

  await page.getByTestId('welcome-clone').click();
  const dialog = page.getByTestId('lfs-clone-dialog');
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId('lfs-clone-download-lfs')).toBeChecked();

  await page.getByTestId('lfs-clone-remote').fill('https://example.com/team/assets.git');
  await page.getByTestId('lfs-clone-parent').fill('/tmp/checkouts');
  await expect(page.getByTestId('lfs-clone-directory')).toHaveValue('assets');
  await page.getByTestId('lfs-clone-start').click();

  await expect(dialog).toContainText('The LFS server rejected the transfer.');
  await expect(page.getByRole('button', { name: 'Open repository' })).toBeVisible();
  const clone = (await stub.commands()).find((entry) => entry.cmd === 'lfs_clone');
  expect(clone?.args).toMatchObject({
    remote: 'https://example.com/team/assets.git',
    parent: '/tmp/checkouts',
    directoryName: 'assets',
    downloadLfs: true
  });
});

test('repository LFS management lists pointer metadata and downloads one file', async ({ page }) => {
  const stub = await boot(page, { open: [fixtureRepo()] });

  await page.getByTestId('toolbar-more').click();
  await page.getByTestId('more-menu-manage-lfs').click();
  const dialog = page.getByTestId('lfs-dialog');
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId('lfs-files-list')).toContainText('assets/model.psd');
  await expect(page.getByTestId('lfs-files-list')).toContainText('aaaaaaaaaaaaaaaa');

  await page.getByRole('button', { name: 'Download', exact: true }).click();

  await expect(page.getByTestId('lfs-files-list')).toContainText('Available in the working tree');
  const download = (await stub.commands()).find((entry) => entry.cmd === 'lfs_download_file');
  expect(download?.args).toMatchObject({ repoId: 7, path: 'assets/model.psd' });

  await page.getByPlaceholder('Full ref, such as refs/heads/main').fill('refs/heads/main');
  await page.getByRole('button', { name: 'Download reference' }).click();
  const selectedReference = (await stub.commands()).find(
    (entry) => entry.cmd === 'lfs_fetch' && entry.args.reference === 'refs/heads/main'
  );
  expect(selectedReference?.args).toMatchObject({ repoId: 7, remote: 'origin' });

  await page.getByRole('button', { name: 'Download full history' }).click();
  const fullHistory = (await stub.commands()).find(
    (entry) => entry.cmd === 'lfs_fetch' && entry.args.allHistory === true
  );
  expect(fullHistory?.args).toMatchObject({ repoId: 7, allHistory: true });

  await page.getByRole('button', { name: 'Check local LFS objects' }).click();
  await expect(dialog).toContainText('Git LFS fsck OK');
});
