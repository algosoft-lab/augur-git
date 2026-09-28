import { describe, expect, it } from "vitest";

import type { CompareRevision } from "../../bridge/types";
import { filterRevisions, isRevisionUnavailable } from "./revisions";

const local = (name: string): CompareRevision => ({
  name,
  full_name: `refs/heads/${name}`,
  kind: "local",
});
const commit = (oid: string): CompareRevision => ({
  name: oid.slice(0, 7),
  full_name: oid,
  kind: "commit",
});

describe("filterRevisions", () => {
  it("offers named refs but leaves commit IDs for manual input", () => {
    const options = [local("main"), commit("abc1234def5678")];
    expect(filterRevisions(options, "")).toEqual([local("main")]);
  });

  it("matches branch names and full refs case-insensitively with ordered characters", () => {
    const feature = local("feature/tauri");
    const remote: CompareRevision = {
      name: "origin/feature/tauri",
      full_name: "refs/remotes/origin/feature/tauri",
      kind: "remote",
    };
    expect(filterRevisions([feature, remote], "Ftau")).toEqual([feature, remote]);
    expect(filterRevisions([feature, remote], "refs/remotes/origin")).toEqual([remote]);
  });

  it("ranks exact, prefix, substring, then ordered-subsequence matches", () => {
    const options = [
      local("pre-main-suffix"),
      local("mainline"),
      local("xmain"),
      local("m-a-i-n"),
      local("main"),
    ];
    expect(filterRevisions(options, "main").map((option) => option.name)).toEqual([
      "main",
      "mainline",
      "xmain",
      "pre-main-suffix",
      "m-a-i-n",
    ]);
  });

  it("ranks the best match across local, remote, and tag groups", () => {
    const weakLocal = local("xmain");
    const exactRemote: CompareRevision = {
      name: "main",
      full_name: "refs/remotes/origin/main",
      kind: "remote",
    };
    const exactTag: CompareRevision = {
      name: "main",
      full_name: "refs/tags/main",
      kind: "tag",
    };

    expect(filterRevisions([weakLocal, exactRemote, exactTag], "main")).toEqual([
      exactRemote,
      exactTag,
      weakLocal,
    ]);
  });
});

describe("isRevisionUnavailable", () => {
  it("is false while the chosen ref is still offered", () => {
    const options = [local("main"), local("next")];
    expect(isRevisionUnavailable(local("main"), options)).toBe(false);
  });

  it("is true once the chosen named ref leaves the list", () => {
    const options = [local("main")];
    expect(isRevisionUnavailable(local("next"), options)).toBe(true);
  });

  it("spares commits, which never come from the list", () => {
    expect(isRevisionUnavailable(commit("abc1234def5678"), [local("main")])).toBe(
      false,
    );
  });

  it("is false with nothing selected", () => {
    expect(isRevisionUnavailable(null, [])).toBe(false);
  });
});
