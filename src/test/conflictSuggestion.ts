import { vi } from "vitest";
import { createBackendFixture } from "./backend";
import { setBackendClientForTests } from "@/lib/backend/client";
import { useRepositoryStore } from "@/stores/repository";
import { useConflictsStore } from "@/stores/conflicts";
import { useAiStore } from "@/stores/ai";
import { useUiStore } from "@/stores/ui";
import { useSettingsStore } from "@/stores/settings";
import type { AiConflictSuggestionResult, AiRunAccepted, ConflictDetail, ConflictVersion, ConflictSnapshot } from "@/lib/backend/types";

export const conflictVersion: ConflictVersion = { exists: true, oid: "a".repeat(40), mode: "100644", kind: "text", text: "original\n", byteLength: 9, bom: false, lineEnding: "lf" };
export const conflictDetail: ConflictDetail = { path: "src/冲突.txt", token: "file-token", operationKind: "merge", base: conflictVersion, ours: { ...conflictVersion, text: "ours\n" }, theirs: { ...conflictVersion, text: "theirs\n" }, working: conflictVersion, editable: true, canChooseOurs: true, canChooseTheirs: true, canDelete: true, unsupportedReason: null };
export function conflictResult(text: string | null = "candidate\n"): AiConflictSuggestionResult {
  return { kind: text === null ? "adviceOnly" : "text", summary: "保留双方修改", explanation: "合并边界处理", resolvedText: text, risks: ["请验证业务行为"], contextMissing: [], context: { path: conflictDetail.path, token: conflictDetail.token, operationKind: "merge", baseOid: conflictVersion.oid, oursOid: conflictVersion.oid, theirsOid: conflictVersion.oid, fingerprint: "frozen" } };
}
export async function setupConflictSuggestion() {
  const repository = useRepositoryStore();
  repository.snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abcdef0", upstream: null, isClean: false, changedFileCount: 1, conflictCount: 1, remotes: [] };
  useSettingsStore().settings.apiKey = "fixture-only";
  const backend = createBackendFixture({
    conflictsDetail: vi.fn(async () => structuredClone(conflictDetail)),
    conflictsSnapshot: vi.fn(async (): Promise<ConflictSnapshot> => ({ operationState: { kind: "merge", conflicts: [{ path: conflictDetail.path, status: "UU" }], abortAction: "merge" }, operationToken: "operation", files: [{ path: conflictDetail.path, status: "UU", supported: true, reason: null }], continueAction: null, stagedFiles: [] })),
    aiStartConflictSuggestion: vi.fn(async (_root: string, runId: string): Promise<AiRunAccepted> => ({ runId, task: "resolveConflict", context: { conflict: conflictResult().context, stagedFileCount: 0, textFileCount: 1, skippedBinaryFiles: [], fingerprint: "frozen" }, totalBatchCount: 1 })),
  });
  setBackendClientForTests(backend);
  const conflicts = useConflictsStore();
  await conflicts.ensureLoaded("C:/repo", repository.generation);
  await conflicts.selectFile(conflictDetail.path);
  useUiStore().openView("conflicts");
  const ai = useAiStore();
  await ai.startConflictSuggestion(conflictDetail.path, conflictDetail.token);
  ai.handleEvent({ runId: ai.runId!, sequence: 2, event: { kind: "conflictSuggestionCompleted", result: conflictResult() } });
  vi.mocked(backend.conflictsDetail).mockClear();
  return { backend, ai, conflicts, repository };
}
