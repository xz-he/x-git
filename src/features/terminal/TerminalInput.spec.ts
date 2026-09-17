import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests } from "@/lib/backend/client";
import { useRepositoryStore } from "@/stores/repository";
import { useTerminalStore } from "@/stores/terminal";
import TerminalInput from "./TerminalInput.vue";
import type { TerminalCompletion } from "@/lib/backend/types";

describe("terminal command input", () => {
  let backend: ReturnType<typeof createBackendFixture>;
  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture(); setBackendClientForTests(backend);
    useTerminalStore().draft = "git status";
    useRepositoryStore().snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abc", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null };
  });
  const suggestions = (values: string[]): TerminalCompletion => ({ start: 4, end: 6, items: values.map(value => ({ value, label: value, description: "Git 命令", kind: "command" })), hasMore: false });
  it("completes a single candidate with Tab without executing", async () => {
    vi.mocked(backend.terminalComplete).mockResolvedValue(suggestions(["status"]));
    const run = vi.spyOn(useTerminalStore(), "run").mockResolvedValue();
    const wrapper = mount(TerminalInput); const input = wrapper.get('input');
    await input.setValue("git st"); await input.trigger("keydown", { key: "Tab" }); await flushPromises();
    expect(useTerminalStore().draft).toBe("git status "); expect(run).not.toHaveBeenCalled();
    await input.trigger("keydown", { key: "Enter" }); expect(run).toHaveBeenCalledOnce(); wrapper.unmount();
  });
  it("cycles multiple matches with Tab/Shift+Tab and Enter only accepts a suggestion", async () => {
    vi.mocked(backend.terminalComplete).mockResolvedValue(suggestions(["stash", "status"]));
    const run = vi.spyOn(useTerminalStore(), "run").mockResolvedValue();
    const wrapper = mount(TerminalInput); const input = wrapper.get('input');
    await input.setValue("git st"); await input.trigger("keydown", { key: "Tab" }); await flushPromises();
    expect(useTerminalStore().draft).toBe("git sta");
    await input.trigger("keydown", { key: "Tab" });
    await input.trigger("keydown", { key: "Tab", shiftKey: true });
    await input.trigger("keydown", { key: "Enter" });
    expect(useTerminalStore().draft).toBe("git stash "); expect(run).not.toHaveBeenCalled();
    await input.trigger("keydown", { key: "Enter" }); expect(run).toHaveBeenCalledOnce(); wrapper.unmount();
  });
  it("replaces the whole token at the cursor and preserves following flags", async () => {
    vi.mocked(backend.terminalComplete).mockResolvedValue({ start: 8, end: 12, items: [{ value: '"中文 文件.txt"', label: "中文 文件.txt", description: "文件", kind: "file" }], hasMore: false });
    const wrapper = mount(TerminalInput); const input = wrapper.get('input');
    await input.setValue("git add test --verbose");
    (input.element as HTMLInputElement).setSelectionRange(10, 10);
    await input.trigger("keydown", { key: "Tab" }); await flushPromises();
    expect(useTerminalStore().draft).toBe('git add "中文 文件.txt" --verbose'); wrapper.unmount();
  });
  it("restores unfinished drafts after browsing history and respects IME", async () => {
    const store = useTerminalStore();
    store.history = [{ runId: "1", command: "git log", startedAt: 1, status: "completed", durationMs: 1 }];
    const run = vi.spyOn(store, "run").mockResolvedValue();
    const wrapper = mount(TerminalInput); const input = wrapper.get('input');
    await input.setValue("git diff --stat");
    await input.trigger("keydown", { key: "ArrowUp" }); expect(store.draft).toBe("git log");
    await input.trigger("keydown", { key: "ArrowDown" }); expect(store.draft).toBe("git diff --stat");
    await input.trigger("keydown", { key: "Enter", isComposing: true }); expect(run).not.toHaveBeenCalled(); wrapper.unmount();
  });
  it("discards a stale completion response after editing or leaving the input", async () => {
    let resolve!: (result: TerminalCompletion) => void;
    vi.mocked(backend.terminalComplete).mockReturnValueOnce(new Promise(next => { resolve = next; }));
    const wrapper = mount(TerminalInput); const input = wrapper.get('input');
    await input.setValue("git st"); await input.trigger("keydown", { key: "Tab" });
    await input.setValue("git diff"); resolve(suggestions(["status"])); await flushPromises();
    expect(useTerminalStore().draft).toBe("git diff"); expect(wrapper.find('[role="listbox"]').exists()).toBe(false); wrapper.unmount();
  });
  it("shows hints while typing and Tab accepts the only already-visible match", async () => {
    vi.useFakeTimers();
    try {
      vi.mocked(backend.terminalComplete).mockResolvedValue(suggestions(["status"]));
      const wrapper = mount(TerminalInput); const input = wrapper.get('input');
      await input.setValue("git st"); await vi.advanceTimersByTimeAsync(180); await flushPromises();
      expect(wrapper.get('[role="listbox"]').text()).toContain("status");
      await input.trigger("keydown", { key: "Tab" }); await flushPromises();
      expect(useTerminalStore().draft).toBe("git status "); wrapper.unmount();
    } finally { vi.useRealTimers(); }
  });
  it("expands the common prefix on the first Tab after automatic hints", async () => {
    vi.useFakeTimers();
    try {
      vi.mocked(backend.terminalComplete).mockResolvedValue(suggestions(["stash", "status"]));
      const wrapper = mount(TerminalInput); const input = wrapper.get('input');
      await input.setValue("git st"); await vi.advanceTimersByTimeAsync(180); await flushPromises();
      await input.trigger("keydown", { key: "Tab" }); await flushPromises();
      expect(useTerminalStore().draft).toBe("git sta");
      await input.trigger("keydown", { key: "ArrowLeft" });
      expect(wrapper.find('[role="listbox"]').exists()).toBe(false); wrapper.unmount();
    } finally { vi.useRealTimers(); }
  });
});
