import { computed, ref, watch } from "vue";
import { defineStore } from "pinia";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type { BackendError, CreateTaskBranchRequest, TaskBranchBinding, TaskBranchResult, TaskBranchRunRequest } from "@/lib/backend/types";
import { useRepositoryStore } from "./repository";
import { useChangesStore } from "./changes";
import { useOperationStore } from "./operation";
import { useUiStore } from "./ui";

export const useTaskBranchesStore = defineStore("task-branches", () => {
  const repositories = useRepositoryStore();
  const bindings = ref<TaskBranchBinding[]>([]);
  const loadedRootPath = ref<string>();
  const loading = ref(false);
  const submitting = ref(false);
  const error = ref<BackendError>();
  const uncertain = ref(false);
  let version = 0;
  const relevantBindings = computed(() => bindings.value.filter((item) =>
    loadedRootPath.value === repositories.snapshot?.rootPath &&
    (item.sourceBranch === repositories.snapshot?.currentBranch || item.targetBranch === repositories.snapshot?.currentBranch),
  ));
  const blocked = computed<boolean>(() => submitting.value || loading.value || uncertain.value || repositories.navigationBusy ||
    useOperationStore().isBlocked || !!repositories.snapshot?.conflictCount);

  function owner() {
    const snapshot = repositories.snapshot;
    return snapshot ? { root: snapshot.rootPath, generation: repositories.generation } : undefined;
  }
  function accepts(id: { root: string; generation: number }) {
    return repositories.snapshot?.rootPath === id.root && repositories.generation === id.generation;
  }
  async function refresh(): Promise<void> {
    const id = owner();
    if (!id || submitting.value) return;
    const request = ++version;
    loading.value = true;
    try {
      const next = await backendClient.taskBranchesSnapshot(id.root);
      if (!accepts(id) || request !== version) return;
      bindings.value = next;
      loadedRootPath.value = id.root;
      uncertain.value = false;
      error.value = undefined;
    } catch (cause) {
      if (accepts(id) && request === version) {
        error.value = normalizeBackendError(cause);
        uncertain.value = true;
      }
    } finally {
      if (request === version) loading.value = false;
    }
  }

  async function refreshState(): Promise<void> {
    const id = owner();
    if (!id || submitting.value || loading.value || repositories.navigationBusy) return;
    loading.value = true;
    try {
      // A lost mutation response can leave HEAD and branch stale as well as the task record.
      await repositories.refresh();
      if (accepts(id)) await refresh();
    } catch (cause) {
      if (accepts(id)) {
        error.value = normalizeBackendError(cause);
        uncertain.value = true;
      }
    } finally {
      if (accepts(id)) loading.value = false;
    }
  }

  async function mutate(action: (root: string) => Promise<TaskBranchResult>, commit?: { id: string; message: string }): Promise<TaskBranchResult | undefined> {
    const id = owner();
    if (!id || submitting.value) return;
    ++version;
    loading.value = false;
    submitting.value = true;
    error.value = undefined;
    const previousCommit = bindings.value.find((item) => item.id === commit?.id)?.sourceCommit;
    try {
      const result = await action(id.root);
      if (!accepts(id)) return;
      bindings.value = result.bindings;
      loadedRootPath.value = id.root;
      error.value = result.error ?? undefined;
      uncertain.value = !result.workspace && !!result.error;
      if (result.workspace) useOperationStore().applyMutationResult(result.workspace);
      const consumed = result.bindings.find((item) => item.id === commit?.id);
      if (commit && consumed?.sourceCommit && consumed.sourceCommit !== previousCommit && useChangesStore().commitMessage === commit.message) {
        useChangesStore().commitMessage = "";
      }
      if (result.bindings.some((item) => item.phase === "conflict" && item.targetBranch === repositories.snapshot?.currentBranch)) {
        useUiStore().openView("conflicts");
      }
      return result;
    } catch (cause) {
      if (accepts(id)) {
        error.value = normalizeBackendError(cause);
        uncertain.value = true;
      }
    } finally {
      submitting.value = false;
    }
  }
  function create(request: CreateTaskBranchRequest) {
    if (blocked.value || repositories.snapshot?.currentBranch !== request.sourceBranch || repositories.snapshot.headShortHash !== request.expectedHead) return Promise.resolve(undefined);
    return mutate((root) => backendClient.taskBranchesCreate(root, request));
  }
  async function unlink(bindingId: string): Promise<void> {
    const id = owner();
    if (!id || submitting.value || loading.value || repositories.navigationBusy ||
      loadedRootPath.value !== id.root || !bindings.value.some((item) => item.id === bindingId)) return;
    ++version;
    submitting.value = true;
    error.value = undefined;
    try {
      const next = await backendClient.taskBranchesUnlink(id.root, bindingId);
      if (!accepts(id)) return;
      bindings.value = next;
      loadedRootPath.value = id.root;
      uncertain.value = false;
    } catch (cause) {
      if (accepts(id)) {
        error.value = normalizeBackendError(cause);
        uncertain.value = true;
      }
    } finally {
      submitting.value = false;
      if (!accepts(id)) void refresh();
    }
  }
  function run(request: TaskBranchRunRequest) {
    if (submitting.value || repositories.navigationBusy || loading.value) return Promise.resolve(undefined);
    if (request.action !== "reconcile" && blocked.value) return Promise.resolve(undefined);
    if (repositories.snapshot?.headShortHash !== request.expectedHead) return Promise.resolve(undefined);
    return mutate((root) => backendClient.taskBranchesRun(root, request), request.action === "commit" ? { id: request.id, message: request.message ?? "" } : undefined);
  }

  watch([() => repositories.snapshot?.rootPath, () => repositories.generation], () => {
    ++version;
    bindings.value = [];
    loadedRootPath.value = undefined;
    error.value = undefined;
    uncertain.value = false;
    loading.value = false;
    void refresh();
  }, { immediate: true });
  watch([() => repositories.snapshot?.headShortHash, () => repositories.snapshot?.currentBranch, () => useOperationStore().state.kind], () => {
    if (!submitting.value && !loading.value) void refresh();
  });
  return { bindings, relevantBindings, loadedRootPath, loading, submitting, error, uncertain, blocked, refresh, refreshState, create, run, unlink };
});
