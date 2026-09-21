import { t } from '@/lib/i18n';
import { defineStore } from "pinia";
import { computed, ref } from "vue";

import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type {
  AiCommitMessageResult,
  AiConflictSuggestionResult,
  AiConnectionConfig,
  AiConnectionTestResult,
  AiContextSummary,
  AiReviewIssue,
  AiReviewResult,
  AiRunAccepted,
  AiRunEvent,
  AiTaskKind,
  BackendError,
  ReviewSource,
  ReviewSkillStatus,
} from "@/lib/backend/types";
import { useChangesStore } from "@/stores/changes";
import { useAiChatStore } from "@/stores/aiChat";
import { useRepositoryStore } from "@/stores/repository";
import { useSettingsStore } from "@/stores/settings";

export type AiRunStatus =
  | "idle"
  | "starting"
  | "running"
  | "completed"
  | "cancelled"
  | "failed";

export type AiConnectionStatus = "idle" | "testing" | "success" | "failed";

const terminalStatuses = new Set<AiRunStatus>([
  "completed",
  "cancelled",
  "failed",
]);

export const useAiStore = defineStore("ai", () => {
  const currentTask = ref<AiTaskKind>();
  const runId = ref<string>();
  const context = ref<AiContextSummary>();
  const lastSequence = ref(0);
  const completedBatchCount = ref(0);
  const totalBatchCount = ref(0);
  const streamedText = ref("");
  const partialIssues = ref<AiReviewIssue[]>([]);
  const reviewResult = ref<AiReviewResult>();
  const partialReviewResult = ref<AiReviewResult>();
  const commitPreview = ref("");
  const commitResult = ref<AiCommitMessageResult>();
  const conflictResult = ref<AiConflictSuggestionResult>();
  const conflictTarget = ref<{ relativePath: string; token: string }>();
  const status = ref<AiRunStatus>("idle");
  const error = ref<BackendError>();
  const connectionStatus = ref<AiConnectionStatus>("idle");
  const connectionResult = ref<AiConnectionTestResult>();
  const connectionError = ref<BackendError>();
  const reviewSource = ref<ReviewSource>();
  const runRoot = ref<string>();
  const runGeneration = ref<number>();
  const reviewSkill = ref<ReviewSkillStatus>();
  const reviewSkillRoot = ref<string>();
  const reviewSkillDirectory = ref<string>();
  const skillLoading = ref(false);
  const progressMessage = ref("");
  const running = computed(() => status.value === "starting" || status.value === "running");
  let skillVersion = 0;
  let lifecycleVersion = 0;
  let cancelRequestedFor: string | undefined;
  let skillPending: { key: string; promise: Promise<void> } | undefined;

  let unlisten: (() => void) | undefined;
  let initializePromise: Promise<void> | undefined;
  let cancellation:
    | { runId: string; promise: Promise<void> }
    | undefined;

  function initialize(): Promise<void> {
    if (unlisten) {
      return Promise.resolve();
    }
    if (initializePromise) {
      return initializePromise;
    }
    const version = lifecycleVersion;
    initializePromise = backendClient
      .aiListen(handleEvent)
      .then((dispose) => {
        if (version !== lifecycleVersion) { dispose(); return; }
        unlisten = dispose;
      })
      .finally(() => {
        initializePromise = undefined;
      });
    return initializePromise;
  }

  function dispose(): void {
    lifecycleVersion += 1;
    skillVersion += 1;
    void cancelActive().catch(() => undefined);
    unlisten?.();
    unlisten = undefined;
  }

  function startReview(source?: ReviewSource): Promise<void> {
    const frozenSource = source?.kind === "stagedFiles"
      ? { ...source, paths: [...source.paths] }
      : source ? { ...source } : undefined;
    return start("reviewChanges", frozenSource);
  }

  function startCommitMessage(): Promise<void> {
    return start("generateCommitMessage");
  }

  function startConflictSuggestion(relativePath: string, token: string): Promise<void> {
    return start("resolveConflict", undefined, { relativePath, token });
  }

  async function start(task: AiTaskKind, source?: ReviewSource, conflict?: { relativePath: string; token: string }): Promise<void> {
    const repositories = useRepositoryStore();
    if (running.value || useAiChatStore().running || cancellation || repositories.navigationBusy) {
      throw { code: "gitOperationInProgress", get message() { return t('uiWaitForTheCurrentOperationToFinish799cb7'); } } satisfies BackendError;
    }
    const rootPath = repositories.snapshot?.rootPath;
    if (!rootPath) {
      const missingRepository: BackendError = {
        code: "invalidRepository",
        get message() { return t('uiOpenAGitRepositoryFirsta00a3e'); },
      };
      error.value = missingRepository;
      status.value = "failed";
      throw missingRepository;
    }

    const nextRunId = crypto.randomUUID();
    prepareRun(task, nextRunId);
    const version = lifecycleVersion;
    runRoot.value = rootPath;
    runGeneration.value = repositories.generation;
    conflictTarget.value = conflict;
    reviewSource.value = task === "reviewChanges" ? source ?? { kind: "staged" } : undefined;
    try {
      await initialize();
      if (version !== lifecycleVersion || !unlisten || runId.value !== nextRunId || cancelRequestedFor === nextRunId) {
        throw { code: "cancelled", get message() { return t('uiAITaskCancelled3f208d'); } } satisfies BackendError;
      }
      const accepted =
        task === "reviewChanges"
          ? source
            ? await backendClient.aiStartReview(rootPath, nextRunId, source)
            : await backendClient.aiStartReview(rootPath, nextRunId)
          : task === "resolveConflict" && conflict
            ? await backendClient.aiStartConflictSuggestion(rootPath, nextRunId, conflict.relativePath, conflict.token)
            : await backendClient.aiStartCommitMessage(rootPath, nextRunId);
      reconcileAcceptance(nextRunId, accepted);
    } catch (cause) {
      const normalized = normalizeBackendError(cause);
      if (runId.value === nextRunId && terminalStatuses.has(status.value)) return;
      if (runId.value === nextRunId) {
        runId.value = undefined;
        status.value = normalized.code === "cancelled" ? "cancelled" : "failed";
        error.value = normalized;
      }
      throw normalized;
    }
  }

  function prepareRun(task: AiTaskKind, nextRunId: string): void {
    currentTask.value = task;
    runId.value = nextRunId;
    context.value = undefined;
    lastSequence.value = 0;
    completedBatchCount.value = 0;
    totalBatchCount.value = 0;
    streamedText.value = "";
    partialIssues.value = [];
    partialReviewResult.value = undefined;
    conflictResult.value = undefined;
    commitPreview.value = "";
    status.value = "starting";
    error.value = undefined;
    cancellation = undefined;
    cancelRequestedFor = undefined;
    progressMessage.value = "";
  }

  function reconcileAcceptance(
    expectedRunId: string,
    accepted: AiRunAccepted,
  ): void {
    if (accepted.runId !== expectedRunId || runId.value !== expectedRunId) {
      return;
    }
    if (terminalStatuses.has(status.value)) return;
    context.value = accepted.context;
    totalBatchCount.value = accepted.totalBatchCount;
    if (!terminalStatuses.has(status.value)) {
      status.value = "running";
    }
  }

  function handleEvent(message: AiRunEvent): void {
    if (
      message.runId !== runId.value ||
      message.sequence <= lastSequence.value ||
      terminalStatuses.has(status.value) ||
      (runRoot.value && useRepositoryStore().snapshot?.rootPath !== runRoot.value)
    ) {
      return;
    }
    lastSequence.value = message.sequence;
    const payload = message.event;
    switch (payload.kind) {
      case "started":
        context.value = payload.context;
        totalBatchCount.value = payload.totalBatchCount;
        status.value = "running";
        break;
      case "batchStarted":
        status.value = "running";
        streamedText.value = "";
        break;
      case "reviewProgress":
        progressMessage.value = payload.message;
        streamedText.value = "";
        break;
      case "delta":
        streamedText.value = (streamedText.value + payload.text).slice(-256 * 1024);
        if (currentTask.value === "generateCommitMessage") {
          commitPreview.value = (commitPreview.value + payload.text).slice(0, 256 * 1024);
        }
        break;
      case "reviewBatchCompleted":
        completedBatchCount.value = Math.max(
          completedBatchCount.value,
          payload.batchIndex,
        );
        appendUniqueIssues(payload.issues);
        if (payload.result) {
          partialReviewResult.value = payload.result;
          partialIssues.value = [...payload.result.issues];
          if (payload.result.context && context.value) context.value = { ...context.value, review: payload.result.context };
        }
        break;
      case "reviewCompleted":
        reviewResult.value = payload.result;
        if (payload.result.context && context.value) context.value = { ...context.value, review: payload.result.context };
        partialIssues.value = [...payload.result.issues];
        completedBatchCount.value = totalBatchCount.value;
        status.value = "completed";
        cancellation = undefined;
        break;
      case "conflictSuggestionCompleted":
        conflictResult.value = payload.result;
        if (context.value) context.value = { ...context.value, conflict: payload.result.context };
        completedBatchCount.value = totalBatchCount.value;
        status.value = "completed";
        cancellation = undefined;
        break;
      case "commitMessageCompleted":
        commitResult.value = payload.result;
        commitPreview.value = payload.result.message;
        completedBatchCount.value = totalBatchCount.value;
        status.value = "completed";
        cancellation = undefined;
        break;
      case "failed":
        error.value = payload.error;
        status.value = "failed";
        cancellation = undefined;
        break;
      case "cancelled":
        completedBatchCount.value = payload.completedBatchCount;
        totalBatchCount.value = payload.totalBatchCount;
        status.value = "cancelled";
        cancellation = undefined;
        break;
    }
  }

  function appendUniqueIssues(issues: AiReviewIssue[]): void {
    const keys = new Set(partialIssues.value.map(issueKey));
    for (const issue of issues) {
      const key = issueKey(issue);
      if (!keys.has(key)) {
        keys.add(key);
        partialIssues.value.push(issue);
      }
    }
  }

  function applyCommitMessage(): boolean {
    if (
      status.value !== "completed" ||
      currentTask.value !== "generateCommitMessage" ||
      !commitResult.value ||
      !isConventionalCommit(commitPreview.value)
    ) {
      return false;
    }
    useChangesStore().commitMessage = commitPreview.value.trim();
    return true;
  }

  function cancelActive(): Promise<void> {
    const activeRunId = runId.value;
    if (
      !activeRunId ||
      (status.value !== "starting" && status.value !== "running")
    ) {
      return Promise.resolve();
    }
    if (cancellation?.runId === activeRunId) {
      return cancellation.promise;
    }
    cancelRequestedFor = activeRunId;
    const promise = backendClient.aiCancel(activeRunId).then(() => {
      if (runId.value === activeRunId && running.value) status.value = "cancelled";
    }).catch((cause) => {
      const normalized = normalizeBackendError(cause);
      if (runId.value === activeRunId && running.value) error.value = normalized;
      throw normalized;
    }).finally(() => {
      if (cancellation?.runId === activeRunId) cancellation = undefined;
    });
    cancellation = { runId: activeRunId, promise };
    return promise;
  }

  async function cancelAndAbandon(): Promise<void> {
    const abandonedRunId = runId.value;
    await cancelActive();
    if (runId.value !== abandonedRunId) {
      return;
    }
    currentTask.value = undefined;
    lifecycleVersion += 1;
    reviewSource.value = undefined;
    runRoot.value = undefined;
    runGeneration.value = undefined;
    reviewResult.value = undefined;
    partialReviewResult.value = undefined;
    commitResult.value = undefined;
    conflictResult.value = undefined;
    conflictTarget.value = undefined;
    reviewSkill.value = undefined;
    reviewSkillRoot.value = undefined;
    reviewSkillDirectory.value = undefined;
    skillVersion += 1;
    skillPending = undefined;
    skillLoading.value = false;
    progressMessage.value = "";
    runId.value = undefined;
    context.value = undefined;
    lastSequence.value = 0;
    completedBatchCount.value = 0;
    totalBatchCount.value = 0;
    streamedText.value = "";
    partialIssues.value = [];
    commitPreview.value = "";
    status.value = "idle";
    error.value = undefined;
    cancellation = undefined;
  }

  function refreshReviewSkill(): Promise<void> {
    const root = useRepositoryStore().snapshot?.rootPath;
    const directory = useSettingsStore().settings.reviewSkillDirectory;
    const key = `${root ?? ""}\0${directory}\0${useRepositoryStore().generation}`;
    if (skillPending?.key === key) return skillPending.promise;
    const version = ++skillVersion;
    reviewSkill.value = undefined;
    reviewSkillRoot.value = root;
    reviewSkillDirectory.value = directory;
    if (!root) { skillLoading.value = false; return Promise.resolve(); }
    skillLoading.value = true;
    const promise = backendClient.aiReviewSkillStatus(root).then((result) => {
      if (version === skillVersion && useRepositoryStore().snapshot?.rootPath === root) reviewSkill.value = result;
    }).catch((cause) => {
      if (version === skillVersion && useRepositoryStore().snapshot?.rootPath === root) {
        reviewSkill.value = { state: "error", info: null, error: normalizeBackendError(cause) };
      }
    }).finally(() => {
      if (version === skillVersion) { skillLoading.value = false; skillPending = undefined; }
    });
    skillPending = { key, promise };
    return promise;
  }

  async function testConnection(
    config: AiConnectionConfig,
  ): Promise<AiConnectionTestResult> {
    connectionStatus.value = "testing";
    connectionResult.value = undefined;
    connectionError.value = undefined;
    try {
      const result = await backendClient.aiTestConnection({ ...config });
      connectionResult.value = result;
      connectionStatus.value = "success";
      return result;
    } catch (cause) {
      const normalized = normalizeBackendError(cause);
      connectionError.value = normalized;
      connectionStatus.value = "failed";
      throw normalized;
    }
  }

  return {
    currentTask,
    runId,
    context,
    lastSequence,
    completedBatchCount,
    totalBatchCount,
    streamedText,
    partialIssues,
    reviewResult,
    partialReviewResult,
    commitPreview,
    commitResult,
    conflictResult,
    conflictTarget,
    status,
    error,
    connectionStatus,
    connectionResult,
    connectionError,
    reviewSource,
    runRoot,
    runGeneration,
    reviewSkill,
    reviewSkillRoot,
    reviewSkillDirectory,
    skillLoading,
    progressMessage,
    running,
    refreshReviewSkill,
    initialize,
    dispose,
    startReview,
    startCommitMessage,
    startConflictSuggestion,
    handleEvent,
    applyCommitMessage,
    cancelActive,
    cancelAndAbandon,
    testConnection,
  };
});

function issueKey(issue: AiReviewIssue): string {
  return [
    issue.severity,
    issue.path,
    issue.startLine ?? "",
    issue.endLine ?? "",
    issue.reason.trim(),
  ].join("\u0000");
}

export function isConventionalCommit(message: string): boolean {
  const subject = message.trim().split(/\r?\n/, 1)[0] ?? "";
  return (
    /^[\x00-\x7F]+$/.test(subject) &&
    /^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([^)]+\))?: \S.*$/.test(
      subject,
    )
  );
}
