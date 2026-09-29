/**
 * The vocabulary the stub backend and the tests share.
 *
 * Its own module because the runtime below is serialised into the page, where
 * this file does not exist: anything the runtime names has to be a type, which
 * the compiler erases, rather than a value.
 */

/** One repository the stub backend pretends to have open. */
export interface StubRepo {
  id: number;
  path: string;
  location: { kind: 'local' } | { kind: 'wsl'; distro: string };
  status: {
    branch: string;
    head: string | null;
    upstream: string | null;
    ahead: number;
    behind: number;
    files: StubFile[];
    diff_stats: {
      staged: { added: number; deleted: number } | null;
      unstaged: { added: number; deleted: number } | null;
      untracked: { added: number; deleted: number } | null;
    };
    branches: { name: string; is_head: boolean }[];
  };
  refs: StubRefs;
  rows: StubLogRow[];
}

export interface StubFile {
  index: string;
  worktree: string;
  path: string;
  old_path: string | null;
}

export interface StubRefs {
  remotes: string[];
  remote_branches: string[];
  tags: string[];
  stashes: { reference: string; description: string }[];
  comparison_revisions: {
    name: string;
    full_name: string;
    kind: 'local' | 'remote' | 'tag' | 'commit';
  }[];
}

export interface StubLogRow {
  oid: string;
  short: string;
  author: string;
  date: string;
  timestamp: number;
  subject: string;
  message: string;
  decorations: string;
  parents: string[];
}
