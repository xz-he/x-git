import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useConflictsStore } from "./conflicts";
import { useRepositoryStore } from "./repository";
import { useUiStore } from "./ui";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests } from "@/lib/backend/client";
import { useOperationStore } from "./operation";
import type { ConflictDetail, ConflictSnapshot, ConflictVersion, RepositoryOperationState, RepositorySnapshot } from "@/lib/backend/types";

const repository: RepositorySnapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abcdef0", upstream: null, isClean: false, changedFileCount: 1, conflictCount: 1, remotes: [] };
const state: RepositoryOperationState = { kind: "merge", conflicts: [{ path: "a.txt", status: "UU" }], abortAction: "merge" };
const snapshot: ConflictSnapshot = { operationState: state, operationToken: "operation", files: [{ path: "a.txt", status: "UU", supported: true, reason: null }], continueAction: null, stagedFiles: [] };
const version: ConflictVersion = { exists: true, oid: "a".repeat(40), mode: "100644", kind: "text", text: "original\n", byteLength: 9, bom: false, lineEnding: "lf" };
const detail: ConflictDetail = { path: "a.txt", token: "file-token", operationKind: "merge", base: version, ours: { ...version, text: "ours\n" }, theirs: { ...version, text: "theirs\n" }, working: version, editable: true, canChooseOurs: true, canChooseTheirs: true, canDelete: true, unsupportedReason: null };

describe("conflict workflow store", () => {
  beforeEach(() => { setActivePinia(createPinia()); useRepositoryStore().snapshot = repository; });
  function backend() {
    const client = createBackendFixture({ conflictsSnapshot: vi.fn(async () => snapshot), conflictsDetail: vi.fn(async () => detail) });
    setBackendClientForTests(client);
    return client;
  }
  it("loads operation state and retains per-file drafts across home navigation", async () => {
    backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt");
    store.edit("edited\n"); useUiStore().homeVisible = true; useUiStore().homeVisible = false;
    await store.selectFile("a.txt");
    expect(store.current?.resolution).toMatchObject({ kind: "text", text: "edited\n" });
    expect(store.hasDirtyDrafts).toBe(true);
  });
  it("refreshes externally changed clean details but preserves edited conflict drafts", async () => {
    const client = backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt");
    vi.mocked(client.conflictsDetail).mockResolvedValue({ ...detail, token: "changed", working: { ...version, text: "external edit\n" } });
    await store.refreshCleanDetails();
    expect(store.current?.resolution).toMatchObject({ text: "external edit\n" });
    expect(store.current?.dirty).toBe(false);
    store.edit("my draft\n");
    await store.refreshCleanDetails();
    expect(client.conflictsDetail).toHaveBeenCalledTimes(2);
    expect(store.current?.resolution).toMatchObject({ text: "my draft\n" });
  });
  it("ignores a clean detail refresh if editing starts before its response", async () => {
    const client = backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt");
    let finish!: (value: ConflictDetail) => void;
    vi.mocked(client.conflictsDetail).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const refresh = store.refreshCleanDetails();
    store.edit("keep while refreshing\n");
    finish({ ...detail, token: "changed", working: { ...version, text: "external edit\n" } });
    await refresh;
    expect(store.current?.resolution).toMatchObject({ text: "keep while refreshing\n" });
    expect(store.current?.detail.token).toBe("file-token");
  });
  it("adopts a side as draft without writing and captures explicit save intent", async () => {
    const client = backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt");
    store.choose("theirs"); store.requestSave();
    expect(client.conflictsResolve).not.toHaveBeenCalled();
    expect(store.confirmation).toMatchObject({ kind: "save", path: "a.txt", token: "file-token", resolution: { kind: "theirs" } });
    store.cancelConfirmation(); expect(client.conflictsResolve).not.toHaveBeenCalled();
  });
  it("keeps draft and recovery location when the file write only partially succeeds", async () => {
    const client = backend(); const store = useConflictsStore();
    vi.mocked(client.conflictsResolve).mockResolvedValue({ workspace: { repository, changes: { files: [], stagedCount: 0, unstagedCount: 1 } }, operationState: state, conflicts: snapshot, refs: null, error: { code: "gitLocked", message: "Saved but not staged" }, recoveryPath: "C:/repo/.git/recovery/file", resolved: false });
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("resolved\n"); store.requestSave();
    await store.confirm();
    expect(store.hasDirtyDrafts).toBe(true); expect(store.error?.message).toContain("Saved"); expect(store.recoveryPath).toContain("recovery");
  });
  it("rejects late detail and invalidates confirmation after repository replacement", async () => {
    const client = backend(); const store = useConflictsStore();
    let finish!: (value: ConflictDetail) => void;
    vi.mocked(client.conflictsDetail).mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    await store.ensureLoaded("C:/repo", 0); const loading = store.selectFile("a.txt");
    useRepositoryStore().generation += 1; store.resetForRepository("C:/other", 1); finish(detail); await loading;
    expect(store.current).toBeUndefined(); expect(store.confirmation).toBeUndefined();
  });
  it("does not discard dirty text when refreshing without explicit confirmation", async () => {
    backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("keep this");
    store.requestReload();
    expect(store.current?.resolution).toMatchObject({ text: "keep this" });
    expect(store.confirmation?.kind).toBe("reload");
    store.cancelConfirmation(); expect(store.hasDirtyDrafts).toBe(true);
  });
  it("discovers a zero-conflict active operation on open and refresh", async () => {
    const client = backend();
    vi.mocked(client.repositoryOpen).mockResolvedValue(repository);
    vi.mocked(client.repositoryRefresh).mockResolvedValue(repository);
    vi.mocked(client.conflictsSnapshot).mockResolvedValue({ ...snapshot, files: [], continueAction: "merge", operationState: { kind: "merge", conflicts: [], abortAction: "merge" } });
    await useRepositoryStore().open(repository.rootPath);
    expect(useOperationStore().state.kind).toBe("merge");
    expect(useConflictsStore().canContinue).toBe(true);
    await useRepositoryStore().refresh();
    expect(client.conflictsSnapshot).toHaveBeenCalledTimes(2);
  });
  it("cancels repository replacement without losing dirty drafts", async () => {
    const client = backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("keep");
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    await useRepositoryStore().open("C:/other");
    expect(client.repositoryOpen).not.toHaveBeenCalled();
    expect(useRepositoryStore().generation).toBe(0); expect(store.current?.resolution).toMatchObject({ text: "keep" });
    confirm.mockRestore();
  });
  it("blocks switching and duplicate saves while a resolution is submitting", async () => {
    const client = backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("keep"); store.requestSave();
    let fail!: (error: unknown) => void;
    vi.mocked(client.conflictsResolve).mockImplementation(() => new Promise((_, reject) => { fail = reject; }));
    const saving = store.confirm();
    expect(useRepositoryStore().navigationBusy).toBe(true);
    await store.confirm(); await useRepositoryStore().open("C:/other");
    expect(client.repositoryOpen).not.toHaveBeenCalled(); expect(client.conflictsResolve).toHaveBeenCalledTimes(1);
    fail({ code: "gitLocked", message: "locked" }); await saving;
    expect(store.hasDirtyDrafts).toBe(true);
  });
  it("invalidates save confirmation on selection or module changes without losing drafts", async () => {
    backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("keep"); store.requestSave();
    useUiStore().openView("history");
    expect(store.confirmation).toBeUndefined(); expect(store.hasDirtyDrafts).toBe(true);
  });
  it("retains the recovery copy when a retry is rejected as stale", async () => {
    const client = backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("resolved"); store.requestSave();
    vi.mocked(client.conflictsResolve).mockResolvedValueOnce({ workspace: { repository, changes: { files: [], stagedCount: 0, unstagedCount: 1 } }, operationState: state, conflicts: snapshot, refs: null, error: { code: "gitLocked", message: "Saved but not staged" }, recoveryPath: "C:/repo/.git/recovery/file", resolved: false });
    await store.confirm();
    vi.mocked(client.conflictsResolve).mockRejectedValueOnce({ code: "staleConflict", message: "reload" });
    await store.confirm();
    expect(store.recoveryPath).toBe("C:/repo/.git/recovery/file");
    expect(store.error).toEqual({ code: "staleConflict", message: "reload" });
  });
  it("ignores delayed pre-save refresh after an authoritative resolution", async () => {
    const client = backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("resolved");
    let finish!: (value: ConflictSnapshot) => void;
    vi.mocked(client.conflictsSnapshot).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const refreshing = store.refresh(); store.requestSave();
    const next: ConflictSnapshot = { ...snapshot, operationToken: "next", files: [], continueAction: "merge", operationState: { ...state, conflicts: [] } };
    vi.mocked(client.conflictsResolve).mockResolvedValueOnce({ workspace: { repository, changes: { files: [], stagedCount: 1, unstagedCount: 0 } }, operationState: next.operationState, conflicts: next, refs: null, error: null, recoveryPath: null, resolved: true });
    await store.confirm(); finish(snapshot); await refreshing;
    expect(store.snapshot?.operationToken).toBe("next"); expect(store.canContinue).toBe(true);
  });
  it("supports literal conflict filenames that are object prototype keys", async () => {
    const client = backend(); const store = useConflictsStore();
    vi.mocked(client.conflictsDetail).mockResolvedValue({ ...detail, path: "__proto__" });
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("__proto__");
    expect(client.conflictsDetail).toHaveBeenCalledWith("C:/repo", "__proto__");
    store.edit("literal file"); expect(store.hasDirtyDrafts).toBe(true);
  });
  it("does not load an old path into a replacement repository after reload", async () => {
    const client = backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("discard"); store.requestReload();
    let finish!: (value: ConflictSnapshot) => void;
    vi.mocked(client.conflictsSnapshot).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const reloading = store.confirm();
    useRepositoryStore().snapshot = { ...repository, rootPath: "C:/other" }; useRepositoryStore().generation = 1;
    store.resetForRepository("C:/other", 1);
    finish(snapshot); await reloading;
    expect(client.conflictsDetail).toHaveBeenCalledTimes(1); expect(store.selectedPath).toBeUndefined();
  });
  it("clears drafts and closes confirmed rebase abort when Git restores the branch", async () => {
    const client = backend(); const store = useConflictsStore();
    useRepositoryStore().snapshot = { ...repository, currentBranch: null };
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("discard"); store.requestAbort();
    const idle = { kind: "none" as const, conflicts: [], abortAction: null };
    vi.mocked(client.refsAbort).mockResolvedValue({ workspace: { repository, changes: { files: [], stagedCount: 0, unstagedCount: 0 } }, refs: { localBranches: [], remoteBranches: [], tags: [] }, operationState: idle });
    vi.mocked(client.conflictsSnapshot).mockResolvedValue({ ...snapshot, operationState: idle, files: [], continueAction: null });
    await store.confirm();
    expect(store.hasDirtyDrafts).toBe(false); expect(store.confirmation).toBeUndefined(); expect(store.snapshot?.files).toEqual([]);
  });
  it("allows whole-side save despite a manual-editing limitation", async () => {
    const client = backend(); const store = useConflictsStore();
    vi.mocked(client.conflictsDetail).mockResolvedValue({ ...detail, editable: false, working: { ...version, kind: "binary", text: null }, unsupportedReason: "Manual editing requires UTF-8 text. A whole side can still be adopted." });
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.choose("ours"); store.requestSave();
    expect(store.confirmation).toMatchObject({ kind: "save", resolution: { kind: "ours" } });
  });
  it("blocks Continue while any retained draft is dirty even after external staging", async () => {
    const client = backend(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0); await store.selectFile("a.txt"); store.edit("unsaved");
    store.snapshot = { ...snapshot, files: [], continueAction: "merge", operationState: { ...state, conflicts: [] } };
    store.requestContinue(); await store.confirm();
    expect(store.canContinue).toBe(false); expect(client.conflictsContinue).not.toHaveBeenCalled();
  });
  it("offers no fake continuation or abort for Stash conflicts", async () => {
    const client = backend(); const store = useConflictsStore();
    vi.mocked(client.conflictsSnapshot).mockResolvedValue({ ...snapshot, operationState: { kind: "none", conflicts: state.conflicts, abortAction: null } });
    await store.ensureLoaded("C:/repo", 0); store.requestContinue(); store.requestAbort();
    expect(store.confirmation).toBeUndefined(); expect(store.canContinue).toBe(false);
  });
});
