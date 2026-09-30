import { boot, expect, fixtureRepo, secondFixtureRepo, test } from './harness';

function repoWithoutConflicts(repo = fixtureRepo()) {
  repo.status.files = repo.status.files.filter((file) => file.index !== 'U');
  return repo;
}

test.describe('provider-neutral Agent prompts', () => {
  test('copies a commit prompt for the current repository without running Git writes', async ({
    page
  }) => {
    const stub = await boot(page, { open: [repoWithoutConflicts()] });

    await page.getByTestId('commit-mode-trigger').click();
    await page.getByTestId('commit-mode-copy-ai-commit-prompt').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toEqual({
      repoId: 7,
      request: { kind: 'commit', amend: false }
    });
    expect(await stub.clipboard()).toContain('/Users/dev/projects/augur-git');
    expect(await stub.clipboard()).toContain('"kind":"commit","amend":false');
    expect((await stub.commandNames()).filter((name) => name === 'run_action')).toHaveLength(0);
    await expect(page.getByTestId('status-message')).toContainText('Prompt copied');
  });

  test('uses Amend mode for the copied prompt', async ({ page }) => {
    const stub = await boot(page, { open: [repoWithoutConflicts()] });

    await page.getByTestId('commit-mode-trigger').click();
    await page.getByTestId('commit-mode-amend').click();
    await page.getByTestId('commit-mode-trigger').click();
    await page.getByTestId('commit-mode-copy-ai-commit-prompt').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toMatchObject({ request: { kind: 'commit', amend: true } });
    expect(await stub.clipboard()).toContain('"kind":"commit","amend":true');
  });

  test('copies a generic conflict prompt from the changes panel', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId('agent-prompt-resolve-conflicts').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toEqual({
      repoId: 7,
      request: { kind: 'resolveConflicts' }
    });
    expect(await stub.clipboard()).toContain('/Users/dev/projects/augur-git');
  });

  test('copies a merge prompt for the selected source branch', async ({ page }) => {
    const stub = await boot(page, { open: [repoWithoutConflicts()] });

    await page.getByTestId('more-menu-trigger').click();
    await page.getByTestId('more-menu-merge').click();
    await page.getByTestId('merge-copy-prompt').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toEqual({
      repoId: 7,
      request: { kind: 'merge', source: 'feature/tauri', noFf: false }
    });
  });

  test('copies a rebase prompt for the selected source branch', async ({ page }) => {
    const stub = await boot(page, { open: [repoWithoutConflicts()] });

    await page.getByTestId('more-menu-trigger').click();
    await page.getByTestId('more-menu-rebase').click();
    await page.getByTestId('rebase-copy-prompt').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toEqual({
      repoId: 7,
      request: { kind: 'rebase', source: 'feature/tauri' }
    });
  });

  test('pull conflicts offer the prompt only from the conflict dialog', async ({ page }) => {
    const stub = await boot(page, {
      open: [repoWithoutConflicts()],
      probeMerge: { has_conflicts: true }
    });

    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'commandDone',
      label: 'pull',
      success: false,
      message: 'CONFLICT (content): resolve the conflict'
    });

    await expect(page.getByTestId('merge-conflict-dialog')).toBeVisible();
    await expect(page.getByTestId('agent-prompt-pull')).toHaveCount(0);
    await page.getByTestId('merge-conflict-copy-prompt').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toEqual({
      repoId: 7,
      request: { kind: 'resolveConflicts', origin: 'merge' }
    });
  });

  test('reports clipboard failures without claiming the prompt was copied', async ({ page }) => {
    const stub = await boot(page, {
      open: [repoWithoutConflicts()],
      refusals: {
        'plugin:clipboard-manager|write_text': {
          key: 'err-unknown',
          detail: 'clipboard unavailable'
        }
      }
    });

    await page.getByTestId('commit-mode-trigger').click();
    await page.getByTestId('commit-mode-copy-ai-commit-prompt').click();

    await expect(page.getByTestId('status-message')).toContainText(
      'Could not copy the prompt to the clipboard'
    );
    expect(await stub.clipboard()).toBe('');
  });

  test('offers an AI prompt after patch application fails', async ({ page }) => {
    const stub = await boot(page, {
      open: [repoWithoutConflicts()],
      failingActions: ['applyPatch']
    });

    await page.getByTestId('more-menu-trigger').click();
    await page.getByTestId('more-menu-apply-patch').click();
    await expect(page.getByTestId('patch-prompt-error')).toBeVisible();
    await page.getByTestId('patch-prompt-error-copy').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toMatchObject({
      repoId: 7,
      request: {
        kind: 'applyPatch',
        path: '/tmp/agent-prompt.patch',
        failure: expect.stringContaining('could not read from remote')
      }
    });
    expect(await stub.clipboard()).toContain('Request: {"kind":"applyPatch"');
  });

  test('copies from a merge conflict dialog with its operation origin', async ({ page }) => {
    const stub = await boot(page, {
      open: [repoWithoutConflicts()],
      probeMerge: { has_conflicts: true }
    });

    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'commandDone',
      label: 'merge',
      success: false,
      message: 'CONFLICT (content): resolve the conflict'
    });
    await page.getByTestId('merge-conflict-copy-prompt').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toEqual({
      repoId: 7,
      request: { kind: 'resolveConflicts', origin: 'merge' }
    });
  });

  test('copies from a rebase conflict dialog with its operation origin', async ({ page }) => {
    const stub = await boot(page, {
      open: [repoWithoutConflicts()],
      probeRebase: { rebase_in_progress: true }
    });

    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'commandDone',
      label: 'rebase',
      success: false,
      message: 'CONFLICT (content): resolve the conflict'
    });
    await page.getByTestId('rebase-conflict-copy-prompt').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toEqual({
      repoId: 7,
      request: { kind: 'resolveConflicts', origin: 'rebase' }
    });
  });

  test('copies from a stash pop conflict dialog without scheduling a commit', async ({ page }) => {
    const stub = await boot(page, {
      open: [repoWithoutConflicts()],
      probeMerge: { has_conflicts: true }
    });

    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'commandDone',
      label: 'stash pop',
      success: false,
      message: 'CONFLICT (content): resolve the conflict'
    });
    await page.getByTestId('stash-pop-conflict-copy-prompt').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toEqual({
      repoId: 7,
      request: { kind: 'resolveConflicts', origin: 'stashPop' }
    });
    expect((await stub.commandNames()).filter((name) => name === 'run_action')).toHaveLength(0);
  });

  test('uses the selected tab repository when copying after switching repositories', async ({
    page
  }) => {
    const first = repoWithoutConflicts();
    const second = repoWithoutConflicts(secondFixtureRepo());
    const stub = await boot(page, { open: [first, second] });

    await page.getByTestId(`tab-${second.path}`).click();
    await page.getByTestId('commit-mode-trigger').click();
    await page.getByTestId('commit-mode-copy-ai-commit-prompt').click();

    const request = (await stub.commands()).find((entry) => entry.cmd === 'generate_agent_prompt');
    expect(request?.args).toEqual({
      repoId: second.id,
      request: { kind: 'commit', amend: false }
    });
    expect(await stub.clipboard()).toContain(second.path);
    expect(await stub.clipboard()).not.toContain(first.path);
  });
});
