<script setup lang="ts">
import { t } from '@/lib/i18n';
import { Download, LoaderCircle } from "@lucide/vue";
import { computed } from "vue";
import { useUpdatesStore } from "@/stores/updates";
import { useUiStore } from "@/stores/ui";

const updates = useUpdatesStore();
const ui = useUiStore();
const label = computed(() => {
  if (updates.phase === "downloading") return t('msgDownloadingUpdatede91f8', { p0: updates.progress === undefined ? '' : ` ${updates.progress}%` });
  if (updates.phase === "ready") return t('msgUpdateVDownloadedClickToInstallc8863e', { p0: updates.latest?.version });
  if (updates.latest) return t('msgNewVersionVAvailableViewUpdate6fb19f', { p0: updates.latest.version });
  if (updates.phase === "checking") return t('uiCheckingForUpdates7d9574');
  if (updates.error) return t('uiUpdateCheckFailedClickForDetailsa8d8fa');
  return t('msgUpdatesCurrentV528468', { p0: updates.currentVersion });
});
function open() {
  ui.updateDialogOpen = true;
  updates.noticeVisible = false;
  if (!updates.latest) void updates.checkForUpdates();
}
</script>

<template>
  <button class="update-button" :class="{ available: updates.latest }" :aria-label="t('uiUpdatesca9576')" :title="label" @click="open">
    <LoaderCircle v-if="updates.busy" :size="17" class="spin" /><Download v-else :size="17" />
    <span v-if="updates.latest" class="update-dot" :aria-label="t('uiUpdateAvailablefb725d')" />
    <span v-else-if="updates.error" class="update-error" :aria-label="t('uiUpdateCheckFailed9e9358')">!</span>
  </button>
</template>

<style scoped>
.update-button { position: relative; display: grid; flex: 0 0 32px; width: 32px; height: 32px; place-items: center; border: 1px solid transparent; border-radius: var(--radius-md); background: transparent; color: var(--text-muted); }
.update-button:hover { border-color: var(--border); background: var(--surface-muted); }
.available { color: var(--primary); background: var(--primary-soft); }
.update-dot { position: absolute; top: 3px; right: 3px; width: 6px; height: 6px; border-radius: 50%; background: var(--primary); }
.update-error { position: absolute; top: 0; right: 2px; color: var(--danger); font-size: 11px; }
.spin { animation: update-button-spin 1s linear infinite; }
@keyframes update-button-spin { to { transform: rotate(360deg); } }
</style>
