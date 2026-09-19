import { mount, enableAutoUnmount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { EditorView } from "@codemirror/view";
import ConflictEditor from "./ConflictEditor.vue";

enableAutoUnmount(afterEach);
afterEach(() => vi.restoreAllMocks());
describe("conflict editor integration", () => {
  it("undoes and redoes externally applied blocks through the normal edit event", async () => {
    const wrapper = mount(ConflictEditor, { props: { modelValue: "before\n", label: "result" } });
    await wrapper.setProps({ modelValue: "after\n" });
    expect(wrapper.emitted("history-change")?.at(-1)).toEqual([{ undo: true, redo: false }]);
    wrapper.vm.undo();
    expect(wrapper.emitted("update:modelValue")?.at(-1)).toEqual(["before\n"]);
    expect(wrapper.emitted("history-change")?.at(-1)).toEqual([{ undo: false, redo: true }]);
    wrapper.vm.redo();
    expect(wrapper.emitted("update:modelValue")?.at(-1)).toEqual(["after\n"]);
  });
  it("maps right click to the clicked source line and supports Shift+F10", async () => {
    const wrapper = mount(ConflictEditor, { props: { modelValue: "first\nsecond\nthird", label: "source", readonly: true, blockActions: true } });
    const view = EditorView.findFromDOM(wrapper.get('.cm-editor').element as HTMLElement)!;
    vi.spyOn(view, "posAtCoords").mockReturnValue(view.state.doc.line(2).from);
    await wrapper.get('.cm-content').trigger("contextmenu", { clientX: 30, clientY: 40 });
    expect(wrapper.emitted("block-context")?.at(-1)).toEqual([{ line: 1, x: 30, y: 40 }]);
    vi.spyOn(view, "coordsAtPos").mockReturnValue({ left: 10, right: 20, top: 30, bottom: 40 });
    await wrapper.get('.cm-content').trigger("keydown", { key: "F10", shiftKey: true });
    expect(wrapper.emitted("block-context")?.at(-1)).toEqual([{ line: 0, x: 10, y: 40 }]);
    expect(wrapper.emitted("update:modelValue")).toBeUndefined();
  });
  it("changes display options without altering text and blocks undo on readonly views", async () => {
    const wrapper = mount(ConflictEditor, { props: { modelValue: "a  b\n", label: "source", readonly: true } });
    await wrapper.setProps({ wrapLines: true, showWhitespace: true, modelValue: "c  d\n" });
    expect(wrapper.find('.cm-lineWrapping').exists()).toBe(true);
    const view = EditorView.findFromDOM(wrapper.get('.cm-editor').element as HTMLElement)!;
    wrapper.vm.undo();
    expect(view.state.doc.toString()).toBe("c  d\n");
    expect(wrapper.emitted("update:modelValue")).toBeUndefined();
  });
});
