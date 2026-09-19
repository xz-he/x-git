import { enableAutoUnmount, flushPromises, mount, DOMWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import RepositorySwitcher from "./RepositorySwitcher.vue";
import AppSidebar from "./AppSidebar.vue";
import { useUiStore } from "@/stores/ui";
import { useRepositoryStore } from "@/stores/repository";
import { useSettingsStore } from "@/stores/settings";
import { useRefsStore } from "@/stores/refs";
import { useConflictsStore } from "@/stores/conflicts";
import { setBackendClientForTests } from "@/lib/backend/client";
import { setDialogAdapterForTests } from "@/lib/backend/dialogs";
import { createBackendFixture } from "@/test/backend";

enableAutoUnmount(afterEach);
afterEach(() => vi.restoreAllMocks());
const snapshot = (rootPath = "D:/work/first") => ({ rootPath, name: rootPath.split('/').at(-1)!, currentBranch: "main", headShortHash: "abc1234", isClean: false, changedFileCount: 2, conflictCount: 0, remotes: [], upstream: null });
const page = () => new DOMWrapper(document.body);
const shortcut = () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, cancelable: true, bubbles: true }));

beforeEach(() => {
  setActivePinia(createPinia());
  setBackendClientForTests(createBackendFixture());
  useRepositoryStore().snapshot = snapshot();
  useSettingsStore().settings.recentRepoPaths = ["D:/work/first", String.raw`\\?\D:\work\first`, "D:/project/second", "D:/elsewhere/second"];
});

describe("repository switcher", () => {
  it("opens from the collapsed sidebar, deduplicates paths, searches and switches with keys", async () => {
    const backend = createBackendFixture({ repositoryOpen: vi.fn(async path => snapshot(path)) });
    setBackendClientForTests(backend);
    const switcher = mount(RepositorySwitcher);
    useUiStore().sidebarCollapsed = true;
    const sidebar = mount(AppSidebar, { attachTo: document.body });
    await sidebar.get('[aria-label="快速切换仓库"]').trigger('click'); await flushPromises();
    expect(page().findAll('[role="option"]')).toHaveLength(3);
    const input = page().get('[aria-label="搜索仓库名称或路径"]');
    expect(document.activeElement).toBe(input.element);
    await input.setValue("second");
    expect(page().findAll('[role="option"]')).toHaveLength(2);
    await input.trigger('keydown', { key: 'ArrowDown' });
    await input.trigger('keydown', { key: 'Enter' }); await flushPromises();
    expect(backend.repositoryOpen).toHaveBeenCalledWith('D:/elsewhere/second');
    expect(useUiStore().repositorySwitcherOpen).toBe(false);
    expect(useUiStore().homeVisible).toBe(false);
    expect(backend.changesDiscardFile).not.toHaveBeenCalled();
    switcher.unmount();
  });

  it("supports Ctrl+P, empty search, Esc focus restoration and listener cleanup", async () => {
    const focus = document.createElement('button'); document.body.append(focus); focus.focus();
    const wrapper = mount(RepositorySwitcher);
    shortcut(); await flushPromises();
    const input = page().get('input');
    await input.setValue('no matching repo');
    expect(page().text()).toContain('没有匹配的仓库');
    await input.trigger('keydown', { key: 'Enter' });
    expect(useUiStore().repositorySwitcherOpen).toBe(true);
    await input.trigger('keydown', { key: 'Escape' }); await flushPromises();
    expect(document.activeElement).toBe(focus);
    wrapper.unmount(); shortcut();
    expect(useUiStore().repositorySwitcherOpen).toBe(false); focus.remove();
  });

  it("blocks switching during a Git operation, retains current repository on failure, and retries", async () => {
    const backend = createBackendFixture({ repositoryOpen: vi.fn().mockRejectedValueOnce({ code: 'invalidRepository', message: '仓库目录不存在' }).mockImplementation(async path => snapshot(path)) });
    setBackendClientForTests(backend);
    mount(RepositorySwitcher); shortcut(); await flushPromises();
    useRefsStore().submitting = true; await flushPromises();
    const input = page().get('input'); await input.setValue('D:/project');
    await input.trigger('keydown', { key: 'Enter' });
    expect(backend.repositoryOpen).not.toHaveBeenCalled();
    expect(page().text()).toContain('有操作正在执行');
    useRefsStore().submitting = false;
    await input.trigger('keydown', { key: 'Enter' }); await flushPromises();
    expect(page().get('[role="alert"]').text()).toBe('仓库目录不存在');
    expect(useRepositoryStore().snapshot?.rootPath).toBe('D:/work/first');
    await input.trigger('keydown', { key: 'Enter' }); await flushPromises();
    expect(useRepositoryStore().snapshot?.rootPath).toBe('D:/project/second');
  });

  it("opens a directory from the picker and avoids reopening the current repository", async () => {
    const backend = createBackendFixture({ repositoryOpen: vi.fn(async path => snapshot(path)) });
    setBackendClientForTests(backend);
    setDialogAdapterForTests({ selectDirectory: vi.fn(async () => 'D:/new/repo') });
    mount(RepositorySwitcher); shortcut(); await flushPromises();
    await page().get('input').trigger('keydown', { key: 'Enter' }); await flushPromises();
    expect(backend.repositoryOpen).not.toHaveBeenCalled();
    shortcut(); await flushPromises();
    await page().get('[aria-label="打开其他仓库"]').trigger('click'); await flushPromises();
    expect(backend.repositoryOpen).toHaveBeenCalledWith('D:/new/repo');
  });

  it("respects a cancelled conflict-draft confirmation", async () => {
    const backend = createBackendFixture(); setBackendClientForTests(backend);
    vi.spyOn(useConflictsStore(), 'hasDirtyDrafts', 'get').mockReturnValue(true);
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    mount(RepositorySwitcher); shortcut(); await flushPromises();
    await page().get('input').setValue('D:/project');
    await page().get('input').trigger('keydown', { key: 'Enter' }); await flushPromises();
    expect(backend.repositoryOpen).not.toHaveBeenCalled();
    expect(useUiStore().repositorySwitcherOpen).toBe(true);
  });
});
