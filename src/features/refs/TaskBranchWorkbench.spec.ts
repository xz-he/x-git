import { flushPromises, mount } from "@vue/test-utils";
import { selectOption } from "@/test/select";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import RefsList from "./RefsList.vue";
import ContextPanel from "@/components/layout/ContextPanel.vue";
import { useUiStore } from "@/stores/ui";
import CommitPanel from "@/features/changes/CommitPanel.vue";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { RepositorySnapshot, TaskBranchBinding, TaskBranchResult } from "@/lib/backend/types";
import { useRepositoryStore } from "@/stores/repository";
import { useChangesStore } from "@/stores/changes";
import { useTaskBranchesStore } from "@/stores/taskBranches";
import { useSettingsStore } from "@/stores/settings";
import { createBackendFixture } from "@/test/backend";

const repo: RepositorySnapshot = {
  rootPath: "C:/repo", name: "repo", currentBranch: "dev", headShortHash: "aaaaaaa",
  isClean: false, changedFileCount: 1, conflictCount: 0, upstream: null,
  remotes: [{ name: "origin", fetchUrl: "https://example.test/repo.git" }],
};
function binding(overrides: Partial<TaskBranchBinding> = {}): TaskBranchBinding {
  return { id: "task-1", rootPath: repo.rootPath, sourceBranch: "dev", targetBranch: "feature/R2026082681825-purchase-orders", ticket: "R2026082681825", description: "采购订单", mode: "remoteMaster", phase: "ready", sourceCommit: null, targetCommit: null, returnAfterSuccess: true, message: null, ...overrides };
}
function result(item: TaskBranchBinding): TaskBranchResult {
  return { bindings: [item], error: null, workspace: null };
}

describe("task branches", () => {
  let backend: BackendClient;
  let wrapper: ReturnType<typeof mount>;
  beforeEach(() => {
    setActivePinia(createPinia());
    useRepositoryStore().snapshot = { ...repo };
    useChangesStore().snapshot = { files: [], stagedCount: 1, unstagedCount: 0 };
    useChangesStore().commitMessage = "R2026082681825 新增采购订单";
    backend = createBackendFixture({
      repositoryRefresh: vi.fn(async () => ({ ...repo })),
      taskBranchesSnapshot: vi.fn(async () => [binding()]),
      taskBranchesCreate: vi.fn(async () => result(binding())),
      taskBranchesRun: vi.fn(async () => result(binding({ phase: "completed", sourceCommit: "b".repeat(40), targetCommit: "c".repeat(40) }))),
    });
    setBackendClientForTests(backend);
  });
  afterEach(() => { wrapper?.unmount(); vi.useRealTimers(); });

  async function openAutoSlugDialog() {
    vi.useFakeTimers();
    useSettingsStore().settings.apiKey = "test-key";
    wrapper = mount(RefsList, { props: { mode: "branches" } });
    await flushPromises();
    await wrapper.get('[aria-label="快速建分支"]').trigger("click");
    await wrapper.get('[aria-label="完整任务单号"]').setValue("R2026082681825");
  }
  const englishValue = () => (wrapper.get('[aria-label="英文描述"]').element as HTMLInputElement).value;

  it("unlinks only the selected task and keeps the commit draft", async () => {
    const other = binding({ id: "task-2", targetBranch: "feature/R2-other" });
    const third = binding({ id: "task-3", targetBranch: "feature/R3-third" });
    vi.mocked(backend.taskBranchesSnapshot).mockResolvedValue([binding(), other, third]);
    vi.mocked(backend.taskBranchesUnlink).mockImplementation(async () => {
      vi.mocked(backend.taskBranchesSnapshot).mockResolvedValue([binding(), third]);
      return [binding(), third];
    });
    wrapper = mount(CommitPanel);
    await flushPromises();
    await selectOption(wrapper, "提交目标任务分支", "task-2");
    await wrapper.get('[aria-label="解除关联"]').trigger("click");
    await flushPromises();
    expect(backend.taskBranchesUnlink).toHaveBeenCalledWith(repo.rootPath, "task-2");
    expect(wrapper.text()).not.toContain(other.targetBranch);
    expect(wrapper.text()).toContain(binding().targetBranch);
    expect(wrapper.get('[aria-label="提交目标任务分支"]').text()).toBe(binding().targetBranch);
    await useTaskBranchesStore().refresh();
    expect(useTaskBranchesStore().bindings.map((item) => item.id)).toEqual(["task-1", "task-3"]);
    expect(useChangesStore().commitMessage).toContain("新增采购订单");
    expect(backend.taskBranchesRun).not.toHaveBeenCalled();
    expect(backend.changesCommit).not.toHaveBeenCalled();
  });

  it("hides the task card after unlinking the last task", async () => {
    vi.mocked(backend.taskBranchesUnlink).mockResolvedValue([]);
    wrapper = mount(CommitPanel);
    await flushPromises();
    await wrapper.get('[aria-label="解除关联"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[aria-label="任务分支提交"]').exists()).toBe(false);
  });

  it("preserves the task and shows the error when unlink fails", async () => {
    vi.mocked(backend.taskBranchesUnlink).mockRejectedValue({ code: "io", message: "无法保存关联记录" });
    wrapper = mount(CommitPanel);
    await flushPromises();
    await wrapper.get('[aria-label="解除关联"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("无法保存关联记录");
    expect(wrapper.text()).toContain(binding().targetBranch);
    expect(useTaskBranchesStore().bindings).toHaveLength(1);
  });

  it("deduplicates unlink and ignores its response after a repository replacement", async () => {
    let resolve!: (items: TaskBranchBinding[]) => void;
    vi.mocked(backend.taskBranchesUnlink).mockImplementation(() => new Promise((next) => { resolve = next; }));
    wrapper = mount(CommitPanel);
    await flushPromises();
    const tasks = useTaskBranchesStore();
    const pending = tasks.unlink("task-1");
    await tasks.unlink("task-1");
    await flushPromises();
    expect(wrapper.get('[aria-label="解除关联"]').attributes()).toHaveProperty("disabled");
    expect(backend.taskBranchesUnlink).toHaveBeenCalledTimes(1);
    vi.mocked(backend.taskBranchesSnapshot).mockResolvedValue([]);
    useRepositoryStore().snapshot = { ...repo, rootPath: "C:/another" };
    useRepositoryStore().generation += 1;
    await flushPromises();
    resolve([binding()]);
    await pending;
    await flushPromises();
    expect(tasks.bindings).toEqual([]);
    expect(tasks.loadedRootPath).toBe("C:/another");
  });

  it.each(["branches", "changes"] as const)("previews and creates a task branch from the %s page", async view => {
    useUiStore().activeView = view;
    wrapper = view === "branches" ? mount(RefsList, { props: { mode: "branches" } }) : mount(ContextPanel, { props: { repository: repo } });
    await flushPromises();
    await wrapper.get('[aria-label="快速建分支"]').trigger("click");
    await flushPromises();
    await wrapper.get('[aria-label="完整任务单号"]').setValue("r2026082681825");
    await wrapper.get('[aria-label="英文描述"]').setValue("Purchase Orders");
    await wrapper.get('[aria-label="中文说明"]').setValue("采购订单");
    expect(wrapper.get('[aria-label="创建方式"]').text()).toBe("从远端 master 创建，留在开发分支");
    expect(wrapper.text()).toContain("feature/R2026082681825-purchase-orders");
    expect(wrapper.text()).toContain("R2026082681825 采购订单");
    expect(backend.taskBranchesCreate).not.toHaveBeenCalled();
    await wrapper.get('[aria-label="确认创建任务分支"]').trigger("click");
    await flushPromises();
    expect(backend.taskBranchesCreate).toHaveBeenCalledWith(repo.rootPath, {
      kind: "feature", ticket: "R2026082681825", slug: "purchase-orders", description: "采购订单",
      mode: "remoteMaster", remote: "origin", sourceBranch: "dev", expectedHead: "aaaaaaa",
    });
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
    expect(useUiStore().activeView).toBe(view);
    expect(wrapper.text()).toContain("feature/R2026082681825-purchase-orders");
  });

  it("fills the English slug from the Chinese description and lets manual edits take over", async () => {
    vi.mocked(backend.aiChat).mockResolvedValueOnce("fix-invoice-deduplication").mockResolvedValueOnce("video-task-type-filter-error");
    await openAutoSlugDialog();
    await wrapper.get('[aria-label="中文说明"]').setValue("修复跨境发票去重逻辑");
    expect(backend.aiChat).not.toHaveBeenCalled();
    expect(wrapper.get('[aria-label="确认创建任务分支"]').attributes()).toHaveProperty("disabled");
    await vi.advanceTimersByTimeAsync(700);
    expect(englishValue()).toBe("fix-invoice-deduplication");
    expect(backend.aiChat).toHaveBeenCalledWith(expect.any(String), [expect.objectContaining({ content: expect.stringContaining("修复跨境发票去重逻辑") })]);
    await wrapper.get('[aria-label="中文说明"]').setValue("视频任务类型筛选错误");
    expect(englishValue()).toBe("");
    await vi.advanceTimersByTimeAsync(700);
    expect(englishValue()).toBe("video-task-type-filter-error");
    await wrapper.get('[aria-label="英文描述"]').setValue("custom-slug");
    await wrapper.get('[aria-label="中文说明"]').setValue("修复视频任务类型筛选错误");
    await vi.advanceTimersByTimeAsync(700);
    expect(englishValue()).toBe("custom-slug");
    expect(backend.aiChat).toHaveBeenCalledTimes(2);
    vi.mocked(backend.aiChat).mockResolvedValueOnce("fix-video-filter");
    await wrapper.get('[aria-label="英文描述"]').setValue("");
    await vi.advanceTimersByTimeAsync(700);
    expect(englishValue()).toBe("fix-video-filter");
  });

  it("debounces edits and ignores cancelled translation responses", async () => {
    let resolve!: (value: string) => void;
    vi.mocked(backend.aiChat).mockImplementationOnce(() => new Promise(next => { resolve = next; })).mockResolvedValue("latest-description");
    await openAutoSlugDialog();
    await wrapper.get('[aria-label="中文说明"]').setValue("说明一");
    await vi.advanceTimersByTimeAsync(300);
    await wrapper.get('[aria-label="中文说明"]').setValue("说明二");
    await vi.advanceTimersByTimeAsync(700);
    expect(backend.aiChat).toHaveBeenCalledTimes(1);
    await wrapper.get('[aria-label="中文说明"]').setValue("最新说明");
    expect(backend.aiCancel).toHaveBeenCalledWith(vi.mocked(backend.aiChat).mock.calls[0]![0]);
    await vi.advanceTimersByTimeAsync(700);
    expect(backend.aiChat).toHaveBeenCalledTimes(1);
    resolve("stale-description");
    await flushPromises();
    expect(englishValue()).toBe("latest-description");
    expect(backend.aiChat).toHaveBeenCalledTimes(2);
  });

  it("preserves manual input when an in-flight translation finishes", async () => {
    let resolve!: (value: string) => void;
    vi.mocked(backend.aiChat).mockImplementationOnce(() => new Promise(next => { resolve = next; }));
    await openAutoSlugDialog();
    await wrapper.get('[aria-label="中文说明"]').setValue("中文说明");
    await vi.advanceTimersByTimeAsync(700);
    await wrapper.get('[aria-label="英文描述"]').setValue("manual-name");
    resolve("automatic-name");
    await flushPromises();
    expect(englishValue()).toBe("manual-name");
    expect(backend.aiCancel).toHaveBeenCalledTimes(1);
  });

  it("shows configuration and translation failures and allows retry", async () => {
    await openAutoSlugDialog();
    useSettingsStore().settings.apiKey = "";
    await wrapper.get('[aria-label="中文说明"]').setValue("中文说明");
    await vi.advanceTimersByTimeAsync(700);
    expect(wrapper.text()).toContain("请先在设置中配置 AI 服务");
    expect(backend.aiChat).not.toHaveBeenCalled();
    useSettingsStore().settings.apiKey = "test-key";
    vi.mocked(backend.aiChat).mockRejectedValueOnce({ code: "aiTransport", message: "连接失败" }).mockResolvedValueOnce("valid-slug");
    const retry = () => wrapper.findAll("button").find(button => button.text() === "重新生成")!;
    await retry().trigger("click");
    await vi.advanceTimersByTimeAsync(700);
    expect(wrapper.text()).toContain("连接失败");
    expect(englishValue()).toBe("");
    await retry().trigger("click");
    await vi.advanceTimersByTimeAsync(700);
    expect(englishValue()).toBe("valid-slug");
  });

  it.each(["建议使用 branch-name", "a".repeat(81), "", "feature/branch-name"])("rejects malformed AI output: %s", async reply => {
    vi.mocked(backend.aiChat).mockResolvedValue(reply);
    await openAutoSlugDialog();
    await wrapper.get('[aria-label="中文说明"]').setValue("中文说明");
    await vi.advanceTimersByTimeAsync(700);
    expect(englishValue()).toBe("");
    expect(wrapper.text()).toContain("AI 未返回有效的英文分支描述");
  });

  it("waits for IME composition and cancels translation on close", async () => {
    vi.mocked(backend.aiChat).mockImplementation(() => new Promise(() => {}));
    await openAutoSlugDialog();
    const input = wrapper.get('[aria-label="中文说明"]');
    (input.element as HTMLInputElement).value = "视频";
    await input.trigger("input", { isComposing: true });
    await vi.advanceTimersByTimeAsync(1000);
    expect(backend.aiChat).not.toHaveBeenCalled();
    await input.trigger("compositionend");
    await vi.advanceTimersByTimeAsync(700);
    expect(backend.aiChat).toHaveBeenCalledTimes(1);
    await wrapper.get('[data-action="cancel"]').trigger("click");
    expect(backend.aiCancel).toHaveBeenCalledTimes(1);
  });

  it("removes pending translation when the repository changes", async () => {
    await openAutoSlugDialog();
    await wrapper.get('[aria-label="中文说明"]').setValue("中文说明");
    useRepositoryStore().generation++;
    await flushPromises();
    await vi.advanceTimersByTimeAsync(700);
    expect(backend.aiChat).not.toHaveBeenCalled();
    expect(wrapper.find('[aria-label="英文描述"]').exists()).toBe(false);
  });

  it("validates ticket type and shows the hotfix preview without truncating the ticket", async () => {
    wrapper = mount(RefsList, { props: { mode: "branches" } });
    await flushPromises();
    await wrapper.get('[aria-label="快速建分支"]').trigger("click");
    await selectOption(wrapper, "任务类型", "hotfix");
    await selectOption(wrapper, "创建方式", "current");
    await wrapper.get('[aria-label="完整任务单号"]').setValue("R2026082681825");
    await wrapper.get('[aria-label="英文描述"]').setValue("fix-sku-bug");
    await wrapper.get('[aria-label="中文说明"]').setValue("修复 SKU");
    expect(wrapper.get('[aria-label="确认创建任务分支"]').attributes()).toHaveProperty("disabled");
    await wrapper.get('[aria-label="完整任务单号"]').setValue("B2026082681825");
    expect(wrapper.text()).toContain("hotfix/B2026082681825-fix-sku-bug");
    await wrapper.get('[aria-label="确认创建任务分支"]').trigger("click");
    await flushPromises();
    expect(backend.taskBranchesCreate).toHaveBeenCalledWith(repo.rootPath, expect.objectContaining({ mode: "current", kind: "hotfix", ticket: "B2026082681825" }));
  });

  it.each([true, false])("lets the user choose returnAfterSuccess=%s after clicking submit and pick", async (returnAfterSuccess) => {
    wrapper = mount(CommitPanel);
    await flushPromises();
    await wrapper.get('[aria-label="提交并移植"]').trigger("click");
    expect(backend.taskBranchesRun).not.toHaveBeenCalled();
    expect(wrapper.get('[role="dialog"]').text()).toContain(binding().targetBranch);
    await wrapper.get(`[aria-label="${returnAfterSuccess ? "成功后返回开发分支" : "成功后留在新分支"}"]`).setValue(true);
    await wrapper.get('[aria-label="确认提交并移植"]').trigger("click");
    await flushPromises();
    expect(backend.taskBranchesRun).toHaveBeenCalledWith(repo.rootPath, { id: "task-1", action: "commit", message: "R2026082681825 新增采购订单", returnAfterSuccess, expectedHead: "aaaaaaa" });
    expect(backend.changesCommit).not.toHaveBeenCalled();
    expect(useChangesStore().commitMessage).toBe("");
  });

  it("does nothing when submit and pick is cancelled", async () => {
    wrapper = mount(CommitPanel);
    await flushPromises();
    await wrapper.get('[aria-label="提交并移植"]').trigger("click");
    await wrapper.get('[data-action="cancel"]').trigger("click");
    expect(backend.taskBranchesRun).not.toHaveBeenCalled();
    expect(useChangesStore().commitMessage).toContain("新增采购订单");
  });

  it("retries the recorded commit without creating another commit", async () => {
    vi.mocked(backend.taskBranchesSnapshot).mockResolvedValue([binding({ phase: "pendingPick", sourceCommit: "b".repeat(40), message: "请先处理剩余变更" })]);
    wrapper = mount(CommitPanel);
    await flushPromises();
    expect(wrapper.text()).toContain("请先处理剩余变更");
    expect(wrapper.find('[aria-label="提交并移植"]').exists()).toBe(false);
    await wrapper.get('[aria-label="移植此提交"]').trigger("click");
    await wrapper.get('[aria-label="成功后留在新分支"]').setValue(true);
    await wrapper.get('[aria-label="确认移植此提交"]').trigger("click");
    await flushPromises();
    expect(backend.taskBranchesRun).toHaveBeenCalledWith(repo.rootPath, expect.objectContaining({ action: "pick", returnAfterSuccess: false }));
    expect(backend.changesCommit).not.toHaveBeenCalled();
    expect(useChangesStore().commitMessage).toContain("新增采购订单");
  });

  it("only returns when pick already succeeded", async () => {
    useRepositoryStore().snapshot!.currentBranch = binding().targetBranch;
    vi.mocked(backend.taskBranchesSnapshot).mockResolvedValue([binding({ phase: "pendingReturn", sourceCommit: "b".repeat(40), targetCommit: "c".repeat(40) })]);
    wrapper = mount(CommitPanel);
    await flushPromises();
    await wrapper.get('[aria-label="返回开发分支"]').trigger("click");
    await flushPromises();
    expect(backend.taskBranchesRun).toHaveBeenCalledWith(repo.rootPath, expect.objectContaining({ action: "return" }));
  });

  it("invalidates the confirmation after HEAD changes", async () => {
    wrapper = mount(CommitPanel);
    await flushPromises();
    await wrapper.get('[aria-label="提交并移植"]').trigger("click");
    useRepositoryStore().snapshot!.headShortHash = "ddddddd";
    await flushPromises();
    expect(wrapper.find('[aria-label="确认提交并移植"]').exists()).toBe(false);
    expect(backend.taskBranchesRun).not.toHaveBeenCalled();
  });

  it("shows transport failures and allows a state refresh instead of blind resubmission", async () => {
    vi.mocked(backend.taskBranchesRun).mockRejectedValueOnce({ code: "unexpected", message: "连接中断" });
    wrapper = mount(CommitPanel);
    await flushPromises();
    await wrapper.get('[aria-label="提交并移植"]').trigger("click");
    await wrapper.get('[aria-label="确认提交并移植"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("连接中断");
    expect(wrapper.find('[aria-label="确认提交并移植"]').exists()).toBe(false);
    expect(wrapper.find('[aria-label="刷新任务状态"]').exists()).toBe(true);
  });

  it("offers recovery when task loading fails in the branches view", async () => {
    vi.mocked(backend.taskBranchesSnapshot).mockRejectedValueOnce({ code: "io", message: "任务状态读取失败" });
    wrapper = mount(RefsList, { props: { mode: "branches" } });
    await flushPromises();
    expect(wrapper.text()).toContain("任务状态读取失败");
    await wrapper.get('[aria-label="重试读取任务分支"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[aria-label="快速建分支"]').attributes()).not.toHaveProperty("disabled");
    expect(backend.repositoryRefresh).toHaveBeenCalledWith(repo.rootPath);
  });

  it("does not duplicate a pending submission or permit repository replacement", async () => {
    let resolve!: (value: TaskBranchResult) => void;
    vi.mocked(backend.taskBranchesRun).mockImplementation(() => new Promise((next) => { resolve = next; }));
    wrapper = mount(CommitPanel);
    await flushPromises();
    const request = { id: "task-1", action: "commit" as const, message: "message", returnAfterSuccess: false, expectedHead: "aaaaaaa" };
    const first = useTaskBranchesStore().run(request);
    const second = useTaskBranchesStore().run(request);
    await useRepositoryStore().open("C:/another");
    expect(backend.repositoryOpen).not.toHaveBeenCalled();
    expect(backend.taskBranchesRun).toHaveBeenCalledTimes(1);
    resolve(result(binding({ phase: "completed", sourceCommit: "b".repeat(40) })));
    await Promise.all([first, second]);
  });

  it("refreshes real HEAD and branch after a lost mutation response", async () => {
    vi.mocked(backend.taskBranchesRun).mockRejectedValueOnce({ code: "unexpected", message: "连接中断" });
    vi.mocked(backend.repositoryRefresh).mockResolvedValue({ ...repo, currentBranch: binding().targetBranch, headShortHash: "ccccccc", isClean: true, changedFileCount: 0 });
    wrapper = mount(CommitPanel);
    await flushPromises();
    await wrapper.get('[aria-label="提交并移植"]').trigger("click");
    await wrapper.get('[aria-label="确认提交并移植"]').trigger("click");
    await flushPromises();
    vi.mocked(backend.taskBranchesSnapshot).mockResolvedValue([binding({ phase: "completed", sourceCommit: "b".repeat(40), targetCommit: "c".repeat(40), returnAfterSuccess: false })]);
    await wrapper.get('[aria-label="刷新任务状态"]').trigger("click");
    await flushPromises();
    expect(useRepositoryStore().snapshot?.currentBranch).toBe(binding().targetBranch);
    expect(useRepositoryStore().snapshot?.headShortHash).toBe("ccccccc");
    expect(useTaskBranchesStore().uncertain).toBe(false);
    expect(wrapper.find('[aria-label="提交并移植"]').exists()).toBe(false);
  });

  it("ignores a late binding response from a replaced repository", async () => {
    let resolve!: (value: TaskBranchBinding[]) => void;
    vi.mocked(backend.taskBranchesSnapshot).mockImplementationOnce(() => new Promise((next) => { resolve = next; })).mockResolvedValue([]);
    wrapper = mount(CommitPanel);
    useRepositoryStore().snapshot = { ...repo, rootPath: "C:/another" };
    useRepositoryStore().generation += 1;
    await flushPromises();
    resolve([binding()]);
    await flushPromises();
    expect(useTaskBranchesStore().bindings).toEqual([]);
    expect(wrapper.text()).not.toContain(binding().targetBranch);
  });
});
