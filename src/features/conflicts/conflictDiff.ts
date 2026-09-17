export interface ConflictDiffRange { oldFrom: number; oldTo: number; newFrom: number; newTo: number }
const SMALL_MATRIX_CELLS = 40_000;
const MAX_LINE_VISITS = 2_000_000;

/** Patience line anchors with a bounded LCS for small gaps. Large unanchored
 * replacements remain one range rather than doing quadratic work on the UI thread. */
export function conflictDiff(before: string, after: string): ConflictDiffRange[] {
  const oldLines = before.replace(/\r\n/g, "\n").split("\n");
  const newLines = after.replace(/\r\n/g, "\n").split("\n");
  const pending: ConflictDiffRange[] = [{ oldFrom: 0, oldTo: oldLines.length, newFrom: 0, newTo: newLines.length }];
  const changes: ConflictDiffRange[] = [];
  let visits = 0;
  while (pending.length) {
    let { oldFrom, oldTo, newFrom, newTo } = pending.pop()!;
    while (oldFrom < oldTo && newFrom < newTo && oldLines[oldFrom] === newLines[newFrom]) { oldFrom++; newFrom++; }
    while (oldFrom < oldTo && newFrom < newTo && oldLines[oldTo - 1] === newLines[newTo - 1]) { oldTo--; newTo--; }
    if (oldFrom === oldTo && newFrom === newTo) continue;
    const height = oldTo - oldFrom, width = newTo - newFrom;
    visits += height + width;
    if (!height || !width || visits > MAX_LINE_VISITS) { changes.push({ oldFrom, oldTo, newFrom, newTo }); continue; }
    if (height * width <= SMALL_MATRIX_CELLS) {
      const matrix = new Uint32Array((height + 1) * (width + 1));
      const at = (i: number, j: number) => i * (width + 1) + j;
      for (let i = height - 1; i >= 0; i--) for (let j = width - 1; j >= 0; j--) {
        matrix[at(i, j)] = oldLines[oldFrom + i] === newLines[newFrom + j]
          ? matrix[at(i + 1, j + 1)]! + 1 : Math.max(matrix[at(i + 1, j)]!, matrix[at(i, j + 1)]!);
      }
      let i = 0, j = 0;
      let change: ConflictDiffRange | undefined;
      while (i < height || j < width) {
        if (i < height && j < width && oldLines[oldFrom + i] === newLines[newFrom + j]) {
          if (change) { changes.push(change); change = undefined; } i++; j++;
        } else {
          change ??= { oldFrom: oldFrom + i, oldTo: oldFrom + i, newFrom: newFrom + j, newTo: newFrom + j };
          if (j === width || (i < height && matrix[at(i + 1, j)]! >= matrix[at(i, j + 1)]!)) i++; else j++;
          change.oldTo = oldFrom + i; change.newTo = newFrom + j;
        }
      }
      if (change) changes.push(change);
      continue;
    }
    const unique = (lines: string[], start: number, end: number) => {
      const found = new Map<string, number>();
      for (let i = start; i < end; i++) found.set(lines[i]!, found.has(lines[i]!) ? -1 : i);
      return found;
    };
    const oldUnique = unique(oldLines, oldFrom, oldTo), newUnique = unique(newLines, newFrom, newTo);
    const pairs: [number, number][] = [];
    for (const [line, oldIndex] of oldUnique) {
      const newIndex = newUnique.get(line);
      if (oldIndex >= 0 && newIndex !== undefined && newIndex >= 0) pairs.push([oldIndex, newIndex]);
    }
    const tails: number[] = [], previous: number[] = [];
    pairs.forEach((pair, index) => {
      let low = 0, high = tails.length;
      while (low < high) { const mid = (low + high) >>> 1; if (pairs[tails[mid]!]![1] < pair[1]) low = mid + 1; else high = mid; }
      previous[index] = low ? tails[low - 1]! : -1; tails[low] = index;
    });
    if (!tails.length) { changes.push({ oldFrom, oldTo, newFrom, newTo }); continue; }
    const anchors: [number, number][] = [];
    for (let index = tails[tails.length - 1]!; index >= 0; index = previous[index]!) anchors.push(pairs[index]!);
    anchors.reverse();
    for (const [oldIndex, newIndex] of anchors) {
      pending.push({ oldFrom, oldTo: oldIndex, newFrom, newTo: newIndex }); oldFrom = oldIndex + 1; newFrom = newIndex + 1;
    }
    pending.push({ oldFrom, oldTo, newFrom, newTo });
  }
  return changes.sort((a, b) => a.newFrom - b.newFrom || a.oldFrom - b.oldFrom);
}
