import { expect, test } from '@playwright/test';
import { boot } from './harness';
import type { CliStatus } from '../src/bridge/types';

const status: CliStatus = {
  state: 'not-installed',
  path: '/home/dev/.local/bin/agit',
  canInstall: true,
  canRemove: false,
  packageManaged: false,
  pathCommand: null,
  detail: ''
};

test('CLI settings installs, refreshes and uninstalls the command', async ({ page }) => {
  const stub = await boot(page, { window: 'settings', cliStatus: status });
  await expect(page.getByTestId('cli-status')).toHaveText('Not installed');
  await page.getByTestId('cli-install').click();
  await expect(page.getByTestId('cli-status')).toHaveText('Available');
  await page.getByTestId('cli-refresh').click();
  await page.getByTestId('cli-uninstall').click();
  await expect(page.getByTestId('cli-status')).toHaveText('Not installed');
  expect(await stub.commandNames()).toEqual(
    expect.arrayContaining(['install_cli', 'uninstall_cli', 'get_cli_status'])
  );
});

test('CLI settings offers repair and explains PATH setup', async ({ page }) => {
  await boot(page, {
    window: 'settings',
    cliStatus: {
      ...status,
      state: 'broken',
      canRemove: true,
      pathCommand: 'fish_add_path ~/.local/bin'
    }
  });
  await expect(page.getByTestId('cli-install')).toHaveText('Repair agit');
  await expect(page.getByTestId('cli-install')).toHaveClass(/cli-settings__button--primary/);
  await expect(page.getByTestId('cli-uninstall')).toHaveClass(/cli-settings__button--danger/);
  await expect(page.getByTestId('settings-cli')).toContainText('fish_add_path ~/.local/bin');
  await page.getByTestId('cli-install').focus();
  await expect(page.getByTestId('cli-install')).toBeFocused();
  await expect(page.getByTestId('cli-install')).toHaveCSS('outline-style', 'solid');
});

test('CLI settings copies the PATH command and shows feedback', async ({ page }) => {
  const stub = await boot(page, {
    window: 'settings',
    cliStatus: { ...status, pathCommand: 'fish_add_path ~/.local/bin' }
  });
  await page.getByTestId('cli-copy-path').click();
  await expect.poll(() => stub.clipboard()).toBe('fish_add_path ~/.local/bin');
  await expect(page.getByTestId('settings-cli').locator('[role="status"]')).toHaveText('✓');
});

for (const state of ['conflict', 'available'] as const) {
  test(`CLI settings protects ${state === 'conflict' ? 'foreign' : 'package-managed'} commands`, async ({
    page
  }) => {
    await boot(page, {
      window: 'settings',
      cliStatus: {
        ...status,
        state,
        canInstall: false,
        packageManaged: state === 'available'
      }
    });
    await expect(page.getByTestId('cli-status')).toBeVisible();
    await expect(page.getByTestId('cli-install')).toHaveCount(0);
    await expect(page.getByTestId('cli-uninstall')).toHaveCount(0);
  });
}
