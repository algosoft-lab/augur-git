/**
 * The repositories, commits, and diffs the stub backend serves.
 *
 * Deterministic on purpose: an object id is derived from a row's index rather
 * than generated, so a test can name a commit's short hash and have it match.
 */

import type { StubLogRow, StubRepo } from './stubTypes';

/** A distinct, deterministic object id for a fixture commit. */
const oid = (seed: number) => {
  const head = (0x9e3779b1 * (seed + 1)).toString(16).padStart(8, '0');
  return (head + seed.toString(16).padStart(4, '0')).padEnd(40, '0').slice(0, 40);
};

function logRow(
  seed: number,
  subject: string,
  author: string,
  minutesAgo: number,
  parents: number[],
  decorations: string,
  body = ''
): StubLogRow {
  return {
    oid: oid(seed),
    short: oid(seed).slice(0, 7),
    author,
    date: '2026-09-27 10:00',
    timestamp: Math.floor(Date.now() / 1000) - minutesAgo * 60,
    subject,
    message: body ? `${subject}\n\n${body}` : subject,
    decorations,
    parents: parents.map(oid)
  };
}

/**
 * The fixture repository.
 *
 * The topology is a trunk with one side branch merged back in, so the graph
 * layout has a lane change, a merge node, and three parents to lay out.
 */
export function fixtureRepo(): StubRepo {
  return {
    id: 7,
    path: '/Users/dev/projects/augur-git',
    location: { kind: 'local' },
    status: {
      branch: 'master',
      head: oid(1),
      upstream: 'origin/master',
      ahead: 2,
      behind: 1,
      files: [
        { index: 'M', worktree: ' ', path: 'src/main.rs', old_path: null },
        // Staged and modified again: two diffs, so it belongs in both groups.
        { index: 'M', worktree: 'M', path: 'src/partial.rs', old_path: null },
        { index: 'A', worktree: ' ', path: 'src/git/worker.rs', old_path: null },
        { index: ' ', worktree: 'M', path: 'src/git/graph.rs', old_path: null },
        { index: '?', worktree: '?', path: 'notes.md', old_path: null },
        { index: 'R', worktree: 'R', path: 'src/old.rs', old_path: 'src/new.rs' },
        {
          index: 'U',
          worktree: 'U',
          path: 'src/conflict.rs',
          old_path: null
        }
      ],
      diff_stats: {
        staged: { added: 12, deleted: 3 },
        unstaged: { added: 8, deleted: 4 },
        untracked: { added: 5, deleted: 0 }
      },
      branches: [
        { name: 'master', is_head: true },
        { name: 'feature/tauri', is_head: false },
        { name: 'release', is_head: false }
      ]
    },
    refs: {
      remotes: ['origin'],
      remote_urls: [{ name: 'origin', url: 'https://example.com/augur-git.git' }],
      remote_branches: ['origin/master', 'origin/feature/tauri'],
      tags: ['v1.0.0', 'v1.1.0'],
      stashes: [{ reference: 'stash@{0}', description: 'WIP on master: 9ab1c2d' }],
      comparison_revisions: [
        { name: 'master', full_name: 'refs/heads/master', kind: 'local' },
        {
          name: 'feature/tauri',
          full_name: 'refs/heads/feature/tauri',
          kind: 'local'
        },
        { name: 'origin/master', full_name: 'refs/remotes/origin/master', kind: 'remote' },
        { name: 'v1.1.0', full_name: 'refs/tags/v1.1.0', kind: 'tag' }
      ]
    },
    rows: [
      logRow(
        1,
        'Add the Tauri command surface',
        'Lihao',
        12,
        [2, 3],
        'HEAD -> master, origin/master, tag: v1.1.0',
        'The command layer is what the webview calls into.\n\nIt stays a thin wrapper.'
      ),
      logRow(4, 'Extract the domain crate', 'Lihao', 90, [5], 'feature/tauri'),
      logRow(2, 'Add the event protocol', 'Ada', 240, [6], ''),
      logRow(3, 'Widen the log page size', 'Ada', 300, [6], ''),
      logRow(5, 'Start the core extraction', 'Lihao', 1500, [7], ''),
      logRow(6, 'Introduce the snapshot store', 'Grace', 2600, [8], ''),
      logRow(7, 'Bootstrap the project', 'Grace', 5000, [8], ''),
      logRow(8, 'First commit', 'Grace', 9000, [], '')
    ]
  };
}

/** A second repository, used to prove the tab list keeps state per tab. */
export function secondFixtureRepo(): StubRepo {
  const repo = fixtureRepo();
  return {
    ...repo,
    id: 9,
    path: '/Users/dev/projects/other-app',
    status: {
      ...repo.status,
      branch: 'trunk',
      head: oid(11),
      upstream: null,
      ahead: 0,
      behind: 0,
      files: [{ index: ' ', worktree: 'M', path: 'README.md', old_path: null }],
      branches: [{ name: 'trunk', is_head: true }]
    },
    rows: [logRow(11, 'Update the readme', 'Ada', 30, [12], 'HEAD -> trunk')]
  };
}

/**
 * A repository with a long linear history, so graph tests can scroll far past
 * the first window of rows.
 */
export function longFixtureRepo(commits = 400): StubRepo {
  const repo = fixtureRepo();
  const rows: StubLogRow[] = Array.from({ length: commits }, (_, index) =>
    logRow(
      index + 20,
      `Long history commit ${index + 1}`,
      'Lihao',
      30 * (index + 1),
      index === commits - 1 ? [] : [index + 21],
      index === 0 ? 'HEAD -> master' : ''
    )
  );
  return {
    ...repo,
    rows,
    status: { ...repo.status, head: rows[0].oid }
  };
}

/** The file list of the newest commit, so the commit panel has content. */
export function commitFiles(): Record<string, unknown>[] {
  return [
    {
      path: 'src/lib.rs',
      old_path: null,
      new_path: 'src/lib.rs',
      status: 'modified',
      old_blob: null,
      new_blob: null,
      added: 4,
      deleted: 1
    },
    {
      path: 'src/commands/repo.rs',
      old_path: null,
      new_path: 'src/commands/repo.rs',
      status: 'added',
      old_blob: null,
      new_blob: null,
      added: 120,
      deleted: 0
    }
  ];
}

/** A diff payload with a hunk, an addition, a deletion, and an inline range. */
export function diffPayload(path: string, language: string | null): Record<string, unknown> {
  const rows = [
    {
      kind: 'hunk',
      old_no: null,
      new_no: null,
      old_text: null,
      new_text: null,
      hunk_header: '@@ -10,6 +10,7 @@ fn run()'
    },
    {
      kind: 'context',
      old_no: 10,
      new_no: 10,
      old_text: '    let mut count = 0;',
      new_text: '    let mut count = 0;'
    },
    {
      kind: 'del',
      old_no: 11,
      new_no: null,
      old_text: '    count += 1;',
      new_text: null
    },
    {
      kind: 'add',
      old_no: null,
      new_no: 11,
      old_text: null,
      new_text: '    count += 2;'
    }
  ];
  return {
    path,
    language,
    rows,
    aligned_rows: rows,
    old_source: null,
    new_source: null,
    // The changed character is the `1` becoming `2`, which is the point of the
    // inline layout.
    inline_old: [[], [], [], []],
    inline_new: [[], [{ start: 13, end: 14 }]],
    binary: false,
    copy_text: `diff --git a/${path} b/${path}\n@@ -10,6 +10,7 @@\n-    count += 1;\n+    count += 2;\n`
  };
}
