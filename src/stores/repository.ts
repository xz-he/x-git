import { t } from '@/lib/i18n';
import { defineStore } from "pinia";
import { computed, ref } from "vue";

import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type { BackendError, RepositorySnapshot } from "@/lib/backend/types";
import { useChangesStore } from "@/stores/changes";
import { useAiStore } from "@/stores/ai";
import { useAiChatStore } from "@/stores/aiChat";
import { useHistoryStore } from "@/stores/history";
import { useOperationStore } from "@/stores/operation";
import { useRefsStore } from "@/stores/refs";
import { useRemotesStore } from "@/stores/remotes";
import { useStashesStore } from "@/stores/stashes";
import { useSettingsStore } from "@/stores/settings";
import { useUiStore } from "@/stores/ui";
import { useConflictsStore } from "@/stores/conflicts";
import { useFilesStore } from "@/stores/files";
import { useConsoleStore } from "@/stores/console";
import { useTaskBranchesStore } from "@/stores/taskBranches";
import { useTerminalStore } from "@/stores/terminal";
import { useActivityStore } from "@/stores/activity";
import { useUpdatesStore } from "@/stores/updates";

export type RepositoryOperation =
  | { kind: "idle" }
  | { kind: "open" | "init" | "clone" | "refresh" };

export const useRepositoryStore = defineStore("repository", () => {
  const snapshot = ref<RepositorySnapshot>();
  const generation = ref(0);
  const operation = ref<RepositoryOperation>({ kind: "idle" });
  const error = ref<BackendError>();
  const refreshingModules = ref(false);
  let refreshVersion = 0;
  const otherOperationBusy = computed<boolean>((): boolean =>
    useUpdatesStore().phase === "installing" ||
    useActivityStore().submitting ||
    operation.value.kind !== "idle" ||
    useChangesStore().operation.kind !== "idle" ||
    useRefsStore().submitting || useHistoryStore().submitting ||
    useStashesStore().submitting || useRemotesStore().running || useConflictsStore().submitting || useFilesStore().submitting || useTaskBranchesStore().submitting,
  );
  const navigationBusy = computed<boolean>(() => otherOperationBusy.value || useTerminalStore().busy);
  // Browsing cached views never submits a Git write. Keep it available during
  // commits, synchronization and refreshes; write controls use navigationBusy.
  const viewNavigationBusy = computed(() => useUpdatesStore().phase === "installing" ||
    useTerminalStore().resetting || (operation.value.kind !== "idle" && operation.value.kind !== "refresh"));

  async function execute(
    kind: Exclude<RepositoryOperation["kind"], "idle">,
    action: () => Promise<RepositorySnapshot>,
    recordRecent: boolean,
  ): Promise<void> {
    const ownerRoot = snapshot.value?.rootPath, ownerGeneration = generation.value;
    const acceptsRefresh = () => kind !== "refresh" || (snapshot.value?.rootPath === ownerRoot && generation.value === ownerGeneration);
    operation.value = { kind };
    if (kind !== "refresh") useConsoleStore().preserveForOpen(generation.value);
    error.value = undefined;
    try {
      if (kind !== "refresh") {
        refreshVersion += 1;
        refreshingModules.value = false;
        await useRemotesStore().cancelAndAbandon();
      }
      const nextSnapshot = await action();
      if (!acceptsRefresh()) return;
      if (kind !== "refresh") {
        await useAiStore().cancelAndAbandon();
        useAiChatStore().resetForRepository();
        await useConsoleStore().resetForRepository();
        await useTerminalStore().resetForRepository();
        useConflictsStore().resetForRepository(nextSnapshot.rootPath, generation.value);
        useFilesStore().resetForRepository(nextSnapshot.rootPath, generation.value);
        useStashesStore().resetForRepository(nextSnapshot.rootPath, generation.value);
        useOperationStore().reset();
        useRefsStore().resetForRepository(
          nextSnapshot.rootPath,
          generation.value,
        );
        useHistoryStore().resetForRepository(
          nextSnapshot.rootPath,
          generation.value,
        );
        useRemotesStore().resetForRepository(
          nextSnapshot.rootPath,
          generation.value,
        );
        const changes = useChangesStore();
        changes.applyWorkspace({
          repository: nextSnapshot,
          changes: { files: [], stagedCount: 0, unstagedCount: 0 },
        });
        changes.selectedPath = undefined;
        useUiStore().homeVisible = false;
      }
      snapshot.value = nextSnapshot;
      await useChangesStore().load(nextSnapshot.rootPath, kind === "refresh");
      if (!acceptsRefresh()) return;
      const conflicts = useConflictsStore();
      if (kind === "refresh" && conflicts.loadedRootPath === nextSnapshot.rootPath) await conflicts.refresh();
      else await conflicts.ensureLoaded(nextSnapshot.rootPath, generation.value);
      if (kind === "refresh" && useUiStore().activeView === "conflicts") await conflicts.refreshCleanDetails();
      if (recordRecent) {
        await useSettingsStore().recordRecentRepository(nextSnapshot.rootPath);
      }
    } catch (cause) {
      // A failed open must not invalidate drafts belonging to the retained repository.
      if (useConflictsStore().loadedRootPath === snapshot.value?.rootPath) useConflictsStore().generation = generation.value;
      error.value = normalizeBackendError(cause);
      throw error.value;
    } finally {
      operation.value = { kind: "idle" };
      if (kind !== "refresh" && snapshot.value && useUiStore().activeView === "files" && !useUiStore().homeVisible) {
        await useFilesStore().ensureLoaded(snapshot.value.rootPath, generation.value);
      }
    }
  }

  async function open(path: string): Promise<void> {
    if (!allowReplacement()) return;
    generation.value += 1;
    return execute("open", () => backendClient.repositoryOpen(path), true);
  }

  async function init(path: string): Promise<void> {
    if (!allowReplacement()) return;
    generation.value += 1;
    return execute("init", () => backendClient.repositoryInit(path), true);
  }

  async function clone(url: string, path: string): Promise<void> {
    if (!allowReplacement()) return;
    generation.value += 1;
    return execute(
      "clone",
      () => backendClient.repositoryClone(url, path),
      true,
    );
  }

  async function refresh(options: { metadata?: boolean; background?: boolean } = {}): Promise<void> {
    if (!snapshot.value || navigationBusy.value || refreshingModules.value) {
      return Promise.resolve();
    }
    const previous = snapshot.value;
    const root = previous.rootPath, ownerGeneration = generation.value;
    refreshingModules.value = true;
    const version = ++refreshVersion;
    try {
      await execute(
        "refresh",
        () => options.background ? backendClient.repositoryRefresh(root, true) : backendClient.repositoryRefresh(root),
        false,
      );
      if (snapshot.value?.rootPath !== root || generation.value !== ownerGeneration) return;
      const workspace = snapshot.value;
      const valid = () => version === refreshVersion && snapshot.value === workspace && generation.value === ownerGeneration;
      const branchChanged = previous.currentBranch !== snapshot.value.currentBranch;
      const metadata = options.metadata !== false || branchChanged || previous.headShortHash !== snapshot.value.headShortHash;
      if (metadata) {
        const refs = useRefsStore(), remotes = useRemotesStore(), stashes = useStashesStore();
        const results = await Promise.allSettled([
          useHistoryStore().refresh(branchChanged, valid),
          refs.snapshot && refs.loadedRootPath === root ? backendClient.refsSnapshot(root).then(next => { if (valid()) refs.applySnapshot(next, root); }) : Promise.resolve(),
          remotes.snapshot && remotes.loadedRootPath === root ? backendClient.remotesSnapshot(root).then(next => { if (valid()) remotes.applySnapshot(next); }) : Promise.resolve(),
          stashes.snapshot && stashes.loadedRootPath === root ? stashes.refresh() : Promise.resolve(),
        ]);
        const failure = results.find(result => result.status === "rejected");
        if (valid() && failure?.status === "rejected") throw failure.reason;
      }
      if (!valid()) return;
      const files = useFilesStore();
      if (useUiStore().activeView === "files" && !useUiStore().homeVisible && files.loadedRootPath === snapshot.value?.rootPath) await files.refresh();
      else await loadVisibleModule(root, ownerGeneration, valid);
    } catch (cause) {
      if (snapshot.value?.rootPath === root && generation.value === ownerGeneration) error.value = normalizeBackendError(cause);
      throw cause;
    } finally { if (version === refreshVersion) refreshingModules.value = false; }
  }

  function allowReplacement(): boolean {
    if (useActivityStore().submitting) return false;
    if (useTerminalStore().busy) return false;
    if (useChangesStore().operation.kind !== "idle" || useRefsStore().submitting || useHistoryStore().submitting || useStashesStore().submitting) return false;
    const conflicts = useConflictsStore();
    if (conflicts.submitting || useFilesStore().submitting || useTaskBranchesStore().submitting || operation.value.kind !== "idle") return false;
    return !conflicts.hasDirtyDrafts || window.confirm(t('uiSwitchingRepositoriesWillDiscardAllUnsavedResolutionDraftsCoc6023f'));
  }

  function loadVisibleModule(root: string, ownerGeneration: number, valid: () => boolean): Promise<void> {
    if (!valid() || useUiStore().homeVisible) return Promise.resolve();
    switch (useUiStore().activeView) {
      case "branches": case "tags": return useRefsStore().ensureLoaded(root, ownerGeneration, valid);
      case "history": return useHistoryStore().ensureLoaded(root, ownerGeneration, valid);
      case "stashes": return useStashesStore().ensureLoaded(root, ownerGeneration, valid);
      case "remotes": return useRemotesStore().ensureLoaded(root, ownerGeneration, valid);
      case "files": return useFilesStore().ensureLoaded(root, ownerGeneration, valid);
      default: return Promise.resolve();
    }
  }

  async function refreshAfterTerminal(root: string, ownerGeneration: number): Promise<void> {
    if (snapshot.value?.rootPath !== root || generation.value !== ownerGeneration) return;
    const version = ++refreshVersion;
    refreshingModules.value = false;
    // Invalidate every module even when hidden; never keep pre-command HEAD/file data.
    useRefsStore().resetForRepository(root, ownerGeneration);
    useHistoryStore().resetForRepository(root, ownerGeneration);
    useStashesStore().resetForRepository(root, ownerGeneration);
    useFilesStore().resetForRepository(root, ownerGeneration);
    useRemotesStore().resetForRepository(root, ownerGeneration);
    await execute("refresh", () => backendClient.repositoryRefresh(root, true), false);
    if (snapshot.value?.rootPath !== root || generation.value !== ownerGeneration) return;
    // Hidden modules stay invalidated and load when opened. Only core workspace
    // reconciliation belongs to the command's lock; visible reads run separately.
    const workspace = snapshot.value;
    const valid = () => version === refreshVersion && generation.value === ownerGeneration && snapshot.value === workspace;
    const reads = [useTaskBranchesStore().refresh(), loadVisibleModule(root, ownerGeneration, valid)];
    refreshingModules.value = true;
    void Promise.allSettled(reads).then(results => {
      if (!valid()) return;
      const failure = results.find(item => item.status === "rejected");
      if (failure?.status === "rejected") error.value = normalizeBackendError(failure.reason);
      else if (useTaskBranchesStore().error) error.value = useTaskBranchesStore().error;
    }).finally(() => { if (version === refreshVersion) refreshingModules.value = false; });
  }

  return {
    snapshot,
    generation,
    operation,
    error,
    navigationBusy,
    refreshingModules,
    otherOperationBusy,
    viewNavigationBusy,
    refreshAfterTerminal,
    open,
    init,
    clone,
    refresh,
  };
});
