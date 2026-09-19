import { createPinia, setActivePinia } from "pinia";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ConflictDetail from "./ConflictDetail.vue";
import ConflictEditor from "./ConflictEditor.vue";
import { setupConflictSuggestion } from "@/test/conflictSuggestion";

enableAutoUnmount(afterEach);
const reveal = vi.fn();
const editorStub = { props: ["modelValue", "label", "wrapLines", "showWhitespace"], emits: ["block-context", "block-select"], methods: { reveal }, template: '<pre :aria-label="label">{{ modelValue }}</pre>' };
describe("TortoiseGit-style conflict actions", () => {
  beforeEach(() => { setActivePinia(createPinia()); reveal.mockClear(); });
  async function open() {
    const fixture = await setupConflictSuggestion();
    const draft = fixture.conflicts.current!;
    draft.detail.base.text = "start\nold\ngap\nnext\nend\n";
    draft.detail.ours.text = "start\nleft\ngap\nnext\nend\n";
    draft.detail.theirs.text = "start\nright\ngap\nnew\nend\n";
    draft.resolution = { kind: "text", text: "start\n<<<<<<< HEAD\nleft\n=======\nright\n>>>>>>> topic\ngap\nnew\nend\n", acknowledgeMarkers: false };
    const wrapper = mount(ConflictDetail, { attachTo: document.body, global: { stubs: { ConflictEditor: editorStub } } });
    await flushPromises();
    async function menu(side = 0, line = 1) {
      wrapper.findAllComponents(ConflictEditor)[side]!.vm.$emit("block-context", { line, x: 100, y: 100 });
      await flushPromises();
      return [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
    }
    return { ...fixture, wrapper, menu };
  }
  it("applies both blocks in chosen order to a draft, then asks before saving", async () => {
    const { wrapper, conflicts, backend, menu } = await open();
    const revision = conflicts.draftRevision;
    (await menu(1))[1]!.click(); await flushPromises();
    expect(conflicts.current?.resolution).toMatchObject({ kind: "text", text: "start\nright\nleft\ngap\nnew\nend\n" });
    expect(conflicts.draftRevision).toBeGreaterThan(revision);
    expect(backend.conflictsResolve).not.toHaveBeenCalled();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    await wrapper.get('[aria-label="保存并标记已解决"]').trigger("click");
    expect(conflicts.confirmation?.kind).toBe("save");
    expect(backend.conflictsResolve).not.toHaveBeenCalled();
  });
  it("disables block actions outside differences but permits whole-file adoption", async () => {
    const { conflicts, menu } = await open();
    const buttons = await menu(0, 0);
    expect(buttons.slice(0, 3).every(button => button.disabled)).toBe(true);
    expect(buttons[3]!.disabled).toBe(false);
    buttons[3]!.click(); await flushPromises();
    expect(conflicts.current?.resolution).toEqual({ kind: "ours" });
  });
  it("dismisses a stale menu after editing, switching files or starting an operation", async () => {
    const { conflicts, menu } = await open();
    await menu(); conflicts.edit("manual"); await flushPromises();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    await menu(); conflicts.submitting = true; await flushPromises();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    conflicts.submitting = false;
    await menu(); conflicts.selectedPath = undefined; await flushPromises();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
  it("navigates all panes, skips mergeable regions for conflict navigation and wraps", async () => {
    const { wrapper } = await open();
    await wrapper.get('[aria-label="下一处差异"]').trigger("click");
    expect(wrapper.get('.block-status').text()).toContain("1 / 2");
    expect(reveal).toHaveBeenCalledTimes(3);
    await wrapper.get('[aria-label="下一处差异"]').trigger("click");
    expect(wrapper.get('.block-status').text()).toContain("2 / 2");
    await wrapper.get('[aria-label="下一处冲突"]').trigger("click");
    expect(wrapper.get('.block-status').text()).toContain("1 / 2");
    await wrapper.get('[aria-label="上一处差异"]').trigger("click");
    expect(wrapper.get('.block-status').text()).toContain("2 / 2");
  });
  it("shares wrap and whitespace settings across the three editors", async () => {
    const { wrapper } = await open();
    const toggles = wrapper.findAll('input[type="checkbox"]');
    await toggles[1]!.setValue(true); await toggles[2]!.setValue(true);
    for (const editor of wrapper.findAllComponents(ConflictEditor)) {
      expect(editor.props("wrapLines")).toBe(true); expect(editor.props("showWhitespace")).toBe(true);
    }
  });
  it("supports keyboard menu dismissal and disables ambiguous replacement", async () => {
    const { conflicts, menu } = await open();
    conflicts.edit("start\nrewritten everything\nend\n"); await flushPromises();
    expect((await menu()).slice(0, 3).every(button => button.disabled)).toBe(true);
    document.querySelector('[role="menu"]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await flushPromises(); expect(document.querySelector('[role="menu"]')).toBeNull();
  });
  it("can right-click the visible anchor of a deleted block at EOF", async () => {
    const { conflicts, menu } = await open();
    conflicts.current!.detail.base.text = "start\nold";
    conflicts.current!.detail.ours.text = "start";
    conflicts.current!.detail.theirs.text = "start\nnew";
    conflicts.edit("start\n<<<<<<< HEAD\n=======\nnew\n>>>>>>> topic");
    await flushPromises();
    const buttons = await menu(0, 0);
    expect(buttons[0]!.disabled).toBe(false);
    buttons[0]!.click(); await flushPromises();
    expect(conflicts.current?.resolution).toMatchObject({ kind: "text", text: "start" });
  });
});
