import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ChangesList from "./ChangesList.vue";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import { useRepositoryStore } from "@/stores/repository";
import { useChangesStore } from "@/stores/changes";
import type { ChangeLineStat } from "@/lib/backend/types";
enableAutoUnmount(afterEach);
describe("changes table", () => {
  let backend: BackendClient;
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    backend = createBackendFixture({ changesLineStats: vi.fn(async () => [
      { path: "src/a.py", scope: "staged" as const, additions: 12, deletions: 3, binary: false },
      { path: "src/a.py", scope: "unstaged" as const, additions: 2, deletions: 0, binary: false },
      { path: "pic.png", scope: "unstaged" as const, additions: null, deletions: null, binary: true },
    ]) });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: "D:/repo", name: "repo", currentBranch: "main", headShortHash: "1234567", isClean: false, changedFileCount: 2, conflictCount: 0, remotes: [], upstream: null };
    useChangesStore().snapshot = { stagedCount: 1, unstagedCount: 2, files: [
      { path: "src/a.py", oldPath: null, indexStatus: "M", worktreeStatus: "M", staged: true, unstaged: true, conflict: false },
      { path: "pic.png", oldPath: null, indexStatus: "?", worktreeStatus: "?", staged: false, unstaged: true, conflict: false },
    ] };
  });
  afterEach(() => localStorage.clear());
  it("always displays the table and retains staged review and unstage actions", async () => {
    const wrapper = mount(ChangesList);
    await flushPromises();
    expect(wrapper.find('[aria-label="列表视图"]').exists()).toBe(false);
    expect(wrapper.find('[aria-label="表格视图"]').exists()).toBe(false);
    expect(backend.changesLineStats).toHaveBeenCalledTimes(1);
    const staged = wrapper.get('[data-change-key="staged:src/a.py"]').element.parentElement!;
    expect(staged.querySelector('.extension')?.textContent).toBe('.py');
    expect(staged.querySelector('.additions')?.textContent).toBe('12');
    expect(staged.querySelector('.deletions')?.textContent).toBe('3');
    const unstaged = wrapper.get('[data-change-key="unstaged:src/a.py"]').element.parentElement!;
    expect(unstaged.querySelector('.additions')?.textContent).toBe('2');
    expect(wrapper.text()).toContain('Binary');
    await wrapper.get('[aria-label="选择已暂存文件 src/a.py"]').setValue(true);
    expect(wrapper.text()).toContain('已选 1');
    expect(wrapper.find('[aria-label="取消暂存 src/a.py"]').exists()).toBe(true);
    expect(backend.changesUnstageFile).not.toHaveBeenCalled();
    expect(wrapper.find('.table-heading').exists()).toBe(true);
  });
  it("discards stale statistics when switching repositories", async () => {
    let finish!: (value: ChangeLineStat[]) => void;
    vi.mocked(backend.changesLineStats).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const wrapper = mount(ChangesList);
    await flushPromises();
    useRepositoryStore().snapshot = undefined;
    await flushPromises();
    finish([{ path: "src/a.py", scope: "staged", additions: 999, deletions: 9, binary: false }]);
    await flushPromises();
    expect(wrapper.text()).not.toContain('999');
  });
});
