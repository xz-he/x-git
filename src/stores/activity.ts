import { t } from '@/lib/i18n';
import { defineStore } from "pinia";
import { ref } from "vue";
import { backendClient } from "@/lib/backend/client";
import type { ActivityEntry } from "@/lib/backend/activity";
import { normalizeBackendError } from "@/lib/backend/errors";
import { useRepositoryStore } from "./repository";
import { useOperationStore } from "./operation";

export const useActivityStore = defineStore("activity", () => {
  const entries = ref<ActivityEntry[]>([]);
  const loading = ref(false), submitting = ref(false), error = ref("");
  let loadVersion = 0;
  let loadedRoot: string | undefined;
  async function load(): Promise<void> {
    const repositories = useRepositoryStore();
    const root = repositories.snapshot?.rootPath, generation = repositories.generation;
    const request = ++loadVersion;
    if (root !== loadedRoot) entries.value = [];
    loadedRoot = root;
    error.value = "";
    if (!root) { loading.value = false; return; }
    loading.value = true;
    const owns = () => request === loadVersion && root === repositories.snapshot?.rootPath && generation === repositories.generation;
    try { const result = await backendClient.activityList(root); if (owns()) entries.value = result; }
    catch (cause) { if (owns()) error.value = normalizeBackendError(cause).message; }
    finally { if (request === loadVersion) loading.value = false; }
  }
  async function rollback(entry: ActivityEntry): Promise<boolean> {
    const repositories = useRepositoryStore(), root = repositories.snapshot?.rootPath, generation = repositories.generation;
    if (!root || submitting.value || repositories.navigationBusy || !entry.rollbackKind || entry.rollbackId) return false;
    submitting.value = true; error.value = "";
    let failure = "";
    try {
      const result = await backendClient.activityRollback(root, entry.id);
      if (root === repositories.snapshot?.rootPath && generation === repositories.generation) {
        useOperationStore().applyMutationResult(result);
        // Invalidate cached commits/refs/files, including hidden views.
        try { await repositories.refreshAfterTerminal(root, generation); }
        catch (cause) { failure = t('msgRollbackCompletedButTheInterfaceCouldNotRefresh06c423', { p0: normalizeBackendError(cause).message }); }
      }
      return true;
    } catch (cause) { failure = normalizeBackendError(cause).message; return false; }
    finally {
      await load();
      if (failure && root === repositories.snapshot?.rootPath && generation === repositories.generation) error.value = failure;
      submitting.value = false;
    }
  }
  return { entries, loading, submitting, error, load, rollback };
});
