import { createPinia, setActivePinia } from "pinia";
import { beforeEach, expect, it, vi } from "vitest";
import { setBackendClientForTests } from "@/lib/backend/client";
import type { CommitDetail, FileDiff } from "@/lib/backend/types";
import { createBackendFixture } from "@/test/backend";
import { useHistoryStore } from "./history";

const diff = (path: string): FileDiff => ({ path, scope: "commit", binary: false, hunks: [] });
const detail = (hash: string): CommitDetail => ({ hash, shortHash: hash, parentHashes: [], message: hash, authorName: "", authorEmail: "", authoredAt: "", committerName: "", committerEmail: "", committedAt: "", references: [], files: [] });
let backend: ReturnType<typeof createBackendFixture>;
beforeEach(() => {
  setActivePinia(createPinia());
  backend = createBackendFixture({ historyDetail: vi.fn(async (_root, hash) => detail(hash)), historyFileDiff: vi.fn(async (_root, _hash, path) => diff(path)) });
  setBackendClientForTests(backend);
});

it("retains multiple loaded files, deduplicates requests, and reuses cached diffs", async () => {
  const store = useHistoryStore();
  store.resetForRepository("C:/repo", 1);
  await store.selectCommit("one");
  let resolve!: (value: FileDiff) => void;
  vi.mocked(backend.historyFileDiff).mockReturnValueOnce(new Promise(next => { resolve = next; }));
  const pending = store.selectFile("a.ts");
  const repeated = store.selectFile("a.ts");
  expect(store.fileLoadingPaths).toContain("a.ts");
  await store.selectFile("b.ts");
  resolve(diff("a.ts"));
  await Promise.all([pending, repeated]);
  expect(store.fileDiffs.get("a.ts")).toEqual(diff("a.ts"));
  expect(store.fileDiffs.get("b.ts")).toEqual(diff("b.ts"));
  expect(store.selectedFilePath).toBe("b.ts");
  await store.selectFile("a.ts");
  expect(backend.historyFileDiff).toHaveBeenCalledTimes(2);
  expect(store.fileDiff).toEqual(diff("a.ts"));
});

it("discards old diff caches and pending results after commit or repository changes", async () => {
  const store = useHistoryStore();
  for (const replace of ["commit", "repository", "query", "mutation"]) {
    store.resetForRepository("C:/repo", 1);
    await store.selectCommit("one");
    await store.selectFile("cached.ts");
    let resolve!: (value: FileDiff) => void;
    vi.mocked(backend.historyFileDiff).mockReturnValueOnce(new Promise(next => { resolve = next; }));
    const pending = store.selectFile("late.ts");
    if (replace === "commit") await store.selectCommit("two");
    else if (replace === "repository") store.resetForRepository("C:/other", 2);
    else if (replace === "query") await store.setReference("other");
    else store.applyPage({ commits: [], nextCursor: null, queryFingerprint: "", continuationLanes: [] }, "C:/repo");
    resolve(diff("late.ts"));
    await pending;
    expect(store.fileDiffs.size).toBe(0);
    expect(store.fileLoadingPaths.size).toBe(0);
  }
});

it("exposes a per-file error and clears it after retry", async () => {
  const store = useHistoryStore();
  store.resetForRepository("C:/repo", 1);
  await store.selectCommit("one");
  vi.mocked(backend.historyFileDiff).mockRejectedValueOnce({ code: "gitCommandFailed", message: "读取失败" });
  await expect(store.selectFile("a.ts")).rejects.toMatchObject({ message: "读取失败" });
  expect(store.fileErrors.get("a.ts")?.message).toBe("读取失败");
  expect(store.fileLoadingPaths.size).toBe(0);
  await store.selectFile("a.ts");
  expect(store.fileErrors.size).toBe(0);
  expect(store.error).toBeUndefined();
  expect(store.fileDiffs.has("a.ts")).toBe(true);
});
