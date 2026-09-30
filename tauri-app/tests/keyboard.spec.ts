import {
  boot,
  expect,
  fixtureRepo,
  longFixtureRepo,
  rightClick,
  secondFixtureRepo,
  test
} from './harness';

function withoutConflicts() {
  const repo = fixtureRepo();
  repo.status.files = repo.status.files.filter((file) => file.path !== 'src/conflict.rs');
  return repo;
}

async function actions(stub: Awaited<ReturnType<typeof boot>>) {
  return (await stub.commands()).filter((entry) => entry.cmd === 'run_action');
}

test.describe('application shortcuts', () => {
  test('runs repository commands and keeps shortcuts out of the commit editor', async ({
    page
  }) => {
    const stub = await boot(page, {
      open: [withoutConflicts()],
      pullAction: 'merge',
      actionDelay: 300
    });
    const busy = page.getByTestId('status-busy');
    const refreshCount = (await stub.commands()).filter(
      (entry) => entry.cmd === 'refresh_repository'
    ).length;

    await page.getByTestId('graph-header').click();
    await page.keyboard.press('f');
    await expect
      .poll(async () => (await actions(stub)).map((entry) => (entry.args.action as any).action))
      .toEqual(['fetch']);
    await expect(busy).toBeVisible();
    await expect(busy).toBeHidden();
    await page.keyboard.press('Shift+r');
    await expect
      .poll(async () =>
        (await stub.commands()).filter((entry) => entry.cmd === 'refresh_repository')
      )
      .toHaveLength(refreshCount + 1);
    await page.keyboard.press('p');
    await expect
      .poll(async () => (await actions(stub)).map((entry) => (entry.args.action as any).action))
      .toEqual(['fetch', 'pullMerge']);
    await expect(busy).toBeVisible();
    await expect(busy).toBeHidden();
    await page.keyboard.press('Shift+p');

    await expect
      .poll(async () => (await actions(stub)).map((entry) => (entry.args.action as any).action))
      .toEqual(['fetch', 'pullMerge', 'push']);
    await expect(busy).toBeVisible();
    await expect(busy).toBeHidden();
    await page.keyboard.press('c');
    const editor = page.getByTestId('commit-message');
    await expect(editor).toBeFocused();
    await page.keyboard.press('p');
    await expect(editor).toHaveValue('p');
    expect((await actions(stub)).length).toBe(3);
  });

  test('keeps Pull disabled by conflicts', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await page.getByTestId('graph-header').click();
    await page.keyboard.press('p');
    expect((await actions(stub)).length).toBe(0);
  });

  test('keeps network shortcuts disabled without a remote', async ({ page }) => {
    const repo = withoutConflicts();
    repo.refs.remotes = [];
    const stub = await boot(page, { open: [repo] });
    await page.getByTestId('graph-header').click();
    await page.keyboard.press('p');
    await page.keyboard.press('Shift+p');
    await page.keyboard.press('f');
    expect((await actions(stub)).length).toBe(0);
  });

  test('keeps Pull and Push disabled while a network action is busy', async ({ page }) => {
    const stub = await boot(page, { open: [withoutConflicts()], actionDelay: 300 });
    await page.getByTestId('graph-header').click();
    await page.keyboard.press('f');
    await expect(page.getByTestId('status-busy')).toBeVisible();
    await page.keyboard.press('p');
    await page.keyboard.press('Shift+p');
    await expect
      .poll(async () => (await actions(stub)).map((entry) => (entry.args.action as any).action))
      .toEqual(['fetch']);
  });

  test('keeps Push on its enabled toolbar path during a conflict', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await page.getByTestId('graph-header').click();
    await page.keyboard.press('Shift+p');
    await expect
      .poll(async () => (await actions(stub)).map((entry) => (entry.args.action as any).action))
      .toEqual(['push']);
  });

  test('routes Pull and Push without an upstream through their existing flows', async ({
    page
  }) => {
    const stub = await boot(page, { open: [secondFixtureRepo()], pullAction: 'merge' });
    await page.getByTestId('graph-header').click();

    await page.keyboard.press('p');
    await expect.poll(async () => (await actions(stub)).length).toBe(1);
    expect(((await actions(stub))[0]!.args.action as any).action).toBe('pullMerge');

    await page.keyboard.press('Shift+p');
    await expect(page.getByTestId('push-upstream-confirm')).toBeVisible();
    expect((await actions(stub)).length).toBe(1);
    await page.keyboard.press('p');
    expect((await actions(stub)).length).toBe(1);
  });

  test('keeps the Pull shortcut on the configured rebase path', async ({ page }) => {
    const stub = await boot(page, { open: [withoutConflicts()], pullAction: 'rebase' });
    await page.getByTestId('graph-header').click();
    await page.keyboard.press('p');
    await expect
      .poll(async () => (await actions(stub)).map((entry) => (entry.args.action as any).action))
      .toEqual(['pullRebase']);
    expect(
      (await stub.commandNames()).filter((command) => command === 'probe_rebase')
    ).toHaveLength(1);
  });

  test('checks out focused refs and toggles staging on the focused file', async ({ page }) => {
    const stub = await boot(page, { open: [withoutConflicts()], actionDelay: 300 });

    for (const testId of [
      'branch-feature/tauri',
      'remote-branch-origin/master',
      'tag-row-v1.0.0'
    ]) {
      await page.getByTestId(testId).focus();
      await page.keyboard.press('Space');
      await expect(page.getByTestId('status-busy')).toHaveText('Working');
      await expect(page.getByTestId('status-busy')).toHaveCount(0);
    }

    await expect
      .poll(async () =>
        (await actions(stub))
          .filter((entry) => (entry.args.action as any).action === 'checkout')
          .map((entry) => (entry.args.action as any).target)
      )
      .toEqual([
        { kind: 'localBranch', localBranch: 'feature/tauri' },
        { kind: 'remoteBranch', remoteBranch: 'origin/master' },
        { kind: 'tag', tag: 'v1.0.0' }
      ]);

    await page.getByTestId('changes-file-notes.md').focus();
    await page.keyboard.press('Space');
    await expect
      .poll(async () =>
        (await stub.commands()).filter((entry) => entry.cmd === 'working_tree_operation')
      )
      .toHaveLength(1);
    const stage = (await stub.commands()).find((entry) => entry.cmd === 'working_tree_operation')!;
    expect(stage.args.action).toBe('stage');
    expect((stage.args.files as any[]).map((file) => file.path)).toEqual(['notes.md']);
  });

  test('moves focus through the refs and changes lists with Vim and arrow keys', async ({
    page
  }) => {
    await boot(page, { open: [withoutConflicts()] });

    const feature = page.getByTestId('branch-feature/tauri');
    const release = page.getByTestId('branch-release');
    await feature.focus();
    await page.keyboard.press('j');
    await expect(release).toBeFocused();
    await page.keyboard.press('ArrowUp');
    await expect(feature).toBeFocused();

    const files = page.locator('[data-keyboard-list="changes"] [data-keyboard-list-item]');
    const notes = page.getByTestId('changes-file-notes.md');
    const notesIndex = await notes.evaluate((selected) =>
      Array.from(
        document.querySelectorAll('[data-keyboard-list="changes"] [data-keyboard-list-item]')
      ).indexOf(selected)
    );
    await notes.focus();
    await page.keyboard.press('k');
    await expect(files.nth(notesIndex - 1)).toBeFocused();
  });

  test('does not run row or global shortcuts while a context menu is open', async ({ page }) => {
    const stub = await boot(page, { open: [withoutConflicts()] });
    const branch = page.getByTestId('branch-feature/tauri');
    await branch.focus();
    await rightClick(page, '[data-testid="ref-feature/tauri"]');
    await expect(page.locator('[role="menu"]')).toBeVisible();

    await page.keyboard.press('Space');
    await page.keyboard.press('p');
    expect((await actions(stub)).length).toBe(0);
    await expect(page.locator('[role="menu"]')).toBeVisible();
  });

  test('navigates the graph, checks out its selection, and scrolls virtual rows', async ({
    page
  }) => {
    const longRepo = longFixtureRepo(500);
    longRepo.status.files = longRepo.status.files.filter((file) => file.path !== 'src/conflict.rs');
    const stub = await boot(page, { open: [longRepo] });
    const graph = page.getByTestId('graph-list');
    const secondCommitId = await page.locator('.graph-row').nth(1).getAttribute('id');

    await page.locator('.graph-row').first().click();
    await page.keyboard.press('j');
    await expect(graph).toHaveAttribute('aria-activedescendant', secondCommitId!);
    await page.keyboard.press('Space');
    await expect
      .poll(async () =>
        (await actions(stub))
          .filter((entry) => (entry.args.action as any).action === 'checkout')
          .map((entry) => (entry.args.action as any).target.commit)
      )
      .toEqual([secondCommitId!.replace('commit-row-', '')]);

    await graph.focus();
    await expect(graph).toBeFocused();
    await page.keyboard.press('/');
    await expect(page.getByTestId('commit-search')).toBeFocused();
    await graph.focus();
    for (let index = 0; index < 45; index += 1) {
      await page.keyboard.press('j');
    }
    await expect.poll(() => graph.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
    const activeId = await graph.getAttribute('aria-activedescendant');
    await expect(page.locator(`#${activeId}`)).toBeAttached();
    expect((await actions(stub)).length).toBe(1);
  });

  test('adjusts diff typography with platform modifiers and blocks browser zoom', async ({
    page
  }) => {
    const stub = await boot(page, {
      open: [withoutConflicts()],
      windows: true,
      typography: { diff_font_size: 19 }
    });
    const dispatch = async (key: string, options: { ctrlKey?: boolean; shiftKey?: boolean } = {}) =>
      page.evaluate(
        ({ pressedKey, modifiers }) =>
          document.body.dispatchEvent(
            new KeyboardEvent('keydown', {
              key: pressedKey,
              code: pressedKey === '=' ? 'Equal' : '',
              bubbles: true,
              cancelable: true,
              ...modifiers
            })
          ),
        { pressedKey: key, modifiers: options }
      );

    expect(await dispatch('=', { ctrlKey: true })).toBe(false);
    await expect
      .poll(() =>
        page.evaluate(() => document.documentElement.style.getPropertyValue('--diff-font-size'))
      )
      .toBe('20px');
    expect(await dispatch('+', { ctrlKey: true, shiftKey: true })).toBe(false);
    expect(await dispatch('-', { ctrlKey: true })).toBe(false);
    await expect
      .poll(() =>
        page.evaluate(() => document.documentElement.style.getPropertyValue('--diff-font-size'))
      )
      .toBe('19px');
    expect(await dispatch('0', { ctrlKey: true })).toBe(false);
    await expect
      .poll(() =>
        page.evaluate(() => document.documentElement.style.getPropertyValue('--diff-font-size'))
      )
      .toBe('16px');
    for (let index = 0; index < 5; index += 1) {
      expect(await dispatch('-', { ctrlKey: true })).toBe(false);
    }
    await expect
      .poll(() =>
        page.evaluate(() => document.documentElement.style.getPropertyValue('--diff-font-size'))
      )
      .toBe('12px');
    expect((await stub.commands()).filter((entry) => entry.cmd === 'set_typography')).toHaveLength(
      7
    );
  });

  test('keeps remapping contextual, detects collisions, and resets to the shipped default', async ({
    page
  }) => {
    const stub = await boot(page, { open: [withoutConflicts()], window: 'settings' });
    await page.getByTestId('settings-nav-shortcuts').click();
    const pull = page.getByTestId('shortcut-repo.pull');

    await expect(pull).toHaveValue('p');
    await expect(page.getByTestId('shortcut-default-repo.pull')).toContainText('Default: p');
    await pull.fill('shift-p');
    await pull.press('Enter');
    await expect(page.getByTestId('shortcut-error')).toContainText('already assigned');
    expect((await stub.commands()).filter((entry) => entry.cmd === 'set_shortcut')).toHaveLength(0);

    await pull.fill('ctrl-alt-p');
    await pull.press('Enter');
    await expect(pull).toHaveValue('ctrl-alt-p');
    await expect(page.getByTestId('shortcut-default-repo.pull')).toContainText('Default: p');
    await page.getByTestId('shortcut-reset-repo.pull').click();
    await expect(pull).toHaveValue('p');
    const writes = (await stub.commands()).filter((entry) => entry.cmd === 'set_shortcut');
    expect(writes).toHaveLength(2);
    expect((writes[0]!.args as any).keys).toEqual(['ctrl-alt-p']);
    expect((writes[1]!.args as any).keys).toBeNull();
  });
});
