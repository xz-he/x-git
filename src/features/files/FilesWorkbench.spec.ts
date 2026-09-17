import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/App.vue";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests } from "@/lib/backend/client";
import type { BackendClient } from "@/lib/backend/client";
import type { RepositoryFileEntry } from "@/lib/backend/types";
import { useRepositoryStore } from "@/stores/repository";

const entries: RepositoryFileEntry[] = [{ name: "src", relativePath: "src", kind: "directory", byteLength: null, reason: null, gitStatus: null }, { name: "note.txt", relativePath: "note.txt", kind: "file", byteLength: 8, reason: null, gitStatus: "??" }, { name: "link", relativePath: "link", kind: "restricted", byteLength: null, reason: "链接不允许跟随", gitStatus: null }];
describe("repository files workbench", () => {
  let pinia: ReturnType<typeof createPinia>;
  beforeEach(() => { pinia = createPinia(); setActivePinia(pinia); });
  async function open() {
    const client = createBackendFixture({
      filesList: vi.fn(async (_root, relativeDir) => ({ relativeDir, token: "list", entries: relativeDir ? [] : entries, nextCursor: null, totalEntries: relativeDir ? 0 : entries.length })),
      filesPreview: vi.fn<BackendClient["filesPreview"]>(async (_root, relativePath) => ({ relativePath, token: "preview", kind: "text", text: "<script>safe</script>\n", byteLength: 23, bom: false, lineEnding: "lf", reason: null })),
      filesPrepare: vi.fn<BackendClient["filesPrepare"]>(async (_root, intent) => ({ intent, token: "prepared", sourcePath: "relativePath" in intent ? intent.relativePath : null, targetPath: intent.kind === "rename" ? intent.newName : "name" in intent ? intent.name : null, entryKind: "file", nodeCount: 3, fileCount: 2, directoryCount: 1, totalBytes: 1024 })),
      filesExecute: vi.fn<BackendClient["filesExecute"]>(async () => ({ applied: true, workspace: null, operationState: null, affectedDirectories: [""], selectedPath: null, recoveryPath: "C:/repo/.git/hq-git-file-recovery/id/payload", error: { code: "gitLocked", message: "已移动，刷新失败" } })),
    });
    setBackendClientForTests(client);
    useRepositoryStore().snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abcdef0", upstream: null, isClean: true, conflictCount: 0, changedFileCount: 0, remotes: [] };
    const wrapper = mount(App, { global: { plugins: [pinia], stubs: { ConflictEditor: { props: ["modelValue", "readonly", "label"], template: '<textarea :aria-label="label" :readonly="readonly" :value="modelValue" />' } } } });
    await flushPromises(); await wrapper.get('[data-testid="app-sidebar"] [data-view="files"]').trigger("click"); await flushPromises();
    return { wrapper, client };
  }
  it("opens files navigation, expands lazily and previews literal readonly text", async () => {
    const { wrapper, client } = await open(); expect(client.filesList).toHaveBeenCalledTimes(1);
    await wrapper.get('[aria-label="展开目录 src"]').trigger("click"); await flushPromises(); expect(client.filesList).toHaveBeenCalledTimes(2);
    await wrapper.get('[aria-label="选择文件 note.txt"]').trigger("click"); await flushPromises();
    expect(wrapper.get('[aria-label="文件只读预览"]').attributes("readonly")).toBeDefined(); expect(wrapper.find("script").exists()).toBe(false);
    wrapper.unmount();
  });
  it("creates only after reviewing exact target and explicit confirmation", async () => {
    const { wrapper, client } = await open(); await wrapper.get('[aria-label="新建文件"]').trigger("click");
    await wrapper.get('[aria-label="文件名称"]').setValue("new.txt"); await wrapper.get('[aria-label="预检查"]').trigger("click"); await flushPromises();
    expect(wrapper.get('[role="dialog"]').text()).toContain("new.txt"); expect(wrapper.get('[role="dialog"]').text()).toContain("C:/repo"); expect(client.filesExecute).not.toHaveBeenCalled();
    await wrapper.get('[aria-label="确认新建文件"]').trigger("click"); await flushPromises(); expect(client.filesExecute).toHaveBeenCalledWith("C:/repo", { intent: { kind: "createFile", parentDir: "", name: "new.txt" }, token: "prepared" }); wrapper.unmount();
  });
  it("shows rename source and target and cancels without writing", async () => {
    const { wrapper, client } = await open(); await wrapper.get('[aria-label="选择文件 note.txt"]').trigger("click"); await flushPromises(); await wrapper.get('[aria-label="重命名选中项"]').trigger("click");
    await wrapper.get('[aria-label="文件名称"]').setValue("renamed.txt"); await wrapper.get('[aria-label="预检查"]').trigger("click"); await flushPromises();
    expect(wrapper.get('[role="dialog"]').text()).toContain("note.txt"); expect(wrapper.get('[role="dialog"]').text()).toContain("renamed.txt"); await wrapper.get('[aria-label="取消"]').trigger("click"); expect(client.filesExecute).not.toHaveBeenCalled(); wrapper.unmount();
  });
  it("shows deletion counts and retains recovery path outside the closed confirmation", async () => {
    const { wrapper, client } = await open(); await wrapper.get('[aria-label="选择文件 note.txt"]').trigger("click"); await flushPromises(); await wrapper.get('[aria-label="删除选中项"]').trigger("click");
    await wrapper.get('[aria-label="预检查"]').trigger("click"); await flushPromises(); const dialog = wrapper.get('[role="alertdialog"]');
    expect(dialog.text()).toContain("2 个文件"); expect(dialog.text()).toContain("1 个目录"); expect(dialog.text()).toContain("忽略");
    await wrapper.get('[aria-label="确认移入恢复区"]').trigger("click"); await flushPromises(); expect(wrapper.find('[role="alertdialog"]').exists()).toBe(false); expect(wrapper.text()).toContain("hq-git-file-recovery/id/payload"); expect(wrapper.find('[aria-label="仅重试刷新"]').exists()).toBe(true); expect(client.filesExecute).toHaveBeenCalledTimes(1); wrapper.unmount();
  });
  it("restricts links to metadata without read or mutation controls", async () => {
    const { wrapper, client } = await open(); await wrapper.get('[aria-label="选择受限项 link"]').trigger("click"); await flushPromises(); expect(wrapper.text()).toContain("链接不允许跟随"); expect(client.filesPreview).not.toHaveBeenCalled(); expect(wrapper.find('[aria-label="重命名选中项"]').exists()).toBe(false); wrapper.unmount();
  });
});
