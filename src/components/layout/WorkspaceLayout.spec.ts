import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import ContextResizeHandle from "./ContextResizeHandle.vue";
import AppSidebar from "./AppSidebar.vue";
import { useUiStore } from "@/stores/ui";
enableAutoUnmount(afterEach);
beforeEach(() => { localStorage.clear(); setActivePinia(createPinia()); });
afterEach(() => localStorage.clear());
describe("workspace layout", () => {
  it("collapses navigation without losing accessible names and remembers the layout", async () => {
    const wrapper = mount(AppSidebar);
    await wrapper.get('[aria-label="折叠侧边栏"]').trigger('click');
    expect(wrapper.classes()).toContain('collapsed');
    expect(wrapper.get('[data-view="changes"]').text()).toContain('文件状态');
    useUiStore().contextWidth = 710;
    await flushPromises();
    setActivePinia(createPinia());
    expect(useUiStore().sidebarCollapsed).toBe(true);
    expect(useUiStore().contextWidth).toBe(710);
  });
  it("supports keyboard resizing within bounds and reset", async () => {
    const wrapper = mount(ContextResizeHandle, { props: { width: 320, maxWidth: 620 } });
    await wrapper.trigger('keydown', { key: 'ArrowRight' });
    await wrapper.trigger('keydown', { key: 'Home' });
    await wrapper.trigger('keydown', { key: 'End' });
    await wrapper.trigger('dblclick');
    expect(wrapper.emitted('resize')).toEqual([[340], [270], [620], [320]]);
  });
  it("normalizes invalid saved widths", () => {
    localStorage.setItem('hq-git.workspace-layout.v1', JSON.stringify({ contextWidth: -100, sidebarCollapsed: 'true' }));
    expect(useUiStore().contextWidth).toBe(270);
    expect(useUiStore().sidebarCollapsed).toBe(false);
  });
});
