import { describe, expect, it } from "vitest";
import { applyConflictBlock, resultBlockRange, type BlockChoice, type BlockSide } from "./conflictBlocks";
import { compareMerge } from "./conflictMerge";

describe("conflict block resolution", () => {
  const base = "start\nold\ngap\nother\nend\n";
  const ours = base.replace("old", "left");
  const theirs = base.replace("old", "right").replace("other", "incoming");
  const marked = "start\n<<<<<<< HEAD\nleft\n||||||| base\nold\n=======\nright\n>>>>>>> topic\ngap\nincoming\nend\n";
  function apply(result: string, side: BlockSide, choice: BlockChoice) {
    return applyConflictBlock({ base, ours, theirs, result, region: compareMerge(base, ours, theirs)[0]!, side, choice });
  }
  it.each([
    ["ours", "this", "left"], ["theirs", "this", "right"],
    ["ours", "first", "left\nright"], ["ours", "last", "right\nleft"],
    ["theirs", "first", "right\nleft"], ["theirs", "last", "left\nright"],
  ] as const)("uses %s / %s and removes the full diff3 block", (side, choice, selected) => {
    expect(apply(marked, side, choice)).toBe(`start\n${selected}\ngap\nincoming\nend\n`);
  });
  it("can replace the same block again after choosing both sides", () => {
    const both = apply(marked, "ours", "first")!;
    expect(apply(both, "theirs", "this")).toBe("start\nright\ngap\nincoming\nend\n");
  });
  it("preserves CRLF, final newline and unrelated manual edits", () => {
    const draft = marked.replace("end", "manually edited end").replace(/\n/g, "\r\n");
    expect(apply(draft, "ours", "this")).toBe("start\r\nleft\r\ngap\r\nincoming\r\nmanually edited end\r\n");
  });
  it("does not invent a final newline for a file without one", () => {
    expect(apply(marked.trimEnd(), "ours", "this")).toBe("start\nleft\ngap\nincoming\nend");
  });
  it("handles competing insertions and a deleted side", () => {
    const insertion = { base: "a\nb", ours: "a\nleft\nb", theirs: "a\nright\nb", result: "a\n<<<<<<< HEAD\nleft\n=======\nright\n>>>>>>> topic\nb" };
    expect(applyConflictBlock({ ...insertion, region: compareMerge(insertion.base, insertion.ours, insertion.theirs)[0]!, side: "theirs", choice: "first" })).toBe("a\nright\nleft\nb");
    const deletion = { base: "a\nold\nb", ours: "a\nb", theirs: "a\nright\nb", result: "a\n<<<<<<< HEAD\n=======\nright\n>>>>>>> topic\nb" };
    expect(applyConflictBlock({ ...deletion, region: compareMerge(deletion.base, deletion.ours, deletion.theirs)[0]!, side: "ours", choice: "this" })).toBe("a\nb");
  });
  it("targets the second repeated block by its coordinates", () => {
    const input = { base: "a\nold\nb\nold\nc", ours: "a\nleft\nb\nleft\nc", theirs: "a\nright\nb\nright\nc", result: "a\n<<<<<<< HEAD\nleft\n=======\nright\n>>>>>>> topic\nb\n<<<<<<< HEAD\nleft\n=======\nright\n>>>>>>> topic\nc" };
    const regions = compareMerge(input.base, input.ours, input.theirs);
    expect(applyConflictBlock({ ...input, region: regions[1]!, side: "ours", choice: "this" })).toBe("a\n<<<<<<< HEAD\nleft\n=======\nright\n>>>>>>> topic\nb\nleft\nc");
  });
  it("refuses to overwrite an edit spanning multiple original regions", () => {
    const region = compareMerge(base, ours, theirs)[0]!;
    expect(resultBlockRange(base, "start\nrewritten everything\nend\n", region)).toBeUndefined();
    expect(apply("start\nrewritten everything\nend\n", "ours", "this")).toBeUndefined();
  });
  it("handles a large block without exceeding the JavaScript argument limit", () => {
    const ours = "a\n".repeat(150_000), theirs = "b\n".repeat(150_000);
    const region = compareMerge("", ours, theirs)[0]!;
    expect(applyConflictBlock({ base: "", ours, theirs, result: "pending\n", region, side: "ours", choice: "first" })).toBe(ours + theirs);
  });
});
