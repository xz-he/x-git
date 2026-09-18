import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ChangesList from "./ChangesList.vue";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import { createBackendFixture } from "@/test/backend";

enableAutoUnmount(afterEach);
afterEach(() => { vi.restoreAllMocks(); document.body.innerHTML = ""; });
describe("drag file selection", () => {
  let backend: BackendClient;
  const frames = new Map<number, FrameRequestCallback>();
  let nextFrame = 0;
  beforeEach(() => {
    setActivePinia(createPinia()); backend = createBackendFixture(); setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: "C:/drag", name: "drag", currentBranch: "main", headShortHash: "aaaaaaa", isClean: false, changedFileCount: 6, conflictCount: 0, remotes: [], upstream: null };
    useChangesStore().snapshot = { stagedCount: 6, unstagedCount: 6, files: Array.from({ length: 6 }, (_, i) => ({
      path: `${i}.ts`, oldPath: null, indexStatus: "M", worktreeStatus: "M", staged: true, unstaged: true, conflict: false,
    })) };
    frames.clear(); nextFrame = 0;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation(callback => { frames.set(++nextFrame, callback); return nextFrame; });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(id => { frames.delete(id); });
  });
  function pointer(target: EventTarget, type: string, y: number, options: { button?: number; buttons?: number; pointerId?: number } = {}) {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: 150, clientY: y, button: options.button ?? 0, buttons: options.buttons ?? (type === "pointerup" ? 0 : 1) });
    Object.defineProperties(event, { pointerId: { value: options.pointerId ?? 1 }, pointerType: { value: "mouse" } });
    target.dispatchEvent(event);
  }
  function rect(top: number, bottom: number): DOMRect { return { top, bottom, left: 100, right: 500, width: 400, height: bottom - top, x: 100, y: top, toJSON: () => ({}) }; }
  async function setup() {
    const wrapper = mount(ChangesList, { attachTo: document.body }); await flushPromises();
    for (const scope of ["staged", "unstaged"]) {
      const pane = wrapper.get(`.${scope}-pane`).element as HTMLElement;
      vi.spyOn(pane, "getBoundingClientRect").mockImplementation(() => rect(100, 240));
      wrapper.findAll(`.${scope}-pane .change-row`).forEach((row, i) => {
        vi.spyOn(row.element, "getBoundingClientRect").mockImplementation(() => rect(120 + i * 30 - pane.scrollTop, 150 + i * 30 - pane.scrollTop));
      });
    }
    return wrapper;
  }
  function checked(scope = "unstaged"): string[] {
    return [...document.querySelectorAll<HTMLInputElement>(`.${scope}-pane .change-row input:checked`)].map(input => input.closest(".change-row")!.querySelector(".file-select")!.getAttribute("data-change-key")!);
  }
  function tick(time: number): void { const current = [...frames.values()]; frames.clear(); current.forEach(callback => callback(time)); }
  it("selects a range for batch stage without opening diffs or changing the other group", async () => {
    const wrapper = await setup();
    const first = wrapper.get('[data-change-key="unstaged:0.ts"]');
    pointer(first.element, "pointerdown", 135); pointer(window, "pointermove", 195); pointer(window, "pointerup", 195);
    await first.trigger("click"); await flushPromises();
    expect(checked()).toEqual(["unstaged:0.ts", "unstaged:1.ts", "unstaged:2.ts"]);
    expect(checked("staged")).toEqual([]);
    expect(backend.changesFileDiff).not.toHaveBeenCalled();
    vi.mocked(backend.changesStageFiles).mockResolvedValue({ workspace: { repository: useRepositoryStore().snapshot!, changes: useChangesStore().snapshot! }, operationState: { kind: "none", conflicts: [], abortAction: null } });
    await wrapper.get('[aria-label="批量暂存"]').trigger("click"); await flushPromises();
    expect(backend.changesStageFiles).toHaveBeenCalledWith("C:/drag", ["0.ts", "1.ts", "2.ts"]);
  });
  it("preserves normal file clicks and checkbox clicks below the drag threshold", async () => {
    const wrapper = await setup();
    const file = wrapper.get('[data-change-key="unstaged:0.ts"]');
    pointer(file.element, "pointerdown", 135); pointer(window, "pointermove", 137); pointer(window, "pointerup", 137);
    await file.trigger("click"); await flushPromises();
    expect(backend.changesFileDiff).toHaveBeenCalledWith("C:/drag", "0.ts", "unstaged");
    expect(checked()).toEqual([]);
    const checkbox = wrapper.get<HTMLInputElement>('[aria-label="选择未暂存文件 1.ts"]');
    pointer(checkbox.element, "pointerdown", 165); pointer(window, "pointerup", 165); checkbox.element.click(); await flushPromises();
    expect(checked()).toEqual(["unstaged:1.ts"]);
  });
  it("supports reverse dragging and shrinking a range while preserving earlier choices", async () => {
    const wrapper = await setup();
    await wrapper.get('[aria-label="选择已暂存文件 5.ts"]').setValue(true);
    pointer(wrapper.get('[data-change-key="staged:3.ts"]').element, "pointerdown", 225);
    pointer(window, "pointermove", 135); await flushPromises();
    expect(checked("staged")).toHaveLength(5);
    pointer(window, "pointermove", 195); await flushPromises();
    expect(checked("staged")).toEqual(["staged:2.ts", "staged:3.ts", "staged:5.ts"]);
    pointer(window, "pointerup", 195);
  });
  it("dragging from a checked checkbox clears the range without a trailing checkbox toggle", async () => {
    const wrapper = await setup(); await wrapper.get('[aria-label="全选未暂存文件"]').setValue(true);
    const checkbox = wrapper.get<HTMLInputElement>('[aria-label="选择未暂存文件 0.ts"]');
    pointer(checkbox.element, "pointerdown", 135); pointer(window, "pointermove", 195); pointer(window, "pointerup", 195);
    checkbox.element.click(); await flushPromises();
    expect(checked()).toEqual(["unstaged:3.ts", "unstaged:4.ts", "unstaged:5.ts"]);
  });
  it("scrolls only the starting pane at its edge and continues selecting rows", async () => {
    const wrapper = await setup();
    const pane = wrapper.get(".unstaged-pane").element;
    pointer(wrapper.get('[data-change-key="unstaged:0.ts"]').element, "pointerdown", 135);
    pointer(window, "pointermove", 239);
    for (let time = 16; time <= 320; time += 16) tick(time);
    await flushPromises();
    expect(pane.scrollTop).toBeGreaterThan(0); expect(checked()).toHaveLength(6);
    expect(wrapper.get(".staged-pane").element.scrollTop).toBe(0);
    pointer(window, "pointerup", 239); expect(frames.size).toBe(0);
  });
  it("ignores right clicks and action buttons, and cancels on repository replacement", async () => {
    const wrapper = await setup();
    pointer(wrapper.get('[data-change-key="unstaged:0.ts"]').element, "pointerdown", 135, { button: 2 });
    pointer(window, "pointermove", 195); await flushPromises(); expect(checked()).toEqual([]);
    pointer(wrapper.get('[aria-label="暂存 0.ts"]').element, "pointerdown", 135);
    pointer(window, "pointermove", 195); await flushPromises(); expect(checked()).toEqual([]);
    pointer(wrapper.get('[data-change-key="unstaged:0.ts"]').element, "pointerdown", 135); pointer(window, "pointermove", 195);
    useRepositoryStore().generation++; await flushPromises();
    pointer(window, "pointermove", 225); await flushPromises();
    expect(checked()).toEqual([]); expect(frames.size).toBe(0);
  });
  it("stops on loss of focus or when Git becomes busy", async () => {
    const wrapper = await setup();
    const first = wrapper.get('[data-change-key="unstaged:0.ts"]').element;
    pointer(first, "pointerdown", 135); pointer(window, "pointermove", 165);
    window.dispatchEvent(new Event("blur")); pointer(window, "pointermove", 225); await flushPromises();
    expect(checked()).toHaveLength(2); expect(frames.size).toBe(0);
    pointer(first, "pointerdown", 135); pointer(window, "pointermove", 195);
    useRepositoryStore().operation = { kind: "refresh" };
    pointer(window, "pointermove", 225); await flushPromises();
    expect(checked()).toHaveLength(3); expect(frames.size).toBe(0);
  });
});
