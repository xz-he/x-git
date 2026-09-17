import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ChangesList from "./ChangesList.vue";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { MutationWorkspace } from "@/lib/backend/types";
import { createBackendFixture } from "@/test/backend";

enableAutoUnmount(afterEach);
describe("batch stage and unstage", () => {
  let backend: BackendClient;
  beforeEach(() => {
    setActivePinia(createPinia()); backend = createBackendFixture(); setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: "C:/batch", name: "batch", currentBranch: "main", headShortHash: "aaaaaaa", isClean: false, changedFileCount: 3, conflictCount: 0, remotes: [], upstream: null };
    useChangesStore().snapshot = { stagedCount: 2, unstagedCount: 2, files: [
      { path: "both.ts", oldPath: null, indexStatus: "M", worktreeStatus: "M", staged: true, unstaged: true, conflict: false },
      { path: "staged.ts", oldPath: null, indexStatus: "M", worktreeStatus: " ", staged: true, unstaged: false, conflict: false },
      { path: "new.txt", oldPath: null, indexStatus: "?", worktreeStatus: "?", staged: false, unstaged: true, conflict: false },
    ] };
  });
  function result(): MutationWorkspace {
    return { workspace: { repository: useRepositoryStore().snapshot!, changes: structuredClone(JSON.parse(JSON.stringify(useChangesStore().snapshot!))) }, operationState: { kind: "none", conflicts: [], abortAction: null } };
  }
  it("stages the selected group with one request and keeps staged selections independent", async () => {
    vi.mocked(backend.changesStageFiles).mockResolvedValue(result());
    const wrapper = mount(ChangesList); await flushPromises();
    expect(wrapper.get('[aria-label="批量暂存"]').attributes('disabled')).toBeDefined();
    await wrapper.get('[aria-label="选择已暂存文件 staged.ts"]').setValue(true);
    await wrapper.get('[aria-label="选择未暂存文件 new.txt"]').setValue(true);
    expect((wrapper.get('[aria-label="全选未暂存文件"]').element as HTMLInputElement).indeterminate).toBe(true);
    await wrapper.get('[aria-label="全选未暂存文件"]').setValue(true);
    await wrapper.get('[aria-label="批量暂存"]').trigger('click'); await flushPromises();
    expect(backend.changesStageFiles).toHaveBeenCalledExactlyOnceWith('C:/batch', ['both.ts', 'new.txt']);
    expect(backend.changesStageFile).not.toHaveBeenCalled();
    expect(backend.changesFileDiff).not.toHaveBeenCalled();
    expect((wrapper.get('[aria-label="选择已暂存文件 staged.ts"]').element as HTMLInputElement).checked).toBe(true);
    expect((wrapper.get('[aria-label="全选未暂存文件"]').element as HTMLInputElement).checked).toBe(false);
  });
  it("unstages checked files using the same staged selection as AI review", async () => {
    vi.mocked(backend.changesUnstageFiles).mockResolvedValue(result());
    const wrapper = mount(ChangesList); await flushPromises();
    await wrapper.get('[aria-label="全选已暂存文件"]').setValue(true);
    await wrapper.get('[aria-label="批量取消暂存"]').trigger('click'); await flushPromises();
    expect(backend.changesUnstageFiles).toHaveBeenCalledExactlyOnceWith('C:/batch', ['both.ts', 'staged.ts']);
    expect(backend.changesUnstageFile).not.toHaveBeenCalled();
    expect(wrapper.get('[aria-label="批量取消暂存"]').attributes('disabled')).toBeDefined();
  });
  it("blocks duplicate batches while pending and retains failed selections for retry", async () => {
    let reject!: (reason: unknown) => void;
    vi.mocked(backend.changesStageFiles).mockImplementationOnce(() => new Promise((_, fail) => { reject = fail; }));
    const wrapper = mount(ChangesList); await flushPromises();
    await wrapper.get('[aria-label="全选未暂存文件"]').setValue(true);
    await wrapper.get('[aria-label="批量暂存"]').trigger('click');
    await wrapper.get('[aria-label="批量暂存"]').trigger('click');
    expect(backend.changesStageFiles).toHaveBeenCalledTimes(1);
    expect(wrapper.get('[aria-label="全选已暂存文件"]').attributes('disabled')).toBeDefined();
    reject({ code: 'gitLocked', message: 'index locked' }); await flushPromises();
    expect((wrapper.get('[aria-label="全选未暂存文件"]').element as HTMLInputElement).checked).toBe(true);
    expect(wrapper.get('[role="alert"]').text()).toContain('index locked');
  });
  it("prunes stale choices and clears both groups after repository replacement", async () => {
    const wrapper = mount(ChangesList); await flushPromises();
    await wrapper.get('[aria-label="全选未暂存文件"]').setValue(true);
    await wrapper.get('[aria-label="全选已暂存文件"]').setValue(true);
    useChangesStore().snapshot!.files = useChangesStore().snapshot!.files.filter(file => file.path !== 'new.txt');
    await flushPromises();
    expect(wrapper.find('[aria-label="选择未暂存文件 new.txt"]').exists()).toBe(false);
    useRepositoryStore().generation++; await flushPromises();
    expect((wrapper.get('[aria-label="全选未暂存文件"]').element as HTMLInputElement).checked).toBe(false);
    expect((wrapper.get('[aria-label="全选已暂存文件"]').element as HTMLInputElement).checked).toBe(false);
    await wrapper.get('[aria-label="批量暂存"]').trigger('click');
    expect(backend.changesStageFiles).not.toHaveBeenCalled();
  });
});
