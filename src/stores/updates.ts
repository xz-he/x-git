import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { defineStore } from "pinia";
import { computed, ref } from "vue";
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
  const busy = computed(() => ["checking", "downloading", "installing"].includes(phase.value));
  const progress = computed(() => totalBytes.value
    ? Math.min(100, Math.round(downloadedBytes.value / totalBytes.value * 100)) : undefined);
  const installBlockReason = computed(() => {
    if (useRepositoryStore().navigationBusy || useConsoleStore().busy || useAiStore().running || useAiChatStore().running)
      return "请等待 Git / AI 任务结束后再安装。";
    if (useSettingsStore().saving) return "请等待设置保存完成后再安装。";
    if (useConflictsStore().hasDirtyDrafts) return "请先保存或放弃未保存的冲突解决草稿。";
    return "";
  });

  async function initialize(): Promise<void> {
    if (initialized || !supported) return;
    initialized = true;
    try { currentVersion.value = await getVersion(); }
    catch { error.value = "无法读取当前应用版本，请重启应用后重试。"; return; }
    if (useSettingsStore().settings.checkUpdatesOnStartup && !import.meta.env.DEV) await checkForUpdates();
  }

  async function checkForUpdates(): Promise<void> {
    // Keep downloaded bytes until installation or application exit.
    if (!supported || busy.value || phase.value === "ready" || phase.value === "installed") return;
    phase.value = "checking";
    error.value = "";
    latest.value = undefined;
    noticeVisible.value = false;
    downloadedBytes.value = 0;
    totalBytes.value = undefined;
    try {
      const previous = candidate;
      candidate = null;
      // Resource cleanup must not prevent a fresh network check after a failed install.
      await previous?.close().catch(() => undefined);
      candidate = await check({ timeout: CHECK_TIMEOUT_MS });
      checkedAt.value = new Date().toLocaleString();
      if (candidate) {
        currentVersion.value = candidate.currentVersion;
        latest.value = { version: candidate.version, body: candidate.body ?? "", date: candidate.date };
        phase.value = "available";
        noticeVisible.value = true;
      } else {
        phase.value = "current";
      }
    } catch (cause) {
      phase.value = "idle";
      error.value = `检查更新失败。请确认网络可访问 GitHub，且 Release 已发布 latest.json。${errorDetail(cause)}`;
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
      error.value = `下载或签名校验失败，未安装任何更新，可以重试。${errorDetail(cause)}`;
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
      error.value = `安装未完成，请重新下载后重试。${errorDetail(cause)}`;
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
      error.value = `更新已安装，请手动退出并重新打开应用。${errorDetail(cause)}`;
    }
  }

  return { supported, currentVersion, phase, latest, error, checkedAt, downloadedBytes, totalBytes,
    noticeVisible, busy, progress, installBlockReason, initialize, checkForUpdates, download, install, restart };
});

function errorDetail(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  return message ? `\n${message}` : "";
}
