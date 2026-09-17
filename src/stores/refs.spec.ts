import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import type {
  RefsMutationResult,
  RefsSnapshot,
} from "@/lib/backend/types";
import { useRefsStore } from "@/stores/refs";
import { useRepositoryStore } from "@/stores/repository";
import { createBackendFixture } from "@/test/backend";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function refsSnapshot(name: string): RefsSnapshot {
  return {
    localBranches: [
      {
        name,
        fullName: `refs/heads/${name}`,
        kind: "local",
        current: name === "main",
        tip: {
          fullHash: "a".repeat(40),
          shortHash: "aaaaaaa",
          subject: "subject",
          author: "HQ Test",
          authoredAt: "2026-09-08T10:00:00+08:00",
        },
      },
    ],
    remoteBranches: [],
    tags: [],
  };
}

describe("refs store", () => {
  let backend: BackendClient;

  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture();
    setBackendClientForTests(backend);
  });

  it("loads once per repository generation", async () => {
    vi.mocked(backend.refsSnapshot).mockResolvedValue(refsSnapshot("main"));
    const store = useRefsStore();

    await store.ensureLoaded("C:/repo", 1);
    await store.ensureLoaded("C:/repo", 1);

    expect(backend.refsSnapshot).toHaveBeenCalledTimes(1);
    expect(store.snapshot).toEqual(refsSnapshot("main"));
    expect(store.selectedFullName).toBe("refs/heads/main");
  });

  it("rejects a response from a replaced repository", async () => {
    const first = deferred<RefsSnapshot>();
    vi.mocked(backend.refsSnapshot).mockReturnValueOnce(first.promise);
    const store = useRefsStore();
    const pending = store.ensureLoaded("C:/one", 1);

    store.resetForRepository("C:/two", 2);
    first.resolve(refsSnapshot("main"));
    await pending;

    expect(store.snapshot).toBeUndefined();
    expect(store.loadedRootPath).toBe("C:/two");
  });

  it("blocks a second mutation before the first backend call finishes", async () => {
    const pending = deferred<RefsMutationResult>();
    vi.mocked(backend.refsMerge).mockReturnValue(pending.promise);
    useRepositoryStore().snapshot = {
      rootPath: "C:/repo",
      name: "repo",
      currentBranch: "main",
      headShortHash: "aaaaaaa",
      isClean: true,
      changedFileCount: 0,
      conflictCount: 0,
      remotes: [],
      upstream: null,
    };
    const store = useRefsStore();

    const first = store.merge("topic");
    expect(store.submitting).toBe(true);
    await expect(store.merge("topic")).rejects.toMatchObject({
      code: "gitOperationInProgress",
    });
    expect(backend.refsMerge).toHaveBeenCalledTimes(1);

    pending.resolve({
      workspace: {
        repository: useRepositoryStore().snapshot!,
        changes: { files: [], stagedCount: 0, unstagedCount: 0 },
      },
      refs: refsSnapshot("main"),
      operationState: { kind: "none", conflicts: [], abortAction: null },
    });
    await first;
    expect(store.submitting).toBe(false);
  });

  it("keeps an authoritative mutation snapshot over an older pending read", async () => {
    const oldRead = deferred<RefsSnapshot>();
    vi.mocked(backend.refsSnapshot).mockReturnValue(oldRead.promise);
    const store = useRefsStore();
    const pending = store.ensureLoaded("C:/repo", 1);

    store.applySnapshot(refsSnapshot("topic"), "C:/repo");
    oldRead.resolve(refsSnapshot("main"));
    await pending;

    expect(store.snapshot).toEqual(refsSnapshot("topic"));
    expect(store.loading).toBe(false);
  });
});
