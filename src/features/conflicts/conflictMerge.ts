import { conflictDiff, type ConflictDiffRange } from "./conflictDiff";

export interface MergeHighlight { from: number; to: number; kind: "conflict" | "mergeable" }
export interface MergeRegion {
  base: { from: number; to: number };
  ours: MergeHighlight;
  theirs: MergeHighlight;
  kind: MergeHighlight["kind"];
}
const lines = (text: string) => text.replace(/\r\n/g, "\n").split("\n");

/** Map zero-based line coordinates, preserving offsets through unchanged code.
 * Within replacements use proportional positions; deleted lines share an anchor. */
export function mapDiffLine(changes: ConflictDiffRange[], line: number, reverse = false): number {
  let low = 0, high = changes.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    const end = reverse ? changes[middle]!.newTo : changes[middle]!.oldTo;
    if (end <= line) low = middle + 1; else high = middle;
  }
  const change = changes[low];
  const previous = changes[low - 1];
  const offset = previous ? reverse ? previous.oldTo - previous.newTo : previous.newTo - previous.oldTo : 0;
  if (change) {
    const from = reverse ? change.newFrom : change.oldFrom;
    const to = reverse ? change.newTo : change.oldTo;
    const targetFrom = reverse ? change.oldFrom : change.newFrom;
    const targetTo = reverse ? change.oldTo : change.newTo;
    if (line >= from) return targetFrom + (line - from) * (targetTo - targetFrom) / (to - from);
  }
  return Math.max(0, line + offset);
}

function boundary(changes: ConflictDiffRange[], position: number, end: boolean): number {
  let low = 0, high = changes.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (changes[middle]!.oldTo < position) low = middle + 1; else high = middle;
  }
  const change = changes[low];
  const previous = changes[low - 1];
  const offset = previous ? previous.newTo - previous.oldTo : 0;
  if (change && position >= change.oldFrom) {
    if (position === change.oldFrom) return end && change.oldFrom === change.oldTo ? change.newTo : change.newFrom;
    if (position <= change.oldTo) {
      const mapped = change.newFrom + (position - change.oldFrom) * (change.newTo - change.newFrom) / (change.oldTo - change.oldFrom);
      return end ? Math.ceil(mapped) : Math.floor(mapped);
    }
  }
  return position + offset;
}
function project(changes: ConflictDiffRange[], from: number, to: number) {
  return { from: boundary(changes, from, false), to: boundary(changes, to, true) };
}

/** Classify changes against the common ancestor, not just differences between
 * the two tips. Touching edits are grouped conservatively (as in a line merge).
 * Both sides producing the same text are safe, as are one-sided changes. */
export function compareMerge(base: string, ours: string, theirs: string): MergeRegion[] {
  const oursChanges = conflictDiff(base, ours), theirsChanges = conflictDiff(base, theirs);
  const oursLines = lines(ours), theirsLines = lines(theirs);
  const edits = [
    ...oursChanges.map(change => ({ ...change, side: "ours" as const })),
    ...theirsChanges.map(change => ({ ...change, side: "theirs" as const })),
  ].sort((a, b) => a.oldFrom - b.oldFrom || a.oldTo - b.oldTo);
  const groups: { from: number; to: number; ours: boolean; theirs: boolean }[] = [];
  for (const edit of edits) {
    let group = groups.at(-1);
    if (!group || edit.oldFrom > group.to) {
      group = { from: edit.oldFrom, to: edit.oldTo, ours: false, theirs: false };
      groups.push(group);
    }
    group.to = Math.max(group.to, edit.oldTo);
    group[edit.side] = true;
  }
  return groups.map(group => {
    const left = project(oursChanges, group.from, group.to);
    const right = project(theirsChanges, group.from, group.to);
    const same = left.to - left.from === right.to - right.from &&
      oursLines.slice(left.from, left.to).every((line, index) => line === theirsLines[right.from + index]);
    const kind = group.ours && group.theirs && !same ? "conflict" : "mergeable";
    return { base: { from: group.from, to: group.to }, ours: { ...left, kind }, theirs: { ...right, kind }, kind };
  });
}

export function resultHighlights(base: string, result: string, regions: MergeRegion[]): MergeHighlight[] {
  const changes = conflictDiff(base, result);
  const highlights: MergeHighlight[] = regions.map(region => ({ ...project(changes, region.base.from, region.base.to), kind: region.kind }));
  // Include complete marker blocks, including diff3 ancestors, even if the
  // draft no longer aligns precisely with the original conflicting region.
  let start: number | undefined;
  const resultLines = lines(result);
  resultLines.forEach((line, index) => {
    if (/^<{7,}(?:\s|$)/.test(line)) start ??= index;
    if (start !== undefined && /^>{7,}(?:\s|$)/.test(line)) {
      highlights.push({ from: start, to: index + 1, kind: "conflict" }); start = undefined;
    }
  });
  if (start !== undefined) highlights.push({ from: start, to: resultLines.length, kind: "conflict" });
  // Coalesce overlaps so navigation and coloring agree. Conflict takes priority.
  const merged: MergeHighlight[] = [];
  for (const item of highlights.sort((a, b) => a.from - b.from || a.to - b.to)) {
    const previous = merged.at(-1);
    if (previous && item.from < Math.max(previous.to, previous.from + 1)) {
      previous.to = Math.max(previous.to, item.to);
      if (item.kind === "conflict") previous.kind = "conflict";
    } else merged.push({ ...item });
  }
  return merged;
}
