import { defineStore } from "pinia";
import { ref } from "vue";

import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import { normalizeFontFamily } from "@/lib/fonts";
import type {
  AiApiFormat,
  AiProvider,
  AppSettings,
  BackendError,
} from "@/lib/backend/types";

export const AI_PROVIDER_DEFAULTS: Record<
  Exclude<AiProvider, "custom">,
  { baseUrl: string; model: string }
> = {
  openAi: {
    baseUrl: "https://api.openai.com/v1/chat/completions",
    model: "gpt-4o-mini",
  },
  qwen: {
    baseUrl:
      "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
    model: "qwen-plus",
  },
  gemini: {
    baseUrl: "https://generativelanguage.googleapis.com/v1beta",
    model: "gemini-2.5-flash",
  },
};

export function defaultSettings(
  overrides: Partial<AppSettings> = {},
): AppSettings {
  const settings: AppSettings = {
    schemaVersion: 1,
    theme: "system",
    fontFamily: "",
    codeFontFamily: "",
    launchAtLogin: false,
    lastRepoPath: null,
    recentRepoPaths: [],
    reviewRuleFiles: [],
    reviewSkillDirectory: "",
    useReviewRuleFilesInReview: false,
    aiDrawerOpen: false,
    aiDrawerWidth: 360,
    aiProvider: "openAi",
    aiApiFormat: "chatCompletions",
    apiKey: "",
    baseUrl: "https://api.openai.com/v1/chat/completions",
    model: "gpt-4o-mini",
    ...overrides,
  };
  return normalizeSettings(settings);
}

export function normalizeSettings(settings: AppSettings): AppSettings {
  const reviewRuleFiles = Array.from(
    new Map(
      settings.reviewRuleFiles
        .map((path) => path.trim().replace(/\\/g, "/"))
        .filter(Boolean)
        .map((path) => [path.toLocaleLowerCase(), path]),
    ).values(),
  );
  return {
    ...settings,
    aiApiFormat: settings.aiApiFormat === "responses" ? "responses" : "chatCompletions",
    fontFamily: normalizeFontFamily(settings.fontFamily),
    codeFontFamily: normalizeFontFamily(settings.codeFontFamily),
    baseUrl: settings.baseUrl.trim(),
    model: settings.model.trim(),
    reviewRuleFiles,
    reviewSkillDirectory: (settings.reviewSkillDirectory ?? "").trim().replace(/\\/g, "/"),
    aiDrawerWidth: Math.min(560, Math.max(300, settings.aiDrawerWidth)),
    recentRepoPaths: Array.from(
      new Map(
        settings.recentRepoPaths.map((path) => [path.toLocaleLowerCase(), path]),
      ).values(),
    ).slice(0, 20),
  };
}

export function applyAiProviderPreset(
  settings: AppSettings,
  provider: AiProvider,
): AppSettings {
  const previous =
    settings.aiProvider === "custom"
      ? undefined
      : AI_PROVIDER_DEFAULTS[settings.aiProvider];
  const next = provider === "custom" ? undefined : AI_PROVIDER_DEFAULTS[provider];
  const replaceBaseUrl =
    !settings.baseUrl.trim() || settings.baseUrl.trim() === previous?.baseUrl ||
    (!!previous && settings.baseUrl.trim() === endpointForFormat(previous.baseUrl, settings.aiApiFormat));
  const replaceModel =
    !settings.model.trim() || settings.model.trim() === previous?.model;

  return {
    ...settings,
    aiProvider: provider,
    baseUrl: replaceBaseUrl && next ? endpointForFormat(next.baseUrl, settings.aiApiFormat) : settings.baseUrl,
    model: replaceModel && next ? next.model : settings.model,
  };
}

function endpointForFormat(baseUrl: string, format: AiApiFormat): string {
  const endpoint = format === "responses" ? "/responses" : "/chat/completions";
  try {
    const url = new URL(baseUrl);
    const path = url.pathname.replace(/\/(?:chat\/completions|responses)\/?$/, endpoint);
    if (path === url.pathname) return baseUrl;
    url.pathname = path;
    return url.toString();
  } catch {
    // Leave incomplete form input untouched; the connection test validates it.
    return baseUrl;
  }
}

export function applyAiApiFormat(settings: AppSettings, format: AiApiFormat): AppSettings {
  return { ...settings, aiApiFormat: format, baseUrl: endpointForFormat(settings.baseUrl.trim(), format) };
}

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<AppSettings>(defaultSettings());
  const migrationWarning = ref<string>();
  const loading = ref(false);
  const saving = ref(false);
  const error = ref<BackendError>();
  let saveQueue: Promise<void> = Promise.resolve();

  async function load(): Promise<void> {
    loading.value = true;
    error.value = undefined;
    try {
      const result = await backendClient.settingsLoad();
      settings.value = normalizeSettings(result.settings);
      migrationWarning.value = result.migrationWarning;
    } catch (cause) {
      error.value = normalizeBackendError(cause);
      throw error.value;
    } finally {
      loading.value = false;
    }
  }

  function save(nextSettings: AppSettings): Promise<void> {
    const requested = normalizeSettings(nextSettings);
    const operation = saveQueue.then(async () => {
      saving.value = true;
      error.value = undefined;
      try {
        settings.value = normalizeSettings(
          await backendClient.settingsSave(requested),
        );
      } catch (cause) {
        error.value = normalizeBackendError(cause);
        throw error.value;
      } finally {
        saving.value = false;
      }
    });
    saveQueue = operation.catch(() => undefined);
    return operation;
  }

  async function recordRecentRepository(path: string): Promise<void> {
    const recentRepoPaths = [
      path,
      ...settings.value.recentRepoPaths.filter(
        (existing) => existing.toLocaleLowerCase() !== path.toLocaleLowerCase(),
      ),
    ].slice(0, 20);
    await save({
      ...settings.value,
      lastRepoPath: path,
      recentRepoPaths,
    });
  }

  return {
    settings,
    migrationWarning,
    loading,
    saving,
    error,
    load,
    save,
    recordRecentRepository,
  };
});
