<script setup lang="ts">
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
    const path = await dialogs.selectDirectory("切换仓库");
    if (!path) return;
    if (repositories.navigationBusy) {
      switchError.value = "请等待当前操作完成后再切换仓库。";
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
  { label: "仓库文件", view: "files" },
  { label: "文件状态", view: "changes" },
  { label: "提交记录", view: "history" },
  { label: "分支", view: "branches" },
  { label: "标签", view: "tags" },
  { label: "远程", view: "remotes" },
];
</script>
<template>
  <header class="topbar">
    <nav aria-label="工作区模块"><button v-for="module in modules" :key="module.view" :data-view="module.view" :class="{ active: ui.activeView === module.view }" :disabled="repositories.viewNavigationBusy" :aria-current="ui.activeView === module.view ? 'page' : undefined" @click="ui.openView(module.view)">{{ module.label }}</button></nav>
    <div class="actions">
      <UpdateButton />
      <button class="icon-button" aria-label="操作历史" :title="activityWarning || '操作历史'" @click="activityOpen = true"><History :size="17" /><span v-if="activityWarning" class="activity-warning">!</span></button>
      <button class="icon-button" aria-label="返回首页" title="返回首页" :disabled="files.submitting" @click="ui.homeVisible = true"><House :size="17" /></button>
      <button class="icon-button" aria-label="切换仓库" title="切换仓库" :disabled="selectingRepository || repositories.navigationBusy" @click="switchRepository"><FolderOpen :size="17" /></button>
      <button class="icon-button" aria-label="刷新仓库" title="刷新仓库" :disabled="repositories.navigationBusy" @click="$emit('refresh')"><RefreshCw :size="17" /></button>
      <button class="icon-button ai-button" aria-label="打开 AI 助手" title="打开 AI 助手" @click="$emit('openAi')"><Bot :size="17" /></button>
      <button class="icon-button" aria-label="打开设置" title="打开设置" @click="$emit('openSettings')"><Settings :size="17" /></button>
    </div>
    <div v-if="switchError" class="switch-error" role="alert"><span>{{ switchError }}</span><button class="icon-button" aria-label="关闭切换错误" title="关闭" @click="switchError = ''"><X :size="15" /></button></div>
    <ActivityDialog v-if="activityOpen" @close="activityOpen = false" />
  </header>
</template>
<style scoped>
.topbar { grid-column: 2 / -1; display: flex; align-items: center; justify-content: space-between; padding: 0 16px; border-bottom: 1px solid var(--border); background: var(--surface-panel); }
nav { display: flex; align-self: stretch; gap: 18px; }
nav button { position: relative; min-width: 44px; background: transparent; color: var(--text-muted); }
nav button.active { color: var(--text); font-weight: 600; }
nav button.active::after { position: absolute; right: 5px; bottom: 0; left: 5px; height: 2px; background: var(--primary); content: ""; }
.actions { display: flex; gap: 6px; }
.activity-warning { color: var(--danger); }
.icon-button { display: grid; width: 32px; height: 32px; place-items: center; border: 1px solid transparent; border-radius: var(--radius-md); background: transparent; }
.icon-button:hover:not(:disabled) { border-color: var(--border); background: var(--surface-muted); }
.ai-button { color: var(--primary); background: var(--primary-soft); }
.switch-error { position: fixed; z-index: 30; top: 60px; right: 16px; display: flex; align-items: center; gap: 10px; max-width: min(440px, calc(100vw - 32px)); padding: 8px 12px; border: 1px solid var(--danger); border-radius: var(--radius-md); background: var(--surface-panel); color: var(--danger); box-shadow: var(--shadow-window); }
.switch-error span { min-width: 0; overflow-wrap: anywhere; }
.switch-error button { flex: 0 0 auto; }
</style>
