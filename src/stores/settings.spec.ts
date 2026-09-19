import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { normalizeBackendError } from "@/lib/backend/errors";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { AppSettings } from "@/lib/backend/types";
import { createBackendFixture } from "@/test/backend";
import {
  applyAiProviderPreset,
  applyAiApiFormat,
  defaultSettings,
  useSettingsStore,
} from "@/stores/settings";
import { resolveTheme } from "@/stores/ui";

describe("settings store", () => {
  let backend: BackendClient;

  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture();
    setBackendClientForTests(backend);
  });

  it("uses system theme and closed clamped drawer defaults", () => {
    const settings = defaultSettings({ aiDrawerWidth: 900 });

    expect(settings.theme).toBe("system");
    expect(settings.aiDrawerOpen).toBe(false);
    expect(settings.aiDrawerWidth).toBe(560);
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("cleans loaded path aliases and puts the latest opened path first when saving", async () => {
    const store = useSettingsStore();
    vi.mocked(backend.settingsLoad).mockResolvedValue({ settings: defaultSettings({ recentRepoPaths: ["D:/work/repo", String.raw`\\?\D:\work\repo`, "D:/other/repo"] }) });
    await store.load();
    expect(store.settings.recentRepoPaths).toEqual(["D:/work/repo", "D:/other/repo"]);
    await store.recordRecentRepository(String.raw`\\?\D:\work\repo`);
    expect(backend.settingsSave).toHaveBeenLastCalledWith(expect.objectContaining({ recentRepoPaths: [String.raw`\\?\D:\work\repo`, "D:/other/repo"] }));
  });

  it("retains a non-fatal migration warning while normalizing loaded settings", async () => {
    vi.mocked(backend.settingsLoad).mockResolvedValue({
      settings: defaultSettings({ aiDrawerWidth: 100 }),
      migrationWarning: "旧版设置损坏。",
    });
    const store = useSettingsStore();

    await store.load();

    expect(store.settings.aiDrawerWidth).toBe(300);
    expect(store.migrationWarning).toBe("旧版设置损坏。");
  });

  it("serializes save operations", async () => {
    const resolvers: Array<(value: AppSettings) => void> = [];
    vi.mocked(backend.settingsSave).mockImplementation(
      (settings) =>
        new Promise((resolve) => {
          resolvers.push(() => resolve(settings));
        }),
    );
    const store = useSettingsStore();

    const first = store.save({ ...store.settings, model: "first" });
    const second = store.save({ ...store.settings, model: "second" });
    await Promise.resolve();
    expect(backend.settingsSave).toHaveBeenCalledTimes(1);
    resolvers[0]?.(defaultSettings({ model: "first" }));
    await first;
    await Promise.resolve();
    expect(backend.settingsSave).toHaveBeenCalledTimes(2);
    resolvers[1]?.(defaultSettings({ model: "second" }));
    await second;
    expect(store.settings.model).toBe("second");
  });

  it("does not leak unknown thrown objects", () => {
    expect(normalizeBackendError({ secret: "plain-key" })).toEqual({
      code: "unexpected",
      message: "发生未知错误。",
    });
  });

  it("applies provider presets without overwriting deliberate custom values", () => {
    const openAi = defaultSettings();
    expect(applyAiProviderPreset(openAi, "gemini")).toMatchObject({
      aiProvider: "gemini",
      baseUrl: "https://generativelanguage.googleapis.com/v1beta",
      model: "gemini-2.5-flash",
    });

    const customized = defaultSettings({
      baseUrl: "https://proxy.example.test/v1",
      model: "private-model",
    });
    expect(applyAiProviderPreset(customized, "qwen")).toMatchObject({
      aiProvider: "qwen",
      baseUrl: "https://proxy.example.test/v1",
      model: "private-model",
    });
  });

  it("switches endpoint format and provider presets without changing custom models", () => {
    const responses = applyAiApiFormat(defaultSettings(), "responses");
    expect(responses).toMatchObject({ aiApiFormat: "responses", baseUrl: "https://api.openai.com/v1/responses", model: "gpt-4o-mini" });
    const gemini = applyAiProviderPreset(responses, "gemini");
    expect(gemini.baseUrl).toBe("https://generativelanguage.googleapis.com/v1beta");
    expect(applyAiProviderPreset(gemini, "openAi").baseUrl).toBe("https://api.openai.com/v1/responses");
    const custom = applyAiApiFormat(defaultSettings({ baseUrl: "https://proxy.test/prefix/v1/chat/completions?route=a", model: "gpt-6-astra" }), "responses");
    expect(custom).toMatchObject({ baseUrl: "https://proxy.test/prefix/v1/responses?route=a", model: "gpt-6-astra" });
    expect(applyAiApiFormat(custom, "chatCompletions").baseUrl).toBe("https://proxy.test/prefix/v1/chat/completions?route=a");
    const routed = defaultSettings({ baseUrl: "https://proxy.test/v1?route=/chat/completions" });
    expect(applyAiApiFormat(routed, "responses").baseUrl).toBe(routed.baseUrl);
  });

  it("normalizes AI fields and repository rule paths when saving", async () => {
    const store = useSettingsStore();
    vi.mocked(backend.settingsSave).mockImplementation(async (settings) => settings);

    await store.save({
      ...store.settings,
      apiKey: " exact key ",
      baseUrl: " https://example.test/v1 ",
      model: " custom-model ",
      reviewRuleFiles: [
        " docs\\review.md ",
        "docs/review.md",
        "",
        "AGENTS.md",
      ],
    });

    expect(backend.settingsSave).toHaveBeenCalledWith(
      expect.objectContaining({
        apiKey: " exact key ",
        baseUrl: "https://example.test/v1",
        model: "custom-model",
        reviewRuleFiles: ["docs/review.md", "AGENTS.md"],
      }),
    );
  });
});
