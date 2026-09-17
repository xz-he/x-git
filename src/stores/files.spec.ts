import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useFilesStore } from "./files";
import { useRepositoryStore } from "./repository";
import { useUiStore } from "./ui";
import { useTerminalStore } from "./terminal";
import { useConflictsStore } from "./conflicts";
import type { ConflictDraft } from "./conflicts";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests } from "@/lib/backend/client";
import type { BackendClient } from "@/lib/backend/client";
import type { FileDirectoryPage, FileMutationResult, RepositoryFileEntry, RepositoryFilePreview, RepositorySnapshot } from "@/lib/backend/types";

const repository: RepositorySnapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abcdef0", upstream: null, isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [] };
const entry = (path: string, kind: RepositoryFileEntry["kind"] = "file"): RepositoryFileEntry => ({ name: path.split("/").pop()!, relativePath: path, kind, byteLength: kind === "file" ? 3 : null, reason: null, gitStatus: null });
const page = (dir = "", entries = [entry("a.txt"), entry("src", "directory")]): FileDirectoryPage => ({ relativeDir: dir, token: "page", entries, nextCursor: null, totalEntries: entries.length });
const preview = (path: string): RepositoryFilePreview => ({ relativePath: path, token: path, kind: "text", text: path, byteLength: 3, bom: false, lineEnding: "none", reason: null });
const applied: FileMutationResult = { applied: true, workspace: null, operationState: null, affectedDirectories: [""], selectedPath: null, recoveryPath: "C:/repo/.git/hq-git-file-recovery/id/payload", error: { code: "gitLocked", message: "已移动，刷新失败" } };
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(r => { resolve = r; }); return { promise, resolve }; }
function setup() {
  const client = createBackendFixture({
    filesList: vi.fn(async (_root, dir) => page(dir)),
    filesPreview: vi.fn(async (_root, path) => preview(path)),
    filesPrepare: vi.fn<BackendClient["filesPrepare"]>(async (_root, intent) => ({ intent, token: "prepared", sourcePath: "relativePath" in intent ? intent.relativePath : null, targetPath: "name" in intent ? intent.name : null, entryKind: "file", nodeCount: 1, fileCount: 1, directoryCount: 0, totalBytes: 3 })),
    filesExecute: vi.fn(async () => applied), repositoryRefresh: vi.fn(async () => repository),
  });
  setBackendClientForTests(client); return client;
}
describe("repository files lifecycle", () => {
  beforeEach(() => { setActivePinia(createPinia()); useRepositoryStore().snapshot = { ...repository }; useUiStore().activeView = "files"; });
  it("loads only direct children and retains expansion and selection across navigation", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    expect(client.filesList).toHaveBeenCalledTimes(1);
    await store.toggleDirectory("src"); await store.selectEntry(entry("a.txt"));
    useUiStore().homeVisible = true; useUiStore().homeVisible = false; await store.ensureLoaded(repository.rootPath, 0);
    expect(store.expanded.has("src")).toBe(true); expect(store.selectedEntry?.relativePath).toBe("a.txt"); expect(client.filesList).toHaveBeenCalledTimes(2);
  });
  it("allows read-only browsing during a terminal command while blocking file mutations", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    useTerminalStore().status = "running";
    expect(store.busy).toBe(false); expect(store.canMutate).toBe(false);
    await store.toggleDirectory("src"); await store.selectEntry(entry("a.txt"));
    expect(store.expanded.has("src")).toBe(true); expect(store.preview?.relativePath).toBe("a.txt");
    store.requestOperation({ kind: "delete", relativePath: "a.txt" });
    expect(store.confirmation).toBeUndefined(); expect(client.filesExecute).not.toHaveBeenCalled();
  });
  it("discards a late preview after selecting another file", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    const old = deferred<RepositoryFilePreview>(); vi.mocked(client.filesPreview).mockImplementationOnce(() => old.promise);
    const pending = store.selectEntry(entry("a.txt")); await store.selectEntry(entry("b.txt")); old.resolve(preview("a.txt")); await pending;
    expect(store.preview?.relativePath).toBe("b.txt");
  });
  it("discards directory and preview responses after repository generation changes", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    const old = deferred<RepositoryFilePreview>(); vi.mocked(client.filesPreview).mockImplementationOnce(() => old.promise);
    const pending = store.selectEntry(entry("a.txt")); useRepositoryStore().generation++;
    old.resolve(preview("a.txt")); await pending; expect(store.preview).toBeUndefined();
  });
  it("replaces expired pagination instead of appending incompatible pages", async () => {
    const client = setup(); const store = useFilesStore(); vi.mocked(client.filesList).mockResolvedValueOnce({ ...page(), nextCursor: "cursor" });
    await store.ensureLoaded(repository.rootPath, 0);
    vi.mocked(client.filesList).mockRejectedValueOnce({ code: "staleFileOperation", message: "目录已变化" }).mockResolvedValueOnce(page("", [entry("new.txt")]));
    await store.loadMore(""); expect(store.directories[""]?.page?.entries.map(item => item.name)).toEqual(["new.txt"]);
  });
  it("prepares and cancels without writes and invalidates preparation when name changes", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    store.requestOperation({ kind: "createFile", parentDir: "", name: "one.txt" }); await store.prepare();
    expect(store.confirmation?.prepared?.token).toBe("prepared"); expect(client.filesExecute).not.toHaveBeenCalled();
    store.changeName("two.txt"); expect(store.confirmation?.prepared).toBeUndefined(); await store.confirm();
    store.cancelConfirmation(); expect(client.filesExecute).not.toHaveBeenCalled();
  });
  it("invalidates confirmations on selection, home, branch and refresh changes", async () => {
    setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    for (const change of [() => store.selectEntry(entry("a.txt")), () => { useUiStore().homeVisible = true; }, () => { useRepositoryStore().snapshot!.currentBranch = "other"; }, () => store.refresh()]) {
      useUiStore().homeVisible = false; store.requestOperation({ kind: "createFile", parentDir: "", name: "new.txt" }); await store.prepare(); await change(); expect(store.confirmation).toBeUndefined();
    }
  });
  it("blocks duplicate execution and repository entry points before the first await", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    store.requestOperation({ kind: "delete", relativePath: "a.txt" }); await store.prepare();
    const pending = deferred<FileMutationResult>(); vi.mocked(client.filesExecute).mockImplementationOnce(() => pending.promise);
    const executing = store.confirm(); expect(useRepositoryStore().navigationBusy).toBe(true);
    await store.confirm(); await useRepositoryStore().open("C:/other"); await useRepositoryStore().init("C:/other"); await useRepositoryStore().clone("url", "C:/other"); await useRepositoryStore().refresh();
    expect(client.filesExecute).toHaveBeenCalledTimes(1); expect(client.repositoryOpen).not.toHaveBeenCalled(); expect(client.repositoryInit).not.toHaveBeenCalled(); expect(client.repositoryClone).not.toHaveBeenCalled(); expect(client.repositoryRefresh).not.toHaveBeenCalled();
    pending.resolve(applied); await executing;
  });
  it("retains recovery and only refreshes after an applied action with refresh failure", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    store.requestOperation({ kind: "delete", relativePath: "a.txt" }); await store.prepare(); await store.confirm();
    expect(store.confirmation).toBeUndefined(); expect(store.result?.applied).toBe(true); expect(store.result?.recoveryPath).toContain("recovery");
    await store.confirm(); await store.retryRefresh(); expect(client.filesExecute).toHaveBeenCalledTimes(1); expect(client.repositoryRefresh).toHaveBeenCalledTimes(1); expect(store.result?.recoveryPath).toContain("recovery");
  });
  it("does not resurrect a deleted file from a read started before mutation", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    const old = deferred<RepositoryFilePreview>(); vi.mocked(client.filesPreview).mockImplementationOnce(() => old.promise);
    const reading = store.selectEntry(entry("a.txt")); store.requestOperation({ kind: "delete", relativePath: "a.txt" }); await store.prepare(); await store.confirm();
    old.resolve(preview("a.txt")); await reading; expect(store.preview).toBeUndefined(); expect(store.selectedEntry).toBeUndefined();
  });
  it("treats prototype directory names as literal entries", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0); await store.toggleDirectory("__proto__");
    expect(client.filesList).toHaveBeenCalledWith(repository.rootPath, "__proto__", undefined); expect(store.directories["__proto__"]?.page?.relativeDir).toBe("__proto__");
  });
  it("blocks mutations while retaining dirty conflict drafts", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    useConflictsStore().drafts = { retained: { dirty: true } as ConflictDraft };
    store.requestOperation({ kind: "delete", relativePath: "a.txt" }); await store.prepare(); expect(client.filesPrepare).not.toHaveBeenCalled(); expect(store.canMutate).toBe(false);
  });
  it("reloads files after the topbar repository refresh", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0); await store.selectEntry(entry("a.txt"));
    await useRepositoryStore().refresh(); expect(store.directories[""]?.page?.entries.length).toBe(2); expect(store.preview?.relativePath).toBe("a.txt"); expect(client.filesList).toHaveBeenCalledTimes(2);
  });
  it("retains expansion and selection when opening another repository fails", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0); await store.toggleDirectory("src"); await store.selectEntry(entry("a.txt"));
    vi.mocked(client.repositoryOpen).mockRejectedValue({ code: "invalidRepository", message: "bad root" });
    await useRepositoryStore().open("C:/bad").catch(() => undefined); await store.ensureLoaded(repository.rootPath, useRepositoryStore().generation);
    expect(store.selectedEntry?.relativePath).toBe("a.txt"); expect(store.expanded.has("src")).toBe(true);
    expect(store.preview?.relativePath).toBe("a.txt"); expect(store.directories.src?.page).toBeDefined();
  });
  it("does not replace a new selection with a delayed refresh selection", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0); await store.selectEntry(entry("a.txt"));
    const old = deferred<FileDirectoryPage>(); vi.mocked(client.filesList).mockImplementationOnce(() => old.promise);
    const pending = store.refresh(); await store.selectEntry(entry("b.txt")); old.resolve(page()); await pending;
    expect(store.selectedEntry?.relativePath).toBe("b.txt"); expect(store.preview?.relativePath).toBe("b.txt");
  });
  it("retains selected entries beyond the first page when refreshing", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0); await store.selectEntry(entry("z.txt"));
    vi.mocked(client.filesList).mockResolvedValueOnce({ ...page(), nextCursor: "page2", totalEntries: 3 }).mockResolvedValueOnce({ ...page("", [entry("z.txt")]), totalEntries: 3 });
    await store.refresh(); expect(store.selectedEntry?.relativePath).toBe("z.txt"); expect(store.preview?.relativePath).toBe("z.txt");
  });
  it("retains a rejected new operation form after an earlier applied operation", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0);
    store.requestOperation({ kind: "delete", relativePath: "a.txt" }); await store.prepare(); await store.confirm();
    store.requestOperation({ kind: "createFile", parentDir: "", name: "next.txt" }); await store.prepare();
    vi.mocked(client.filesExecute).mockRejectedValueOnce({ code: "staleFileOperation", message: "changed" }); await store.confirm();
    expect(store.confirmation?.intent).toMatchObject({ name: "next.txt" }); expect(store.confirmation?.prepared).toBeUndefined(); expect(store.error?.code).toBe("staleFileOperation");
  });
  it("loads the tree after successfully reopening the same repository", async () => {
    const client = setup(); const store = useFilesStore(); await store.ensureLoaded(repository.rootPath, 0); await store.selectEntry(entry("a.txt"));
    vi.mocked(client.repositoryOpen).mockResolvedValue(repository); await useRepositoryStore().open(repository.rootPath);
    expect(store.directories[""]?.page).toBeDefined(); expect(store.selectedEntry).toBeUndefined();
  });
});
