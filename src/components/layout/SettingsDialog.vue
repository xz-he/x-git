<script setup lang="ts">
import {
  Eye,
  EyeOff,
  Monitor,
  Moon,
  Sun,
  X,
} from "@lucide/vue";
import { computed, reactive, ref, watch } from "vue";

import type {
  AiConnectionConfig,
  AiApiFormat,
  AiProvider,
  AppSettings,
  ThemePreference,
} from "@/lib/backend/types";
import { useAiStore } from "@/stores/ai";
import {
  applyAiProviderPreset,
  applyAiApiFormat,
  useSettingsStore,
} from "@/stores/settings";
import { useUiStore } from "@/stores/ui";
import FontSettings from "./FontSettings.vue";
import UpdateSettings from "./UpdateSettings.vue";

defineEmits<{ close: [] }>();
const settingsStore = useSettingsStore();
const ai = useAiStore();
const ui = useUiStore();
const activeTab = computed({ get: () => ui.settingsTab, set: value => { ui.settingsTab = value; } });
const revealKey = ref(false);
const saved = ref(false);
const aiForm = reactive<AppSettings>({
  ...settingsStore.settings,
  reviewRuleFiles: [...settingsStore.settings.reviewRuleFiles],
});

const themes: Array<{
  value: ThemePreference;
  label: string;
  ariaLabel: string;
  icon: typeof Sun;
}> = [
  { value: "light", label: "浅色", ariaLabel: "使用浅色主题", icon: Sun },
  { value: "dark", label: "深色", ariaLabel: "使用深色主题", icon: Moon },
  { value: "system", label: "跟随系统", ariaLabel: "使用系统主题", icon: Monitor },
];

const providers: Array<{ value: AiProvider; label: string }> = [
  { value: "openAi", label: "OpenAI" },
  { value: "qwen", label: "通义千问" },
  { value: "gemini", label: "Gemini" },
  { value: "custom", label: "自定义兼容服务" },
];

function selectTheme(theme: ThemePreference): void {
  settingsStore.settings.theme = theme;
  aiForm.theme = theme;
  ui.applyTheme(theme);
  void settingsStore.save({ ...settingsStore.settings }).catch(() => undefined);
}

function selectProvider(event: Event): void {
  const provider = (event.target as HTMLSelectElement).value as AiProvider;
  Object.assign(aiForm, applyAiProviderPreset(aiForm, provider));
  saved.value = false;
}

function connectionConfig(): AiConnectionConfig {
  return {
    provider: aiForm.aiProvider,
    apiFormat: aiForm.aiApiFormat,
    apiKey: aiForm.apiKey,
    baseUrl: aiForm.baseUrl,
    model: aiForm.model,
  };
}

function selectApiFormat(event: Event): void {
  Object.assign(aiForm, applyAiApiFormat(aiForm, (event.target as HTMLSelectElement).value as AiApiFormat));
}

watch(() => [aiForm.aiProvider, aiForm.aiApiFormat, aiForm.baseUrl, aiForm.model, aiForm.apiKey], () => {
  saved.value = false;
  ai.connectionStatus = "idle";
  ai.connectionResult = undefined;
  ai.connectionError = undefined;
});

async function testConnection(): Promise<void> {
  await ai.testConnection({ ...connectionConfig() }).catch(() => undefined);
}

async function saveAiSettings(): Promise<void> {
  saved.value = false;
  await settingsStore.save({
    ...aiForm,
    fontFamily: settingsStore.settings.fontFamily,
    codeFontFamily: settingsStore.settings.codeFontFamily,
    theme: settingsStore.settings.theme,
    checkUpdatesOnStartup: settingsStore.settings.checkUpdatesOnStartup,
    reviewRuleFiles: [...aiForm.reviewRuleFiles],
  });
  Object.assign(aiForm, {
    ...settingsStore.settings,
    reviewRuleFiles: [...settingsStore.settings.reviewRuleFiles],
  });
  saved.value = true;
}
</script>

<template>
  <div class="backdrop" @click.self="$emit('close')">
    <section
      class="settings-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-title"
    >
      <header>
        <div>
          <h2 id="settings-title">设置</h2>
          <p>应用外观、AI 服务与版本更新</p>
        </div>
        <button
          class="icon-button"
          aria-label="关闭设置"
          title="关闭设置"
          @click="$emit('close')"
        >
          <X :size="17" />
        </button>
      </header>

      <nav class="tabs" aria-label="设置分类">
        <button
          :class="{ active: activeTab === 'appearance' }"
          aria-label="外观设置"
          @click="activeTab = 'appearance'"
        >
          外观
        </button>
        <button
          :class="{ active: activeTab === 'ai' }"
          aria-label="AI 设置"
          @click="activeTab = 'ai'"
        >
          AI
        </button>
        <button :class="{ active: activeTab === 'updates' }" @click="activeTab = 'updates'">版本更新</button>
      </nav>

      <div v-if="activeTab === 'appearance'" class="panel appearance-panel">
        <div class="setting-row">
          <div>
            <strong>主题</strong>
            <span>选择应用的显示方式</span>
          </div>
          <div class="segmented" role="group" aria-label="主题">
            <button
              v-for="theme in themes"
              :key="theme.value"
              :class="{ active: settingsStore.settings.theme === theme.value }"
              :aria-label="theme.ariaLabel"
              @click="selectTheme(theme.value)"
            >
              <component :is="theme.icon" :size="15" />{{ theme.label }}
            </button>
          </div>
        </div>
        <p v-if="ui.nativeThemeError" role="alert">{{ ui.nativeThemeError }}</p>
        <FontSettings />
      </div>

      <UpdateSettings v-else-if="activeTab === 'updates'" class="panel" />
      <form v-else class="panel ai-panel" @submit.prevent="saveAiSettings">
        <fieldset :disabled="ai.connectionStatus === 'testing'">
          <label>
            <span>提供商</span>
            <select
              :value="aiForm.aiProvider"
              aria-label="AI 提供商"
              @change="selectProvider"
            >
              <option
                v-for="provider in providers"
                :key="provider.value"
                :value="provider.value"
              >
                {{ provider.label }}
              </option>
            </select>
          </label>

          <label v-if="aiForm.aiProvider !== 'gemini'">
            <span>接口格式</span>
            <select :value="aiForm.aiApiFormat" aria-label="AI 接口格式" @change="selectApiFormat">
              <option value="chatCompletions">Chat Completions</option>
              <option value="responses">OpenAI Responses</option>
            </select>
          </label>
          <p v-if="aiForm.aiProvider !== 'gemini' && aiForm.aiApiFormat === 'responses'" class="storage-warning">
            使用 Responses API。服务地址可填写 /v1 基础地址或完整 /responses 地址；需要服务端支持此格式。
          </p>

          <label>
            <span>API Key</span>
            <div class="input-with-action">
              <input
                v-model="aiForm.apiKey"
                :type="revealKey ? 'text' : 'password'"
                aria-label="API Key"
                autocomplete="off"
                spellcheck="false"
                @input="saved = false"
              />
              <button
                type="button"
                class="icon-button input-action"
                :aria-label="revealKey ? '隐藏 API Key' : '显示 API Key'"
                :title="revealKey ? '隐藏 API Key' : '显示 API Key'"
                @click="revealKey = !revealKey"
              >
                <EyeOff v-if="revealKey" :size="16" />
                <Eye v-else :size="16" />
              </button>
            </div>
          </label>

          <label>
            <span>服务地址</span>
            <input
              v-model="aiForm.baseUrl"
              aria-label="服务地址"
              spellcheck="false"
              @input="saved = false"
            />
          </label>

          <label>
            <span>模型</span>
            <input
              v-model="aiForm.model"
              aria-label="模型"
              spellcheck="false"
              @input="saved = false"
            />
          </label>

          <label><span>仓库审查技能目录</span><input v-model="aiForm.reviewSkillDirectory" aria-label="仓库审查技能目录" placeholder="留空自动查找，例如 code-review-expert" spellcheck="false" @input="saved = false" /></label>
          <p class="storage-warning">填写当前仓库内包含 SKILL.md 的相对目录。留空查找 code-review-expert 或 code-review-export；同时存在时请指定。审查完整加载技能及引用规则，历史提交也使用当前仓库的规则。</p>
          <p v-if="aiForm.reviewRuleFiles.length" class="storage-warning">旧规则文件配置已保留，审查规则现由仓库 SKILL 提供，不再合并旧列表。</p>

          <p class="storage-warning">
            API Key 为兼容旧版设置而以明文保存在本机，请仅在受信任的设备上使用。
          </p>

          <div
            v-if="ai.connectionStatus === 'success'"
            class="feedback success"
            role="status"
          >
            {{ ai.connectionResult?.message }}
          </div>
          <div
            v-else-if="ai.connectionStatus === 'failed'"
            class="feedback error"
            role="alert"
          >
            {{ ai.connectionError?.message }}
            <details v-if="ai.connectionError?.diagnostics">
              <summary>诊断信息</summary>
              <pre>{{ ai.connectionError.diagnostics }}</pre>
            </details>
          </div>
          <div v-if="saved" class="feedback success" role="status">
            设置已保存。
          </div>

          <footer>
            <button
              type="button"
              class="secondary-button"
              aria-label="测试 AI 连接"
              @click="testConnection"
            >
              {{
                ai.connectionStatus === "testing" ? "正在测试..." : "测试连接"
              }}
            </button>
            <button
              type="button"
              class="primary-button"
              aria-label="保存 AI 设置"
              :disabled="settingsStore.saving"
              @click="saveAiSettings"
            >
              {{ settingsStore.saving ? "保存中..." : "保存" }}
            </button>
          </footer>
        </fieldset>
      </form>
    </section>
  </div>
</template>

<style scoped>
.backdrop { position: fixed; z-index: 20; inset: 0; display: grid; place-items: center; background: var(--overlay); }
.settings-dialog { display: flex; flex-direction: column; width: min(600px, calc(100vw - 32px)); max-height: min(720px, calc(100vh - 32px)); overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--surface-panel); box-shadow: var(--shadow-window); }
header { display: flex; align-items: center; justify-content: space-between; padding: 16px 18px 12px; }
h2, p { margin: 0; }
h2 { font-size: 15px; }
header p, .setting-row span, .rules-heading span { margin-top: 4px; color: var(--text-muted); font-size: 11px; }
.icon-button { display: grid; width: 30px; height: 30px; flex: 0 0 30px; place-items: center; border-radius: var(--radius-md); background: transparent; }
.tabs { display: flex; gap: 18px; padding: 0 18px; border-bottom: 1px solid var(--border); }
.tabs button { height: 36px; padding: 0 2px; border-bottom: 2px solid transparent; background: transparent; color: var(--text-muted); }
.tabs button.active { border-bottom-color: var(--primary); color: var(--primary); }
.panel { min-height: 0; overflow-y: auto; padding: 18px; }
header, .tabs { flex-shrink: 0; }
.appearance-panel { min-height: 92px; }
.setting-row { display: flex; align-items: center; justify-content: space-between; gap: 24px; }
.setting-row > div:first-child, .rules-heading > div { display: grid; gap: 2px; }
.segmented { display: flex; padding: 3px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); }
.segmented button { display: inline-flex; height: 30px; align-items: center; gap: 5px; padding: 0 10px; border-radius: var(--radius-sm); background: transparent; color: var(--text-muted); white-space: nowrap; }
.segmented button.active { background: var(--surface-panel); color: var(--primary); box-shadow: 0 1px 3px rgb(22 34 50 / 12%); }
fieldset { display: grid; gap: 13px; min-width: 0; margin: 0; padding: 0; border: 0; }
.ai-panel label:not(.toggle-row) { display: grid; gap: 6px; color: var(--text-muted); font-size: 11px; }
input, select { width: 100%; height: 34px; min-width: 0; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); color: var(--text); }
input:focus, select:focus { border-color: var(--primary); outline: 2px solid var(--primary-soft); }
.input-with-action { position: relative; }
.input-with-action input { padding-right: 38px; }
.input-action { position: absolute; top: 2px; right: 2px; }
.rules-heading { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding-top: 3px; }
.toggle-row { display: flex; align-items: center; gap: 7px; color: var(--text-muted); font-size: 11px; }
.toggle-row input { width: 15px; height: 15px; }
.rule-row, .rule-entry { display: flex; min-width: 0; align-items: center; gap: 8px; }
.rule-row { min-height: 32px; padding-left: 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); }
.rule-row span { min-width: 0; flex: 1; overflow: hidden; font-family: var(--font-code); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
.rule-entry input { flex: 1; }
.storage-warning { color: var(--text-muted); font-size: 11px; line-height: 1.55; }
.feedback { padding: 8px 9px; border-radius: var(--radius-md); font-size: 11px; }
.feedback.success { background: var(--diff-add-bg); color: var(--success); }
.feedback.error { background: var(--danger-soft); color: var(--danger); }
details { margin-top: 6px; }
pre { overflow-wrap: anywhere; white-space: pre-wrap; }
footer { display: flex; justify-content: flex-end; gap: 8px; padding-top: 3px; }
.primary-button, .secondary-button { height: 34px; padding: 0 13px; border-radius: var(--radius-md); }
.primary-button { background: var(--primary); color: white; }
.secondary-button { border: 1px solid var(--border); background: var(--surface-panel); color: var(--text); }
button:disabled, fieldset:disabled { cursor: not-allowed; opacity: 0.65; }
</style>
