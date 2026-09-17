import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "@/App.vue";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { RepositorySnapshot } from "@/lib/backend/types";
import { useRepositoryStore } from "@/stores/repository";
import { defaultSettings, useSettingsStore } from "@/stores/settings";
import { useUiStore } from "@/stores/ui";
import { createBackendFixture } from "@/test/backend";
import { setDialogAdapterForTests } from "@/lib/backend/dialogs";
import { useChangesStore } from "@/stores/changes";
import { useStashesStore } from "@/stores/stashes";

enableAutoUnmount(afterEach);
afterEach(async () => { await flushPromises(); });

const repository: RepositorySnapshot = {
  rootPath: "D:\\work\\hq-git",
  name: "hq-git",
  currentBranch: "main",
  headShortHash: "abc1234",
  isClean: false,
  changedFileCount: 3,
  conflictCount: 0,
  remotes: [],
  upstream: { name: "origin/main", ahead: 2, behind: 1 },
};

describe("application shell", () => {
  let pinia: ReturnType<typeof createPinia>;

  beforeEach(() => {
    pinia = createPinia();
    setActivePinia(pinia);
    setBackendClientForTests(createBackendFixture());
    useRepositoryStore().snapshot = repository;
  });

  it("renders the approved three-column workbench with a closed AI drawer", () => {
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    expect(wrapper.find('[data-testid="app-sidebar"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="context-panel"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="detail-panel"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="ai-drawer"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("代码");
    expect(wrapper.text()).toContain("变更");
    expect(wrapper.text()).toContain("工具");
  });

  it.each([
    [String.raw`\\?\D:\hq-project\fancyqube`, String.raw`D:\hq-project\fancyqube`],
    [String.raw`\\?\UNC\server\share\repo`, String.raw`\\server\share\repo`],
  ])("displays a readable repository path while retaining the backend path (%s)", async (rawPath, displayPath) => {
    const backend = createBackendFixture({ repositoryOpen: vi.fn(async () => ({ ...repository, rootPath: rawPath })) });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { ...repository, rootPath: rawPath };
    useUiStore().activeView = "changes";
    const wrapper = mount(App, { global: { plugins: [pinia] } });
    await flushPromises();
    const path = wrapper.get('[data-testid="context-panel"] .repo span');
    expect(path.text()).toBe(displayPath);
    expect(path.attributes("title")).toBe(displayPath);
    expect(useRepositoryStore().snapshot?.rootPath).toBe(rawPath);
    useUiStore().activeView = "files";
    await flushPromises();
    expect(wrapper.get('[aria-label="选择仓库根目录"]').attributes("title")).toBe(displayPath);
    useSettingsStore().settings.recentRepoPaths = [rawPath];
    await wrapper.get('[aria-label="返回首页"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[aria-label="当前仓库"] span').text()).toBe(displayPath);
    expect(wrapper.get('.recent-open small').text()).toBe(displayPath);
    expect(wrapper.get('.recent-open').attributes("title")).toBe(displayPath);
    await wrapper.get('.recent-open').trigger("click");
    await flushPromises();
    expect(backend.repositoryOpen).toHaveBeenCalledWith(rawPath);
    wrapper.unmount();
  });

  it("returns home and resumes without clearing the repository or drafts", async () => {
    const changes = useChangesStore();
    changes.commitMessage = "unfinished commit";
    useStashesStore().message = "unfinished stash";
    const wrapper = mount(App, { global: { plugins: [pinia] } });
    await flushPromises();
    await wrapper.get('[aria-label="返回首页"]').trigger("click");
    expect(wrapper.find('[aria-label="仓库入口"]').exists()).toBe(true);
    expect(useRepositoryStore().snapshot).toEqual(repository);
    await wrapper.get('[aria-label="继续当前仓库"]').trigger("click");
    expect(wrapper.find('[data-testid="app-sidebar"]').exists()).toBe(true);
    expect(changes.commitMessage).toBe("unfinished commit");
    expect(useStashesStore().message).toBe("unfinished stash");
  });

  it("keeps automatic reopening and still allows returning home afterwards", async () => {
    useRepositoryStore().snapshot = undefined;
    const backend = createBackendFixture({
      settingsLoad: vi.fn(async () => ({ settings: defaultSettings({ lastRepoPath: repository.rootPath }) })),
      repositoryOpen: vi.fn(async () => repository),
    });
    setBackendClientForTests(backend);
    const wrapper = mount(App, { global: { plugins: [pinia] } });
    await flushPromises();
    expect(backend.repositoryOpen).toHaveBeenCalledWith(repository.rootPath);
    await wrapper.get('[aria-label="返回首页"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[aria-label="仓库入口"]').exists()).toBe(true);
    expect(backend.repositoryOpen).toHaveBeenCalledTimes(1);
  });

  it("switches through the directory dialog and cancellation keeps the workspace", async () => {
    const selectDirectory = vi.fn().mockResolvedValueOnce(null).mockResolvedValueOnce("C:/next");
    setDialogAdapterForTests({ selectDirectory });
    const backend = createBackendFixture({ repositoryOpen: vi.fn(async () => ({ ...repository, rootPath: "C:/next" })) });
    setBackendClientForTests(backend);
    const wrapper = mount(App, { global: { plugins: [pinia] } });
    await flushPromises();
    await wrapper.get('[aria-label="切换仓库"]').trigger("click");
    await flushPromises();
    expect(backend.repositoryOpen).not.toHaveBeenCalled();
    expect(useRepositoryStore().snapshot).toEqual(repository);
    await wrapper.get('[aria-label="切换仓库"]').trigger("click");
    await flushPromises();
    expect(backend.repositoryOpen).toHaveBeenCalledWith("C:/next");
    expect(useRepositoryStore().snapshot?.rootPath).toBe("C:/next");
  });

  it.each([false, true])("locks the workspace while switching, including home resume (%s)", async (fromHome) => {
    let finishOpen!: (snapshot: RepositorySnapshot) => void;
    setDialogAdapterForTests({ selectDirectory: vi.fn(async () => "C:/next") });
    setBackendClientForTests(createBackendFixture({
      repositoryOpen: vi.fn(() => new Promise<RepositorySnapshot>((resolve) => { finishOpen = resolve; })),
    }));
    const wrapper = mount(App, { global: { plugins: [pinia] } });
    await flushPromises();
    if (fromHome) {
      await wrapper.get('[aria-label="返回首页"]').trigger("click");
      await wrapper.get('[aria-label="打开本地仓库"]').trigger("click");
      await flushPromises();
      await wrapper.get('[aria-label="继续当前仓库"]').trigger("click");
    } else {
      await wrapper.get('[aria-label="切换仓库"]').trigger("click");
    }
    await flushPromises();
    expect(wrapper.get(".app-shell").attributes("inert")).toBeDefined();
    expect(wrapper.get('[aria-label="仓库加载状态"]').text()).toContain("正在切换仓库");
    finishOpen({ ...repository, rootPath: "C:/next" });
    await flushPromises();
    expect(wrapper.get(".app-shell").attributes("inert")).toBeUndefined();
    expect(useRepositoryStore().snapshot?.rootPath).toBe("C:/next");
  });

  it("shows switch errors and keeps the current repository", async () => {
    setDialogAdapterForTests({ selectDirectory: vi.fn(async () => "C:/invalid") });
    setBackendClientForTests(createBackendFixture({ repositoryOpen: vi.fn().mockRejectedValue({ code: "invalidRepository", message: "不是 Git 仓库。" }) }));
    const wrapper = mount(App, { global: { plugins: [pinia] } });
    await flushPromises();
    await wrapper.get('[aria-label="切换仓库"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toContain("不是 Git 仓库");
    expect(useRepositoryStore().snapshot).toEqual(repository);
    expect(wrapper.get(".app-shell").attributes("inert")).toBeUndefined();
  });

  it("disables switching during a mutation, including from the home screen", async () => {
    const wrapper = mount(App, { global: { plugins: [pinia] } });
    await flushPromises();
    useStashesStore().submitting = true;
    await flushPromises();
    expect(wrapper.get('[aria-label="切换仓库"]').attributes("disabled")).toBeDefined();
    await wrapper.get('[aria-label="返回首页"]').trigger("click");
    expect(wrapper.get('[aria-label="打开本地仓库"]').attributes("disabled")).toBeDefined();
    expect(wrapper.get('[aria-label="继续当前仓库"]').attributes("disabled")).toBeUndefined();
  });

  it("opens another repository from home and returns to its workspace", async () => {
    setDialogAdapterForTests({ selectDirectory: vi.fn(async () => "C:/next") });
    setBackendClientForTests(createBackendFixture({ repositoryOpen: vi.fn(async () => ({ ...repository, rootPath: "C:/next" })) }));
    const wrapper = mount(App, { global: { plugins: [pinia] } });
    await flushPromises();
    await wrapper.get('[aria-label="返回首页"]').trigger("click");
    await wrapper.get('[aria-label="打开本地仓库"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="app-sidebar"]').exists()).toBe(true);
    expect(useRepositoryStore().snapshot?.rootPath).toBe("C:/next");
  });

  it("opens and clamps the resizable AI drawer", async () => {
    const settings = useSettingsStore();
    settings.settings = defaultSettings({ aiDrawerWidth: 900 });
    const repositoryStore = useRepositoryStore();
    repositoryStore.snapshot = repository;
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    await wrapper.get('[aria-label="打开 AI 助手"]').trigger("click");
    await vi.waitFor(() =>
      expect(wrapper.get('[data-testid="ai-drawer"]').attributes("style")).toContain(
        "560px",
      ),
    );
  });

  it("gives every icon button an accessible name", () => {
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    for (const button of wrapper.findAll("button.icon-button")) {
      expect(button.attributes("aria-label")).toBeTruthy();
    }
  });

  it("keeps topbar and sidebar on one typed active view", async () => {
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    const historyButtons = wrapper.findAll('[data-view="history"]');
    expect(historyButtons).toHaveLength(2);
    await historyButtons[0]!.trigger("click");

    expect(useUiStore().activeView).toBe("history");
    expect(wrapper.findAll('[aria-current="page"]')).toHaveLength(2);
  });

  it("routes sidebar remote tools through the shared remote workbench", async () => {
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    await wrapper.get('[aria-label="打开拉取"]').trigger("click");

    expect(useUiStore().activeView).toBe("remotes");
    await vi.waitFor(() =>
      expect(wrapper.find('[aria-label="确认拉取"]').exists()).toBe(true),
    );
  });

  it("opens settings and applies the selected theme", async () => {
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    await wrapper.get('[aria-label="打开设置"]').trigger("click");
    await wrapper.get('[aria-label="使用深色主题"]').trigger("click");

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(useSettingsStore().settings.theme).toBe("dark");
  });
});
