import { conflictDiff } from "./conflictDiff";
import type { MergeRegion } from "./conflictMerge";

export type BlockSide = "ours" | "theirs";
export type BlockChoice = "this" | "first" | "last";
const split = (text: string) => text.replace(/\r\n/g, "\n").split("\n");

/** An edit crossing a region boundary cannot safely be attributed to one block.
 * Never use proportional scroll coordinates for destructive text replacement. */
export function resultBlockRange(base: string, result: string, region: MergeRegion) {
  const { from, to } = region.base;
  const changes = conflictDiff(base, result);
  if (changes.some(change =>
    (change.oldFrom < from && change.oldTo > from) ||
    (change.oldFrom < to && change.oldTo > to))) return undefined;
  const boundary = (position: number, end: boolean) => {
    let offset = 0;
    for (const change of changes) {
      if (change.oldFrom > position) break;
      if (change.oldFrom === position) return end && change.oldTo === position ? change.newTo : change.newFrom;
      if (change.oldTo === position) {
        offset = change.newTo - change.oldTo;
        continue;
      }
      offset = change.newTo - change.oldTo;
    }
    return position + offset;
  };
  return { from: boundary(from, false), to: boundary(to, true) };
}

export function applyConflictBlock(input: {
  base: string; ours: string; theirs: string; result: string;
  region: MergeRegion; side: BlockSide; choice: BlockChoice;
}): string | undefined {
  const range = resultBlockRange(input.base, input.result, input.region);
  if (!range) return undefined;
  const take = (side: BlockSide) => {
    const source = input.region[side];
    return split(input[side]).slice(source.from, source.to);
  };
  const selected = take(input.side), other = take(input.side === "ours" ? "theirs" : "ours");
  const replacement = input.choice === "this" ? selected : input.choice === "first" ? [...selected, ...other] : [...other, ...selected];
  const result = split(input.result);
  // The working file determines line endings; the backend preserves its BOM.
  // Avoid spread arguments to splice: a large block can exceed the JS call stack.
  return result.slice(0, range.from).concat(replacement, result.slice(range.to)).join(input.result.includes("\r\n") ? "\r\n" : "\n");
}
