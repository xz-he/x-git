import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { useAiStore } from "./ai";
import { useRepositoryStore } from "./repository";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests } from "@/lib/backend/client";
import type { AiRunAccepted } from "@/lib/backend/types";

describe("conflict suggestion AI lifecycle", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    useRepositoryStore().snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abcdef0", upstream: null, isClean: false, changedFileCount: 1, conflictCount: 1, remotes: [] };
  });
  it("captures path and token before awaiting and does not require review SKILL", async () => {
    const backend = createBackendFixture({ aiStartConflictSuggestion: vi.fn(async (_root: string, runId: string): Promise<AiRunAccepted> => ({ runId, task: "resolveConflict", context: { stagedFileCount: 0, textFileCount: 1, skippedBinaryFiles: [], fingerprint: "fixed" }, totalBatchCount: 1 })) });
    setBackendClientForTests(backend);
    const ai = useAiStore();
    await ai.startConflictSuggestion("a.txt", "token-a");
    expect(backend.aiStartConflictSuggestion).toHaveBeenCalledWith("C:/repo", expect.any(String), "a.txt", "token-a");
    expect(backend.aiReviewSkillStatus).not.toHaveBeenCalled();
    expect(ai.conflictTarget).toEqual({ relativePath: "a.txt", token: "token-a" });
    expect(ai.currentTask).toBe("resolveConflict");
  });
  it("shares the active run guard with reviews and retains terminal events before acceptance", async () => {
    let accept!: (value: AiRunAccepted) => void;
    const backend = createBackendFixture({ aiStartConflictSuggestion: vi.fn(() => new Promise<AiRunAccepted>(resolve => { accept = resolve; })) });
    setBackendClientForTests(backend);
    const ai = useAiStore();
    const pending = ai.startConflictSuggestion("a.txt", "token-a");
    await flushPromises();
    await expect(ai.startReview()).rejects.toMatchObject({ code: "gitOperationInProgress" });
    ai.handleEvent({ runId: ai.runId!, sequence: 2, event: { kind: "cancelled", completedBatchCount: 0, totalBatchCount: 1 } });
    accept({ runId: ai.runId!, task: "resolveConflict", context: { stagedFileCount: 0, textFileCount: 1, skippedBinaryFiles: [], fingerprint: "fixed" }, totalBatchCount: 1 });
    await pending;
    expect(ai.status).toBe("cancelled");
    expect(ai.conflictResult).toBeUndefined();
  });
});
