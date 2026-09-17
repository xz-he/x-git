import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useConflictSuggestionStore } from "./conflictSuggestion";
import { conflictDetail, conflictResult, setupConflictSuggestion } from "@/test/conflictSuggestion";
import type { ConflictDetail } from "@/lib/backend/types";

describe("AI conflict draft application", () => {
  beforeEach(() => setActivePinia(createPinia()));
  it("previews without mutation, revalidates twice and applies only to the draft", async () => {
    const { backend, conflicts } = await setupConflictSuggestion();
    conflicts.edit("my unsaved work");
    const store = useConflictSuggestionStore();
    await store.requestPreview();
    expect(store.preview?.before).toBe("my unsaved work");
    expect(conflicts.current?.resolution).toMatchObject({ text: "my unsaved work" });
    await store.confirmApply();
    expect(backend.conflictsDetail).toHaveBeenCalledTimes(2);
    expect(conflicts.current?.resolution).toMatchObject({ kind: "text", text: "candidate\n" });
    expect(conflicts.current?.dirty).toBe(true);
    expect(backend.conflictsResolve).not.toHaveBeenCalled();
    expect(conflicts.confirmation).toBeUndefined();
    expect(store.error).toBeUndefined();
    expect(store.appliedMessage).toContain("尚未写入或暂存");
  });
  it("preserves a valid empty text candidate instead of deleting", async () => {
    const { ai, conflicts } = await setupConflictSuggestion();
    ai.conflictResult = conflictResult("");
    const store = useConflictSuggestionStore();
    await store.requestPreview(); await store.confirmApply();
    expect(conflicts.current?.resolution).toMatchObject({ kind: "text", text: "" });
  });
  it("does not apply adviceOnly", async () => {
    const { ai, backend, conflicts } = await setupConflictSuggestion();
    ai.conflictResult = conflictResult(null);
    const store = useConflictSuggestionStore();
    await store.requestPreview();
    expect(store.preview).toBeUndefined();
    expect(backend.conflictsDetail).not.toHaveBeenCalled();
    expect(conflicts.current?.dirty).toBe(false);
  });
  it("rejects an externally changed source without losing dirty text", async () => {
    const { backend, conflicts } = await setupConflictSuggestion();
    conflicts.edit("keep");
    vi.mocked(backend.conflictsDetail).mockResolvedValue({ ...conflictDetail, token: "changed" });
    const store = useConflictSuggestionStore();
    await store.requestPreview();
    expect(store.error?.code).toBe("staleConflict");
    expect(store.preview).toBeUndefined();
    expect(conflicts.current?.resolution).toMatchObject({ text: "keep" });
  });
  it("invalidates a preview even when draft edits return to the same text", async () => {
    const { conflicts, backend } = await setupConflictSuggestion();
    const store = useConflictSuggestionStore();
    await store.requestPreview();
    conflicts.edit("temporary"); conflicts.edit("original\n");
    await store.confirmApply();
    expect(store.preview).toBeUndefined();
    expect(conflicts.current?.resolution).toMatchObject({ text: "original\n" });
    expect(backend.conflictsResolve).not.toHaveBeenCalled();
  });
  it("rejects changes detected during confirmation", async () => {
    const { backend, conflicts } = await setupConflictSuggestion();
    const store = useConflictSuggestionStore();
    await store.requestPreview();
    vi.mocked(backend.conflictsDetail).mockResolvedValue({ ...conflictDetail, token: "changed-after-preview" });
    await store.confirmApply();
    expect(store.error?.code).toBe("staleConflict");
    expect(conflicts.current?.dirty).toBe(false);
  });
  it("does not reopen a preview from a late response after repository replacement", async () => {
    const { backend, repository, conflicts } = await setupConflictSuggestion();
    let finish!: (detail: ConflictDetail) => void;
    vi.mocked(backend.conflictsDetail).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const store = useConflictSuggestionStore();
    const pending = store.requestPreview();
    repository.generation++;
    finish(conflictDetail); await pending;
    expect(store.preview).toBeUndefined();
    expect(conflicts.current?.dirty).toBe(false);
  });
  it("new AI runs invalidate old application intent and preserve the draft", async () => {
    const { ai, conflicts } = await setupConflictSuggestion();
    const store = useConflictSuggestionStore();
    await store.requestPreview();
    await ai.startConflictSuggestion(conflictDetail.path, conflictDetail.token);
    await store.confirmApply();
    expect(store.preview).toBeUndefined();
    expect(conflicts.current?.dirty).toBe(false);
  });
  it("preserves edits made while the confirmation backend check is pending", async () => {
    const { backend, conflicts } = await setupConflictSuggestion();
    const store = useConflictSuggestionStore();
    await store.requestPreview();
    let finish!: (detail: ConflictDetail) => void;
    vi.mocked(backend.conflictsDetail).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const pending = store.confirmApply();
    conflicts.edit("newer work");
    finish(conflictDetail); await pending;
    expect(conflicts.current?.resolution).toMatchObject({ text: "newer work" });
    expect(store.preview).toBeUndefined();
    expect(store.appliedMessage).toBe("");
    expect(backend.conflictsResolve).not.toHaveBeenCalled();
  });
  it("invalidates a preview when selecting away and back to the same path", async () => {
    const { conflicts } = await setupConflictSuggestion();
    const store = useConflictSuggestionStore();
    await store.requestPreview();
    conflicts.selectedPath = undefined;
    conflicts.selectedPath = conflictDetail.path;
    await store.confirmApply();
    expect(conflicts.current?.dirty).toBe(false);
    expect(store.preview).toBeUndefined();
  });
});
