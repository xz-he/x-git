import { computed, ref } from "vue";
import { defineStore } from "pinia";

import type {
  HistoryMutationResult,
  MutationWorkspace,
  RefsMutationResult,
  RemoteOperationResult,
  RepositoryOperationState,
  StashMutationResult,
} from "@/lib/backend/types";
import { useChangesStore } from "@/stores/changes";
import { useHistoryStore } from "@/stores/history";
import { useRefsStore } from "@/stores/refs";
import { useStashesStore } from "@/stores/stashes";

type MutationResult =
  | MutationWorkspace
  | StashMutationResult
  | RefsMutationResult
  | RemoteOperationResult
  | HistoryMutationResult;

function idleState(): RepositoryOperationState {
  return { kind: "none", conflicts: [], abortAction: null };
}

export const useOperationStore = defineStore("operation-state", () => {
  const state = ref<RepositoryOperationState>(idleState());
  const isBlocked = computed(
    () => state.value.kind !== "none" || state.value.conflicts.length > 0,
  );

  function applyMutationResult(result: MutationResult): void {
    useChangesStore().applyWorkspace(result.workspace);
    state.value = result.operationState;
    if ("stashes" in result) {
      useStashesStore().applySnapshot(result.stashes, result.workspace.repository.rootPath);
    }
    if ("refs" in result) {
      useRefsStore().applySnapshot(
        result.refs,
        result.workspace.repository.rootPath,
      );
    }
    if ("history" in result) {
      useHistoryStore().applyPage(
        result.history,
        result.workspace.repository.rootPath,
      );
    }
  }

  function reset(): void {
    state.value = idleState();
  }

  return { state, isBlocked, applyMutationResult, reset };
});
