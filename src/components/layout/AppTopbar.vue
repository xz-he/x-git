<script setup lang="ts">
import { language, t } from '@/lib/i18n';
import { Bot, FolderOpen, History, House, RefreshCw, Settings, X } from "@lucide/vue";
import ActivityDialog from "@/features/activity/ActivityDialog.vue";
import UpdateButton from "./UpdateButton.vue";
import { activityWarning } from "@/lib/backend/activity";
import { ref } from "vue";
import { dialogs } from "@/lib/backend/dialogs";
import { normalizeBackendError } from "@/lib/backend/errors";
import { useRepositoryStore } from "@/stores/repository";
import type { WorkspaceView } from "@/stores/ui";
import { useUiStore } from "@/stores/ui";
import { useFilesStore } from "@/stores/files";

defineEmits<{ openAi: []; openSettings: []; refresh: [] }>();
const ui = useUiStore();
const files = useFilesStore();
const repositories = useRepositoryStore();
const selectingRepository = ref(false);
const switchError = ref("");
const activityOpen = ref(false);

async function switchRepository(): Promise<void> {
  if (selectingRepository.value || repositories.navigationBusy) return;
  selectingRepository.value = true;
  switchError.value = "";
  try {
    const path = await dialogs.selectDirectory(t('uiSwitchRepository46c7e1'));
    if (!path) return;
    if (repositories.navigationBusy) {
      switchError.value = t('uiWaitForTheCurrentOperationToFinishBeforeSwitchingRepositorie73a652');
      return;
    }
    await repositories.open(path);
  } catch (cause) {
    switchError.value = normalizeBackendError(cause).message;
  } finally {
    selectingRepository.value = false;
  }
}
const modules: Array<{ label: string; view: WorkspaceView }> = [
  { get label() { return t('files'); }, view: "files" },
  { get label() { return t('changes'); }, view: "changes" },
  { get label() { return t('history'); }, view: "history" },
  { get label() { return t('branches'); }, view: "branches" },
  { get label() { return t('tags'); }, view: "tags" },
  { get label() { return t('remotes'); }, view: "remotes" },
];
</script>
<template>
  <header class="topbar">
    <nav :aria-label="t('uiWorkspaceModules682898')"><button v-for="module in modules" :key="module.view" :data-view="module.view" :class="{ active: ui.activeView === module.view }" :aria-label="module.label" :disabled="repositories.viewNavigationBusy" :aria-current="ui.activeView === module.view ? 'page' : undefined" @click="ui.openView(module.view)"><span>{{ language === 'bilingual' ? t(module.view, {}, 'en') : module.label }}</span><small v-if="language === 'bilingual'">{{ t(module.view, {}, 'zh-CN') }}</small></button></nav>
    <div class="actions">
      <UpdateButton />
      <button class="icon-button" :aria-label="t('uiOperationHistory56833a')" :title="activityWarning || t('uiOperationHistory56833a')" @click="activityOpen = true"><History :size="17" /><span v-if="activityWarning" class="activity-warning">!</span></button>
      <button class="icon-button" :aria-label="t('uiHome8befab')" :title="t('uiHome8befab')" :disabled="files.submitting" @click="ui.homeVisible = true"><House :size="17" /></button>
      <button class="icon-button" :aria-label="t('uiSwitchRepository46c7e1')" :title="t('uiSwitchRepository46c7e1')" :disabled="selectingRepository || repositories.navigationBusy" @click="switchRepository"><FolderOpen :size="17" /></button>
      <button class="icon-button" :aria-label="t('uiRefreshRepository70ad8c')" :title="t('uiRefreshRepository70ad8c')" :disabled="repositories.navigationBusy" @click="$emit('refresh')"><RefreshCw :size="17" /></button>
      <button class="icon-button ai-button" :aria-label="t('uiOpenAIAssistantf4bad3')" :title="t('uiOpenAIAssistantf4bad3')" @click="$emit('openAi')"><Bot :size="17" /></button>
      <button class="icon-button" :aria-label="t('uiOpenSettings857329')" :title="t('uiOpenSettings857329')" @click="$emit('openSettings')"><Settings :size="17" /></button>
    </div>
    <div v-if="switchError" class="switch-error" role="alert"><span>{{ switchError }}</span><button class="icon-button" :aria-label="t('uiDismissRepositorySwitchError13318f')" :title="t('uiClose6c14bd')" @click="switchError = ''"><X :size="15" /></button></div>
    <ActivityDialog v-if="activityOpen" @close="activityOpen = false" />
  </header>
</template>
<style scoped>
.topbar { grid-column: 2 / -1; display: flex; align-items: center; justify-content: space-between; padding: 0 16px; border-bottom: 1px solid var(--border); background: var(--surface-panel); }
nav { display: flex; min-width: 0; align-self: stretch; gap: 12px; overflow-x: auto; }
nav button { position: relative; display: flex; flex: 0 0 auto; flex-direction: column; align-items: center; justify-content: center; gap: 2px; min-width: 44px; white-space: nowrap; background: transparent; color: var(--text-muted); }
nav button small { font-size: 11px; font-weight: 400; }
nav button.active { color: var(--text); font-weight: 600; }
nav button.active::after { position: absolute; right: 5px; bottom: 0; left: 5px; height: 2px; background: var(--primary); content: ""; }
.actions { display: flex; flex-shrink: 0; gap: 6px; padding-left: 10px; }
.activity-warning { color: var(--danger); }
.icon-button { display: grid; width: 32px; height: 32px; place-items: center; border: 1px solid transparent; border-radius: var(--radius-md); background: transparent; }
.icon-button:hover:not(:disabled) { border-color: var(--border); background: var(--surface-muted); }
.ai-button { color: var(--primary); background: var(--primary-soft); }
.switch-error { position: fixed; z-index: 30; top: 60px; right: 16px; display: flex; align-items: center; gap: 10px; max-width: min(440px, calc(100vw - 32px)); padding: 8px 12px; border: 1px solid var(--danger); border-radius: var(--radius-md); background: var(--surface-panel); color: var(--danger); box-shadow: var(--shadow-window); }
.switch-error span { min-width: 0; overflow-wrap: anywhere; }
.switch-error button { flex: 0 0 auto; }
</style>
