<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from "vue";
import { Check, RefreshCw, Trash2, ArrowUp, ArrowDown, ChevronsUp, ChevronsDown, Undo2, Redo2, ArrowRightToLine, ArrowLeftToLine, Sparkles } from "@lucide/vue";
import type { ConflictContentKind } from "@/lib/backend/types";
import { useConflictsStore } from "@/stores/conflicts";
import ConflictEditor from "./ConflictEditor.vue";
import { conflictDiff, type ConflictDiffRange } from "./conflictDiff";
import { compareMerge, mapDiffLine, resultHighlights, type MergeHighlight } from "./conflictMerge";
import { useConflictSuggestionStore } from "@/stores/conflictSuggestion";
import { applyConflictBlock, resultBlockRange, type BlockSide, type BlockChoice } from "./conflictBlocks";
const conflicts = useConflictsStore();
const suggestion = useConflictSuggestionStore();
function startSuggestion(): void { void suggestion.start().catch(() => undefined); }
const tab = ref<"base" | "ours">("ours");
const detail = computed(() => conflicts.current?.detail);
const source = computed(() => detail.value?.[tab.value]);
const isRebase = computed(() => detail.value?.operationKind === "rebase");
const tabs = computed(() => [{ key: "base" as const, get label() { return t('uiCommonAncestora4dcb8'); } }, { key: "ours" as const, label: isRebase.value ? t('uiRebaseTargetReplayedCommits7b96b5') : t('uiCurrentVersion46e66f') }, { key: "theirs" as const, label: isRebase.value ? t('uiCommitBeingReplayedb4080d') : t('uiIncomingVersion931ad5') }]);
const resolution = computed(() => conflicts.current?.resolution);
const resultText = computed(() => resolution.value?.kind === "text" ? resolution.value.text : resolution.value?.kind === "ours" || resolution.value?.kind === "theirs" ? detail.value?.[resolution.value.kind].text ?? "" : "");
const syncEnabled = ref(true);
const wrapLines = ref(false);
const showWhitespace = ref(false);
const historyState = ref({ undo: false, redo: false });
const activeRegion = ref(-1);
const feedback = ref("");
const contextMenu = ref<{ side: BlockSide; index: number; x: number; y: number; revision: number }>();
const menuElement = ref<HTMLElement>();
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
const conflictCount = computed(() => regions.value?.filter(region => region.kind === "conflict").length ?? 0);
const activeBlock = computed(() => regions.value?.[activeRegion.value]);
const canApplyBlock = computed(() => !!detail.value?.editable && !conflicts.busy && resolution.value?.kind !== "delete" && !!activeBlock.value && baseText.value !== undefined && !!resultBlockRange(baseText.value, resultText.value, activeBlock.value));
function revealRegion(index: number): void {
  const region = regions.value?.[index];
  if (!region) return;
  activeRegion.value = index;
  leftEditor.value?.reveal?.(region[tab.value].from);
  rightEditor.value?.reveal?.(region.theirs.from);
  const resultRange = baseText.value === undefined ? undefined : resultBlockRange(baseText.value, resultText.value, region);
  resultEditor.value?.reveal?.(resultRange?.from ?? mapDiffLine(draftComparison.value.left, region[tab.value].from));
}
function navigate(direction: number, conflictsOnly = false): void {
  const indexes = (regions.value ?? []).flatMap((region, index) => !conflictsOnly || region.kind === "conflict" ? [index] : []);
  if (!indexes.length) return;
  const index = direction > 0 ? indexes.find(index => index > activeRegion.value) ?? indexes[0]! : [...indexes].reverse().find(index => activeRegion.value < 0 || index < activeRegion.value) ?? indexes.at(-1)!;
  revealRegion(index);
}
function selectBlock(side: "base" | BlockSide, line: number): number {
  const lastLine = (textOf(detail.value?.[side]) ?? "").replace(/\r\n/g, "\n").split("\n").length - 1;
  const index = regions.value?.findIndex(region => {
    const range = region[side];
    // Empty/deleted blocks at EOF are painted on the last visible line.
    const from = range.from === range.to ? Math.min(range.from, lastLine) : range.from;
    return line >= from && line < Math.max(range.to, from + 1);
  }) ?? -1;
  activeRegion.value = index;
  return index;
}
async function openBlockMenu(side: BlockSide, position: { line: number; x: number; y: number }) {
  if (conflicts.busy) return;
  const index = selectBlock(side, position.line);
  if (index >= 0) revealRegion(index);
  contextMenu.value = { side, index, x: position.x, y: position.y, revision: conflicts.draftRevision };
  await nextTick();
  if (!contextMenu.value || !menuElement.value) return;
  const bounds = menuElement.value.getBoundingClientRect();
  const margin = 8;
  contextMenu.value.x = Math.max(margin, Math.min(position.x, window.innerWidth - bounds.width - margin));
  contextMenu.value.y = Math.max(margin, Math.min(position.y, window.innerHeight - bounds.height - margin));
  menuElement.value.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
}
function closeMenu() { contextMenu.value = undefined; }
function menuKey(event: KeyboardEvent) {
  if (event.key === "Escape" || event.key === "Tab") { closeMenu(); return; }
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  const items = [...(menuElement.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
  const current = items.indexOf(document.activeElement as HTMLButtonElement);
  const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
  items[next]?.focus();
}
function applyBlock(side: BlockSide, choice: BlockChoice) {
  const menu = contextMenu.value;
  if (menu && (menu.revision !== conflicts.draftRevision || menu.index !== activeRegion.value)) { closeMenu(); return; }
  closeMenu();
  if (!canApplyBlock.value || !activeBlock.value || baseText.value === undefined || !detail.value) return;
  const next = applyConflictBlock({ base: baseText.value, ours: textOf(detail.value.ours)!, theirs: rightText.value!, result: resultText.value, region: activeBlock.value, side, choice });
  if (next === undefined) return;
  conflicts.edit(next);
  feedback.value = t('uiResolutionUpdatedBelowContinueEditingOrUndoThenClickMarkAsRe9928ca');
  const index = activeRegion.value;
  void nextTick(() => revealRegion(index));
}
function useWholeFile(side: BlockSide) {
  if (conflicts.busy || (contextMenu.value && contextMenu.value.revision !== conflicts.draftRevision)) return;
  closeMenu(); conflicts.choose(side);
  feedback.value = t('uiTheCompleteVersionWasPlacedInTheResultCheckItBeforeMarkingAs9bd463');
}
watch(() => [conflicts.selectedPath, detail.value?.token, conflicts.loadedRootPath, conflicts.generation], () => {
  closeMenu(); activeRegion.value = -1; feedback.value = ""; historyState.value = { undo: false, redo: false };
});
watch(() => [conflicts.draftRevision, conflicts.busy, tab.value], closeMenu);
watch(() => resolution.value?.kind, kind => { if (kind === "delete") historyState.value = { undo: false, redo: false }; });
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
const notices: Record<ConflictContentKind, string> = { get missing() { return t('uiThisVersionDoesNotExistDeleted77dfa6'); }, text: "", get binary() { return t('uiBinaryFileUseACompleteVersione90e1c'); }, get unsupportedEncoding() { return t('uiNonUTF8ContentUseACompleteVersion4ac3c4'); }, get tooLarge() { return t('uiOver2MiBUseACompleteVersion2ad4a2'); }, get mixedLineEndings() { return t('uiMixedLineEndingsUseACompleteVersion5becb1'); }, get unsupported() { return t('uiThisFileTypeCannotBeEditedHere8e3df3'); } };
watch(() => conflicts.selectedPath, () => { tab.value = "ours"; });
</script>
<template>
  <section class="conflict-detail">
    <div v-if="conflicts.detailLoading" class="module-state" role="status">{{ t('uiReadingFileVersions4d75ce') }}</div>
    <div v-else-if="conflicts.detailError" class="module-state error" role="alert">{{ conflicts.detailError.message }}<details v-if="conflicts.detailError.diagnostics"><summary>{{ t('uiDiagnostics0b673e') }}</summary><pre>{{ conflicts.detailError.diagnostics }}</pre></details><button @click="conflicts.requestReload">{{ t('uiRetrye2d53a') }}</button></div>
    <template v-else-if="detail">
      <header><strong>{{ detail.path }}</strong><span v-if="conflicts.current?.dirty" class="draft">{{ t('uiUnsavedDraft10c8bf') }}</span><button :title="t('uiReloadFile7db995')" :aria-label="t('uiReloadFile7db995')" :disabled="conflicts.busy" @click="conflicts.requestReload"><RefreshCw :size="15" /></button><button class="save" :aria-label="t('uiSaveAndMarkResolvedf0e27c')" :disabled="!conflicts.canSave" @click="conflicts.requestSave"><Check :size="16" />Mark as Resolved</button></header>
      <p v-if="detail.unsupportedReason" class="warning" role="alert">{{ detail.unsupportedReason }}</p>
      <div class="workbench-ribbon" role="toolbar" :aria-label="t('uiConflictResolutionToolbar0875e4')">
        <div class="ribbon-group">
          <button :aria-label="t('uiPreviousDifferencef30de6')" :title="t('uiPreviousDifferencef30de6')" :disabled="!regions?.length" @click="navigate(-1)"><ArrowUp :size="17" />Prev Diff</button>
          <button :aria-label="t('uiNextDifference392010')" :title="t('uiNextDifference392010')" :disabled="!regions?.length" @click="navigate(1)"><ArrowDown :size="17" />Next Diff</button>
          <button :aria-label="t('uiPreviousConflict2de3b2')" :title="t('uiPreviousConflict2de3b2')" :disabled="!conflictCount" @click="navigate(-1, true)"><ChevronsUp :size="17" />Prev Conflict</button>
          <button :aria-label="t('uiNextConflicte3051a')" :title="t('uiNextConflicte3051a')" :disabled="!conflictCount" @click="navigate(1, true)"><ChevronsDown :size="17" />Next Conflict</button>
        </div>
        <div class="ribbon-group">
          <button :aria-label="t('uiUseCurrentVersionHunk0f6f4d')" :disabled="!canApplyBlock" :title="t('uiCopyTheCurrentVersionOfTheSelectedHunkIntoTheResult79fc21')" @click="applyBlock('ours', 'this')"><ArrowRightToLine :size="17" />Use Ours</button>
          <button :aria-label="t('uiUseIncomingVersionHunk038155')" :disabled="!canApplyBlock" :title="t('uiCopyTheIncomingVersionOfTheSelectedHunkIntoTheResult98ef49')" @click="applyBlock('theirs', 'this')"><ArrowLeftToLine :size="17" />Use Theirs</button>
        </div>
        <div class="ribbon-group">
          <button :aria-label="t('uiUndoResolutionEditac2594')" :disabled="conflicts.busy || !historyState.undo" @click="resultEditor?.undo()"><Undo2 :size="17" />Undo</button>
          <button :aria-label="t('uiRedoResolutionEdit8165c3')" :disabled="conflicts.busy || !historyState.redo" @click="resultEditor?.redo()"><Redo2 :size="17" />Redo</button>
          <button :aria-label="t('uiAIResolutionSuggestion38d6eb')" :disabled="!!suggestion.startReason" :title="suggestion.startReason || t('uiGenerateSuggestionsFromTheThreeVersionsAndOnDiskFileUnsavedDa830d6')" @click="startSuggestion"><Sparkles :size="17" />AI Suggest</button>
        </div>
      </div>
      <div class="ai-scope">{{ t('uiAIUsesTheThreeVersionsAndOnDiskFileUnsavedDraftsAreExcluded6d290a') }}<span v-if="suggestion.startReason">{{ suggestion.startReason }}</span></div>
      <div class="merge-toolbar">
        <div class="merge-legend" :aria-label="t('uiThreeWayDiffLegendeaa939')"><span class="legend-conflict">{{ t('uiConflictConfirmationNeedede208ee') }}</span><span class="legend-mergeable">{{ t('uiCanMergeAutomatically99030a') }}</span></div>
        <label class="sync-toggle"><input v-model="syncEnabled" type="checkbox" />{{ t('uiSynchronizedScrolling0137ff') }}</label>
        <label class="sync-toggle"><input v-model="wrapLines" type="checkbox" />Wrap Lines</label>
        <label class="sync-toggle"><input v-model="showWhitespace" type="checkbox" />Show Whitespaces</label>
        <div class="adopt"><button :aria-label="t('uiUseIndexStage2e60996')" :title="t('uiUseEntireFilebb0069') + tabs[1]?.label" :disabled="!detail.canChooseOurs || conflicts.busy" @click="useWholeFile('ours')">Use Ours File</button><button :aria-label="t('uiUseIndexStage34fdc42')" :title="t('uiUseEntireFilebb0069') + tabs[2]?.label" :disabled="!detail.canChooseTheirs || conflicts.busy" @click="useWholeFile('theirs')">Use Theirs File</button><button :title="t('uiDeleteFile935cd5')" :aria-label="t('uiDeleteFileAsResolution7aa0b6')" :disabled="!detail.canDelete || conflicts.busy" @click="conflicts.choose('delete')"><Trash2 :size="15" /></button></div>
      </div>
      <p v-if="!regions" class="warning">{{ t('uiCompleteThreeWayTextIsUnavailableAutomaticallyMergeableAreas0dbbd9') }}</p>
      <div class="block-status" role="status"><span>{{ activeRegion < 0 ? '—' : activeRegion + 1 }} / {{ regions?.length ?? 0 }} {{ t('uidifferences3d20b8') }} {{ conflictCount }} {{ t('uioriginalConflicts21f5aa') }}</span><span>{{ feedback || (activeBlock && !canApplyBlock && detail.editable && !conflicts.busy ? t('uiTheDraftCrossedThisHunkBoundaryOrIsMarkedForDeletionEditTheR26257c') : t('uiClickAHunkToSelectItRightClickToUseOneSideOrKeepBothSidesInOdbd38c')) }}</span></div>
      <div class="versions">
        <section class="version">
          <div class="tabs" role="tablist" :aria-label="t('uiConflictSourceVersionsa8bcd6')"><button v-for="item in tabs.filter(item => item.key !== 'theirs')" :key="item.key" role="tab" :aria-selected="tab === item.key" @click="tab = item.key as 'base' | 'ours'">{{ item.label }}</button></div>
          <div class="metadata">{{ tab === 'base' ? t('uiIndexStage1d15fd4') : tab === 'ours' ? t('uiIndexStage2fe4c48') : t('uiIndexStage336233c') }} · {{ source?.byteLength ?? 0 }} bytes · {{ source?.lineEnding }} {{ source?.bom ? 'UTF-8 BOM' : '' }}<code>{{ source?.oid }}</code></div>
          <ConflictEditor v-if="leftText !== undefined" ref="leftEditor" :key="tab + detail.path + detail.token" :model-value="leftText" :merge-highlights="leftHighlights" :wrap-lines="wrapLines" :show-whitespace="showWhitespace" hide-navigation :block-actions="tab === 'ours'" :comparison-label="t('uiThreeWayDiffeb3f14')" readonly :label="t('uiReadOnlyffc1d0') + tabs.find(item => item.key === tab)?.label" @block-context="openBlockMenu('ours', $event)" @block-select="selectBlock(tab, $event)" @viewport-scroll="synchronize('left', $event)" />
          <div v-else class="content-state">{{ source ? notices[source.kind] : '' }}</div>
        </section>
        <section class="version">
          <h2>{{ tabs[2]?.label }}</h2>
          <div class="metadata">{{ t('uiIndexStage36d235f') }} {{ detail.theirs.byteLength }} bytes · {{ detail.theirs.lineEnding }} {{ detail.theirs.bom ? 'UTF-8 BOM' : '' }}<code>{{ detail.theirs.oid }}</code></div>
          <ConflictEditor v-if="rightText !== undefined" ref="rightEditor" :key="detail.path + detail.token" :model-value="rightText" :merge-highlights="rightHighlights" :wrap-lines="wrapLines" :show-whitespace="showWhitespace" hide-navigation block-actions :comparison-label="t('uiThreeWayDiffeb3f14')" readonly :label="t('uiReadOnlyffc1d0') + tabs[2]?.label" @block-context="openBlockMenu('theirs', $event)" @block-select="selectBlock('theirs', $event)" @viewport-scroll="synchronize('right', $event)" />
          <div v-else class="content-state">{{ notices[detail.theirs.kind] }}</div>
        </section>
        <section class="version result"><h2>{{ t('uiResolutionResult638c90') }} <small>{{ resolution?.kind === 'ours' ? t('uiUseIndexStage2e60996') : resolution?.kind === 'theirs' ? t('uiUseIndexStage34fdc42') : resolution?.kind === 'delete' ? t('uiDeleteFile935cd5') : t('uiManualEditing697762') }}</small></h2>
          <ConflictEditor v-if="detail.editable && resolution?.kind !== 'delete'" ref="resultEditor" :key="detail.path + detail.token" :model-value="resultText" :merge-highlights="draftComparison.highlights" :wrap-lines="wrapLines" :show-whitespace="showWhitespace" hide-navigation :comparison-label="t('uiThreeWayDiffeb3f14')" :readonly="conflicts.busy" :label="t('uiResolutionEditor943c2d')" @history-change="historyState = $event" @update:model-value="conflicts.edit" @viewport-scroll="synchronize('result', $event)" />
          <div v-else class="content-state">{{ resolution?.kind === 'delete' ? t('uiFileWillBeDeletedAfterConfirmation9aa2d2') : notices[detail.working.kind] || t('uiTheSelectedCompleteVersionWillBeUsedca38f7') }}</div>
        </section>
      </div>
      <footer><span>{{ detail.working.lineEnding.toUpperCase() }} {{ detail.working.bom ? 'UTF-8 BOM' : '' }}</span><span>{{ t('uiReadOnlySourceVersionsAboveResultToSaveBelowMarkAsResolvedSa2cd42c') }}</span></footer>
      <Teleport to="body"><div v-if="contextMenu" class="conflict-menu-shield" @pointerdown.self="closeMenu" @contextmenu.prevent.self="closeMenu">
        <div ref="menuElement" role="menu" :aria-label="t('uiConflictHunkActions5a5315')" class="conflict-block-menu" :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }" @keydown="menuKey" @contextmenu.prevent>
          <div class="menu-heading">{{ contextMenu.side === 'ours' ? tabs[1]?.label : tabs[2]?.label }} · {{ contextMenu.index < 0 ? t('uiUnchangedArea08c77b') : t('msgDifference4b172c', { p0: contextMenu.index + 1 }) }}</div>
          <button role="menuitem" :disabled="!canApplyBlock" @click="applyBlock(contextMenu.side, 'this')">Use this text block<small>{{ t('uiUseThisSideOnly558b01') }}</small></button>
          <button role="menuitem" :disabled="!canApplyBlock" @click="applyBlock(contextMenu.side, 'first')">Use both text blocks (this one first)<small>{{ t('uiKeepBothThisSideFirst8f1e88') }}</small></button>
          <button role="menuitem" :disabled="!canApplyBlock" @click="applyBlock(contextMenu.side, 'last')">Use both text blocks (this one last)<small>{{ t('uiKeepBothThisSideLastf0129f') }}</small></button>
          <button role="menuitem" :disabled="conflicts.busy || !(contextMenu.side === 'ours' ? detail.canChooseOurs : detail.canChooseTheirs)" @click="useWholeFile(contextMenu.side)">Use this whole file<small>{{ t('uiUseThisSideSEntireFile2ec039') }}</small></button>
        </div>
      </div></Teleport>
    </template>
    <div v-else class="module-state">{{ conflicts.snapshot?.files.length ? t('uiSelectAConflictedFile9d4c85') : t('uiNoConflictedFileSelected2a1071') }}</div>
  </section>
</template>
<style scoped>
.conflict-detail { grid-row: 1 / -1; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
.ai-scope { padding: 4px 10px; color: var(--text-muted); font-size: 11px; }.ai-scope span { margin-left: 8px; }
.workbench-ribbon { display: flex; flex-wrap: wrap; gap: 8px; padding: 6px 10px; background: var(--surface-muted); border-bottom: 1px solid var(--border); }
.ribbon-group { display: flex; flex-wrap: wrap; gap: 3px; padding-right: 8px; border-right: 1px solid var(--border); }.ribbon-group:last-child { border-right: 0; }
.ribbon-group button { display: flex; align-items: center; flex-direction: column; justify-content: center; gap: 5px; padding: 7px 9px; border: 1px solid transparent; border-radius: 4px; background: transparent; color: var(--text); font-size: 11px; }
.ribbon-group button:hover:not(:disabled) { background: var(--primary-soft); border-color: var(--primary-border); color: var(--primary); }
.block-status { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 16px; padding: 6px 10px; border-bottom: 1px solid var(--border); color: var(--text-muted); font-size: 11px; }.block-status span:first-child { color: var(--text); flex-shrink: 0; }
header button.save { width: auto; height: auto; background: var(--primary); padding: 8px 10px; }
.conflict-menu-shield { position: fixed; inset: 0; z-index: 1200; }
.conflict-block-menu { position: fixed; width: max-content; min-width: 260px; max-width: calc(100vw - 16px); max-height: calc(100vh - 16px); overflow: auto; padding: 5px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-panel); color: var(--text); box-shadow: 0 8px 30px #0003; }
.menu-heading { padding: 6px 9px; color: var(--text-muted); font-size: 11px; border-bottom: 1px solid var(--border); }
.conflict-block-menu button { display: block; width: 100%; padding: 8px 9px; text-align: left; background: transparent; color: var(--text); border-radius: 3px; font-size: 12px; }.conflict-block-menu button:last-child { border-top: 1px solid var(--border); }
.conflict-block-menu button:hover:not(:disabled), .conflict-block-menu button:focus-visible { background: var(--primary-soft); outline: 1px solid var(--primary); }
.conflict-block-menu small { display: block; color: var(--text-muted); font-size: 11px; margin-top: 3px; }
button:disabled { opacity: .5; cursor: not-allowed; }
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
