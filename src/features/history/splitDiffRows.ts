import type { DiffHunk, DiffLine, DiffLineKind } from "@/lib/backend/types";

export interface SplitCell {
  kind: DiffLineKind;
  content: string;
  lineNumber: number | null;
}
export interface SplitRow {
  kind: "header" | "line";
  left: SplitCell | null;
  right: SplitCell | null;
}

/** Pair consecutive changed blocks in linear time; absent lines stay blank. */
export function splitDiffRows(hunks: DiffHunk[]): SplitRow[] {
  const result: SplitRow[] = [];
  const cell = (line: DiffLine, side: "oldLine" | "newLine"): SplitCell => ({ kind: line.kind, content: line.content, lineNumber: line[side] });
  for (const hunk of hunks) {
    const header: SplitCell = { kind: "meta", content: hunk.header, lineNumber: null };
    result.push({ kind: "header", left: header, right: header });
    let left: SplitCell[] = [];
    let right: SplitCell[] = [];
    let previous: DiffLineKind | undefined;
    const flush = () => {
      let oldIndex = 0;
      let newIndex = 0;
      while (oldIndex < left.length || newIndex < right.length) {
        const oldCell = left[oldIndex];
        const newCell = right[newIndex];
        const annotation = oldCell?.kind === "meta" || newCell?.kind === "meta";
        const takeOld = !!oldCell && (!annotation || oldCell.kind === "meta");
        const takeNew = !!newCell && (!annotation || newCell.kind === "meta");
        result.push({ kind: "line", left: takeOld ? oldCell : null, right: takeNew ? newCell : null });
        if (takeOld) oldIndex++;
        if (takeNew) newIndex++;
      }
      left = [];
      right = [];
    };
    for (const line of hunk.lines) {
      if (line.kind === "deletion") {
        if (previous === "addition") flush();
        left.push(cell(line, "oldLine"));
      } else if (line.kind === "addition") {
        right.push(cell(line, "newLine"));
      } else if (line.kind === "meta" && (previous === "deletion" || previous === "addition")) {
        (previous === "deletion" ? left : right).push(cell(line, "oldLine"));
        continue;
      } else {
        flush();
        result.push({ kind: "line", left: cell(line, "oldLine"), right: cell(line, "newLine") });
      }
      previous = line.kind;
    }
    flush();
  }
  return result;
}
