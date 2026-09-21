import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import type {
  CommitSummary,
  CommitDetail,
  FileDiff,
  HistoryMutationResult,
  HistoryPage,
} from "@/lib/backend/types";
import { useHistoryStore } from "@/stores/history";
import { useOperationStore } from "@/stores/operation";
import { useRepositoryStore } from "@/stores/repository";
import { createBackendFixture } from "@/test/backend";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function commit(index: number): CommitSummary {
  const hash = index.toString(16).padStart(40, "0");
  return {
    hash,
    shortHash: hash.slice(0, 7),
    parentHashes: [],
    subject: `commit ${index}`,
    authorName: "HQ Test",
    authorEmail: "hq@example.test",
    authoredAt: "2026-09-08T10:00:00+08:00",
    references: [],
    topology: { lane: 0, parents: [] },
  };
}

function page(
  commits: CommitSummary[],
  nextCursor: string | null,
): HistoryPage {
  return {
    commits,
    nextCursor,
    queryFingerprint: "query-1",
    continuationLanes: [],
  };
}

describe("history store", () => {
  let backend: BackendClient;

  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture();
    setBackendClientForTests(backend);
    vi.mocked(backend.historyDetail).mockImplementation(async (_root, hash) => ({
      hash, shortHash: hash.slice(0, 7), parentHashes: [], message: "fixture",
      authorName: "Test", authorEmail: "test@example.test", authoredAt: "",
      committerName: "Test", committerEmail: "test@example.test", committedAt: "",
      references: [], files: [],
    }));
    vi.mocked(backend.historyFileDiff).mockResolvedValue({ path: "new.ts", binary: false, hunks: [] } as unknown as FileDiff);
  });

  it("appends pages without moving selection", async () => {
    vi.mocked(backend.historyPage)
      .mockResolvedValueOnce(page(Array.from({ length: 200 }, (_, i) => commit(i)), "next"))
      .mockResolvedValueOnce(page(Array.from({ length: 5 }, (_, i) => commit(200 + i)), null));
    const store = useHistoryStore();

    await store.ensureLoaded("C:/repo", 1);
    store.selectedHash = store.commits[0]!.hash;
    await store.loadNextPage();

    expect(store.commits).toHaveLength(205);
    expect(store.selectedHash).toBe(commit(0).hash);
    expect(backend.historyPage).toHaveBeenLastCalledWith("C:/repo", {
      reference: null,
      search: "",
      cursor: "next",
    });
  });

  it("keeps loaded pages and selection when background reconciliation has no new commits", async () => {
    const firstPage = page(Array.from({ length: 200 }, (_, i) => commit(i)), "next");
    vi.mocked(backend.historyPage)
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce(page(Array.from({ length: 5 }, (_, i) => commit(200 + i)), "tail"))
      .mockResolvedValueOnce(firstPage);
    const store = useHistoryStore();
    await store.ensureLoaded("C:/repo", 1);
    await store.loadNextPage();
    await store.selectCommit(commit(204).hash);
    await store.refresh();
    expect(store.commits).toHaveLength(205);
    expect(store.nextCursor).toBe("tail");
    expect(store.selectedHash).toBe(commit(204).hash);
    expect(store.detail?.hash).toBe(commit(204).hash);
  });

  it("immediately shows loading and coalesces repeated clicks on the pending commit", async () => {
    const pendingDetail = deferred<CommitDetail>();
    vi.mocked(backend.historyDetail).mockReturnValueOnce(pendingDetail.promise);
    const store = useHistoryStore();
    store.resetForRepository("C:/repo", 1);
    const first = store.selectCommit(commit(1).hash);
    const second = store.selectCommit(commit(1).hash);
    expect(store.selectedHash).toBe(commit(1).hash);
    expect(store.detailLoading).toBe(true);
    expect(backend.historyDetail).toHaveBeenCalledTimes(1);
    pendingDetail.resolve({ ...commit(1), message: "loaded", committerName: "Test", committerEmail: "", committedAt: "", files: [] });
    await Promise.all([first, second]);
    expect(store.detailLoading).toBe(false);
    expect(store.detail?.message).toBe("loaded");
  });

  it("does not let an earlier detail completion clear the newer loading state", async () => {
    const firstDetail = deferred<CommitDetail>();
    const secondDetail = deferred<CommitDetail>();
    vi.mocked(backend.historyDetail).mockReturnValueOnce(firstDetail.promise).mockReturnValueOnce(secondDetail.promise);
    const store = useHistoryStore();
    store.resetForRepository("C:/repo", 1);
    const first = store.selectCommit(commit(1).hash);
    const second = store.selectCommit(commit(2).hash);
    firstDetail.resolve({ ...commit(1), message: "old", committerName: "Test", committerEmail: "", committedAt: "", files: [] });
    await first;
    expect(store.selectedHash).toBe(commit(2).hash);
    expect(store.detail).toBeUndefined();
    expect(store.detailLoading).toBe(true);
    secondDetail.resolve({ ...commit(2), message: "new", committerName: "Test", committerEmail: "", committedAt: "", files: [] });
    await second;
    expect(store.detail?.message).toBe("new");
    expect(store.detailLoading).toBe(false);
  });

  it("completes a pending detail while another history page loads", async () => {
    const pendingDetail = deferred<CommitDetail>();
    vi.mocked(backend.historyDetail).mockReturnValueOnce(pendingDetail.promise);
    vi.mocked(backend.historyPage)
      .mockResolvedValueOnce(page([commit(1)], "next"))
      .mockResolvedValueOnce(page([commit(2)], null));
    const store = useHistoryStore();
    await store.ensureLoaded("C:/repo", 1);
    const pending = store.selectCommit(commit(1).hash);
    await store.loadNextPage();
    pendingDetail.resolve({ ...commit(1), message: "loaded", committerName: "Test", committerEmail: "", committedAt: "", files: [] });
    await pending;
    expect(store.detail?.message).toBe("loaded");
    expect(store.detailLoading).toBe(false);
  });

  it("discards pending details when the query or repository changes", async () => {
    for (const replace of ["query", "repository", "mutation"]) {
      const pendingDetail = deferred<CommitDetail>();
      vi.mocked(backend.historyDetail).mockReturnValueOnce(pendingDetail.promise);
      vi.mocked(backend.historyPage).mockResolvedValue(page([], null));
      const store = useHistoryStore();
      store.resetForRepository("C:/repo", 1);
      const pending = store.selectCommit(commit(1).hash);
      if (replace === "query") await store.setReference("other");
      else if (replace === "repository") store.resetForRepository("C:/other", 2);
      else store.applyPage(page([], null), "C:/repo");
      pendingDetail.resolve({ ...commit(1), message: "stale", committerName: "Test", committerEmail: "", committedAt: "", files: [] });
      await pending;
      expect(store.detail).toBeUndefined();
      expect(store.detailLoading).toBe(false);
    }
  });

  it("clears failed pending requests and allows retrying the same commit", async () => {
    vi.mocked(backend.historyDetail).mockRejectedValueOnce({ code: "gitCommandFailed", message: "read failed" });
    const store = useHistoryStore();
    store.resetForRepository("C:/repo", 1);
    await expect(store.selectCommit(commit(1).hash)).rejects.toMatchObject({ message: "read failed" });
    expect(store.detailLoading).toBe(false);
    expect(store.error?.message).toBe("read failed");
    await store.selectCommit(commit(1).hash);
    expect(store.detail?.hash).toBe(commit(1).hash);
    expect(store.error).toBeUndefined();
  });

  it("discards an old file response after selecting another commit", async () => {
    const pendingDiff = deferred<FileDiff>();
    vi.mocked(backend.historyFileDiff).mockReturnValueOnce(pendingDiff.promise);
    const store = useHistoryStore();
    store.resetForRepository("C:/repo", 1);
    await store.selectCommit(commit(1).hash);
    const pending = store.selectFile("old.ts");
    await store.selectCommit(commit(2).hash);
    pendingDiff.resolve({ path: "old.ts", binary: false, hunks: [] } as unknown as FileDiff);
    await pending;
    expect(store.selectedHash).toBe(commit(2).hash);
    expect(store.fileDiff).toBeUndefined();
    expect(store.selectedFilePath).toBeUndefined();
  });

  it("retains the latest file selection when responses arrive out of order", async () => {
    const pendingDiff = deferred<FileDiff>();
    vi.mocked(backend.historyFileDiff).mockReturnValueOnce(pendingDiff.promise);
    const store = useHistoryStore();
    store.resetForRepository("C:/repo", 1);
    await store.selectCommit(commit(1).hash);
    const pending = store.selectFile("old.ts");
    await store.selectFile("new.ts");
    const latest = store.fileDiff;
    pendingDiff.resolve({ path: "old.ts", binary: false, hunks: [] } as unknown as FileDiff);
    await pending;
    expect(store.selectedFilePath).toBe("new.ts");
    expect(store.fileDiff).toEqual(latest);
  });

  it("rejects a page response from a replaced repository", async () => {
    const pendingPage = deferred<HistoryPage>();
    vi.mocked(backend.historyPage).mockReturnValueOnce(pendingPage.promise);
    const store = useHistoryStore();
    const pending = store.ensureLoaded("C:/one", 1);

    store.resetForRepository("C:/two", 2);
    pendingPage.resolve(page([commit(0)], null));
    await pending;

    expect(store.commits).toEqual([]);
    expect(store.loadedRootPath).toBe("C:/two");
  });

  it("restarts an invalid cursor while retaining query and selection", async () => {
    const firstPage = page([commit(0)], "stale");
    const restarted = page([commit(0), commit(1)], null);
    vi.mocked(backend.historyPage)
      .mockResolvedValueOnce(firstPage)
      .mockRejectedValueOnce({
        code: "invalidHistoryCursor",
        message: "历史记录已变化。",
      })
      .mockResolvedValueOnce(restarted);
    const store = useHistoryStore();

    await store.ensureLoaded("C:/repo", 1);
    store.query.reference = "refs/heads/main";
    store.query.search = "needle";
    store.selectedHash = commit(0).hash;
    await store.loadNextPage();

    expect(store.commits).toEqual(restarted.commits);
    expect(store.selectedHash).toBe(commit(0).hash);
    expect(store.query).toEqual({
      reference: "refs/heads/main",
      search: "needle",
      cursor: null,
    });
    expect(backend.historyPage).toHaveBeenNthCalledWith(3, "C:/repo", {
      reference: "refs/heads/main",
      search: "needle",
      cursor: null,
    });
  });

  it("debounces search changes for 250 milliseconds", async () => {
    vi.useFakeTimers();
    vi.mocked(backend.historyPage).mockResolvedValue(page([], null));
    const store = useHistoryStore();
    store.resetForRepository("C:/repo", 1);

    store.setSearch("need");
    await vi.advanceTimersByTimeAsync(249);
    expect(backend.historyPage).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(backend.historyPage).toHaveBeenCalledTimes(1);
    expect(store.query.search).toBe("need");
    vi.useRealTimers();
  });

  it("runs checkout once and applies the authoritative mutation result", async () => {
    const result: HistoryMutationResult = {
      workspace: {
        repository: {
          rootPath: "C:/repo",
          name: "repo",
          currentBranch: null,
          headShortHash: "0000000",
          isClean: true,
          changedFileCount: 0,
          conflictCount: 0,
          remotes: [],
          upstream: null,
        },
        changes: { files: [], stagedCount: 0, unstagedCount: 0 },
      },
      history: page([commit(0)], null),
      operationState: { kind: "none", conflicts: [], abortAction: null },
    };
    vi.mocked(backend.historyCheckout).mockResolvedValue(result);
    const repository = useRepositoryStore();
    repository.snapshot = { ...result.workspace.repository, currentBranch: "main" };
    const store = useHistoryStore();
    store.resetForRepository("C:/repo", 1);

    await store.checkout(commit(0).hash);

    expect(backend.historyCheckout).toHaveBeenCalledWith(
      "C:/repo",
      commit(0).hash,
    );
    expect(repository.snapshot?.currentBranch).toBeNull();
    expect(store.commits).toEqual(result.history.commits);
    expect(useOperationStore().state.kind).toBe("none");
    expect(store.submitting).toBe(false);
  });

  it("keeps an authoritative mutation page over an older pending read", async () => {
    const oldRead = deferred<HistoryPage>();
    vi.mocked(backend.historyPage).mockReturnValue(oldRead.promise);
    const store = useHistoryStore();
    const pending = store.ensureLoaded("C:/repo", 1);
    const authoritative = page([commit(2)], null);

    store.applyPage(authoritative, "C:/repo");
    oldRead.resolve(page([commit(1)], null));
    await pending;

    expect(store.commits).toEqual(authoritative.commits);
    expect(store.loading).toBe(false);
  });

  it("applies cherry-pick conflict state before reporting retained unstaged backup", async () => {
    const failure = { code: "gitConflict" as const, message: "Local changes retained in stash backup" };
    const result: HistoryMutationResult = {
      workspace: {
        repository: { rootPath: "C:/repo", name: "repo", currentBranch: "release", headShortHash: "1234567", isClean: false, changedFileCount: 1, conflictCount: 1, remotes: [], upstream: null },
        changes: { files: [], stagedCount: 0, unstagedCount: 1 },
      },
      history: page([commit(1)], null),
      operationState: { kind: "cherryPick", conflicts: [], abortAction: "cherryPick" },
      error: failure,
    };
    vi.mocked(backend.historyCherryPick).mockResolvedValue(result);
    useRepositoryStore().snapshot = { ...result.workspace.repository, currentBranch: "main" };
    const store = useHistoryStore();
    store.resetForRepository("C:/repo", 1);
    await expect(store.cherryPick({ commit: commit(1).hash, targetBranch: "release", returnAfterSuccess: true })).rejects.toEqual(failure);
    expect(store.error).toEqual(failure);
    expect(useRepositoryStore().snapshot?.currentBranch).toBe("release");
    expect(useOperationStore().state.kind).toBe("cherryPick");
    expect(store.submitting).toBe(false);
  });
});
