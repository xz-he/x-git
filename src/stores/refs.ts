import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";

import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type {
  AbortAction,
  BackendError,
  CreateBranchRequest,
  DeleteBranchRequest,
  RefsMutationResult,
  RefsSnapshot,
} from "@/lib/backend/types";
import { createModuleLifecycle } from "@/stores/moduleLifecycle";
import { useOperationStore } from "@/stores/operation";
import { useRepositoryStore } from "@/stores/repository";
import { useTerminalStore } from "@/stores/terminal";
import { useUiStore } from "@/stores/ui";

export type IntegrationAction = "merge" | "rebase";
interface IntegrationRequest {
  action: IntegrationAction;
  rootPath: string;
  generation: number;
  source: string;
  target: string;
  destination: string;
}

export const useRefsStore = defineStore("refs", () => {
  const snapshot = ref<RefsSnapshot>();
  const selectedFullName = ref<string>();
  const loadedRootPath = ref<string>();
  const generation = ref(0);
  const loading = ref(false);
  const submitting = ref(false);
  const error = ref<BackendError>();
  const integrationRequest = ref<IntegrationRequest>();
  const integrationTargets = computed(() =>
    (snapshot.value?.localBranches ?? []).filter((branch) =>
      branch.kind === "local" && (integrationRequest.value?.action === "merge" ||
        (!branch.current && branch.name !== useRepositoryStore().snapshot?.currentBranch)),
    ),
  );
  const integrationBlocked = computed(() =>
    useRepositoryStore().navigationBusy || useOperationStore().isBlocked ||
    (useRepositoryStore().snapshot?.conflictCount ?? 0) > 0,
  );
  const lifecycle = createModuleLifecycle();
  let requestVersion = 0;
  let pending: Promise<void> | undefined;

  function resetForRepository(rootPath: string, nextGeneration: number): void {
    integrationRequest.value = undefined;
    requestVersion += 1;
    lifecycle.replaceRepository(rootPath, nextGeneration);
    loadedRootPath.value = rootPath;
    generation.value = nextGeneration;
    snapshot.value = undefined;
    selectedFullName.value = undefined;
    loading.value = false;
    error.value = undefined;
    pending = undefined;
  }

  function ensureLoaded(rootPath: string, nextGeneration: number): Promise<void> {
    if (
      loadedRootPath.value !== rootPath ||
      generation.value !== nextGeneration
    ) {
      resetForRepository(rootPath, nextGeneration);
    }
    if (snapshot.value) {
      return Promise.resolve();
    }
    if (pending) {
      return pending;
    }

    const token = lifecycle.begin(rootPath, nextGeneration);
    const version = ++requestVersion;
    loading.value = true;
    error.value = undefined;
    pending = (async () => {
      try {
        const result = await backendClient.refsSnapshot(rootPath);
        if (!lifecycle.accept(token) || version !== requestVersion) {
          return;
        }
        snapshot.value = result;
        selectedFullName.value =
          result.localBranches.find((branch) => branch.current)?.fullName ??
          result.localBranches[0]?.fullName ??
          result.remoteBranches[0]?.fullName;
      } catch (cause) {
        if (!lifecycle.accept(token) || version !== requestVersion) {
          return;
        }
        error.value = normalizeBackendError(cause);
        throw error.value;
      } finally {
        if (version === requestVersion) {
          loading.value = false;
          pending = undefined;
        }
      }
    })();
    return pending;
  }

  function applySnapshot(next: RefsSnapshot, rootPath: string): void {
    requestVersion += 1;
    loading.value = false;
    pending = undefined;
    const previousSelection = selectedFullName.value;
    snapshot.value = next;
    loadedRootPath.value = rootPath;
    selectedFullName.value =
      next.localBranches.find(
        (branch) => branch.fullName === previousSelection,
      )?.fullName ??
      next.remoteBranches.find(
        (branch) => branch.fullName === previousSelection,
      )?.fullName ??
      next.localBranches.find((branch) => branch.current)?.fullName ??
      next.localBranches[0]?.fullName ??
      next.remoteBranches[0]?.fullName;
  }

  function requireRootPath(): string {
    const rootPath =
      useRepositoryStore().snapshot?.rootPath ?? loadedRootPath.value;
    if (!rootPath) {
      throw {
        code: "invalidRepository",
        message: "请先打开 Git 仓库。",
      } satisfies BackendError;
    }
    return rootPath;
  }

  async function mutate(
    action: (rootPath: string) => Promise<RefsMutationResult>,
  ): Promise<void> {
    if (submitting.value || useTerminalStore().busy) {
      throw {
        code: "gitOperationInProgress",
        message: "已有 Git 操作正在提交。",
      } satisfies BackendError;
    }
    submitting.value = true;
    error.value = undefined;
    try {
      useOperationStore().applyMutationResult(
        await action(requireRootPath()),
      );
    } catch (cause) {
      error.value = normalizeBackendError(cause);
      throw error.value;
    } finally {
      submitting.value = false;
    }
  }

  function createBranch(request: CreateBranchRequest): Promise<void> {
    return mutate((rootPath) => backendClient.refsCreate(rootPath, request));
  }

  function switchBranch(name: string): Promise<void> {
    return mutate((rootPath) => backendClient.refsSwitch(rootPath, name));
  }

  function deleteBranch(request: DeleteBranchRequest): Promise<void> {
    return mutate((rootPath) => backendClient.refsDelete(rootPath, request));
  }

  async function merge(target: string, destination?: string): Promise<void> {
    if (integrationBlocked.value) return Promise.reject(integrationBusyError());
    const repositories = useRepositoryStore();
    const root = repositories.snapshot?.rootPath;
    const owner = repositories.generation;
    const previousBranch = repositories.snapshot?.currentBranch;
    try {
      await mutate((rootPath) => destination === undefined
        ? backendClient.refsMerge(rootPath, target)
        : backendClient.refsMerge(rootPath, target, destination));
    } catch (cause) {
      // Switching may succeed even if the merge fails. Refresh after mutate
      // releases its busy flag so the UI cannot keep showing the old branch.
      if (destination && destination !== previousBranch && repositories.snapshot?.rootPath === root && repositories.generation === owner) {
        await repositories.refresh().catch(() => undefined);
      }
      throw cause;
    }
  }

  function rebase(target: string): Promise<void> {
    if (integrationBlocked.value) return Promise.reject(integrationBusyError());
    return mutate((rootPath) => backendClient.refsRebase(rootPath, target));
  }

  function integrationBusyError(): BackendError {
    return { code: "gitOperationInProgress", message: "请先完成当前 Git 操作或解决冲突。" };
  }

  function integrationIsCurrent(request: IntegrationRequest): boolean {
    const repositories = useRepositoryStore();
    const ui = useUiStore();
    return repositories.snapshot?.rootPath === request.rootPath &&
      repositories.generation === request.generation &&
      repositories.snapshot.currentBranch === request.source &&
      loadedRootPath.value === request.rootPath && generation.value === request.generation &&
      !ui.homeVisible && ui.activeView === "branches";
  }

  function requestIntegration(action: IntegrationAction, target = selectedFullName.value): void {
    const repositories = useRepositoryStore();
    const repository = repositories.snapshot;
    if (!repository?.currentBranch || integrationBlocked.value || useUiStore().homeVisible) return;
    useUiStore().openView("branches");
    if (loadedRootPath.value !== repository.rootPath || generation.value !== repositories.generation) {
      resetForRepository(repository.rootPath, repositories.generation);
      target = undefined;
    }
    error.value = undefined;
    integrationRequest.value = {
      action, rootPath: repository.rootPath, generation: repositories.generation,
      source: repository.currentBranch,
      destination: repository.currentBranch,
      target: (action === "merge" ? snapshot.value?.localBranches ?? [] : integrationTargets.value)
        .find((branch) => branch.fullName === target)?.fullName ?? "",
    };
    void loadIntegrationTargets();
  }

  async function loadIntegrationTargets(): Promise<void> {
    const request = integrationRequest.value;
    if (!request || !integrationIsCurrent(request) || integrationBlocked.value) return;
    await ensureLoaded(request.rootPath, request.generation).catch(() => undefined);
  }

  function cancelIntegration(): void {
    if (!submitting.value) integrationRequest.value = undefined;
  }

  async function confirmIntegration(): Promise<void> {
    const request = integrationRequest.value;
    if (!request || integrationBlocked.value || loading.value) return;
    if (!integrationIsCurrent(request)) {
      integrationRequest.value = undefined;
      return;
    }
    const target = integrationTargets.value.find((branch) => branch.fullName === request.target || branch.name === request.target);
    if (!target) return;
    if (request.action === "merge" && (!snapshot.value?.localBranches.some(branch => branch.name === request.destination)
      || target.name === request.destination)) return;
    try {
      await (request.action === "merge" ? merge(target.name, request.destination) : rebase(target.name));
      if (integrationRequest.value === request) integrationRequest.value = undefined;
    } catch {
      // Preserve the selected target and backend error for retry.
    }
  }

  watch(
    () => [useRepositoryStore().generation, useRepositoryStore().snapshot?.rootPath,
      useRepositoryStore().snapshot?.currentBranch, useUiStore().activeView, useUiStore().homeVisible],
    () => {
      if (integrationRequest.value && !integrationIsCurrent(integrationRequest.value)) {
        integrationRequest.value = undefined;
      }
    },
    { flush: "sync" },
  );

  function abort(action: AbortAction): Promise<void> {
    return mutate((rootPath) => backendClient.refsAbort(rootPath, action));
  }

  return {
    snapshot,
    selectedFullName,
    loadedRootPath,
    generation,
    loading,
    submitting,
    error,
    integrationRequest,
    integrationTargets,
    integrationBlocked,
    requestIntegration,
    loadIntegrationTargets,
    cancelIntegration,
    confirmIntegration,
    resetForRepository,
    ensureLoaded,
    applySnapshot,
    createBranch,
    switchBranch,
    deleteBranch,
    merge,
    rebase,
    abort,
  };
});
