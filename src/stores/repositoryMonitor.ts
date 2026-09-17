import { ref, watch, type WatchStopHandle } from "vue";
import { defineStore } from "pinia";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type { BackendError } from "@/lib/backend/types";
import { useRepositoryStore } from "./repository";

const POLL_MS = 1_000;
const RETRY_MS = 5_000;
type WatchSnapshot = Awaited<ReturnType<typeof backendClient.repositoryWatchSnapshot>>;

export const useRepositoryMonitorStore = defineStore("repository-monitor", () => {
  const repositories = useRepositoryStore();
  const error = ref<BackendError>();
  let stopWatch: WatchStopHandle | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = true;
  let running = false;
  let epoch = 0;
  let applied: WatchSnapshot | undefined;

  function schedule(delay = POLL_MS) {
    clearTimeout(timer);
    if (!disposed && repositories.snapshot) timer = setTimeout(() => { timer = undefined; void poll(); }, delay);
  }
  async function poll() {
    if (disposed || running || !repositories.snapshot) return;
    const root = repositories.snapshot.rootPath, generation = repositories.generation, owner = epoch;
    const accepts = () => !disposed && epoch === owner && repositories.snapshot?.rootPath === root && repositories.generation === generation;
    let delay = POLL_MS;
    running = true;
    try {
      const next = await backendClient.repositoryWatchSnapshot(root);
      if (!accepts() || repositories.navigationBusy) return;
      // Backend versions advance only for a different Git/content fingerprint.
      // A new watcher starts at zero; establishing its baseline is not a change.
      const previous = applied?.watchId === next.watchId ? applied : undefined;
      const full = next.metadataVersion !== (previous?.metadataVersion ?? 0);
      if (full || next.worktreeVersion !== (previous?.worktreeVersion ?? 0)) {
        await repositories.refresh({ metadata: full, background: true });
        if (!accepts()) return;
      }
      applied = next;
      error.value = undefined;
    } catch (cause) {
      if (!accepts()) return;
      error.value = normalizeBackendError(cause);
      delay = RETRY_MS;
      // Retry detection without treating an error as evidence of a Git change.
    } finally {
      running = false;
      if (!timer) schedule(delay);
    }
  }
  function requestRefresh() { schedule(150); }
  function onVisibility() { if (document.visibilityState === "visible") requestRefresh(); }
  function initialize() {
    if (!disposed) return;
    disposed = false;
    stopWatch = watch([() => repositories.snapshot?.rootPath, () => repositories.generation], () => {
      epoch++; applied = undefined; error.value = undefined;
      schedule();
    }, { immediate: true });
    window.addEventListener("focus", requestRefresh);
    document.addEventListener("visibilitychange", onVisibility);
  }
  function dispose() {
    disposed = true; epoch++; clearTimeout(timer); timer = undefined;
    stopWatch?.(); stopWatch = undefined;
    window.removeEventListener("focus", requestRefresh);
    document.removeEventListener("visibilitychange", onVisibility);
    void backendClient.repositoryWatchStop().catch(() => undefined);
  }
  return { error, initialize, dispose, requestRefresh };
});
