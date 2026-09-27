import { describe, expect, it } from "vitest";

import { codeFor, isConflicted, isStaged, isUntracked } from "../app/repoState";
import type { FileStatus } from "../bridge/types";

function file(index: string, worktree: string, path = "src/main.rs"): FileStatus {
  return { index, worktree, path, old_path: null };
}

describe("porcelain status classification", () => {
  it("recognizes a staged change", () => {
    expect(isStaged(file("M", " "))).toBe(true);
    expect(isStaged(file("A", "M"))).toBe(true);
    expect(isStaged(file(" ", "M"))).toBe(false);
    expect(isStaged(file("?", "?"))).toBe(false);
  });

  it("recognizes an untracked file", () => {
    expect(isUntracked(file("?", "?"))).toBe(true);
    expect(isUntracked(file("?", "M"))).toBe(false);
  });

  it("recognizes every unmerged spelling", () => {
    for (const [index, worktree] of [
      ["U", " "],
      [" ", "U"],
      ["D", "D"],
      ["A", "A"],
    ] as const) {
      expect(isConflicted(file(index, worktree)), `${index}${worktree}`).toBe(true);
      // A conflicted entry is never staged or unstaged: it belongs to its own
      // group in the changes panel.
      expect(isStaged(file(index, worktree))).toBe(false);
    }
  });

  it("picks the character that applies to the requested side", () => {
    const both = file("M", "M");
    expect(codeFor(both, true)).toBe("M");
    expect(codeFor(both, false)).toBe("M");

    const stagedOnly = file("A", " ");
    expect(codeFor(stagedOnly, true)).toBe("A");
    // The unstaged side has nothing, so the aggregate code is used.
    expect(codeFor(stagedOnly, false)).toBe("A");

    const worktreeOnly = file(" ", "D");
    expect(codeFor(worktreeOnly, false)).toBe("D");
    expect(codeFor(worktreeOnly, true)).toBe("D");
  });
});
