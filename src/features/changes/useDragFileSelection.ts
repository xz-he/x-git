import { onBeforeUnmount, ref, watch, type Ref } from "vue";
import type { ChangeScope } from "@/lib/backend/types";

const DRAG_THRESHOLD = 5;
const EDGE_SIZE = 36;
const SCROLL_SPEED = 650;
const MAX_FRAME_SECONDS = 0.05;
interface Options {
  blocked: () => boolean;
  identity: () => string;
  paths: (scope: ChangeScope) => string[];
  selections: Record<ChangeScope, Ref<Set<string>>>;
}
interface Gesture {
  pointerId: number; scope: ChangeScope; pane: HTMLElement; row: HTMLElement;
  startX: number; startY: number; x: number; y: number; anchor: number;
  baseline: Set<string>; remove: boolean; paths: string[];
  rows: { top: number; bottom: number }[]; lastIndex: number; lastFrame: number;
}

/** Range selection shares the existing checkbox sets; dragging never runs Git. */
export function useDragFileSelection(options: Options) {
  const dragging = ref(false);
  let gesture: Gesture | undefined;
  let frame = 0;
  let suppressClick = false;
  let clickTimer: ReturnType<typeof setTimeout> | undefined;

  function stop(): void {
    const previous = gesture;
    gesture = undefined;
    if (dragging.value) {
      suppressClick = true;
      clearTimeout(clickTimer);
      clickTimer = setTimeout(() => { suppressClick = false; }, 0);
    }
    dragging.value = false;
    cancelAnimationFrame(frame); frame = 0;
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", end);
    window.removeEventListener("pointercancel", end);
    window.removeEventListener("blur", stop);
    window.removeEventListener("resize", stop);
    previous?.row.removeEventListener("lostpointercapture", stop);
    if (previous?.row.hasPointerCapture?.(previous.pointerId)) previous.row.releasePointerCapture(previous.pointerId);
  }
  function end(event: PointerEvent): void { if (event.pointerId === gesture?.pointerId) stop(); }
  function updateSelection(): void {
    const g = gesture;
    if (!g || !dragging.value) return;
    const bounds = g.pane.getBoundingClientRect();
    const y = Math.max(bounds.top, Math.min(bounds.bottom - 1, g.y)) - bounds.top + g.pane.scrollTop;
    let index = g.rows.findIndex(row => y < row.bottom);
    if (index < 0) index = g.rows.length - 1;
    if (index === g.lastIndex) return;
    g.lastIndex = index;
    const selected = new Set(g.baseline);
    for (let i = Math.min(index, g.anchor); i <= Math.max(index, g.anchor); i++) {
      const path = g.paths[i]!;
      if (g.remove) selected.delete(path); else selected.add(path);
    }
    options.selections[g.scope].value = selected;
  }
  function scroll(time: number): void {
    const g = gesture;
    if (!g || !dragging.value) return;
    const seconds = g.lastFrame ? Math.min(MAX_FRAME_SECONDS, (time - g.lastFrame) / 1000) : 0;
    g.lastFrame = time;
    const bounds = g.pane.getBoundingClientRect();
    const edge = Math.min(EDGE_SIZE, bounds.height / 3);
    if (edge > 0 && g.x >= bounds.left && g.x <= bounds.right) {
      const speed = g.y < bounds.top + edge ? -Math.min(1, (bounds.top + edge - g.y) / edge)
        : g.y > bounds.bottom - edge ? Math.min(1, (g.y - bounds.bottom + edge) / edge) : 0;
      g.pane.scrollTop += speed * SCROLL_SPEED * seconds;
    }
    updateSelection();
    frame = requestAnimationFrame(scroll);
  }
  function move(event: PointerEvent): void {
    const g = gesture;
    if (!g || event.pointerId !== g.pointerId) return;
    if (!(event.buttons & 1) || options.blocked()) { stop(); return; }
    g.x = event.clientX; g.y = event.clientY;
    if (!dragging.value) {
      if (Math.hypot(g.x - g.startX, g.y - g.startY) < DRAG_THRESHOLD) return;
      dragging.value = true;
      g.row.setPointerCapture?.(g.pointerId);
      g.row.addEventListener("lostpointercapture", stop);
      frame = requestAnimationFrame(scroll);
    }
    event.preventDefault();
    updateSelection();
  }
  function start(event: PointerEvent, path: string, scope: ChangeScope): void {
    if (event.button !== 0 || (event.pointerType && event.pointerType !== "mouse") || options.blocked()) return;
    const target = event.target as HTMLElement;
    // File names and checkboxes can start selection; action buttons retain their clicks.
    if (target.closest("button:not(.file-select), a, select, textarea")) return;
    stop(); suppressClick = false; clearTimeout(clickTimer);
    const row = event.currentTarget as HTMLElement;
    const pane = row.closest<HTMLElement>(".staged-pane, .unstaged-pane");
    if (!pane) return;
    const paths = options.paths(scope);
    const anchor = paths.indexOf(path);
    const rows = [...pane.querySelectorAll<HTMLElement>(".change-row")];
    if (anchor < 0 || rows.length !== paths.length) return;
    const paneTop = pane.getBoundingClientRect().top;
    gesture = {
      pointerId: event.pointerId, scope, pane, row, startX: event.clientX, startY: event.clientY,
      x: event.clientX, y: event.clientY, anchor, paths,
      baseline: new Set(options.selections[scope].value),
      remove: !!target.closest('input[type="checkbox"]') && options.selections[scope].value.has(path),
      rows: rows.map(element => { const rect = element.getBoundingClientRect(); return { top: rect.top - paneTop + pane.scrollTop, bottom: rect.bottom - paneTop + pane.scrollTop }; }),
      lastIndex: -1, lastFrame: 0,
    };
    window.addEventListener("pointermove", move, { passive: false });
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("blur", stop);
    window.addEventListener("resize", stop);
  }
  function captureClick(event: MouseEvent): void {
    if (!suppressClick && !dragging.value) return;
    event.preventDefault(); event.stopPropagation();
    suppressClick = false;
  }
  watch(() => [options.identity(), options.blocked(), JSON.stringify(options.paths("staged")), JSON.stringify(options.paths("unstaged"))], stop, { flush: "sync" });
  onBeforeUnmount(() => { stop(); clearTimeout(clickTimer); });
  return { dragging, start, captureClick };
}
