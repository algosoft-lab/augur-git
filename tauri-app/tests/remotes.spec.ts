import { expect, test } from '@playwright/test';
import { boot, fixtureRepo, rightClick } from './harness';

/** A repository with two remotes and a clean working tree. */
function multiRemoteRepo() {
  const repo = fixtureRepo();
  repo.refs.remotes = ['origin', 'upstream'];
  repo.refs.remote_urls = [
    { name: 'origin', url: 'https://example.com/augur-git.git' },
    { name: 'upstream', url: 'https://example.com/upstream.git' }
  ];
  repo.refs.remote_branches = ['origin/master', 'origin/feature/tauri', 'upstream/main'];
  // A conflicted file would refuse every pull, which is a different test.
  repo.status.files = repo.status.files.filter((file) => file.index !== 'U');
  return repo;
}

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
      {
        action: 'remoteSetUrl',
        name: 'origin',
        url: 'git@example.com:augur-git.git',
        oldUrl: 'https://example.com/augur-git.git'
      },
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

test.describe('multi-remote operations', () => {
  test('fetches one remote from its group header', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="sidebar-toggle-remote-upstream"]');
    await expect(page.getByTestId('context-fetch-remote')).toContainText('upstream');
    await expect(page.getByTestId('context-pull-remote')).toBeVisible();
    await expect(page.getByTestId('context-push-remote')).toBeVisible();
    await expect(page.getByTestId('context-edit-remote-url')).toBeVisible();
    await expect(page.getByTestId('context-remove-remote')).toBeVisible();
    await expect(page.getByTestId('context-manage-remotes')).toBeVisible();

    await page.getByTestId('context-fetch-remote').click();

    const actions = (await stub.commands())
      .filter((entry) => entry.cmd === 'run_action')
      .map((entry) => (entry.args as any).action);
    expect(actions).toEqual([{ action: 'fetchRemote', remote: 'upstream' }]);
  });

  test('fetches one remote from the button beside its header', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('remote-fetch-origin').click();

    const actions = (await stub.commands())
      .filter((entry) => entry.cmd === 'run_action')
      .map((entry) => (entry.args as any).action);
    expect(actions).toEqual([{ action: 'fetchRemote', remote: 'origin' }]);
  });

  test('pulls and pushes one remote branch from its row', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="remote-branch-origin/master"]');
    await expect(page.getByTestId('context-fetch-remote-branch')).toBeVisible();
    await expect(page.getByTestId('context-pull-remote-branch')).toContainText('origin/master');
    await expect(page.getByTestId('context-push-remote-branch')).toContainText('origin/master');
    // The ref operations are still there, behind the network ones.
    await expect(page.getByTestId('context-checkout')).toBeVisible();
    await expect(page.getByTestId('context-copy-branch')).toBeVisible();

    await page.getByTestId('context-pull-remote-branch').click();
    await expect
      .poll(async () =>
        (await stub.commands())
          .filter((entry) => entry.cmd === 'run_action')
          .map((entry) => (entry.args as any).action.action)
      )
      .toEqual(['pullFrom']);
  });

  test('checks the repository before a pull that rebases', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="remote-branch-origin/master"]');
    await page.getByTestId('context-pull-remote-branch-rebase').click();

    await expect
      .poll(async () =>
        (await stub.commands())
          .filter((entry) => entry.cmd === 'run_action')
          .map((entry) => (entry.args as any).action)
      )
      .toEqual([{ action: 'pullFrom', remote: 'origin', branch: 'master', rebase: true }]);
    // The preflight probe runs before the pull.
    expect(await stub.commandNames()).toContain('probe_rebase');
  });

  test('asks before publishing the current branch to another remote', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="sidebar-toggle-remote-upstream"]');
    await page.getByTestId('context-push-remote').click();

    await expect(page.getByTestId('push-upstream-dialog')).toBeVisible();
    await expect(page.getByTestId('push-upstream-dialog')).toContainText('upstream');
    await page.getByTestId('push-upstream-confirm').click();

    const actions = (await stub.commands())
      .filter((entry) => entry.cmd === 'run_action')
      .map((entry) => (entry.args as any).action);
    expect(actions).toEqual([{ action: 'pushSetUpstream', remote: 'upstream', branch: 'master' }]);
  });

  test('pushes the current branch to the branch of one remote', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="remote-branch-upstream/main"]');
    await page.getByTestId('context-push-remote-branch').click();

    const actions = (await stub.commands())
      .filter((entry) => entry.cmd === 'run_action')
      .map((entry) => (entry.args as any).action);
    expect(actions).toEqual([
      { action: 'pushTo', remote: 'upstream', branch: 'main', force: false }
    ]);
  });

  test('pushes the current branch to the remote it tracks', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="sidebar-toggle-remote-origin"]');
    await page.getByTestId('context-push-remote').click();

    const actions = (await stub.commands())
      .filter((entry) => entry.cmd === 'run_action')
      .map((entry) => (entry.args as any).action);
    expect(actions).toEqual([
      { action: 'pushTo', remote: 'origin', branch: 'master', force: false }
    ]);
  });

  test('opens the remote settings on the remote the menu named', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="sidebar-toggle-remote-upstream"]');
    await page.getByTestId('context-edit-remote-url').click();

    await expect(page.getByTestId('manage-remotes-dialog')).toBeVisible();
    await expect(page.getByTestId('remote-name-input')).toHaveValue('upstream');
    await expect(page.getByTestId('remote-url-input')).toHaveValue(
      'https://example.com/upstream.git'
    );

    await page.getByTestId('remote-url-input').fill('https://example.com/other.git');
    await page.getByTestId('remote-submit').click();

    const actions = (await stub.commands())
      .filter((entry) => entry.cmd === 'run_action')
      .map((entry) => (entry.args as any).action);
    expect(actions).toEqual([
      {
        action: 'remoteSetUrl',
        name: 'upstream',
        url: 'https://example.com/other.git',
        oldUrl: 'https://example.com/upstream.git'
      }
    ]);
  });

  test('asks before removing the remote the menu named', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="sidebar-toggle-remote-upstream"]');
    await page.getByTestId('context-remove-remote').click();

    await expect(page.getByTestId('manage-remotes-dialog')).toBeVisible();
    await expect(page.getByTestId('remote-remove-confirm-panel')).toContainText('upstream');

    await page.getByTestId('remote-remove-confirm').click();
    const actions = (await stub.commands())
      .filter((entry) => entry.cmd === 'run_action')
      .map((entry) => (entry.args as any).action);
    expect(actions).toEqual([{ action: 'remoteRemove', name: 'upstream' }]);
  });
});

test.describe('remote editing', () => {
  async function openManager(page: import('@playwright/test').Page) {
    await expect(page.getByTestId('repo-7')).toBeVisible();
    await page.getByTestId('toolbar-more').click();
    await page.getByTestId('more-menu-manage-remotes').click();
    await page.getByTestId('remote-edit-origin').click();
  }

  test('renames independently and saves a retained URL draft under the new name', async ({
    page
  }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await openManager(page);
    await expect(page.getByTestId('remote-rename')).toBeDisabled();
    await expect(page.getByTestId('remote-submit')).toBeDisabled();
    await page.getByTestId('remote-name-input').fill('upstream');
    await expect(page.getByTestId('remote-name-error')).toContainText('already exists');
    await expect(page.getByTestId('remote-rename')).toBeDisabled();
    await page.getByTestId('remote-name-input').fill('-invalid');
    await expect(page.getByTestId('remote-rename')).toBeDisabled();
    await page.getByTestId('remote-name-input').fill('primary');
    await page.getByTestId('remote-url-input').fill('git@example.com:Org/New.git');
    await page.getByTestId('remote-rename').click();
    await expect(page.getByTestId('remotes-notice')).toContainText('renamed to primary');
    await expect(page.getByTestId('remote-edit-primary')).toHaveAttribute('aria-current', 'true');
    await expect(page.getByTestId('remote-name-input')).toHaveValue('primary');
    await expect(page.getByTestId('remote-url-input')).toHaveValue('git@example.com:Org/New.git');
    await expect(page.getByTestId('remote-rename')).toBeDisabled();
    await page.getByTestId('remote-submit').click();
    await expect(page.getByTestId('remote-submit')).toBeDisabled();
    const actions = (await stub.commands())
      .filter((entry) => entry.cmd === 'run_action')
      .map((entry) => (entry.args as any).action);
    expect(actions).toEqual([
      { action: 'remoteRename', oldName: 'origin', newName: 'primary' },
      {
        action: 'remoteSetUrl',
        name: 'primary',
        url: 'git@example.com:Org/New.git',
        oldUrl: 'https://example.com/augur-git.git'
      }
    ]);
    await page.getByTestId('manage-remotes-close').click();
    await expect(page.getByTestId('manage-remotes-dialog')).toHaveCount(0);
    await expect(page.getByTestId('sidebar-toggle-remote-primary')).toBeVisible();
  });

  test('converts a draft, guards closing, and writes only after saving', async ({ page }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await openManager(page);
    await page.getByTestId('remote-convert').click();
    await expect(page.getByTestId('remote-url-input')).toHaveValue('git@example.com:augur-git.git');
    expect((await stub.commands()).filter((entry) => entry.cmd === 'run_action')).toHaveLength(0);
    await page.getByTestId('manage-remotes-close').click();
    await expect(page.getByTestId('remote-discard-panel')).toBeVisible();
    await page.getByTestId('remote-keep-editing').click();
    await expect(page.getByTestId('remote-url-input')).toHaveValue('git@example.com:augur-git.git');
    await page.getByTestId('remote-submit').click();
    await expect(page.getByTestId('remotes-notice')).toContainText('updated');
    await page.getByTestId('remote-convert').click();
    await expect(page.getByTestId('remote-url-input')).toHaveValue(
      'https://example.com/augur-git.git'
    );
    await page.keyboard.press('Escape');
    await page.getByTestId('remote-discard').click();
    await expect(page.getByTestId('manage-remotes-dialog')).toHaveCount(0);
    expect((await stub.commands()).filter((entry) => entry.cmd === 'run_action')).toHaveLength(1);
  });

  test('guards switching remotes and preserves independent name edits when saving a URL', async ({
    page
  }) => {
    await boot(page, { open: [multiRemoteRepo()] });
    await openManager(page);
    await page.getByTestId('remote-name-input').fill('primary');
    await page.getByTestId('remote-url-input').fill('https://example.com/new.git');
    await page.getByTestId('remote-submit').click();
    await expect(page.getByTestId('remotes-notice')).toContainText('updated');
    await expect(page.getByTestId('remote-name-input')).toHaveValue('primary');
    await expect(page.getByTestId('remote-rename')).toBeEnabled();
    await page.getByTestId('remote-edit-upstream').click();
    await expect(page.getByTestId('remote-discard-panel')).toBeVisible();
    await page.getByTestId('remote-keep-editing').click();
    await expect(page.getByTestId('remote-name-input')).toHaveValue('primary');
    await page.getByTestId('remote-edit-upstream').click();
    await page.getByTestId('remote-discard').click();
    await expect(page.getByTestId('remote-name-input')).toHaveValue('upstream');
    await expect(page.getByTestId('remote-url-input')).toHaveValue(
      'https://example.com/upstream.git'
    );
  });

  for (const operation of ['remoteRename', 'remoteSetUrl']) {
    test(`preserves drafts when ${operation} fails`, async ({ page }) => {
      await boot(page, { open: [multiRemoteRepo()], failingActions: [operation] });
      await openManager(page);
      await page.getByTestId('remote-name-input').fill('primary');
      await page.getByTestId('remote-url-input').fill('git@example.com:New.git');
      await page
        .getByTestId(operation === 'remoteRename' ? 'remote-rename' : 'remote-submit')
        .click();
      await expect(page.getByTestId('remotes-notice')).toContainText('could not read from remote');
      await expect(page.getByTestId('remote-name-input')).toHaveValue('primary');
      await expect(page.getByTestId('remote-url-input')).toHaveValue('git@example.com:New.git');
      await expect(page.getByTestId('remote-edit-origin')).toHaveAttribute('aria-current', 'true');
      await expect(page.getByTestId('remote-rename')).toBeEnabled();
      await expect(page.getByTestId('remote-submit')).toBeEnabled();
    });
  }

  test('locks selection, closing, conversion, and duplicate submissions while saving', async ({
    page
  }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()], actionDelay: 1200 });
    await openManager(page);
    await page.getByTestId('remote-url-input').fill('https://example.com/New.git');
    await page.getByTestId('remote-submit').click();
    await expect(page.getByTestId('remote-url-input')).toBeDisabled();
    await expect(page.getByTestId('remote-name-input')).toBeDisabled();
    await expect(page.getByTestId('remote-edit-upstream')).toBeDisabled();
    await expect(page.getByTestId('remote-add-another')).toBeDisabled();
    await expect(page.getByTestId('remote-convert')).toBeDisabled();
    await expect(page.getByTestId('manage-remotes-close')).toBeDisabled();
    await expect(page.getByTestId('remote-submit')).toBeDisabled();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('manage-remotes-dialog')).toBeVisible();
    await expect(page.getByTestId('remotes-notice')).toContainText('updated');
    expect((await stub.commands()).filter((entry) => entry.cmd === 'run_action')).toHaveLength(1);
  });

  test('retains the draft and clears pending state when IPC rejects the action', async ({
    page
  }) => {
    await boot(page, {
      open: [multiRemoteRepo()],
      refusals: { run_action: { key: 'err-invalid-action', detail: 'Remote changed externally.' } }
    });
    await openManager(page);
    await page.getByTestId('remote-url-input').fill('https://example.com/new.git');
    await page.getByTestId('remote-submit').click();
    await expect(page.getByTestId('remotes-notice')).toContainText(
      'This operation cannot be run with the values provided.'
    );
    await expect(page.getByTestId('remote-url-input')).toHaveValue('https://example.com/new.git');
    await expect(page.getByTestId('remote-submit')).toBeEnabled();
  });

  test('guards backdrop closing and discards a draft before removal confirmation', async ({
    page
  }) => {
    const stub = await boot(page, { open: [multiRemoteRepo()] });
    await openManager(page);
    await page.getByTestId('remote-url-input').fill('https://example.com/new.git');
    await page.getByTestId('manage-remotes-dialog-overlay').click({ position: { x: 1, y: 1 } });
    await expect(page.getByTestId('remote-discard-panel')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('remote-url-input')).toHaveValue('https://example.com/new.git');
    await page.getByTestId('remote-remove-action').click();
    await page.getByTestId('remote-discard').click();
    await expect(page.getByTestId('remote-remove-confirm-panel')).toBeVisible();
    await page.getByTestId('remote-remove-cancel').click();
    await expect(page.getByTestId('remote-url-input')).toHaveValue(
      'https://example.com/augur-git.git'
    );
    expect((await stub.commands()).filter((entry) => entry.cmd === 'run_action')).toHaveLength(0);
  });

  test('explains unsupported conversion and independent push URLs without blocking manual edits', async ({
    page
  }) => {
    const repo = multiRemoteRepo();
    repo.refs.remote_urls[0]!.has_push_url = true;
    await boot(page, { open: [repo] });
    await openManager(page);
    await expect(page.getByTestId('remote-push-url-hint')).toContainText('separate push URL');
    await page.getByTestId('remote-url-input').fill('https://example.com:8443/Org/Repo.git');
    await expect(page.getByTestId('remote-convert')).toBeDisabled();
    await expect(page.getByTestId('remote-conversion-hint')).toContainText('ports');
    await expect(page.getByTestId('remote-submit')).toBeEnabled();
    await page.getByTestId('remote-url-input').fill('/local/repo.git');
    await expect(page.getByTestId('remote-convert')).toHaveCount(0);
    await page.getByTestId('remote-submit').click();
    await expect(page.getByTestId('remotes-notice')).toContainText('updated');
    await expect(page.getByTestId('remote-push-url-hint')).toBeVisible();
  });
});
