import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { defineComponent, ref } from "vue";
import AppSelect from "./AppSelect.vue";

enableAutoUnmount(afterEach);
afterEach(() => vi.restoreAllMocks());
const options = [{ value: "feature", label: "需求 / 优化" }, { value: "disabled", label: "不可用", disabled: true }, { value: "hotfix", label: "Bug 修复" }];
const list = () => document.querySelector<HTMLElement>('[role="listbox"]')!;
describe("shared dropdown", () => {
  it("opens without changing the value, skips disabled options, confirms and restores focus", async () => {
    const wrapper = mount(AppSelect, { attachTo: document.body, props: { modelValue: "feature", options, "aria-label": "任务类型" } });
    const trigger = wrapper.get('[role="combobox"]');
    await trigger.trigger("keydown", { key: "ArrowDown" }); await flushPromises();
    expect(wrapper.emitted("update:modelValue")).toBeUndefined();
    expect(list().querySelector('[aria-selected="true"]')?.textContent).toBe("需求 / 优化");
    await trigger.trigger("keydown", { key: "ArrowDown" });
    await trigger.trigger("keydown", { key: "Enter" });
    expect(wrapper.emitted("update:modelValue")?.at(-1)).toEqual(["hotfix"]);
    expect(list()).toBeNull(); expect(document.activeElement).toBe(trigger.element);
  });
  it("keeps numeric values numeric and null placeholders unselected", async () => {
    const wrapper = mount(AppSelect, { props: { modelValue: null, placeholder: "选择父提交", options: [{ value: 1, label: "父提交 1" }, { value: 2, label: "父提交 2" }] } });
    await wrapper.get('button').trigger("click"); await flushPromises();
    expect(list().querySelector('[aria-selected="true"]')).toBeNull();
    list().querySelector<HTMLElement>('[data-value="2"]')!.click();
    expect(wrapper.emitted("update:modelValue")?.at(-1)).toEqual([2]);
  });
  it("searches long lists, reports no results and cancels without changing selection", async () => {
    const wrapper = mount(AppSelect, { attachTo: document.body, props: { modelValue: "branch-1", "aria-label": "分支", options: Array.from({ length: 30 }, (_, i) => ({ value: `branch-${i}`, label: `branch-${i}` })) } });
    await wrapper.get('button').trigger("click"); await flushPromises();
    const search = document.querySelector<HTMLInputElement>('[aria-label="搜索分支"]')!;
    expect(document.activeElement).toBe(search);
    search.value = "branch-22"; search.dispatchEvent(new Event("input", { bubbles: true })); await flushPromises();
    expect(list().querySelectorAll('[role="option"]')).toHaveLength(1);
    search.value = "unknown"; search.dispatchEvent(new Event("input", { bubbles: true })); await flushPromises();
    expect(list().textContent).toContain("无匹配选项");
    search.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); await flushPromises();
    expect(list()).toBeNull(); expect(wrapper.emitted("update:modelValue")).toBeUndefined();
  });
  it("retains free text, supports suggestions and does not consume composing Enter", async () => {
    const wrapper = mount(AppSelect, { props: { modelValue: "", options, editable: true } });
    const input = wrapper.get('input');
    await input.setValue("unknown"); await flushPromises();
    expect(wrapper.emitted("update:modelValue")?.at(-1)).toEqual(["unknown"]);
    expect(list().textContent).toContain("可继续输入");
    await input.trigger("keydown", { key: "Enter", isComposing: true });
    expect(list()).not.toBeNull();
    await input.trigger("keydown", { key: "Escape" });
    await input.trigger("click"); await flushPromises();
    list().querySelector<HTMLElement>('[data-value="hotfix"]')!.click();
    expect(wrapper.emitted("update:modelValue")?.at(-1)).toEqual(["hotfix"]);
  });
  it("dismisses on outside pointer, Tab, an offscreen anchor and disabled state", async () => {
    const wrapper = mount(AppSelect, { attachTo: document.body, props: { modelValue: "feature", options } });
    const trigger = wrapper.get('button');
    await trigger.trigger("click"); await flushPromises();
    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })); await flushPromises(); expect(list()).toBeNull();
    await trigger.trigger("click"); await trigger.trigger("keydown", { key: "Tab" }); expect(list()).toBeNull();
    await trigger.trigger("click"); await flushPromises();
    window.dispatchEvent(new Event("scroll")); await flushPromises(); expect(list()).not.toBeNull();
    const bounds = vi.spyOn(trigger.element, "getBoundingClientRect").mockReturnValue({ x: 0, y: -60, top: -60, bottom: -24, left: 0, right: 200, width: 200, height: 36, toJSON() {} });
    window.dispatchEvent(new Event("scroll")); await vi.waitFor(() => expect(list()).toBeNull());
    bounds.mockRestore();
    await trigger.trigger("click"); await wrapper.setProps({ disabled: true }); expect(list()).toBeNull();
    expect(wrapper.emitted("update:modelValue")).toBeUndefined();
  });
  it("respects a disabled ancestor fieldset even though the popup is teleported", async () => {
    const disabled = ref(false);
    const wrapper = mount(defineComponent({ components: { AppSelect }, setup: () => ({ disabled, options }), template: '<fieldset :disabled="disabled"><AppSelect model-value="feature" :options="options" /></fieldset>' }), { attachTo: document.body });
    await wrapper.get('button').trigger("click"); await flushPromises();
    disabled.value = true; await flushPromises();
    expect(list()).toBeNull();
    expect(wrapper.findComponent({ name: "AppSelect" }).emitted("update:modelValue")).toBeUndefined();
  });
  it("positions above a low trigger and cleans up the popup when unmounted", async () => {
    const wrapper = mount(AppSelect, { props: { modelValue: "feature", options } });
    vi.spyOn(wrapper.get('button').element, "getBoundingClientRect").mockReturnValue({ x: 20, y: 730, top: 730, left: 20, bottom: 766, right: 220, width: 200, height: 36, toJSON() {} });
    await wrapper.get('button').trigger("click"); await flushPromises();
    const popup = document.querySelector<HTMLElement>('.select-popover')!;
    expect(Number.parseFloat(popup.style.top)).toBeLessThan(730);
    expect(popup.style.width).toBe("200px");
    wrapper.unmount(); expect(list()).toBeNull();
  });
});
