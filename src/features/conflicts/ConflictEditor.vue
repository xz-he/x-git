<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Compartment, EditorState, StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView, keymap, lineNumbers, type DecorationSet } from "@codemirror/view";
import { ArrowUp, ArrowDown } from "@lucide/vue";
import { conflictDiff, type ConflictDiffRange } from "./conflictDiff";
import type { MergeHighlight } from "./conflictMerge";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";

const props = withDefaults(defineProps<{ modelValue: string; readonly?: boolean; label: string; compareText?: string; comparisonLabel?: string; mergeHighlights?: MergeHighlight[] }>(), { readonly: false });
const emit = defineEmits<{ "update:modelValue": [value: string]; "viewport-scroll": [position: { line: number; left: number }] }>();
const host = ref<HTMLElement>();
const mode = new Compartment();
let fontObserver: MutationObserver | undefined;
let view: EditorView | undefined;
let replacing = false;
const changes = ref<(ConflictDiffRange & { kind?: MergeHighlight["kind"] })[]>([]);
const active = ref(-1);
const setHighlights = StateEffect.define<DecorationSet>();
const highlights = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update: (value, transaction) => {
    value = value.map(transaction.changes);
    for (const effect of transaction.effects) if (effect.is(setHighlights)) value = effect.value;
    return value;
  },
  provide: field => EditorView.decorations.from(field),
});
let comparisonTimer: ReturnType<typeof setTimeout> | undefined;
function decorate(): void {
  if (!view) return;
  const decorations = new Map<number, { kind: string; active: boolean }>();
  changes.value.forEach((change, index) => {
    const kind = change.kind ?? (change.newFrom === change.newTo ? "deletion" : change.oldFrom === change.oldTo ? "addition" : "change");
    for (let line = change.newFrom; line < Math.max(change.newTo, change.newFrom + 1); line++) {
      const number = Math.min(view!.state.doc.lines, line + 1);
      const previous = decorations.get(number);
      decorations.set(number, { kind: previous?.kind === "conflict" ? previous.kind : kind, active: previous?.active || index === active.value });
    }
  });
  const ranges = [...decorations].sort((a, b) => a[0] - b[0]).map(([line, value]) => Decoration.line({ attributes: {
    class: `cm-diff-${value.kind}${value.active ? ' cm-diff-current' : ''}`,
    "data-diff-kind": value.kind,
    title: value.kind === "conflict" ? "双方改动冲突，需要确认解决结果" : value.kind === "mergeable" ? "单边改动或双方相同改动，可自动合并" : value.kind === "deletion" ? "对比版本的内容在此处被删除" : "与对比版本不同",
  } }).range(view!.state.doc.line(line).from));
  view.dispatch({ effects: setHighlights.of(Decoration.set(ranges)) });
}
function updateComparison(): void {
  changes.value = props.mergeHighlights !== undefined
    ? props.mergeHighlights.map(item => ({ oldFrom: item.from, oldTo: item.to, newFrom: item.from, newTo: item.to, kind: item.kind }))
    : props.compareText === undefined ? [] : conflictDiff(props.compareText, props.modelValue);
  active.value = -1; decorate();
}
function jump(direction: number): void {
  if (!view || !changes.value.length) return;
  userScroll();
  active.value = active.value < 0 ? direction > 0 ? 0 : changes.value.length - 1 : (active.value + direction + changes.value.length) % changes.value.length;
  const line = Math.min(view.state.doc.lines, changes.value[active.value]!.newFrom + 1);
  const position = view.state.doc.line(line).from;
  view.dispatch({ selection: { anchor: position }, effects: EditorView.scrollIntoView(position, { y: "center" }) });
  decorate();
}
const normalize = (value: string) => value.replace(/\r\n/g, "\n");
const modeExtensions = () => [EditorState.readOnly.of(props.readonly), EditorView.editable.of(!props.readonly), EditorView.contentAttributes.of({ "aria-label": props.label })];
let scrollFrame = 0;
let releaseFrame = 0;
let synchronizing = false;
function getScrollPosition() {
  if (!view) return { line: 0, left: 0 };
  const height = Math.max(0, view.scrollDOM.scrollTop - view.documentPadding.top);
  const block = view.lineBlockAtHeight(height);
  return { line: view.state.doc.lineAt(block.from).number - 1 + (height - block.top) / Math.max(1, block.height), left: view.scrollDOM.scrollLeft };
}
function onScroll() {
  if (synchronizing) return;
  cancelAnimationFrame(scrollFrame);
  scrollFrame = requestAnimationFrame(() => {
    if (!synchronizing && view) emit("viewport-scroll", getScrollPosition());
  });
}
function scrollTo(position: { line: number; left: number }) {
  if (!view) return;
  synchronizing = true;
  cancelAnimationFrame(scrollFrame);
  cancelAnimationFrame(releaseFrame);
  const line = Math.max(0, Math.min(view.state.doc.lines - 1, position.line));
  // Let CodeMirror render the destination before scrolling. Setting scrollTop
  // alone can leave a newly mounted/virtualized editor with a blank viewport.
  view.dispatch({ effects: EditorView.scrollIntoView(view.state.doc.line(Math.floor(line) + 1).from, {
    y: "start", yMargin: -(line % 1) * view.defaultLineHeight,
  }) });
  releaseFrame = requestAnimationFrame(() => {
    if (!view) return;
    view.scrollDOM.scrollLeft = position.left;
    releaseFrame = requestAnimationFrame(() => { synchronizing = false; });
  });
}
function userScroll() { synchronizing = false; }
defineExpose({ scrollTo, getScrollPosition });
onMounted(() => {
  view = new EditorView({ parent: host.value, state: EditorState.create({ doc: normalize(props.modelValue), extensions: [
    lineNumbers(), history(), keymap.of([...defaultKeymap, ...historyKeymap]), mode.of(modeExtensions()), highlights,
    EditorView.updateListener.of(update => { if (update.docChanged && !props.readonly && !replacing) emit("update:modelValue", update.state.doc.toString()); }),
    EditorView.theme({ "&": { height: "100%", color: "var(--text)", backgroundColor: "var(--surface-panel)" }, ".cm-scroller": { overflow: "auto", fontFamily: "var(--font-code)", fontSize: "12px" }, ".cm-gutters": { backgroundColor: "var(--surface-muted)", color: "var(--text-muted)", borderColor: "var(--border)" }, ".cm-content": { caretColor: "var(--text)" }, ".cm-cursor": { borderLeftColor: "var(--text)" }, "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--primary-soft)" } }),
  ] }) });
  view.scrollDOM.addEventListener("scroll", onScroll, { passive: true });
  view.dom.addEventListener("wheel", userScroll, { passive: true });
  view.dom.addEventListener("pointerdown", userScroll, { passive: true });
  view.dom.addEventListener("keydown", userScroll);
  updateComparison();
  fontObserver = new MutationObserver(() => { view?.requestMeasure(); void document.fonts?.ready.then(() => view?.requestMeasure()); });
  fontObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["style"] });
});
watch(() => props.modelValue, value => {
  const next = normalize(value);
  if (view && view.state.doc.toString() !== next) {
    replacing = true;
    try { view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next } }); }
    finally { replacing = false; }
  }
});
watch(() => [props.readonly, props.label], () => view?.dispatch({ effects: mode.reconfigure(modeExtensions()) }));
watch(() => [props.modelValue, props.compareText, props.mergeHighlights], () => {
  clearTimeout(comparisonTimer); comparisonTimer = setTimeout(updateComparison, 80);
});
onBeforeUnmount(() => {
  clearTimeout(comparisonTimer); cancelAnimationFrame(scrollFrame); cancelAnimationFrame(releaseFrame);
  fontObserver?.disconnect();
  view?.scrollDOM.removeEventListener("scroll", onScroll);
  view?.dom.removeEventListener("wheel", userScroll);
  view?.dom.removeEventListener("pointerdown", userScroll);
  view?.dom.removeEventListener("keydown", userScroll);
  view?.destroy(); view = undefined;
});
</script>
<template><div class="conflict-editor">
  <div v-if="compareText !== undefined || mergeHighlights !== undefined" class="diff-navigation">
    <span>{{ comparisonLabel || '版本差异' }} <small aria-live="polite">{{ changes.length ? `${active < 0 ? '—' : active + 1} / ${changes.length} 处差异` : '无差异' }}</small></span>
    <button :aria-label="label + '：上一处差异'" title="上一处差异" :disabled="!changes.length" @click="jump(-1)"><ArrowUp :size="15" /></button>
    <button :aria-label="label + '：下一处差异'" title="下一处差异" :disabled="!changes.length" @click="jump(1)"><ArrowDown :size="15" /></button>
  </div>
  <div ref="host" class="editor-host" />
</div></template>
<style scoped>
.conflict-editor { display: flex; flex-direction: column; min-width: 0; min-height: 0; height: 100%; overflow: hidden; }
.editor-host { flex: 1; min-height: 0; overflow: hidden; }
.diff-navigation { display: flex; align-items: center; gap: 4px; padding: 4px 7px; border-bottom: 1px solid var(--border); background: var(--surface-muted); font-size: 11px; }
.diff-navigation span { flex: 1; min-width: 0; }.diff-navigation small { margin-left: 6px; color: var(--text-muted); }
.diff-navigation button { display: grid; place-items: center; width: 26px; height: 26px; border: 1px solid var(--border); border-radius: 4px; background: var(--surface-panel); color: var(--text); }.diff-navigation button:disabled { opacity: .4; cursor: not-allowed; }
:deep(.cm-diff-addition) { background: var(--diff-add-bg); box-shadow: inset 3px 0 var(--success); }
:deep(.cm-diff-change) { background: color-mix(in srgb, var(--warning) 18%, transparent); box-shadow: inset 3px 0 var(--warning); }
:deep(.cm-diff-deletion) { background: var(--diff-remove-bg, var(--danger-soft)); box-shadow: inset 3px 0 var(--danger); }
:deep(.cm-diff-conflict) { background: color-mix(in srgb, var(--danger) 20%, var(--surface-panel)); box-shadow: inset 3px 0 var(--danger); }
:deep(.cm-diff-mergeable) { background: color-mix(in srgb, #edc83b 26%, var(--surface-panel)); box-shadow: inset 3px 0 #c7a125; }
:deep(.cm-diff-current) { outline: 1px solid var(--primary); outline-offset: -1px; }
</style>
