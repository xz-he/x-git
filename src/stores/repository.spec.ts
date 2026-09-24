import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { RepositorySnapshot } from "@/lib/backend/types";
import { useRepositoryStore } from "@/stores/repository";
import { useAiStore } from "@/stores/ai";
import { useHistoryStore } from "@/stores/history";
import { useOperationStore } from "@/stores/operation";
import { useRefsStore } from "@/stores/refs";
import { useRemotesStore } from "@/stores/remotes";
import { useStashesStore } from "@/stores/stashes";
import { useChangesStore } from "@/stores/changes";
import { createBackendFixture } from "@/test/backend";
import { flushPromises } from "@vue/test-utils";

function repositoryFixture(
  overrides: Partial<RepositorySnapshot> = {},
): RepositorySnapshot {
  return {
    rootPath: "D:\\work\\repo",
    name: "repo",
    currentBranch: "main",
    headShortHash: "abc1234",
    isClean: true,
    changedFileCount: 0,
    conflictCount: 0,
    remotes: [],
    upstream: null,
    ...overrides,
  };
}

describe("repository store", () => {
  let backend: BackendClient;

  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture();
    setBackendClientForTests(backend);
  });

  it("unlocks after the new branch workspace is ready while history refresh is still pending", async () => {
    const repositories = useRepositoryStore();
    repositories.snapshot = repositoryFixture();
    const history = useHistoryStore();
    vi.mocked(backend.historyPage).mockResolvedValueOnce({ commits: [], nextCursor: null, queryFingerprint: "main", continuationLanes: [] });
    await history.ensureLoaded(repositories.snapshot.rootPath, repositories.generation);
    let finishHistory!: (value: Awaited<ReturnType<BackendClient["historyPage"]>>) => void;
    vi.mocked(backend.historyPage).mockReturnValueOnce(new Promise(resolve => { finishHistory = resolve; }));
    vi.mocked(backend.repositoryRefresh).mockResolvedValue(repositoryFixture({ currentBranch: "topic", headShortHash: "bbbbbbb" }));
    const refreshing = repositories.refresh({ background: true });
    await flushPromises();
    const busyWithNewBranch = repositories.navigationBusy;
    const viewBusyWithNewBranch = repositories.viewNavigationBusy;
    expect(repositories.snapshot.currentBranch).toBe("topic");
    expect(history.loading).toBe(true);
    finishHistory({ commits: [], nextCursor: null, queryFingerprint: "topic", continuationLanes: [] });
    await refreshing;
    expect(busyWithNewBranch).toBe(false);
    expect(viewBusyWithNewBranch).toBe(false);
  });

  it("refreshes cached history after an external branch switch", async () => {
    const repositories = useRepositoryStore();
    repositories.snapshot = repositoryFixture();
    const history = useHistoryStore();
    vi.mocked(backend.historyPage).mockResolvedValueOnce({ commits: [], nextCursor: null, queryFingerprint: "old-branch", continuationLanes: [] });
    await history.ensureLoaded(repositories.snapshot.rootPath, repositories.generation);
    vi.mocked(backend.repositoryRefresh).mockResolvedValue(repositoryFixture({ currentBranch: "feature/dev", headShortHash: "def5678" }));
    vi.mocked(backend.historyPage).mockResolvedValue({ commits: [], nextCursor: null, queryFingerprint: "new-branch", continuationLanes: [] });
    await repositories.refresh();
    expect(repositories.snapshot.currentBranch).toBe("feature/dev");
    expect(history.queryFingerprint).toBe("new-branch");
    expect(backend.historyPage).toHaveBeenCalledTimes(2);
  });

  it("keeps actions locked until the critical workspace refresh completes", async () => {
    const repositories = useRepositoryStore();
    repositories.snapshot = repositoryFixture();
    let finish!: (value: RepositorySnapshot) => void;
    vi.mocked(backend.repositoryRefresh).mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
    const pending = repositories.refresh();
    await flushPromises();
    expect(repositories.navigationBusy).toBe(true);
    finish(repositoryFixture({ currentBranch: "topic" }));
    await pending;
    expect(repositories.navigationBusy).toBe(false);
  });

  it("discards delayed metadata after another branch switch and allows history to retry", async () => {
    const repositories = useRepositoryStore();
    repositories.snapshot = repositoryFixture();
    const root = repositories.snapshot.rootPath;
    const refs = useRefsStore(), remotes = useRemotesStore(), history = useHistoryStore();
    await Promise.all([refs.ensureLoaded(root, 0), remotes.ensureLoaded(root, 0), history.ensureLoaded(root, 0)]);
    let finishRefs!: (value: Awaited<ReturnType<BackendClient["refsSnapshot"]>>) => void;
    let finishRemotes!: (value: Awaited<ReturnType<BackendClient["remotesSnapshot"]>>) => void;
    let finishHistory!: (value: Awaited<ReturnType<BackendClient["historyPage"]>>) => void;
    const oldRefs = refs.snapshot!, oldRemotes = remotes.snapshot!;
    vi.mocked(backend.refsSnapshot).mockReturnValueOnce(new Promise(resolve => { finishRefs = resolve; }));
    vi.mocked(backend.remotesSnapshot).mockReturnValueOnce(new Promise(resolve => { finishRemotes = resolve; }));
    vi.mocked(backend.historyPage).mockReturnValueOnce(new Promise(resolve => { finishHistory = resolve; }));
    vi.mocked(backend.repositoryRefresh).mockResolvedValue(repositoryFixture({ currentBranch: "topic" }));
    const pending = repositories.refresh();
    await flushPromises();
    expect(repositories.navigationBusy).toBe(false);
    await repositories.refresh();
    expect(backend.repositoryRefresh).toHaveBeenCalledTimes(1);
    const newerRefs = { localBranches: [], remoteBranches: [], tags: [] };
    vi.mocked(backend.refsSwitch).mockResolvedValueOnce({
      workspace: { repository: repositoryFixture({ currentBranch: "newer" }), changes: { files: [], stagedCount: 0, unstagedCount: 0 } },
      refs: newerRefs, operationState: { kind: "none", conflicts: [], abortAction: null },
    });
    const switching = refs.switchBranch("newer");
    expect(repositories.navigationBusy).toBe(true);
    await switching;
    const newerRemotes = { ...oldRemotes };
    remotes.applySnapshot(newerRemotes);
    const retainedRemotes = remotes.snapshot;
    finishRefs(oldRefs); finishRemotes(oldRemotes);
    finishHistory({ commits: [], nextCursor: null, queryFingerprint: "stale", continuationLanes: [] });
    await pending;
    expect(repositories.snapshot.currentBranch).toBe("newer");
    expect(refs.snapshot).toEqual(newerRefs);
    expect(remotes.snapshot).toBe(retainedRemotes);
    expect(history.queryFingerprint).not.toBe("stale");
    vi.mocked(backend.historyPage).mockResolvedValueOnce({ commits: [], nextCursor: null, queryFingerprint: "newer", continuationLanes: [] });
    await history.refresh();
    expect(history.queryFingerprint).toBe("newer");
    expect(repositories.refreshingModules).toBe(false);
  });

  it("ignores old metadata failures when another repository is opened", async () => {
    const repositories = useRepositoryStore();
    repositories.snapshot = repositoryFixture();
    await useRefsStore().ensureLoaded(repositories.snapshot.rootPath, 0);
    let fail!: (reason: unknown) => void;
    vi.mocked(backend.refsSnapshot).mockReturnValueOnce(new Promise((_, reject) => { fail = reject; }));
    vi.mocked(backend.repositoryRefresh).mockResolvedValue(repositoryFixture());
    const oldRefresh = repositories.refresh();
    await flushPromises();
    const next = repositoryFixture({ rootPath: "C:/other" });
    vi.mocked(backend.repositoryOpen).mockResolvedValue(next);
    await repositories.open(next.rootPath);
    fail({ code: "gitCommandFailed", message: "old failure" });
    await oldRefresh;
    expect(repositories.snapshot.rootPath).toBe(next.rootPath);
    expect(repositories.error).toBeUndefined();
    expect(useRefsStore().snapshot).toBeUndefined();
    expect(repositories.refreshingModules).toBe(false);
  });

  it("replaces repository state with one coherent snapshot", async () => {
    const snapshot = repositoryFixture();
    vi.mocked(backend.repositoryOpen).mockResolvedValue(snapshot);
    const store = useRepositoryStore();

    await store.open("D:\\work\\repo");

    expect(store.snapshot).toEqual(snapshot);
    expect(store.operation).toEqual({ kind: "idle" });
  });

  it("clears Stash and other lazy state before replacement Changes resolves", async () => {
    const next = repositoryFixture();
    vi.mocked(backend.repositoryOpen).mockResolvedValue(next);
    let finish!: (value: { files: []; stagedCount: number; unstagedCount: number }) => void;
    vi.mocked(backend.changesSnapshot).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    useStashesStore().snapshot = { entries: [] };
    useStashesStore().message = "old draft";
    useOperationStore().state = { kind: "none", conflicts: [{ path: "old", status: "UU" }], abortAction: null };
    const opening = useRepositoryStore().open(next.rootPath);
    await vi.waitFor(() => expect(backend.changesSnapshot).toHaveBeenCalled());
    expect(useStashesStore().snapshot).toBeUndefined();
    expect(useStashesStore().message).toBe("");
    expect(useStashesStore().loadedRootPath).toBe(next.rootPath);
    expect(useRefsStore().snapshot).toBeUndefined();
    expect(useHistoryStore().commits).toEqual([]);
    expect(useRemotesStore().snapshot).toBeUndefined();
    expect(useOperationStore().state.conflicts).toEqual([]);
    expect(backend.stashSnapshot).not.toHaveBeenCalled();
    finish({ files: [], stagedCount: 0, unstagedCount: 0 });
    await opening;
  });

  it("retains the previous snapshot and exposes a normalized error", async () => {
    const previous = repositoryFixture({ name: "previous" });
    const store = useRepositoryStore();
    store.snapshot = previous;
    vi.mocked(backend.repositoryOpen).mockRejectedValue({
      code: "invalidRepository",
      message: "不是 Git 仓库。",
    });

    await expect(store.open("D:\\invalid")).rejects.toMatchObject({
      code: "invalidRepository",
    });

    expect(store.snapshot).toEqual(previous);
    expect(store.error).toEqual({
      code: "invalidRepository",
      message: "不是 Git 仓库。",
    });
    expect(store.operation).toEqual({ kind: "idle" });
  });

  it("keeps the newly opened identity coherent when loading its Changes fails", async () => {
    const store = useRepositoryStore();
    store.snapshot = repositoryFixture({ rootPath: "C:/old" });
    useChangesStore().selectedPath = "old.txt";
    const next = repositoryFixture({ rootPath: "C:/new" });
    vi.mocked(backend.repositoryOpen).mockResolvedValue(next);
    vi.mocked(backend.changesSnapshot).mockRejectedValue({ code: "gitLocked", message: "locked" });
    await expect(store.open(next.rootPath)).rejects.toMatchObject({ code: "gitLocked" });
    expect(store.snapshot?.rootPath).toBe(next.rootPath);
    expect(useChangesStore().selectedPath).toBeUndefined();
    expect(useChangesStore().snapshot?.files).toEqual([]);
    expect(useStashesStore().loadedRootPath).toBe(next.rootPath);
  });

  it("cancels active AI after successful open and before replacing state, but not refresh", async () => {
    const next = repositoryFixture();
    vi.mocked(backend.repositoryOpen).mockResolvedValue(next);
    vi.mocked(backend.repositoryInit).mockResolvedValue(next);
    vi.mocked(backend.repositoryClone).mockResolvedValue(next);
    vi.mocked(backend.repositoryRefresh).mockResolvedValue(next);
    vi.mocked(backend.changesSnapshot).mockResolvedValue({
      files: [],
      stagedCount: 0,
      unstagedCount: 0,
    });
    const store = useRepositoryStore();
    const ai = useAiStore();

    for (const [runId, action, repositoryMethod] of [
      ["open-run", () => store.open("D:\\work\\repo"), backend.repositoryOpen],
      ["init-run", () => store.init("D:\\work\\repo"), backend.repositoryInit],
      [
        "clone-run",
        () => store.clone("https://example.test/repo.git", "D:\\work\\repo"),
        backend.repositoryClone,
      ],
    ] as const) {
      ai.runId = runId;
      ai.status = "running";
      await action();
      const cancelOrder = vi.mocked(backend.aiCancel).mock.invocationCallOrder.at(-1)!;
      const operationOrder = vi.mocked(repositoryMethod).mock.invocationCallOrder.at(-1)!;
      expect(cancelOrder).toBeGreaterThan(operationOrder);
      expect(ai.runId).toBeUndefined();
    }

    ai.runId = "refresh-run";
    ai.status = "running";
    store.snapshot = next;
    const cancelCount = vi.mocked(backend.aiCancel).mock.calls.length;
    await store.refresh();
    expect(backend.aiCancel).toHaveBeenCalledTimes(cancelCount);
    expect(ai.runId).toBe("refresh-run");
  });

  it("abandons active remote synchronization before repository replacement", async () => {
    const next = repositoryFixture();
    vi.mocked(backend.repositoryOpen).mockResolvedValue(next);
    const remotes = useRemotesStore();
    remotes.runId = "git-run";
    remotes.status = "running";

    await useRepositoryStore().open(next.rootPath);

    expect(backend.gitRunCancel).toHaveBeenCalledWith("git-run");
    expect(remotes.runId).toBeUndefined();
    expect(remotes.selectedRemoteName).toBeUndefined();
    expect(remotes.loadedRootPath).toBe(next.rootPath);
  });

  it("keeps the AI review running when a requested repository fails to open", async () => {
    const store = useRepositoryStore();
    store.snapshot = repositoryFixture();
    const ai = useAiStore();
    ai.runId = "retained-review";
    ai.runRoot = store.snapshot.rootPath;
    ai.status = "running";
    ai.reviewSource = { kind: "commit", revision: "a".repeat(40) };
    vi.mocked(backend.repositoryOpen).mockRejectedValue({ code: "invalidRepository", message: "missing" });
    await expect(store.open("D:/missing")).rejects.toMatchObject({ code: "invalidRepository" });
    expect(backend.aiCancel).not.toHaveBeenCalled();
    expect(ai.status).toBe("running");
    ai.handleEvent({ runId: "retained-review", sequence: 1, event: { kind: "reviewProgress", phase: "evidence", message: "读取定义" } });
    expect(ai.progressMessage).toBe("读取定义");
  });

  it("increments generation for replacement attempts but not refresh", async () => {
    const next = repositoryFixture();
    vi.mocked(backend.repositoryOpen).mockResolvedValue(next);
    vi.mocked(backend.repositoryRefresh).mockResolvedValue(next);
    const store = useRepositoryStore();

    await store.open(next.rootPath);
    expect(store.generation).toBe(1);

    await store.refresh();
    expect(store.generation).toBe(1);
  });

  it("resets lazy modules on replacement without loading them or clearing them on refresh", async () => {
    const next = repositoryFixture();
    vi.mocked(backend.repositoryOpen).mockResolvedValue(next);
    vi.mocked(backend.repositoryRefresh).mockResolvedValue(next);
    const store = useRepositoryStore();

    await store.open(next.rootPath);

    expect(useRefsStore().loadedRootPath).toBe(next.rootPath);
    expect(useHistoryStore().loadedRootPath).toBe(next.rootPath);
    expect(backend.refsSnapshot).not.toHaveBeenCalled();
    expect(backend.historyPage).not.toHaveBeenCalled();

    useRefsStore().selectedFullName = "refs/heads/main";
    useHistoryStore().selectedHash = "a".repeat(40);
    await store.refresh();

    expect(useRefsStore().selectedFullName).toBe("refs/heads/main");
    expect(useHistoryStore().selectedHash).toBe("a".repeat(40));
  });

  it("clears operation state only after a repository replacement succeeds", async () => {
    const next = repositoryFixture();
    vi.mocked(backend.repositoryOpen).mockResolvedValue(next);
    const operation = useOperationStore();
    operation.state = {
      kind: "merge",
      conflicts: [{ path: "README.md", status: "UU" }],
      abortAction: "merge",
    };

    await useRepositoryStore().open(next.rootPath);

    expect(operation.state).toEqual({
      kind: "none",
      conflicts: [],
      abortAction: null,
    });
  });
});
