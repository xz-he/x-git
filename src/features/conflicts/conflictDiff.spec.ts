import { describe, expect, it } from "vitest";
import { conflictDiff } from "./conflictDiff";

describe("conflict version line comparison", () => {
  it("locates additions, removals and separated replacements without shifting later matches", () => {
    expect(conflictDiff("a\nb\nc\nd\ne", "a\ninsert\nb\nc\nchanged\ne")).toEqual([
      { oldFrom: 1, oldTo: 1, newFrom: 1, newTo: 2 },
      { oldFrom: 3, oldTo: 4, newFrom: 4, newTo: 5 },
    ]);
    expect(conflictDiff("a\nb\nc", "a\nc")).toEqual([{ oldFrom: 1, oldTo: 2, newFrom: 1, newTo: 1 }]);
    expect(conflictDiff("中文\r\n", "中文\n")).toEqual([]);
    expect(conflictDiff("a\n", "a")).toEqual([{ oldFrom: 1, oldTo: 2, newFrom: 1, newTo: 1 }]);
  });
  it("keeps two far-apart changes localized in a large source file", () => {
    const before = Array.from({ length: 30_000 }, (_, i) => `value_${i} = ${i}`);
    const after = [...before]; after[100] = "new first"; after[28000] = "new last";
    expect(conflictDiff(before.join("\n"), after.join("\n"))).toEqual([
      { oldFrom: 100, oldTo: 101, newFrom: 100, newTo: 101 }, { oldFrom: 28000, oldTo: 28001, newFrom: 28000, newTo: 28001 },
    ]);
  });
  it("reconstructs edited repeated-line files from all detected ranges", () => {
    for (let seed = 1; seed <= 100; seed++) {
      const original = Array.from({ length: 200 }, (_, i) => `${i % 13}`);
      const edited = [...original];
      edited.splice(seed, seed % 7, "插入", "新内容"); edited.splice(180, seed % 5);
      const reconstructed = [...original];
      for (const range of conflictDiff(original.join("\n"), edited.join("\n")).reverse()) {
        reconstructed.splice(range.oldFrom, range.oldTo - range.oldFrom, ...edited.slice(range.newFrom, range.newTo));
      }
      expect(reconstructed).toEqual(edited);
    }
  });
});
