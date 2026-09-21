<script setup lang="ts">
import { t } from '@/lib/i18n';
import { onMounted, onBeforeUnmount, ref, watch } from "vue";
import { Maximize2, Minimize2 } from "@lucide/vue";
import { useRepositoryStore } from "@/stores/repository";
import type { RepositorySnapshot } from "@/lib/backend/types";
import CommitPanel from "@/features/changes/CommitPanel.vue";
import DiffViewer from "@/features/changes/DiffViewer.vue";
import HistoryDetail from "@/features/history/HistoryDetail.vue";
import RefDetail from "@/features/refs/RefDetail.vue";
import RemoteDetail from "@/features/remotes/RemoteDetail.vue";
import StashDetail from "@/features/stashes/StashDetail.vue";
import ConflictDetail from "@/features/conflicts/ConflictDetail.vue";
import FileDetail from "@/features/files/FileDetail.vue";
import ConsoleDetail from "@/features/terminal/ConsoleDetail.vue";
import { useChangesStore } from "@/stores/changes";
import { useUiStore } from "@/stores/ui";
import { useTerminalStore } from "@/stores/terminal";

defineProps<{ repository?: RepositorySnapshot }>();
const changesStore = useChangesStore();
const ui = useUiStore();
const terminal = useTerminalStore();
const repositories = useRepositoryStore();
const fullscreenButton = ref<HTMLButtonElement>();
function exitFullscreen(event: KeyboardEvent): void {
  if (event.key === 'Escape' && ui.diffFullscreen) {
    event.preventDefault();
    ui.diffFullscreen = false;
    fullscreenButton.value?.focus();
  }
}
watch([() => ui.activeView, () => repositories.snapshot?.rootPath, () => repositories.generation], () => { ui.diffFullscreen = false; });
onMounted(() => window.addEventListener('keydown', exitFullscreen));
onBeforeUnmount(() => {
  window.removeEventListener('keydown', exitFullscreen);
  ui.diffFullscreen = false;
});
</script>
<template>
  <main class="detail" :class="{ 'diff-fullscreen': ui.diffFullscreen && ui.activeView === 'changes' }" data-testid="detail-panel" :inert="terminal.busy && ui.activeView === 'conflicts' || undefined">
    <header v-if="ui.activeView === 'changes'">
      <div>
        <span>{{ changesStore.selectedScope === "staged" ? t('uiStagedDiff495abb') : t('uiUnstagedDiff79bbad') }}</span>
        <strong>{{ changesStore.selectedPath ?? repository?.name ?? t('noRepository') }}</strong>
      </div>
      <button ref="fullscreenButton" class="fullscreen-button" :aria-label="ui.diffFullscreen ? t('uiExitFullScreenb2440b') : t('uiViewFileDiffInFullScreen7391c4')" :title="ui.diffFullscreen ? t('uiExitFullScreenEsce0b9ee') : t('uiViewFileDiffInFullScreen7391c4')" :aria-pressed="ui.diffFullscreen" :disabled="!ui.diffFullscreen && !changesStore.selectedDiff" @click="ui.diffFullscreen = !ui.diffFullscreen">
        <Minimize2 v-if="ui.diffFullscreen" :size="16" /><Maximize2 v-else :size="16" />{{ ui.diffFullscreen ? t('uiExitFullScreenEsce0b9ee') : t('uiFullScreen7b2376') }}
      </button>
    </header>
    <template v-if="ui.activeView === 'changes'">
      <div class="diff-area"><p v-if="ui.diffFullscreen && changesStore.error" class="fullscreen-error" role="alert">{{ changesStore.error.message }}</p><DiffViewer /></div>
      <CommitPanel v-show="!ui.diffFullscreen" />
    </template>
    <RefDetail v-else-if="ui.activeView === 'branches' || ui.activeView === 'tags'" :mode="ui.activeView" />
    <HistoryDetail v-else-if="ui.activeView === 'history'" />
    <RemoteDetail v-else-if="ui.activeView === 'remotes'" />
    <StashDetail v-else-if="ui.activeView === 'stashes'" />
    <ConflictDetail v-else-if="ui.activeView === 'conflicts'" />
    <FileDetail v-else-if="ui.activeView === 'files'" />
    <ConsoleDetail v-else-if="ui.activeView === 'terminal'" />
    <div v-else class="module-state">{{ t('uiThisModuleWillBeAvailableInAFutureUpdatec1c06b') }}</div>
  </main>
</template>
<style scoped>
.detail { container-type: inline-size; grid-column: 3; grid-row: 3; display: grid; grid-template-rows: auto minmax(0, 1fr) auto; min-width: 0; min-height: 0; background: var(--surface-app); overflow: hidden; }
header { display: flex; height: 56px; align-items: center; padding: 0 24px; border-bottom: 1px solid var(--border); background: var(--surface-panel); }
header div { display: grid; gap: 4px; }
header div { min-width: 0; }
header div { flex: 1; }
header { gap: 12px; }
.fullscreen-button { display: inline-flex; align-items: center; gap: 6px; flex-shrink: 0; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface-panel); font-size: 12px; }
.detail.diff-fullscreen { position: fixed; inset: 0; z-index: 70; grid-template-rows: auto minmax(0, 1fr); }
.diff-area { display: flex; flex-direction: column; min-height: 0; min-width: 0; overflow: hidden; }
.diff-area > .diff-viewer { flex: 1; }
.fullscreen-error { margin: 0; padding: 8px 12px; color: var(--danger); background: var(--danger-soft); }
header span { color: var(--text-muted); font-size: 11px; }
header strong { overflow: hidden; font-size: 16px; text-overflow: ellipsis; white-space: nowrap; }
</style>
