import { expect, test } from '@playwright/test';
import { boot } from './harness';
import type { CliStatus } from '../src/bridge/types';

const status: CliStatus = {
  state: 'not-installed',
  path: '/home/dev/.local/bin/agit',
  applicationPath: null,
  shell: { state: 'unknown', shell: null, path: null, reason: 'shell-not-probed' },
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

test('CLI settings distinguishes application PATH from an available user shell', async ({
  page
}) => {
  await boot(page, {
    window: 'settings',
    cliStatus: {
      ...status,
      state: 'not-on-path',
      pathCommand: 'fish_add_path ~/.local/bin',
      shell: {
        state: 'available',
        shell: 'zsh',
        path: '/home/dev/.local/bin/agit',
        reason: null
      }
    }
  });
  await expect(page.getByTestId('cli-status')).toContainText('Available in your user shell');
  await expect(page.getByTestId('cli-copy-path')).toHaveCount(0);
  await expect(page.getByTestId('cli-install')).toHaveCount(0);
});

test('CLI settings explains shell misses, conflicts, and unknown probes', async ({ page }) => {
  await boot(page, {
    window: 'settings',
    cliStatus: {
      ...status,
      state: 'not-on-path',
      pathCommand: 'fish_add_path ~/.local/bin',
      shell: { state: 'not-found', shell: 'zsh', path: null, reason: null }
    }
  });
  await expect(page.getByTestId('settings-cli')).toContainText('Your user shell did not find agit');
  await expect(page.getByTestId('cli-copy-path')).toBeVisible();
});

test('CLI settings shows a shell conflict without install actions', async ({ page }) => {
  await boot(page, {
    window: 'settings',
    cliStatus: {
      ...status,
      shell: {
        state: 'conflict',
        shell: 'zsh',
        path: '/usr/local/bin/agit',
        reason: null
      }
    }
  });
  await expect(page.getByTestId('cli-shell-conflict')).toContainText('/usr/local/bin/agit');
  await expect(page.getByTestId('cli-install')).toHaveCount(0);
  await expect(page.getByTestId('cli-copy-path')).toHaveCount(0);
});

test('CLI settings labels an unavailable shell probe as unknown', async ({ page }) => {
  await boot(page, {
    window: 'settings',
    cliStatus: {
      ...status,
      shell: { state: 'unknown', shell: 'zsh', path: null, reason: 'shell-timeout' }
    }
  });
  await expect(page.getByTestId('settings-cli')).toContainText(
    'Could not confirm the PATH from your user shell'
  );
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
