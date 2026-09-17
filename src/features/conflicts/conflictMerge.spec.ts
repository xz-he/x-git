import { describe, expect, it } from "vitest";
import { conflictDiff } from "./conflictDiff";
import { compareMerge, mapDiffLine, resultHighlights } from "./conflictMerge";

describe("three-way conflict classification", () => {
  it("separates one-sided mergeable changes from overlapping conflicting edits", () => {
    const base = "start\none\ngap\ntwo\ngap2\nthree\nend";
    const regions = compareMerge(base, base.replace("one", "ours-one").replace("three", "ours-three"), base.replace("two", "theirs-two").replace("three", "theirs-three"));
    expect(regions.map(region => [region.base.from, region.kind])).toEqual([[1, "mergeable"], [3, "mergeable"], [5, "conflict"]]);
  });
  it("marks identical changes and identical insertions as mergeable", () => {
    expect(compareMerge("a\nb\nc", "a\nnew\nc", "a\nnew\nc").map(region => region.kind)).toEqual(["mergeable"]);
    expect(compareMerge("a\nc", "a\nnew\nc", "a\nnew\nc")[0]).toMatchObject({ kind: "mergeable", base: { from: 1, to: 1 }, ours: { from: 1, to: 2 } });
  });
  it("flags competing insertions and delete/modify conflicts", () => {
    expect(compareMerge("a\nc", "a\nleft\nc", "a\nright\nc")[0]?.kind).toBe("conflict");
    expect(compareMerge("a\nb\nc", "a\nc", "a\nchanged\nc")[0]).toMatchObject({ kind: "conflict", ours: { from: 1, to: 1 }, theirs: { from: 1, to: 2 } });
  });
  it("keeps independent insertions and deletions yellow and tracks shifted source lines", () => {
    const regions = compareMerge("a\nb\nc\nd\ne", "a\nnew1\nnew2\nb\nc\nd\ne", "a\nb\nc\ne");
    expect(regions).toMatchObject([
      { kind: "mergeable", ours: { from: 1, to: 3 }, theirs: { from: 1, to: 1 } },
      { kind: "mergeable", ours: { from: 5, to: 6 }, theirs: { from: 3, to: 3 } },
    ]);
  });
  it("handles add/add, identical deletion, CRLF and unchanged files", () => {
    expect(compareMerge("", "ours", "theirs")[0]?.kind).toBe("conflict");
    expect(compareMerge("old", "", "")[0]?.kind).toBe("mergeable");
    expect(compareMerge("a\r\nb\r\n", "a\nb\n", "a\nb\n")).toEqual([]);
  });
  it("conservatively marks adjacent opposite edits as conflicting", () => {
    expect(compareMerge("a\nb\nc\nd", "a\nours\nc\nd", "a\nb\ntheirs\nd")[0]?.kind).toBe("conflict");
  });
  it("projects changes into the draft and colors the whole diff3 marker block red", () => {
    const base = "start\nvalue\ngap\nnext\nend";
    const regions = compareMerge(base, base.replace("value", "ours"), base.replace("value", "theirs").replace("next", "added"));
    const draft = "start\n<<<<<<< ours\nours\n||||||| base\nvalue\n=======\ntheirs\n>>>>>>> theirs\ngap\nadded\nend";
    expect(resultHighlights(base, draft, regions)).toEqual([
      { from: 1, to: 8, kind: "conflict" }, { from: 9, to: 10, kind: "mergeable" },
    ]);
  });
  it("does not mistake an ordinary less-than expression for a conflict marker", () => {
    expect(resultHighlights("", "a << b\n=======\nordinary", [])).toEqual([]);
  });
  it("keeps distant differences localized in a 30,000-line file", () => {
    const base = Array.from({ length: 30_000 }, (_, i) => `setting_${i} = ${i}`).join("\n");
    const regions = compareMerge(base, base.replace("setting_100 =", "ours ="), base.replace("setting_28000 =", "theirs ="));
    expect(regions.map(region => [region.base.from, region.kind])).toEqual([[100, "mergeable"], [28000, "mergeable"]]);
  });
});

describe("linked scrolling line mapping", () => {
  it("aligns common lines after insertions and deletions in both directions", () => {
    const changes = conflictDiff("a\nb\nc\nd\ne", "a\ninsert1\ninsert2\nb\nc\ne");
    expect(mapDiffLine(changes, 1)).toBe(3);
    expect(mapDiffLine(changes, 2.25)).toBe(4.25);
    expect(mapDiffLine(changes, 4)).toBe(5);
    expect(mapDiffLine(changes, 4.25, true)).toBe(2.25);
    expect(mapDiffLine(changes, 5, true)).toBe(4);
    expect(mapDiffLine(changes, 1, true)).toBe(1);
  });
  it("maps proportionally within unequal replacement blocks", () => {
    const changes = conflictDiff("a\nold1\nold2\nb", "a\nnew1\nnew2\nnew3\nnew4\nb");
    expect(mapDiffLine(changes, 2)).toBe(3);
    expect(mapDiffLine(changes, 3, true)).toBe(2);
    expect(mapDiffLine([], 42.5)).toBe(42.5);
  });
});
