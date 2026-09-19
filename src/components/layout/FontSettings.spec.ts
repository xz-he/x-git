import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import FontSettings from "./FontSettings.vue";
import { setBackendClientForTests } from "@/lib/backend/client";
import { createBackendFixture } from "@/test/backend";
import { selectOption } from "@/test/select";

enableAutoUnmount(afterEach);
beforeEach(() => setActivePinia(createPinia()));

describe("installed font selection", () => {
  it("loads installed fonts, searches Chinese aliases, saves the family and previews code fonts", async () => {
    const backend = createBackendFixture({ settingsFonts: vi.fn(async () => ["Microsoft YaHei", "Custom Installed Font", "Consolas", "Consolas"]) });
    setBackendClientForTests(backend);
    const wrapper = mount(FontSettings);
    await flushPromises();
    expect(wrapper.text()).toContain("已读取 3 种本机字体");
    await wrapper.get('[aria-label="全局字体"]').setValue("微软雅黑");
    await flushPromises();
    expect(document.querySelectorAll('[role="option"]')).toHaveLength(1);
    await selectOption(wrapper, "全局字体", "Microsoft YaHei");
    await selectOption(wrapper, "代码与终端字体", "Custom Installed Font");
    expect(wrapper.get('pre').attributes('style')).toContain('Custom Installed Font');
    await wrapper.get('form').trigger('submit'); await flushPromises();
    expect(backend.settingsSave).toHaveBeenCalledWith(expect.objectContaining({ fontFamily: "Microsoft YaHei", codeFontFamily: "Custom Installed Font" }));
  });

  it("falls back on failure, refreshes after installation, and keeps free text", async () => {
    const backend = createBackendFixture({ settingsFonts: vi.fn().mockRejectedValueOnce(new Error("unavailable")).mockResolvedValueOnce(["New Font"]).mockRejectedValueOnce(new Error("retry")) });
    setBackendClientForTests(backend);
    const wrapper = mount(FontSettings); await flushPromises();
    expect(wrapper.text()).toContain("暂时无法读取系统字体");
    await wrapper.get('[aria-label="全局字体"]').setValue("User Font");
    await wrapper.get('[aria-label="刷新字体列表"]').trigger('click'); await flushPromises();
    expect(wrapper.text()).toContain("已读取 1 种本机字体");
    expect((wrapper.get('[aria-label="全局字体"]').element as HTMLInputElement).value).toBe("User Font");
    await wrapper.get('[aria-label="刷新字体列表"]').trigger('click'); await flushPromises();
    expect(wrapper.text()).toContain("已保留上次读取的字体");
    await wrapper.get('[aria-label="全局字体"]').trigger('keydown', { key: 'Escape' });
    await selectOption(wrapper, "代码与终端字体", "New Font");
    await wrapper.get('form').trigger('submit'); await flushPromises();
    expect(backend.settingsSave).toHaveBeenCalledWith(expect.objectContaining({ fontFamily: "User Font", codeFontFamily: "New Font" }));
  });
});
