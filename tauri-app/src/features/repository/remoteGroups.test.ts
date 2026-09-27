import { describe, expect, it } from "vitest";

import { groupRemoteBranches } from "./remoteGroups";

const names = (items: string[]) => items;

describe("remote branch grouping", () => {
  it("groups by known remote and sorts by label", () => {
    const groups = groupRemoteBranches(
      names(["origin", "upstream"]),
      names(["upstream/dev", "origin/main", "origin/feature/x", "origin/a"]),
    );
    expect(groups.map((group) => group.remote)).toEqual(["origin", "upstream"]);
    expect(groups[0]!.branches.map((entry) => entry.label)).toEqual([
      "a",
      "feature/x",
      "main",
    ]);
    expect(groups[1]!.branches[0]!.fullName).toBe("upstream/dev");
  });

  it("keeps a remote with no branches visible", () => {
    const groups = groupRemoteBranches(names(["origin", "empty"]), names([]));
    expect(groups.map((group) => group.remote)).toEqual(["empty", "origin"]);
  });

  it("prefers the longest matching remote so a slashed name groups correctly", () => {
    const groups = groupRemoteBranches(
      names(["foo", "foo/bar"]),
      names(["foo/bar/topic"]),
    );
    expect(groups).toHaveLength(2);
    const target = groups.find((group) => group.remote === "foo/bar");
    expect(target?.branches[0]?.label).toBe("topic");
  });

  it("falls back to the first path segment for an unknown remote", () => {
    const groups = groupRemoteBranches(names([]), names(["upstream/main"]));
    expect(groups[0]!.remote).toBe("upstream");
    expect(groups[0]!.branches[0]!.label).toBe("main");
  });

  it("collects a name without a slash under the catch-all group", () => {
    const groups = groupRemoteBranches(names([]), names(["loose"]));
    expect(groups[0]!.remote).toBe("(other)");
  });

  it("skips the symbolic remote HEAD alias", () => {
    const groups = groupRemoteBranches(names(["origin"]), names(["origin/HEAD", "origin/main"]));
    expect(groups[0]!.branches).toHaveLength(1);
  });
});
