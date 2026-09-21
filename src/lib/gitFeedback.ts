import { t } from '@/lib/i18n';
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
  get filesOpen() { return t('uiOpenFile38820b'); }, get filesInspect() { return t('uiReadFileHistory54621f'); }, get filesIgnore() { return t('uiIgnoreFile98d883'); }, get filesUntrack() { return t('uiStopTracking50a5fd'); }, get filesLfsTrack() { return t('uiTrackFileExtensionWithGitLFS380602'); },
  get activityRollback() { return t('uiRollBackOperation26bde4'); }, get changesStageFiles() { return t('uiStageSelectedFiles6d66ba'); }, get changesUnstageFiles() { return t('uiUnstageSelectedFileseaed74'); },
  get repositoryOpen() { return t('uiOpenRepository47f538'); }, get repositoryInit() { return t('uiInitializeRepositoryc681d9'); }, get repositoryClone() { return t('uiCloneRepository584580'); }, get repositoryRefresh() { return t('uiRefreshRepository70ad8c'); },
  get changesStageFile() { return t('uiStageFile6c6746'); }, get changesUnstageFile() { return t('uiUnstage807956'); }, get changesStageHunk() { return t('uiStageHunkb02c85'); }, get changesUnstageHunk() { return t('uiUnstageHunkb3c8db'); },
  get changesStageLines() { return t('uiStageLines5bc946'); }, get changesUnstageLines() { return t('uiUnstageLines7d15eb'); }, get changesDiscardFile() { return t('uiDiscardFileChanges6923a0'); }, get changesCommit() { return t('uiCommit2ca619'); },
  get changesScanNoise() { return t('uiDetectNonSubstantiveChangesdeff45'); }, get changesRestoreNoise() { return t('uiRestoreNonSubstantiveChanges179f9e'); }, get refsCreate() { return t('uiCreateBranchcd7ca0'); }, get refsSwitch() { return t('uiSwitchBranchcfeb33'); }, get refsDelete() { return t('uiDeleteBranch6203f5'); },
  get refsMerge() { return t('uiMerge8cbd5c'); }, get refsRebase() { return t('uiRebaseb66763'); }, get refsAbort() { return t('uiAbortGitOperation750351'); }, get historyCheckout() { return t('uiCheckout4401e3'); },
  get historyRevert() { return t('uiRevertCommit7794fc'); }, get historyCherryPick() { return t('uiCherryPickCommit1c6e85'); }, get historyReset() { return t('uiResetCommit4deab4'); },
  get stashCreate() { return t('uiStashd339dc'); }, get stashApply() { return t('uiApplyStashb19844'); }, get stashPop() { return t('uiPopStash0d142f'); }, get conflictsResolve() { return t('conflicts'); }, get conflictsContinue() { return t('uiContinueGitOperation042a2a'); },
  get taskBranchesCreate() { return t('uiCreateTaskBranch6f6fc8'); }, get taskBranchesRun() { return t('uiCommitAndCherryPickbece15'); }, get taskBranchesUnlink() { return t('uiUnlinkBranchf2b89f'); }, get filesExecute() { return t('uiFileActionsfd1cf9'); },
  get remoteStartFetch() { return t('uiFetcha82f56'); }, get remoteStartPull() { return t('uiPull0edfd5'); }, get remoteStartPush() { return t('uiPush7931c5'); },
};
// Terminal commands already show progress and results in their own terminal.
const asyncActions = new Set(["remoteStartFetch", "remoteStartPull", "remoteStartPush"]);
const listeners = new Set(["gitRunListen"]);
const phaseNames: Record<GitRunProgressPhase, string> = {
  get enumerating() { return t('uiEnumeratingObjectsb278da'); }, get counting() { return t('uiCountingObjects3edfa6'); }, get compressing() { return t('uiCompressingObjects04c067'); }, get receiving() { return t('uiReceivingObjects6a511e'); }, get resolving() { return t('uiResolvingDeltas4c6de3'); }, get writing() { return t('uiWritingObjects4b9b85'); }, get updating() { return t('uiUpdatingRefs85899e'); },
};
function clean(value: string, limit = MAX_TEXT): string { return cleanGitErrorText(value).slice(0, limit); }
function object(value: unknown): Record<string, unknown> { return value !== null && typeof value === "object" ? value as Record<string, unknown> : {}; }
function targetFor(action: string, args: unknown[]): string {
  const request = object(args[asyncActions.has(action) ? 2 : 1]);
  if (action === "remoteStartPush") return clean(`${request.localBranch} → ${request.remote}/${request.remoteBranch}`, MAX_TARGET);
  if (action === "remoteStartPull") return clean(`${request.remote}/${request.remoteBranch} → ${request.localBranch ?? t('uiCurrentBranch0eb05c')}`, MAX_TARGET);
  if (action === "refsMerge") return clean(`${args[1]} → ${args[2] ?? t('uiCurrentBranch0eb05c')}`, MAX_TARGET);
  if (action === "remoteStartFetch") return clean(t('msgRemotecf26d2', { p0: request.remote }), MAX_TARGET);
  if (["repositoryClone", "changesCommit", "activityRollback"].includes(action)) return "";
  if (Array.isArray(args[1])) return t('msgFiles62287c', { p0: args[1].length });
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
    title: actions[action]!, target: targetFor(action, args), status: "running", message: asyncActions.has(action) ? t('uiStartingGitOperation4d041c') : t('uiRunningPleaseWait634d7c'),
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
    finish(item, "conflicted", t('uiThisActionHasRunButTheGitOperationIsNotFinishedContinueOrAbo8f1349')); return;
  }
  finish(item, "success", t('msgSucceeded981c8a', { p0: item.title, p1: item.target ? `：${item.target}` : "" }));
}
export function dismissFeedback(id: string): void { gitFeedback.value = gitFeedback.value.filter(item => item.id !== id || item.status === "running"); }
export function clearCompletedFeedback(): void { gitFeedback.value = gitFeedback.value.filter(item => item.status === "running"); }
export function resetGitFeedback(): void { gitFeedback.value = []; cancellations.clear(); feedbackExpanded.value = true; }
export async function cancelFeedback(id: string): Promise<void> {
  const item = gitFeedback.value.find(item => item.id === id), cancel = cancellations.get(id);
  if (!item || !cancel || item.status !== "running" || item.cancelling) return;
  item.cancelling = true;
  try { await cancel(); if (item.status === "running") item.message = t('uiStopRequestedWaitingForGitToFinishf560ca'); }
  catch (cause) { if (item.status === "running") { const error = normalizeBackendError(cause); item.cancelling = false; item.message = t('msgStopRequestFailedTheGitOperationMayStillBeRunninge3d03a', { p0: clean(error.message) }); item.diagnostics = error.diagnostics ? clean(error.diagnostics) : undefined; } }
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
    if (event.kind === "started") { item.message = t('uiGitOperationInProgressWaitingForProgressInformation8f156e'); return; }
    if (event.kind === "output") { item.message = t('uiCommandRunningSeeTheGitTerminalForDetailedOutput87c08f'); return; }
    if (event.kind === "progress") {
      const text = clean(String(event.text ?? ""));
      item.message = text;
      item.phase = phaseNames[event.phase as GitRunProgressPhase] ?? t('uiProcessing656aa6');
      const percent = /(?:^|\s)(\d{1,3})%/.exec(text);
      item.percent = percent && Number(percent[1]) <= PERCENT_MAX ? Number(percent[1]) : undefined;
      return;
    }
    if (event.kind === "completed") finishResult(item, event.result);
    else if (event.kind === "conflicted") finish(item, "conflicted", t('uiOperationHasConflictsResolveThemInTheConflictWorkbenchThenCo5b678a'));
    else if (event.kind === "cancelled" || event.cancelled === true || event.outcome === "cancelled") finish(item, "cancelled", t('uiOperationCancelledCheckTheCurrentRepositoryState1c4bf0'));
    else if (event.error) fail(item, event.error);
    else if (event.kind === "exited" || event.kind === "terminal") {
      if (event.exitCode === 0 && (event.kind === "exited" || event.outcome === "completed")) finishResult(item, undefined);
      else finish(item, "failed", event.outcome === "timedOut" ? t('uiCommandTimedOutCheckTerminalOutput1b504e') : t('msgCommandFailedExitCodeCheckTerminalOutput79d8c7', { p0: event.exitCode ?? t('uiUnknownd9c32a') }));
    } else if (event.kind === "failed") finish(item, "failed", t('uiGitOperationFailedCheckDiagnosticsd439b6'));
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
          const cancel = Reflect.get(target, "gitRunCancel");
          if (typeof cancel === "function") {
            cancellations.set(item.id, () => Reflect.apply(cancel, target, [runId]));
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
