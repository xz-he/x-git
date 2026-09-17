import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "@/App.vue";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import { setDialogAdapterForTests, type DialogAdapter } from "@/lib/backend/dialogs";
import type { RepositorySnapshot } from "@/lib/backend/types";
import { defaultSettings, useSettingsStore } from "@/stores/settings";
import { createBackendFixture } from "@/test/backend";

function repositoryFixture(
  overrides: Partial<RepositorySnapshot> = {},
): RepositorySnapshot {
  return {
    rootPath: "D:\\work\\repo",
    name: "repo",
    currentBranch: "main",
    headShortHash: "abc1234",
    isClean: true,
    changedFileCount: 0,
    conflictCount: 0,
    remotes: [],
    upstream: null,
    ...overrides,
  };
}

describe("repository welcome", () => {
  let backend: BackendClient;
  let dialogs: DialogAdapter;
  let pinia: ReturnType<typeof createPinia>;

  beforeEach(() => {
    pinia = createPinia();
    setActivePinia(pinia);
    backend = createBackendFixture();
    dialogs = { selectDirectory: vi.fn() };
    setBackendClientForTests(backend);
    setDialogAdapterForTests(dialogs);
  });

  it("opens a selected directory and shows one coherent repository snapshot", async () => {
    vi.mocked(dialogs.selectDirectory).mockResolvedValue("D:\\work\\repo");
    vi.mocked(backend.repositoryOpen).mockResolvedValue(repositoryFixture());
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    await wrapper.get('[aria-label="打开本地仓库"]').trigger("click");
    await flushPromises();

    expect(backend.repositoryOpen).toHaveBeenCalledWith("D:\\work\\repo");
    expect(wrapper.text()).toContain("repo");
    expect(wrapper.find('[data-testid="detail-panel"]').exists()).toBe(true);
  });

  it("does nothing when directory selection is cancelled", async () => {
    vi.mocked(dialogs.selectDirectory).mockResolvedValue(null);
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    await wrapper.get('[aria-label="打开本地仓库"]').trigger("click");
    await flushPromises();

    expect(backend.repositoryOpen).not.toHaveBeenCalled();
  });

  it("shows structured failures with expandable diagnostics", async () => {
    vi.mocked(dialogs.selectDirectory).mockResolvedValue("D:\\invalid");
    vi.mocked(backend.repositoryOpen).mockRejectedValue({
      code: "invalidRepository",
      message: "所选目录不是 Git 仓库。",
      diagnostics: "fatal: not a git repository",
    });
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    await wrapper.get('[aria-label="打开本地仓库"]').trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("所选目录不是 Git 仓库。");
    expect(wrapper.get("details").text()).toContain("fatal: not a git repository");
  });

  it("reopens a recent repository", async () => {
    const settings = useSettingsStore();
    settings.settings = defaultSettings({
      recentRepoPaths: ["D:\\recent\\repo"],
    });
    vi.mocked(backend.repositoryOpen).mockResolvedValue(
      repositoryFixture({ rootPath: "D:\\recent\\repo" }),
    );
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    await wrapper.get("button.recent-open").trigger("click");
    await flushPromises();

    expect(backend.repositoryOpen).toHaveBeenCalledWith("D:\\recent\\repo");
  });

  it("validates clone URL and target before invoking the backend", async () => {
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    await wrapper.get('[aria-label="克隆远程仓库"]').trigger("click");
    await wrapper.get('[aria-label="开始克隆"]').trigger("click");

    expect(wrapper.text()).toContain("请输入仓库地址");
    expect(backend.repositoryClone).not.toHaveBeenCalled();
  });

  it("opens settings before a repository is selected", async () => {
    const wrapper = mount(App, { global: { plugins: [pinia] } });

    await wrapper.get('[aria-label="打开设置"]').trigger("click");

    expect(wrapper.find('[aria-label="使用系统主题"]').exists()).toBe(true);
  });
});
