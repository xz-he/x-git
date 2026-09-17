<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

const props = defineProps<{ hasStaged: boolean }>();
const STORAGE_KEY = "hq-git.changes-split.v1";
const HANDLE_HEIGHT = 8;
const MIN_STAGED_HEIGHT = 70;
const MIN_UNSTAGED_HEIGHT = 110;
const DEFAULT_STAGED_HEIGHT = 240;
const DEFAULT_EMPTY_HEIGHT = 80;
const DEFAULT_MAX_RATIO = 0.45;
const KEY_STEP = 20;
const container = ref<HTMLElement>();
const height = ref(0);
const ratio = ref<number>();
const dragging = ref(false);
try {
  const saved: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null");
  if (typeof saved === "number" && Number.isFinite(saved) && saved > 0 && saved < 1) ratio.value = saved;
} catch { /* Layout works even when local storage is unavailable. */ }
const availableHeight = computed(() => Math.max(0, height.value - HANDLE_HEIGHT));
const minHeight = computed(() => Math.min(MIN_STAGED_HEIGHT, availableHeight.value / 2));
const maxHeight = computed(() => Math.max(minHeight.value, availableHeight.value - Math.min(MIN_UNSTAGED_HEIGHT, availableHeight.value / 2)));
const stagedHeight = computed(() => Math.max(minHeight.value, Math.min(maxHeight.value, ratio.value !== undefined
  ? availableHeight.value * ratio.value
  : Math.min(availableHeight.value * DEFAULT_MAX_RATIO, props.hasStaged ? DEFAULT_STAGED_HEIGHT : DEFAULT_EMPTY_HEIGHT))));
let observer: ResizeObserver | undefined;
let handle: HTMLElement | undefined;
let pointerId: number | undefined;
let startY = 0;
let startHeight = 0;

function measure(): void { height.value = container.value?.getBoundingClientRect().height ?? 0; }
function setHeight(value: number): void {
  if (!availableHeight.value) return;
  ratio.value = Math.max(minHeight.value, Math.min(maxHeight.value, value)) / availableHeight.value;
}
function persist(): void {
  try {
    if (ratio.value === undefined) localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, JSON.stringify(ratio.value));
  } catch { /* Keep the current layout without persistence. */ }
}
function start(event: PointerEvent): void {
  if (event.button !== 0 || dragging.value) return;
  event.preventDefault();
  measure();
  startY = event.clientY;
  startHeight = stagedHeight.value;
  pointerId = event.pointerId;
  handle = event.currentTarget as HTMLElement;
  handle.focus();
  handle.setPointerCapture?.(pointerId);
  dragging.value = true;
}
function move(event: PointerEvent): void {
  if (dragging.value && event.pointerId === pointerId) setHeight(startHeight + event.clientY - startY);
}
function stop(): void {
  if (!dragging.value) return;
  dragging.value = false;
  const currentHandle = handle, currentPointer = pointerId;
  handle = undefined; pointerId = undefined;
  if (currentHandle && currentPointer !== undefined && currentHandle.hasPointerCapture?.(currentPointer)) currentHandle.releasePointerCapture(currentPointer);
  persist();
}
function key(event: KeyboardEvent): void {
  if (!["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  measure();
  setHeight(event.key === "Home" ? minHeight.value : event.key === "End" ? maxHeight.value : stagedHeight.value + (event.key === "ArrowUp" ? -KEY_STEP : KEY_STEP));
  persist();
}
function reset(): void { ratio.value = undefined; persist(); }
onMounted(() => {
  measure();
  if (typeof ResizeObserver !== "undefined") { observer = new ResizeObserver(measure); observer.observe(container.value!); }
  window.addEventListener("resize", measure);
});
onBeforeUnmount(() => { stop(); observer?.disconnect(); window.removeEventListener("resize", measure); });
</script>

<template>
  <div ref="container" class="change-groups-split" :class="{ dragging }" :style="{ gridTemplateRows: `${stagedHeight}px ${HANDLE_HEIGHT}px minmax(0, 1fr)` }">
    <div class="staged-pane" role="region" aria-label="已暂存文件区域" tabindex="0"><slot name="staged" /></div>
    <div class="change-groups-resizer" role="separator" aria-label="调整已暂存和未暂存区域高度" aria-orientation="horizontal"
      :aria-valuemin="Math.round(minHeight)" :aria-valuemax="Math.round(maxHeight)" :aria-valuenow="Math.round(stagedHeight)" tabindex="0"
      title="上下拖动调整高度；双击恢复默认高度" @pointerdown="start" @pointermove="move" @pointerup="stop" @pointercancel="stop" @lostpointercapture="stop" @keydown="key" @dblclick="reset"><span /></div>
    <div class="unstaged-pane" role="region" aria-label="未暂存文件区域" tabindex="0"><slot name="unstaged" /></div>
  </div>
</template>

<style scoped>
.change-groups-split { display: grid; flex: 1; min-height: 0; min-width: 0; overflow: hidden; }
.staged-pane, .unstaged-pane { min-height: 0; min-width: 0; overflow: auto; overscroll-behavior: contain; }
/* Hide only the staged vertical scrollbar; retain horizontal access to wide tables. */
.staged-pane::-webkit-scrollbar { width: 0; height: 8px; }
.change-groups-resizer { display: grid; place-items: center; cursor: row-resize; touch-action: none; user-select: none; border-block: 1px solid var(--border); background: var(--surface-muted); }
.change-groups-resizer span { width: 32px; height: 2px; border-radius: 2px; background: var(--text-muted); opacity: .5; }
.change-groups-resizer:hover, .change-groups-resizer:focus-visible, .dragging .change-groups-resizer { background: var(--primary-soft); outline: none; border-color: var(--primary); }
.change-groups-resizer:hover span, .change-groups-resizer:focus-visible span, .dragging .change-groups-resizer span { background: var(--primary); opacity: 1; }
.dragging { user-select: none; }
</style>
