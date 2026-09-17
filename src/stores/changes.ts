import { defineStore } from "pinia";
import { ref } from "vue";

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
import { useTerminalStore } from "@/stores/terminal";

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
  let selectionVersion = 0;
  let loadVersion = 0;

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

  async function run<T>(
    kind: Exclude<ChangesOperation["kind"], "idle">,
    action: () => Promise<T>,
  ): Promise<T> {
    if (kind !== "load" && kind !== "diff" && useTerminalStore().busy) {
      throw { code: "gitOperationInProgress", message: "请等待终端命令结束后再操作。" } satisfies BackendError;
    }
    operation.value = { kind };
    error.value = undefined;
    try {
      return await action();
    } catch (cause) {
      error.value = normalizeBackendError(cause);
      throw error.value;
    } finally {
      operation.value = { kind: "idle" };
    }
  }

  function applyWorkspace(workspace: WorkingTreeSnapshot): void {
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
    await run("load", async () => {
      const nextSnapshot = await backendClient.changesSnapshot(rootPath);
      if (request !== loadVersion || useRepositoryStore().generation !== generation ||
        (useRepositoryStore().snapshot && useRepositoryStore().snapshot?.rootPath !== rootPath)) return;
      snapshot.value = nextSnapshot;
      loadedRootPath.value = rootPath;
      if (selection !== selectionVersion) return;
      if (preserveSelection && selected && nextSnapshot.files.some(file => file.path === selected && (scope === "staged" ? file.staged : file.unstaged))) {
        const diff = await backendClient.changesFileDiff(rootPath, selected, scope);
        if (request === loadVersion && generation === useRepositoryStore().generation && selection === selectionVersion) {
          selectedDiff.value = diff;
          highlightedLine.value = undefined;
        }
        return;
      }
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
    const generation = useRepositoryStore().generation;
    highlightedLine.value = undefined;
    navigationError.value = undefined;
    await run("diff", async () => {
      const diff = await backendClient.changesFileDiff(
        requireRootPath(),
        relativePath,
        scope,
      );
      if (selection !== selectionVersion || generation !== useRepositoryStore().generation) return;
      selectedPath.value = relativePath;
      selectedScope.value = scope;
      selectedDiff.value = diff;
    });
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
      navigationError.value = "该位置已不在当前已暂存变更中。";
      return false;
    }

    navigationError.value = undefined;
    await run("diff", async () => {
      const diff = await backendClient.changesFileDiff(
        requireRootPath(),
        relativePath,
        "staged",
      );
      selectedPath.value = relativePath;
      selectedScope.value = "staged";
      selectedDiff.value = diff;
      highlightedLine.value = line;
    });
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
