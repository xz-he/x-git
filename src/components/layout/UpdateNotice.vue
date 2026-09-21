<script setup lang="ts">
import { t } from '@/lib/i18n';
import { useUpdatesStore } from "@/stores/updates";
import { useUiStore } from "@/stores/ui";
const updates = useUpdatesStore();
const ui = useUiStore();
function open(): void { ui.updateDialogOpen = true; updates.noticeVisible = false; }
</script>
<template>
  <div v-if="updates.phase === 'installing'" class="install-overlay" role="alertdialog" aria-modal="true" :aria-label="t('uiInstallingUpdate515926')"><p>{{ t('uiStartingTheUpdateInstallerPleaseWaitf0fa17') }}</p></div>
  <aside v-else-if="updates.noticeVisible && !ui.settingsDialogOpen && !ui.updateDialogOpen" class="update-notice" role="status">
    <span>{{ updates.phase === 'ready' ? t('uiUpdateDownloadeddf0ebf') : t('uiNewVersionAvailable010474') }} v{{ updates.latest?.version }}</span>
    <button @click="open">View Update</button><button :aria-label="t('uiUpdateLater992321')" @click="updates.noticeVisible = false">Later</button>
  </aside>
</template>
<style scoped>
.update-notice { position: fixed; top: 65px; right: 20px; z-index: 45; display: flex; align-items: center; gap: 12px; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); box-shadow: var(--shadow-window); }
button { color: var(--primary); background: transparent; padding: 5px; }
.install-overlay { position: fixed; inset: 0; z-index: 1000; display: grid; place-items: center; background: var(--surface-app); color: var(--text); }
</style>
