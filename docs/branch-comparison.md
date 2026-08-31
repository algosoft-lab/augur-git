# Revision comparison

augur-git provides a read-only comparison workspace for two local revisions.
The endpoints can be local branches, remote-tracking branches, tags, commits
from the loaded history, or a short/full commit SHA entered manually.
Comparing revisions never checks out a ref, fetches from a remote, or changes
`HEAD`, the index, or the working tree.

## Comparison semantics

Every comparison has an explicit direction:

```text
Base commit → Target commit
```

The selected endpoints are resolved to stable object IDs before file metadata
is read. If a revision moves while the view is loading, the active comparison
continues to use the original snapshots. The comparison is always a direct
tip-to-tip diff; there is no merge-base mode.

The revision selectors contain local and remote-tracking refs, tags, and the
latest commit-log snapshot. The graph currently loads a bounded history
window, so older commits may be selected by entering their 7–64 character
hexadecimal SHA. SHA input is resolved only against the repository's existing
object database; the comparison never performs an implicit fetch.

The comparison engine lives in `src/core/git/branch_compare.rs`. The existing
Git worker only carries the request, generation token, and event forwarding.
This keeps diff parsing and rendering out of the worker coordinator. UI
coordination remains isolated in `src/workspace/repo_tab/branch_compare.rs`.

## Loading and cancellation

The worker first sends complete changed-file metadata, then streams each file's
patch and source blobs. The view initially shows **All changed files** and
updates as documents arrive. It displays aggregate additions/deletions and the
number of files loaded so far. A file can be selected independently, and the
existing inline and side-by-side renderers are reused. Side-by-side mode
automatically falls back to inline below the narrow-window threshold.

Each comparison has a generation ID. Starting another comparison, changing a
selection, leaving the view, or closing the repository invalidates the previous
generation. The read-only comparison worker checks the generation between
files, while the UI ignores events from older generations. A failure to read
one file is shown on that file; revision resolution failures are shown as
comparison-level errors without stopping the repository Git worker.

## Manual verification

1. Open a repository with `main`, `feature-a`, and `feature-b`; keep `main`
   checked out and compare the two feature branch tips.
2. Compare a local branch with an existing `origin/*` remote-tracking branch
   and a tag; verify that no checkout or fetch occurs.
3. Select two commits from the revision lists, then compare a full SHA and a
   short SHA entered in the endpoint fields. Use commits that are not checked
   out and, if possible, older than the visible history window.
4. Swap Base/Target; additions and deletions should reverse with the direction.
5. Check a rename, binary file, Unicode path, empty diff, and a large diff
   while starting a second comparison before the first finishes.
6. Try an invalid SHA and a non-commit object; verify that a comparison-level
   error is shown and the repository view remains usable.
7. Switch between all-files and single-file views, copy the diff, change the
   configured diff layout, and return to the repository workspace.
