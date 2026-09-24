import { t } from '@/lib/i18n';
import { computed, ref, watch } from "vue";
import { defineStore } from "pinia";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type { BackendError, FileDiff, StashDetail, StashEntry, StashMutationResult, StashSelection, StashSnapshot } from "@/lib/backend/types";
import { createModuleLifecycle } from "@/stores/moduleLifecycle";
import { useOperationStore } from "@/stores/operation";
import { useRepositoryStore } from "@/stores/repository";
import { useChangesStore } from "@/stores/changes";

function selection(entry: StashEntry): StashSelection {
  return { selector: entry.selector, expectedObjectId: entry.objectId };
}

export const useStashesStore = defineStore("stashes", () => {
  const snapshot = ref<StashSnapshot>();
  const selectedEntry = ref<StashEntry>();
  const detail = ref<StashDetail>();
  const selectedFilePath = ref<string>();
  const selectedFileUntracked = ref(false);
  const fileDiff = ref<FileDiff>();
  const loadedRootPath = ref<string>();
  const generation = ref(0);
  const message = ref("");
  const includeUntracked = ref(false);
  const selectFiles = ref(false);
  const selectedPaths = ref<string[]>([]);
  const availableFiles = computed(() => (useChangesStore().snapshot?.files ?? [])
    .filter(file => !file.conflict && (includeUntracked.value || file.indexStatus !== "?")));
  watch(availableFiles, files => {
    const available = new Set(files.map(file => file.path));
    selectedPaths.value = selectedPaths.value.filter(path => available.has(path));
  }, { flush: "sync" });
  const loading = ref(false);
  const detailLoading = ref(false);
  const diffLoading = ref(false);
  const submitting = ref(false);
  const error = ref<BackendError>();
  const detailError = ref<BackendError>();
  const diffError = ref<BackendError>();
  const outcome = ref<StashMutationResult["outcome"]>();
  const lifecycle = createModuleLifecycle();
  let listVersion = 0;
  let detailVersion = 0;
  let diffVersion = 0;
  let mutationVersion = 0;
  let pending: Promise<void> | undefined;
  const canMutate = computed(() => !submitting.value && !useOperationStore().isBlocked && !useRepositoryStore().navigationBusy);

  function clearDetail(): void {
    detailVersion += 1;
    diffVersion += 1;
    detail.value = undefined;
    fileDiff.value = undefined;
    selectedFilePath.value = undefined;
    selectedFileUntracked.value = false;
    detailLoading.value = false;
    diffLoading.value = false;
    detailError.value = undefined;
    diffError.value = undefined;
  }

  function resetForRepository(rootPath: string, nextGeneration: number): void {
    lifecycle.replaceRepository(rootPath, nextGeneration);
    loadedRootPath.value = rootPath;
    generation.value = nextGeneration;
    listVersion += 1;
    mutationVersion += 1;
    pending = undefined;
    snapshot.value = undefined;
    selectedEntry.value = undefined;
    message.value = "";
    includeUntracked.value = false;
    selectFiles.value = false;
    selectedPaths.value = [];
    loading.value = false;
    submitting.value = false;
    error.value = undefined;
    outcome.value = undefined;
    clearDetail();
  }

  function applySnapshot(next: StashSnapshot, rootPath: string): void {
    listVersion += 1;
    pending = undefined;
    loading.value = false;
    loadedRootPath.value = rootPath;
    snapshot.value = next;
    const previous = selectedEntry.value;
    const previousDetail = detail.value;
    const previousDiff = fileDiff.value;
    const previousFile = selectedFilePath.value;
    const previousUntracked = selectedFileUntracked.value;
    selectedEntry.value = next.entries.find((entry) => entry.objectId === previous?.objectId && entry.selector === previous.selector)
      ?? next.entries.find((entry) => entry.objectId === previous?.objectId);
    clearDetail();
    if (selectedEntry.value && previousDetail?.entry.objectId === selectedEntry.value.objectId) {
      detail.value = { ...previousDetail, entry: selectedEntry.value };
      fileDiff.value = previousDiff;
      selectedFilePath.value = previousFile;
      selectedFileUntracked.value = previousUntracked;
    }
  }

  function ensureLoaded(rootPath: string, nextGeneration: number, acceptsRefresh: () => boolean = () => true): Promise<void> {
    if (loadedRootPath.value !== rootPath || generation.value !== nextGeneration) resetForRepository(rootPath, nextGeneration);
    if (snapshot.value) return Promise.resolve();
    if (pending) return pending;
    const token = lifecycle.begin(rootPath, nextGeneration);
    const version = ++listVersion;
    loading.value = true;
    error.value = undefined;
    pending = (async () => {
      try {
        const next = await backendClient.stashSnapshot(rootPath);
        if (!lifecycle.accept(token) || version !== listVersion || !acceptsRefresh()) return;
        snapshot.value = next;
      } catch (cause) {
        if (!lifecycle.accept(token) || version !== listVersion || !acceptsRefresh()) return;
        error.value = normalizeBackendError(cause);
        throw error.value;
      } finally {
        if (version === listVersion) { loading.value = false; pending = undefined; }
      }
    })();
    return pending;
  }

  async function refresh(): Promise<void> {
    const rootPath = loadedRootPath.value;
    if (!rootPath || submitting.value) return;
    const token = lifecycle.begin(rootPath, generation.value);
    const previousId = selectedEntry.value?.objectId;
    listVersion += 1;
    pending = undefined;
    snapshot.value = undefined;
    clearDetail();
    await ensureLoaded(rootPath, generation.value);
    if (!lifecycle.accept(token)) return;
    const next = snapshot.value as StashSnapshot | undefined;
    const previous = next?.entries.find((entry) => entry.objectId === previousId);
    selectedEntry.value = undefined;
    if (previous) await selectEntry(previous);
  }

  async function selectEntry(entry: StashEntry): Promise<void> {
    const rootPath = loadedRootPath.value;
    if (!rootPath || submitting.value) return;
    clearDetail();
    selectedEntry.value = entry;
    const token = lifecycle.begin(rootPath, generation.value);
    const version = ++detailVersion;
    detailLoading.value = true;
    try {
      const next = await backendClient.stashDetail(rootPath, selection(entry));
      if (!lifecycle.accept(token) || version !== detailVersion) return;
      detail.value = next;
    } catch (cause) {
      if (!lifecycle.accept(token) || version !== detailVersion) return;
      detailError.value = normalizeBackendError(cause);
      throw detailError.value;
    } finally {
      if (version === detailVersion) detailLoading.value = false;
    }
  }

  async function selectFile(relativePath: string, untracked = false): Promise<void> {
    const rootPath = loadedRootPath.value;
    const entry = selectedEntry.value;
    if (!rootPath || !entry || submitting.value) return;
    const token = lifecycle.begin(rootPath, generation.value);
    const version = ++diffVersion;
    selectedFilePath.value = relativePath;
    selectedFileUntracked.value = untracked;
    fileDiff.value = undefined;
    diffError.value = undefined;
    diffLoading.value = true;
    try {
      const next = await backendClient.stashFileDiff(rootPath, selection(entry), relativePath, untracked);
      if (!lifecycle.accept(token) || version !== diffVersion) return;
      fileDiff.value = next;
    } catch (cause) {
      if (!lifecycle.accept(token) || version !== diffVersion) return;
      diffError.value = normalizeBackendError(cause);
      throw diffError.value;
    } finally {
      if (version === diffVersion) diffLoading.value = false;
    }
  }

  async function mutate(action: (rootPath: string) => Promise<StashMutationResult>): Promise<void> {
    if (!canMutate.value) throw { code: "gitOperationInProgress", get message() { return t('uiFinishTheCurrentGitOperationOrResolveConflictsFirst020dd7'); } } satisfies BackendError;
    const repositories = useRepositoryStore();
    const rootPath = repositories.snapshot?.rootPath ?? loadedRootPath.value;
    if (!rootPath) throw { code: "invalidRepository", get message() { return t('uiOpenAGitRepositoryFirsta00a3e'); } } satisfies BackendError;
    if (loadedRootPath.value !== rootPath || generation.value !== repositories.generation) {
      throw { code: "invalidRepository", get message() { return t('uiRepositoryChangedRefreshTheStashListAndTryAgain18eac8'); } } satisfies BackendError;
    }
    const repositoryGeneration = repositories.generation;
    const version = ++mutationVersion;
    const accepts = () => version === mutationVersion && repositories.generation === repositoryGeneration && (!repositories.snapshot || repositories.snapshot.rootPath === rootPath);
    submitting.value = true;
    error.value = undefined;
    outcome.value = undefined;
    try {
      const next = await action(rootPath);
      if (!accepts()) return;
      useOperationStore().applyMutationResult(next);
      outcome.value = next.outcome;
      if (next.error) throw next.error;
      if (next.outcome === "created") { message.value = ""; includeUntracked.value = false; selectedPaths.value = []; }
    } catch (cause) {
      if (!accepts()) return;
      error.value = normalizeBackendError(cause);
      throw error.value;
    } finally {
      if (version === mutationVersion) submitting.value = false;
    }
  }

  function create(): Promise<void> {
    const paths = selectedPaths.value.filter(path => availableFiles.value.some(file => file.path === path));
    if (selectFiles.value && !paths.length) {
      error.value = { code: "invalidPath", get message() { return t('uiSelectAtLeastOneFileToStashc7e904'); } };
      return Promise.reject(error.value);
    }
    const request = { message: message.value, includeUntracked: includeUntracked.value,
      ...(selectFiles.value ? { paths } : {}) };
    return mutate((path) => backendClient.stashCreate(path, request));
  }
  function apply(entry: StashEntry): Promise<void> {
    return mutate((path) => backendClient.stashApply(path, selection(entry)));
  }
  function pop(entry: StashEntry): Promise<void> {
    return mutate((path) => backendClient.stashPop(path, selection(entry)));
  }

  return { snapshot, selectedEntry, detail, selectedFilePath, selectedFileUntracked, fileDiff, loadedRootPath, generation, message, includeUntracked, selectFiles, selectedPaths, availableFiles, loading, detailLoading, diffLoading, submitting, canMutate, error, detailError, diffError, outcome, resetForRepository, ensureLoaded, refresh, applySnapshot, selectEntry, selectFile, create, apply, pop };
});
