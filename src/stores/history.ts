import { t } from '@/lib/i18n';
import { defineStore } from "pinia";
import { markRaw, ref } from "vue";

import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type {
  BackendError,
  CommitDetail,
  CommitSummary,
  CherryPickRequest,
  FileDiff,
  HistoryMutationResult,
  HistoryPage,
  HistoryQuery,
  ResetRequest,
  RevertRequest,
  SquashRequest,
} from "@/lib/backend/types";
import { createModuleLifecycle } from "@/stores/moduleLifecycle";
import { useOperationStore } from "@/stores/operation";
import { useRepositoryStore } from "@/stores/repository";
import { useTerminalStore } from "@/stores/terminal";

function emptyQuery(): HistoryQuery {
  return { reference: null, search: "", cursor: null };
}

export const useHistoryStore = defineStore("history", () => {
  const query = ref<HistoryQuery>(emptyQuery());
  const commits = ref<CommitSummary[]>([]);
  const nextCursor = ref<string | null>(null);
  const queryFingerprint = ref("");
  const selectedHash = ref<string>();
  const checkedHashes = ref<string[]>([]);
  const detail = ref<CommitDetail>();
  const selectedFilePath = ref<string>();
  const fileDiff = ref<FileDiff>();
  const fileRevealVersion = ref(0);
  const fileDiffs = ref(new Map<string, FileDiff>());
  const fileLoadingPaths = ref(new Set<string>());
  const fileErrors = ref(new Map<string, BackendError>());
  const pendingFiles = new Map<string, Promise<FileDiff>>();
  const loadedRootPath = ref<string>();
  const generation = ref(0);
  const loading = ref(false);
  const detailLoading = ref(false);
  const submitting = ref(false);
  const error = ref<BackendError>();
  const notice = ref<string>();
  const lifecycle = createModuleLifecycle();
  let loaded = false;
  let refreshEnabled = false;
  let requestVersion = 0;
  let commitSelectionVersion = 0;
  let fileSelectionVersion = 0;
  let initialPending: Promise<void> | undefined;
  let detailPending: { hash: string; request: Promise<CommitDetail> } | undefined;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  function clearFileDiffs(): void {
    fileDiffs.value.clear();
    fileLoadingPaths.value.clear();
    fileErrors.value.clear();
    pendingFiles.clear();
  }

  function resetForRepository(rootPath: string, nextGeneration: number): void {
    if (searchTimer) {
      clearTimeout(searchTimer);
      searchTimer = undefined;
    }
    requestVersion += 1;
    lifecycle.replaceRepository(rootPath, nextGeneration);
    commitSelectionVersion += 1;
    clearFileDiffs();
    loadedRootPath.value = rootPath;
    generation.value = nextGeneration;
    query.value = emptyQuery();
    commits.value = [];
    checkedHashes.value = [];
    nextCursor.value = null;
    queryFingerprint.value = "";
    selectedHash.value = undefined;
    detail.value = undefined;
    selectedFilePath.value = undefined;
    fileDiff.value = undefined;
    loading.value = false;
    detailLoading.value = false;
    detailPending = undefined;
    error.value = undefined;
    loaded = false;
    refreshEnabled = false;
    notice.value = undefined;
    initialPending = undefined;
  }

  async function fetchPage(
    cursor: string | null,
    append: boolean,
    allowCursorRestart: boolean,
    preserveTail = false,
    acceptsRefresh: () => boolean = () => true,
  ): Promise<void> {
    const rootPath = loadedRootPath.value;
    if (!rootPath) {
      return;
    }
    const token = lifecycle.begin(rootPath, generation.value);
    refreshEnabled = true;
    const version = ++requestVersion;
    loading.value = true;
    error.value = undefined;
    try {
      const page = await backendClient.historyPage(rootPath, {
        reference: query.value.reference,
        search: query.value.search,
        cursor,
      });
      if (!lifecycle.accept(token) || version !== requestVersion || !acceptsRefresh()) {
        return;
      }
      const keepTail = preserveTail && commits.value.length > page.commits.length && page.commits.length > 0 &&
        page.queryFingerprint === queryFingerprint.value && page.commits.every((commit, index) => commit.hash === commits.value[index]?.hash);
      commits.value = append ? [...commits.value, ...page.commits] : keepTail ? [...page.commits, ...commits.value.slice(page.commits.length)] : page.commits;
      checkedHashes.value = checkedHashes.value.filter(hash => commits.value.some(commit => commit.hash === hash));
      if (!keepTail) nextCursor.value = page.nextCursor;
      queryFingerprint.value = page.queryFingerprint;
      loaded = true;
    } catch (cause) {
      if (!lifecycle.accept(token) || version !== requestVersion || !acceptsRefresh()) {
        return;
      }
      const normalized = normalizeBackendError(cause);
      if (allowCursorRestart && cursor && normalized.code === "invalidHistoryCursor") {
        query.value.cursor = null;
        nextCursor.value = null;
        queryFingerprint.value = "";
        await fetchPage(null, false, false);
        return;
      }
      error.value = normalized;
      throw normalized;
    } finally {
      if (version === requestVersion) {
        loading.value = false;
      }
    }
  }

  async function ensureLoaded(rootPath: string, nextGeneration: number, acceptsRefresh: () => boolean = () => true): Promise<void> {
    if (
      loadedRootPath.value !== rootPath ||
      generation.value !== nextGeneration
    ) {
      resetForRepository(rootPath, nextGeneration);
    }
    if (!loaded) {
      if (loading.value) {
        return;
      }
      if (initialPending) {
        return initialPending;
      }
      const request = fetchPage(null, false, false, false, acceptsRefresh);
      initialPending = request;
      try {
        await request;
      } finally {
        if (initialPending === request) {
          initialPending = undefined;
        }
      }
    }
  }

  function loadNextPage(): Promise<void> {
    if (!nextCursor.value || loading.value) {
      return Promise.resolve();
    }
    return fetchPage(nextCursor.value, true, true);
  }

  async function refresh(branchChanged = false, acceptsRefresh: () => boolean = () => true): Promise<void> {
    if (!refreshEnabled || searchTimer) return;
    if (branchChanged && !query.value.reference) {
      await restartQuery(true, acceptsRefresh);
      return;
    }
    // Commit contents are immutable. Keep the selection and file diff while
    // updating the list, cursor and branch decorations in the background.
    await fetchPage(null, false, false, true, acceptsRefresh);
    const selected = commits.value.find(commit => commit.hash === selectedHash.value);
    if (selected && detail.value?.hash === selected.hash) detail.value.references = selected.references;
  }

  async function restartQuery(clearSelection = true, acceptsRefresh: () => boolean = () => true): Promise<void> {
    checkedHashes.value = [];
    requestVersion += 1;
    commitSelectionVersion += 1;
    clearFileDiffs();
    detailLoading.value = false;
    detailPending = undefined;
    query.value.cursor = null;
    commits.value = [];
    nextCursor.value = null;
    queryFingerprint.value = "";
    detail.value = undefined;
    selectedFilePath.value = undefined;
    fileDiff.value = undefined;
    if (clearSelection) {
      selectedHash.value = undefined;
    }
    loaded = false;
    await fetchPage(null, false, false, false, acceptsRefresh);
  }

  function setReference(reference: string | null): Promise<void> {
    query.value.reference = reference;
    return restartQuery();
  }

  function setSearch(search: string): void {
    checkedHashes.value = [];
    query.value.search = search;
    if (searchTimer) {
      clearTimeout(searchTimer);
    }
    searchTimer = setTimeout(() => {
      searchTimer = undefined;
      void restartQuery().catch(() => undefined);
    }, 250);
  }

  async function selectCommit(hash: string): Promise<void> {
    const rootPath = loadedRootPath.value;
    if (!rootPath) {
      return;
    }
    if (detailPending?.hash === hash) {
      await detailPending.request.catch(() => undefined);
      return;
    }
    const token = lifecycle.begin(rootPath, generation.value);
    const selection = ++commitSelectionVersion;
    clearFileDiffs();
    fileSelectionVersion += 1;
    selectedHash.value = hash;
    detail.value = undefined;
    selectedFilePath.value = undefined;
    fileDiff.value = undefined;
    detailLoading.value = true;
    error.value = undefined;
    let result: CommitDetail;
    try {
      const request = backendClient.historyDetail(rootPath, hash);
      detailPending = { hash, request };
      result = await request;
    } catch (cause) {
      const normalized = normalizeBackendError(cause);
      if (!lifecycle.accept(token) || selection !== commitSelectionVersion) {
        return;
      }
      error.value = normalized;
      throw normalized;
    } finally {
      if (lifecycle.accept(token) && selection === commitSelectionVersion) {
        detailLoading.value = false;
        detailPending = undefined;
      }
    }
    if (!lifecycle.accept(token) || selection !== commitSelectionVersion) {
      return;
    }
    selectedHash.value = hash;
    detail.value = result;
    selectedFilePath.value = undefined;
    fileDiff.value = undefined;
  }

  async function selectFile(relativePath: string, reveal = true): Promise<void> {
    const rootPath = loadedRootPath.value;
    const hash = selectedHash.value;
    if (!rootPath || !hash || detailLoading.value) {
      return;
    }
    const token = lifecycle.begin(rootPath, generation.value);
    const commitSelection = commitSelectionVersion;
    const selection = ++fileSelectionVersion;
    const acceptsCommit = () => lifecycle.accept(token)
      && commitSelection === commitSelectionVersion
      && hash === selectedHash.value;
    const acceptsSelection = () => acceptsCommit() && selection === fileSelectionVersion;
    const cached = fileDiffs.value.get(relativePath);
    if (cached) {
      selectedFilePath.value = relativePath;
      fileDiff.value = cached;
      if (reveal) fileRevealVersion.value += 1;
      return;
    }
    fileLoadingPaths.value.add(relativePath);
    if (error.value === fileErrors.value.get(relativePath)) error.value = undefined;
    fileErrors.value.delete(relativePath);
    let request = pendingFiles.get(relativePath);
    if (!request) {
      request = backendClient.historyFileDiff(rootPath, hash, relativePath);
      pendingFiles.set(relativePath, request);
    }
    let result: FileDiff;
    try {
      result = await request;
    } catch (cause) {
      const normalized = normalizeBackendError(cause);
      if (!acceptsCommit()) {
        return;
      }
      fileErrors.value.set(relativePath, normalized);
      if (acceptsSelection()) error.value = normalized;
      throw normalized;
    } finally {
      if (acceptsCommit() && pendingFiles.get(relativePath) === request) {
        pendingFiles.delete(relativePath);
        fileLoadingPaths.value.delete(relativePath);
      }
    }
    if (acceptsCommit()) fileDiffs.value.set(relativePath, markRaw(result));
    if (!acceptsSelection()) {
      return;
    }
    selectedFilePath.value = relativePath;
    fileDiff.value = result;
    if (reveal) fileRevealVersion.value += 1;
  }

  function applyPage(page: HistoryPage, rootPath: string): void {
    checkedHashes.value = [];
    if (searchTimer) {
      clearTimeout(searchTimer);
      searchTimer = undefined;
    }
    requestVersion += 1;
    commitSelectionVersion += 1;
    clearFileDiffs();
    detailLoading.value = false;
    detailPending = undefined;
    loading.value = false;
    initialPending = undefined;
    loadedRootPath.value = rootPath;
    query.value = emptyQuery();
    commits.value = page.commits;
    nextCursor.value = page.nextCursor;
    queryFingerprint.value = page.queryFingerprint;
    selectedHash.value = undefined;
    detail.value = undefined;
    selectedFilePath.value = undefined;
    fileDiff.value = undefined;
    loaded = true;
    refreshEnabled = true;
  }

  function requireRootPath(): string {
    const rootPath =
      useRepositoryStore().snapshot?.rootPath ?? loadedRootPath.value;
    if (!rootPath) {
      throw {
        code: "invalidRepository",
        get message() { return t('uiOpenAGitRepositoryFirsta00a3e'); },
      } satisfies BackendError;
    }
    return rootPath;
  }

  async function mutate(
    action: (rootPath: string) => Promise<HistoryMutationResult>,
  ): Promise<void> {
    if (submitting.value || useTerminalStore().busy) {
      throw {
        code: "gitOperationInProgress",
        get message() { return t('uiAGitOperationIsAlreadyBeingSubmittedb4d501'); },
      } satisfies BackendError;
    }
    submitting.value = true;
    error.value = undefined;
    notice.value = undefined;
    try {
      const result = await action(requireRootPath());
      useOperationStore().applyMutationResult(result);
      notice.value = result.notice ?? undefined;
      if (result.error) throw result.error;
    } catch (cause) {
      error.value = normalizeBackendError(cause);
      throw error.value;
    } finally {
      submitting.value = false;
    }
  }

  function checkout(commit: string): Promise<void> {
    return mutate((rootPath) =>
      backendClient.historyCheckout(rootPath, commit),
    );
  }

  function cherryPick(request: CherryPickRequest): Promise<void> {
    return mutate((rootPath) =>
      backendClient.historyCherryPick(rootPath, request),
    );
  }

  function reset(request: ResetRequest): Promise<void> {
    return mutate((rootPath) =>
      backendClient.historyReset(rootPath, request),
    );
  }

  function revert(request: RevertRequest): Promise<void> {
    return mutate((rootPath) => backendClient.historyRevert(rootPath, request));
  }

  function squash(request: SquashRequest): Promise<void> {
    return mutate(rootPath => backendClient.historySquash(rootPath, request));
  }

  return {
    query,
    commits,
    nextCursor,
    queryFingerprint,
    selectedHash,
    checkedHashes,
    detail,
    selectedFilePath,
    fileDiff,
    fileRevealVersion,
    fileDiffs,
    fileLoadingPaths,
    fileErrors,
    loadedRootPath,
    generation,
    loading,
    detailLoading,
    submitting,
    error,
    notice,
    resetForRepository,
    ensureLoaded,
    loadNextPage,
    refresh,
    setReference,
    setSearch,
    selectCommit,
    selectFile,
    applyPage,
    checkout,
    cherryPick,
    reset,
    revert,
    squash,
  };
});
