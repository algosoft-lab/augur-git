# Revision comparison

augur-git provides a read-only comparison workspace for two repository revisions.
The endpoints can be local branches, remote-tracking branches, tags, commits
from the loaded history, or a short/full commit SHA entered manually.
Comparing revisions never checks out a ref, fetches from a remote, or changes
`HEAD`, the index, or the working tree.

Selecting Compare opens the comparison in a separate native application window.
The repository window stays available, so revisions or commit IDs can be copied
from the main view and pasted into either comparison field.

## Comparison semantics

Every comparison has an explicit direction:

```text
Base commit → Target commit
```

The selected endpoints are resolved to stable object IDs before file metadata
is read. If a revision moves while the view is loading, the active comparison
continues to use the original snapshots. The comparison is always a direct
tip-to-tip diff; there is no merge-base mode.

Each endpoint is one editable revision picker. Its popup groups local and
remote-tracking branches, tags, and the latest commit-log snapshot. Typing
filters names, refs, abbreviated object IDs, and commit subjects. Selecting a
candidate writes its compact name into the field. The graph currently loads a
bounded history window, so older commits can still be selected by pasting their
7–64 character hexadecimal SHA; a **Use commit SHA** action is offered when that
SHA is not in the visible list. The SHA is resolved only against the
repository's existing object database, so the comparison never performs an
implicit fetch. Enable **Manual input** below either field when pasting or
typing a revision without opening the candidate popup; unchecking it restores
the searchable suggestions.

Empty or malformed text leaves the endpoint invalid and disables Compare until
it is corrected. Editing either field only changes the pending endpoints;
comparison starts after the user presses Compare. A refreshed refs or log
snapshot does not overwrite text currently being edited or a manually entered
SHA. If a selected ref disappears, it remains displayed and Git reports a
comparison-level resolution error if the user starts a new comparison.

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
3. Select two commits from the grouped revision pickers, then paste a full SHA
   into one picker and a short SHA into the other. Use commits that are not
   checked out and, if possible, older than the visible history window.
4. Swap Base/Target; additions and deletions should reverse with the direction.
5. Check a rename, binary file, Unicode path, empty diff, and a large diff
   while starting a second comparison before the first finishes.
6. Try an invalid SHA and a non-commit object; verify that a comparison-level
   error is shown and the repository view remains usable.
7. Switch between all-files and single-file views, copy the diff, change the
   configured diff layout, and close the comparison window.
