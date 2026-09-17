<script setup lang="ts">
import { computed, nextTick, onMounted, onBeforeUnmount, ref, useSlots, watch } from "vue";
import type { DiffHunk } from "@/lib/backend/types";
import { splitDiffRows } from "./splitDiffRows";

const props = withDefaults(defineProps<{
  hunks: DiffHunk[];
  rowHeight?: number;
  fontSize?: number;
  hideHunkHeaders?: boolean;
  fillHeight?: boolean;
  highlightedLine?: { lineNumber: number; kind: "addition" | "deletion" };
}>(), { rowHeight: 22, fontSize: 12, hideHunkHeaders: false, fillHeight: false });
const slots = useSlots();
const MAX_HEIGHT = 484;
const OVERSCAN = 12;
const sides = ["left", "right"] as const;
const panes = ref<HTMLElement[]>([]);
const scrollTop = ref(0);
const viewport = ref<HTMLElement>();
const viewportHeight = ref(MAX_HEIGHT);
let observer: ResizeObserver | undefined;
onMounted(() => {
  if (typeof ResizeObserver === 'undefined' || !viewport.value) return;
  observer = new ResizeObserver(([entry]) => {
    if (entry && entry.contentRect.height > 0) viewportHeight.value = entry.contentRect.height;
  });
  observer.observe(viewport.value);
});
onBeforeUnmount(() => observer?.disconnect());
const rows = computed(() => splitDiffRows(props.hunks).filter(row => !props.hideHunkHeaders || row.kind !== "header"));
const height = computed(() => props.fillHeight ? viewportHeight.value : Math.min(MAX_HEIGHT, rows.value.length * props.rowHeight + 18));
const start = computed(() => Math.max(0, Math.floor(scrollTop.value / props.rowHeight) - OVERSCAN));
const end = computed(() => Math.min(rows.value.length, start.value + Math.ceil(height.value / props.rowHeight) + OVERSCAN * 2));
const visible = computed(() => rows.value.slice(start.value, end.value));
// Stable full-file width prevents horizontal scroll from jumping as virtual rows change.
const columns = computed(() => {
  let width = 0;
  for (const row of rows.value) for (const side of sides) {
    const content = row[side]?.content ?? "";
    let length = 0;
    for (const char of content) length += char === "\t" ? 4 - length % 4 : (char.codePointAt(0)! > 255 ? 2 : 1);
    width = Math.max(width, length);
  }
  return width;
});

function syncScroll(event: Event): void {
  const source = event.currentTarget as HTMLElement;
  scrollTop.value = source.scrollTop;
  for (const pane of panes.value) {
    if (pane === source) continue;
    if (pane.scrollTop !== source.scrollTop) pane.scrollTop = source.scrollTop;
    if (pane.scrollLeft !== source.scrollLeft) pane.scrollLeft = source.scrollLeft;
  }
}

watch(() => props.hunks, () => {
  scrollTop.value = 0;
  for (const pane of panes.value) { pane.scrollTop = 0; pane.scrollLeft = 0; }
});

watch([rows, () => props.highlightedLine], async () => {
  const target = props.highlightedLine;
  if (!target) return;
  const side = target.kind === "addition" ? "right" : "left";
  const index = rows.value.findIndex(row => row[side]?.kind === target.kind && row[side]?.lineNumber === target.lineNumber);
  if (index < 0) return;
  scrollTop.value = Math.max(0, index * props.rowHeight - height.value / 2);
  await nextTick();
  for (const pane of panes.value) pane.scrollTop = scrollTop.value;
  panes.value[side === "right" ? 1 : 0]?.querySelector(`[data-row="${index}"]`)?.scrollIntoView?.({ block: "center" });
}, { immediate: true, flush: "post" });
</script>

<template>
  <div class="split-diff" :class="{ 'fill-height': fillHeight }" :style="{ '--row-height': `${rowHeight}px`, '--diff-font-size': `${fontSize}px` }">
    <div class="side-labels"><span>修改前</span><span>修改后</span></div>
    <div ref="viewport" class="split-panes">
      <div v-for="side in sides" :key="side" ref="panes" class="diff-pane" :aria-label="side === 'left' ? '修改前' : '修改后'" role="region" tabindex="0" :style="{ height: fillHeight ? '100%' : `${height}px` }" @scroll="syncScroll">
        <div class="diff-canvas" :style="{ minWidth: `calc(${columns}ch + ${slots.cell ? 120 : 80}px)` }">
          <div aria-hidden="true" :style="{ height: `${start * rowHeight}px` }" />
          <div v-for="(row, index) in visible" :key="start + index" class="diff-cell" :class="[row[side]?.kind, { empty: !row[side], 'hunk-header': row.kind === 'header', 'custom-cell': !!slots.cell }]" :data-row="start + index">
            <slot name="cell" :cell="row[side]" :side="side">
            <span class="line-number" aria-hidden="true">{{ row[side]?.lineNumber }}</span>
            <span class="line-marker" aria-hidden="true">{{ row[side]?.kind === 'addition' ? '+' : row[side]?.kind === 'deletion' ? '−' : '' }}</span>
            <code>{{ row[side]?.content }}</code>
            </slot>
          </div>
          <div aria-hidden="true" :style="{ height: `${(rows.length - end) * rowHeight}px` }" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.split-diff { min-width: 0; background: var(--surface-panel); }
.split-diff.fill-height { display: flex; flex-direction: column; min-height: 0; flex: 1; overflow: hidden; }
.fill-height .side-labels { flex-shrink: 0; }
.fill-height .split-panes { flex: 1; min-height: 0; overflow: hidden; }
.fill-height .diff-pane { min-height: 0; }
.side-labels, .split-panes { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
.side-labels { border-bottom: 1px solid var(--border); background: var(--surface-muted); color: var(--text-muted); font-size: 11px; }
.side-labels span { padding: 5px 12px; }
.side-labels span + span, .diff-pane + .diff-pane { border-left: 1px solid var(--border); }
.diff-pane { min-width: 0; overflow: auto; scrollbar-gutter: stable; overflow-anchor: none; }
.diff-canvas { width: 100%; font: var(--diff-font-size)/var(--row-height) var(--font-code); tab-size: 4; }
.diff-cell.custom-cell { display: block; }
.diff-cell { display: grid; grid-template-columns: 48px 18px minmax(0, 1fr); height: var(--row-height); white-space: pre; }
.line-number { padding-right: 8px; color: var(--text-muted); text-align: right; user-select: none; }
.line-marker { color: var(--text-muted); user-select: none; }
.diff-cell code { padding-right: 12px; font: inherit; white-space: pre; }
.diff-cell.addition { background: var(--diff-add-bg); }
.diff-cell.deletion { background: var(--diff-delete-bg); }
.addition .line-number, .addition .line-marker { background: color-mix(in srgb, var(--success) 9%, transparent); color: var(--success); }
.deletion .line-number, .deletion .line-marker { background: color-mix(in srgb, var(--danger) 9%, transparent); color: var(--danger); }
.diff-cell.empty { background: var(--surface-muted); }
.diff-cell.meta { color: var(--text-muted); font-style: italic; }
.diff-cell.hunk-header { background: var(--primary-soft); color: var(--text-muted); font-style: normal; }
</style>
