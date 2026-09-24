import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { RepositorySnapshot } from "@/lib/backend/types";
import { useRepositoryStore } from "./repository";
import { useRepositoryMonitorStore } from "./repositoryMonitor";
import { useChangesStore } from "./changes";
import { useHistoryStore } from "./history";

const repository: RepositorySnapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "aaaaaaa", isClean: true, changedFileCount: 0, conflictCount: 0, upstream: null, remotes: [] };
describe("automatic repository refresh", () => {
  let backend: BackendClient;
  let version: { watchId: string; worktreeVersion: number; metadataVersion: number };
  beforeEach(() => {
    vi.useFakeTimers(); setActivePinia(createPinia());
    version = { watchId: "watch", worktreeVersion: 0, metadataVersion: 0 };
    backend = createBackendFixture({ repositoryRefresh: vi.fn(async () => ({ ...repository })), repositoryWatchSnapshot: vi.fn(async () => ({ ...version })) });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { ...repository };
    useRepositoryMonitorStore().initialize();
  });
  afterEach(() => { useRepositoryMonitorStore().dispose(); vi.useRealTimers(); });
  const tick = () => vi.advanceTimersByTimeAsync(1000);

  it("does not refresh an unchanged repository on startup, elapsed time, focus or watcher errors", async () => {
    await vi.advanceTimersByTimeAsync(120_000);
    window.dispatchEvent(new Event("focus"));
    document.dispatchEvent(new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(150);
    expect(backend.repositoryWatchSnapshot).toHaveBeenCalled();
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
    vi.mocked(backend.repositoryWatchSnapshot).mockRejectedValue({ code: "io", message: "watch unavailable" });
    await vi.advanceTimersByTimeAsync(15_000);
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
  });

  it("checks lightweight counters without repeatedly loading an unchanged repository", async () => {
    await tick();
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(backend.repositoryWatchSnapshot).toHaveBeenCalledTimes(11);
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
  });
  it("coalesces bursts and retains the selected file and commit message while reloading its diff", async () => {
    await tick();
    const changes = useChangesStore();
    changes.selectedPath = "file.txt"; changes.selectedScope = "unstaged"; changes.commitMessage = "draft commit";
    vi.mocked(backend.changesSnapshot).mockResolvedValue({ files: [{ path: "file.txt", oldPath: null, indexStatus: " ", worktreeStatus: "M", staged: false, unstaged: true, conflict: false }], stagedCount: 0, unstagedCount: 1 });
    vi.mocked(backend.changesFileDiff).mockResolvedValue({ path: "file.txt", scope: "unstaged", binary: false, hunks: [] });
    version.worktreeVersion += 100;
    await tick();
    expect(backend.repositoryRefresh).toHaveBeenCalledTimes(1);
    expect(backend.changesFileDiff).toHaveBeenCalledWith(repository.rootPath, "file.txt", "unstaged");
    expect(changes.selectedPath).toBe("file.txt");
    expect(changes.commitMessage).toBe("draft commit");
    await tick(); expect(backend.repositoryRefresh).toHaveBeenCalledTimes(1);
  });
  it("refreshes history after an external branch switch without resetting search", async () => {
    await tick();
    const history = useHistoryStore();
    await history.ensureLoaded(repository.rootPath, 0);
    history.query.search = "ticket";
    vi.mocked(backend.repositoryRefresh).mockResolvedValue({ ...repository, currentBranch: "feature/dev", headShortHash: "bbbbbbb" });
    vi.mocked(backend.historyPage).mockResolvedValue({ commits: [], nextCursor: null, queryFingerprint: "new-head", continuationLanes: [] });
    version.metadataVersion++;
    await tick();
    expect(useRepositoryStore().snapshot?.currentBranch).toBe("feature/dev");
    expect(history.queryFingerprint).toBe("new-head");
    expect(history.query.search).toBe("ticket");
  });
  it("defers changes during a mutation and refreshes after it ends", async () => {
    await tick();
    useChangesStore().operation = { kind: "stageFile" };
    version.metadataVersion++;
    await tick(); await tick();
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
    useChangesStore().operation = { kind: "idle" };
    await tick(); expect(backend.repositoryRefresh).toHaveBeenCalledTimes(1);
  });
  it("ignores a late watcher response from the previous repository", async () => {
    let resolve!: (value: typeof version) => void;
    vi.mocked(backend.repositoryWatchSnapshot).mockImplementationOnce(() => new Promise(next => { resolve = next; }));
    await tick();
    useRepositoryStore().snapshot = { ...repository, rootPath: "C:/other" };
    useRepositoryStore().generation++;
    await nextTick();
    resolve({ ...version }); await Promise.resolve(); await Promise.resolve();
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
    vi.mocked(backend.repositoryRefresh).mockResolvedValue({ ...repository, rootPath: "C:/other" });
    await tick();
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
    version.worktreeVersion++;
    await tick();
    expect(backend.repositoryRefresh).toHaveBeenCalledWith("C:/other", true);
  });
  it("does not consume filesystem changes while a metadata refresh is pending", async () => {
    await tick();
    useRepositoryStore().refreshingModules = true;
    version.metadataVersion++;
    await tick();
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
    useRepositoryStore().refreshingModules = false;
    await tick();
    expect(backend.repositoryRefresh).toHaveBeenCalledTimes(1);
    await tick();
    expect(backend.repositoryRefresh).toHaveBeenCalledTimes(1);
  });
  it("checks on focus and retries detection errors without losing pending changes", async () => {
    await tick();
    window.dispatchEvent(new Event("focus")); await vi.advanceTimersByTimeAsync(150);
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
    vi.mocked(backend.repositoryWatchSnapshot).mockRejectedValue({ code: "io", message: "watch unavailable" });
    await tick(); await vi.advanceTimersByTimeAsync(5000);
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
    version.metadataVersion++;
    vi.mocked(backend.repositoryWatchSnapshot).mockImplementation(async () => ({ ...version }));
    await vi.advanceTimersByTimeAsync(5000);
    expect(backend.repositoryRefresh).toHaveBeenCalledTimes(1);
    await tick(); expect(backend.repositoryRefresh).toHaveBeenCalledTimes(1);
  });
  it("stops polling and removes focus listeners on disposal", async () => {
    await tick();
    useRepositoryMonitorStore().dispose();
    window.dispatchEvent(new Event("focus")); await vi.advanceTimersByTimeAsync(60_000);
    expect(backend.repositoryRefresh).not.toHaveBeenCalled();
    expect(backend.repositoryWatchStop).toHaveBeenCalled();
  });
  it("does not consume a changed version when refreshing it fails", async () => {
    await tick();
    version.worktreeVersion++;
    vi.mocked(backend.repositoryRefresh).mockRejectedValueOnce({ code: "gitLocked", message: "busy" });
    await tick();
    expect(backend.repositoryRefresh).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(5000);
    expect(backend.repositoryRefresh).toHaveBeenCalledTimes(2);
    await tick(); expect(backend.repositoryRefresh).toHaveBeenCalledTimes(2);
  });
});
