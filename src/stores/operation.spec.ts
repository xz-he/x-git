import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";

import type {
  HistoryMutationResult,
  RefsMutationResult,
} from "@/lib/backend/types";
import { useChangesStore } from "@/stores/changes";
import { useHistoryStore } from "@/stores/history";
import { useOperationStore } from "@/stores/operation";
import { useRefsStore } from "@/stores/refs";
import { useRepositoryStore } from "@/stores/repository";

function workspace(conflictCount = 0) {
  return {
    repository: {
      rootPath: "C:/repo",
      name: "repo",
      currentBranch: "main",
      headShortHash: "abc1234",
      isClean: conflictCount === 0,
      changedFileCount: conflictCount,
      conflictCount,
      remotes: [],
      upstream: null,
    },
    changes: {
      files:
        conflictCount === 0
          ? []
          : [
              {
                path: "README.md",
                oldPath: null,
                indexStatus: "U",
                worktreeStatus: "U",
                staged: false,
                unstaged: true,
                conflict: true,
              },
            ],
      stagedCount: 0,
      unstagedCount: conflictCount,
    },
  };
}

describe("operation store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("atomically applies workspace refs and operation state", () => {
    const result: RefsMutationResult = {
      workspace: workspace(1),
      refs: {
        localBranches: [],
        remoteBranches: [],
        tags: [],
      },
      operationState: {
        kind: "merge",
        conflicts: [{ path: "README.md", status: "UU" }],
        abortAction: "merge",
      },
    };

    useOperationStore().applyMutationResult(result);

    expect(useRepositoryStore().snapshot?.conflictCount).toBe(1);
    expect(useChangesStore().snapshot?.files[0]?.conflict).toBe(true);
    expect(useRefsStore().snapshot).toEqual(result.refs);
    expect(useOperationStore().state).toEqual(result.operationState);
    expect(useOperationStore().isBlocked).toBe(true);
  });

  it("applies a refreshed history page and clears stale detail state", () => {
    const result: HistoryMutationResult = {
      workspace: workspace(),
      history: {
        commits: [],
        nextCursor: null,
        queryFingerprint: "fresh",
        continuationLanes: [],
      },
      operationState: {
        kind: "none",
        conflicts: [],
        abortAction: null,
      },
    };
    const history = useHistoryStore();
    history.selectedHash = "a".repeat(40);

    useOperationStore().applyMutationResult(result);

    expect(history.commits).toEqual([]);
    expect(history.queryFingerprint).toBe("fresh");
    expect(history.selectedHash).toBeUndefined();
    expect(useOperationStore().isBlocked).toBe(false);
  });
});
