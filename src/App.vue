<script setup lang="ts">
import { language, t } from '@/lib/i18n';
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { applyFontPreferences } from "@/lib/fonts";
import AppSidebar from "@/components/layout/AppSidebar.vue";
import RepositorySwitcher from "@/components/layout/RepositorySwitcher.vue";
import AppTopbar from "@/components/layout/AppTopbar.vue";
import ContextPanel from "@/components/layout/ContextPanel.vue";
import DetailPanel from "@/components/layout/DetailPanel.vue";
import AppStatusbar from "@/components/layout/AppStatusbar.vue";
import AiDrawerShell from "@/components/layout/AiDrawerShell.vue";
import SettingsDialog from "@/components/layout/SettingsDialog.vue";
import GitFeedbackPanel from "@/components/layout/GitFeedbackPanel.vue";
import UpdateNotice from "@/components/layout/UpdateNotice.vue";
import UpdateDialog from "@/components/layout/UpdateDialog.vue";
import { useUpdatesStore } from "@/stores/updates";
import WelcomeView from "@/features/repository/WelcomeView.vue";
import ConflictBanner from "@/features/conflicts/ConflictBanner.vue";
import ConflictDialogs from "@/features/conflicts/ConflictDialogs.vue";
import FileDialogs from "@/features/files/FileDialogs.vue";
import { useRepositoryStore } from "@/stores/repository";
import { useRepositoryMonitorStore } from "@/stores/repositoryMonitor";
import { useAiStore } from "@/stores/ai";
import { useAiChatStore } from "@/stores/aiChat";
import { onGitFailure } from "@/lib/gitFailure";
import { useRemotesStore } from "@/stores/remotes";
import { useSettingsStore } from "@/stores/settings";
import { useUiStore } from "@/stores/ui";
import { useConsoleStore } from "@/stores/console";
import { useTerminalStore } from "@/stores/terminal";

const repositories = useRepositoryStore();
const repositoryMonitor = useRepositoryMonitorStore();
const ai = useAiStore();
const chat = useAiChatStore();
const stopGitFailures = onGitFailure(chat.captureFailure);
const remotes = useRemotesStore();
const settingsStore = useSettingsStore();
const ui = useUiStore();
const consoleStore = useConsoleStore();
const terminal = useTerminalStore();
const updates = useUpdatesStore();
const viewportWidth = ref(window.innerWidth);
const sidebarWidth = computed(() => ui.sidebarCollapsed ? 56 : language.value === 'bilingual' ? 252 : 200);
const maxContextWidth = computed(() => Math.max(270, Math.min(1000, Math.max(1100, viewportWidth.value) - sidebarWidth.value - (settingsStore.settings.aiDrawerOpen ? drawerWidth.value : 0) - 280)));
const contextWidth = computed(() => Math.min(ui.contextWidth, maxContextWidth.value));
function updateViewport(): void { viewportWidth.value = window.innerWidth; }
watch(() => [settingsStore.settings.fontFamily, settingsStore.settings.codeFontFamily],
  () => applyFontPreferences(settingsStore.settings), { immediate: true });
let applicationDisposed = false;
const switchingRepository = computed(() =>
  repositories.operation.kind !== "idle" && repositories.operation.kind !== "refresh",
);
const drawerWidth = computed(() =>
  Math.min(560, Math.max(300, settingsStore.settings.aiDrawerWidth)),
);
function updateDrawer(open: boolean) {
  settingsStore.settings.aiDrawerOpen = open;
  void settingsStore.save({ ...settingsStore.settings }).catch(() => undefined);
}
function resizeDrawer(width: number) {
  settingsStore.settings.aiDrawerWidth = width;
}
function persistDrawerWidth() {
  void settingsStore.save({ ...settingsStore.settings }).catch(() => undefined);
}
async function startApplication() {
  await ai.initialize().catch(() => undefined);
  if (applicationDisposed) return;
  await remotes.initialize().catch(() => undefined);
  if (applicationDisposed) return;
  await consoleStore.initialize().catch(() => undefined);
  if (applicationDisposed) return;
  await terminal.initialize().catch(() => undefined);
  if (applicationDisposed) return;
  await settingsStore.load().catch(() => undefined);
  if (applicationDisposed) return;
  ui.applyTheme(settingsStore.settings.theme);
  void updates.initialize();
  const lastRepository = settingsStore.settings.lastRepoPath;
  if (!repositories.snapshot && lastRepository) {
    await repositories.open(lastRepository).catch(() => undefined);
  }
}
onMounted(() => {
  repositoryMonitor.initialize();
  window.addEventListener("resize", updateViewport);
  void startApplication();
});
onBeforeUnmount(() => {
  updates.dispose();
  repositoryMonitor.dispose();
  window.removeEventListener("resize", updateViewport);
  applicationDisposed = true;
  ai.dispose();
  stopGitFailures();
  chat.resetForRepository();
  remotes.dispose();
  consoleStore.dispose();
  terminal.dispose();
});
</script>
<template>
  <div style="display: contents" :inert="updates.phase === 'installing' || undefined">
  <div v-if="repositories.snapshot && !ui.homeVisible" class="app-shell" :style="{ '--sidebar-width': sidebarWidth + 'px', '--context-width': contextWidth + 'px' }" :inert="switchingRepository || undefined" :aria-busy="switchingRepository">
    <AppSidebar v-show="!ui.diffFullscreen" />
    <AppTopbar v-show="!ui.diffFullscreen" @open-ai="updateDrawer(true)" @open-settings="ui.settingsDialogOpen = true" @refresh="void repositories.refresh()" />
    <ConflictBanner v-show="!ui.diffFullscreen" />
    <ContextPanel v-show="!ui.diffFullscreen" :repository="repositories.snapshot" :width="contextWidth" :max-width="maxContextWidth" />
    <DetailPanel :repository="repositories.snapshot" />
    <AppStatusbar v-show="!ui.diffFullscreen" :repository="repositories.snapshot" />
    <AiDrawerShell v-if="settingsStore.settings.aiDrawerOpen" v-show="!ui.diffFullscreen" :width="drawerWidth" @close="updateDrawer(false)" @resize="resizeDrawer" @resize-end="persistDrawerWidth" />
  </div>
  <WelcomeView v-else />
  <div v-if="repositories.snapshot && !ui.homeVisible && switchingRepository" class="repository-loading" role="status" :aria-label="t('uiRepositoryLoadingStatus3285fd')">{{ t('uiSwitchingRepositories049d92') }}</div>
  <SettingsDialog v-if="ui.settingsDialogOpen" @close="ui.settingsDialogOpen = false" />
  <UpdateDialog v-if="ui.updateDialogOpen" />
  <ConflictDialogs />
  <FileDialogs />
  <GitFeedbackPanel />
  </div>
  <UpdateNotice />
  <RepositorySwitcher />
</template>
<style scoped>
.app-shell {
  display: grid;
  grid-template-columns: var(--sidebar-width, 200px) var(--context-width, 320px) minmax(0, 1fr) auto;
  grid-template-rows: 56px auto minmax(0, 1fr) 48px;
  min-width: 1100px;
  min-height: 700px;
  height: 100vh;
  background: var(--surface-app);
}
.repository-loading {
  position: fixed;
  z-index: 40;
  inset: 0;
  display: grid;
  place-items: center;
  background: color-mix(in srgb, var(--surface-app) 75%, transparent);
  color: var(--text);
}
</style>
