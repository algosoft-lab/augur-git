/**
 * Group remote-tracking branches under their remote.
 *
 * A remote whose name contains a slash, such as `foo/bar`, has to beat a
 * hypothetical `foo`, so the longest matching remote wins. Names without a
 * slash fall into a catch-all group, and symbolic `remote/HEAD` aliases are
 * skipped because they only duplicate the branch they point at.
 */

export interface RemoteBranchEntry {
  /** Full short name as produced by `git branch -r`, e.g. `origin/main`. */
  fullName: string;
  /** Display label with the remote prefix removed, e.g. `main`. */
  label: string;
}

export interface RemoteBranchGroup {
  remote: string;
  branches: RemoteBranchEntry[];
}

const OTHER_GROUP = "(other)";

export function groupRemoteBranches(
  remotes: string[],
  branches: string[],
): RemoteBranchGroup[] {
  const groups = new Map<string, RemoteBranchEntry[]>();
  for (const remote of remotes) {
    groups.set(remote, []);
  }
  for (const branch of branches) {
    if (branch.endsWith("/HEAD")) {
      continue;
    }
    const matched = matchRemote(remotes, branch);
    const [remote, label] = matched
      ? [matched[0], matched[1]]
      : branch.includes("/")
        ? [branch.slice(0, branch.indexOf("/")), branch.slice(branch.indexOf("/") + 1)]
        : [OTHER_GROUP, branch];
    const bucket = groups.get(remote) ?? [];
    bucket.push({ fullName: branch, label });
    groups.set(remote, bucket);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([remote, entries]) => ({
      remote,
      branches: [...entries].sort((a, b) => (a.label < b.label ? -1 : 1)),
    }));
}

function matchRemote(
  remotes: string[],
  branch: string,
): [string, string] | null {
  let best: [string, string] | null = null;
  for (const remote of remotes) {
    if (!branch.startsWith(`${remote}/`)) {
      continue;
    }
    if (!best || remote.length > best[0].length) {
      best = [remote, branch.slice(remote.length + 1)];
    }
  }
  return best;
}
