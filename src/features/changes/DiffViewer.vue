<script setup lang="ts">
import { t } from '@/lib/i18n';
import {
  Check,
  FileQuestion,
  Layers,
  LoaderCircle,
  Minus,
  Plus,
} from "@lucide/vue";
import { computed, ref, watch } from "vue";

import type { FileDiff } from "@/lib/backend/types";
import SplitDiff from "@/features/history/SplitDiff.vue";
import type { SplitCell } from "@/features/history/splitDiffRows";
import { useChangesStore } from "@/stores/changes";
import { useTerminalStore } from "@/stores/terminal";

const changesStore = useChangesStore();
const selectedLines = ref<number[]>([]);
const dragAnchor = ref<number>();
const dragging = ref(false);
const diff = computed(() => changesStore.selectedDiff);
const diffHunks = computed(() => diff.value?.hunks.map(hunk => ({ hunk, hunks: [hunk] })) ?? []);
const addedLineNumbers = computed(
  () =>
    new Set(
      diff.value?.hunks.flatMap((hunk) =>
        hunk.lines
          .filter((line) => line.kind === "addition" && line.newLine)
          .map((line) => line.newLine!),
      ) ?? [],
    ),
);
const isStaged = computed(() => diff.value?.scope === "staged");
const canMutate = computed(() => diff.value?.scope !== "commit" && !useTerminalStore().busy);
const busy = computed(() => changesStore.operation.kind !== "idle");
const actionLabel = computed(() =>
  isStaged.value ? t('uiUnstageSelectedLines196037') : t('uiStageSelectedLinesb1e846'),
);
const highlightedLine = computed(() => {
  const lineNumber = changesStore.highlightedLine;
  if (!lineNumber) return undefined;
  const kind = addedLineNumbers.value.has(lineNumber) ? "addition" : "deletion";
  return { lineNumber, kind } as const;
});

watch(
  () => changesStore.selectedDiff,
  () => {
    selectedLines.value = [];
  },
);

function lineAnchor(line: SplitCell): number | undefined {
  return line.lineNumber ?? undefined;
}

function isChanged(line: SplitCell): boolean {
  return line.kind === "addition" || line.kind === "deletion";
}

function isReviewHighlighted(line: SplitCell): boolean {
  const target = changesStore.highlightedLine;
  if (!target || !isChanged(line)) return false;
  if (line.kind === "addition") return line.lineNumber === target;
  return !addedLineNumbers.value.has(target) && line.lineNumber === target;
}

function toggleLine(line: SplitCell, checked: boolean): void {
  const anchor = lineAnchor(line);
  if (!anchor) return;
  const next = new Set(selectedLines.value);
  if (checked) next.add(anchor);
  else next.delete(anchor);
  selectedLines.value = [...next].sort((left, right) => left - right);
}

function startDrag(line: SplitCell): void {
  if (!canMutate.value || !isChanged(line)) return;
  const anchor = lineAnchor(line);
  if (!anchor) return;
  dragging.value = true;
  dragAnchor.value = anchor;
  selectedLines.value = [anchor];
}

function extendDrag(line: SplitCell): void {
  if (
    !canMutate.value ||
    !dragging.value ||
    !dragAnchor.value ||
    !isChanged(line)
  )
    return;
  const current = lineAnchor(line);
  if (!current) return;
  const start = Math.min(dragAnchor.value, current);
  const end = Math.max(dragAnchor.value, current);
  selectedLines.value = Array.from(
    { length: end - start + 1 },
    (_, index) => start + index,
  );
}

function stopDrag(): void {
  dragging.value = false;
}

async function refreshCurrentDiff(current: FileDiff): Promise<void> {
  if (current.scope === "commit") return;
  const change = changesStore.snapshot?.files.find(
    (file) => file.path === current.path,
  );
  const stillAvailable =
    current.scope === "staged" ? change?.staged : change?.unstaged;
  if (stillAvailable) {
    await changesStore
      .selectFile(current.path, current.scope)
      .catch(() => undefined);
  }
}

async function applyHunk(index: number): Promise<void> {
  const current = diff.value;
  if (!current || current.scope === "commit") return;
  try {
    if (current.scope === "staged") {
      await changesStore.unstageHunk(current.path, index);
    } else {
      await changesStore.stageHunk(current.path, index);
    }
    await refreshCurrentDiff(current);
  } catch {
    // The shared changes panel renders the structured error.
  }
}

async function applySelectedLines(): Promise<void> {
  const current = diff.value;
  if (
    !current ||
    current.scope === "commit" ||
    selectedLines.value.length === 0
  )
    return;
  const start = selectedLines.value[0]!;
  const end = selectedLines.value.at(-1)!;
  try {
    if (current.scope === "staged") {
      await changesStore.unstageLines(current.path, start, end);
    } else {
      await changesStore.stageLines(current.path, start, end);
    }
    await refreshCurrentDiff(current);
  } catch {
    // The shared changes panel renders the structured error.
  }
}
</script>

<template>
  <section class="diff-viewer" :aria-label="t('uiFileDiff3a0c2c')">
    <div v-if="changesStore.operation.kind === 'diff'" class="diff-state">
      <LoaderCircle :size="20" class="spin" />
      <span>{{ t('uiLoadingDiff59e762') }}</span>
    </div>
    <div v-else-if="!diff" class="diff-state">
      <FileQuestion :size="24" />
      <strong>{{ t('uiSelectAChangedFile629837') }}</strong>
      <span>{{ t('uiTheFileDiffWillAppearHere449d1d') }}</span>
    </div>
    <div v-else-if="diff.binary" class="diff-state">
      <FileQuestion :size="24" />
      <strong>{{ t('uiBinaryFilea1a0e6') }}</strong>
      <span>{{ t('uiTextDiffIsUnavailableForThisFile4a4268') }}</span>
    </div>
    <div v-else-if="diff.hunks.length === 0" class="diff-state">
      <Check :size="24" />
      <strong>{{ t('uiNoDiffToDisplayad923f') }}</strong>
      <span>{{ t('uiFileContentMayHaveBeenRefreshed52c706') }}</span>
    </div>
    <div v-else class="diff-content" :class="{ 'single-hunk': diffHunks.length === 1 }" @pointerup="stopDrag" @pointercancel="stopDrag" @pointerleave="stopDrag">
      <div v-if="canMutate && selectedLines.length" class="line-action">
        <span>
          {{ t('uiSelected743aaf') }} {{ selectedLines[0] }}
          <template v-if="selectedLines.length > 1">
            - {{ selectedLines.at(-1) }}
          </template>
        </span>
        <button
          :aria-label="actionLabel"
          :disabled="busy"
          @click="applySelectedLines"
        >
          <Minus v-if="isStaged" :size="14" />
          <Plus v-else :size="14" />
          {{ actionLabel }}
        </button>
      </div>
      <section
        v-for="{ hunk, hunks } in diffHunks"
        :key="hunk.index"
        class="diff-hunk"
      >
        <header>
          <span><Layers :size="14" />{{ hunk.header }}</span>
          <button
            v-if="canMutate"
            class="icon-button"
            :aria-label="
              (isStaged ? (t('uiUnstageHunka73167') + ' ') : (t('uiStageHunk8c453b') + ' ')) + (hunk.index + 1)
            "
            :title="
              (isStaged ? (t('uiUnstageHunka73167') + ' ') : (t('uiStageHunk8c453b') + ' ')) + (hunk.index + 1)
            "
            :disabled="busy"
            @click="applyHunk(hunk.index)"
          >
            <Minus v-if="isStaged" :size="15" />
            <Plus v-else :size="15" />
          </button>
        </header>
        <SplitDiff
          :hunks="hunks"
          :fill-height="diffHunks.length === 1"
          :row-height="28"
          :font-size="14"
          :highlighted-line="highlightedLine"
          hide-hunk-headers
        >
          <template #cell="{ cell: line }">
          <div
            v-if="line"
            class="diff-line"
            :class="[
              line.kind,
              {
                selected:
                  lineAnchor(line) && selectedLines.includes(lineAnchor(line)!),
                'review-highlight':
                  isReviewHighlighted(line),
              },
            ]"
            :data-diff-line="lineAnchor(line)"
            @pointerdown="startDrag(line)"
            @pointerenter="extendDrag(line)"
          >
            <label v-if="canMutate && isChanged(line)" class="line-select" @pointerdown.stop>
              <input
                type="checkbox"
                :aria-label="
                  (t('uiSelectChangedLine198ea2') + ' ') + lineAnchor(line) + '：' + line.content
                "
                :checked="
                  !!lineAnchor(line) &&
                  selectedLines.includes(lineAnchor(line)!)
                "
                @change="
                  toggleLine(
                    line,
                    ($event.target as HTMLInputElement).checked,
                  )
                "
              />
            </label>
            <span v-else class="line-select" />
            <span class="line-number">{{ line.lineNumber ?? "" }}</span>
            <code><span class="marker">{{ line.kind === "addition" ? "+" : line.kind === "deletion" ? "-" : " " }}</span>{{ line.content }}</code>
          </div>
          </template>
        </SplitDiff>
      </section>
    </div>
  </section>
</template>

<style scoped>
.diff-viewer {
  min-width: 0;
  min-height: 0;
  overflow: auto;
  background: var(--surface-panel);
}

.diff-state {
  display: grid;
  min-height: 260px;
  place-content: center;
  justify-items: center;
  gap: 8px;
  color: var(--text-muted);
  text-align: center;
}

.diff-state strong {
  color: var(--text);
  font-size: 13px;
}

.diff-state span {
  font-size: 11px;
}

.diff-content {
  min-width: 0;
}
.diff-content.single-hunk { display: flex; flex-direction: column; height: 100%; min-height: 0; overflow: hidden; }
.single-hunk .line-action { flex-shrink: 0; }
.single-hunk .diff-hunk { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.single-hunk .diff-hunk > header { flex-shrink: 0; }

.line-action {
  position: sticky;
  z-index: 3;
  top: 0;
  display: flex;
  height: 40px;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
  padding: 0 12px;
  border-bottom: 1px solid var(--border);
  background: var(--surface-panel);
  color: var(--text-muted);
  font-size: 12px;
}

.line-action button {
  display: inline-flex;
  height: 28px;
  align-items: center;
  gap: 5px;
  padding: 0 9px;
  border-radius: var(--radius-md);
  background: var(--primary);
  color: white;
}

.diff-hunk + .diff-hunk {
  border-top: 8px solid var(--surface-app);
}

.diff-hunk > header {
  position: sticky;
  z-index: 2;
  top: 0;
  display: flex;
  height: 36px;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px 0 14px;
  border-bottom: 1px solid var(--border);
  background: var(--primary-soft);
  color: var(--primary);
  font-family: var(--font-code);
  font-size: 11px;
}

.diff-hunk > header span {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 7px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.icon-button {
  display: grid;
  width: 28px;
  height: 28px;
  flex: 0 0 auto;
  place-items: center;
  border-radius: var(--radius-md);
  background: transparent;
}

.icon-button:hover:not(:disabled) {
  background: var(--surface-panel);
}

.diff-line {
  display: grid;
  grid-template-columns: 30px 48px minmax(0, 1fr);
  height: var(--row-height);
  font: inherit;
  user-select: none;
}

.diff-line.addition {
  background: var(--diff-add-bg);
}

.diff-line.deletion {
  background: var(--diff-delete-bg);
}

.diff-line.selected {
  box-shadow: inset 3px 0 var(--primary);
}

.diff-line.review-highlight {
  position: relative;
  box-shadow:
    inset 3px 0 var(--primary),
    inset 0 0 0 1px var(--primary-border);
  background: var(--primary-soft);
}

.line-select {
  display: grid;
  place-items: center;
}

.line-select input {
  width: 13px;
  height: 13px;
  margin: 0;
  accent-color: var(--primary);
}

.line-number {
  padding: 0 8px;
  border-left: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
  color: var(--text-muted);
  text-align: right;
  font-size: 12px;
}

.diff-line code {
  padding: 0 12px;
  color: var(--text);
  white-space: pre;
  font: inherit;
}

.marker {
  display: inline-block;
  width: 14px;
  color: var(--text-muted);
}

.spin {
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
