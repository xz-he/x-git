<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from "vue";
import { Check, Copy, RefreshCw, Trash2 } from "@lucide/vue";
import type { ConflictContentKind } from "@/lib/backend/types";
import { useConflictsStore } from "@/stores/conflicts";
import ConflictEditor from "./ConflictEditor.vue";
import { conflictDiff, type ConflictDiffRange } from "./conflictDiff";
import { compareMerge, mapDiffLine, resultHighlights, type MergeHighlight } from "./conflictMerge";
import { useConflictSuggestionStore } from "@/stores/conflictSuggestion";
const conflicts = useConflictsStore();
const suggestion = useConflictSuggestionStore();
function startSuggestion(): void { void suggestion.start().catch(() => undefined); }
const tab = ref<"base" | "ours">("ours");
const detail = computed(() => conflicts.current?.detail);
const source = computed(() => detail.value?.[tab.value]);
const isRebase = computed(() => detail.value?.operationKind === "rebase");
const tabs = computed(() => [{ key: "base" as const, label: "共同祖先" }, { key: "ours" as const, label: isRebase.value ? "变基目标 + 已重放提交" : "当前版本" }, { key: "theirs" as const, label: isRebase.value ? "正在重放的提交" : "传入版本" }]);
const resolution = computed(() => conflicts.current?.resolution);
const resultText = computed(() => resolution.value?.kind === "text" ? resolution.value.text : resolution.value?.kind === "ours" || resolution.value?.kind === "theirs" ? detail.value?.[resolution.value.kind].text ?? "" : "");
const syncEnabled = ref(true);
const leftEditor = ref<InstanceType<typeof ConflictEditor>>();
const rightEditor = ref<InstanceType<typeof ConflictEditor>>();
const resultEditor = ref<InstanceType<typeof ConflictEditor>>();
const textOf = (version: typeof source.value) => version?.kind === "missing" ? "" : version?.text ?? undefined;
const baseText = computed(() => textOf(detail.value?.base));
const leftText = computed(() => textOf(source.value));
const rightText = computed(() => textOf(detail.value?.theirs));
const regions = computed(() => {
  const base = baseText.value, ours = textOf(detail.value?.ours), theirs = rightText.value;
  return base !== undefined && ours !== undefined && theirs !== undefined ? compareMerge(base, ours, theirs) : undefined;
});
const leftHighlights = computed(() => regions.value?.map(region => tab.value === "base" ? { ...region.base, kind: region.kind } : region.ours));
const rightHighlights = computed(() => regions.value?.map(region => region.theirs));
const leftRight = computed(() => leftText.value !== undefined && rightText.value !== undefined ? conflictDiff(leftText.value, rightText.value) : []);
const draftComparison = shallowRef<{ highlights?: MergeHighlight[]; left: ConflictDiffRange[]; right: ConflictDiffRange[] }>({ left: [], right: [] });
let comparisonTimer: ReturnType<typeof setTimeout> | undefined;
watch([() => detail.value?.token, leftText, rightText, resultText, regions], (next, previous) => {
  clearTimeout(comparisonTimer);
  const update = () => {
    draftComparison.value = {
      highlights: regions.value && baseText.value !== undefined ? resultHighlights(baseText.value, resultText.value, regions.value) : undefined,
      left: leftText.value !== undefined ? conflictDiff(leftText.value, resultText.value) : [],
      right: rightText.value !== undefined ? conflictDiff(rightText.value, resultText.value) : [],
    };
  };
  if (next[0] !== previous?.[0] || next[1] !== previous?.[1] || next[2] !== previous?.[2]) update();
  else comparisonTimer = setTimeout(update, 80);
}, { immediate: true });
type Pane = "left" | "right" | "result";
let lastPane: Pane = "left";
function synchronize(pane: Pane, position: { line: number; left: number }) {
  lastPane = pane;
  if (!syncEnabled.value) return;
  const editors = { left: leftEditor.value, right: rightEditor.value, result: resultEditor.value };
  for (const target of ["left", "right", "result"] as const) {
    if (target === pane) continue;
    const pair = pane === "left" || target === "left"
      ? pane === "right" || target === "right" ? leftRight.value : draftComparison.value.left
      : draftComparison.value.right;
    const reverse = pane === "result" || (pane === "right" && target === "left");
    editors[target]?.scrollTo?.({ line: mapDiffLine(pair, position.line, reverse), left: position.left });
  }
}
watch(syncEnabled, enabled => {
  const editor = { left: leftEditor.value, right: rightEditor.value, result: resultEditor.value }[lastPane];
  if (enabled && editor?.getScrollPosition) synchronize(lastPane, editor.getScrollPosition());
});
watch(tab, async (next, previous) => {
  const position = leftEditor.value?.getScrollPosition?.();
  const before = textOf(detail.value?.[previous]), after = textOf(detail.value?.[next]);
  const token = detail.value?.token;
  if (!position || before === undefined || after === undefined) return;
  const mapped = { ...position, line: mapDiffLine(conflictDiff(before, after), position.line) };
  await nextTick();
  if (detail.value?.token !== token || tab.value !== next) return;
  leftEditor.value?.scrollTo?.(mapped);
  synchronize("left", mapped);
});
onBeforeUnmount(() => clearTimeout(comparisonTimer));
const notices: Record<ConflictContentKind, string> = { missing: "此版本不存在（删除）", text: "", binary: "二进制文件，只能采用完整版本", unsupportedEncoding: "非 UTF-8 内容，只能采用完整版本", tooLarge: "超过 2 MiB，只能采用完整版本", mixedLineEndings: "混合换行符，只能采用完整版本", unsupported: "不支持在此处修改此文件类型" };
watch(() => conflicts.selectedPath, () => { tab.value = "ours"; });
</script>
<template>
  <section class="conflict-detail">
    <div v-if="conflicts.detailLoading" class="module-state" role="status">正在读取文件版本...</div>
    <div v-else-if="conflicts.detailError" class="module-state error" role="alert">{{ conflicts.detailError.message }}<details v-if="conflicts.detailError.diagnostics"><summary>诊断信息</summary><pre>{{ conflicts.detailError.diagnostics }}</pre></details><button @click="conflicts.requestReload">重试</button></div>
    <template v-else-if="detail">
      <header><strong>{{ detail.path }}</strong><span v-if="conflicts.current?.dirty" class="draft">未保存草稿</span><button title="重新读取文件" aria-label="重新读取文件" :disabled="conflicts.busy" @click="conflicts.requestReload"><RefreshCw :size="15" /></button></header>
      <p v-if="detail.unsupportedReason" class="warning" role="alert">{{ detail.unsupportedReason }}</p>
      <div class="ai-action"><button aria-label="AI 解决建议" :disabled="!!suggestion.startReason" :title="suggestion.startReason" @click="startSuggestion">AI 解决建议</button><span>三方版本与磁盘工作文件；未保存草稿不发送。<small v-if="suggestion.startReason">{{ suggestion.startReason }}</small></span></div>
      <div class="merge-toolbar">
        <div class="merge-legend" aria-label="三方差异图例"><span class="legend-conflict">冲突，需确认</span><span class="legend-mergeable">可自动合并</span></div>
        <label class="sync-toggle"><input v-model="syncEnabled" type="checkbox" />同步滚动</label>
        <div class="adopt"><button aria-label="采用索引版本 2" :disabled="!detail.canChooseOurs || conflicts.busy" @click="conflicts.choose('ours')"><Copy :size="14" />采用版本 2</button><button aria-label="采用索引版本 3" :disabled="!detail.canChooseTheirs || conflicts.busy" @click="conflicts.choose('theirs')"><Copy :size="14" />采用版本 3</button><button title="删除文件" aria-label="删除文件作为解决结果" :disabled="!detail.canDelete || conflicts.busy" @click="conflicts.choose('delete')"><Trash2 :size="15" /></button></div>
      </div>
      <p v-if="!regions" class="warning">无法读取完整三方文本，暂不判断可自动合并区域。</p>
      <div class="versions">
        <section class="version">
          <div class="tabs" role="tablist" aria-label="冲突源版本"><button v-for="item in tabs.filter(item => item.key !== 'theirs')" :key="item.key" role="tab" :aria-selected="tab === item.key" @click="tab = item.key as 'base' | 'ours'">{{ item.label }}</button></div>
          <div class="metadata">{{ tab === 'base' ? '索引版本 1' : tab === 'ours' ? '索引版本 2' : '索引版本 3' }} · {{ source?.byteLength ?? 0 }} bytes · {{ source?.lineEnding }} {{ source?.bom ? 'UTF-8 BOM' : '' }}<code>{{ source?.oid }}</code></div>
          <ConflictEditor v-if="leftText !== undefined" ref="leftEditor" :key="tab + detail.token" :model-value="leftText" :merge-highlights="leftHighlights" comparison-label="三方差异" readonly :label="'只读' + tabs.find(item => item.key === tab)?.label" @viewport-scroll="synchronize('left', $event)" />
          <div v-else class="content-state">{{ source ? notices[source.kind] : '' }}</div>
        </section>
        <section class="version">
          <h2>{{ tabs[2]?.label }}</h2>
          <div class="metadata">索引版本 3 · {{ detail.theirs.byteLength }} bytes · {{ detail.theirs.lineEnding }} {{ detail.theirs.bom ? 'UTF-8 BOM' : '' }}<code>{{ detail.theirs.oid }}</code></div>
          <ConflictEditor v-if="rightText !== undefined" ref="rightEditor" :key="detail.token" :model-value="rightText" :merge-highlights="rightHighlights" comparison-label="三方差异" readonly :label="'只读' + tabs[2]?.label" @viewport-scroll="synchronize('right', $event)" />
          <div v-else class="content-state">{{ notices[detail.theirs.kind] }}</div>
        </section>
        <section class="version result"><h2>解决结果 <small>{{ resolution?.kind === 'ours' ? '采用索引版本 2' : resolution?.kind === 'theirs' ? '采用索引版本 3' : resolution?.kind === 'delete' ? '删除文件' : '手动编辑' }}</small></h2>
          <ConflictEditor v-if="detail.editable && resolution?.kind !== 'delete'" ref="resultEditor" :key="detail.token" :model-value="resultText" :merge-highlights="draftComparison.highlights" comparison-label="三方差异" :readonly="conflicts.busy" label="解决结果编辑器" @update:model-value="conflicts.edit" @viewport-scroll="synchronize('result', $event)" />
          <div v-else class="content-state">{{ resolution?.kind === 'delete' ? '文件将在确认后删除' : notices[detail.working.kind] || '将采用所选完整版本' }}</div>
        </section>
      </div>
      <footer><span>{{ detail.working.lineEnding.toUpperCase() }} {{ detail.working.bom ? 'UTF-8 BOM' : '' }}</span><button class="save" aria-label="保存并标记已解决" :disabled="!conflicts.canSave" @click="conflicts.requestSave"><Check :size="16" />保存并标记已解决</button></footer>
    </template>
    <div v-else class="module-state">{{ conflicts.snapshot?.files.length ? '选择冲突文件' : '没有选中的冲突文件' }}</div>
  </section>
</template>
<style scoped>
.conflict-detail { grid-row: 1 / -1; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
.ai-action { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid var(--border); }.ai-action button { padding: 7px 10px; border: 1px solid var(--primary-border); border-radius: 4px; background: var(--primary-soft); color: var(--primary); }.ai-action span { flex: 1; min-width: 160px; color: var(--text-muted); font-size: 11px; }.ai-action small { display: block; margin-top: 3px; }button:disabled { opacity: .5; cursor: not-allowed; }
header { min-height: 48px; display: flex; gap: 8px; align-items: center; padding: 8px 14px; border-bottom: 1px solid var(--border); }header strong { flex: 1; overflow-wrap: anywhere; min-width: 0; }header button { background: transparent; width: 28px; height: 28px; flex-shrink: 0; }.draft { color: var(--warning); font-size: 11px; }
.versions { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); grid-template-rows: minmax(0, 1fr) minmax(0, 1fr); flex: 1; min-height: 0; min-width: 0; }.version { display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }.version + .version { border-left: 1px solid var(--border); }.version :deep(.conflict-editor) { flex: 1; }.version.result { grid-column: 1 / -1; border-left: 0; border-top: 1px solid var(--border); }
.tabs { display: flex; flex-wrap: wrap; border-bottom: 1px solid var(--border); }.tabs button { flex: 1; min-height: 34px; padding: 6px 8px; background: transparent; font-size: 11px; }.tabs button[aria-selected=true] { color: var(--primary); background: var(--primary-soft); box-shadow: inset 0 -2px var(--primary); }
.metadata { padding: 5px 10px; font-size: 10px; color: var(--text-muted); overflow-wrap: anywhere; }.metadata code { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }h2 { font-size: 13px; margin: 0; min-height: 35px; padding: 8px 10px; border-bottom: 1px solid var(--border); }h2 small { font-weight: 400; font-size: 10px; color: var(--text-muted); margin-left: 6px; }
.merge-toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; padding: 6px 10px; border-bottom: 1px solid var(--border); font-size: 11px; }
.merge-legend { display: flex; flex-wrap: wrap; gap: 12px; }.merge-legend span { display: inline-flex; align-items: center; gap: 5px; }.merge-legend span::before { content: ''; width: 12px; height: 12px; border-radius: 2px; }.legend-conflict::before { background: color-mix(in srgb, var(--danger) 20%, var(--surface-panel)); border: 1px solid var(--danger); }.legend-mergeable::before { background: color-mix(in srgb, #edc83b 26%, var(--surface-panel)); border: 1px solid #c7a125; }
.sync-toggle { display: flex; align-items: center; gap: 5px; cursor: pointer; }.sync-toggle input { accent-color: var(--primary); }
.adopt { display: flex; flex-wrap: wrap; gap: 6px; margin-left: auto; }.adopt button { display: flex; align-items: center; gap: 4px; padding: 6px; border: 1px solid var(--border); background: var(--surface-panel); border-radius: 4px; font-size: 11px; }
.content-state { flex: 1; padding: 20px; color: var(--text-muted); overflow: auto; }.warning { margin: 0; padding: 10px; color: var(--warning); overflow-wrap: anywhere; }footer { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 6px; min-height: 48px; padding: 8px 12px; border-top: 1px solid var(--border); }footer span { font-size: 10px; color: var(--text-muted); }.save { display: flex; gap: 6px; align-items: center; padding: 8px 10px; background: var(--primary); color: white; border-radius: 4px; }
</style>
