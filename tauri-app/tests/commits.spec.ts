import { expect, test } from '@playwright/test';

import { boot, fixtureRepo, longFixtureRepo, rightClick, secondFixtureRepo } from './harness';
import { diffPayload } from './fixtures/stubBackend';

/**
 * Selecting a commit and reading its diff.
 *
 * The point of these is the selection flow: the file list comes from a second
 * event, the diff from a third, and the viewer has to say which commit it is
 * showing so a stale panel cannot be mistaken for current content.
 */

test.describe('commit selection', () => {
  test('waits for a user selection instead of showing a permanent commit placeholder', async ({
    page
  }) => {
    await boot(page, { open: [fixtureRepo()] });

    await expect(page.getByTestId('bottom-no-commit-state')).toBeVisible();
    await expect(page.getByTestId('bottom-panel')).toContainText('No commit selected');
    await expect(page.getByTestId('diff-hunk')).toHaveCount(0);

    await page.locator('.graph-row').first().click();
    await expect(page.getByTestId('bottom-commit-hash')).toHaveText('13c6ef3');
    await expect(page.getByTestId('bottom-no-commit-state')).toHaveCount(0);
    await expect(page.getByTestId('bottom-panel')).not.toContainText('No commit selected');
  });

  test('previews supported image changes and switches SVG between image and diff', async ({
    page
  }) => {
    const stub = await boot(page, { open: [fixtureRepo()], includeImageFixtures: true });
    await page.locator('.graph-row').first().click();
    await page.getByTestId('bottom-file-assets/changed.png').click();
    await expect(page.getByTestId('diff-image-preview')).toBeVisible();
    await expect(page.getByTestId('diff-image-old')).toHaveAttribute(
      'src',
      /^data:image\/png;base64,/
    );
    await expect(page.getByTestId('diff-image-new')).toHaveAttribute(
      'src',
      /^data:image\/png;base64,/
    );

    await page.getByTestId('bottom-file-assets/added.jpg').click();
    await expect(page.getByTestId('diff-image-old-absent')).toHaveText('No image');
    await expect(page.getByTestId('diff-image-new')).toHaveAttribute(
      'src',
      /^data:image\/jpeg;base64,/
    );

    await page.getByTestId('bottom-file-assets/deleted.ico').click();
    await expect(page.getByTestId('diff-image-old')).toHaveAttribute(
      'src',
      /^data:image\/x-icon;base64,/
    );
    await expect(page.getByTestId('diff-image-new-absent')).toHaveText('No image');

    await page.getByTestId('bottom-file-assets/logo.svg').click();
    await expect(page.getByTestId('diff-image-old')).toHaveAttribute(
      'src',
      /^data:image\/svg\+xml;base64,/
    );
    await page.getByTestId('svg-preview-diff').click();
    await expect(page.getByTestId('diff-row').first()).toBeVisible();
    await page.getByTestId('svg-preview-image').click();
    await expect(page.getByTestId('diff-image-preview')).toBeVisible();

    const previewCall = (await stub.commands()).find((entry) => entry.cmd === 'load_image_preview');
    expect(previewCall?.args.target).toMatchObject({ kind: 'change' });
  });

  test('stacks previews in a narrow panel and shows an unavailable state on read failure', async ({
    page
  }) => {
    const stub = await boot(page, {
      open: [fixtureRepo()],
      includeImageFixtures: true,
      imagePreviewFailure: 'preview read failed'
    });
    await page.locator('.graph-row').first().click();
    await page.getByTestId('bottom-file-assets/changed.png').click();
    await page.setViewportSize({ width: 420, height: 800 });
    await expect(page.getByTestId('diff-image-preview')).toHaveClass(
      /diff__image-preview--stacked/
    );
    await expect(page.getByTestId('diff-image-old-unavailable')).toHaveText('Preview unavailable');
    expect((await stub.commands()).some((entry) => entry.cmd === 'load_image_preview')).toBe(true);
  });

  test('drops an older lazy preview reply after selecting another image', async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      includeImageFixtures: true,
      imagePreviewDelay: 220
    });
    await page.locator('.graph-row').first().click();
    await page.getByTestId('bottom-file-assets/changed.png').click();
    await page.getByTestId('bottom-file-assets/logo.svg').click();
    await expect(page.getByTestId('diff-image-preview')).toBeVisible();
    await expect(page.getByTestId('diff-image-old')).toHaveAttribute(
      'alt',
      'assets/logo.svg before'
    );
    await expect(page.locator('img[alt^="assets/changed.png"]')).toHaveCount(0);
  });

  test('loads binary previews in the virtualized all-files view', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], includeImageFixtures: true });
    await page.setViewportSize({ width: 1800, height: 1000 });
    await page.locator('.graph-row').first().click();
    await expect(page.getByTestId('bottom-file-assets/logo.svg')).toBeVisible();
    const virtualList = page.getByTestId('diff-rows');
    await virtualList.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    await expect(page.getByTestId('diff-image-preview').first()).toBeVisible();
    await expect(page.getByTestId('svg-preview-toolbar').first()).toBeVisible();
  });

  test('loads previews for unstaged working-tree image changes', async ({ page }) => {
    const repo = fixtureRepo();
    repo.status.files.push({
      index: ' ',
      worktree: 'M',
      path: 'assets/working.png',
      old_path: null
    });
    const stub = await boot(page, { open: [repo] });
    await page.getByTestId('changes-file-assets/working.png').click();
    await expect(page.getByTestId('diff-image-preview')).toBeVisible();
    const previewCall = (await stub.commands()).find((entry) => entry.cmd === 'load_image_preview');
    expect(previewCall?.args.target).toMatchObject({
      kind: 'workingTree',
      diffKind: 'unstaged',
      file: { path: 'assets/working.png' }
    });
  });

  test('loads previews for staged image changes from the index', async ({ page }) => {
    const repo = fixtureRepo();
    repo.status.files.push({
      index: 'A',
      worktree: ' ',
      path: 'assets/staged.png',
      old_path: null
    });
    const stub = await boot(page, { open: [repo] });
    await page.getByTestId('changes-file-assets/staged.png').click();
    await expect(page.getByTestId('diff-image-preview')).toBeVisible();
    await expect(page.getByTestId('diff-image-old-absent')).toHaveText('No image');
    const previewCall = (await stub.commands()).find((entry) => entry.cmd === 'load_image_preview');
    expect(previewCall?.args.target).toMatchObject({
      kind: 'workingTree',
      diffKind: 'staged',
      file: { path: 'assets/staged.png', index: 'A' }
    });
  });

  test('keeps the graph position when selecting a visible commit', async ({ page }) => {
    await boot(page, { open: [longFixtureRepo()] });

    const list = page.getByTestId('graph-list');
    await list.evaluate((element) => {
      element.scrollTop = 72 * 36;
    });
    await expect.poll(() => list.evaluate((element) => element.scrollTop)).toBe(72 * 36);

    const row = page.locator('.graph-row', { hasText: 'Long history commit 74' });
    await expect(row).toBeVisible();
    const shortHash = await row.locator('.graph-row__hash').innerText();
    await row.click();

    await expect.poll(() => list.evaluate((element) => element.scrollTop)).toBe(72 * 36);
    await expect(row).toHaveClass(/is-selected/);
    await expect(page.getByTestId('bottom-commit-hash')).toHaveText(shortHash);
  });

  test("loads a commit's files and then its diff", async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.locator('.graph-row').first().click();

    // The commit's files arrive in their own event.
    await expect(page.getByTestId('bottom-file-src/lib.rs')).toBeVisible();
    await expect(page.getByTestId('bottom-file-src/commands/repo.rs')).toBeVisible();
    // The panel names the commit so the content is attributable.
    await expect(page.getByTestId('bottom-panel')).toContainText('Add the Tauri command surface');

    // Every changed file is shown at once, each under its own path header.
    await expect(page.getByTestId('bottom-panel')).toContainText('All changed files');
    // The commit has two parents, so the panel says the diff is against the
    // first parent rather than an implicit comparison.
    await expect(page.getByTestId('bottom-panel')).toContainText('vs first parent');
    const headers = page.getByTestId('diff-file-header');
    await expect(headers).toHaveCount(2);
    await expect(headers.nth(0)).toContainText('src/lib.rs');
    await expect(headers.nth(1)).toContainText('src/commands/repo.rs');

    // One hunk per file, because both files of the commit are shown.
    await expect(page.getByTestId('diff-hunk')).toHaveCount(2);
    await expect(page.getByTestId('diff-view')).toContainText('count += 2');
    // The deleted line is present too, because the inline layout pairs them.
    await expect(page.getByTestId('diff-view')).toContainText('count += 1');
  });

  test('requests the files again when the same commit is selected twice', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    const row = page.locator('.graph-row').first();

    await row.click();
    await expect(page.getByTestId('diff-hunk')).toHaveCount(2);
    await row.click();

    await expect(page.getByTestId('diff-hunk')).toHaveCount(2);
    await expect(page.getByTestId('diff-loading-label')).toHaveCount(0);
    const commands = await stub.commands();
    const selections = commands.filter((entry) => entry.cmd === 'select_commit');
    const fileDiffs = commands.filter((entry) => entry.cmd === 'load_commit_file_diff');
    expect(selections).toHaveLength(2);
    expect(Number((selections[1]!.args as any).requestId)).toBeGreaterThan(
      Number((selections[0]!.args as any).requestId)
    );
    expect(fileDiffs).toHaveLength(4);
  });

  test('shows a commit file-list error and lets the user retry', async ({ page }) => {
    const stub = await boot(page, {
      open: [fixtureRepo()],
      commitFilesFailure: 'fatal: bad object'
    });

    await page.locator('.graph-row').first().click();

    await expect(page.getByTestId('diff-error-label')).toHaveText('Unable to load commit diff');
    await expect(page.getByTestId('diff-error')).toContainText('fatal: bad object');
    await expect(page.getByTestId('diff-loading-label')).toHaveCount(0);
    await page.getByTestId('bottom-retry-commit-diff').click();

    await expect(
      (await stub.commands()).filter((entry) => entry.cmd === 'select_commit')
    ).toHaveLength(2);
  });

  test('shows a rejected commit selection command instead of loading forever', async ({ page }) => {
    const stub = await boot(page, {
      open: [fixtureRepo()],
      refusals: {
        select_commit: { key: 'err-git-run', detail: 'fatal: request was refused' }
      }
    });

    await page.locator('.graph-row').first().click();

    await expect(page.getByTestId('diff-error-label')).toHaveText('Unable to load commit diff');
    await expect(page.getByTestId('diff-error')).toContainText('fatal: request was refused');
    await expect(page.getByTestId('diff-loading-label')).toHaveCount(0);
    await page.getByTestId('bottom-retry-commit-diff').click();
    await expect(
      (await stub.commands()).filter((entry) => entry.cmd === 'select_commit')
    ).toHaveLength(2);
  });

  test('shows successful files while reporting partial commit diff failures', async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      commitDiffFailurePaths: ['src/commands/repo.rs']
    });

    await page.locator('.graph-row').first().click();

    await expect(page.getByTestId('diff-hunk')).toHaveCount(1);
    await expect(page.getByTestId('diff-view')).toContainText(
      'Some file diffs could not be loaded (1).'
    );
    await expect(page.getByTestId('bottom-retry-commit-diff')).toBeVisible();
  });

  test('shows a rejected file diff request instead of loading forever', async ({ page }) => {
    await boot(page, {
      open: [fixtureRepo()],
      refusals: {
        load_commit_file_diff: { key: 'err-git-run', detail: 'fatal: request was refused' }
      }
    });

    await page.locator('.graph-row').first().click();

    await expect(page.getByTestId('diff-error-label')).toHaveText('Unable to load commit diff');
    await expect(page.getByTestId('diff-error')).toContainText('fatal: request was refused');
    await expect(page.getByTestId('bottom-retry-commit-diff')).toBeVisible();
  });

  test('ends loading after a commit diff timeout and accepts a late success', async ({ page }) => {
    await page.clock.install();
    const stub = await boot(page, { open: [fixtureRepo()], commitDiffNeverResponds: true });

    await page.locator('.graph-row').first().click();
    await expect(page.getByTestId('diff-loading-label')).toBeVisible();
    await page.clock.fastForward(30_001);

    await expect(page.getByTestId('diff-error-label')).toHaveText('Unable to load commit diff');
    await expect(page.getByTestId('diff-error')).toContainText('Timed out');
    await expect(page.getByTestId('bottom-retry-commit-diff')).toBeVisible();

    const selection = (await stub.commands()).find((entry) => entry.cmd === 'select_commit')!;
    const requestId = Number((selection.args as any).requestId);
    const oid = String((selection.args as any).oid);
    await page.getByTestId('bottom-file-src/lib.rs').click();
    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'fileDiff',
      requestId,
      oid,
      file: {
        path: 'src/lib.rs',
        old_path: null,
        new_path: 'src/lib.rs',
        status: 'modified',
        old_blob: null,
        new_blob: null,
        added: 4,
        deleted: 1
      },
      document: diffPayload('src/lib.rs', 'rust')
    });

    await expect(page.getByTestId('diff-error-label')).toHaveCount(0);
    await expect(page.getByTestId('diff-hunk')).toBeVisible();
  });

  test('marks the changed characters in the inline layout', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()], diffLayout: 'inline' });

    await page.locator('.graph-row').first().click();
    await expect(page.getByTestId('diff-hunk').first()).toBeVisible();
    await expect(page.locator('.diff--inline')).toBeVisible();

    // The backend reports the changed character, so it is marked inside the
    // line. A whole-line colour would hide a one-character edit.
    const addedMarks = page.locator('.diff__row--add .diff__text mark');
    const deletedMarks = page.locator('.diff__row--del .diff__text mark');
    await expect(addedMarks).toHaveCount(2);
    await expect(deletedMarks).toHaveCount(2);
    const addedMark = addedMarks.first();
    const deletedMark = deletedMarks.first();
    await expect(addedMark).toBeVisible();
    await expect(addedMark).toHaveText('2');
    await expect(deletedMark).toBeVisible();
    await expect(deletedMark).toHaveText('1');
    await expectChangeMarkStyle(addedMark, 'base-green');
    await expectChangeMarkStyle(deletedMark, 'base-red');
  });

  test('switches to the side-by-side layout when the preference says so', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.evaluate(() => {
      (window as any).__STUB__.config.view.diff_layout = 'side-by-side';
    });

    await page.locator('.graph-row').first().click();
    await expect(page.getByTestId('diff-hunk').first()).toBeVisible();

    // A single file shows one text cell per row; the file list is what narrows
    // the view, so a commit with two files still lists both.
    await page.getByTestId('bottom-file-src/lib.rs').click();
    await expect(page.getByTestId('bottom-file-src/lib.rs')).toHaveClass(/is-selected/);
    await expect(page.getByTestId('diff-file-header')).toHaveCount(0);
    const firstRow = page.locator('[data-testid="diff-row"]').first();
    await expect(firstRow.locator('.diff__side')).toHaveCount(2);
    await expect(firstRow.locator('.diff__text')).toHaveCount(2);

    const addedMark = page.locator('.diff__side--add .diff__text mark');
    const deletedMark = page.locator('.diff__side--del .diff__text mark');
    await expect(addedMark).toHaveText('2');
    await expect(deletedMark).toHaveText('1');
    await expectChangeMarkStyle(addedMark, 'base-green');
    await expectChangeMarkStyle(deletedMark, 'base-red');
  });

  test('aligns diff columns and preserves tab and wide-character advances', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()], diffLayout: 'side-by-side' });
    await page.setViewportSize({ width: 1800, height: 1000 });
    await page.locator('.graph-row').first().click();
    await page.getByTestId('bottom-file-src/lib.rs').click();
    await expect(page.getByTestId('diff-row').first()).toBeVisible();

    const selection = (await stub.commands()).find((entry) => entry.cmd === 'select_commit')!;
    const oid = String((selection.args as any).oid);
    const requestId = Number((selection.args as any).requestId);
    const code = `\tconst label = "${'界'.repeat(100)}";`;
    const rows = [
      {
        kind: 'hunk' as const,
        old_no: null,
        new_no: null,
        old_text: null,
        new_text: null,
        old_line_index: null,
        new_line_index: null,
        hunk_header: '@@ -1 +1 @@'
      },
      ...Array.from({ length: 600 }, (_, index) => {
        const line = index + 1;
        const oldText =
          line === 1 ? code : line === 600 ? 'final wrapped-list row' : `line ${line}`;
        const newText = line === 1 ? 'short replacement' : oldText;
        return {
          kind: 'context' as const,
          old_no: line,
          new_no: line,
          old_text: oldText,
          new_text: newText,
          old_line_index: index,
          new_line_index: index,
          hunk_header: null
        };
      })
    ];
    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'fileDiff',
      requestId,
      oid,
      file: {
        path: 'src/lib.rs',
        old_path: null,
        new_path: 'src/lib.rs',
        status: 'modified',
        old_blob: null,
        new_blob: null,
        added: 1,
        deleted: 1
      },
      document: {
        path: 'src/lib.rs',
        language: 'rust',
        rows,
        aligned_rows: rows,
        old_source: null,
        new_source: null,
        inline_old: Array.from({ length: 600 }, () => []),
        inline_new: Array.from({ length: 600 }, () => []),
        binary: false,
        copy_text: ''
      }
    });

    const metrics = await page
      .locator('.diff__row--split')
      .first()
      .evaluate((row) => {
        const sides = [...row.querySelectorAll<HTMLElement>('.diff__side')];
        const codeCell = sides[0]!.querySelector<HTMLElement>('.diff__text')!;
        const gutter = sides[0]!.querySelector<HTMLElement>('.diff__gutter')!;
        const hunk = document.querySelector<HTMLElement>('.diff__hunk');
        return {
          widths: sides.map((side) => side.getBoundingClientRect().width),
          fontSize: getComputedStyle(codeCell).fontSize,
          lineHeight: getComputedStyle(row).lineHeight,
          gutterWidth: gutter.getBoundingClientRect().width,
          tabSize: getComputedStyle(codeCell).tabSize,
          scrolls: codeCell.scrollWidth > codeCell.clientWidth,
          text: codeCell.textContent,
          hunkFontSize: hunk ? getComputedStyle(hunk).fontSize : null
        };
      });
    expect(Math.abs(metrics.widths[0]! - metrics.widths[1]!)).toBeLessThanOrEqual(1);
    expect(metrics.fontSize).toBe('12px');
    expect(metrics.lineHeight).toBe('22px');
    expect(metrics.gutterWidth).toBe(42);
    expect(metrics.tabSize).toBe('4');
    expect(metrics.scrolls).toBe(true);
    expect(metrics.text).toBe(code);
    expect(metrics.hunkFontSize).toBe('11px');

    const softWrap = page.getByTestId('bottom-soft-wrap');
    await expect(softWrap).toHaveAttribute('aria-pressed', 'false');
    await expect(softWrap).toHaveText('');
    await softWrap.click();
    await expect(softWrap).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('.diff--soft-wrap')).toBeVisible();
    const wrappedMetrics = await page
      .locator('.diff__row--split')
      .first()
      .evaluate((row) => {
        const codeCell = row.querySelector<HTMLElement>('.diff__side--old .diff__text')!;
        return {
          rowHeight: row.getBoundingClientRect().height,
          textHeight: codeCell.getBoundingClientRect().height,
          newSideHeight: row.querySelector<HTMLElement>('.diff__side--new')!.getBoundingClientRect()
            .height,
          scrolls: codeCell.scrollWidth > codeCell.clientWidth
        };
      });
    expect(wrappedMetrics.rowHeight).toBeGreaterThan(22);
    expect(wrappedMetrics.textHeight).toBeGreaterThan(22);
    expect(wrappedMetrics.newSideHeight).toBe(wrappedMetrics.rowHeight);
    expect(wrappedMetrics.scrolls).toBe(false);

    await softWrap.click();
    await expect(softWrap).toHaveAttribute('aria-pressed', 'false');
    expect(
      await page
        .locator('.diff__row--split')
        .first()
        .locator('.diff__side--old .diff__text')
        .evaluate((cell) => cell.scrollWidth > cell.clientWidth)
    ).toBe(true);
    await softWrap.click();
    await expect(softWrap).toHaveAttribute('aria-pressed', 'true');

    const diffRows = page.getByTestId('diff-rows');
    await diffRows.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    await expect(page.locator('[data-testid="diff-row"]').last()).toContainText(
      'final wrapped-list row'
    );
    expect(await page.locator('[data-testid="diff-row"]').count()).toBeLessThan(100);
  });

  test('persists the soft-wrap setting after reload', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });
    await page.locator('.graph-row').first().click();
    await page.getByTestId('bottom-file-src/lib.rs').waitFor();

    await page.getByTestId('bottom-soft-wrap').click();
    await expect(page.getByTestId('bottom-soft-wrap')).toHaveAttribute('aria-pressed', 'true');

    await page.reload();
    await expect(page.getByTestId('graph')).toBeVisible();
    await page.locator('.graph-row').first().click();
    await page.getByTestId('bottom-file-src/lib.rs').waitFor();
    await expect(page.getByTestId('bottom-soft-wrap')).toHaveAttribute('aria-pressed', 'true');
  });

  test('drops a diff that arrives after the selection moved on', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await page.locator('.graph-row').first().click();
    await page.getByTestId('bottom-file-src/lib.rs').click();
    await expect(page.getByTestId('diff-file-header')).toHaveCount(0);
    await expect(page.getByTestId('diff-view')).toContainText('count += 2');
    const selection = (await stub.commands()).find((entry) => entry.cmd === 'select_commit')!;
    const requestId = Number((selection.args as any).requestId);

    // A late answer for a commit that is no longer selected must not replace
    // what is on screen.
    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'fileDiff',
      requestId: requestId - 1,
      oid: '0000000000000000000000000000000000000001',
      file: {
        path: 'src/other.rs',
        old_path: null,
        new_path: 'src/other.rs',
        status: 'modified',
        old_blob: null,
        new_blob: null,
        added: 1,
        deleted: 1
      },
      document: {
        path: 'src/other.rs',
        language: 'rust',
        rows: [
          {
            kind: 'hunk',
            old_no: null,
            new_no: null,
            old_text: null,
            new_text: null,
            hunk_header: '@@ -1 +1 @@'
          }
        ],
        aligned_rows: [],
        old_source: null,
        new_source: null,
        inline_old: [],
        inline_new: [],
        binary: false,
        copy_text: ''
      }
    });

    await expect(page.getByTestId('diff-view')).toContainText('count += 2');
    await expect(page.getByTestId('diff-view')).not.toContainText('@@ -1 +1 @@');
  });

  test('drops a late file list for a commit that is no longer selected', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    const rows = page.locator('.graph-row');
    await rows.nth(0).click();
    await expect(page.getByTestId('bottom-file-src/lib.rs')).toBeVisible();
    const oldSelection = (await stub.commands()).filter(
      (entry) => entry.cmd === 'select_commit'
    )[0]!;
    const oldOid = String((oldSelection.args as any).oid);
    const oldRequestId = Number((oldSelection.args as any).requestId);

    await rows.nth(1).click();
    await expect(page.getByTestId('bottom-panel')).toContainText(
      await rows.nth(1).locator('.graph-row__subject').innerText()
    );
    await stub.emit('augur://repo-event', {
      repoId: 7,
      type: 'commitFiles',
      requestId: oldRequestId,
      oid: oldOid,
      files: [
        {
          path: 'stale-only.rs',
          old_path: null,
          new_path: 'stale-only.rs',
          status: 'modified',
          old_blob: null,
          new_blob: null,
          added: 1,
          deleted: 0
        }
      ],
      merge_parent: null
    });

    await expect(page.getByTestId('bottom-file-stale-only.rs')).toHaveCount(0);
    await expect(page.getByTestId('bottom-panel')).not.toContainText('No commit selected');
  });

  test('clears the selection back to the placeholder', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.locator('.graph-row').first().click();
    await expect(page.getByTestId('bottom-file-src/lib.rs')).toBeVisible();

    await page.getByTestId('bottom-clear-commit').click();

    await expect(page.getByTestId('bottom-file-src/lib.rs')).toHaveCount(0);
    await expect(page.getByTestId('bottom-panel')).toContainText('No commit selected');
  });

  test('loads a working-tree diff and says which side it is', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId('changes-file-src/main.rs').click();

    await expect(page.getByTestId('bottom-panel')).toContainText('Staged');
    await expect(page.getByTestId('diff-hunk')).toBeVisible();
    await expect(page.getByTestId('diff-view')).toContainText('count += 2');
  });

  test('labels an unstaged file as a working-tree change', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId('changes-file-src/git/graph.rs').click();

    await expect(page.getByTestId('bottom-panel')).toContainText('Changes');
  });

  test('shows commit actions in the requested order', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await rightClick(page, '.graph-row');

    const menuItems = await page
      .locator('.context-menu [role="menuitem"]')
      .evaluateAll((items) => items.map((item) => item.getAttribute('data-testid')));
    expect(menuItems).toEqual([
      'context-show-message',
      'context-checkout',
      'context-copy-message',
      'context-copy-oid'
    ]);
  });

  test('copies the commit message to the clipboard', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    await rightClick(page, '.graph-row');
    await page.getByTestId('context-copy-message').click();

    // The worker runs the command so the message is Git's own rendering, and
    // the result goes to the clipboard rather than the status line.
    const actions = (await stub.commands()).filter((entry) => entry.cmd === 'run_action');
    expect(actions).toHaveLength(1);
    expect((actions[0]!.args as any).action).toMatchObject({
      action: 'copyCommitMessage'
    });

    // The stub answers with a failure for this label so the reporting path is
    // observable; a success would put the message on the clipboard.
    await expect(page.getByTestId('status-message')).toContainText('Failed to copy commit message');
  });

  test('keeps the hover preview out of desktop mode', async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });

    // The desktop list already shows the subject, the author, and the date, so
    // hovering a row stays passive: no preview, and no message fetched for it.
    await page.locator('.graph-row').first().hover();
    await expect(page.getByTestId('commit-preview')).toHaveCount(0);
    expect(
      await stub
        .commands()
        .then((all) => all.filter((entry) => entry.cmd === 'request_commit_message'))
    ).toHaveLength(0);
  });

  test('shows the commit message dialog on request', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    // A right click inside the row opens the context menu, exactly as it does
    // in the application.
    const row = page.locator('.graph-row').first();
    const box = (await row.boundingBox())!;
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2, {
      button: 'right'
    });
    await expect(page.getByTestId('context-show-message')).toBeVisible();
    await page.getByTestId('context-show-message').click();

    const dialog = page.getByTestId('commit-message-dialog');
    await expect(dialog).toBeVisible();
    // The message is fetched on demand and rendered in full.
    await expect(page.getByTestId('commit-message-body')).toContainText(
      'Add the Tauri command surface'
    );
    // The dialog identifies the commit by hash and names its author and date,
    // which the collapsed row cannot show.
    await expect(dialog.locator('.commit-preview__hash')).toHaveText('13c6ef3');
    await expect(page.getByTestId('commit-message-author')).toHaveText('Author Lihao');
    await expect(page.getByTestId('commit-message-date')).toContainText('Date ');
    // The co-author trailer is part of the message and is listed separately
    // from the body.
    await expect(page.getByTestId('commit-message-coauthors')).toContainText('Co-authored-by');
    await expect(page.getByTestId('commit-message-coauthors')).toContainText('ada@example.com');

    await page.getByTestId('commit-message-close').click();
    await expect(page.getByTestId('commit-message-dialog')).toHaveCount(0);
  });

  test('opens the context menu at the cursor after scrolling deep', async ({ page }) => {
    await boot(page, { open: [longFixtureRepo()] });

    // A deep scroll is what used to fling the menu far from the cursor: the
    // fixed-position menu was laid out against the virtual list's transformed
    // window instead of the viewport.
    const list = page.getByTestId('graph-list');
    await list.evaluate((element) => {
      element.scrollTop = 72 * 36;
    });
    await expect.poll(() => list.evaluate((element) => element.scrollTop)).toBe(72 * 36);

    // The first fully visible row after that scroll. Rows are matched by
    // subject because DOM order is the rendered window, not the item index.
    const row = page.locator('.graph-row', { hasText: 'Long history commit 74' });
    await expect(row).toBeVisible();
    const box = (await row.boundingBox())!;
    const cursor = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
    await page.mouse.click(cursor.x, cursor.y, { button: 'right' });

    const menu = page.locator('.context-menu');
    await expect(menu).toBeVisible();
    const menuBox = (await menu.boundingBox())!;
    expect(Math.abs(menuBox.x - cursor.x)).toBeLessThanOrEqual(2);
    expect(Math.abs(menuBox.y - cursor.y)).toBeLessThanOrEqual(2);
    expect(menuBox.y + menuBox.height).toBeLessThanOrEqual(800);

    // Near the bottom edge the menu flips up instead of leaving the window.
    // The event is dispatched manually because locator.dispatchEvent drops
    // pointer coordinates, and no graph row sits near the window bottom in the
    // default layout.
    await page.evaluate(() => {
      const row = [...document.querySelectorAll<HTMLElement>('.graph-row')].find((element) =>
        element.textContent?.includes('Long history commit 74')
      );
      row?.dispatchEvent(
        new MouseEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          clientX: 640,
          clientY: 780
        })
      );
    });
    await expect
      .poll(() => menu.boundingBox().then((value) => value!.y + value!.height))
      .toBeLessThanOrEqual(792);
    const flipped = (await menu.boundingBox())!;
    expect(flipped.y).toBeLessThan(780);
  });

  test("copies a commit's diff from the button and the keyboard", async ({ page }) => {
    const stub = await boot(page, { open: [fixtureRepo()] });
    await page.locator('.graph-row').first().click();
    await page.getByTestId('diff-hunk').first().waitFor();

    // The commit's totals are in the header, so the size of the change is
    // readable without summing the file list.
    await expect(page.getByTestId('bottom-commit-stat-added')).toHaveText('+124');
    await expect(page.getByTestId('bottom-commit-stat-deleted')).toHaveText('-1');

    // A copy is available even for a single file, because a pasted hunk with no
    // file in it cannot be pasted anywhere useful.
    await expect(page.getByTestId('bottom-copy-diff')).toBeVisible();
    await page.getByTestId('bottom-copy-diff').click();
    const writes = async () =>
      page.evaluate(() =>
        (window as any).__STUB__.log.filter(
          (entry: any) => entry.cmd === 'plugin:clipboard-manager|write_text'
        )
      );
    await expect.poll(async () => (await writes()).length).toBeGreaterThan(0);
    const copied = String((await writes()).at(-1)!.args.text);
    // Every document names its own file, in whatever order the commit lists
    // them.
    expect(copied).toMatch(/^diff -- src\//m);
    expect(copied.split('diff -- ').length - 1).toBe(2);

    // The same gesture from the keyboard.
    const before = (await writes()).length;
    await page.getByTestId('diff-view').click();
    await page.keyboard.press('Meta+c');
    await expect.poll(async () => (await writes()).length).toBeGreaterThan(before);
  });

  test('names the file and offers a copy in the working-tree view', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.getByTestId('changes-file-src/main.rs').first().click();
    await page.getByTestId('diff-hunk').first().waitFor();

    // The working-tree view has no commit to name, so it names the file.
    await expect(page.getByTestId('bottom-working-path')).toHaveText('src/main.rs');
    await expect(page.getByTestId('bottom-copy-diff')).toBeVisible();
  });

  test('marks HEAD with a filled disc and the rest with rings', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    // The nodes carry no labels, so the tip of the history has to be findable
    // from the shape alone.
    const row = page.locator('.graph-row').first();
    const headFill = await row.locator('svg circle').first().getAttribute('fill');
    const otherFill = await page
      .locator('.graph-row')
      .nth(1)
      .locator('svg circle')
      .first()
      .getAttribute('fill');
    expect(headFill).not.toBe('var(--background)');
    expect(otherFill).toBe('var(--background)');
  });

  test('clears the selection when the filter hides it', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });

    await page.locator('.graph-row').first().click();
    await expect(page.getByTestId('diff-hunk').first()).toBeVisible();

    // A query that matches nothing removes the row the diff belongs to, so the
    // panel must not keep showing a commit the list no longer contains.
    await page.getByTestId('commit-search').fill('nothing matches this');
    await expect(page.getByTestId('commit-search-no-results')).toBeVisible();
    // The panel stays mounted with its placeholder, as the reference does, but
    // the diff is gone.
    await expect(page.getByTestId('diff-hunk')).toHaveCount(0);
    await expect(page.getByTestId('bottom-panel')).toContainText('No commit selected');

    // Restoring the query does not resurrect the selection.
    await page.getByTestId('commit-search').fill('');
    await expect(page.locator('.graph-row')).toHaveCount(8);
    await expect(page.getByTestId('diff-hunk')).toHaveCount(0);
  });

  test('names each column in a header that tracks the rows', async ({ page }) => {
    await boot(page, { open: [fixtureRepo()] });
    await page.setViewportSize({ width: 1280, height: 800 });

    // The header uses the same widths as the rows, so a label always sits over
    // the column it names.
    for (const column of ['graph', 'hash', 'message', 'author', 'date']) {
      await expect(page.getByTestId(`graph-header-${column}`)).toBeVisible();
    }
    await expect(page.getByTestId('graph-header-hash')).toHaveText('Hash');

    // Narrowing removes the author label and then the message label, in step
    // with the columns themselves.
    await page.setViewportSize({ width: 1000, height: 800 });
    await expect(page.getByTestId('graph-header-author')).toHaveCount(0);
    await expect(page.getByTestId('graph-header-message')).toBeVisible();
    await page.setViewportSize({ width: 800, height: 800 });
    await expect(page.getByTestId('graph-header-message')).toHaveCount(0);
    // The date's threshold already accounts for it, so it never goes away.
    await expect(page.getByTestId('graph-header-date')).toBeVisible();
  });

  test('hides the author and then the message when the window narrows', async ({ page }) => {
    await boot(page, { open: [secondFixtureRepo()] });

    // The lane area is 12 + 24 * lanes + 8, and the two columns need their own
    // widths on top of the message minimum. The thresholds are the backend's,
    // so this asserts the whole column rule rather than a duplicated formula.
    const thresholds = await page.evaluate(() =>
      (window as any).__STUB__.log
        .filter((entry: any) => entry.cmd === 'column_visibility')
        .map((entry: any) => entry.args)
    );
    expect((thresholds as unknown[]).length).toBeGreaterThan(0);

    // At the default width both optional columns are present.
    await expect(page.locator('.graph-row__author').first()).toBeVisible();
    await expect(page.locator('.graph-row__subject').first()).toBeVisible();

    // The author goes first, because its threshold is the higher of the two.
    await page.setViewportSize({ width: 1000, height: 800 });
    await expect(page.locator('.graph-row__author')).toHaveCount(0);
    await expect(page.locator('.graph-row__subject').first()).toBeVisible();

    // The subject goes next. The date and the hash stay: the threshold for the
    // subject already accounts for the date, and the hash identifies the row.
    await page.setViewportSize({ width: 800, height: 800 });
    await expect(page.locator('.graph-row__subject')).toHaveCount(0);
    await expect(page.locator('.graph-row__date').first()).toBeVisible();
    await expect(page.locator('.graph-row__hash').first()).toBeVisible();
  });

  test('filters the graph by commit message', async ({ page }) => {
    await boot(page, { open: [secondFixtureRepo()] });
    await expect(page.locator('.graph-row')).toHaveCount(1);

    await page.getByTestId('commit-search').fill('nothing matches this');
    await expect(page.getByTestId('commit-search-no-results')).toBeVisible();
    await expect(page.locator('.graph-row')).toHaveCount(0);

    await page.getByTestId('commit-search').fill('readme');
    await expect(page.getByTestId('commit-search-results')).toContainText('1 / 1');
    await expect(page.locator('.graph-row')).toHaveCount(1);
  });
});

async function expectChangeMarkStyle(
  mark: import('@playwright/test').Locator,
  themeColor: 'base-green' | 'base-red'
) {
  await expect(mark).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(mark).toHaveCSS('box-shadow', /inset/);
  await expect(mark).toHaveClass(/token-number/);

  const colors = await mark.evaluate((marked, name) => {
    const plain = document.querySelector('.diff__text .token-number:not(mark)');
    if (!marked || !plain) return null;
    return {
      accent: getComputedStyle(marked).getPropertyValue('--diff-change-accent').trim(),
      expected: getComputedStyle(document.documentElement).getPropertyValue(`--${name}`).trim(),
      markedText: getComputedStyle(marked).color,
      plainText: getComputedStyle(plain).color
    };
  }, themeColor);

  expect(colors).not.toBeNull();
  expect(colors?.accent).toBe(colors?.expected);
  expect(colors?.markedText).toBe(colors?.plainText);
}

test.describe('commit message editor', () => {
  test('offers the last commit message from the mode menu once Amend is chosen', async ({
    page
  }) => {
    await boot(page, { open: [fixtureRepo()] });

    // A plain commit has nothing to reuse, so the item is present but inert.
    await page.getByTestId('commit-mode-trigger').click();
    await expect(page.getByTestId('commit-mode-fill-last-message')).toBeDisabled();
    await page.getByTestId('commit-mode-amend').click();

    await page.getByTestId('commit-mode-trigger').click();
    await page.getByTestId('commit-mode-fill-last-message').click();
    await expect(page.getByTestId('commit-message')).toHaveValue('Add the Tauri command surface');
  });
});
