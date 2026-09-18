<script setup lang="ts">
import {
  FileCode2,
  Minus,
  Plus,
  ScanSearch,
  Trash2,
  TriangleAlert,
  X,
} from "@lucide/vue";
import { computed, nextTick, ref, watch } from "vue";

import type { ChangeScope, FileChange, ChangeLineStat } from "@/lib/backend/types";
import { backendClient } from "@/lib/backend/client";
import { useTerminalStore } from "@/stores/terminal";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { useAiStore } from "@/stores/ai";
import { useSettingsStore } from "@/stores/settings";
import { useReviewSkill } from "@/features/ai/useReviewSkill";
import NoiseCleanup from "./NoiseCleanup.vue";
import ChangeGroupsSplit from "./ChangeGroupsSplit.vue";
import ChangeFileActions from "./ChangeFileActions.vue";
import { useDragFileSelection } from "./useDragFileSelection";
const fileActions = ref<InstanceType<typeof ChangeFileActions>>();

const changesStore = useChangesStore();
const repositoryStore = useRepositoryStore();
const ai = useAiStore();
const settings = useSettingsStore();
const skill = useReviewSkill();
const terminal = useTerminalStore();
const lineStats = ref<ChangeLineStat[]>([]);
const statsLoading = ref(false);
const statsError = ref(false);
const statsVersion = ref(0);
const statMap = computed(() => new Map(lineStats.value.map(stat => [`${stat.scope}:${stat.path}`, stat])));
watch([() => repositoryStore.snapshot?.rootPath, () => repositoryStore.generation, () => changesStore.snapshot, () => terminal.busy, statsVersion], async ([root], _, onCleanup) => {
  let current = true;
  onCleanup(() => { current = false; });
  lineStats.value = [];
  statsError.value = false;
  statsLoading.value = false;
  if (!root || terminal.busy) return;
  statsLoading.value = true;
  try {
    const result = await backendClient.changesLineStats(root);
    if (current) lineStats.value = result;
  } catch { if (current) statsError.value = true; }
  finally { if (current) statsLoading.value = false; }
}, { immediate: true });

function extension(path: string): string {
  const name = path.split('/').pop() ?? '';
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot) : '—';
}
function count(path: string, scope: ChangeScope, kind: 'additions' | 'deletions'): string {
  const stat = statMap.value.get(`${scope}:${path}`);
  if (statsLoading.value) return '…';
  if (stat?.binary) return 'Binary';
  return stat?.[kind]?.toLocaleString() ?? '—';
}
function tableStatus(file: FileChange, scope: ChangeScope): string {
  if (file.conflict) return 'Conflict';
  const status = scope === 'staged' ? file.indexStatus : file.worktreeStatus;
  return ({ A: 'Added', '?': 'Untracked', D: 'Deleted', R: 'Renamed', C: 'Copied', T: 'Type changed' } as Record<string, string>)[status] ?? 'Modified';
}
const reviewSelection = ref(new Set<string>());
const unstagedSelection = ref(new Set<string>());
const pendingDiscard = ref<string>();
const cancelDiscardButton = ref<HTMLButtonElement>();
const stagedFiles = computed(
  () => changesStore.snapshot?.files.filter((file) => file.staged) ?? [],
);
const unstagedFiles = computed(
  () => changesStore.snapshot?.files.filter((file) => file.unstaged)
    .sort((a, b) => Number(tableStatus(a, 'unstaged') === 'Untracked') - Number(tableStatus(b, 'unstaged') === 'Untracked')) ?? [],
);
const busy = computed(() => repositoryStore.navigationBusy);
const { dragging, start: startDragSelection, captureClick: captureSelectionClick } = useDragFileSelection({
  blocked: () => busy.value || !!pendingDiscard.value,
  identity: () => JSON.stringify([repositoryStore.snapshot?.rootPath, repositoryStore.generation, repositoryStore.snapshot?.currentBranch]),
  paths: scope => (scope === "staged" ? stagedFiles.value : unstagedFiles.value).map(file => file.path),
  selections: { staged: reviewSelection, unstaged: unstagedSelection },
});
const reviewBusy = computed(() => busy.value || ai.running);
const selectedReviewPaths = computed(() => stagedFiles.value
  .filter(file => reviewSelection.value.has(file.path)).map(file => file.path));
const allReviewSelected = computed(() => stagedFiles.value.length > 0 && selectedReviewPaths.value.length === stagedFiles.value.length);
const selectedUnstagedPaths = computed(() => unstagedFiles.value.filter(file => unstagedSelection.value.has(file.path)).map(file => file.path));
const allUnstagedSelected = computed(() => unstagedFiles.value.length > 0 && selectedUnstagedPaths.value.length === unstagedFiles.value.length);
const reviewReason = computed(() => {
  if (reviewBusy.value) return "请等待当前操作结束。";
  if (!selectedReviewPaths.value.length) return "勾选文件后，仅审查这些文件的暂存变更。";
  if (!settings.settings.apiKey.trim() || !settings.settings.baseUrl.trim() || !settings.settings.model.trim()) return "请先在设置中完成 AI 服务配置。";
  if (!skill.ready.value) return ai.skillLoading ? "正在读取仓库审查 SKILL…" : "仓库审查 SKILL 未就绪，请在 AI 助手中检查。";
  return "";
});

watch(stagedFiles, files => {
  const available = new Set(files.map(file => file.path));
  reviewSelection.value = new Set([...reviewSelection.value].filter(path => available.has(path)));
});
watch(unstagedFiles, files => {
  const available = new Set(files.map(file => file.path));
  unstagedSelection.value = new Set([...unstagedSelection.value].filter(path => available.has(path)));
});
watch([() => repositoryStore.snapshot?.rootPath, () => repositoryStore.generation], () => {
  reviewSelection.value = new Set();
  unstagedSelection.value = new Set();
}, { flush: "sync" });

function toggleReviewFile(path: string, event: Event): void {
  if (busy.value) return;
  if ((event.target as HTMLInputElement).checked) reviewSelection.value.add(path);
  else reviewSelection.value.delete(path);
}

function toggleAllReview(event: Event): void {
  if (busy.value) return;
  reviewSelection.value = new Set((event.target as HTMLInputElement).checked ? stagedFiles.value.map(file => file.path) : []);
}

function toggleUnstagedFile(path: string, event: Event): void {
  if (busy.value) return;
  if ((event.target as HTMLInputElement).checked) unstagedSelection.value.add(path);
  else unstagedSelection.value.delete(path);
}
function toggleAllUnstaged(event: Event): void {
  if (busy.value) return;
  unstagedSelection.value = new Set((event.target as HTMLInputElement).checked ? unstagedFiles.value.map(file => file.path) : []);
}
async function stageSelected(): Promise<void> {
  if (busy.value || !selectedUnstagedPaths.value.length) return;
  try { await changesStore.stageFiles(selectedUnstagedPaths.value); unstagedSelection.value.clear(); }
  catch { /* Keep choices for retry; the store exposes the error. */ }
}
async function unstageSelected(): Promise<void> {
  if (busy.value || !selectedReviewPaths.value.length) return;
  try { await changesStore.unstageFiles(selectedReviewPaths.value); reviewSelection.value.clear(); }
  catch { /* Keep choices for retry; the store exposes the error. */ }
}

function reviewSelected(): void {
  if (reviewReason.value) return;
  settings.settings.aiDrawerOpen = true;
  void ai.startReview({ kind: "stagedFiles", paths: selectedReviewPaths.value }).catch(() => undefined);
}

function selectFile(path: string, scope: ChangeScope): void {
  void changesStore.selectFile(path, scope).catch(() => undefined);
}

function stage(path: string): void {
  void changesStore.stageFile(path).catch(() => undefined);
}

function unstage(path: string): void {
  void changesStore.unstageFile(path).catch(() => undefined);
}

async function requestDiscard(path: string): Promise<void> {
  pendingDiscard.value = path;
  await nextTick();
  cancelDiscardButton.value?.focus();
}

async function confirmDiscard(): Promise<void> {
  const path = pendingDiscard.value;
  if (!path) return;
  try {
    await changesStore.discardFile(path);
    pendingDiscard.value = undefined;
  } catch {
    // The store exposes the structured error in the panel.
  }
}
</script>

<template>
  <div class="changes-list table-mode" :class="{ 'drag-selecting': dragging }" @click.capture="captureSelectionClick" @dragstart.prevent>
    <ChangeFileActions ref="fileActions" />
    <div class="changes-toolbar">
    <div class="stats-caption">{{ statsLoading ? '统计中…' : '增删行数按暂存范围统计' }}</div>
    <div v-if="statsError" class="stats-error" role="status">行数统计失败 <button @click="statsVersion++">重试</button></div>
    <NoiseCleanup />
    <div v-if="changesStore.error" class="error-banner" role="alert">
      <TriangleAlert :size="15" />
      <span>{{ changesStore.error.message }}</span>
      <button
        class="icon-button"
        aria-label="关闭错误提示"
        title="关闭错误提示"
        @click="changesStore.error = undefined"
      >
        <X :size="14" />
      </button>
    </div>

    </div>
    <ChangeGroupsSplit :has-staged="stagedFiles.length > 0">
    <template #staged>
    <section class="change-group" aria-labelledby="staged-heading">
      <header id="staged-heading">
        <span>已暂存</span>
        <strong>{{ stagedFiles.length }}</strong>
      </header>
      <div v-if="stagedFiles.length === 0" class="group-empty">
        没有已暂存文件
      </div>
      <div v-else class="review-selection-bar">
        <div class="review-selection-actions">
          <label class="review-select-all"><input type="checkbox" aria-label="全选已暂存文件" :checked="allReviewSelected" :indeterminate="selectedReviewPaths.length > 0 && !allReviewSelected" :disabled="busy" @change="toggleAllReview" />全选<span>已选 {{ selectedReviewPaths.length }}</span></label>
          <button class="review-selected-button" aria-label="批量取消暂存" :disabled="busy || !selectedReviewPaths.length" @click="unstageSelected"><Minus :size="14" />取消暂存所选</button>
          <button class="review-selected-button" aria-label="AI 审查所选暂存文件" :title="reviewReason || '使用仓库 SKILL 审查所选文件的暂存变更'" :disabled="!!reviewReason" @click="reviewSelected"><ScanSearch :size="14" />AI 审查所选</button>
        </div>
        <small>{{ reviewReason || '仅审查所选暂存变更，使用仓库 SKILL。' }}</small>
      </div>
      <div v-if="stagedFiles.length" class="table-heading" aria-label="已暂存文件表头"><span /><span>Path</span><span>Extension</span><span>Status</span><span>Lines added</span><span>Lines removed</span><span>操作</span></div>
      <div
        v-for="file in stagedFiles"
        :key="'staged:' + file.path"
        class="change-row staged-review-row"
        @pointerdown="startDragSelection($event, file.path, 'staged')"
        @contextmenu="fileActions?.open($event, file, 'staged')"
        @keydown="fileActions?.open($event, file, 'staged')"
        :data-status="tableStatus(file, 'staged')"
        :class="{
          'batch-selected': reviewSelection.has(file.path),
          selected:
            changesStore.selectedPath === file.path &&
            changesStore.selectedScope === 'staged',
        }"
      >
        <input class="review-file-checkbox" type="checkbox" :aria-label="'选择已暂存文件 ' + file.path" :title="'选择已暂存文件 ' + file.path" :checked="reviewSelection.has(file.path)" :disabled="busy" @change="toggleReviewFile(file.path, $event)" />
        <button
          class="file-select"
          :data-change-key="'staged:' + file.path"
          @click="selectFile(file.path, 'staged')"
        >
          <span class="status-swatch staged" aria-hidden="true" />
          <FileCode2 :size="15" class="file-icon" />
          <span class="file-copy">
            <strong :title="file.path">{{ file.path }}</strong>
          </span>
        </button>
          <span class="table-cell extension" :title="extension(file.path)">{{ extension(file.path) }}</span>
          <span class="table-cell file-status" :title="tableStatus(file, 'staged')">{{ tableStatus(file, 'staged') }}</span>
          <span class="table-cell additions">{{ count(file.path, 'staged', 'additions') }}</span>
          <span class="table-cell deletions">{{ count(file.path, 'staged', 'deletions') }}</span>
        <button
          class="icon-button"
          :aria-label="'取消暂存 ' + file.path"
          :title="'取消暂存 ' + file.path"
          :disabled="busy"
          @click.stop="unstage(file.path)"
        >
          <Minus :size="15" />
        </button>
      </div>
    </section>

    </template>
    <template #unstaged>
    <section class="change-group" aria-labelledby="unstaged-heading">
      <header id="unstaged-heading">
        <span>未暂存</span>
        <strong>{{ unstagedFiles.length }}</strong>
      </header>
      <div v-if="unstagedFiles.length === 0" class="group-empty">
        没有未暂存文件
      </div>
      <div v-if="unstagedFiles.length" class="review-selection-bar">
        <div class="review-selection-actions">
          <label class="review-select-all"><input type="checkbox" aria-label="全选未暂存文件" :checked="allUnstagedSelected" :indeterminate="selectedUnstagedPaths.length > 0 && !allUnstagedSelected" :disabled="busy" @change="toggleAllUnstaged" />全选<span>已选 {{ selectedUnstagedPaths.length }}</span></label>
          <button class="review-selected-button" aria-label="批量暂存" :disabled="busy || !selectedUnstagedPaths.length" @click="stageSelected"><Plus :size="14" />暂存所选</button>
        </div>
      </div>
      <div v-if="unstagedFiles.length" class="table-heading" aria-label="未暂存文件表头"><span /><span>Path</span><span>Extension</span><span>Status</span><span>Lines added</span><span>Lines removed</span><span>操作</span></div>
      <div
        v-for="file in unstagedFiles"
        :key="'unstaged:' + file.path"
        class="change-row unstaged-row"
        @pointerdown="startDragSelection($event, file.path, 'unstaged')"
        @contextmenu="fileActions?.open($event, file, 'unstaged')"
        @keydown="fileActions?.open($event, file, 'unstaged')"
        :data-status="tableStatus(file, 'unstaged')"
        :class="{
          'batch-selected': unstagedSelection.has(file.path),
          selected:
            changesStore.selectedPath === file.path &&
            changesStore.selectedScope === 'unstaged',
        }"
      >
        <input class="review-file-checkbox" type="checkbox" :aria-label="'选择未暂存文件 ' + file.path" :checked="unstagedSelection.has(file.path)" :disabled="busy" @change="toggleUnstagedFile(file.path, $event)" />
        <button
          class="file-select"
          :data-change-key="'unstaged:' + file.path"
          @click="selectFile(file.path, 'unstaged')"
        >
          <span
            class="status-swatch"
            :class="{ conflict: file.conflict }"
            aria-hidden="true"
          />
          <FileCode2 :size="15" class="file-icon" />
          <span class="file-copy">
            <strong :title="file.path">{{ file.path }}</strong>
          </span>
        </button>
          <span class="table-cell extension" :title="extension(file.path)">{{ extension(file.path) }}</span>
          <span class="table-cell file-status" :title="tableStatus(file, 'unstaged')">{{ tableStatus(file, 'unstaged') }}</span>
          <span class="table-cell additions">{{ count(file.path, 'unstaged', 'additions') }}</span>
          <span class="table-cell deletions">{{ count(file.path, 'unstaged', 'deletions') }}</span>
        <span class="row-actions">
          <button
            class="icon-button"
            :aria-label="'丢弃 ' + file.path"
            :title="'丢弃 ' + file.path"
            :disabled="busy"
            @click.stop="requestDiscard(file.path)"
          >
            <Trash2 :size="14" />
          </button>
          <button
            class="icon-button primary-action"
            :aria-label="'暂存 ' + file.path"
            :title="'暂存 ' + file.path"
            :disabled="busy"
            @click.stop="stage(file.path)"
          >
            <Plus :size="15" />
          </button>
        </span>
      </div>
    </section>

    </template>
    </ChangeGroupsSplit>
    <div
      v-if="pendingDiscard"
      class="modal-backdrop"
      role="presentation"
      @click.self="pendingDiscard = undefined"
    >
      <section
        class="discard-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="discard-title"
        aria-describedby="discard-description"
      >
        <div class="dialog-icon"><TriangleAlert :size="20" /></div>
        <div>
          <h2 id="discard-title">丢弃文件变更？</h2>
          <p id="discard-description">
            将从仓库 <strong>{{ repositoryStore.snapshot?.name }}</strong>
            丢弃以下路径的未暂存内容：
          </p>
          <code>{{ pendingDiscard }}</code>
        </div>
        <footer>
          <button
            ref="cancelDiscardButton"
            class="secondary-button"
            aria-label="取消丢弃"
            @click="pendingDiscard = undefined"
          >
            取消
          </button>
          <button
            class="danger-button"
            :aria-label="'确认丢弃 ' + pendingDiscard"
            :disabled="busy"
            @click="confirmDiscard"
          >
            <Trash2 :size="15" />
            丢弃变更
          </button>
        </footer>
      </section>
    </div>
  </div>
</template>

<style scoped>
.change-row { user-select: none; }
.drag-selecting, .drag-selecting .change-row, .drag-selecting .file-select { cursor: crosshair; }
.stats-caption { padding: 10px 12px 0; color: var(--text-muted); font-size: 10px; }
.stats-error { padding: 8px 12px; color: var(--danger); font-size: 11px; }
.stats-error button { background: transparent; color: var(--primary); }
.table-mode .change-row, .table-heading { display: grid; grid-template-columns: 18px minmax(210px, 1fr) 66px 86px 80px 92px 60px; min-width: 646px; gap: 4px; }
.table-heading { align-items: center; min-height: 30px; padding: 3px 5px 3px 4px; color: var(--text-muted); font-size: 10px; background: var(--surface-muted); border-bottom: 1px solid var(--border); }
.table-mode .change-row { min-height: 32px; border-bottom: 1px solid var(--border); border-radius: 0; }
.table-mode .file-copy small { display: none; }
.table-mode .file-select { grid-template-columns: 4px 15px minmax(0, 1fr); gap: 5px; }
.table-cell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
.table-cell.additions { color: var(--success); }
.table-cell.deletions { color: var(--danger); }
.change-row { --file-status-color: color-mix(in srgb, var(--primary) 80%, var(--text)); }
.change-row[data-status="Added"],
.change-row[data-status="Copied"] { --file-status-color: color-mix(in srgb, var(--success) 85%, var(--text)); }
.change-row[data-status="Deleted"],
.change-row[data-status="Conflict"] { --file-status-color: color-mix(in srgb, var(--danger) 85%, var(--text)); }
.change-row[data-status="Untracked"] { --file-status-color: var(--text-muted); }
.change-row .file-select,
.change-row .file-icon,
.change-row .extension,
.change-row .file-status { color: var(--file-status-color); }
.additions, .deletions, .table-heading span:nth-child(5), .table-heading span:nth-child(6) { text-align: right; font-variant-numeric: tabular-nums; padding-right: 8px; }
.changes-list {
  display: flex;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
  overflow: hidden;
}
.changes-toolbar { flex: 0 0 auto; }

.error-banner {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  gap: 8px;
  align-items: center;
  margin: 10px;
  padding: 9px 8px 9px 10px;
  border: 1px solid color-mix(in srgb, var(--danger) 35%, var(--border));
  border-radius: var(--radius-md);
  background: var(--danger-soft);
  color: var(--danger);
  font-size: 12px;
}

.change-group {
  padding: 8px 8px 4px;
}

.change-group > header {
  display: flex;
  height: 30px;
  align-items: center;
  justify-content: space-between;
  padding: 0 6px;
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 600;
}

.change-group > header strong {
  min-width: 20px;
  color: var(--text-muted);
  text-align: center;
}

.group-empty {
  padding: 12px 8px 16px;
  color: var(--text-muted);
  font-size: 11px;
}

.change-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 2px;
  min-height: 42px;
  align-items: center;
  padding: 3px 5px 3px 4px;
  border-radius: var(--radius-md);
  cursor: default;
}

.staged-review-row { grid-template-columns: 18px minmax(0, 1fr) auto; }
.review-file-checkbox, .review-select-all input { width: 14px; height: 14px; margin: 0; accent-color: var(--primary); }
.review-selection-bar { display: grid; gap: 7px; padding: 6px 5px 10px; }
.review-selection-actions { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 7px; }
.review-select-all { display: inline-flex; align-items: center; gap: 6px; font-size: 11px; cursor: pointer; }
.review-select-all span { color: var(--text-muted); }
.review-selected-button { display: inline-flex; align-items: center; gap: 5px; min-height: 28px; padding: 4px 7px; border: 1px solid var(--primary-border); border-radius: var(--radius-sm); background: var(--primary-soft); color: var(--primary); font-size: 11px; }
.review-selected-button:disabled { opacity: 0.55; cursor: not-allowed; }
.review-selection-bar small { color: var(--text-muted); font-size: 10px; line-height: 1.5; }

.file-select {
  display: grid;
  grid-template-columns: 4px 17px minmax(0, 1fr);
  gap: 7px;
  min-width: 0;
  align-items: center;
  align-self: stretch;
  padding: 0;
  background: transparent;
  text-align: left;
}

.change-row:hover,
.change-row.selected {
  background: var(--surface-muted);
}

.change-row.selected {
  box-shadow: inset 0 0 0 1px var(--primary-border);
}
.change-row.batch-selected { background: var(--primary-soft); box-shadow: inset 3px 0 var(--primary); }

.status-swatch {
  width: 3px;
  height: 22px;
  border-radius: 2px;
  background: var(--warning);
}

.status-swatch.staged {
  background: var(--success);
}

.status-swatch.conflict {
  background: var(--danger);
}

.file-icon {
  color: var(--text-muted);
}

.file-copy {
  display: grid;
  min-width: 0;
  gap: 2px;
}

.file-copy strong {
  overflow: hidden;
  font-size: 12px;
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-copy small {
  color: var(--text-muted);
  font-size: 10px;
}

.row-actions {
  display: flex;
  gap: 2px;
}

.icon-button {
  display: grid;
  width: 28px;
  height: 28px;
  place-items: center;
  border-radius: var(--radius-md);
  background: transparent;
}

.icon-button:hover:not(:disabled) {
  background: var(--surface-panel);
}

.primary-action {
  color: var(--primary);
}

.modal-backdrop {
  position: fixed;
  z-index: 20;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 24px;
  background: var(--overlay);
}

.discard-dialog {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 12px;
  width: min(420px, calc(100vw - 48px));
  padding: 20px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface-panel);
  box-shadow: var(--shadow-window);
}

.dialog-icon {
  display: grid;
  width: 36px;
  height: 36px;
  place-items: center;
  border-radius: var(--radius-md);
  background: var(--danger-soft);
  color: var(--danger);
}

.discard-dialog h2 {
  margin: 1px 0 8px;
  font-size: 15px;
}

.discard-dialog p {
  margin: 0 0 10px;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.6;
}

.discard-dialog code {
  display: block;
  overflow-wrap: anywhere;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--surface-muted);
  font-size: 11px;
}

.discard-dialog footer {
  grid-column: 1 / -1;
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 8px;
}

.secondary-button,
.danger-button {
  display: inline-flex;
  height: 32px;
  align-items: center;
  gap: 6px;
  padding: 0 12px;
  border-radius: var(--radius-md);
}

.secondary-button {
  border: 1px solid var(--border);
  background: var(--surface-panel);
}

.danger-button {
  background: var(--danger);
  color: white;
}
</style>
