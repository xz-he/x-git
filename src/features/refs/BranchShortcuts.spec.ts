import { flushPromises, mount } from "@vue/test-utils";
import { selectOption } from "@/test/select";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "@/App.vue";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { RefsMutationResult, RefsSnapshot, RepositorySnapshot } from "@/lib/backend/types";
import { useRefsStore } from "@/stores/refs";
import { useRepositoryStore } from "@/stores/repository";
import { useOperationStore } from "@/stores/operation";
import { useHistoryStore } from "@/stores/history";
import { useUiStore } from "@/stores/ui";
import { createBackendFixture } from "@/test/backend";

const repository: RepositorySnapshot = {
  rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "aaaaaaa",
  isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null,
};
function snapshot(): RefsSnapshot {
  const tip = { fullHash: "a".repeat(40), shortHash: "aaaaaaa", subject: "topic", author: "Test", authoredAt: "2026-09-11" };
  return {
    localBranches: ["main", "topic"].map((name) => ({ name, fullName: `refs/heads/${name}`, kind: "local", current: name === "main", tip })),
    remoteBranches: [{ name: "origin/topic", fullName: "refs/remotes/origin/topic", kind: "remote", current: false, tip }], tags: [],
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}
function result(): RefsMutationResult {
  return { workspace: { repository, changes: { files: [], stagedCount: 0, unstagedCount: 0 } }, refs: snapshot(), operationState: { kind: "none", conflicts: [], abortAction: null } };
}

describe("branch sidebar shortcuts", () => {
  let backend: BackendClient;
  let wrapper: ReturnType<typeof mount>;
  beforeEach(() => {
    const pinia = createPinia();
    setActivePinia(pinia);
    backend = createBackendFixture({ refsSnapshot: vi.fn(async () => snapshot()), refsMerge: vi.fn(async () => result()), refsRebase: vi.fn(async () => result()) });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { ...repository };
    wrapper = mount(App, { attachTo: document.body, global: { plugins: [pinia] } });
  });
  afterEach(() => wrapper.unmount());

  it.each(["合并", "变基"])("opens %s with explicit context and an empty eligible-target selector without mutating", async (label) => {
    await flushPromises();
    expect(backend.refsSnapshot).not.toHaveBeenCalled();
    await wrapper.get(`[aria-label="打开${label}"]`).trigger("click");
    await flushPromises();
    expect(useUiStore().activeView).toBe("branches");
    const dialog = wrapper.get('[role="dialog"]');
    expect(dialog.text()).toContain("C:/repo");
    expect(dialog.text()).toContain("main");
    const select = dialog.get(label === "合并" ? '[aria-label="合并源分支"]' : '[aria-label="目标分支"]');
    if (label === "合并") {
      expect(select.element).toHaveProperty("value", "");
      expect(dialog.get('[aria-label="合并目标分支"]').element).toHaveProperty("value", "main");
    } else expect(select.text()).toBe("选择目标分支");
    expect(dialog.get('.primary-button').attributes()).toHaveProperty("disabled");
    expect(document.activeElement).toBe(dialog.get('[data-action="cancel"]').element);
    await selectOption(dialog, label === "合并" ? "合并源分支" : "目标分支", label === "合并" ? "topic" : "refs/heads/topic");
    expect(backend.refsMerge).not.toHaveBeenCalled();
    expect(backend.refsRebase).not.toHaveBeenCalled();
    await dialog.get('[data-action="cancel"]').trigger("click");
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
  });

  it.each(["合并", "变基"])("shares %s target confirmation with branch detail and preserves failures", async (label) => {
    useUiStore().openView("branches");
    await flushPromises();
    await wrapper.get('[aria-label="查看分支 topic"]').trigger("click");
    await wrapper.get(`[aria-label="打开${label}"]`).trigger("click");
    await flushPromises();
    const selector = label === "合并" ? '[aria-label="合并源分支"]' : '[aria-label="目标分支"]';
    if (label === "合并") expect(wrapper.get(selector).element).toHaveProperty("value", "topic");
    else expect(wrapper.get(selector).text()).toBe("topic");
    await wrapper.get('[data-action="cancel"]').trigger("click");
    await wrapper.get(`[aria-label="${label === "合并" ? "合并分支" : "变基到分支"} topic"]`).trigger("click");
    const method = label === "合并" ? backend.refsMerge : backend.refsRebase;
    vi.mocked(method).mockRejectedValueOnce({ code: "unexpected", message: "Branch action failed" });
    await wrapper.get('[role="dialog"] .primary-button').trigger("click");
    await flushPromises();
    if (label === "合并") expect(method).toHaveBeenCalledWith("C:/repo", "topic", "main");
    else expect(method).toHaveBeenCalledWith("C:/repo", "topic");
    expect(wrapper.get('[role="dialog"] [role="alert"]').text()).toContain("Branch action failed");
    if (label === "合并") expect(wrapper.get(selector).element).toHaveProperty("value", "topic");
    else expect(wrapper.get(selector).text()).toBe("topic");
    await wrapper.get('[role="dialog"] .primary-button').trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
  });

  it("shows loading, failure with retry, and no eligible targets", async () => {
    const pending = deferred<RefsSnapshot>();
    vi.mocked(backend.refsSnapshot).mockReturnValueOnce(pending.promise);
    await wrapper.get('[aria-label="打开合并"]').trigger("click");
    expect(wrapper.get('[role="dialog"]').text()).toContain("正在读取分支");
    pending.resolve({ ...snapshot(), localBranches: [snapshot().localBranches[0]!] });
    await flushPromises();
    expect(wrapper.get('[role="dialog"]').text()).toContain("没有可用的目标分支");
    await wrapper.get('[data-action="cancel"]').trigger("click");
    useRefsStore().resetForRepository(repository.rootPath, 0);
    vi.mocked(backend.refsSnapshot).mockRejectedValueOnce({ code: "unexpected", message: "Cannot read refs" });
    await wrapper.get('[aria-label="打开变基"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[role="dialog"] [role="alert"]').text()).toContain("Cannot read refs");
    await wrapper.get('[aria-label="重试读取分支"]').trigger("click");
    await flushPromises();
    await wrapper.get('[aria-label="目标分支"]').trigger("click"); await flushPromises();
    expect(document.querySelector('[role="listbox"]')?.textContent).toContain("topic");
  });

  it("selects both merge branches with suggestions and rejects identical or unknown destinations", async () => {
    await wrapper.get('[aria-label="打开合并"]').trigger("click");
    await flushPromises();
    const dialog = wrapper.get('[role="dialog"]');
    await dialog.get('[aria-label="合并源分支"]').setValue("main");
    expect(dialog.get('.primary-button').attributes()).toHaveProperty("disabled");
    await dialog.get('[aria-label="合并目标分支"]').setValue("missing");
    await useRefsStore().confirmIntegration();
    expect(backend.refsMerge).not.toHaveBeenCalled();
    await dialog.get('[aria-label="合并目标分支"]').setValue("topic");
    expect(dialog.text()).toContain("将 main 合并到 topic");
    await dialog.get('[aria-label="合并目标分支"]').trigger("click");
    expect(document.querySelector('[role="listbox"]')?.textContent).toContain('main');
    await dialog.get('[aria-label="合并目标分支"]').trigger("keydown", { key: "Escape" });
    await dialog.get('.primary-button').trigger('click');
    await flushPromises();
    expect(backend.refsMerge).toHaveBeenCalledExactlyOnceWith("C:/repo", "main", "topic");
  });

  it.each(["view", "home", "repository"])("discards pending shortcuts after changing %s", async (change) => {
    const pending = deferred<RefsSnapshot>();
    vi.mocked(backend.refsSnapshot).mockReturnValueOnce(pending.promise);
    await wrapper.get('[aria-label="打开合并"]').trigger("click");
    if (change === "view") useUiStore().openView("history");
    if (change === "home") useUiStore().homeVisible = true;
    if (change === "repository") useRepositoryStore().generation += 1;
    await flushPromises();
    pending.resolve(snapshot());
    await flushPromises();
    useUiStore().homeVisible = false;
    useUiStore().openView("branches");
    await flushPromises();
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
    expect(backend.refsMerge).not.toHaveBeenCalled();
  });

  it("blocks existing repository conflicts before operation state has loaded", async () => {
    useRepositoryStore().snapshot!.conflictCount = 1;
    await flushPromises();
    expect(useOperationStore().isBlocked).toBe(false);
    expect(wrapper.get('[aria-label="打开合并"]').attributes()).toHaveProperty("disabled");
    expect(wrapper.get('[aria-label="打开变基"]').attributes()).toHaveProperty("disabled");
    useRefsStore().requestIntegration("merge");
    expect(useRefsStore().integrationRequest).toBeUndefined();
    await expect(useRefsStore().merge("topic")).rejects.toMatchObject({ code: "gitOperationInProgress" });
    expect(backend.refsMerge).not.toHaveBeenCalled();
  });

  it("blocks conflicts, independent operations, stale targets and synchronous duplicate confirmation", async () => {
    await wrapper.get('[aria-label="打开合并"]').trigger("click");
    await flushPromises();
    await wrapper.get('[aria-label="合并源分支"]').setValue("topic");
    useHistoryStore().submitting = true;
    await flushPromises();
    expect(wrapper.get('[role="dialog"] .primary-button').attributes()).toHaveProperty("disabled");
    await useRefsStore().confirmIntegration();
    expect(backend.refsMerge).not.toHaveBeenCalled();
    useHistoryStore().submitting = false;
    useOperationStore().state = { kind: "merge", conflicts: [], abortAction: "merge" };
    await flushPromises();
    expect(wrapper.get('[role="dialog"] .primary-button').attributes()).toHaveProperty("disabled");
    await useRefsStore().confirmIntegration();
    expect(backend.refsMerge).not.toHaveBeenCalled();
    useOperationStore().reset();
    useRefsStore().snapshot!.localBranches = [snapshot().localBranches[0]!];
    await flushPromises();
    await useRefsStore().confirmIntegration();
    expect(backend.refsMerge).not.toHaveBeenCalled();
    useRefsStore().snapshot = snapshot();
    const pending = deferred<RefsMutationResult>();
    vi.mocked(backend.refsMerge).mockReturnValue(pending.promise);
    const first = useRefsStore().confirmIntegration();
    const second = useRefsStore().confirmIntegration();
    expect(backend.refsMerge).toHaveBeenCalledTimes(1);
    pending.resolve(result());
    await Promise.all([first, second]);
  });
});
