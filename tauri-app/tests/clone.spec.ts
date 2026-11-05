import { expect, test } from '@playwright/test';

import { boot } from './harness';
import { fixtureRepo } from './fixtures/stubBackend';

test('the more menu starts a clone and asks one question per step', async ({ page }) => {
  const stub = await boot(page, { open: [fixtureRepo()] });

  await page.getByTestId('toolbar-more').click();
  await page.getByTestId('more-menu-clone').click();

  const dialog = page.getByTestId('lfs-clone-dialog');
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId('lfs-clone-remote')).toBeVisible();
  await expect(page.getByTestId('lfs-clone-continue')).toBeDisabled();

  await page.getByTestId('lfs-clone-remote').fill('https://gitlab.example.com/team/api.git');
  await page.getByTestId('lfs-clone-continue').click();

  // The second page carries the repository it will clone from the first.
  await expect(dialog).toContainText('https://gitlab.example.com/team/api.git');
  const destination = page.getByTestId('lfs-clone-destination');
  await expect(destination).toHaveAttribute('placeholder', '/home/you/projects/api');

  // A bare name cannot name a folder, and the reason is spelled out.
  await destination.fill('api');
  await expect(page.getByTestId('clone-destination-error')).toContainText('absolute path');
  await expect(page.getByTestId('lfs-clone-start')).toBeDisabled();

  await destination.fill('/home/dev/checkouts/api');
  await expect(page.getByTestId('clone-destination-error')).toHaveCount(0);
  await page.getByTestId('lfs-clone-start').click();

  await expect(dialog).toHaveCount(0);
  const clone = (await stub.commands()).find((entry) => entry.cmd === 'lfs_clone');
  expect(clone?.args).toMatchObject({
    remote: 'https://gitlab.example.com/team/api.git',
    parent: '/home/dev/checkouts',
    directoryName: 'api'
  });
  const opened = (await stub.commands()).find((entry) => entry.cmd === 'open_repository');
  expect(opened?.args).toMatchObject({ path: '/home/dev/checkouts/api' });
});

test('the clone wizard answers a credential prompt in the same card', async ({ page }) => {
  await boot(page, { open: [fixtureRepo()] });

  await page.getByTestId('toolbar-more').click();
  await page.getByTestId('more-menu-clone').click();
  await page.getByTestId('lfs-clone-remote').fill('https://gitlab.example.com/team/api.git');

  await page.evaluate(() =>
    window.__STUB__.emit('augur://lfs-credential-request', {
      requestId: 'login-1',
      kind: 'login',
      prompt: '',
      protocol: 'https',
      host: 'gitlab.example.com',
      username: 'dev',
      repository: '/home/dev/checkouts/api',
      canPersist: true,
      keychainAvailable: true,
      plaintextAvailable: false,
      defaultStorage: 'keychain',
      stored: false
    })
  );

  const dialog = page.getByTestId('lfs-clone-dialog');
  await expect(page.getByTestId('clone-credentials')).toBeVisible();
  await expect(dialog.getByTestId('credential-username')).toHaveValue('dev');
  // The standalone credential card stays out of the way while the wizard is open.
  await expect(page.getByTestId('credential-dialog')).toHaveCount(0);

  // Closing the wizard hands the unanswered prompt back to the standalone card.
  await page.getByTestId('lfs-clone-dialog-overlay').click({ position: { x: 4, y: 4 } });
  await expect(dialog).toHaveCount(0);
  await expect(page.getByTestId('credential-dialog')).toBeVisible();
  await expect(page.getByTestId('credential-username')).toHaveValue('dev');
});
