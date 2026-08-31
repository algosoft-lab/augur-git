# Branch comparison

augur-git provides a read-only comparison workspace for any two local or
remote-tracking branches known to the repository. Comparing branches never
checks out either ref and does not change `HEAD`, the index, or the working
tree.

## Comparison semantics

The comparison controls expose two explicit modes:

- **Compare tips** compares the resolved commit at the Base ref with the
  resolved commit at the Target ref.
- **Since common ancestor** resolves `merge-base(Base, Target)` and compares
  that commit with the Target commit. This is equivalent to the changes that
  the Target branch introduces after the branches diverged.

Both refs are resolved to object IDs before file metadata is read. This keeps a
comparison stable if a ref moves while the view is loading. Existing local
remote-tracking refs are used as-is; the comparison does not fetch.

The comparison engine lives in `src/core/git/branch_compare.rs`; the existing
Git worker only carries the request, generation token, and event forwarding.
This keeps the already large worker coordinator from gaining diff parsing or
rendering responsibilities. UI coordination is similarly isolated in
`src/workspace/repo_tab/branch_compare.rs`.

## Loading and cancellation

The worker first sends the complete changed-file metadata, then streams each
file's patch and source blobs. The view initially shows **All changed files**
and updates as documents arrive. It displays aggregate additions/deletions and
the number of files loaded so far. A file can be selected independently, and
the existing inline and side-by-side renderers are reused. Side-by-side mode
automatically falls back to inline below the narrow-window threshold.

Each comparison has a generation ID. Starting another comparison, changing a
selection, leaving the view, or closing the repository invalidates the previous
generation. The read-only comparison worker checks the generation between
files, while the UI ignores events from older generations. A failure to read
one file is shown on that file; ref resolution and merge-base failures are
shown as comparison-level errors without stopping the repository Git worker.

## Manual verification

1. Open a repository with `main`, `feature-a`, and `feature-b`; keep `main`
   checked out and compare `feature-a` to `feature-b`.
2. Compare a local branch with an existing `origin/*` remote-tracking branch;
   verify that no checkout or fetch occurs.
3. Use both comparison modes and swap Base/Target; additions and deletions
   should reverse with the direction.
4. Check a rename, binary file, Unicode path, empty diff, and a large diff
   while starting a second comparison before the first finishes.
5. Switch between all-files and single-file views, copy the diff, change the
   configured diff layout, and return to the repository workspace.
