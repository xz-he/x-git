import { t } from '@/lib/i18n';
import { normalizeBackendError } from "@/lib/backend/errors";
import type { BackendError } from "@/lib/backend/types";

export interface GitFailure { root: string; command: string; error: BackendError; output?: string; }
const listeners = new Set<(failure: GitFailure) => void>();
let epoch = 0;
export function resetGitFailureScope(): void { epoch++; }
export function onGitFailure(listener: (failure: GitFailure) => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
export function reportGitFailure(failure: GitFailure): void {
  if (failure.error.code === "cancelled") return;
  for (const listener of listeners) listener(failure);
}

export function cleanGitErrorText(value: string): string {
  return value
    .replace(/\x1b\][^\x07]*(?:\x07|\x1b\\)/g, "")
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g, "")
    .replace(/(https?:\/\/)[^\s/@]+@/gi, "$1[REDACTED]@")
    .replace(/((?:authorization\s*[:=]\s*(?:bearer|basic)|(?:api[_-]?key|access_token|password|token)\s*[:=])\s*)[^\s&"']+/gi, "$1[REDACTED]");
}

const actions: Record<string, string> = {
  get activityRollback() { return t('uiRollBackHistoryEntry8c4178'); },
  get changesStageFiles() { return t('uiBatchGitAdd781d9f'); }, get changesUnstageFiles() { return t('uiUnstageSelectedFileseaed74'); },
  get repositoryOpen() { return t('uiOpenRepository47f538'); }, repositoryInit: "git init", repositoryClone: "git clone", get repositoryRefresh() { return t('uiRefreshRepository70ad8c'); },
  changesStageFile: "git add", get changesUnstageFile() { return t('uiUnstage807956'); }, get changesStageHunk() { return t('uiStageHunkb02c85'); }, get changesUnstageHunk() { return t('uiUnstageHunkb3c8db'); },
  get changesStageLines() { return t('uiStageLines5bc946'); }, get changesUnstageLines() { return t('uiUnstageLines7d15eb'); }, get changesDiscardFile() { return t('uiDiscardFileChanges6923a0'); }, changesCommit: "git commit",
  get changesRestoreNoise() { return t('uiRestoreNonSubstantiveChanges179f9e'); }, refsCreate: "git branch", refsSwitch: "git switch", refsDelete: "git branch -d",
  refsMerge: "git merge", refsRebase: "git rebase", get refsAbort() { return t('uiAbortGitOperation750351'); },
  historyCheckout: "git checkout", historyRevert: "git revert", historyCherryPick: "git cherry-pick", historyReset: "git reset",
  stashCreate: "git stash push", stashApply: "git stash apply", stashPop: "git stash pop",
  get conflictsResolve() { return t('conflicts'); }, get conflictsContinue() { return t('uiContinueGitOperation042a2a'); }, get taskBranchesCreate() { return t('uiCreateTaskBranch6f6fc8'); }, get taskBranchesRun() { return t('uiCommitAndCherryPickbece15'); },
};

// Observe user-facing Git operations at the IPC boundary, without changing their errors.
export function observeGitFailures<T extends object>(client: T): T {
  return new Proxy(client, {
    get(target, key, receiver) {
      const method = Reflect.get(target, key, receiver);
      const action = typeof key === "string" ? actions[key] : undefined;
      if (!action || typeof method !== "function") return method;
      return async (...args: unknown[]) => {
        if (key === "repositoryRefresh" && args[1] === true) return Reflect.apply(method, target, args);
        const version = epoch;
        const report = (cause: unknown) => {
          const error = normalizeBackendError(cause);
          if (version !== epoch || error.code.startsWith("ai") || error.code === "unexpected") return;
          reportGitFailure({ root: String(args[key === "repositoryClone" ? 1 : 0] ?? ""),
            command: t('msgOperationArgumentsaab145', { p0: action, p1: JSON.stringify(args.slice(1)) }), error });
        };
        try {
          const result = await Reflect.apply(method, target, args);
          if (result && typeof result === "object" && "error" in result && result.error) report(result.error);
          return result;
        } catch (error) { report(error); throw error; }
      };
    },
  });
}
