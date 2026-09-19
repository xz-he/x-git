import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ChangesList from "./ChangesList.vue";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { useTerminalStore } from "@/stores/terminal";
import { useUiStore } from "@/stores/ui";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { MutationWorkspace } from "@/lib/backend/types";
import { createBackendFixture } from "@/test/backend";

enableAutoUnmount(afterEach);
afterEach(() => { vi.useRealTimers(); document.body.innerHTML = ""; });
describe("file context actions", () => {
  let backend: BackendClient;
  beforeEach(() => {
    setActivePinia(createPinia()); backend = createBackendFixture(); setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: "C:/files", name: "files", currentBranch: "main", headShortHash: "aaaaaaa", isClean: false, changedFileCount: 2, conflictCount: 0, remotes: [], upstream: null };
    useChangesStore().snapshot = { stagedCount: 1, unstagedCount: 2, files: [
      { path: "both.ts", oldPath: null, indexStatus: "M", worktreeStatus: "M", staged: true, unstaged: true, conflict: false },
      { path: "tests/new file.ts", oldPath: null, indexStatus: "?", worktreeStatus: "?", staged: false, unstaged: true, conflict: false },
    ] };
  });
  function result(): MutationWorkspace { return { workspace: { repository: useRepositoryStore().snapshot!, changes: JSON.parse(JSON.stringify(useChangesStore().snapshot!)) }, operationState: { kind: "none", conflicts: [], abortAction: null } }; }
  function button(action: string): HTMLButtonElement { return document.querySelector(`[role="menuitem"][data-action="${action}"]`)!; }
  async function open(path = "tests/new file.ts", scope = "unstaged") {
    const wrapper = mount(ChangesList, { attachTo: document.body }); await flushPromises();
    await wrapper.get(`[data-change-key="${scope}:${path}"]`).trigger("contextmenu", { clientX: 100, clientY: 100 }); await flushPromises();
    return wrapper;
  }
  async function click(action: string) { button(action).click(); await flushPromises(); }
  it("opens clicked untracked file and disables operations needing committed content", async () => {
    await open();
    expect(button("untrack").disabled).toBe(true); expect(button("history").disabled).toBe(true); expect(button("restore").disabled).toBe(true);
    await click("open");
    expect(backend.filesOpen).toHaveBeenCalledExactlyOnceWith("C:/files", "tests/new file.ts", false);
  });
  it("stages only the clicked file and unstages according to group", async () => {
    vi.mocked(backend.changesStageFile).mockResolvedValue(result()); vi.mocked(backend.changesUnstageFile).mockResolvedValue(result());
    const wrapper = await open(); await click("stage");
    expect(backend.changesStageFile).toHaveBeenCalledExactlyOnceWith("C:/files", "tests/new file.ts");
    await wrapper.get('[data-change-key="staged:both.ts"]').trigger("contextmenu"); await flushPromises(); await click("stage");
    expect(backend.changesUnstageFile).toHaveBeenCalledExactlyOnceWith("C:/files", "both.ts");
  });
  it("previews and saves explicit ignore rule and scope without stopping tracking", async () => {
    vi.useFakeTimers();
    vi.mocked(backend.filesIgnorePreview).mockResolvedValue({ pattern: "*.ts", targetPath: "C:/files/.gitignore", tracked: true });
    vi.mocked(backend.filesIgnore).mockResolvedValue(result());
    await open("both.ts"); await click("ignore");
    const radio = document.querySelector<HTMLInputElement>('input[value="extension"]')!; radio.checked = true; radio.dispatchEvent(new Event("change", { bubbles: true }));
    document.querySelector<HTMLButtonElement>("#ignore-scope")!.click(); await flushPromises();
    document.querySelector<HTMLElement>('[role="option"][data-value="repository"]')!.click();
    await flushPromises(); await vi.advanceTimersByTimeAsync(200); await flushPromises();
    expect(document.body.textContent).toContain("不会停止跟踪");
    document.querySelector<HTMLButtonElement>('[aria-label="保存忽略规则"]')!.click(); await flushPromises();
    expect(backend.filesIgnore).toHaveBeenCalledExactlyOnceWith("C:/files", { relativePath: "both.ts", scope: "repository", rule: { kind: "extension" } });
    expect(backend.filesUntrack).not.toHaveBeenCalled();
  });
  it("requires confirmation before untrack, retains errors and closes stale context", async () => {
    vi.mocked(backend.filesUntrack).mockRejectedValue({ code: "gitCommandFailed", message: "index differs" });
    await open("both.ts"); await click("untrack");
    expect(backend.filesUntrack).not.toHaveBeenCalled();
    document.querySelector<HTMLButtonElement>('[aria-label="确认执行"]')!.click(); await flushPromises();
    expect(backend.filesUntrack).toHaveBeenCalledExactlyOnceWith("C:/files", "both.ts");
    expect(document.body.textContent).toContain("index differs");
    useRepositoryStore().generation++; await flushPromises();
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
  });
  it("prepares recoverable deletion and sends the reviewed token only after confirmation", async () => {
    const intent = { kind: "delete" as const, relativePath: "tests/new file.ts" };
    vi.mocked(backend.filesPrepare).mockResolvedValue({ intent, token: "reviewed", sourcePath: null, targetPath: null, entryKind: "file", nodeCount: 1, fileCount: 1, directoryCount: 0, totalBytes: 10 });
    vi.mocked(backend.filesExecute).mockResolvedValue({ applied: true, workspace: result().workspace, operationState: result().operationState, affectedDirectories: [], selectedPath: null, recoveryPath: "C:/files/recovery/one", error: null });
    await open(); await click("delete");
    expect(backend.filesExecute).not.toHaveBeenCalled();
    document.querySelector<HTMLButtonElement>('[aria-label="确认执行"]')!.click(); await flushPromises();
    expect(backend.filesExecute).toHaveBeenCalledExactlyOnceWith("C:/files", { intent, token: "reviewed" });
    expect(document.body.textContent).toContain("C:/files/recovery/one");
  });
  it("custom action drafts a quoted file command without running it", async () => {
    await open(); await click("custom");
    expect(useUiStore().activeView).toBe("terminal");
    expect(useTerminalStore().draft).toBe("git --literal-pathspecs diff -- 'tests/new file.ts'");
    expect(backend.terminalStart).not.toHaveBeenCalled();
  });
  it("supports keyboard context menu and escape", async () => {
    const wrapper = await open();
    document.querySelector('[role="menu"]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await flushPromises();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    await wrapper.get('[data-change-key="staged:both.ts"]').trigger("keydown", { key: "F10", shiftKey: true }); await flushPromises();
    expect(button("stage").textContent).toContain("Unstage");
  });
});
