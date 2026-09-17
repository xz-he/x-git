import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import type {
  AiConnectionConfig,
  AiRunAccepted,
  AiRunEvent,
  RepositorySnapshot,
} from "@/lib/backend/types";
import { useAiStore } from "@/stores/ai";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { createBackendFixture } from "@/test/backend";

const repository: RepositorySnapshot = {
  rootPath: "D:\\work\\repo",
  name: "repo",
  currentBranch: "main",
  headShortHash: "abc1234",
  isClean: false,
  changedFileCount: 1,
  conflictCount: 0,
  remotes: [],
  upstream: null,
};

const context = {
  stagedFileCount: 1,
  textFileCount: 1,
  skippedBinaryFiles: [],
  fingerprint: "fingerprint",
};

function accepted(
  runId: string,
  task: AiRunAccepted["task"] = "reviewChanges",
): AiRunAccepted {
  return { runId, task, context, totalBatchCount: 2 };
}

function event(
  runId: string,
  sequence: number,
  payload: AiRunEvent["event"],
): AiRunEvent {
  return { runId, sequence, event: payload };
}

describe("AI store", () => {
  let backend: BackendClient;
  let emit: (event: AiRunEvent) => void;
  let unlisten: () => void;

  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture();
    unlisten = vi.fn<() => void>();
    vi.mocked(backend.aiListen).mockImplementation(async (listener) => {
      emit = listener;
      return unlisten;
    });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = repository;
    vi.stubGlobal("crypto", {
      randomUUID: vi.fn(() => "11111111-1111-4111-8111-111111111111"),
    });
  });

  it("subscribes once and disposes the listener", async () => {
    const store = useAiStore();

    await store.initialize();
    await store.initialize();
    store.dispose();

    expect(backend.aiListen).toHaveBeenCalledTimes(1);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("keeps an early event delivered before start acceptance", async () => {
    const store = useAiStore();
    await store.initialize();
    vi.mocked(backend.aiStartReview).mockImplementation(async (_path, runId) => {
      emit(event(runId, 2, { kind: "delta", text: "early" }));
      return accepted(runId);
    });

    await store.startReview();

    expect(store.runId).toBe("11111111-1111-4111-8111-111111111111");
    expect(store.streamedText).toBe("early");
    expect(store.lastSequence).toBe(2);
  });

  it("ignores duplicate sequence numbers and stale run events", async () => {
    const store = useAiStore();
    await store.initialize();
    vi.mocked(backend.aiStartReview).mockResolvedValue(
      accepted("11111111-1111-4111-8111-111111111111"),
    );
    await store.startReview();

    emit(event(store.runId!, 2, { kind: "delta", text: "new" }));
    emit(event(store.runId!, 1, { kind: "delta", text: "old" }));
    emit(event("other", 3, { kind: "delta", text: "stale" }));

    expect(store.streamedText).toBe("new");
  });

  it("retains partial review issues after cancellation", () => {
    const store = useAiStore();
    store.runId = "run-1";
    store.currentTask = "reviewChanges";
    store.status = "running";
    emit = store.handleEvent;
    emit(
      event("run-1", 1, {
        kind: "reviewBatchCompleted",
        batchIndex: 1,
        issues: [
          {
            severity: "warning",
            path: "src/main.ts",
            startLine: 8,
            endLine: 8,
            reason: "reason",
            suggestedFix: "fix",
          },
        ],
      }),
    );
    emit(
      event("run-1", 2, {
        kind: "cancelled",
        completedBatchCount: 1,
        totalBatchCount: 2,
      }),
    );

    expect(store.status).toBe("cancelled");
    expect(store.completedBatchCount).toBe(1);
    expect(store.partialIssues).toHaveLength(1);
  });

  it("applies only a successfully completed valid commit preview", () => {
    const store = useAiStore();
    store.runId = "run-1";
    store.currentTask = "generateCommitMessage";
    store.status = "running";
    store.handleEvent(
      event("run-1", 1, {
        kind: "commitMessageCompleted",
        result: {
          message: "feat(ui): add review",
          contextFingerprint: "fingerprint",
        },
      }),
    );
    store.commitPreview = "invalid";
    expect(store.applyCommitMessage()).toBe(false);
    store.commitPreview = "feat(ui): edit review";

    expect(store.applyCommitMessage()).toBe(true);
    expect(useChangesStore().commitMessage).toBe("feat(ui): edit review");
  });

  it("coalesces repeated cancellation requests", async () => {
    const store = useAiStore();
    store.runId = "run-1";
    store.status = "running";

    await Promise.all([store.cancelActive(), store.cancelActive()]);

    expect(backend.aiCancel).toHaveBeenCalledTimes(1);
  });

  it("normalizes start failures without discarding the last success", async () => {
    const store = useAiStore();
    store.reviewResult = {
      summary: "previous",
      issues: [],
      reviewedFiles: [],
      skippedBinaryFiles: [],
      warnings: [],
    };
    vi.mocked(backend.aiStartReview).mockRejectedValue({
      code: "aiTransport",
      message: "连接失败。",
    });

    await expect(store.startReview()).rejects.toMatchObject({
      code: "aiTransport",
    });

    expect(store.status).toBe("failed");
    expect(store.runId).toBeUndefined();
    expect(store.reviewResult?.summary).toBe("previous");
  });

  it("tracks connection-test success and failure independently", async () => {
    const store = useAiStore();
    const config: AiConnectionConfig = {
      provider: "custom",
      apiKey: "test-key",
      baseUrl: "http://127.0.0.1:3000/v1",
      model: "test-model",
    };
    vi.mocked(backend.aiTestConnection).mockResolvedValue({
      provider: "custom",
      model: "test-model",
      message: "连接成功。",
    });

    await store.testConnection(config);
    expect(store.connectionStatus).toBe("success");
    expect(store.connectionResult?.model).toBe("test-model");

    vi.mocked(backend.aiTestConnection).mockRejectedValue({
      code: "aiAuthentication",
      message: "认证失败。",
    });
    await expect(store.testConnection(config)).rejects.toMatchObject({
      code: "aiAuthentication",
    });
    expect(store.connectionStatus).toBe("failed");
  });
});
