import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import NoiseCleanup from "./NoiseCleanup.vue";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import { useRepositoryStore } from "@/stores/repository";
import { useChangesStore } from "@/stores/changes";

const candidates = [
  { path: "staged.py", staged: true, fingerprint: "staged-token" },
  { path: "工作区.py", staged: false, fingerprint: "work-token" },
];
enableAutoUnmount(afterEach);
describe("newline cleanup", () => {
  let backend: BackendClient;
  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture({ changesScanNoise: vi.fn(async () => ({ candidates, skipped: [{ path: "real.py", reason: "包含内容差异" }] })) });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: "D:/repo", name: "repo", currentBranch: "main", headShortHash: "abc1234", isClean: false, changedFileCount: 3, conflictCount: 0, remotes: [], upstream: null };
    useChangesStore().snapshot = { stagedCount: 1, unstagedCount: 2, files: candidates.map(file => ({ path: file.path, oldPath: null, staged: file.staged, unstaged: !file.staged, indexStatus: file.staged ? "M" : " ", worktreeStatus: file.staged ? " " : "M", conflict: false })) };
  });
  it("only restores confirmed selected candidates and refreshes the workspace", async () => {
    vi.mocked(backend.changesRestoreNoise).mockResolvedValue({ restored: ["staged.py"], skipped: [], workspace: { workspace: { repository: useRepositoryStore().snapshot!, changes: { stagedCount: 0, unstagedCount: 1, files: [] } }, operationState: { kind: "none", conflicts: [], abortAction: null } } });
    const wrapper = mount(NoiseCleanup);
    await wrapper.get('[aria-label="检测无实质变更"]').trigger("click");
    await flushPromises();
    expect(backend.changesRestoreNoise).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("real.py");
    expect(wrapper.text()).toContain("暂存区 + 工作区");
    await wrapper.get('[aria-label="还原 工作区.py"]').setValue(false);
    await wrapper.get('[aria-label="还原所选文件"]').trigger("click");
    await flushPromises();
    expect(backend.changesRestoreNoise).toHaveBeenCalledWith("D:/repo", [candidates[0]]);
    expect(useChangesStore().snapshot?.stagedCount).toBe(0);
    expect(wrapper.find('[role="alertdialog"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("已还原 1 个文件");
  });
  it("cancel and empty selection never mutate files", async () => {
    const wrapper = mount(NoiseCleanup);
    await wrapper.get('[aria-label="检测无实质变更"]').trigger("click");
    await flushPromises();
    for (const file of candidates) await wrapper.get(`[aria-label="还原 ${file.path}"]`).setValue(false);
    expect(wrapper.get('[aria-label="还原所选文件"]').attributes("disabled")).toBeDefined();
    await wrapper.get('[aria-label="取消"]').trigger("click");
    expect(backend.changesRestoreNoise).not.toHaveBeenCalled();
  });
  it("invalidates stale results and requires rescanning", async () => {
    vi.mocked(backend.changesRestoreNoise).mockRejectedValue({ code: "staleFileOperation", message: "文件已变化" });
    const wrapper = mount(NoiseCleanup);
    await wrapper.get('[aria-label="检测无实质变更"]').trigger("click");
    await flushPromises();
    await wrapper.get('[aria-label="还原所选文件"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toContain("重新检测");
    expect(wrapper.get('[aria-label="还原所选文件"]').attributes("disabled")).toBeDefined();
  });
  it("clears the dialog when the repository generation changes", async () => {
    const wrapper = mount(NoiseCleanup);
    await wrapper.get('[aria-label="检测无实质变更"]').trigger("click");
    await flushPromises();
    useRepositoryStore().generation += 1;
    await flushPromises();
    expect(wrapper.find('[role="alertdialog"]').exists()).toBe(false);
    expect(backend.changesRestoreNoise).not.toHaveBeenCalled();
  });
});
