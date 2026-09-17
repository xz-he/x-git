import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { FileDiff, StashDetail, StashEntry, StashMutationResult, StashSnapshot } from "@/lib/backend/types";
import { useStashesStore } from "@/stores/stashes";
import { useRepositoryStore } from "@/stores/repository";
import { useOperationStore } from "@/stores/operation";
import { useChangesStore } from "@/stores/changes";
import { createBackendFixture } from "@/test/backend";

const root = "C:/repo";
const entry = (id = "a", selector = "stash@{0}"): StashEntry => ({
  selector, objectId: id.repeat(40), branch: "main", description: "checkpoint", timestamp: "2026-09-10T10:00:00+08:00",
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}
function result(outcome: StashMutationResult["outcome"] = "created"): StashMutationResult {
  return {
    outcome, stashes: { entries: [entry()] },
    workspace: { repository: { rootPath: root, name: "repo", currentBranch: "main", headShortHash: "aaaaaaa", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null }, changes: { files: [], stagedCount: 0, unstagedCount: 0 } },
    operationState: { kind: "none", conflicts: [], abortAction: null }, error: null,
  };
}

describe("stashes store", () => {
  let backend: BackendClient;
  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture();
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = result().workspace.repository;
    useStashesStore().resetForRepository(root, 0);
  });

  it("coalesces lazy reads and rejects replaced-root responses", async () => {
    const pending = deferred<StashSnapshot>();
    vi.mocked(backend.stashSnapshot).mockReturnValue(pending.promise);
    const store = useStashesStore();
    const first = store.ensureLoaded(root, 0);
    const second = store.ensureLoaded(root, 0);
    expect(backend.stashSnapshot).toHaveBeenCalledTimes(1);
    store.resetForRepository("C:/other", 1);
    pending.resolve({ entries: [entry()] });
    await Promise.all([first, second]);
    expect(store.snapshot).toBeUndefined();
  });

  it("loads an empty stack only once", async () => {
    const store = useStashesStore();
    await store.ensureLoaded(root, 0);
    await store.ensureLoaded(root, 0);
    expect(backend.stashSnapshot).toHaveBeenCalledTimes(1);
  });

  it("keeps only the newest selected entry detail", async () => {
    const pending = deferred<StashDetail>();
    vi.mocked(backend.stashDetail).mockReturnValueOnce(pending.promise).mockResolvedValueOnce({ entry: entry("b", "stash@{1}"), files: [] });
    const store = useStashesStore();
    const first = store.selectEntry(entry());
    await store.selectEntry(entry("b", "stash@{1}"));
    pending.resolve({ entry: entry(), files: [] });
    await first;
    expect(store.detail?.entry.objectId).toBe("b".repeat(40));
    expect(backend.stashDetail).toHaveBeenCalledWith(root, { selector: "stash@{0}", expectedObjectId: "a".repeat(40) });
  });

  it("keeps only the newest selected file diff", async () => {
    const pending = deferred<FileDiff>();
    const store = useStashesStore();
    vi.mocked(backend.stashDetail).mockResolvedValue({ entry: entry(), files: [] });
    await store.selectEntry(entry());
    vi.mocked(backend.stashFileDiff).mockReturnValueOnce(pending.promise).mockResolvedValueOnce({ path: "two", scope: "commit", binary: false, hunks: [] });
    const first = store.selectFile("one");
    await store.selectFile("two");
    pending.resolve({ path: "one", scope: "commit", binary: false, hunks: [] });
    await first;
    expect(store.fileDiff?.path).toBe("two");
  });

  it("distinguishes tracked deletion from a same-path untracked saved file", async () => {
    const store = useStashesStore();
    vi.mocked(backend.stashDetail).mockResolvedValue({ entry: entry(), files: [] });
    vi.mocked(backend.stashFileDiff).mockResolvedValue({ path: "file.txt", scope: "commit", binary: false, hunks: [] });
    await store.selectEntry(entry());
    await store.selectFile("file.txt", true);
    expect(backend.stashFileDiff).toHaveBeenCalledWith(root, { selector: "stash@{0}", expectedObjectId: "a".repeat(40) }, "file.txt", true);
    expect(store.selectedFileUntracked).toBe(true);
  });

  it("keeps form values on failure and noChanges, clears only Created", async () => {
    const store = useStashesStore();
    store.message = "checkpoint";
    expect(store.includeUntracked).toBe(false);
    vi.mocked(backend.stashCreate).mockRejectedValueOnce({ code: "gitLocked", message: "locked" }).mockResolvedValueOnce(result("noChanges")).mockResolvedValueOnce(result());
    await expect(store.create()).rejects.toMatchObject({ code: "gitLocked" });
    expect(store.message).toBe("checkpoint");
    await store.create();
    expect(store.message).toBe("checkpoint");
    expect(store.outcome).toBe("noChanges");
    await store.create();
    expect(store.message).toBe("");
    expect(backend.stashCreate).toHaveBeenCalledWith(root, { message: "checkpoint", includeUntracked: false });
  });

  it("prunes disappeared file choices and clears them after creation or repository replacement", async () => {
    const store = useStashesStore();
    const changes = useChangesStore();
    const file = { path: "chosen.txt", oldPath: null, indexStatus: "M", worktreeStatus: " ", staged: true, unstaged: false, conflict: false };
    changes.snapshot = { files: [file], stagedCount: 1, unstagedCount: 0 };
    store.selectFiles = true; store.selectedPaths = [file.path];
    changes.snapshot = { files: [], stagedCount: 0, unstagedCount: 0 };
    expect(store.selectedPaths).toEqual([]);
    await expect(store.create()).rejects.toMatchObject({ code: "invalidPath" });
    expect(backend.stashCreate).not.toHaveBeenCalled();
    changes.snapshot = { files: [file], stagedCount: 1, unstagedCount: 0 };
    store.selectedPaths = [file.path];
    vi.mocked(backend.stashCreate).mockResolvedValue(result());
    await store.create();
    expect(store.selectedPaths).toEqual([]);
    store.selectedPaths = [file.path];
    store.resetForRepository("C:/other", 1);
    expect(store.selectedPaths).toEqual([]);
    expect(store.selectFiles).toBe(false);
  });

  it("applies authoritative workspace and retained conflicts even with an error", async () => {
    const conflict = result("retained");
    conflict.operationState.conflicts = [{ path: "file.txt", status: "UU" }];
    conflict.workspace.repository.conflictCount = 1;
    conflict.error = { code: "gitConflict", message: "conflicted" };
    vi.mocked(backend.stashPop).mockResolvedValue(conflict);
    const store = useStashesStore();
    await expect(store.pop(entry())).rejects.toMatchObject({ code: "gitConflict" });
    expect(store.snapshot).toEqual(conflict.stashes);
    expect(useOperationStore().state).toEqual(conflict.operationState);
    expect(useRepositoryStore().snapshot?.conflictCount).toBe(1);
  });

  it("blocks duplicate submissions synchronously", async () => {
    const pending = deferred<StashMutationResult>();
    vi.mocked(backend.stashApply).mockReturnValue(pending.promise);
    const store = useStashesStore();
    const first = store.apply(entry());
    expect(store.submitting).toBe(true);
    await expect(store.apply(entry())).rejects.toMatchObject({ code: "gitOperationInProgress" });
    expect(backend.stashApply).toHaveBeenCalledTimes(1);
    pending.resolve(result("applied"));
    await first;
    expect(store.submitting).toBe(false);
  });

  it("never routes a selection to a repository other than the loaded owner", async () => {
    const store = useStashesStore();
    store.resetForRepository("C:/other", 0);
    await expect(store.apply(entry())).rejects.toMatchObject({ code: "invalidRepository" });
    expect(backend.stashApply).not.toHaveBeenCalled();
  });

  it("discards mutations from an old repository generation", async () => {
    const pending = deferred<StashMutationResult>();
    vi.mocked(backend.stashCreate).mockReturnValue(pending.promise);
    const store = useStashesStore();
    const first = store.create();
    useRepositoryStore().generation += 1;
    store.resetForRepository("C:/other", 1);
    store.message = "new repository draft";
    pending.resolve(result());
    await first;
    expect(store.snapshot).toBeUndefined();
    expect(store.message).toBe("new repository draft");
  });

  it("does not allow an old read to overwrite a mutation snapshot", async () => {
    const pending = deferred<StashSnapshot>();
    vi.mocked(backend.stashSnapshot).mockReturnValue(pending.promise);
    const store = useStashesStore();
    const first = store.ensureLoaded(root, 0);
    store.applySnapshot({ entries: [entry("b")] }, root);
    pending.resolve({ entries: [entry()] });
    await first;
    expect(store.snapshot?.entries[0]?.objectId).toBe("b".repeat(40));
  });

  it("preserves inspected immutable detail after Apply and conflict refresh", async () => {
    const store = useStashesStore();
    const detail = { entry: entry(), files: [] };
    vi.mocked(backend.stashDetail).mockResolvedValue(detail);
    vi.mocked(backend.stashApply).mockResolvedValue(result("applied"));
    await store.selectEntry(entry());
    await store.apply(entry());
    expect(store.detail).toEqual(detail);
    expect(store.selectedEntry).toEqual(entry());
  });
});
