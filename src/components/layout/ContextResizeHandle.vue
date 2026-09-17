<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";
const props = defineProps<{ width: number; maxWidth: number }>();
const emit = defineEmits<{ resize: [width: number] }>();
const MIN_WIDTH = 270;
const KEY_STEP = 20;
const dragging = ref(false);
let startX = 0;
let startWidth = 0;
let pointerId: number | undefined;
let handle: HTMLElement | undefined;
function setWidth(width: number): void { emit("resize", Math.max(MIN_WIDTH, Math.min(props.maxWidth, width))); }
function start(event: PointerEvent): void {
  if (event.button !== 0) return;
  event.preventDefault();
  startX = event.clientX;
  startWidth = props.width;
  pointerId = event.pointerId;
  handle = event.currentTarget as HTMLElement;
  handle.setPointerCapture?.(event.pointerId);
  dragging.value = true;
}
function move(event: PointerEvent): void {
  if (dragging.value && event.pointerId === pointerId) setWidth(startWidth + event.clientX - startX);
}
function stop(): void {
  if (handle && pointerId !== undefined && handle.hasPointerCapture?.(pointerId)) handle.releasePointerCapture(pointerId);
  dragging.value = false;
  pointerId = undefined;
  handle = undefined;
}
function key(event: KeyboardEvent): void {
  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  setWidth(event.key === "Home" ? MIN_WIDTH : event.key === "End" ? props.maxWidth : props.width + (event.key === "ArrowLeft" ? -KEY_STEP : KEY_STEP));
}
onBeforeUnmount(stop);
</script>
<template>
  <div class="context-resizer" :class="{ dragging }" role="separator" aria-label="调整文件列表宽度" aria-orientation="vertical" :aria-valuemin="MIN_WIDTH" :aria-valuemax="maxWidth" :aria-valuenow="width" tabindex="0" title="左右拖动调整宽度；双击恢复默认宽度" @pointerdown="start" @pointermove="move" @pointerup="stop" @pointercancel="stop" @lostpointercapture="stop" @keydown="key" @dblclick="setWidth(320)" />
</template>
<style scoped>
.context-resizer { position: absolute; top: 0; bottom: 0; right: 0; width: 6px; z-index: 8; cursor: col-resize; touch-action: none; user-select: none; }
.context-resizer:hover, .context-resizer:focus-visible, .context-resizer.dragging { background: var(--primary); opacity: .55; }
</style>
