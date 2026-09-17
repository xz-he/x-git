import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { StashEntry } from "@/lib/backend/types";
import StashList from "@/features/stashes/StashList.vue";
import StashDetail from "@/features/stashes/StashDetail.vue";
import { useStashesStore } from "@/stores/stashes";
import { useRepositoryStore } from "@/stores/repository";
import { useOperationStore } from "@/stores/operation";
import { useChangesStore } from "@/stores/changes";
import { createBackendFixture } from "@/test/backend";

const entry: StashEntry = { selector: "stash@{0}", objectId: "a".repeat(40), branch: "feature/long-branch", description: "checkpoint".repeat(20), timestamp: "2026-09-10T10:00:00+08:00" };
describe("stash workbench", () => {
  let pinia: ReturnType<typeof createPinia>;
  let backend: BackendClient;
  beforeEach(() => {
    pinia = createPinia();
    setActivePinia(pinia);
    backend = createBackendFixture();
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "aaaaaaa", isClean: false, changedFileCount: 1, conflictCount: 0, remotes: [], upstream: null };
    useStashesStore().resetForRepository("C:/repo", 0);
  });
  function selected() {
    const store = useStashesStore();
    store.snapshot = { entries: [entry] };
    store.selectedEntry = entry;
    store.detail = { entry, files: [{ status: "A", path: "new file.txt", oldPath: null, additions: 1, deletions: 0, binary: false, untracked: true }] };
    return store;
  }

  it("excludes untracked by default and retains optional message on failure", async () => {
    vi.mocked(backend.stashCreate).mockRejectedValue({ code: "gitLocked", message: "locked" });
    const wrapper = mount(StashList, { attachTo: document.body, global: { plugins: [pinia] } });
    expect((wrapper.get('[aria-label="包含未跟踪文件"]').element as HTMLInputElement).checked).toBe(false);
    await wrapper.get('[aria-label="贮藏说明"]').setValue("checkpoint");
    await wrapper.get('[data-action="create-stash"]').trigger("click");
    await flushPromises();
    expect(backend.stashCreate).toHaveBeenCalledWith("C:/repo", { message: "checkpoint", includeUntracked: false });
    expect((wrapper.get('[aria-label="贮藏说明"]').element as HTMLInputElement).value).toBe("checkpoint");
    expect(wrapper.get('[role="alert"]').text()).toContain("locked");
    wrapper.unmount();
  });

  it("renders descriptions with full titles, branches and dates", () => {
    selected();
    const wrapper = mount(StashList, { global: { plugins: [pinia] } });
    expect(wrapper.get('.stash-description').attributes("title")).toBe(entry.description);
    expect(wrapper.text()).toContain(entry.branch);
    expect(wrapper.get("time").attributes("datetime")).toBe(entry.timestamp);
  });

  it("submits only checked paths and prevents empty partial stash", async () => {
    useChangesStore().snapshot = { files: [
      { path: "first.ts", oldPath: null, indexStatus: "M", worktreeStatus: "M", staged: true, unstaged: true, conflict: false },
      { path: "keep.ts", oldPath: null, indexStatus: " ", worktreeStatus: "M", staged: false, unstaged: true, conflict: false },
      { path: "new.txt", oldPath: null, indexStatus: "?", worktreeStatus: "?", staged: false, unstaged: true, conflict: false },
    ], stagedCount: 1, unstagedCount: 3 };
    vi.mocked(backend.stashCreate).mockRejectedValue({ code: "gitLocked", message: "locked" });
    const wrapper = mount(StashList, { attachTo: document.body, global: { plugins: [pinia] } });
    await wrapper.get('[aria-label="选择部分文件"]').setValue(true);
    expect(wrapper.get('[data-action="create-stash"]').attributes('disabled')).toBeDefined();
    expect(wrapper.find('[aria-label="贮藏文件 new.txt"]').exists()).toBe(false);
    await wrapper.get('[aria-label="贮藏文件 first.ts"]').setValue(true);
    await wrapper.get('form').trigger('submit'); await flushPromises();
    expect(backend.stashCreate).toHaveBeenLastCalledWith("C:/repo", { message: "", includeUntracked: false, paths: ["first.ts"] });
    expect(useStashesStore().selectedPaths).toEqual(["first.ts"]);
    await wrapper.get('[aria-label="包含未跟踪文件"]').setValue(true);
    await wrapper.get('[aria-label="贮藏文件 new.txt"]').setValue(true);
    await wrapper.get('form').trigger('submit'); await flushPromises();
    expect(backend.stashCreate).toHaveBeenLastCalledWith("C:/repo", { message: "", includeUntracked: true, paths: ["first.ts", "new.txt"] });
    await wrapper.get('[aria-label="包含未跟踪文件"]').setValue(false);
    expect(useStashesStore().selectedPaths).toEqual(["first.ts"]);
    await wrapper.get('[aria-label="全选贮藏文件"]').setValue(true);
    expect(useStashesStore().selectedPaths).toEqual(["first.ts", "keep.ts"]);
    await wrapper.get('[aria-label="全选贮藏文件"]').setValue(false);
    const calls = vi.mocked(backend.stashCreate).mock.calls.length;
    await wrapper.get('form').trigger('submit'); await flushPromises();
    expect(backend.stashCreate).toHaveBeenCalledTimes(calls);
    wrapper.unmount();
  });

  it("requires Pop confirmation and focuses Cancel", async () => {
    selected();
    const wrapper = mount(StashDetail, { attachTo: document.body, global: { plugins: [pinia] } });
    await wrapper.get('[data-action="pop-stash"]').trigger("click");
    await flushPromises();
    const dialog = wrapper.get('[role="dialog"]');
    expect(dialog.text()).toContain("仅在无冲突成功后移除此条贮藏");
    expect(document.activeElement).toBe(dialog.get('[data-action="cancel"]').element);
    expect(backend.stashPop).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("loads selected saved untracked file without stage controls", async () => {
    selected();
    vi.mocked(backend.stashFileDiff).mockResolvedValue({ path: "new file.txt", scope: "commit", binary: false, hunks: [{ index: 0, header: "@@ -0,0 +1 @@", lines: [{ kind: "addition", oldLine: null, newLine: 1, content: "saved text" }] }] });
    const wrapper = mount(StashDetail, { global: { plugins: [pinia] } });
    await wrapper.get('[aria-label="查看贮藏文件 new file.txt（未跟踪）"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[aria-label="贮藏文件差异"]').text()).toContain("saved text");
    expect(wrapper.text()).toContain("未跟踪");
    expect(wrapper.find('[aria-label="暂存块 1"]').exists()).toBe(false);
  });

  it("renders binary diff and local retry", async () => {
    const store = selected();
    store.fileDiff = { path: "image.png", scope: "commit", binary: true, hunks: [] };
    const wrapper = mount(StashDetail, { global: { plugins: [pinia] } });
    expect(wrapper.text()).toContain("二进制文件");
    store.detailError = { code: "staleStash", message: "贮藏已变化" };
    await flushPromises();
    expect(wrapper.find('[aria-label="重新读取贮藏"]').exists()).toBe(true);
    expect(wrapper.get('[data-action="pop-stash"]').attributes("disabled")).toBeDefined();
  });

  it("disables mutations for persistent conflicts and immediate submission", async () => {
    const store = selected();
    const wrapper = mount(StashDetail, { global: { plugins: [pinia] } });
    useOperationStore().state.conflicts = [{ path: "file.txt", status: "UU" }];
    await flushPromises();
    expect(wrapper.get('[data-action="apply-stash"]').attributes("disabled")).toBeDefined();
    useOperationStore().reset();
    store.submitting = true;
    await flushPromises();
    expect(wrapper.get('[data-action="pop-stash"]').attributes("disabled")).toBeDefined();
  });

  it("renders empty, loading and no-changes states", async () => {
    const store = useStashesStore();
    store.snapshot = { entries: [] };
    const wrapper = mount(StashList, { global: { plugins: [pinia] } });
    expect(wrapper.text()).toContain("暂无贮藏");
    store.loading = true;
    await flushPromises();
    expect(wrapper.text()).toContain("正在读取贮藏");
    store.loading = false;
    store.outcome = "noChanges";
    await flushPromises();
    expect(wrapper.get('[role="status"]').text()).toContain("没有可贮藏的变更");
  });
});
