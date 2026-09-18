import { ref } from "vue";
import { normalizeBackendError } from "@/lib/backend/errors";
import { cleanGitErrorText } from "@/lib/gitFailure";
import { formatDisplayPath } from "@/lib/formatPath";
import type { BackendError, GitRunProgressPhase } from "@/lib/backend/types";

export type FeedbackStatus = "running" | "success" | "failed" | "cancelled" | "conflicted" | "warning";
export interface GitFeedback {
  id: string; root: string; title: string; target: string; status: FeedbackStatus;
  message: string; diagnostics?: string; phase?: string; percent?: number;
  startedAt: number; finishedAt?: number; canCancel: boolean; cancelling: boolean;
}
const MAX_COMPLETED = 30;
const MAX_TEXT = 4096;
const MAX_TARGET = 240;
const PERCENT_MAX = 100;
export const gitFeedback = ref<GitFeedback[]>([]);
export const feedbackExpanded = ref(true);
const cancellations = new Map<string, () => Promise<unknown>>();
const actions: Record<string, string> = {
  filesOpen: "打开文件", filesInspect: "读取文件历史", filesIgnore: "忽略文件", filesUntrack: "停止跟踪", filesLfsTrack: "跟踪 Git LFS 文件类型",
  activityRollback: "回滚操作", changesStageFiles: "批量暂存", changesUnstageFiles: "批量取消暂存",
  repositoryOpen: "打开仓库", repositoryInit: "初始化仓库", repositoryClone: "Clone 仓库", repositoryRefresh: "刷新仓库",
  changesStageFile: "暂存文件", changesUnstageFile: "取消暂存", changesStageHunk: "暂存代码块", changesUnstageHunk: "取消暂存代码块",
  changesStageLines: "暂存代码行", changesUnstageLines: "取消暂存代码行", changesDiscardFile: "丢弃文件修改", changesCommit: "Commit 提交",
  changesScanNoise: "检测无实质变更", changesRestoreNoise: "恢复无效变更", refsCreate: "创建分支", refsSwitch: "切换分支", refsDelete: "删除分支",
  refsMerge: "Merge 合并", refsRebase: "Rebase 变基", refsAbort: "中止 Git 操作", historyCheckout: "Checkout 检出",
  historyRevert: "Revert 提交", historyCherryPick: "Cherry-pick 提交", historyReset: "Reset 提交",
  stashCreate: "Stash 贮藏", stashApply: "应用贮藏", stashPop: "弹出贮藏", conflictsResolve: "解决冲突", conflictsContinue: "继续 Git 操作",
  taskBranchesCreate: "创建任务分支", taskBranchesRun: "提交并移植", taskBranchesUnlink: "解除分支关联", filesExecute: "文件操作",
  remoteStartFetch: "Fetch 获取", remoteStartPull: "Pull 拉取", remoteStartPush: "Push 推送", terminalStart: "Git 终端命令", consoleStart: "Git 命令",
};
const asyncActions = new Set(["remoteStartFetch", "remoteStartPull", "remoteStartPush", "terminalStart", "consoleStart"]);
const listeners = new Set(["gitRunListen", "terminalListen", "consoleListen"]);
const phaseNames: Record<GitRunProgressPhase, string> = {
  enumerating: "枚举对象", counting: "统计对象", compressing: "压缩对象", receiving: "接收对象", resolving: "解析差异", writing: "发送对象", updating: "更新引用",
};
function clean(value: string, limit = MAX_TEXT): string { return cleanGitErrorText(value).slice(0, limit); }
function object(value: unknown): Record<string, unknown> { return value !== null && typeof value === "object" ? value as Record<string, unknown> : {}; }
function targetFor(action: string, args: unknown[]): string {
  const request = object(args[asyncActions.has(action) ? 2 : 1]);
  if (action === "remoteStartPush") return clean(`${request.localBranch} → ${request.remote}/${request.remoteBranch}`, MAX_TARGET);
  if (action === "remoteStartPull") return clean(`${request.remote}/${request.remoteBranch} → ${request.localBranch ?? '当前分支'}`, MAX_TARGET);
  if (action === "refsMerge") return clean(`${args[1]} → ${args[2] ?? '当前分支'}`, MAX_TARGET);
  if (action === "remoteStartFetch") return clean(`远程：${request.remote}`, MAX_TARGET);
  if (["terminalStart", "consoleStart", "repositoryClone", "changesCommit", "activityRollback"].includes(action)) return "";
  if (Array.isArray(args[1])) return `${args[1].length} 个文件`;
  const value = typeof args[1] === "string" ? args[1] : request.relativePath ?? request.name ?? request.commit ?? request.target ?? request.ticket;
  return typeof value === "string" ? clean(value, MAX_TARGET) : "";
}
function prune(): void {
  let retained = 0;
  gitFeedback.value = gitFeedback.value.filter(item => item.status === "running" || retained++ < MAX_COMPLETED);
}
export function beginGitFeedback(action: string, args: unknown[]): GitFeedback {
  const id = asyncActions.has(action) ? String(args[1]) : crypto.randomUUID();
  const existing = gitFeedback.value.find(item => item.id === id);
  if (existing) return existing;
  const item: GitFeedback = { id, root: String(args[action === "repositoryClone" ? 1 : 0] ?? ""),
    title: actions[action]!, target: targetFor(action, args), status: "running", message: asyncActions.has(action) ? "正在启动 Git 操作…" : "正在执行，请稍候…",
    startedAt: Date.now(), canCancel: false, cancelling: false };
  gitFeedback.value.unshift(item); prune(); feedbackExpanded.value = true;
  return gitFeedback.value[0]!;
}
function finish(item: GitFeedback, status: Exclude<FeedbackStatus, "running">, message: string, error?: BackendError): void {
  if (item.status !== "running") return;
  Object.assign(item, { status, message: clean(message), diagnostics: error?.diagnostics ? clean(error.diagnostics) : undefined,
    finishedAt: Date.now(), canCancel: false, cancelling: false, percent: undefined, phase: undefined });
  cancellations.delete(item.id); feedbackExpanded.value = true; prune();
}
function fail(item: GitFeedback, cause: unknown): void {
  const error = normalizeBackendError(cause);
  finish(item, error.code === "gitRefreshFailed" ? "warning" : error.code === "cancelled" ? "cancelled" : error.code === "gitConflict" ? "conflicted" : "failed", error.message, error);
}
export function failGitFeedback(id: string, cause: unknown): void { const item = gitFeedback.value.find(item => item.id === id); if (item) fail(item, cause); }
function finishResult(item: GitFeedback, result: unknown): void {
  const value = object(result);
  if (value.error) { fail(item, value.error); return; }
  const operation = object(value.operationState ?? object(value.workspace).operationState);
  if ((typeof operation.kind === "string" && operation.kind !== "none") || (Array.isArray(operation.conflicts) && operation.conflicts.length > 0)) {
    finish(item, "conflicted", "本次操作已执行，Git 流程尚未结束。请到冲突工作台继续或中止。"); return;
  }
  finish(item, "success", `${item.title}成功${item.target ? `：${item.target}` : ""}`);
}
export function dismissFeedback(id: string): void { gitFeedback.value = gitFeedback.value.filter(item => item.id !== id || item.status === "running"); }
export function clearCompletedFeedback(): void { gitFeedback.value = gitFeedback.value.filter(item => item.status === "running"); }
export function resetGitFeedback(): void { gitFeedback.value = []; cancellations.clear(); feedbackExpanded.value = true; }
export async function cancelFeedback(id: string): Promise<void> {
  const item = gitFeedback.value.find(item => item.id === id), cancel = cancellations.get(id);
  if (!item || !cancel || item.status !== "running" || item.cancelling) return;
  item.cancelling = true;
  try { await cancel(); if (item.status === "running") item.message = "已请求停止，正在等待 Git 结束…"; }
  catch (cause) { if (item.status === "running") { const error = normalizeBackendError(cause); item.cancelling = false; item.message = `停止请求失败：${clean(error.message)}，Git 操作可能仍在运行。`; item.diagnostics = error.diagnostics ? clean(error.diagnostics) : undefined; } }
}

// The observer runs for native clients and test fixtures alike. A start response
// only acknowledges an asynchronous command; terminal events decide its outcome.
export function observeGitFeedback<T extends object>(client: T): T {
  const runs = new Map<string, { item: GitFeedback; sequence: number }>();
  function eventReceived(message: unknown): void {
    const envelope = object(message), runId = String(envelope.runId ?? ""), run = runs.get(runId);
    if (!run || run.item.status !== "running" || typeof envelope.sequence !== "number" || envelope.sequence <= run.sequence) return;
    if (typeof envelope.rootPath === "string") {
      const normalize = (path: string) => { const displayed = formatDisplayPath(path).replaceAll("\\", "/").replace(/\/$/, ""); return /^[a-z]:\//i.test(displayed) || displayed.startsWith("//") ? displayed.toLowerCase() : displayed; };
      if (normalize(envelope.rootPath) !== normalize(run.item.root)) return;
    }
    run.sequence = envelope.sequence;
    const event = object(envelope.event), item = run.item;
    if (event.kind === "started") { item.message = "Git 操作进行中，等待进度信息…"; return; }
    if (event.kind === "output") { item.message = "命令执行中，详细输出见 Git 终端。"; return; }
    if (event.kind === "progress") {
      const text = clean(String(event.text ?? ""));
      item.message = text;
      item.phase = phaseNames[event.phase as GitRunProgressPhase] ?? "正在处理";
      const percent = /(?:^|\s)(\d{1,3})%/.exec(text);
      item.percent = percent && Number(percent[1]) <= PERCENT_MAX ? Number(percent[1]) : undefined;
      return;
    }
    if (event.kind === "completed") finishResult(item, event.result);
    else if (event.kind === "conflicted") finish(item, "conflicted", "操作产生冲突，请到冲突工作台解决后继续。");
    else if (event.kind === "cancelled" || event.cancelled === true || event.outcome === "cancelled") finish(item, "cancelled", "操作已取消，请检查仓库当前状态。");
    else if (event.error) fail(item, event.error);
    else if (event.kind === "exited" || event.kind === "terminal") {
      if (event.exitCode === 0 && (event.kind === "exited" || event.outcome === "completed")) finishResult(item, undefined);
      else finish(item, "failed", event.outcome === "timedOut" ? "命令执行超时，请查看终端输出。" : `命令失败（退出码 ${event.exitCode ?? "未知"}），请查看终端输出。`);
    } else if (event.kind === "failed") finish(item, "failed", "Git 操作失败，请查看诊断信息。");
    if (item.status !== "running") runs.delete(runId);
  }
  return new Proxy(client, {
    get(target, key, receiver) {
      const method = Reflect.get(target, key, receiver);
      if (typeof key !== "string" || typeof method !== "function") return method;
      if (listeners.has(key)) return (listener: (event: unknown) => void) => Reflect.apply(method, target, [(event: unknown) => { eventReceived(event); listener(event); }]);
      if (!actions[key]) return method;
      return async (...args: unknown[]) => {
        if (key === "repositoryRefresh" && args[1] === true) return Reflect.apply(method, target, args);
        const item = beginGitFeedback(key, args), asynchronous = asyncActions.has(key), runId = String(args[1] ?? "");
        if (asynchronous) {
          runs.set(runId, { item, sequence: 0 });
          const cancelMethod = key === "terminalStart" ? "terminalTerminate" : key === "consoleStart" ? "consoleCancel" : "gitRunCancel";
          const cancel = Reflect.get(target, cancelMethod);
          if (typeof cancel === "function") {
            cancellations.set(item.id, () => Reflect.apply(cancel, target, cancelMethod === "gitRunCancel" ? [runId] : [args[0], runId]));
            item.canCancel = true;
          }
        }
        try {
          const result = await Reflect.apply(method, target, args);
          if (!asynchronous) finishResult(item, result);
          return result;
        } catch (cause) { fail(item, cause); if (asynchronous) runs.delete(runId); throw cause; }
      };
    },
  });
}
