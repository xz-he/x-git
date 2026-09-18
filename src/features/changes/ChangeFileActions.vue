<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { ExternalLink, FolderOpen, Copy, Columns2, Plus, Minus, Package, Trash2, Undo2, EyeOff, Unlink, GitCommitHorizontal, GitMerge, Terminal, History, ListOrdered } from "@lucide/vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
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
  { id: "open", label: "Open", hint: "打开文件", icon: ExternalLink, enabled: exists.value },
  { id: "reveal", label: "Show in Explorer", hint: "在资源管理器中显示", icon: FolderOpen, enabled: exists.value },
  { id: "copy", label: "Copy Path", hint: "复制完整路径", icon: Copy, enabled: true },
  { id: "diff", label: "External Diff", hint: "使用已配置的 Git difftool", icon: Columns2, enabled: !untracked.value && !file.value?.conflict, separator: true },
  { id: "stage", label: context.value?.scope === "staged" ? "Unstage" : "Stage", hint: "仅操作此文件", icon: context.value?.scope === "staged" ? Minus : Plus, enabled: canWrite.value },
  { id: "lfs", label: "Track Type with Git LFS…", hint: "跟踪此扩展名，需要安装 Git LFS", icon: Package, enabled: canWrite.value && exists.value && !!extension.value },
  { id: "delete", label: "Remove…", hint: "移除磁盘文件（可恢复）", icon: Trash2, enabled: canWrite.value && exists.value },
  { id: "restore", label: "Restore Changes…", hint: "从暂存区恢复，丢弃此文件未暂存修改", icon: Undo2, enabled: canWrite.value && !untracked.value && !!file.value?.unstaged && !file.value?.conflict && context.value?.scope === "unstaged" },
  { id: "ignore", label: "Ignore…", hint: "配置忽略规则", icon: EyeOff, enabled: canWrite.value && exists.value },
  { id: "untrack", label: "Stop Tracking…", hint: "从 Git 索引移除，保留磁盘文件", icon: Unlink, enabled: canWrite.value && exists.value && !untracked.value },
  { id: "commit", label: "Commit…", hint: "暂存此文件并填写提交说明，不自动提交", icon: GitCommitHorizontal, enabled: canWrite.value },
  { id: "conflict", label: "Resolve Conflicts", hint: "在冲突工作台处理此文件", icon: GitMerge, enabled: !!file.value?.conflict && !conflicts.hasDirtyDrafts, separator: true },
  { id: "custom", label: "Custom Action…", hint: "在内嵌 Git 终端编辑文件命令", icon: Terminal, enabled: true },
  { id: "history", label: "File History…", hint: "查看最近 100 次文件提交", icon: History, enabled: !untracked.value && file.value?.indexStatus !== "A", separator: true },
  { id: "blame", label: "Blame…", hint: "按行追溯（包含当前工作区内容）", icon: ListOrdered, enabled: exists.value && !untracked.value && file.value?.indexStatus !== "A" },
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
    else if (action === "copy") { await navigator.clipboard.writeText(formatDisplayPath(id.root).replace(/[\\/]$/, "") + "/" + path); notice.value = "已复制完整文件路径。"; }
    else if (action === "stage" || action === "commit") {
      if (action === "commit" && id.scope === "staged") { /* Already staged. */ }
      else if (id.scope === "staged") await changes.unstageFile(path);
      else await changes.stageFile(path);
      if (action === "commit" && accepts(id)) { notice.value = "请填写提交说明；提交将包含所有已暂存文件。"; await nextTick(); document.querySelector<HTMLTextAreaElement>('[aria-label="提交说明"]')?.focus(); }
    } else if (action === "delete") {
      const result = await backendClient.filesPrepare(id.root, { kind: "delete", relativePath: path });
      if (accepts(id)) { prepared.value = result; dialog.value = "delete"; }
    } else if (action === "history" || action === "blame") {
      dialog.value = action; inspection.value = "";
      const text = await backendClient.filesInspect(id.root, path, action);
      if (accepts(id)) inspection.value = text || "没有可显示的记录。";
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
const confirmationText = computed(() => dialog.value === "delete" ? "将文件移至仓库恢复目录。此操作不会自动提交删除。" : dialog.value === "restore" ? "丢弃此文件的未暂存修改，恢复至暂存区版本。此修改无法通过本操作撤销。" : dialog.value === "untrack" ? "停止 Git 跟踪并暂存此移除，保留磁盘文件。若不希望文件再次出现，请另行添加忽略规则。" : `将 ${extension.value} 类型加入 .gitattributes 的 Git LFS 规则。不会迁移已有历史；规则文件需要另行暂存、提交。`);
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
      if (result.applied) { dialog.value = undefined; notice.value = result.recoveryPath ? `文件已移除，可从此目录恢复：${formatDisplayPath(result.recoveryPath)}` : "文件已移除。"; }
      if (result.error) throw result.error;
      if (!result.applied) throw new Error("文件未移除，请重新打开菜单重试。");
    } else if (action === "ignore") apply(id, await backendClient.filesIgnore(id.root, ignoreRequest));
    else if (action === "untrack") apply(id, await backendClient.filesUntrack(id.root, id.file.path));
    else if (action === "lfs") apply(id, await backendClient.filesLfsTrack(id.root, id.file.path));
    if (accepts(id)) { dialog.value = undefined; if (action !== "delete") notice.value = "操作成功。"; }
  }, action !== "restore");
}
onBeforeUnmount(() => { shown.value = false; });
</script>

<template>
  <div v-if="notice && !dialog" class="file-action-notice" role="status">{{ notice }} <button aria-label="关闭文件操作结果" @click="notice = ''">×</button></div>
  <div v-if="error && !dialog" class="file-action-error" role="alert">{{ error }} <button aria-label="关闭文件操作错误" @click="error = ''">×</button></div>
  <Teleport to="body">
    <div v-if="shown" class="file-menu-shield" @pointerdown.self="close" @contextmenu.prevent.self="close">
      <div ref="menu" class="file-context-menu" role="menu" aria-label="文件操作" :style="{ left: position.x + 'px', top: position.y + 'px' }" @keydown="menuKey">
        <div class="menu-path" :title="file?.path">{{ file?.path }}</div>
        <button v-for="item in items" :key="item.id" role="menuitem" :data-action="item.id" :class="{ separator: item.separator }" :title="item.hint" :disabled="busy || !item.enabled" @click="act(item.id)"><component :is="item.icon" :size="15" /><span>{{ item.label }}</span></button>
      </div>
    </div>
    <div v-if="dialog" class="file-action-dialog" @keydown.esc.stop.prevent="close">
      <ConfirmDialog v-if="dialog === 'ignore'" title="Ignore 忽略" :description="file?.path" confirm-label="保存忽略规则" :busy="loading" :confirm-disabled="!canWrite || !preview || previewLoading || !!previewError" @cancel="close" @confirm="confirm">
        <fieldset :disabled="loading" class="ignore-options">
          <legend>忽略文件名或模式</legend>
          <label><input v-model="ruleKind" type="radio" value="exact" />忽略精确的文件名</label>
          <label><input v-model="ruleKind" type="radio" value="extension" :disabled="!extension" />忽略所有 {{ extension || '（无扩展名）' }} 文件</label>
          <label><input v-model="ruleKind" type="radio" value="directory" :disabled="!directories.length" />忽略目录下所有文件</label>
          <select v-model="directory" aria-label="忽略目录" :disabled="ruleKind !== 'directory'"><option v-for="dir in directories" :key="dir" :value="dir">{{ dir }}/</option></select>
          <label><input v-model="ruleKind" type="radio" value="custom" />忽略自定义模式</label>
          <input v-model="custom" aria-label="自定义忽略模式" :disabled="ruleKind !== 'custom'" placeholder="例如 **/*.log" />
          <label for="ignore-scope">忽略范围</label>
          <select id="ignore-scope" v-model="scope"><option value="local">仅此仓库本机（.git/info/exclude）</option><option value="repository">仓库共享（.gitignore，可提交）</option><option value="global">本机所有仓库（全局忽略）</option></select>
        </fieldset>
        <p v-if="previewLoading" role="status">正在预览规则…</p>
        <div v-if="preview" class="ignore-preview"><span>规则</span><code>{{ preview.pattern }}</code><span>写入文件</span><code>{{ formatDisplayPath(preview.targetPath) }}</code></div>
        <p v-if="scope === 'global'">全局规则会影响本机所有使用该全局忽略文件的仓库。</p>
        <p v-if="preview?.tracked" class="warning">此文件已被跟踪，添加忽略规则不会停止跟踪。需要时请另行选择 Stop Tracking。</p>
        <p v-if="previewError || error" class="file-action-error" role="alert">{{ previewError || error }}</p>
      </ConfirmDialog>
      <ConfirmDialog v-else-if="dialog === 'history' || dialog === 'blame'" :title="dialog === 'history' ? 'File History · 最近 100 次提交' : 'Blame · 按行追溯'" :description="file?.path" confirm-label="关闭" :busy="loading" @cancel="close" @confirm="close">
        <p v-if="loading" role="status">正在读取…</p><p v-if="error" class="file-action-error" role="alert">{{ error }}</p><pre class="file-inspection">{{ inspection }}</pre>
      </ConfirmDialog>
      <ConfirmDialog v-else :title="confirmationTitle" :description="confirmationText" confirm-label="确认执行" :busy="loading" :confirm-disabled="!canWrite" :danger="dialog !== 'lfs'" @cancel="close" @confirm="confirm"><code class="confirmed-path">{{ file?.path }}</code><p v-if="error" class="file-action-error" role="alert">{{ error }}</p></ConfirmDialog>
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
