import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { ref } from "vue";
import type { BackendError } from "./types";

export interface ActivityEntry {
  id: string; title: string; createdAt: number;
  status: "pending" | "success" | "failed";
  message: string; rollbackKind: "index" | "revert" | "switch" | null;
  rollbackReason: string; rollbackId: string | null;
}
export const activityWarning = ref("");
export const ACTIVITY_UPDATED = "hq-git-activity-updated";
const asyncActions: Record<string, string> = { terminal_start: "Git 终端命令", console_start: "Git 命令", remote_start_fetch: "Fetch 远程", remote_start_pull: "Pull 远程", remote_start_push: "Push 远程" };
const pendingRuns = new Map<string, { path: string; title: string }>();
async function finishRun(runId: string, success: boolean, message: string): Promise<void> {
  const run = pendingRuns.get(runId);
  if (!run) return;
  pendingRuns.delete(runId);
  try { await tauriInvoke("activity_record_external", { ...run, success, message }); }
  catch { activityWarning.value = "命令已结束，但操作历史保存失败。请检查仓库状态，不要重复执行命令。"; }
  finally { window.dispatchEvent(new CustomEvent(ACTIVITY_UPDATED, { detail: run.path })); }
}
export function recordAsyncEvent(message: { runId: string; event: { kind: string; exitCode?: number | null; cancelled?: boolean; outcome?: string; error?: BackendError | null } }): void {
  const event = message.event;
  if (event.kind === "started" || event.kind === "progress" || event.kind === "output") return;
  const refreshFailed = event.error?.code === "gitRefreshFailed";
  const success = refreshFailed || event.kind === "completed" || (event.kind === "exited" && event.exitCode === 0 && !event.cancelled) || (event.kind === "terminal" && event.outcome === "completed");
  const description = refreshFailed ? event.error!.message : success ? "操作完成" : event.cancelled || event.kind === "cancelled" || event.outcome === "cancelled" ? "操作已取消，请检查仓库实际状态。" : "操作未成功完成，请查看命令输出或冲突工作台。";
  void finishRun(message.runId, success, description);
}
const recordedActions = new Set([
  "changes_stage_file", "changes_stage_files", "changes_unstage_file", "changes_unstage_files",
  "changes_stage_hunk", "changes_unstage_hunk", "changes_stage_lines", "changes_unstage_lines",
  "changes_discard_file", "changes_restore_noise", "changes_commit",
  "refs_create_branch", "refs_switch_branch", "refs_delete_branch", "refs_merge", "refs_rebase", "refs_abort",
  "history_checkout", "history_revert", "history_cherry_pick", "history_reset",
  "stash_create", "stash_apply", "stash_pop", "conflicts_resolve", "conflicts_continue", "files_execute",
  "task_branches_create", "task_branches_run", "task_branches_unlink",
]);

// Keep every existing IPC response contract, while recording native mutations
// at one boundary. Read queries and AI requests never enter operation history.
export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (asyncActions[command] && args) {
    const runId = String(args.runId);
    pendingRuns.set(runId, { path: String(args.path), title: asyncActions[command] });
    try { return await tauriInvoke<T>(command, args); }
    catch (cause) { await finishRun(runId, false, "命令启动失败，请查看命令输出。"); throw cause; }
  }
  if (!recordedActions.has(command)) return tauriInvoke<T>(command, args);
  try {
    const reply = await tauriInvoke<{ value: T; error: BackendError | null; warning: string | null }>("activity_execute", { action: command, args });
    if (reply.warning) activityWarning.value = reply.warning;
    if (reply.error) throw reply.error;
    return reply.value;
  } finally { window.dispatchEvent(new CustomEvent(ACTIVITY_UPDATED, { detail: args?.path })); }
}
