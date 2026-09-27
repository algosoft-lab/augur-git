import { describe, expect, it } from "vitest";

import { statBlocks, isBinary, statusKey, statusModifier } from "./fileMeta";
import type { FileChange } from "../../bridge/types";

function file(patch: Partial<FileChange> = {}): FileChange {
  return {
    path: "src/main.rs",
    old_path: null,
    new_path: "src/main.rs",
    status: "modified",
    old_blob: null,
    new_blob: null,
    added: 1,
    deleted: 1,
    ...patch,
  };
}

describe("stat blocks", () => {
  it("divides five segments between additions and deletions", () => {
    expect(statBlocks(5, 0)).toEqual({ added: 5, deleted: 0 });
    expect(statBlocks(0, 5)).toEqual({ added: 0, deleted: 5 });
    expect(statBlocks(1, 1)).toEqual({ added: 3, deleted: 2 });
  });

  it("keeps any addition visible by rounding up", () => {
    expect(statBlocks(1, 99).added).toBe(1);
  });

  it("is empty when there are no counts", () => {
    expect(statBlocks(0, 0)).toEqual({ added: 0, deleted: 0 });
    expect(statBlocks(null, null)).toEqual({ added: 0, deleted: 0 });
  });
});

describe("binary detection", () => {
  it("treats a missing line count as binary", () => {
    expect(isBinary(file({ added: null }))).toBe(true);
    expect(isBinary(file({ deleted: null }))).toBe(true);
    expect(isBinary(file())).toBe(false);
  });
});

describe("status keys", () => {
  it("maps every status to a colour modifier", () => {
    expect(statusModifier("added")).toBe("add");
    expect(statusModifier("deleted")).toBe("del");
    expect(statusModifier("modified")).toBe("mod");
    expect(statusModifier("renamed")).toBe("ren");
    expect(statusModifier("copied")).toBe("cpy");
    expect(statusModifier("unmerged")).toBe("conflict");
    expect(statusModifier("unknown")).toBe("unknown");
  });

  it("derives the catalog key from the modifier so the two cannot drift", () => {
    for (const status of [
      "added",
      "deleted",
      "modified",
      "renamed",
      "copied",
      "unmerged",
      "unknown",
    ] as const) {
      expect(statusKey(status)).toBe(`status-${statusModifier(status)}`);
    }
    expect(statusKey("modified")).toBe("status-mod");
  });
});
