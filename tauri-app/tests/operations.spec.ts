import { expect, test } from '@playwright/test';

import { boot, fixtureRepo, rightClick } from './harness';

/**
 * Git operations and the dialogs that guard them.
 *
 * Two things are asserted for every action: the interface asked the backend for
 * the exact named operation, and the command label the backend would report is
 * the one the reference application reports. The label is protocol, because it
 * is what a result message and the settings page both display.
 */

/** The label the reference application reports for a completed operation. */
const LABELS: Record<string, string> = {
  fetch: 'fetch --all --prune',
  pullMerge: 'pull',
  pullRebase: 'pull --rebase',
  push: 'push',
  pushForce: 'push --force',
  pushSetUpstream: 'push --set-upstream',
  pushRenameRemote: 'push --rename',
  pushDeleteRemote: 'push --delete',
  merge: 'merge',
  abortMerge: 'merge --abort',
  rebase: 'rebase',
  abortRebase: 'rebase --abort'
};

async function openRepository(page: import('@playwright/test').Page) {
  const stub = await boot(page, { open: [fixtureRepo()] });
  await expect(page.getByTestId('repo-7')).toBeVisible();
  return stub;
}
/**
 * The fixture without its unmerged file.
 *
 * A conflict disables exactly the actions that would disturb the merge, so a
 * test about those actions needs a repository that has none.
 */
function cleanRepo() {
  const repo = fixtureRepo();
  repo.status.files = repo.status.files.filter((file) => file.index !== 'U');
  return repo;
}

test.describe('toolbar operations', () => {
  test('fetches and reports the reference command label', async ({ page }) => {
    await openRepository(page);

    await page.getByTestId('toolbar-fetch').click();

    // The label is what the status line shows, so it is checked rather than the
    // argv: the frontend never sees argv.
    await expect(page.getByTestId('status-message')).toContainText(LABELS.fetch ?? '');
  });

  test('pulls with a merge by default', async ({ page }) => {
    // A conflict blocks the integration actions, so the clean fixture is the
    // one that can reach the button.
    const stub = await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('toolbar-pull').click();

    await expect(page.getByTestId('status-message')).toContainText(LABELS.pullMerge ?? '');
    const actions = (await stub.commands()).filter((entry) => entry.cmd === 'run_action');
    expect(actions).toHaveLength(1);
    expect((actions[0]!.args as any).action.action).toBe('pullMerge');
  });

  test('pulls with a rebase when the preference asks for it', async ({ page }) => {
    const stub = await boot(page, { open: [cleanRepo()], pullAction: 'rebase' });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('toolbar-pull').click();

    await expect(page.getByTestId('status-message')).toContainText(LABELS.pullRebase ?? '');
    // The rebase runs behind the same preflight a branch rebase uses.
    expect((await stub.commandNames()).filter((c) => c === 'probe_rebase')).toHaveLength(1);
    const actions = (await stub.commands()).filter((entry) => entry.cmd === 'run_action');
    expect((actions.at(-1)!.args as any).action.action).toBe('pullRebase');
  });

  test('keeps operation progress in the status bar only', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], actionDelay: 250 });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('toolbar-fetch').click();

    await expect(page.getByTestId('toolbar-busy')).toHaveCount(0);
    await expect(page.getByTestId('status-busy')).toHaveText('Working');

    // The lower-right progress state clears and the result remains visible.
    await expect(page.getByTestId('status-busy')).toHaveCount(0);
    await expect(page.getByTestId('status-message')).toContainText(LABELS.fetch ?? '');
  });

  test('reports a rebase the preflight refused to start', async ({ page }) => {
    // An unresolved merge blocks the integration actions, so the clean fixture
    // is the one that can reach the button.
    await boot(page, {
      open: [cleanRepo()],
      pullAction: 'rebase',
      refusals: {
        probe_rebase: { key: 'err-git-run', detail: 'fatal: cannot rebase here' }
      }
    });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('toolbar-pull').click();

    await expect(page.getByTestId('status-message')).toHaveText(
      'Rebase was not started: fatal: cannot rebase here'
    );
  });

  test('disables network actions without a remote', async ({ page }) => {
    // A repository with no remote configured.
    const bare = fixtureRepo();
    bare.refs.remotes = [];
    bare.status.upstream = null;
    await boot(page, { open: [bare] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await expect(page.getByTestId('toolbar-fetch')).toBeDisabled();
    await expect(page.getByTestId('toolbar-push')).toBeDisabled();
    await expect(page.getByTestId('toolbar-pull')).toBeDisabled();
    // Local actions stay available.
    await expect(page.getByTestId('toolbar-branch')).toBeEnabled();
  });

  test('blocks integration actions while a merge is unresolved', async ({ page }) => {
    await openRepository(page);

    // The fixture has an unmerged file, so the actions that would replace the
    // in-progress merge are refused.
    await page.getByTestId('branch-menu-trigger').click();
    await expect(page.getByTestId('branch-menu-merge')).toBeDisabled();
    await expect(page.getByTestId('branch-menu-merge-no-ff')).toBeDisabled();
    await expect(page.getByTestId('branch-menu-rebase')).toBeDisabled();
    // Stashing is still offered, because it leaves the merge alone.
    await expect(page.getByTestId('branch-menu-stash')).toBeEnabled();
    // Everything that would disturb the unresolved merge is refused: recording
    // a branch, popping a stash, and applying a patch that could overwrite the
    // conflicted file.
    await expect(page.getByTestId('branch-menu-branch-new')).toBeDisabled();
    await expect(page.getByTestId('branch-menu-stash-pop')).toBeDisabled();
    await expect(page.getByTestId('branch-menu-apply-patch')).toBeDisabled();
    // Renaming writes a ref and does not touch the working tree.
    await expect(page.getByTestId('branch-menu-branch-rename')).toBeEnabled();
  });

  test('does not discard files while conflicts are unresolved', async ({ page }) => {
    await openRepository(page);

    // A group-wide restore would throw away the work the merge is waiting on.
    await expect(page.getByTestId('changes-discard-all')).toBeDisabled();
    await expect(page.getByTestId('changes-stage-all')).toBeDisabled();
    // Staging is still available for the staged group.
    await expect(page.getByTestId('changes-unstage-all')).toBeEnabled();
  });

  test('tells the four WSL failures apart', async ({ page }) => {
    // The backend distinguishes a missing distribution, a distribution without
    // Git, a path that is not a repository, and a path that does not exist. The
    // dialog showed the raw detail for all four, which is a line of transport
    // text with nothing in it saying which of the four had happened.
    await boot(page, {
      windows: true,
      refusals: {
        probe_wsl_repository: {
          key: 'err-wsl-git-missing',
          detail: 'bash: git: command not found'
        }
      }
    });

    await page.getByTestId('welcome-open-wsl').click();
    await expect(page.getByTestId('wsl-dialog')).toBeVisible();

    await page.getByTestId('wsl-path').fill('/home/dev/repo');
    await expect(page.getByTestId('wsl-check-failed')).toHaveText(
      'Git is not available inside the WSL distribution: bash: git: command not found'
    );
  });

  test('says the distribution list is loading', async ({ page }) => {
    await boot(page, { windows: true, wslDelay: 300 });

    await page.getByTestId('welcome-open-wsl').click();
    await expect(page.getByTestId('wsl-loading-distros')).toHaveText('Loading…');

    // The list replaces the loading text once it arrives.
    await expect(page.getByTestId('wsl-distro')).toBeVisible();
    await expect(page.getByTestId('wsl-loading-distros')).toHaveCount(0);
  });

  test('a WSL path reason is a whole sentence with nothing to add', async ({ page }) => {
    await boot(page, { windows: true });

    await page.getByTestId('welcome-open-wsl').click();
    await page.getByTestId('wsl-path').fill('home/dev/repo');

    // The reason keys have no `{ $detail }`, so rendering must not append the
    // empty detail and produce a sentence with a stray colon.
    await expect(page.getByTestId('wsl-check-failed')).toHaveText(
      'Enter an absolute Linux path starting with /'
    );
  });

  test("names the working-tree diff's loading and failure states", async ({ page }) => {
    // The loading state only exists while the diff is in flight, so the diff has
    // to be slow for it to be observable at all.
    await boot(page, { open: [fixtureRepo()], workingDiffDelay: 600 });

    // A bare spinner and a bare error both read as a broken panel, so the
    // reference names both and so does this.
    await page.getByTestId('changes-file-src/main.rs').first().click();
    await expect(page.getByTestId('diff-loading-label')).toHaveText(
      'Loading working-tree diff\u2026'
    );
    await page.getByTestId('diff-hunk').first().waitFor();
    await expect(page.getByTestId('diff-loading-label')).toHaveCount(0);
  });

  test("puts a heading above a failed working-tree diff's reason", async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      workingDiffFailure: 'fatal: unable to read the working tree'
    });

    await page.getByTestId('changes-file-src/main.rs').first().click();
    // The heading says what failed; Git's own words say why. Without the
    // heading the reason has no subject.
    await expect(page.getByTestId('diff-error-label')).toHaveText(
      'Unable to load working-tree diff'
    );
    await expect(page.getByTestId('diff-error')).toContainText(
      'fatal: unable to read the working tree'
    );
  });

  test('localizes a refused command rather than pasting its detail', async ({ page }) => {
    // A refusal carries a catalog key and a detail, and only the key is written
    // for a reader. The interface showed the detail alone, so a Git failure
    // arrived as a bare `fatal:` line with nothing saying what it was a failure
    // of.
    await boot(page, {
      open: [fixtureRepo()],
      refusals: {
        working_tree_operation: {
          key: 'err-git-run',
          detail: 'fatal: pathspec did not match any files'
        }
      }
    });

    await page.getByTestId('changes-file-src/main.rs').first().click();
    await page.getByTestId('changes-toggle-src/main.rs').first().click();

    // The key's own sentence, with Git's words inside it.
    await expect(page.getByTestId('status-message')).toHaveText(
      'Failed to run git: fatal: pathspec did not match any files'
    );
  });

  test('reports a failed operation without losing the repository', async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      failingActions: ['fetch']
    });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('toolbar-fetch').click();

    // The failure is the first line of Git's message, not the whole thing, and
    // the repository stays usable.
    await expect(page.getByTestId('status-message')).toContainText('fetch');
    await expect(page.getByTestId('status-message')).toContainText('could not read from remote');
    await expect(page.getByTestId('graph')).toBeVisible();
  });
});

test.describe('branch navigation', () => {
  test('checks out a local branch on double click', async ({ page }) => {
    const repo = cleanRepo();
    const stub = await boot(page, { open: [repo] });

    await page.getByTestId('branch-feature/tauri').dblclick();

    const commands = await stub.commands();
    const checkout = commands.find((entry) => entry.cmd === 'run_action');
    expect(checkout?.args).toMatchObject({
      repoId: 7,
      action: {
        action: 'checkout',
        target: { kind: 'localBranch', localBranch: 'feature/tauri' }
      }
    });
  });

  test('keeps checkout blocked while the repository has conflicts', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId('branch-feature/tauri').dblclick();

    expect((await stub.commands()).some((entry) => entry.cmd === 'run_action')).toBe(false);
  });
});

test.describe('branch dialogs', () => {
  test('refuses an invalid or duplicate branch name', async ({ page }) => {
    await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('branch-menu-trigger').click();
    await page.getByTestId('branch-menu-branch-new').click();
    await expect(page.getByTestId('branch-dialog')).toBeVisible();

    const input = page.getByTestId('branch-name-input');
    const confirm = page.getByTestId('branch-dialog-confirm');

    // Nothing typed yet.
    await expect(confirm).toBeDisabled();

    await input.fill('feature/ok');
    await expect(confirm).toBeEnabled();

    // An existing local branch.
    await input.fill('master');
    await expect(page.getByTestId('branch-name-error')).toContainText(
      'Branch "master" already exists.'
    );
    await expect(confirm).toBeDisabled();

    // Invalid ref syntax.
    await input.fill('bad..name');
    await expect(page.getByTestId('branch-name-error')).toContainText('Invalid branch name');
    await expect(confirm).toBeDisabled();

    // A space is the mistake that slips past a naive check.
    await input.fill('has space');
    await expect(confirm).toBeDisabled();

    await input.fill('feature/ok');
    await expect(confirm).toBeEnabled();
  });

  test('creates a branch and asks the backend for it by name', async ({ page }) => {
    const stub = await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('branch-menu-trigger').click();
    await page.getByTestId('branch-menu-branch-new').click();
    await page.getByTestId('branch-name-input').fill('feature/tauri-2');
    await page.getByTestId('branch-dialog-confirm').click();

    await expect(page.getByTestId('branch-dialog')).toHaveCount(0);
    const actions = (await stub.commands()).filter((entry) => entry.cmd === 'run_action');
    expect(
      actions.some(
        (entry) =>
          (entry.args as any).action.action === 'createBranch' &&
          (entry.args as any).action.name === 'feature/tauri-2'
      )
    ).toBe(true);
  });

  test("lets a rename keep its own name but not another branch's", async ({ page }) => {
    await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="branch-release"]');
    await page.getByTestId('context-rename').click();
    await expect(page.getByTestId('branch-dialog')).toBeVisible();

    // Unchanged, so the confirm is available: a rename to the same name is a
    // no-op rather than a collision.
    await expect(page.getByTestId('branch-dialog-confirm')).toBeEnabled();

    await page.getByTestId('branch-name-input').fill('master');
    await expect(page.getByTestId('branch-name-error')).toBeVisible();
    await expect(page.getByTestId('branch-dialog-confirm')).toBeDisabled();
  });

  test('warns before a force delete and passes the flag through', async ({ page }) => {
    const stub = await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="branch-release"]');
    await page.getByTestId('context-delete').click();

    const dialog = page.getByTestId('delete-ref-dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText('release');
    // The force option only exists for a branch, and defaults to off.
    await expect(page.getByTestId('delete-force')).not.toBeChecked();

    await page.getByTestId('delete-ref-confirm').click();
    const actions = (await stub.commands()).filter((e) => e.cmd === 'run_action');
    const last = actions[actions.length - 1]!;
    expect((last.args as any).action).toMatchObject({
      action: 'deleteBranch',
      name: 'release',
      force: false
    });
  });

  test('confirms a stash drop by reference', async ({ page }) => {
    const stub = await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    const stash = page.getByTestId('stash-row-stash@{0}');
    await rightClick(page, '[data-testid="stash-row-stash@{0}"]');
    await page.getByTestId('context-drop').click();

    await expect(page.getByTestId('stash-drop-dialog')).toContainText('stash@{0}');
    await page.getByTestId('stash-drop-confirm').click();

    const actions = (await stub.commands()).filter((e) => e.cmd === 'run_action');
    expect((actions[actions.length - 1]!.args as any).action).toMatchObject({
      action: 'stashDrop',
      stashRef: 'stash@{0}'
    });
  });
});

test.describe('push confirmations', () => {
  test('never force pushes without the confirmation', async ({ page }) => {
    const stub = await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('toolbar-push-force').click();

    // The dialog is the only path to the operation.
    await expect(page.getByTestId('force-push-dialog')).toBeVisible();
    const before = (await stub.commands()).filter((e) => e.cmd === 'run_action');
    expect(before).toHaveLength(0);

    await page.getByTestId('force-push-confirm').click();
    const after = (await stub.commands()).filter((e) => e.cmd === 'run_action');
    expect(after).toHaveLength(1);
    expect((after[0]!.args as any).action.action).toBe('pushForce');
  });

  test('cancelling a force push runs nothing', async ({ page }) => {
    const stub = await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('toolbar-push-force').click();
    await page.getByTestId('force-push-cancel').click();

    await expect(page.getByTestId('force-push-dialog')).toHaveCount(0);
    expect((await stub.commands()).filter((e) => e.cmd === 'run_action')).toHaveLength(0);
  });

  test('offers to publish a branch that has no upstream', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    // A branch with a remote but no upstream cannot be pushed with a plain
    // `git push`, so the confirmation appears first.
    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'status',
      branch: 'topic',
      head: '0'.repeat(39) + '1',
      upstream: null,
      ahead: 1,
      behind: 0,
      files: [],
      branches: [
        { name: 'topic', is_head: true },
        { name: 'master', is_head: false }
      ]
    });

    await page.getByTestId('toolbar-push').click();

    const dialog = page.getByTestId('push-upstream-dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText('topic');
    await expect(dialog).toContainText('origin');

    await page.getByTestId('push-upstream-confirm').click();
    const actions = (await stub.commands()).filter((e) => e.cmd === 'run_action');
    expect((actions[actions.length - 1]!.args as any).action).toMatchObject({
      action: 'pushSetUpstream',
      branch: 'topic',
      remote: 'origin'
    });
  });

  test('pushes directly when the branch already tracks one', async ({ page }) => {
    const stub = await openRepository(page);

    await page.getByTestId('toolbar-push').click();

    // No dialog, because the fixture's branch tracks origin/master.
    await expect(page.getByTestId('push-upstream-dialog')).toHaveCount(0);
    await expect(page.getByTestId('status-message')).toContainText(LABELS.push ?? '');
    void stub;
  });
});

test.describe('discarding changes', () => {
  test('lists the files and requires a confirmation', async ({ page }) => {
    // A repository with no conflicts, so a discard is offered at all.
    const repo = cleanRepo();
    repo.status.files = [
      { index: ' ', worktree: 'M', path: 'src/main.rs', old_path: null },
      { index: ' ', worktree: '?', path: 'notes.md', old_path: null }
    ];
    const stub = await boot(page, { open: [repo] });
    await expect(page.getByTestId('changes-file-notes.md')).toBeVisible();

    await page.getByTestId('changes-discard-all').click();

    const dialog = page.getByTestId('discard-dialog');
    await expect(dialog).toBeVisible();
    // The dialog names the files, so the destruction is specific.
    await expect(dialog).toContainText('src/main.rs');
    await expect(dialog).toContainText('notes.md');
    expect((await stub.commands()).filter((e) => e.cmd === 'working_tree_operation')).toHaveLength(
      0
    );

    await page.getByTestId('discard-confirm').click();
    const operations = (await stub.commands()).filter((e) => e.cmd === 'working_tree_operation');
    expect(operations).toHaveLength(1);
    expect((operations[0]!.args as any).action).toBe('discard');
    expect((operations[0]!.args as any).all).toBe(true);
  });

  test('cancelling a discard runs nothing', async ({ page }) => {
    const stub = await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await rightClick(page, '[data-testid="changes-file-src/main.rs"]');
    await page.getByTestId('context-discard').click();
    await expect(page.getByTestId('discard-dialog')).toBeVisible();
    await page.getByTestId('discard-cancel').click();

    expect((await stub.commands()).filter((e) => e.cmd === 'working_tree_operation')).toHaveLength(
      0
    );
  });
});

test.describe('merge and rebase', () => {
  test('runs a merge that Git reports as already up to date', async ({ page }) => {
    // The reference does not special-case a no-op merge: it runs it, and Git
    // answers "Already up to date". That is a success, not a refusal, so the
    // merge is still sent and still reported as finished.
    await boot(page, { open: [cleanRepo()], probeMerge: { already_merged: true } });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('branch-menu-trigger').click();
    await page.getByTestId('branch-menu-merge').click();
    await expect(page.getByTestId('merge-dialog')).toBeVisible();
    await page.getByTestId('merge-dialog-confirm').click();

    // The dialog closed and the merge was sent under the reference's label.
    await expect(page.getByTestId('merge-dialog')).toHaveCount(0);
    await expect(page.getByTestId('status-message')).toContainText('merge');
  });

  test('offers the source branch and the no-ff option', async ({ page }) => {
    await boot(page, { open: [cleanRepo()] });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('branch-menu-trigger').click();
    await page.getByTestId('branch-menu-merge-no-ff').click();

    const dialog = page.getByTestId('merge-dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText('master');
    // Every local branch except the checked-out one is offered.
    const options = page.getByTestId('merge-source').locator('option');
    await expect(options).toHaveCount(2);
    await expect(options.nth(0)).toHaveText('feature/tauri');
    // The no-ff option is on for the menu entry that asked for it.
    await expect(page.getByTestId('merge-no-ff')).toBeChecked();
  });

  test('refuses a rebase over a dirty tree with a specific message', async ({ page }) => {
    // The preflight probe reports a dirty tree, so the rebase is refused before
    // Git can produce an unhelpful failure.
    const dirty = cleanRepo();
    dirty.status.files = [{ index: ' ', worktree: 'M', path: 'src/main.rs', old_path: null }];
    await boot(page, { open: [dirty], probeRebase: { has_changes: true } });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('branch-menu-trigger').click();
    await page.getByTestId('branch-menu-rebase').click();
    await expect(page.getByTestId('rebase-dialog')).toBeVisible();
    await page.getByTestId('rebase-dialog-confirm').click();

    await expect(page.getByTestId('status-message')).toContainText(
      'Rebase requires a clean working tree'
    );
  });

  test('refuses a rebase while another operation is in progress', async ({ page }) => {
    await boot(page, {
      open: [cleanRepo()],
      probeRebase: { other_operation_in_progress: true }
    });
    await expect(page.getByTestId('repo-7')).toBeVisible();

    await page.getByTestId('branch-menu-trigger').click();
    await page.getByTestId('branch-menu-rebase').click();
    await page.getByTestId('rebase-dialog-confirm').click();

    await expect(page.getByTestId('status-message')).toContainText(
      'Rebase was not started because another Git operation is in progress.'
    );
  });
});
