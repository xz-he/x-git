<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { ExternalLink, FolderOpen, Copy, Columns2, Plus, Minus, Package, Trash2, Undo2, EyeOff, Unlink, GitCommitHorizontal, GitMerge, Terminal, History, ListOrdered } from "@lucide/vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import AppSelect from "@/components/common/AppSelect.vue";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import { formatDisplayPath } from "@/lib/formatPath";
import type { ChangeScope, FileChange, IgnoreRequest, IgnorePreview, MutationWorkspace, PreparedFileOperation } from "@/lib/backend/types";
import { useRepositoryStore } from "@/stores/repository";
import { useChangesStore } from "@/stores/changes";
import { useFilesStore } from "@/stores/files";
import { useOperationStore } from "@/stores/operation";
import { useConflictsStore } from "@/stores/conflicts";
import { useTerminalStore } from "@/stores/terminal";
import { useUiStore } from "@/stores/ui";

const repo = useRepositoryStore(), changes = useChangesStore(), files = useFilesStore();
const operation = useOperationStore(), conflicts = useConflictsStore(), terminal = useTerminalStore(), ui = useUiStore();
interface Context { root: string; generation: number; branch: string | null; file: FileChange; scope: ChangeScope }
const context = ref<Context>();
const menu = ref<HTMLElement>();
const shown = ref(false), loading = ref(false), error = ref(""), notice = ref("");
const position = ref({ x: 0, y: 0 });
const dialog = ref<"ignore" | "delete" | "restore" | "untrack" | "lfs" | "history" | "blame">();
const prepared = ref<PreparedFileOperation>();
const inspection = ref("");
let opener: HTMLElement | undefined;
const busy = computed(() => loading.value || repo.navigationBusy);
const file = computed(() => context.value?.file);
const untracked = computed(() => file.value?.indexStatus === "?" || file.value?.worktreeStatus === "?");
const exists = computed(() => file.value?.worktreeStatus !== "D" && !(file.value?.indexStatus === "D" && !file.value?.unstaged));
const extension = computed(() => { const name = file.value?.path.split("/").pop() ?? ""; const dot = name.lastIndexOf("."); return dot > 0 && dot < name.length - 1 ? name.slice(dot) : ""; });
const directories = computed(() => { const parts = file.value?.path.split("/").slice(0, -1) ?? []; return parts.map((_, i) => parts.slice(0, i + 1).join("/")); });
const canWrite = computed(() => !busy.value && !operation.isBlocked && !conflicts.hasDirtyDrafts);
const items = computed(() => [
  { id: "open", label: "Open", get hint() { return t('uiOpenFile38820b'); }, icon: ExternalLink, enabled: exists.value },
  { id: "reveal", label: "Show in Explorer", get hint() { return t('uiShowInFileExplorere7d4d0'); }, icon: FolderOpen, enabled: exists.value },
  { id: "copy", label: "Copy Path", get hint() { return t('uiCopyFullPath090e94'); }, icon: Copy, enabled: true },
  { id: "diff", label: "External Diff", get hint() { return t('uiUseTheConfiguredGitDifftool92806c'); }, icon: Columns2, enabled: !untracked.value && !file.value?.conflict, separator: true },
  { id: "stage", label: context.value?.scope === "staged" ? "Unstage" : "Stage", get hint() { return t('uiAppliesToThisFileOnly28d78c'); }, icon: context.value?.scope === "staged" ? Minus : Plus, enabled: canWrite.value },
  { id: "lfs", label: "Track Type with Git LFS…", get hint() { return t('uiTrackThisExtensionRequiresGitLFS2bdee3'); }, icon: Package, enabled: canWrite.value && exists.value && !!extension.value },
  { id: "delete", label: "Remove…", get hint() { return t('uiRemoveFileFromDiskRecoverablec1b06d'); }, icon: Trash2, enabled: canWrite.value && exists.value },
  { id: "restore", label: "Restore Changes…", get hint() { return t('uiRestoreFromTheIndexDiscardingUnstagedChangesInThisFilec1f1e6'); }, icon: Undo2, enabled: canWrite.value && !untracked.value && !!file.value?.unstaged && !file.value?.conflict && context.value?.scope === "unstaged" },
  { id: "ignore", label: "Ignore…", get hint() { return t('uiConfigureIgnoreRulesde4047'); }, icon: EyeOff, enabled: canWrite.value && exists.value },
  { id: "untrack", label: "Stop Tracking…", get hint() { return t('uiRemoveFromTheGitIndexKeepingTheFileOnDisk049c52'); }, icon: Unlink, enabled: canWrite.value && exists.value && !untracked.value },
  { id: "commit", label: "Commit…", get hint() { return t('uiStageThisFileAndEnterACommitMessageDoesNotCommitAutomaticall5770bf'); }, icon: GitCommitHorizontal, enabled: canWrite.value },
  { id: "conflict", label: "Resolve Conflicts", get hint() { return t('uiResolveThisFileInTheConflictWorkbencheff40d'); }, icon: GitMerge, enabled: !!file.value?.conflict && !conflicts.hasDirtyDrafts, separator: true },
  { id: "custom", label: "Custom Action…", get hint() { return t('uiEditAFileCommandInTheEmbeddedGitTerminalcbf4a6'); }, icon: Terminal, enabled: true },
  { id: "history", label: "File History…", get hint() { return t('uiViewTheLatest100CommitsForThisFile225f53'); }, icon: History, enabled: !untracked.value && file.value?.indexStatus !== "A", separator: true },
  { id: "blame", label: "Blame…", get hint() { return t('uiLineHistoryIncludingWorkingTreeContent18b017'); }, icon: ListOrdered, enabled: exists.value && !untracked.value && file.value?.indexStatus !== "A" },
]);
function accepts(id: Context): boolean { return repo.snapshot?.rootPath === id.root && repo.generation === id.generation && repo.snapshot.currentBranch === id.branch; }
function close(): void { if (loading.value) return; shown.value = false; dialog.value = undefined; context.value = undefined; error.value = ""; opener?.focus(); }
async function open(event: MouseEvent | KeyboardEvent, selected: FileChange, scope: ChangeScope): Promise<void> {
  if (busy.value || !repo.snapshot) return;
  if (event instanceof KeyboardEvent && event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
  event.preventDefault(); event.stopPropagation();
  opener = event.target instanceof HTMLElement ? event.target.closest<HTMLElement>("button") ?? event.currentTarget as HTMLElement : undefined;
  const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
  position.value = event instanceof MouseEvent && (event.clientX || event.clientY) ? { x: event.clientX, y: event.clientY } : { x: bounds.left, y: bounds.bottom };
  context.value = { root: repo.snapshot.rootPath, generation: repo.generation, branch: repo.snapshot.currentBranch, file: { ...selected }, scope };
  dialog.value = undefined; error.value = ""; notice.value = ""; shown.value = true;
  await nextTick();
  const rect = menu.value?.getBoundingClientRect();
  position.value = { x: Math.max(8, Math.min(position.value.x, window.innerWidth - (rect?.width ?? 280) - 8)), y: Math.max(8, Math.min(position.value.y, window.innerHeight - (rect?.height ?? 540) - 8)) };
  menu.value?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
}
defineExpose({ open });
function menuKey(event: KeyboardEvent): void {
  if (event.key === "Escape" || event.key === "Tab") { close(); return; }
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  const buttons = [...(menu.value?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
  const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
  const index = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (current + (event.key === "ArrowUp" ? -1 : 1) + buttons.length) % buttons.length;
  buttons[index]?.focus();
}
watch([() => repo.snapshot?.rootPath, () => repo.generation, () => repo.snapshot?.currentBranch, () => ui.activeView], () => { shown.value = false; dialog.value = undefined; context.value = undefined; error.value = ""; });
const quote = (path: string) => "'" + path.replaceAll("'", "'\"'\"'") + "'";
async function run(task: (id: Context) => Promise<void>, mutation = false): Promise<void> {
  const id = context.value;
  if (!id || !accepts(id) || busy.value) return;
  loading.value = true; error.value = "";
  if (mutation) files.submitting = true;
  try { await task(id); }
  catch (cause) { if (accepts(id)) error.value = cause instanceof Error ? cause.message : normalizeBackendError(cause).message; }
  finally { if (mutation) files.submitting = false; loading.value = false; }
}
function apply(id: Context, result: MutationWorkspace): void { if (accepts(id)) operation.applyMutationResult(result); }
async function act(action: string): Promise<void> {
  if (!items.value.find(item => item.id === action)?.enabled || busy.value) return;
  shown.value = false;
  if (["restore", "untrack", "lfs"].includes(action)) { dialog.value = action as "restore" | "untrack" | "lfs"; return; }
  if (action === "ignore") { ruleKind.value = "exact"; scope.value = "local"; directory.value = directories.value.at(-1) ?? ""; custom.value = ""; dialog.value = "ignore"; return; }
  await run(async id => {
    const path = id.file.path;
    if (action === "open" || action === "reveal") await backendClient.filesOpen(id.root, path, action === "reveal");
    else if (action === "copy") { await navigator.clipboard.writeText(formatDisplayPath(id.root).replace(/[\\/]$/, "") + "/" + path); notice.value = t('uiFullFilePathCopiede19047'); }
    else if (action === "stage" || action === "commit") {
      if (action === "commit" && id.scope === "staged") { /* Already staged. */ }
      else if (id.scope === "staged") await changes.unstageFile(path);
      else await changes.stageFile(path);
      if (action === "commit" && accepts(id)) { notice.value = t('uiEnterACommitMessageTheCommitWillIncludeAllStagedFilescbcbe6'); await nextTick(); document.querySelector<HTMLTextAreaElement>('[data-testid="commit-message"]')?.focus(); }
    } else if (action === "delete") {
      const result = await backendClient.filesPrepare(id.root, { kind: "delete", relativePath: path });
      if (accepts(id)) { prepared.value = result; dialog.value = "delete"; }
    } else if (action === "history" || action === "blame") {
      dialog.value = action; inspection.value = "";
      const text = await backendClient.filesInspect(id.root, path, action);
      if (accepts(id)) inspection.value = text || t('uiNoRecordsToDisplay63ad82');
    } else if (action === "conflict") {
      await conflicts.ensureLoaded(id.root, id.generation);
      if (accepts(id)) { ui.openView("conflicts"); await conflicts.selectFile(path); }
    } else if (action === "diff" || action === "custom") {
      terminal.draft = `git --literal-pathspecs ${action === "diff" ? "difftool --no-prompt" : "diff"}${id.scope === "staged" ? " --cached" : ""} -- ${quote(path)}`;
      ui.openView("terminal"); await nextTick();
      if (action === "diff") await terminal.run();
      else terminal.focus();
    }
  });
}
const ruleKind = ref<IgnoreRequest["rule"]["kind"]>("exact");
const scope = ref<IgnoreRequest["scope"]>("local");
const directory = ref(""), custom = ref("");
const preview = ref<IgnorePreview>(), previewError = ref(""), previewLoading = ref(false);
const request = computed<IgnoreRequest>(() => ({ relativePath: file.value?.path ?? "", scope: scope.value, rule: ruleKind.value === "directory" ? { kind: "directory", directory: directory.value } : ruleKind.value === "custom" ? { kind: "custom", pattern: custom.value } : { kind: ruleKind.value } }));
watch([request, dialog], ([value, current], _, onCleanup) => {
  let active = true;
  preview.value = undefined; previewError.value = ""; previewLoading.value = current === "ignore";
  if (current !== "ignore" || !context.value) return;
  const id = context.value;
  const timer = window.setTimeout(async () => {
    try { const result = await backendClient.filesIgnorePreview(id.root, value); if (active && accepts(id)) preview.value = result; }
    catch (cause) { if (active) previewError.value = normalizeBackendError(cause).message; }
    finally { if (active) previewLoading.value = false; }
  }, 180);
  onCleanup(() => { active = false; window.clearTimeout(timer); });
});
const confirmationTitle = computed(() => ({ delete: "Remove File", restore: "Restore Changes", untrack: "Stop Tracking", lfs: "Track Type with Git LFS" })[dialog.value as "delete" | "restore" | "untrack" | "lfs"] ?? "");
const confirmationText = computed(() => dialog.value === "delete" ? t('uiMovesTheFileToTheRepositoryRecoveryDirectoryDoesNotCommitThee0412d') : dialog.value === "restore" ? t('uiDiscardsThisFileSUnstagedChangesAndRestoresTheIndexedVersiona3d5c7') : dialog.value === "untrack" ? t('uiStopsTrackingTheFileAndStagesItsRemovalKeepingItOnDiskAddAnI7ae57b') : t('msgAddGitLFSRulesForFilesToGitattributesExistingHisto867824', { p0: extension.value }));
async function confirm(): Promise<void> {
  const action = dialog.value;
  if (!action || !canWrite.value || (action === "ignore" && (!preview.value || previewLoading.value || previewError.value))) return;
  const ignoreRequest = request.value;
  await run(async id => {
    if (action === "restore") await changes.discardFile(id.file.path);
    else if (action === "delete" && prepared.value) {
      const result = await backendClient.filesExecute(id.root, { intent: prepared.value.intent, token: prepared.value.token });
      if (!accepts(id)) return;
      if (result.workspace) changes.applyWorkspace(result.workspace);
      if (result.operationState) operation.state = result.operationState;
      if (result.applied) { dialog.value = undefined; notice.value = result.recoveryPath ? t('msgFileRemovedRecoverItFrom4d8012', { p0: formatDisplayPath(result.recoveryPath) }) : t('uiFileRemoved163f41'); }
      if (result.error) throw result.error;
      if (!result.applied) throw new Error(t('uiFileWasNotRemovedReopenTheMenuAndTryAgain672ba5'));
    } else if (action === "ignore") apply(id, await backendClient.filesIgnore(id.root, ignoreRequest));
    else if (action === "untrack") apply(id, await backendClient.filesUntrack(id.root, id.file.path));
    else if (action === "lfs") apply(id, await backendClient.filesLfsTrack(id.root, id.file.path));
    if (accepts(id)) { dialog.value = undefined; if (action !== "delete") notice.value = t('uiOperationSucceededa8be77'); }
  }, action !== "restore");
}
onBeforeUnmount(() => { shown.value = false; });
</script>

<template>
  <div v-if="notice && !dialog" class="file-action-notice" role="status">{{ notice }} <button :aria-label="t('uiDismissFileOperationResultbc6190')" @click="notice = ''">×</button></div>
  <div v-if="error && !dialog" class="file-action-error" role="alert">{{ error }} <button :aria-label="t('uiDismissFileOperationError362c97')" @click="error = ''">×</button></div>
  <Teleport to="body">
    <div v-if="shown" class="file-menu-shield" @pointerdown.self="close" @contextmenu.prevent.self="close">
      <div ref="menu" class="file-context-menu" role="menu" :aria-label="t('uiFileActionsfd1cf9')" :style="{ left: position.x + 'px', top: position.y + 'px' }" @keydown="menuKey">
        <div class="menu-path" :title="file?.path">{{ file?.path }}</div>
        <button v-for="item in items" :key="item.id" role="menuitem" :data-action="item.id" :class="{ separator: item.separator }" :title="item.hint" :disabled="busy || !item.enabled" @click="act(item.id)"><component :is="item.icon" :size="15" /><span>{{ item.label }}</span></button>
      </div>
    </div>
    <div v-if="dialog" class="file-action-dialog" @keydown.esc.stop.prevent="close">
      <ConfirmDialog v-if="dialog === 'ignore'" :title="t('uiIgnoredd01a7')" :description="file?.path" :confirm-label="t('uiSaveIgnoreRuleb19638')" :busy="loading" :confirm-disabled="!canWrite || !preview || previewLoading || !!previewError" @cancel="close" @confirm="confirm">
        <fieldset :disabled="loading" class="ignore-options">
          <legend>{{ t('uiIgnoreFileNameOrPatternaf245c') }}</legend>
          <label><input v-model="ruleKind" type="radio" value="exact" />{{ t('uiIgnoreThisExactFileNamef62511') }}</label>
          <label><input v-model="ruleKind" type="radio" value="extension" :disabled="!extension" />{{ t('uiIgnoreAll9fc2a5') }} {{ extension || t('uiNoExtensiond2a954') }} {{ t('uifiles49deaf') }}</label>
          <label><input v-model="ruleKind" type="radio" value="directory" :disabled="!directories.length" />{{ t('uiIgnoreAllFilesInADirectoryfe0b03') }}</label>
          <AppSelect v-model="directory" :aria-label="t('uiIgnoreDirectory742f1c')" :disabled="loading || ruleKind !== 'directory'" :options="directories.map(dir => ({ value: dir, label: dir + '/' }))" />
          <label><input v-model="ruleKind" type="radio" value="custom" />{{ t('uiIgnoreACustomPatternee1663') }}</label>
          <input v-model="custom" :aria-label="t('uiCustomIgnorePattern91022b')" :disabled="ruleKind !== 'custom'" :placeholder="t('uieGLog5be018')" />
          <label for="ignore-scope">{{ t('uiIgnoreScope663249') }}</label>
          <AppSelect id="ignore-scope" v-model="scope" :aria-label="t('uiIgnoreScope663249')" :disabled="loading" :options="[{ value: 'local', label: t('uiThisRepositoryOnThisDeviceGitInfoExclude3af437') }, { value: 'repository', label: t('uiSharedWithRepositoryGitignoreCommittabled01d97') }, { value: 'global', label: t('uiAllRepositoriesOnThisDeviceGlobalIgnore5075e4') }] as const" />
        </fieldset>
        <p v-if="previewLoading" role="status">{{ t('uiPreviewingRule19f087') }}</p>
        <div v-if="preview" class="ignore-preview"><span>{{ t('uiRuleed904c') }}</span><code>{{ preview.pattern }}</code><span>{{ t('uiWriteToFilee620fd') }}</span><code>{{ formatDisplayPath(preview.targetPath) }}</code></div>
        <p v-if="scope === 'global'">{{ t('uiGlobalRulesAffectAllRepositoriesOnThisDeviceUsingThisGlobalIa3d188') }}</p>
        <p v-if="preview?.tracked" class="warning">{{ t('uiThisFileIsAlreadyTrackedIgnoringItDoesNotStopTrackingUseStop00af94') }}</p>
        <p v-if="previewError || error" class="file-action-error" role="alert">{{ previewError || error }}</p>
      </ConfirmDialog>
      <ConfirmDialog v-else-if="dialog === 'history' || dialog === 'blame'" :title="dialog === 'history' ? t('uiFileHistoryLatest100Commitscd670c') : t('uiBlameLineHistory26c8ae')" :description="file?.path" :confirm-label="t('uiClose6c14bd')" :busy="loading" @cancel="close" @confirm="close">
        <p v-if="loading" role="status">{{ t('uiLoadingfcabad') }}</p><p v-if="error" class="file-action-error" role="alert">{{ error }}</p><pre class="file-inspection">{{ inspection }}</pre>
      </ConfirmDialog>
      <ConfirmDialog v-else :title="confirmationTitle" :description="confirmationText" :confirm-label="t('uiConfirmae1109')" :busy="loading" :confirm-disabled="!canWrite" :danger="dialog !== 'lfs'" @cancel="close" @confirm="confirm"><code class="confirmed-path">{{ file?.path }}</code><p v-if="error" class="file-action-error" role="alert">{{ error }}</p></ConfirmDialog>
    </div>
  </Teleport>
</template>

<style scoped>
.file-menu-shield { position: fixed; inset: 0; z-index: 90; }
.file-context-menu { position: fixed; width: 286px; max-width: calc(100vw - 16px); max-height: calc(100dvh - 16px); overflow-y: auto; padding: 6px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); box-shadow: var(--shadow-lg); color: var(--text); }
.menu-path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding: 5px 8px 9px; color: var(--text-muted); font-size: 12px; }
.file-context-menu button { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 31px; padding: 5px 8px; background: transparent; color: inherit; text-align: left; font-size: 12px; border-radius: 4px; }
.file-context-menu button.separator { border-top: 1px solid var(--border); margin-top: 4px; padding-top: 9px; }
.file-context-menu button:hover:not(:disabled), .file-context-menu button:focus-visible { background: var(--surface-muted); outline: 1px solid var(--primary); }
.file-context-menu button:disabled { opacity: .4; }
.ignore-options { display: grid; gap: 9px; border: 1px solid var(--border); border-radius: 6px; padding: 12px; font-size: 12px; }
.ignore-options label { display: flex; align-items: center; gap: 8px; }
.ignore-options select, .ignore-options > input { width: 100%; min-width: 0; padding: 7px; border: 1px solid var(--border); border-radius: 4px; color: var(--text); background: var(--surface-panel); }
.ignore-preview { display: grid; gap: 6px; margin-top: 12px; font-size: 12px; }
.ignore-preview code, .confirmed-path { overflow-wrap: anywhere; white-space: pre-wrap; }
.ignore-preview span { color: var(--text-muted); }
.file-action-error { color: var(--danger); overflow-wrap: anywhere; }
.file-action-notice, .file-action-error { font-size: 12px; padding: 8px; }
.file-action-notice { background: var(--surface-muted); overflow-wrap: anywhere; }
.file-action-notice button, .file-action-error button { float: right; background: transparent; color: inherit; }
.warning { color: var(--warning, var(--text-muted)); font-size: 12px; line-height: 1.6; }
.file-inspection { max-height: 55dvh; overflow: auto; background: var(--surface-muted); padding: 10px; font-size: var(--code-font-size, 13px); line-height: 1.7; }
.file-action-dialog :deep(.confirm-dialog) { max-height: calc(100dvh - 48px); overflow: auto; width: min(680px, calc(100vw - 48px)); }
.file-action-dialog :deep(.dialog-content) { grid-column: 1 / -1; }
</style>
