import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import type {
  ChangesSnapshot,
  FileDiff,
  MutationWorkspace,
  RepositorySnapshot,
  WorkingTreeSnapshot,
} from "@/lib/backend/types";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { createBackendFixture } from "@/test/backend";

function repositoryFixture(
  overrides: Partial<RepositorySnapshot> = {},
): RepositorySnapshot {
  return {
    rootPath: "D:\\work\\repo",
    name: "repo",
    currentBranch: "main",
    headShortHash: "abc1234",
    isClean: false,
    changedFileCount: 1,
    conflictCount: 0,
    remotes: [],
    upstream: null,
    ...overrides,
  };
}

function changesFixture(): ChangesSnapshot {
  return {
    stagedCount: 0,
    unstagedCount: 1,
    files: [
      {
        path: "src/main.ts",
        oldPath: null,
        indexStatus: " ",
        worktreeStatus: "M",
        staged: false,
        unstaged: true,
        conflict: false,
      },
    ],
  };
}

function diffFixture(): FileDiff {
  return {
    path: "src/main.ts",
    scope: "unstaged",
    binary: false,
    hunks: [
      {
        index: 0,
        header: "@@ -1 +1 @@",
        lines: [
          {
            kind: "addition",
            oldLine: null,
            newLine: 1,
            content: "changed",
          },
        ],
      },
    ],
  };
}

function workingTreeFixture(): WorkingTreeSnapshot {
  return {
    repository: repositoryFixture({
      changedFileCount: 1,
      headShortHash: "def5678",
    }),
    changes: {
      ...changesFixture(),
      stagedCount: 1,
      unstagedCount: 0,
      files: [
        {
          ...changesFixture().files[0]!,
          indexStatus: "M",
          worktreeStatus: " ",
          staged: true,
          unstaged: false,
        },
      ],
    },
  };
}

function mutationFixture(): MutationWorkspace {
  return {
    workspace: workingTreeFixture(),
    operationState: {
      kind: "none",
      conflicts: [],
      abortAction: null,
    },
  };
}

describe("changes store", () => {
  let backend: BackendClient;

  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture();
    setBackendClientForTests(backend);
  });

  it("loads a snapshot and selects the requested diff scope", async () => {
    vi.mocked(backend.changesSnapshot).mockResolvedValue(changesFixture());
    vi.mocked(backend.changesFileDiff).mockResolvedValue(diffFixture());
    const store = useChangesStore();

    await store.load("D:\\work\\repo");
    await store.selectFile("src/main.ts", "unstaged");

    expect(store.snapshot).toEqual(changesFixture());
    expect(backend.changesFileDiff).toHaveBeenCalledWith(
      "D:\\work\\repo",
      "src/main.ts",
      "unstaged",
    );
    expect(store.selectedDiff).toEqual(diffFixture());
  });

  it("replaces repository and changes from one mutation result", async () => {
    vi.mocked(backend.changesStageFile).mockResolvedValue(mutationFixture());
    const repositoryStore = useRepositoryStore();
    repositoryStore.snapshot = repositoryFixture();
    const store = useChangesStore();

    await store.stageFile("src/main.ts");

    expect(repositoryStore.snapshot).toEqual(
      mutationFixture().workspace.repository,
    );
    expect(store.snapshot).toEqual(mutationFixture().workspace.changes);
    expect(store.operation).toEqual({ kind: "idle" });
  });

  it("retains previous snapshots when a mutation fails", async () => {
    const previousRepository = repositoryFixture();
    const previousChanges = changesFixture();
    const repositoryStore = useRepositoryStore();
    repositoryStore.snapshot = previousRepository;
    const store = useChangesStore();
    store.snapshot = previousChanges;
    vi.mocked(backend.changesStageFile).mockRejectedValue({
      code: "gitLocked",
      message: "Git 正在执行其他操作。",
    });

    await expect(store.stageFile("src/main.ts")).rejects.toMatchObject({
      code: "gitLocked",
    });

    expect(repositoryStore.snapshot).toEqual(previousRepository);
    expect(store.snapshot).toEqual(previousChanges);
    expect(store.error?.code).toBe("gitLocked");
  });

  it("clears commit input and selected diff after a successful commit", async () => {
    const workspace = mutationFixture();
    workspace.workspace.changes = {
      files: [],
      stagedCount: 0,
      unstagedCount: 0,
    };
    vi.mocked(backend.changesCommit).mockResolvedValue({
      shortHash: "def5678",
      subject: "feat: save work",
      workspace,
    });
    const repositoryStore = useRepositoryStore();
    repositoryStore.snapshot = repositoryFixture();
    const store = useChangesStore();
    store.commitMessage = "feat: save work";
    store.selectedDiff = diffFixture();

    const result = await store.commit();

    expect(result.shortHash).toBe("def5678");
    expect(store.commitMessage).toBe("");
    expect(store.selectedPath).toBeUndefined();
    expect(store.selectedDiff).toBeUndefined();
    expect(repositoryStore.snapshot).toEqual(workspace.workspace.repository);
    expect(store.snapshot).toEqual(workspace.workspace.changes);
  });

  it("repository open loads changes for the resolved root path", async () => {
    vi.mocked(backend.repositoryOpen).mockResolvedValue(repositoryFixture());
    vi.mocked(backend.changesSnapshot).mockResolvedValue(changesFixture());

    await useRepositoryStore().open("D:\\work\\repo\\nested");

    expect(backend.changesSnapshot).toHaveBeenCalledWith("D:\\work\\repo");
    expect(useChangesStore().snapshot).toEqual(changesFixture());
  });
});
