<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed } from "vue";
import { RefreshCw } from "@lucide/vue";
import { useAiStore } from "@/stores/ai";
import { useUiStore } from "@/stores/ui";
const ai = useAiStore();
const ui = useUiStore();
const message = computed(() => ai.skillLoading ? t('uiReadingRepositoryReviewSkill53472d') : ai.reviewSkill?.error?.message
  ?? (ai.reviewSkill?.state === "missing" ? t('uiRepositoryReviewSKILLMdNotFounde579d2') : ai.reviewSkill?.state === "ambiguous" ? t('uiTwoReviewSkillsFoundSpecifyADirectoryInSettingsd75565') : ai.reviewSkill?.state === "ready" ? t('uiRepositoryReviewSkillReady01de76') : t('uiRepositoryReviewSkillNotLoadedYet6bd99d')));
</script>
<template>
  <section class="skill-status" :aria-label="t('uiRepositoryReviewSkill981eda')">
    <strong>{{ message }}</strong>
    <template v-if="ai.reviewSkill?.state === 'ready' && ai.reviewSkill.info">
      <span>{{ ai.reviewSkill.info.name }} {{ ai.reviewSkill.info.version }}</span>
      <code>{{ ai.reviewSkill.info.directory }}/SKILL.md</code>
      <small>{{ t('uiUsesRulesMaintainedInThisRepositoryRevalidatedWhenReviewStare44df1') }}</small>
    </template>
    <small v-else>{{ t('uiAutoDetectsCodeReviewExpertSKILLMdOrCodeReviewExportSKILLMdcf65fb') }}</small>
    <div><button :disabled="ai.skillLoading || ai.running" :aria-label="t('uiReloadReviewSkilld86646')" @click="ai.refreshReviewSkill()"><RefreshCw :size="12" />{{ t('uiReload778497') }}</button><button @click="ui.settingsDialogOpen = true">{{ t('uiSetSkillDirectory5de3b1') }}</button></div>
  </section>
</template>
<style scoped>
.skill-status { display: grid; gap: 6px; min-width: 0; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); font-size: 11px; overflow-wrap: anywhere; }
small { color: var(--text-muted); line-height: 1.5; }code { white-space: pre-wrap; user-select: text; }div { display: flex; flex-wrap: wrap; gap: 6px; }button { display: inline-flex; align-items: center; gap: 4px; padding: 5px 7px; border-radius: var(--radius-sm); background: var(--surface-panel); color: var(--primary); }
</style>
