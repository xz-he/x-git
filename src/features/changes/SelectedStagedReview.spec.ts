import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ChangesList from "./ChangesList.vue";
import AiReviewView from "@/features/ai/AiReviewView.vue";
import { reviewReport } from "@/features/ai/reviewPresentation";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import { createBackendFixture } from "@/test/backend";
import { useAiStore } from "@/stores/ai";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { useSettingsStore } from "@/stores/settings";
import type { ReviewContext } from "@/lib/backend/types";

const root = "D:/selected-review";
const paths = ["src/a.ts", "docs/中文 [x].md", "src/c.ts"];
enableAutoUnmount(afterEach);

describe("selected staged review", () => {
  let backend: BackendClient;
  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture({
      aiStartReview: vi.fn(async (_root, runId) => ({ runId, task: "reviewChanges" as const, context: { stagedFileCount: 2, textFileCount: 2, skippedBinaryFiles: [], fingerprint: "selected" }, totalBatchCount: 1 })),
    });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: root, name: "selected-review", currentBranch: "main", headShortHash: "abc1234", isClean: false, changedFileCount: 4, conflictCount: 0, remotes: [], upstream: null };
    Object.assign(useSettingsStore().settings, { apiKey: "fixture-key", baseUrl: "https://example.test/v1", model: "fixture-model", aiDrawerOpen: false });
    useChangesStore().snapshot = { stagedCount: 3, unstagedCount: 1, files: [...paths.map(path => ({ path, oldPath: null, indexStatus: "M", worktreeStatus: " ", staged: true, unstaged: false, conflict: false })), { path: "unstaged.ts", oldPath: null, indexStatus: " ", worktreeStatus: "M", staged: false, unstaged: true, conflict: false }] };
  });

  it("reviews only checked staged files without changing the index or preview", async () => {
    const wrapper = mount(ChangesList);
    await flushPromises();
    const review = wrapper.get('[aria-label="AI 审查所选暂存文件"]');
    expect(review.attributes("disabled")).toBeDefined();
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(6);
    await wrapper.get('[aria-label="选择已暂存文件 src/a.ts"]').setValue(true);
    await wrapper.get('[aria-label="选择已暂存文件 docs/中文 [x].md"]').setValue(true);
    expect((wrapper.get('[aria-label="全选已暂存文件"]').element as HTMLInputElement).indeterminate).toBe(true);
    expect(backend.changesFileDiff).not.toHaveBeenCalled();
    await review.trigger("click");
    await flushPromises();
    expect(backend.aiStartReview).toHaveBeenCalledWith(root, expect.any(String), { kind: "stagedFiles", paths: paths.slice(0, 2) });
    expect(useSettingsStore().settings.aiDrawerOpen).toBe(true);
    expect(backend.changesStageFile).not.toHaveBeenCalled();
    expect(backend.changesUnstageFile).not.toHaveBeenCalled();
    expect(review.attributes("disabled")).toBeDefined();
  });

  it("supports select all and clears selections when files are unstaged or repository changes", async () => {
    const wrapper = mount(ChangesList);
    await flushPromises();
    const all = wrapper.get('[aria-label="全选已暂存文件"]');
    await all.setValue(true);
    expect(wrapper.text()).toContain("已选 3");
    useChangesStore().snapshot!.files[0]!.staged = false;
    await flushPromises();
    expect(wrapper.text()).toContain("已选 2");
    await all.setValue(false);
    expect(wrapper.get('[aria-label="AI 审查所选暂存文件"]').attributes("disabled")).toBeDefined();
    await all.setValue(true);
    useRepositoryStore().generation += 1;
    await flushPromises();
    expect((all.element as HTMLInputElement).checked).toBe(false);
    expect(wrapper.get('[aria-label="AI 审查所选暂存文件"]').attributes("disabled")).toBeDefined();
    await all.setValue(true);
    useRepositoryStore().snapshot = { ...useRepositoryStore().snapshot!, rootPath: "D:/other" };
    await flushPromises();
    expect(wrapper.text()).toContain("已选 0");
  });

  it("requires repository skill before starting", async () => {
    vi.mocked(backend.aiReviewSkillStatus).mockResolvedValue({ state: "missing", info: null, error: null });
    const wrapper = mount(ChangesList);
    await flushPromises();
    await wrapper.get('[aria-label="全选已暂存文件"]').setValue(true);
    expect(wrapper.get('[aria-label="AI 审查所选暂存文件"]').attributes("disabled")).toBeDefined();
    expect(wrapper.text()).toContain("SKILL");
    expect(backend.aiStartReview).not.toHaveBeenCalled();
  });

  it("freezes selected paths before waiting for the AI listener", async () => {
    const source = { kind: "stagedFiles" as const, paths: [paths[0]!] };
    const started = useAiStore().startReview(source);
    source.paths.push(paths[1]!);
    await started;
    expect(useAiStore().reviewSource).toEqual({ kind: "stagedFiles", paths: [paths[0]] });
    expect(backend.aiStartReview).toHaveBeenCalledWith(root, expect.any(String), { kind: "stagedFiles", paths: [paths[0]] });
  });

  it("shows and retries the selected scope and keeps it in the exported report", async () => {
    const ai = useAiStore();
    const context: ReviewContext = { source: { kind: "stagedFiles", paths: paths.slice(0, 2) }, resolvedCommit: "a".repeat(40), baseCommit: "a".repeat(40), changedFileCount: 2, skill: { directory: "code-review-expert", name: "review", version: null, fingerprint: "rules", files: ["SKILL.md"] }, excludedFiles: [], evidenceSources: [] };
    ai.runRoot = root;
    ai.status = "cancelled";
    ai.reviewSource = context.source;
    ai.context = { stagedFileCount: 2, textFileCount: 2, skippedBinaryFiles: [], fingerprint: "frozen", review: context };
    const wrapper = mount(AiReviewView);
    expect(wrapper.text()).toContain("所选暂存文件");
    expect(wrapper.text()).toContain(paths[0]);
    expect(wrapper.text()).toContain(paths[1]);
    const report = reviewReport([], context.source, context, "summary", [], []);
    expect(report).toContain("所选暂存文件");
    expect(report).toContain(paths[1]);
    expect(report).not.toContain("历史提交");
    await wrapper.get('[aria-label="重新审查"]').trigger("click");
    await flushPromises();
    expect(backend.aiStartReview).toHaveBeenCalledWith(root, expect.any(String), context.source);
  });
});
