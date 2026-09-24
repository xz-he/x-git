import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, expect, it, vi } from "vitest";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests } from "@/lib/backend/client";
import type { FileDiff, MutationWorkspace } from "@/lib/backend/types";
import { useRepositoryStore } from "./repository";
import { useChangesStore } from "./changes";
import { useRemotesStore } from "./remotes";
import { useHistoryStore } from "./history";
import { useStashesStore } from "./stashes";
import { useActivityStore } from "./activity";
import { useUiStore } from "./ui";
import AppSidebar from "@/components/layout/AppSidebar.vue";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const diff = (path: string): FileDiff => ({ path, scope: "unstaged", hunks: [], binary: false });
let backend: ReturnType<typeof createBackendFixture>;
beforeEach(() => {
  setActivePinia(createPinia());
  backend = createBackendFixture(); setBackendClientForTests(backend);
  useRepositoryStore().snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abc", isClean: false, changedFileCount: 1, conflictCount: 0, remotes: [], upstream: null };
});

it("does not lock the toolbar while reading a file diff, and selects the new file immediately", async () => {
  const pending = deferred<FileDiff>();
  vi.mocked(backend.changesFileDiff).mockReturnValueOnce(pending.promise);
  const changes = useChangesStore();
  const reading = changes.selectFile("large.txt", "unstaged");
  const locked = useRepositoryStore().navigationBusy;
  const selection = changes.selectedPath;
  pending.resolve(diff("large.txt")); await reading;
  expect(locked).toBe(false);
  expect(selection).toBe("large.txt");
});

it("a slow diff cannot overwrite a completed stage or unlock an active mutation", async () => {
  const changes = useChangesStore();
  const readingResult = deferred<FileDiff>(), mutationResult = deferred<MutationWorkspace>();
  vi.mocked(backend.changesFileDiff).mockReturnValueOnce(readingResult.promise);
  vi.mocked(backend.changesStageFile).mockReturnValueOnce(mutationResult.promise);
  const reading = changes.selectFile("a.txt", "unstaged");
  const staging = changes.stageFile("a.txt");
  readingResult.resolve(diff("a.txt")); await reading;
  const lockedDuringMutation = useRepositoryStore().navigationBusy;
  mutationResult.resolve({ workspace: { repository: { ...useRepositoryStore().snapshot! }, changes: { files: [], stagedCount: 0, unstagedCount: 0 } }, operationState: { kind: "none", conflicts: [], abortAction: null } });
  await staging;
  expect(lockedDuringMutation).toBe(true);
  expect(changes.selectedDiff).toBeUndefined();
});

it("refresh unlocks once file statuses are ready without awaiting the selected diff", async () => {
  const changes = useChangesStore(), repo = useRepositoryStore();
  changes.selectedPath = "a.txt";
  vi.mocked(backend.repositoryRefresh).mockResolvedValue({ ...repo.snapshot! });
  vi.mocked(backend.changesSnapshot).mockResolvedValue({ files: [{ path: "a.txt", oldPath: null, indexStatus: " ", worktreeStatus: "M", staged: false, unstaged: true, conflict: false }], stagedCount: 0, unstagedCount: 1 });
  const pending = deferred<FileDiff>();
  vi.mocked(backend.changesFileDiff).mockReturnValueOnce(pending.promise);
  const refreshing = repo.refresh();
  await flushPromises();
  const locked = repo.navigationBusy;
  pending.resolve(diff("a.txt")); await refreshing;
  expect(locked).toBe(false);
});

it("terminal completion invalidates hidden views without eagerly loading all of them", async () => {
  const repo = useRepositoryStore();
  vi.mocked(backend.repositoryRefresh).mockResolvedValue({ ...repo.snapshot! });
  await repo.refreshAfterTerminal("C:/repo", 0);
  for (const method of [backend.historyPage, backend.stashSnapshot, backend.remotesSnapshot, backend.refsSnapshot]) {
    expect(method).not.toHaveBeenCalled();
  }
});

it.each(["commit", "stash", "history", "remote"])("keeps page navigation available during %s while blocking additional Git writes", async kind => {
  if (kind === "commit") useChangesStore().operation = { kind: "commit" };
  if (kind === "stash") useStashesStore().submitting = true;
  if (kind === "history") useHistoryStore().submitting = true;
  if (kind === "remote") useRemotesStore().status = "running";
  expect(useRepositoryStore().viewNavigationBusy).toBe(false);
  expect(useRepositoryStore().navigationBusy).toBe(true);
  const sidebar = mount(AppSidebar);
  try {
    expect(sidebar.get('[data-view="history"]').attributes("disabled")).toBeUndefined();
    await sidebar.get('[data-view="history"]').trigger("click");
    expect(useUiStore().activeView).toBe("history");
    for (const action of sidebar.findAll('.nav-item:not([data-view])')) expect(action.attributes("disabled")).toBeDefined();
  } finally { sidebar.unmount(); }
  await expect(useChangesStore().stageFile("a.txt")).rejects.toMatchObject({ code: "gitOperationInProgress" });
  expect(backend.changesStageFile).not.toHaveBeenCalled();
});

it("ignores an outdated diff failure after selecting another file", async () => {
  const pending = deferred<FileDiff>();
  vi.mocked(backend.changesFileDiff).mockReturnValueOnce(pending.promise).mockResolvedValueOnce(diff("b.txt"));
  const changes = useChangesStore();
  const first = changes.selectFile("a.txt", "unstaged");
  await changes.selectFile("b.txt", "unstaged");
  pending.reject({ code: "gitCommandFailed", message: "old error" });
  await first;
  expect(changes.selectedPath).toBe("b.txt");
  expect(changes.selectedDiff?.path).toBe("b.txt");
  expect(changes.error).toBeUndefined();
  expect(changes.diffLoading).toBe(false);
});

it("drops a pending diff after the workspace is mutated", async () => {
  const changes = useChangesStore();
  const pending = deferred<FileDiff>();
  vi.mocked(backend.changesFileDiff).mockReturnValueOnce(pending.promise);
  const reading = changes.selectFile("a.txt", "unstaged");
  changes.applyWorkspace({ repository: { ...useRepositoryStore().snapshot! }, changes: { files: [], stagedCount: 0, unstagedCount: 0 } });
  pending.resolve(diff("a.txt")); await reading;
  expect(changes.selectedDiff).toBeUndefined();
  expect(changes.diffLoading).toBe(false);
});

it.each([false, true])("clears pending diff state when a file list reload removes its selection (preserve=%s)", async preserve => {
  const changes = useChangesStore();
  const pending = deferred<FileDiff>();
  vi.mocked(backend.changesFileDiff).mockReturnValueOnce(pending.promise);
  vi.mocked(backend.changesSnapshot).mockResolvedValue({ files: [], stagedCount: 0, unstagedCount: 0 });
  const reading = changes.selectFile("a.txt", "unstaged");
  await changes.load("C:/repo", preserve);
  pending.resolve(diff("a.txt")); await reading;
  expect(changes.selectedPath).toBeUndefined();
  expect(changes.selectedDiff).toBeUndefined();
  expect(changes.diffLoading).toBe(false);
});

it("unlocks after rollback without waiting for the operation history list", async () => {
  const repo = useRepositoryStore(), activity = useActivityStore();
  vi.mocked(backend.activityRollback).mockResolvedValue({ workspace: { repository: { ...repo.snapshot! }, changes: { files: [], stagedCount: 0, unstagedCount: 0 } }, operationState: { kind: "none", conflicts: [], abortAction: null } });
  vi.spyOn(repo, "refreshAfterTerminal").mockResolvedValue();
  const pending = deferred<Awaited<ReturnType<typeof backend.activityList>>>();
  vi.mocked(backend.activityList).mockReturnValueOnce(pending.promise);
  const rollingBack = activity.rollback({ id: "record", title: "stage", createdAt: 0, status: "success", message: "", rollbackKind: "index", rollbackId: null, rollbackReason: "" });
  await flushPromises();
  const locked = repo.navigationBusy;
  expect(activity.loading).toBe(true);
  pending.resolve([]); await rollingBack;
  expect(locked).toBe(false);
});
