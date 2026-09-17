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
  activityRollback: "回滚操作历史",
  changesStageFiles: "批量 git add", changesUnstageFiles: "批量取消暂存",
  repositoryOpen: "打开仓库", repositoryInit: "git init", repositoryClone: "git clone", repositoryRefresh: "刷新仓库",
  changesStageFile: "git add", changesUnstageFile: "取消暂存", changesStageHunk: "暂存代码块", changesUnstageHunk: "取消暂存代码块",
  changesStageLines: "暂存代码行", changesUnstageLines: "取消暂存代码行", changesDiscardFile: "丢弃文件修改", changesCommit: "git commit",
  changesRestoreNoise: "恢复无效变更", refsCreate: "git branch", refsSwitch: "git switch", refsDelete: "git branch -d",
  refsMerge: "git merge", refsRebase: "git rebase", refsAbort: "中止 Git 操作",
  historyCheckout: "git checkout", historyRevert: "git revert", historyCherryPick: "git cherry-pick", historyReset: "git reset",
  stashCreate: "git stash push", stashApply: "git stash apply", stashPop: "git stash pop",
  conflictsResolve: "解决冲突", conflictsContinue: "继续 Git 操作", taskBranchesCreate: "创建任务分支", taskBranchesRun: "提交并移植",
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
            command: `${action}\n操作参数：${JSON.stringify(args.slice(1))}`, error });
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
