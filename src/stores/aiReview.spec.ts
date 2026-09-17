import { createPinia, setActivePinia } from "pinia";
import { flushPromises } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { AiRunAccepted, RepositorySnapshot, ReviewSkillStatus } from "@/lib/backend/types";
import { useAiStore } from "@/stores/ai";
import { useRepositoryStore } from "@/stores/repository";
import { defaultSettings } from "@/stores/settings";
import { createBackendFixture } from "@/test/backend";

const root = "D:/fixture/review";
const hash = "a".repeat(40);
const repository: RepositorySnapshot = { rootPath: root, name: "review", currentBranch: "main", headShortHash: "abc1234", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null };
const ready: ReviewSkillStatus = { state: "ready", info: { directory: "code-review-expert", name: "code-review-expert", version: "v2.2.1", fingerprint: "rules", files: ["SKILL.md"] }, error: null };
const acceptance = (runId: string): AiRunAccepted => ({ runId, task: "reviewChanges", context: { stagedFileCount: 0, textFileCount: 1, skippedBinaryFiles: [], fingerprint: "context" }, totalBatchCount: 1 });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; }

describe("repository skill review lifecycle", () => {
  let backend: BackendClient;
  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture({ aiStartReview: vi.fn(async (_path, id) => acceptance(id)), aiReviewSkillStatus: vi.fn(async () => ready) });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { ...repository };
  });

  it("captures a historical review source without requiring staged changes", async () => {
    const ai = useAiStore();
    const source = { kind: "commit" as const, revision: hash };
    const start = ai.startReview(source);
    source.revision = "b".repeat(40);
    await start;
    expect(ai.reviewSource).toEqual({ kind: "commit", revision: hash });
    expect(backend.aiStartReview).toHaveBeenCalledWith(root, expect.any(String), { kind: "commit", revision: hash });
    expect(backend.aiListen).toHaveBeenCalledTimes(1);
  });

  it("prevents duplicate starts while acceptance is pending", async () => {
    const pending = deferred<AiRunAccepted>();
    vi.mocked(backend.aiStartReview).mockReturnValue(pending.promise);
    const ai = useAiStore();
    const first = ai.startReview();
    await flushPromises();
    await expect(ai.startReview()).rejects.toMatchObject({ code: "gitOperationInProgress" });
    pending.resolve(acceptance(ai.runId!));
    await first;
    expect(backend.aiStartReview).toHaveBeenCalledTimes(1);
  });

  it("preserves a terminal result over late start failure and events", async () => {
    const ai = useAiStore();
    vi.mocked(backend.aiStartReview).mockImplementation(async (_path, runId) => {
      ai.handleEvent({ runId, sequence: 2, event: { kind: "reviewCompleted", result: { summary: "finished", issues: [], reviewedFiles: [], skippedBinaryFiles: [], warnings: [] } } });
      throw { code: "aiTransport", message: "late rejection" };
    });
    await ai.startReview();
    ai.handleEvent({ runId: ai.runId!, sequence: 3, event: { kind: "started", context: acceptance(ai.runId!).context, totalBatchCount: 1 } });
    expect(ai.status).toBe("completed");
    expect(ai.error).toBeUndefined();
    expect(ai.reviewResult?.summary).toBe("finished");
  });

  it("does not attach a listener when disposed during subscription", async () => {
    const pending = deferred<() => void>();
    const unlisten = vi.fn();
    vi.mocked(backend.aiListen).mockReturnValue(pending.promise);
    const ai = useAiStore();
    const init = ai.initialize();
    ai.dispose();
    pending.resolve(unlisten);
    await init;
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("allows retry after cancellation transport failure", async () => {
    const ai = useAiStore();
    await ai.startReview();
    vi.mocked(backend.aiCancel).mockRejectedValueOnce({ code: "aiTransport", message: "retry" });
    await expect(ai.cancelActive()).rejects.toMatchObject({ code: "aiTransport" });
    await ai.cancelActive();
    expect(backend.aiCancel).toHaveBeenCalledTimes(2);
  });

  it("isolates skill status from stale repository reads", async () => {
    const pending = deferred<ReviewSkillStatus>();
    vi.mocked(backend.aiReviewSkillStatus).mockReturnValueOnce(pending.promise).mockResolvedValueOnce({ state: "missing", info: null, error: null });
    const ai = useAiStore();
    const old = ai.refreshReviewSkill();
    useRepositoryStore().snapshot = { ...repository, rootPath: "D:/fixture/other" };
    await ai.refreshReviewSkill();
    pending.resolve(ready);
    await old;
    expect(ai.reviewSkill?.state).toBe("missing");
    expect(ai.reviewSkillRoot).toBe("D:/fixture/other");
  });

  it("clears successful review and rule ownership on abandonment", async () => {
    const ai = useAiStore();
    await ai.refreshReviewSkill();
    await ai.startReview({ kind: "commit", revision: hash });
    ai.handleEvent({ runId: ai.runId!, sequence: 2, event: { kind: "reviewCompleted", result: { summary: "old", issues: [], reviewedFiles: [], skippedBinaryFiles: [], warnings: [] } } });
    await ai.cancelAndAbandon();
    expect(ai.reviewResult).toBeUndefined();
    expect(ai.reviewSource).toBeUndefined();
    expect(ai.reviewSkill).toBeUndefined();
  });

  it("normalizes the review directory while retaining legacy configuration", () => {
    const settings = defaultSettings({ reviewSkillDirectory: " rules\\review ", reviewRuleFiles: ["old.md"], useReviewRuleFilesInReview: true });
    expect(settings.reviewSkillDirectory).toBe("rules/review");
    expect(settings.reviewRuleFiles).toEqual(["old.md"]);
    expect(settings.useReviewRuleFilesInReview).toBe(true);
  });
});
