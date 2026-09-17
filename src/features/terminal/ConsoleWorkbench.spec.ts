import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/App.vue";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import { useRepositoryStore } from "@/stores/repository";
import { useTerminalStore } from "@/stores/terminal";
import { useChangesStore } from "@/stores/changes";
import type { TerminalEvent } from "@/lib/backend/types";

const screen = vi.hoisted(() => ({ write: vi.fn(async (_data: string | Uint8Array) => undefined), attach: vi.fn(), detach: vi.fn(), focus: vi.fn(), reset: vi.fn(), clear: vi.fn(), dispose: vi.fn(), text: vi.fn(() => "saved result"), cols: 80, rows: 24 }));
vi.mock("./terminalScreen", () => ({ createTerminalScreen: vi.fn(async () => screen) }));

describe("interactive Git workbench", () => {
  let pinia: ReturnType<typeof createPinia>;
  let backend: BackendClient;
  let emit: (event: TerminalEvent) => void;
  beforeEach(() => {
    pinia = createPinia(); setActivePinia(pinia);
    const repository = useRepositoryStore();
    repository.snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abc", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null };
    vi.spyOn(repository, "refreshAfterTerminal").mockResolvedValue();
    backend = createBackendFixture({ terminalListen: vi.fn(async listener => { emit = listener; return () => undefined; }), terminalStart: vi.fn(async (rootPath, runId) => ({ rootPath, runId })) });
    setBackendClientForTests(backend);
  });
  async function open() {
    const wrapper = mount(App, { global: { plugins: [pinia] } }); await flushPromises();
    await wrapper.get('.sidebar [data-view="terminal"]').trigger("click"); await flushPromises(); return wrapper;
  }
  function send(event: TerminalEvent["event"], sequence: number) { emit({ rootPath: "C:/repo", runId: useTerminalStore().runId!, sequence, event }); }

  it("opens terminal input with unrestricted Git help and examples only fill", async () => {
    const wrapper = await open();
    expect(wrapper.get('[aria-label="Git 命令"]').element).toBeInstanceOf(HTMLInputElement);
    expect(wrapper.text()).not.toContain("只读查询"); expect(wrapper.text()).toContain("Tab 补全");
    await wrapper.get('[data-command="git log --oneline -n 20"]').trigger("click");
    expect(useTerminalStore().draft).toBe("git log --oneline -n 20"); expect(backend.terminalStart).not.toHaveBeenCalled(); wrapper.unmount();
  });
  it("runs Enter, streams bytes to terminal and exposes exit status", async () => {
    const wrapper = await open();
    await wrapper.get('[aria-label="Git 命令"]').trigger("keydown", { key: "Enter" }); await flushPromises();
    send({ kind: "output", data: btoa("<img src=x>") }, 1);
    send({ kind: "exited", exitCode: 128, durationMs: 250, cancelled: false, error: null }, 2); await flushPromises();
    expect(screen.write.mock.calls.some(([data]) => typeof data !== "string" && Array.from(data).join(",") === Array.from(new TextEncoder().encode("<img src=x>")).join(","))).toBe(true);
    expect(wrapper.find('.terminal-host img').exists()).toBe(false); expect(wrapper.text()).toContain("退出码 128"); wrapper.unmount();
  });
  it("supports input/interruption and remains reachable while browsing during a run", async () => {
    const wrapper = await open();
    const input = wrapper.get('[aria-label="Git 命令"]');
    await input.trigger("keydown", { key: "Enter", isComposing: true }); expect(backend.terminalStart).not.toHaveBeenCalled();
    await wrapper.get('[aria-label="运行命令"]').trigger("click"); await flushPromises();
    expect(wrapper.get('[aria-label="运行命令"]').attributes("disabled")).toBeDefined();
    await wrapper.get('[aria-label="中断命令"]').trigger("click"); await flushPromises();
    expect(backend.terminalWrite).toHaveBeenCalledWith("C:/repo", useTerminalStore().runId, btoa("\x03"));
    await wrapper.get('.sidebar [data-view="changes"]').trigger("click");
    expect(wrapper.get('[aria-label="创建提交"]').attributes("disabled")).toBeDefined();
    expect(wrapper.get('[data-testid="context-panel"]').attributes("inert")).toBeUndefined();
    expect(wrapper.get('[data-testid="detail-panel"]').attributes("inert")).toBeUndefined();
    useChangesStore().operation = { kind: "diff" };
    await flushPromises();
    expect(wrapper.get('.sidebar [data-view="terminal"]').attributes("disabled")).toBeUndefined();
    await wrapper.get('.sidebar [data-view="terminal"]').trigger("click"); await flushPromises();
    await wrapper.get('[aria-label="终止命令"]').trigger("click"); expect(backend.terminalTerminate).toHaveBeenCalledOnce(); wrapper.unmount();
  });
  it("recalls without executing and preserves the screen when returning from home", async () => {
    const wrapper = await open(); await wrapper.get('[aria-label="运行命令"]').trigger("click"); await flushPromises();
    send({ kind: "exited", exitCode: 0, durationMs: 12, cancelled: false, error: null }, 1); await flushPromises();
    await wrapper.get('[data-testid="console-history-entry"]').trigger("click"); expect(backend.terminalStart).toHaveBeenCalledOnce();
    await wrapper.get('[aria-label="返回首页"]').trigger("click"); await wrapper.get('[aria-label="继续当前仓库"]').trigger("click"); await flushPromises();
    expect(useTerminalStore().outputText()).toBe("saved result"); wrapper.unmount();
  });
  it("copies the visible terminal text and reports clipboard failure", async () => {
    const copy = vi.fn().mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
    const wrapper = await open();
    await wrapper.get('[aria-label="复制输出"]').trigger("click"); await flushPromises();
    expect(copy).toHaveBeenCalledWith("saved result"); expect(wrapper.text()).toContain("已复制");
    await wrapper.get('[aria-label="复制输出"]').trigger("click"); await flushPromises(); expect(wrapper.text()).toContain("复制失败"); wrapper.unmount();
  });
  it("does not subscribe after unmount during startup", async () => {
    let finish!: (value: () => void) => void;
    vi.mocked(backend.aiListen).mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
    const wrapper = mount(App, { global: { plugins: [pinia] } }); wrapper.unmount(); finish(() => undefined); await flushPromises();
    expect(backend.terminalListen).not.toHaveBeenCalled();
  });
});
