import { expect, test } from '@playwright/test';

import { boot } from './harness';
import { fixtureRepo } from './fixtures/stubBackend';

test('the Git settings section loads, edits and saves the global configuration', async ({
  page
}) => {
  const stub = await boot(page, {
    window: 'settings',
    settingsSection: 'git',
    gitConfig: { 'user.name': 'Ada Lovelace' }
  });

  await expect(page.getByTestId('settings-git')).toBeVisible();
  const name = page.getByTestId('git-user-name');
  const email = page.getByTestId('git-user-email');
  await expect(name).toHaveValue('Ada Lovelace');
  await expect(email).toHaveValue('');

  // Nothing changed yet: save stays disabled.
  await expect(page.getByTestId('git-config-save')).toBeDisabled();

  // A broken email disables saving and names the problem.
  await email.fill('not-an-email');
  await expect(page.getByTestId('git-config-save')).toBeDisabled();
  await expect(page.getByTestId('git-user-email-error')).toBeVisible();

  await email.fill('ada@example.com');
  await page.getByTestId('git-config-save').click();
  await expect(page.getByTestId('git-config-saved')).toBeVisible();

  const written = (await stub.commands()).filter((entry) => entry.cmd === 'write_git_config');
  expect(written).toHaveLength(1);
  expect(written[0]?.args.entries).toEqual([{ key: 'user.email', value: 'ada@example.com' }]);
});

test('search finds the Git identity settings', async ({ page }) => {
  await boot(page, { window: 'settings' });
  const search = page.getByTestId('settings-search');
  await search.fill('user email');
  await page.getByTestId('settings-search-result-settings-field-git-user-email').click();
  await expect(page.getByTestId('settings-git')).toBeVisible();
  await expect(page.getByTestId('settings-field-git-user-email')).toHaveClass(/is-flash/);
});

test('a commit without an identity opens the dialog and saving retries it', async ({ page }) => {
  const stub = await boot(page, {
    open: [fixtureRepo()],
    identity: { name: null, email: null }
  });

  // The fixture repository already has staged changes.
  await page.getByTestId('commit-message').fill('Add the Tauri command surface');
  await page.getByTestId('commit-submit').click();

  const dialog = page.getByTestId('git-identity-dialog');
  await expect(dialog).toBeVisible();
  // No commit was dispatched while the identity is missing.
  const commitCalls = (await stub.commands()).filter(
    (entry) =>
      entry.cmd === 'run_action' && (entry.args.action as { action?: string })?.action === 'commit'
  );
  expect(commitCalls).toHaveLength(0);

  await page.getByTestId('git-identity-name').fill('Ada Lovelace');
  await page.getByTestId('git-identity-email').fill('ada@example.com');
  await page.getByTestId('git-identity-save').click();

  await expect(dialog).toHaveCount(0);
  const commands = await stub.commands();
  const written = commands.find((entry) => entry.cmd === 'write_git_config');
  expect(written?.args.entries).toEqual([
    { key: 'user.name', value: 'Ada Lovelace' },
    { key: 'user.email', value: 'ada@example.com' }
  ]);
  const retried = commands.filter(
    (entry) =>
      entry.cmd === 'run_action' && (entry.args.action as { action?: string })?.action === 'commit'
  );
  expect(retried).toHaveLength(1);
  expect((retried[0]?.args.action as { message?: string })?.message).toBe(
    'Add the Tauri command surface'
  );
});

test('a failed commit whose Git output names the identity opens the same dialog', async ({
  page
}) => {
  const stub = await boot(page, {
    open: [fixtureRepo()],
    // The probe passes, so only the command's own output can reveal the cause.
    identity: { name: 'Ada Lovelace', email: 'ada@example.com' }
  });

  await page.getByTestId('commit-message').fill('Add the Tauri command surface');
  await page.getByTestId('commit-submit').click();

  // The stub's commit succeeds generically; emit the commandDone a real Git
  // refusal would carry, to exercise the fallback that reads the message.
  await stub.emit('augur://repo-event', {
    repoId: 7,
    type: 'commandDone',
    label: 'commit',
    success: false,
    message:
      'Author identity unknown\n\n*** Please tell me who you are.\n\nRun\n\n  git config --global user.email "you@example.com"\n  git config --global user.name "Your Name"'
  });

  await expect(page.getByTestId('git-identity-dialog')).toBeVisible();
  await expect(page.getByTestId('git-identity-name')).toHaveValue('Ada Lovelace');
  await expect(page.getByTestId('git-identity-email')).toHaveValue('ada@example.com');
});

test('an unrelated commit failure leaves the interface alone', async ({ page }) => {
  const stub = await boot(page, { open: [fixtureRepo()], failingActions: ['commit'] });

  await page.getByTestId('commit-message').fill('Add the Tauri command surface');
  await page.getByTestId('commit-submit').click();

  await expect(page.getByTestId('status-message')).toBeVisible();
  await expect(page.getByTestId('git-identity-dialog')).toHaveCount(0);
  const commitCalls = (await stub.commands()).filter(
    (entry) =>
      entry.cmd === 'run_action' && (entry.args.action as { action?: string })?.action === 'commit'
  );
  expect(commitCalls).toHaveLength(1);
});
