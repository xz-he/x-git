import { t } from '@/lib/i18n';
import { defineStore } from "pinia";
import { ref, watch } from "vue";

import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type {
  NoiseCandidate, NoiseScan, NoiseRestoreResult,
  BackendError,
  ChangeScope,
  ChangesSnapshot,
  CommitResult,
  FileDiff,
  MutationWorkspace,
  WorkingTreeSnapshot,
} from "@/lib/backend/types";
import { useOperationStore } from "@/stores/operation";
import { useRepositoryStore } from "@/stores/repository";

export type ChangesOperation =
  | { kind: "idle" }
  | {
      kind:
        | "load"
        | "diff"
        | "stageFile"
        | "stageFiles"
        | "unstageFiles"
        | "unstageFile"
        | "stageHunk"
        | "unstageHunk"
        | "stageLines"
        | "unstageLines"
        | "discardFile"
        | "scanNoise"
        | "restoreNoise"
        | "commit";
    };

export const useChangesStore = defineStore("changes", () => {
  const snapshot = ref<ChangesSnapshot>();
  const loadedRootPath = ref<string>();
  const selectedPath = ref<string>();
  const selectedScope = ref<ChangeScope>("unstaged");
  const selectedDiff = ref<FileDiff>();
  const highlightedLine = ref<number>();
  const navigationError = ref<string>();
  const commitMessage = ref("");
  const operation = ref<ChangesOperation>({ kind: "idle" });
  const error = ref<BackendError>();
  const diffLoading = ref(false);
  let selectionVersion = 0;
  let loadVersion = 0;
  let operationVersion = 0;

  function invalidateReads(): void {
    selectionVersion++;
    loadVersion++;
    diffLoading.value = false;
    selectedDiff.value = undefined;
  }
  watch(() => [useRepositoryStore().generation, useRepositoryStore().snapshot?.rootPath,
    useRepositoryStore().snapshot?.currentBranch, useRepositoryStore().snapshot?.headShortHash], invalidateReads, { flush: "sync" });

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

  async function run<T>(
    kind: Exclude<ChangesOperation["kind"], "idle">,
    action: () => Promise<T>,
  ): Promise<T> {
    if (kind !== "load" && kind !== "diff" && useRepositoryStore().navigationBusy) {
      throw { code: "gitOperationInProgress", get message() { return t('uiAGitOperationIsAlreadyBeingSubmittedb4d501'); } } satisfies BackendError;
    }
    if (kind !== "load" && kind !== "diff") invalidateReads();
    const owner = ++operationVersion;
    const generation = useRepositoryStore().generation;
    operation.value = { kind };
    error.value = undefined;
    try {
      return await action();
    } catch (cause) {
      const failure = normalizeBackendError(cause);
      if (owner === operationVersion && generation === useRepositoryStore().generation) error.value = failure;
      throw failure;
    } finally {
      if (owner === operationVersion) operation.value = { kind: "idle" };
    }
  }

  function applyWorkspace(workspace: WorkingTreeSnapshot): void {
    invalidateReads();
    useRepositoryStore().snapshot = workspace.repository;
    snapshot.value = workspace.changes;
    loadedRootPath.value = workspace.repository.rootPath;
    selectedDiff.value = undefined;
    highlightedLine.value = undefined;
    navigationError.value = undefined;
  }

  async function load(rootPath: string, preserveSelection = false): Promise<void> {
    const request = ++loadVersion;
    const generation = useRepositoryStore().generation;
    const selected = selectedPath.value;
    const scope = selectedScope.value;
    const selection = preserveSelection ? selectionVersion : ++selectionVersion;
    if (!preserveSelection) { diffLoading.value = false; selectedDiff.value = undefined; }
    await run("load", async () => {
      const nextSnapshot = await backendClient.changesSnapshot(rootPath);
      if (request !== loadVersion || useRepositoryStore().generation !== generation ||
        (useRepositoryStore().snapshot && useRepositoryStore().snapshot?.rootPath !== rootPath)) return;
      snapshot.value = nextSnapshot;
      loadedRootPath.value = rootPath;
      if (selection !== selectionVersion) return;
      if (preserveSelection && selected && nextSnapshot.files.some(file => file.path === selected && (scope === "staged" ? file.staged : file.unstaged))) {
        // File status is ready. A large diff must not extend the workspace write lock.
        void selectFile(selected, scope).catch(() => undefined);
        return;
      }
      selectionVersion++;
      diffLoading.value = false;
      selectedPath.value = undefined;
      selectedDiff.value = undefined;
      highlightedLine.value = undefined;
      navigationError.value = undefined;
    });
  }

  async function selectFile(
    relativePath: string,
    scope: ChangeScope,
  ): Promise<void> {
    const selection = ++selectionVersion;
    const repositories = useRepositoryStore();
    const generation = repositories.generation;
    const root = requireRootPath();
    const workspace = repositories.snapshot;
    const valid = () => selection === selectionVersion && generation === repositories.generation && workspace === repositories.snapshot;
    selectedPath.value = relativePath;
    selectedScope.value = scope;
    selectedDiff.value = undefined;
    diffLoading.value = true;
    highlightedLine.value = undefined;
    navigationError.value = undefined;
    error.value = undefined;
    try {
      const diff = await backendClient.changesFileDiff(
        root,
        relativePath,
        scope,
      );
      if (!valid()) return;
      selectedDiff.value = diff;
    } catch (cause) {
      if (!valid()) return;
      error.value = normalizeBackendError(cause);
      throw error.value;
    } finally {
      if (selection === selectionVersion) diffLoading.value = false;
    }
  }

  async function revealStagedLine(
    relativePath: string,
    line: number,
  ): Promise<boolean> {
    const isCurrentlyStaged = snapshot.value?.files.some(
      (file) => file.path === relativePath && file.staged,
    );
    if (!isCurrentlyStaged) {
      highlightedLine.value = undefined;
      navigationError.value = t('uiThisLocationIsNoLongerPartOfTheStagedChanges50ded4');
      return false;
    }

    const reading = selectFile(relativePath, "staged");
    const selection = selectionVersion;
    await reading;
    if (selection !== selectionVersion || !selectedDiff.value) return false;
    highlightedLine.value = line;
    return true;
  }

  async function mutate(
    kind: Exclude<ChangesOperation["kind"], "idle" | "load" | "diff" | "commit">,
    action: (rootPath: string) => Promise<MutationWorkspace>,
  ): Promise<void> {
    await run(kind, async () => {
      useOperationStore().applyMutationResult(
        await action(requireRootPath()),
      );
    });
  }

  function stageFiles(relativePaths: string[]): Promise<void> {
    const paths = [...relativePaths];
    return mutate("stageFiles", root => backendClient.changesStageFiles(root, paths));
  }
  function unstageFiles(relativePaths: string[]): Promise<void> {
    const paths = [...relativePaths];
    return mutate("unstageFiles", root => backendClient.changesUnstageFiles(root, paths));
  }
  function stageFile(relativePath: string): Promise<void> {
    return mutate("stageFile", (rootPath) =>
      backendClient.changesStageFile(rootPath, relativePath),
    );
  }

  function unstageFile(relativePath: string): Promise<void> {
    return mutate("unstageFile", (rootPath) =>
      backendClient.changesUnstageFile(rootPath, relativePath),
    );
  }

  function stageHunk(relativePath: string, hunkIndex: number): Promise<void> {
    return mutate("stageHunk", (rootPath) =>
      backendClient.changesStageHunk(rootPath, relativePath, hunkIndex),
    );
  }

  function unstageHunk(relativePath: string, hunkIndex: number): Promise<void> {
    return mutate("unstageHunk", (rootPath) =>
      backendClient.changesUnstageHunk(rootPath, relativePath, hunkIndex),
    );
  }

  function stageLines(
    relativePath: string,
    startLine: number,
    endLine: number,
  ): Promise<void> {
    return mutate("stageLines", (rootPath) =>
      backendClient.changesStageLines(
        rootPath,
        relativePath,
        startLine,
        endLine,
      ),
    );
  }

  function unstageLines(
    relativePath: string,
    startLine: number,
    endLine: number,
  ): Promise<void> {
    return mutate("unstageLines", (rootPath) =>
      backendClient.changesUnstageLines(
        rootPath,
        relativePath,
        startLine,
        endLine,
      ),
    );
  }

  function discardFile(relativePath: string): Promise<void> {
    return mutate("discardFile", (rootPath) =>
      backendClient.changesDiscardFile(rootPath, relativePath),
    );
  }

  function scanNoise(): Promise<NoiseScan> {
    return run("scanNoise", () => backendClient.changesScanNoise(requireRootPath()));
  }

  function restoreNoise(selected: NoiseCandidate[]): Promise<NoiseRestoreResult> {
    return run("restoreNoise", async () => {
      const result = await backendClient.changesRestoreNoise(requireRootPath(), selected);
      useOperationStore().applyMutationResult(result.workspace);
      return result;
    });
  }

  async function commit(): Promise<CommitResult> {
    return run("commit", async () => {
      const result = await backendClient.changesCommit(
        requireRootPath(),
        commitMessage.value,
      );
      useOperationStore().applyMutationResult(result.workspace);
      commitMessage.value = "";
      selectedPath.value = undefined;
      selectedDiff.value = undefined;
      return result;
    });
  }

  return {
    snapshot,
    selectedPath,
    selectedScope,
    selectedDiff,
    highlightedLine,
    navigationError,
    commitMessage,
    operation,
    error,
    diffLoading,
    applyWorkspace,
    load,
    selectFile,
    revealStagedLine,
    stageFile,
    stageFiles,
    unstageFiles,
    unstageFile,
    stageHunk,
    unstageHunk,
    stageLines,
    unstageLines,
    discardFile,
    scanNoise,
    restoreNoise,
    commit,
  };
});
