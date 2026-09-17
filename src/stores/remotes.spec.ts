import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type {
  GitRunEvent,
  RemoteOperationResult,
  RemoteSnapshot,
  RepositorySnapshot,
} from "@/lib/backend/types";
import { useOperationStore } from "@/stores/operation";
import { useRefsStore } from "@/stores/refs";
import { useRemotesStore } from "@/stores/remotes";
import { useRepositoryStore } from "@/stores/repository";
import { createBackendFixture } from "@/test/backend";
import { gitFeedback } from "@/lib/gitFeedback";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

function repository(): RepositorySnapshot {
  return {
    rootPath: "C:/repo",
    name: "repo",
    currentBranch: "main",
    headShortHash: "abc1234",
    isClean: true,
    changedFileCount: 0,
    conflictCount: 0,
    remotes: [{ name: "origin", fetchUrl: "C:/origin.git" }],
    upstream: null,
  };
}

function remoteSnapshot(oid = "a".repeat(40)): RemoteSnapshot {
  return {
    remotes: [
      {
        name: "origin",
        fetchUrl: "C:/origin.git",
        pushUrl: "C:/origin.git",
        branches: [
          {
            name: "main",
            fullName: "refs/remotes/origin/main",
            objectId: oid,
            trackingLocal: "main",
            ahead: 0,
            behind: 0,
          },
        ],
      },
    ],
  };
}

function result(oid = "b".repeat(40)): RemoteOperationResult {
  return {
    workspace: {
      repository: { ...repository(), headShortHash: oid.slice(0, 7) },
      changes: { files: [], stagedCount: 0, unstagedCount: 0 },
    },
    refs: { localBranches: [], remoteBranches: [], tags: [] },
    remotes: remoteSnapshot(oid),
    operationState: { kind: "none", conflicts: [], abortAction: null },
  };
}

function event(
  runId: string,
  sequence: number,
  payload: GitRunEvent["event"],
): GitRunEvent {
  return { runId, sequence, event: payload };
}

describe("remotes store", () => {
  let backend: BackendClient;
  let emit: ((event: GitRunEvent) => void) | undefined;

  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture({
      remotesSnapshot: vi.fn(async () => remoteSnapshot()),
      gitRunListen: vi.fn(async (listener) => {
        emit = listener;
        return () => undefined;
      }),
      remoteStartFetch: vi.fn(async (_path, runId) => ({
        runId,
        operation: "fetch" as const,
      })),
    });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = repository();
    useRepositoryStore().generation = 1;
  });

  it("loads once per repository generation and selects the first remote", async () => {
    const store = useRemotesStore();

    await store.ensureLoaded("C:/repo", 1);
    await store.ensureLoaded("C:/repo", 1);

    expect(backend.remotesSnapshot).toHaveBeenCalledTimes(1);
    expect(store.selectedRemoteName).toBe("origin");
  });

  it("rejects a remote snapshot from a replaced repository generation", async () => {
    const first = deferred<RemoteSnapshot>();
    vi.mocked(backend.remotesSnapshot).mockReturnValueOnce(first.promise);
    const store = useRemotesStore();
    const pending = store.ensureLoaded("C:/repo", 1);

    store.resetForRepository("C:/other", 2);
    first.resolve(remoteSnapshot());
    await pending;

    expect(store.snapshot).toBeUndefined();
    expect(store.loadedRootPath).toBe("C:/other");
  });

  it("ignores wrong-run and out-of-order events", async () => {
    const store = useRemotesStore();
    await store.initialize();
    await store.fetch("origin");
    const runId = store.runId!;

    emit?.(event("other", 99, { kind: "completed", result: result() }));
    emit?.(
      event(runId, 2, {
        kind: "progress",
        phase: "receiving",
        text: "50%",
      }),
    );
    emit?.(event(runId, 1, { kind: "started", operation: "fetch" }));

    expect(store.progress?.text).toBe("50%");
    expect(store.lastSequence).toBe(2);
    expect(store.status).toBe("running");
  });

  it("requests cancellation once and waits for the terminal event", async () => {
    const store = useRemotesStore();
    await store.initialize();
    await store.fetch("origin");
    const runId = store.runId!;

    await Promise.all([store.cancel(), store.cancel()]);

    expect(backend.gitRunCancel).toHaveBeenCalledTimes(1);
    expect(store.running).toBe(true);
    expect(store.cancelRequested).toBe(true);

    emit?.(event(runId, 2, { kind: "cancelled", result: result() }));
    expect(store.running).toBe(false);
    expect(store.status).toBe("cancelled");
  });

  it("atomically applies an authoritative completed result", async () => {
    const store = useRemotesStore();
    await store.initialize();
    await store.fetch("origin");
    const runId = store.runId!;
    const completed = result("c".repeat(40));

    emit?.(event(runId, 2, { kind: "completed", result: completed }));

    expect(store.snapshot).toEqual(completed.remotes);
    expect(useRepositoryStore().snapshot).toEqual(completed.workspace.repository);
    expect(useRefsStore().snapshot).toEqual(completed.refs);
    expect(useOperationStore().state).toEqual(completed.operationState);
    expect(store.status).toBe("completed");
  });

  it("applies authoritative conflicted and failed results", async () => {
    const store = useRemotesStore();
    await store.initialize();
    await store.fetch("origin");
    const conflictRun = store.runId!;
    const conflictResult = result("d".repeat(40));
    conflictResult.operationState = {
      kind: "none",
      conflicts: [{ path: "README.md", status: "UU" }],
      abortAction: null,
    };
    emit?.(
      event(conflictRun, 2, {
        kind: "conflicted",
        result: conflictResult,
      }),
    );
    expect(store.status).toBe("conflicted");
    expect(useOperationStore().isBlocked).toBe(true);

    await store.fetch("origin");
    const failedRun = store.runId!;
    const failedResult = result("e".repeat(40));
    emit?.(
      event(failedRun, 2, {
        kind: "failed",
        error: { code: "gitNetwork", message: "网络失败。" },
        result: failedResult,
      }),
    );
    expect(store.status).toBe("failed");
    expect(store.error?.code).toBe("gitNetwork");
    expect(store.snapshot).toEqual(failedResult.remotes);
  });

  it("ignores terminal events after repository ownership changes", async () => {
    const store = useRemotesStore();
    await store.initialize();
    await store.fetch("origin");
    const oldRun = store.runId!;
    store.resetForRepository("C:/other", 2);

    emit?.(event(oldRun, 2, { kind: "completed", result: result() }));

    expect(store.snapshot).toBeUndefined();
    expect(store.runId).toBeUndefined();
  });

  it("shows an explicit failure if the progress listener cannot be attached", async () => {
    vi.mocked(backend.gitRunListen).mockRejectedValue({ code: "io", message: "无法订阅 Git 进度" });
    const store = useRemotesStore();
    await expect(store.fetch("origin")).rejects.toMatchObject({ code: "io" });
    expect(backend.remoteStartFetch).not.toHaveBeenCalled();
    expect(gitFeedback.value).toHaveLength(1);
    expect(gitFeedback.value[0]).toMatchObject({ status: "failed", message: "无法订阅 Git 进度", target: "远程：origin" });
  });
});
