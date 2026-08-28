# UX Optimization: Commit Review and Working Tree Layout

## Goal

Keep the commit history readable in the center while making the right panel a
single source-control workspace. The persistent commit details surface is
removed because it duplicates information already present in the graph and
competes with the working-tree workflow.

## Information architecture

```text
Left: repository navigator
  Branches / Remotes / Remote branches / Tags / Stashes

Center: history and selected-commit diff
  Commit graph
  Changed files and diff for the selected commit

Right: source-control workspace
  Always-visible commit message editor
  Working Tree
    Staged
    Changes
```

The right panel keeps its existing resizable width. The left panel no longer
owns working-tree state, so repository navigation and source-control actions
have clearer ownership.

## Interaction behavior

- Hovering a commit row for 0.5 seconds opens a tooltip containing the full
  parsed commit message, including the subject, body, co-authors, author, date,
  hash, and ref decorations.
- The full message is requested on the first hover and cached by commit OID.
  If the request is still in flight when the tooltip opens, the tooltip shows a
  loading state and updates in place when the response arrives.
- Clicking a commit row still selects it and keeps the existing selected-commit
  file list and diff behavior in the center-bottom panel.
- The commit editor is always expanded at the top of the right panel. Its
  existing Enter/Shift+Enter, commit/amend, and staged-change gating behavior
  is preserved.
- Staged and unstaged files are grouped below the editor. Group headers remain
  collapsible, the list is independently scrollable, file status colors remain
  visible, and the selected file remains highlighted.
- An empty working tree shows a compact empty state instead of empty groups.

## Implementation boundaries

- `GraphView` owns hover state, message caching, and row interaction.
- `CommitHoverPreview` owns the live tooltip rendering and asynchronous
  message state.
- `ChangesPanel` owns staged/unstaged grouping, collapse state, selection, and
  locale updates.
- `RepoTab` coordinates Git snapshots and routes commit-message responses to
  the graph preview.
- `DetailPanel` and its dedicated detail translations are removed from the
  active UI path.

## Acceptance checks

1. The right panel shows the commit editor without a collapse control.
2. A repository with staged and unstaged files shows both groups below the
   editor; a clean repository shows the empty state.
3. The left panel contains repository refs but no staged/changes groups.
4. A commit row held for at least 0.5 seconds shows its full message; moving to
   another row replaces the preview, and leaving the row hides it.
5. Selecting a commit still loads its changed-file list and diff.
6. Switching locale updates the editor, working-tree panel, and hover preview.
