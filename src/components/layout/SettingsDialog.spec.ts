import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import SettingsDialog from "@/components/layout/SettingsDialog.vue";
import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import { createBackendFixture } from "@/test/backend";
import { useSettingsStore } from "@/stores/settings";

describe("settings dialog", () => {
  let backend: BackendClient;

  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture();
    setBackendClientForTests(backend);
  });

  it("switches between appearance and AI settings", async () => {
    const wrapper = mount(SettingsDialog);

    expect(wrapper.find('[aria-label="使用系统主题"]').exists()).toBe(true);
    await wrapper.get('[aria-label="AI 设置"]').trigger("click");
    expect(wrapper.find('[aria-label="AI 提供商"]').exists()).toBe(true);
    await wrapper.get('[aria-label="外观设置"]').trigger("click");
    expect(wrapper.find('[aria-label="使用系统主题"]').exists()).toBe(true);
  });

  it("saves global and code fonts and keeps them when saving an existing AI form", async () => {
    const wrapper = mount(SettingsDialog);
    await wrapper.get('[aria-label="全局字体"]').setValue(" Microsoft YaHei ");
    await wrapper.get('[aria-label="代码与终端字体"]').setValue("Cascadia Code");
    expect(backend.settingsSave).not.toHaveBeenCalled();
    await wrapper.get('form[aria-label="全局字体设置"]').trigger("submit"); await flushPromises();
    expect(backend.settingsSave).toHaveBeenLastCalledWith(expect.objectContaining({ fontFamily: "Microsoft YaHei", codeFontFamily: "Cascadia Code" }));
    await wrapper.get('[aria-label="AI 设置"]').trigger("click");
    await wrapper.get('[aria-label="模型"]').setValue("another-model");
    await wrapper.get('[aria-label="保存 AI 设置"]').trigger("click"); await flushPromises();
    expect(backend.settingsSave).toHaveBeenLastCalledWith(expect.objectContaining({ fontFamily: "Microsoft YaHei", codeFontFamily: "Cascadia Code", model: "another-model" }));
    await wrapper.get('[aria-label="外观设置"]').trigger("click");
    await wrapper.get('[aria-label="恢复默认字体"]').trigger("click"); await flushPromises();
    expect(backend.settingsSave).toHaveBeenLastCalledWith(expect.objectContaining({ fontFamily: "", codeFontFamily: "" }));
    wrapper.unmount();
  });

  it("keeps the saved font on failure and lets the user retry the draft", async () => {
    vi.mocked(backend.settingsSave).mockRejectedValueOnce({ code: "io", message: "无法保存字体" });
    const wrapper = mount(SettingsDialog);
    await wrapper.get('[aria-label="全局字体"]').setValue("SimSun");
    await wrapper.get('form[aria-label="全局字体设置"]').trigger("submit"); await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toContain("无法保存字体");
    expect(useSettingsStore().settings).toMatchObject({ fontFamily: "" });
    await wrapper.get('form[aria-label="全局字体设置"]').trigger("submit"); await flushPromises();
    expect(useSettingsStore().settings).toMatchObject({ fontFamily: "SimSun" }); wrapper.unmount();
  });

  it("tests unsaved Gemini settings without exposing the key", async () => {
    vi.mocked(backend.aiTestConnection).mockResolvedValue({
      provider: "gemini",
      model: "gemini-2.5-flash",
      message: "连接成功。",
    });
    const wrapper = mount(SettingsDialog);
    await wrapper.get('[aria-label="AI 设置"]').trigger("click");
    await wrapper.get('[aria-label="AI 提供商"]').setValue("gemini");
    await wrapper.get('[aria-label="API Key"]').setValue("secret-key");
    await wrapper
      .get('[aria-label="模型"]')
      .setValue("gemini-2.5-flash");
    await wrapper.get('[aria-label="测试 AI 连接"]').trigger("click");
    await flushPromises();

    expect(backend.aiTestConnection).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "gemini",
        apiKey: "secret-key",
        model: "gemini-2.5-flash",
      }),
    );
    expect(wrapper.text()).not.toContain("secret-key");
    expect(wrapper.text()).toContain("连接成功。");
  });

  it("tests and persists Responses format without replacing a custom endpoint or model", async () => {
    vi.mocked(backend.aiTestConnection).mockResolvedValue({ provider: "openAi", model: "gpt-6-astra", message: "连接成功。" });
    const wrapper = mount(SettingsDialog);
    await wrapper.get('[aria-label="AI 设置"]').trigger("click");
    await wrapper.get('[aria-label="服务地址"]').setValue("https://proxy.example.test/v1");
    await wrapper.get('[aria-label="模型"]').setValue("gpt-6-astra");
    await wrapper.get('[aria-label="AI 接口格式"]').setValue("responses");
    await wrapper.get('[aria-label="测试 AI 连接"]').trigger("click");
    await flushPromises();
    expect(backend.aiTestConnection).toHaveBeenCalledWith(expect.objectContaining({ apiFormat: "responses", baseUrl: "https://proxy.example.test/v1", model: "gpt-6-astra" }));
    await wrapper.get('[aria-label="保存 AI 设置"]').trigger("click");
    await flushPromises();
    expect(backend.settingsSave).toHaveBeenCalledWith(expect.objectContaining({ aiApiFormat: "responses" }));
    wrapper.unmount();
    const reopened = mount(SettingsDialog);
    await reopened.get('[aria-label="AI 设置"]').trigger("click");
    expect(reopened.get('[aria-label="AI 接口格式"]').element).toHaveProperty("value", "responses");
    reopened.unmount();
  });

  it("reveals the key only through the icon control", async () => {
    const wrapper = mount(SettingsDialog);
    await wrapper.get('[aria-label="AI 设置"]').trigger("click");
    const key = wrapper.get('[aria-label="API Key"]');
    expect(key.attributes("type")).toBe("password");

    await wrapper.get('[aria-label="显示 API Key"]').trigger("click");
    expect(key.attributes("type")).toBe("text");
    expect(wrapper.find('[aria-label="隐藏 API Key"]').exists()).toBe(true);
  });

  it("configures a repository skill directory without deleting legacy rules", async () => {
    useSettingsStore().settings.reviewRuleFiles = ["docs/review.md"];
    useSettingsStore().settings.useReviewRuleFilesInReview = true;
    const wrapper = mount(SettingsDialog);
    await wrapper.get('[aria-label="AI 设置"]').trigger("click");
    await wrapper.get('[aria-label="仓库审查技能目录"]').setValue(" rules\\review ");
    await wrapper.get('[aria-label="保存 AI 设置"]').trigger("click");
    await flushPromises();
    expect(backend.settingsSave).toHaveBeenCalledWith(expect.objectContaining({ reviewSkillDirectory: "rules/review", reviewRuleFiles: ["docs/review.md"], useReviewRuleFilesInReview: true }));
    expect(wrapper.text()).toContain("SKILL.md");
    expect(wrapper.find('[aria-label="新增规则文件路径"]').exists()).toBe(false);
  });

  it("saves normalized AI settings explicitly and shows storage warning", async () => {
    const wrapper = mount(SettingsDialog);
    await wrapper.get('[aria-label="AI 设置"]').trigger("click");
    await wrapper
      .get('[aria-label="服务地址"]')
      .setValue(" https://example.test/v1 ");
    await wrapper.get('[aria-label="模型"]').setValue(" model ");
    await wrapper.get('[aria-label="保存 AI 设置"]').trigger("click");
    await flushPromises();

    expect(backend.settingsSave).toHaveBeenCalledWith(
      expect.objectContaining({
        baseUrl: "https://example.test/v1",
        model: "model",
      }),
    );
    expect(wrapper.text()).toContain(
      "API Key 为兼容旧版设置而以明文保存在本机，请仅在受信任的设备上使用。",
    );
  });
});
