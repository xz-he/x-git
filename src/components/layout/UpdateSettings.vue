<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, ref } from "vue";
import { ArrowDownToLine, CircleCheck, LoaderCircle, RefreshCw } from "@lucide/vue";
import { useUpdatesStore } from "@/stores/updates";
import { useSettingsStore } from "@/stores/settings";

const updates = useUpdatesStore();
const settings = useSettingsStore();
const confirmInstall = ref(false);
const preferenceError = ref("");
const status = computed(() => ({
  get idle() { return t('uiCheckForNewReleasesOnGitHub8799ea'); }, get checking() { return t('uiCheckingForUpdatesc8bea4'); }, get current() { return t('uiYouAreUpToDatedb620f'); },
  get available() { return t('uiNewVersionAvailable010474'); }, get downloading() { return t('uiDownloadingAndVerifyingUpdate720bd3'); }, get ready() { return t('uiDownloadCompleteSignatureVerified648186'); },
  get installing() { return t('uiStartingInstaller1e5738'); }, get installed() { return t('uiUpdateInstalledWaitingToRestart530194'); },
}[updates.phase]));
function bytes(value: number): string { return `${(value / (1024 * 1024)).toFixed(1)} MB`; }
async function setAutoCheck(event: Event): Promise<void> {
  preferenceError.value = "";
  try { await settings.save({ ...settings.settings, checkUpdatesOnStartup: (event.target as HTMLInputElement).checked }); }
  catch { preferenceError.value = t('uiCouldNotSaveAutomaticUpdateSettingsPleaseTryAgainb626a0'); }
}
async function install(): Promise<void> {
  confirmInstall.value = false;
  await updates.install();
}
</script>

<template>
  <section class="update-settings" :aria-label="t('uiUpdatesca9576')">
    <div class="version-heading"><strong>HQ Git</strong><span>{{ t('uiCurrentVersionV0239b8') }}{{ updates.currentVersion }}</span></div>
    <label class="auto-check"><input type="checkbox" :checked="settings.settings.checkUpdatesOnStartup" :disabled="settings.saving" @change="setAutoCheck" />{{ t('uiCheckForUpdatesAutomaticallyNotifyOnlyNoAutomaticDownloadOrIad630c') }}</label>
    <p class="muted">{{ t('uiChecksOnStartupAndEvery30MinutesAndWhenReturningToTheWindowOc02df3') }}</p>
    <p v-if="preferenceError" class="error" role="alert">{{ preferenceError }}</p>
    <p v-if="!updates.supported" class="muted">{{ t('uiUpdatesAreAvailableOnlyInTheInstalledDesktopAppNotInBrowserP8a70d0') }}</p>
    <div v-else class="update-state">
      <p role="status" class="status"><LoaderCircle v-if="updates.busy" :size="17" class="spin" /><CircleCheck v-else-if="updates.phase === 'current' || updates.phase === 'ready'" :size="17" />{{ status }}</p>
      <p v-if="updates.checkedAt" class="muted">{{ t('uiLastChecked643cfa') }}{{ updates.checkedAt }}</p>
      <template v-if="updates.latest">
        <strong>{{ t('uiNewVersionVda0cfd') }}{{ updates.latest.version }}</strong>
        <p v-if="updates.latest.date" class="muted">{{ t('uiPublished55f0f1') }}{{ new Date(updates.latest.date).toLocaleString() }}</p>
        <details><summary>{{ t('uiReleaseNotesc62021') }}</summary><pre>{{ updates.latest.body || t('uiNoReleaseNotesProvideda7be1e') }}</pre></details>
      </template>
      <div v-if="updates.phase === 'downloading'" class="download-progress">
        <progress :value="updates.progress" max="100" :aria-label="t('uiUpdateDownloadProgress909ed3')" />
        <span>{{ bytes(updates.downloadedBytes) }}<template v-if="updates.totalBytes"> / {{ bytes(updates.totalBytes) }} · {{ updates.progress }}%</template><template v-else> {{ t('uiDownloading19b4a6') }}</template></span>
      </div>
      <p v-if="updates.error" class="error" role="alert">{{ updates.error }}</p>
      <p v-if="updates.phase === 'ready' && updates.installBlockReason" class="muted">{{ updates.installBlockReason }}</p>
      <div v-if="confirmInstall && updates.phase === 'ready'" class="confirmation" role="alertdialog" :aria-label="t('uiConfirmUpdateInstallation80157b')">
        <strong>{{ t('uiInstallTheUpdateAndRestartHQGit2bc0af') }}</strong>
        <p>{{ t('uiInstallationWillCloseTheAppSaveAnyUnsavedCommitMessagesAndAI90c17d') }}</p>
        <div class="actions"><button @click="confirmInstall = false">Cancel</button><button class="primary" :disabled="!!updates.installBlockReason" @click="install">Install &amp; Restart</button></div>
      </div>
      <div v-else class="actions">
        <button v-if="!['ready', 'installed'].includes(updates.phase)" :disabled="updates.busy" @click="updates.checkForUpdates()"><RefreshCw :size="14" />Check for Updates</button>
        <button v-if="updates.phase === 'available' || updates.phase === 'downloading'" class="primary" :disabled="updates.busy" @click="updates.download"><ArrowDownToLine :size="14" />{{ updates.phase === 'downloading' ? 'Downloading…' : 'Download Update' }}</button>
        <button v-if="updates.phase === 'ready'" class="primary" :disabled="!!updates.installBlockReason" @click="confirmInstall = true">Install Update…</button>
        <button v-if="updates.phase === 'installed'" class="primary" :disabled="!!updates.installBlockReason" @click="updates.restart">Restart</button>
      </div>
      <p class="muted">{{ t('uiYouCanKeepWorkingDuringDownloadOrCloseThisWindowAndReturnToI3c82dc') }}</p>
    </div>
  </section>
</template>

<style scoped>
.update-settings, .update-state { display: grid; gap: 14px; }
.version-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
.version-heading strong { font-size: 19px; }
.version-heading span, .muted { color: var(--text-muted); }
p { margin: 0; line-height: 1.6; }
.auto-check, .status { display: flex; gap: 8px; align-items: center; }
.auto-check { line-height: 1.6; }
.auto-check input { accent-color: var(--primary); }
.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }
button { display: inline-flex; align-items: center; gap: 6px; min-height: 34px; padding: 7px 12px; border: 1px solid var(--border); border-radius: var(--radius-md); color: var(--text); background: var(--surface-panel); }
button.primary { color: white; background: var(--primary); border-color: var(--primary); }
button:disabled { opacity: .55; cursor: not-allowed; }
.error { padding: 10px; color: var(--danger); background: var(--danger-soft); border-radius: var(--radius-md); white-space: pre-wrap; overflow-wrap: anywhere; }
.confirmation { display: grid; gap: 12px; padding: 14px; background: var(--surface-muted); border: 1px solid var(--border); border-radius: var(--radius-md); }
pre { max-height: 200px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font: inherit; line-height: 1.6; }
summary { cursor: pointer; color: var(--primary); }
.download-progress { display: grid; gap: 6px; color: var(--text-muted); }
progress { width: 100%; height: 10px; accent-color: var(--primary); }
.spin { animation: update-spin 1s linear infinite; }
@keyframes update-spin { to { transform: rotate(360deg); } }
</style>
