import { t } from '@/lib/i18n';
import { computed, ref } from "vue";
import { defineStore } from "pinia";

import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import { reportGitFailure } from "@/lib/gitFailure";
import { beginGitFeedback, failGitFeedback } from "@/lib/gitFeedback";
import type {
  BackendError,
  GitRunAccepted,
  GitRunEvent,
  GitRunOperation,
  GitRunProgressPhase,
  PullRequest,
  PushRequest,
  RemoteOperationResult,
  RemoteSnapshot,
} from "@/lib/backend/types";
import { createModuleLifecycle } from "@/stores/moduleLifecycle";
import { useOperationStore } from "@/stores/operation";
import { useRepositoryStore } from "@/stores/repository";
import { useTerminalStore } from "@/stores/terminal";

export type GitRunStatus =
  | "idle"
  | "starting"
  | "running"
  | "completed"
  | "conflicted"
  | "cancelled"
  | "failed";

export interface GitRunProgress {
  phase: GitRunProgressPhase;
  text: string;
}

export type RemoteDialogAction = "pull" | "push";

export const useRemotesStore = defineStore("remotes", () => {
  const snapshot = ref<RemoteSnapshot>();
  const selectedRemoteName = ref<string>();
  const loadedRootPath = ref<string>();
  const generation = ref(0);
  const loading = ref(false);
  const runId = ref<string>();
  const lastSequence = ref(0);
  const operation = ref<GitRunOperation>();
  const progress = ref<GitRunProgress>();
  const cancelRequested = ref(false);
  const status = ref<GitRunStatus>("idle");
  const error = ref<BackendError>();
  const requestedAction = ref<RemoteDialogAction>();
  const running = computed(
    () => status.value === "starting" || status.value === "running",
  );
  const lifecycle = createModuleLifecycle();

  let requestVersion = 0;
  let pending: Promise<void> | undefined;
  let unlisten: (() => void) | undefined;
  let initializePromise: Promise<void> | undefined;
  let cancellation: { runId: string; promise: Promise<void> } | undefined;
  let runOwner:
    | { runId: string; rootPath: string; generation: number }
    | undefined;

  function initialize(): Promise<void> {
    if (unlisten) {
      return Promise.resolve();
    }
    if (initializePromise) {
      return initializePromise;
    }
    initializePromise = backendClient
      .gitRunListen(handleEvent)
      .then((dispose) => {
        unlisten = dispose;
      })
      .finally(() => {
        initializePromise = undefined;
      });
    return initializePromise;
  }

  function dispose(): void {
    unlisten?.();
    unlisten = undefined;
  }

  function resetForRepository(rootPath: string, nextGeneration: number): void {
    requestVersion += 1;
    lifecycle.replaceRepository(rootPath, nextGeneration);
    loadedRootPath.value = rootPath;
    generation.value = nextGeneration;
    snapshot.value = undefined;
    selectedRemoteName.value = undefined;
    loading.value = false;
    pending = undefined;
    requestedAction.value = undefined;
    abandonRun();
  }

  function ensureLoaded(rootPath: string, nextGeneration: number, acceptsRefresh: () => boolean = () => true): Promise<void> {
    if (
      loadedRootPath.value !== rootPath ||
      generation.value !== nextGeneration
    ) {
      resetForRepository(rootPath, nextGeneration);
    }
    if (snapshot.value) {
      return Promise.resolve();
    }
    if (pending) {
      return pending;
    }

    const token = lifecycle.begin(rootPath, nextGeneration);
    const version = ++requestVersion;
    loading.value = true;
    error.value = undefined;
    pending = (async () => {
      try {
        const result = await backendClient.remotesSnapshot(rootPath);
        if (!lifecycle.accept(token) || version !== requestVersion || !acceptsRefresh()) {
          return;
        }
        applySnapshot(result);
      } catch (cause) {
        if (!lifecycle.accept(token) || version !== requestVersion || !acceptsRefresh()) {
          return;
        }
        error.value = normalizeBackendError(cause);
        throw error.value;
      } finally {
        if (version === requestVersion) {
          loading.value = false;
          pending = undefined;
        }
      }
    })();
    return pending;
  }

  function applySnapshot(next: RemoteSnapshot): void {
    const previous = selectedRemoteName.value;
    snapshot.value = next;
    selectedRemoteName.value =
      next.remotes.find((remote) => remote.name === previous)?.name ??
      next.remotes[0]?.name;
  }

  function fetch(remote: string): Promise<void> {
    return start("fetch", { remote }, (rootPath, nextRunId) =>
      backendClient.remoteStartFetch(rootPath, nextRunId, { remote }),
    );
  }

  function pull(remote: string, remoteBranch: string, localBranch?: string): Promise<void> {
    return start("pull", { remote, remoteBranch, localBranch }, (rootPath, nextRunId) =>
      backendClient.remoteStartPull(rootPath, nextRunId, {
        remote,
        remoteBranch,
        ...(localBranch ? { localBranch } : {}),
      } satisfies PullRequest),
    );
  }

  function push(request: PushRequest): Promise<void> {
    return start("push", request, (rootPath, nextRunId) =>
      backendClient.remoteStartPush(rootPath, nextRunId, request),
    );
  }

  function requestAction(action: RemoteDialogAction): void {
    requestedAction.value = action;
  }

  function dismissAction(): void {
    requestedAction.value = undefined;
  }

  async function fetchSelected(): Promise<void> {
    const repository = useRepositoryStore();
    const rootPath = repository.snapshot?.rootPath;
    if (!rootPath) {
      return;
    }
    await ensureLoaded(rootPath, repository.generation);
    const remote = selectedRemoteName.value;
    if (remote) {
      await fetch(remote);
    }
  }

  async function start(
    nextOperation: GitRunOperation,
    request: object,
    action: (rootPath: string, runId: string) => Promise<GitRunAccepted>,
  ): Promise<void> {
    if (running.value || useTerminalStore().busy) {
      throw {
        code: "gitOperationInProgress",
        get message() { return t('uiARemoteSyncOperationIsAlreadyRunningec9a25'); },
      } satisfies BackendError;
    }
    const repository = useRepositoryStore();
    const rootPath = repository.snapshot?.rootPath;
    if (!rootPath) {
      const missing: BackendError = {
        code: "invalidRepository",
        get message() { return t('uiOpenAGitRepositoryFirsta00a3e'); },
      };
      error.value = missing;
      status.value = "failed";
      throw missing;
    }

    const nextRunId = crypto.randomUUID();
    prepareRun(nextRunId, nextOperation, rootPath, repository.generation);
    const feedbackActions = { fetch: "remoteStartFetch", pull: "remoteStartPull", push: "remoteStartPush" };
    beginGitFeedback(feedbackActions[nextOperation], [rootPath, nextRunId, request]);
    try {
      await initialize();
      if (!ownsRun(nextRunId)) { failGitFeedback(nextRunId, { code: "cancelled", get message() { return t('uiCancelledBeforeTheOperationStarteda347a1'); } }); return; }
      const accepted = await action(rootPath, nextRunId);
      if (
        accepted.runId === nextRunId &&
        ownsRun(nextRunId) &&
        !isTerminal(status.value)
      ) {
        operation.value = accepted.operation;
        status.value = "running";
      }
    } catch (cause) {
      const normalized = normalizeBackendError(cause);
      failGitFeedback(nextRunId, normalized);
      if (ownsRun(nextRunId)) {
        reportGitFailure({ root: rootPath, command: `git ${nextOperation}`, error: normalized });
        status.value = "failed";
        error.value = normalized;
        runId.value = undefined;
        runOwner = undefined;
      }
      throw normalized;
    }
  }

  function prepareRun(
    nextRunId: string,
    nextOperation: GitRunOperation,
    rootPath: string,
    nextGeneration: number,
  ): void {
    runId.value = nextRunId;
    lastSequence.value = 0;
    operation.value = nextOperation;
    progress.value = undefined;
    cancelRequested.value = false;
    status.value = "starting";
    error.value = undefined;
    cancellation = undefined;
    runOwner = {
      runId: nextRunId,
      rootPath,
      generation: nextGeneration,
    };
  }

  function handleEvent(message: GitRunEvent): void {
    if (
      !ownsRun(message.runId) ||
      message.sequence <= lastSequence.value
    ) {
      return;
    }
    lastSequence.value = message.sequence;
    const payload = message.event;
    switch (payload.kind) {
      case "started":
        operation.value = payload.operation;
        status.value = "running";
        break;
      case "progress":
        progress.value = { phase: payload.phase, text: payload.text };
        status.value = "running";
        break;
      case "completed":
        applyTerminalResult(payload.result);
        status.value = "completed";
        finishRun();
        break;
      case "conflicted":
        if (runOwner) reportGitFailure({ root: runOwner.rootPath, command: `git ${operation.value}`,
          error: { code: "gitConflict", get message() { return t('uiTheGitOperationHasConflictsCheckConflictedFiles20a0cc'); } }, output: progress.value?.text });
        applyTerminalResult(payload.result);
        status.value = "conflicted";
        finishRun();
        break;
      case "cancelled":
        applyTerminalResult(payload.result);
        status.value = "cancelled";
        finishRun();
        break;
      case "failed":
        if (runOwner) reportGitFailure({ root: runOwner.rootPath, command: `git ${operation.value}`, error: payload.error, output: progress.value?.text });
        if (payload.result) {
          applyTerminalResult(payload.result);
        }
        error.value = payload.error;
        status.value = "failed";
        finishRun();
        break;
    }
  }

  function applyTerminalResult(result: RemoteOperationResult): void {
    useOperationStore().applyMutationResult(result);
    applySnapshot(result.remotes);
  }

  function cancel(): Promise<void> {
    const activeRunId = runId.value;
    if (!activeRunId || !running.value) {
      return Promise.resolve();
    }
    if (cancellation?.runId === activeRunId) {
      return cancellation.promise;
    }
    cancelRequested.value = true;
    const promise = backendClient.gitRunCancel(activeRunId).catch((cause) => {
      error.value = normalizeBackendError(cause);
      cancelRequested.value = false;
      cancellation = undefined;
      throw error.value;
    });
    cancellation = { runId: activeRunId, promise };
    return promise;
  }

  async function cancelAndAbandon(): Promise<void> {
    const abandonedRunId = runId.value;
    await cancel();
    if (runId.value === abandonedRunId) {
      abandonRun();
    }
  }

  function ownsRun(candidate: string): boolean {
    const repository = useRepositoryStore();
    return (
      candidate === runId.value &&
      runOwner?.runId === candidate &&
      runOwner.rootPath === repository.snapshot?.rootPath &&
      runOwner.generation === repository.generation
    );
  }

  function finishRun(): void {
    cancelRequested.value = false;
    cancellation = undefined;
    runOwner = undefined;
  }

  function abandonRun(): void {
    runId.value = undefined;
    lastSequence.value = 0;
    operation.value = undefined;
    progress.value = undefined;
    cancelRequested.value = false;
    status.value = "idle";
    error.value = undefined;
    cancellation = undefined;
    runOwner = undefined;
  }

  return {
    snapshot,
    selectedRemoteName,
    loadedRootPath,
    generation,
    loading,
    runId,
    lastSequence,
    operation,
    progress,
    cancelRequested,
    status,
    error,
    requestedAction,
    running,
    initialize,
    dispose,
    resetForRepository,
    ensureLoaded,
    applySnapshot,
    fetch,
    pull,
    push,
    requestAction,
    dismissAction,
    fetchSelected,
    handleEvent,
    cancel,
    cancelAndAbandon,
  };
});

function isTerminal(status: GitRunStatus): boolean {
  return ["completed", "conflicted", "cancelled", "failed"].includes(status);
}
