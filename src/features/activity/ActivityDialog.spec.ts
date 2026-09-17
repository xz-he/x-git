import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ActivityDialog from "./ActivityDialog.vue";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { ActivityEntry } from "@/lib/backend/activity";
import { useRepositoryStore } from "@/stores/repository";
import { useActivityStore } from "@/stores/activity";

enableAutoUnmount(afterEach);
const entry: ActivityEntry = { id: "record-a", title: "批量暂存 · a.ts、b.ts", createdAt: 1, status: "success", message: "操作完成", rollbackKind: "index", rollbackReason: "恢复原暂存内容，保留工作区修改。", rollbackId: null };
describe("operation history dialog", () => {
  let backend: BackendClient;
  beforeEach(() => {
    setActivePinia(createPinia()); backend = createBackendFixture({ activityList: vi.fn(async () => [{ ...entry }]) }); setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "aaaaaaa", isClean: false, changedFileCount: 1, conflictCount: 0, remotes: [], upstream: null };
  });
  function open() { return mount(ActivityDialog, { global: { stubs: { Teleport: true } } }); }
  it("clicking a record reveals rollback, and cancel never executes it", async () => {
    const wrapper = open(); await flushPromises();
    expect(wrapper.find(".rollback").exists()).toBe(false);
    await wrapper.get(".record").trigger("click");
    expect(wrapper.get(".details").text()).toContain("保留工作区修改");
    await wrapper.get(".rollback").trigger("click");
    expect(wrapper.text()).toContain("确认回滚此操作");
    await wrapper.get('[aria-label="取消"]').trigger("click");
    expect(backend.activityRollback).not.toHaveBeenCalled();
  });
  it("confirms once, blocks navigation during rollback and refreshes cached views", async () => {
    let finish!: (value: any) => void;
    vi.mocked(backend.activityRollback).mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    const refresh = vi.spyOn(useRepositoryStore(), "refreshAfterTerminal").mockResolvedValue();
    const wrapper = open(); await flushPromises(); await wrapper.get(".record").trigger("click"); await wrapper.get(".rollback").trigger("click");
    await wrapper.get('[aria-label="确认回滚"]').trigger("click");
    expect(useRepositoryStore().navigationBusy).toBe(true);
    await wrapper.get('[aria-label="确认回滚"]').trigger("click");
    expect(backend.activityRollback).toHaveBeenCalledExactlyOnceWith("C:/repo", "record-a");
    vi.mocked(backend.activityList).mockResolvedValue([{ ...entry, rollbackId: "rollback-a", rollbackKind: null }]);
    finish({ workspace: { repository: useRepositoryStore().snapshot, changes: { files: [], stagedCount: 0, unstagedCount: 0 } }, operationState: { kind: "none", conflicts: [], abortAction: null } });
    await flushPromises();
    expect(refresh).toHaveBeenCalledExactlyOnceWith("C:/repo", 0);
    expect(wrapper.get(".rollback").attributes("disabled")).toBeDefined();
    expect(useActivityStore().submitting).toBe(false);
  });
  it("shows a failed rollback and disables unsupported records", async () => {
    vi.mocked(backend.activityRollback).mockRejectedValue({ code: "staleFileOperation", message: "仓库已有后续变化" });
    const wrapper = open(); await flushPromises(); await wrapper.get(".record").trigger("click"); await wrapper.get(".rollback").trigger("click");
    await wrapper.get('[aria-label="确认回滚"]').trigger("click"); await flushPromises();
    expect(wrapper.text()).toContain("仓库已有后续变化");
    await wrapper.get('[aria-label="取消"]').trigger("click");
    useActivityStore().entries = [{ ...entry, rollbackKind: null, status: "failed", rollbackReason: "失败操作不能回滚" }]; await flushPromises();
    expect(wrapper.get(".rollback").attributes("disabled")).toBeDefined();
  });
  it("clears old repository records and ignores delayed responses", async () => {
    let resolveOld!: (value: ActivityEntry[]) => void;
    vi.mocked(backend.activityList).mockImplementationOnce(() => new Promise(resolve => { resolveOld = resolve; })).mockResolvedValue([]);
    const wrapper = open(); await flushPromises();
    useRepositoryStore().snapshot!.rootPath = "C:/other"; useRepositoryStore().generation++;
    await flushPromises(); resolveOld([{ ...entry }]); await flushPromises();
    expect(wrapper.text()).toContain("暂无操作记录"); expect(wrapper.find(".record").exists()).toBe(false);
  });
});
