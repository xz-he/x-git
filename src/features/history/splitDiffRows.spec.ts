import { describe, expect, it } from "vitest";
import type { DiffLine } from "@/lib/backend/types";
import { splitDiffRows } from "./splitDiffRows";

const line = (kind: DiffLine["kind"], content: string, oldLine: number | null, newLine: number | null): DiffLine => ({ kind, content, oldLine, newLine });
const rows = (lines: DiffLine[]) => splitDiffRows([{ index: 0, header: "@@ -1,3 +1,4 @@", lines }]).slice(1);

describe("split diff alignment", () => {
  it("pairs a replacement block and pads its shorter side before context", () => {
    const result = rows([line("deletion", "old", 1, null), line("addition", "new", null, 1), line("addition", "extra", null, 2), line("context", "same", 2, 3)]);
    expect(result.map(row => [row.left?.content, row.right?.content])).toEqual([["old", "new"], [undefined, "extra"], ["same", "same"]]);
    expect(result[2]?.left?.lineNumber).toBe(2);
    expect(result[2]?.right?.lineNumber).toBe(3);
  });

  it("leaves the absent side blank for additions and deletions", () => {
    expect(rows([line("addition", "new file", null, 1)])[0]).toMatchObject({ left: null, right: { kind: "addition", lineNumber: 1 } });
    expect(rows([line("deletion", "deleted file", 1, null)])[0]).toMatchObject({ left: { kind: "deletion", lineNumber: 1 }, right: null });
  });

  it("attaches no-newline markers to their side without splitting a replacement", () => {
    const result = rows([line("deletion", "old", 7, null), line("meta", "\\ No newline at end of file", null, null), line("addition", "new", null, 7), line("meta", "\\ No newline at end of file", null, null)]);
    expect(result).toHaveLength(2);
    expect(result[0]).toMatchObject({ left: { content: "old" }, right: { content: "new" } });
    expect(result[1]).toMatchObject({ left: { kind: "meta", lineNumber: null }, right: { kind: "meta", lineNumber: null } });
  });

  it("preserves hunk separators and literal content without changing source data", () => {
    const hunks = [{ index: 0, header: "@@ -1 +1 @@", lines: [line("context", "<script>中文\t</script>", 1, 1)] }, { index: 1, header: "@@ -80 +80 @@", lines: [] }];
    const original = JSON.stringify(hunks);
    const result = splitDiffRows(hunks);
    expect(result.map(row => row.kind)).toEqual(["header", "line", "header"]);
    expect(result[1]?.right?.content).toBe("<script>中文\t</script>");
    expect(JSON.stringify(hunks)).toBe(original);
  });

  it("does not align a no-newline annotation with an extra changed code line", () => {
    const result = rows([line("deletion", "old", 1, null), line("meta", "\\ No newline at end of file", null, null), line("addition", "first", null, 1), line("addition", "second", null, 2)]);
    expect(result.map(row => [row.left?.content, row.right?.content])).toEqual([["old", "first"], ["\\ No newline at end of file", undefined], [undefined, "second"]]);
  });
});
