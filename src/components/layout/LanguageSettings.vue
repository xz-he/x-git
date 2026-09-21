<script setup lang="ts">
import { ref } from "vue";
import AppSelect from "@/components/common/AppSelect.vue";
import { t, type AppLanguage } from "@/lib/i18n";
import { useSettingsStore } from "@/stores/settings";

const settings = useSettingsStore();
const failed = ref(false);
const options: { value: AppLanguage; label: string }[] = [
  { value: "zh-CN", label: "中文" },
  { value: "en", label: "English" },
  { value: "bilingual", label: "中英对照 / English + 中文" },
];
async function select(language: AppLanguage) {
  failed.value = false;
  try { await settings.save({ ...settings.settings, language }); }
  catch { failed.value = true; }
}
</script>
<template>
  <section class="language-settings" data-testid="language-settings">
    <div><strong>{{ t('language') }}</strong><p>{{ t('languageHint') }}</p></div>
    <AppSelect :model-value="settings.settings.language" :options="options" aria-label="Language / 语言" :disabled="settings.saving" @update:model-value="select" />
    <p v-if="failed" class="error" role="alert">{{ t('languageError') }}</p>
  </section>
</template>
<style scoped>
.language-settings { display: grid; grid-template-columns: minmax(0, 1fr) minmax(180px, 240px); align-items: center; gap: 12px; margin-bottom: 22px; padding-bottom: 18px; border-bottom: 1px solid var(--border); }
p { margin: 5px 0 0; color: var(--text-muted); font-size: 12px; line-height: 1.5; }.error { grid-column: 1 / -1; color: var(--danger); }
</style>
