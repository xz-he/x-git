import { t } from '@/lib/i18n';
import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { defineStore } from "pinia";
import { computed, onScopeDispose, ref, watch } from "vue";
import { version } from "../../package.json";
import { useAiStore } from "@/stores/ai";
import { useAiChatStore } from "@/stores/aiChat";
import { useConflictsStore } from "@/stores/conflicts";
import { useConsoleStore } from "@/stores/console";
import { useRepositoryStore } from "@/stores/repository";
import { useSettingsStore } from "@/stores/settings";

type UpdatePhase = "idle" | "checking" | "current" | "available" | "downloading" | "ready" | "installing" | "installed";
const CHECK_TIMEOUT_MS = 30_000;
const DOWNLOAD_TIMEOUT_MS = 30 * 60_000;
const AUTO_CHECK_INTERVAL_MS = 30 * 60_000;
const RESUME_CHECK_INTERVAL_MS = 5 * 60_000;

export const useUpdatesStore = defineStore("updates", () => {
  const supported = isTauri();
  const currentVersion = ref(version);
  const phase = ref<UpdatePhase>("idle");
  const latest = ref<{ version: string; body: string; date?: string }>();
  const error = ref("");
  const checkedAt = ref("");
  const downloadedBytes = ref(0);
  const totalBytes = ref<number>();
  const noticeVisible = ref(false);
  let candidate: Update | null = null;
  let initialized = false;
  let lifecycle = 0;
  let lastCheckAttempt: number | undefined;
  let notifiedVersion = "";
  let autoTimer: ReturnType<typeof setInterval> | undefined;
  let stopPreferenceWatch: (() => void) | undefined;
  const busy = computed(() => ["checking", "downloading", "installing"].includes(phase.value));
  const progress = computed(() => totalBytes.value
    ? Math.min(100, Math.round(downloadedBytes.value / totalBytes.value * 100)) : undefined);
  const installBlockReason = computed(() => {
    if (useRepositoryStore().navigationBusy || useConsoleStore().busy || useAiStore().running || useAiChatStore().running)
      return t('uiWaitForGitAITasksToFinishBeforeInstalling7d77a8');
    if (useSettingsStore().saving) return t('uiWaitForSettingsToFinishSavingBeforeInstalling621a0a');
    if (useConflictsStore().hasDirtyDrafts) return t('uiSaveOrDiscardUnsavedResolutionDraftsFirst575b22');
    return "";
  });

  async function initialize(): Promise<void> {
    if (initialized || !supported) return;
    initialized = true;
    const currentLifecycle = ++lifecycle;
    try { currentVersion.value = await getVersion(); }
    catch { error.value = t('uiCouldNotReadTheCurrentAppVersionRestartTheAppAndTryAgain70347a'); return; }
    if (currentLifecycle !== lifecycle || import.meta.env.DEV) return;
    autoTimer = setInterval(checkAutomatically, AUTO_CHECK_INTERVAL_MS);
    window.addEventListener("focus", checkAutomatically);
    window.addEventListener("online", checkAutomatically);
    document.addEventListener("visibilitychange", checkAutomatically);
    stopPreferenceWatch = watch(() => useSettingsStore().settings.checkUpdatesOnStartup, enabled => {
      if (enabled) checkAutomatically();
    });
    await checkAutomatically();
  }

  async function checkAutomatically(): Promise<void> {
    if (!initialized || !useSettingsStore().settings.checkUpdatesOnStartup || document.hidden || !navigator.onLine) return;
    if (lastCheckAttempt !== undefined && Date.now() - lastCheckAttempt < RESUME_CHECK_INTERVAL_MS) return;
    await checkForUpdates({ background: true });
  }

  function dispose(): void {
    initialized = false;
    lifecycle++;
    clearInterval(autoTimer);
    autoTimer = undefined;
    stopPreferenceWatch?.();
    stopPreferenceWatch = undefined;
    window.removeEventListener("focus", checkAutomatically);
    window.removeEventListener("online", checkAutomatically);
    document.removeEventListener("visibilitychange", checkAutomatically);
  }
  onScopeDispose(dispose);

  async function checkForUpdates(options: { background?: boolean } = {}): Promise<void> {
    // Keep downloaded bytes until installation or application exit.
    if (!supported || busy.value || phase.value === "ready" || phase.value === "installed") return;
    const previousPhase = phase.value;
    lastCheckAttempt = Date.now();
    phase.value = "checking";
    error.value = "";
    downloadedBytes.value = 0;
    totalBytes.value = undefined;
    try {
      const next = await check({ timeout: CHECK_TIMEOUT_MS });
      const previous = candidate;
      candidate = next;
      // Keep an available update on transient network failure; replace it only after success.
      if (previous !== next) await previous?.close().catch(() => undefined);
      checkedAt.value = new Date().toLocaleString();
      if (candidate) {
        currentVersion.value = candidate.currentVersion;
        latest.value = { version: candidate.version, body: candidate.body ?? "", date: candidate.date };
        phase.value = "available";
        if (!options.background || notifiedVersion !== candidate.version) noticeVisible.value = true;
        notifiedVersion = candidate.version;
      } else {
        latest.value = undefined;
        noticeVisible.value = false;
        phase.value = "current";
      }
    } catch (cause) {
      phase.value = candidate ? previousPhase : "idle";
      error.value = t('msgUpdateCheckFailedCheckThatGitHubIsAccessibleAndThef33398', { p0: errorDetail(cause) });
    }
  }

  function onDownloadEvent(event: DownloadEvent): void {
    if (event.event === "Started") {
      downloadedBytes.value = 0;
      totalBytes.value = event.data.contentLength;
    } else if (event.event === "Progress") downloadedBytes.value += event.data.chunkLength;
    // Finished only means the transfer ended. Wait for download() to verify its signature.
  }

  async function download(): Promise<void> {
    if (!candidate || phase.value !== "available") return;
    phase.value = "downloading";
    error.value = "";
    downloadedBytes.value = 0;
    totalBytes.value = undefined;
    try {
      await candidate.download(onDownloadEvent, { timeout: DOWNLOAD_TIMEOUT_MS });
      phase.value = "ready";
      noticeVisible.value = true;
    } catch (cause) {
      phase.value = "available";
      error.value = t('msgDownloadOrSignatureVerificationFailedNoUpdateWasIn96a9ba', { p0: errorDetail(cause) });
    }
  }

  // Called only by the explicit confirmation button in UpdateSettings.
  async function install(): Promise<void> {
    if (!candidate || phase.value !== "ready") return;
    if (installBlockReason.value) { error.value = installBlockReason.value; return; }
    phase.value = "installing";
    error.value = "";
    noticeVisible.value = false;
    try {
      // Windows exits here and the installer restarts HQ Git.
      await candidate.install();
    } catch (cause) {
      // The native plugin may have consumed the downloaded resource on failure.
      phase.value = "available";
      error.value = t('msgInstallationDidNotCompleteDownloadAgainAndRetryc18ad7', { p0: errorDetail(cause) });
      return;
    }
    phase.value = "installed";
    await restart();
  }

  async function restart(): Promise<void> {
    if (phase.value !== "installed") return;
    if (installBlockReason.value) { error.value = installBlockReason.value; return; }
    phase.value = "installing";
    try { await relaunch(); }
    catch (cause) {
      phase.value = "installed";
      error.value = t('msgUpdateInstalledCloseAndReopenTheAppManually3e0608', { p0: errorDetail(cause) });
    }
  }

  return { supported, currentVersion, phase, latest, error, checkedAt, downloadedBytes, totalBytes,
    noticeVisible, busy, progress, installBlockReason, initialize, dispose, checkForUpdates, download, install, restart };
});

function errorDetail(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  return message ? `\n${message}` : "";
}
