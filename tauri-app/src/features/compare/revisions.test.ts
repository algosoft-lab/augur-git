import { describe, expect, it } from "vitest";

import type { CompareRevision } from "../../bridge/types";
import { isRevisionUnavailable } from "./revisions";

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
