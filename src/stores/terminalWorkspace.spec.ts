import { createPinia, setActivePinia } from "pinia";
import { beforeEach, expect, it, vi } from "vitest";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests } from "@/lib/backend/client";
import { useRepositoryStore } from "./repository";
import { useTerminalStore } from "./terminal";
import { useChangesStore } from "./changes";
import { useHistoryStore } from "./history";
import { useRefsStore } from "./refs";
import { useOperationStore } from "./operation";
import { useUiStore } from "./ui";
import { flushPromises } from "@vue/test-utils";

let backend: ReturnType<typeof createBackendFixture>;
beforeEach(() => {
  setActivePinia(createPinia()); backend = createBackendFixture(); setBackendClientForTests(backend);
  useRepositoryStore().snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "old", headShortHash: "abc", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null };
});

it("refreshes real HEAD, module snapshots and conflict state after a terminal command", async () => {
  const repo = useRepositoryStore();
  vi.mocked(backend.repositoryRefresh).mockResolvedValue({ ...repo.snapshot!, currentBranch: "new", headShortHash: "def" });
  vi.mocked(backend.conflictsSnapshot).mockResolvedValue({ operationState: { kind: "cherryPick", conflicts: [], abortAction: "cherryPick" }, operationToken: "token", files: [], continueAction: "cherryPick", stagedFiles: [] });
  useHistoryStore().selectedHash = "old"; useRefsStore().selectedFullName = "old";
  useTerminalStore().refreshing = true;
  await repo.refreshAfterTerminal("C:/repo", repo.generation);
  expect(repo.snapshot?.currentBranch).toBe("new"); expect(repo.snapshot?.headShortHash).toBe("def");
  expect(useOperationStore().state.kind).toBe("cherryPick");
  expect(useHistoryStore().selectedHash).toBeUndefined();
  expect(backend.taskBranchesSnapshot).toHaveBeenCalled();
  for (const method of [backend.refsSnapshot, backend.historyPage, backend.stashSnapshot, backend.remotesSnapshot]) expect(method).not.toHaveBeenCalled();
  await useHistoryStore().ensureLoaded("C:/repo", 0);
  expect(backend.historyPage).toHaveBeenCalled();
});

it("refreshes the visible history in the background after command reconciliation", async () => {
  const repo = useRepositoryStore();
  useUiStore().activeView = "history";
  vi.mocked(backend.repositoryRefresh).mockResolvedValue({ ...repo.snapshot! });
  let finish!: (value: Awaited<ReturnType<typeof backend.historyPage>>) => void;
  vi.mocked(backend.historyPage).mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
  await repo.refreshAfterTerminal("C:/repo", 0);
  expect(useHistoryStore().loading).toBe(true);
  expect(repo.navigationBusy).toBe(false);
  finish({ commits: [], nextCursor: null, queryFingerprint: "new", continuationLanes: [] });
  await flushPromises();
  expect(useHistoryStore().queryFingerprint).toBe("new");
  expect(repo.refreshingModules).toBe(false);
});

it("discards stale post-command history and retries the visible view on the next refresh", async () => {
  const repo = useRepositoryStore(), history = useHistoryStore();
  useUiStore().activeView = "history";
  vi.mocked(backend.repositoryRefresh).mockImplementation(async () => ({ ...repo.snapshot! }));
  let finish!: (value: Awaited<ReturnType<typeof backend.historyPage>>) => void;
  vi.mocked(backend.historyPage).mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
  await repo.refreshAfterTerminal("C:/repo", 0);
  useChangesStore().applyWorkspace({ repository: { ...repo.snapshot!, currentBranch: "newer", headShortHash: "newer" }, changes: { files: [], stagedCount: 0, unstagedCount: 0 } });
  finish({ commits: [], nextCursor: null, queryFingerprint: "old", continuationLanes: [] });
  await flushPromises();
  expect(history.queryFingerprint).not.toBe("old");
  vi.mocked(backend.historyPage).mockResolvedValueOnce({ commits: [], nextCursor: null, queryFingerprint: "newer", continuationLanes: [] });
  await repo.refresh({ metadata: false });
  expect(history.queryFingerprint).toBe("newer");
  expect(backend.historyPage).toHaveBeenCalledTimes(2);
});

it("does not switch repositories or submit UI writes while a terminal owns the repository", async () => {
  const repo = useRepositoryStore(); useTerminalStore().status = "running";
  await repo.open("C:/other"); expect(backend.repositoryOpen).not.toHaveBeenCalled(); expect(repo.generation).toBe(0);
  await expect(useChangesStore().stageFile("a.txt")).rejects.toMatchObject({ code: "gitOperationInProgress" });
  await expect(useHistoryStore().checkout("abc")).rejects.toMatchObject({ code: "gitOperationInProgress" });
  expect(backend.changesStageFile).not.toHaveBeenCalled(); expect(backend.historyCheckout).not.toHaveBeenCalled();
});

it("invalidates cached commit data even when the post-command repository refresh fails", async () => {
  useHistoryStore().selectedHash = "old";
  vi.mocked(backend.repositoryRefresh).mockRejectedValueOnce({ code: "io", message: "refresh failed" });
  await expect(useRepositoryStore().refreshAfterTerminal("C:/repo", 0)).rejects.toMatchObject({ code: "io" });
  expect(useHistoryStore().selectedHash).toBeUndefined();
});
