import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AiTaskHome from "@/features/ai/AiTaskHome.vue";
import AiReviewView from "@/features/ai/AiReviewView.vue";
import HistoryDetail from "@/features/history/HistoryDetail.vue";
import AiDrawerShell from "@/components/layout/AiDrawerShell.vue";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { ReviewContext } from "@/lib/backend/types";
import { useAiStore } from "@/stores/ai";
import { useChangesStore } from "@/stores/changes";
import { useHistoryStore } from "@/stores/history";
import { useRepositoryStore } from "@/stores/repository";
import { useSettingsStore } from "@/stores/settings";
import { createBackendFixture } from "@/test/backend";

const root = "D:/fixture/review";
const hash = "a".repeat(40);
const context: ReviewContext = {
  source: { kind: "commit", revision: hash }, resolvedCommit: hash, baseCommit: null,
  changedFileCount: 1, skill: { directory: "code-review-expert", name: "code-review-expert", version: "v2.2.1", fingerprint: "rules", files: ["SKILL.md", "references/severity-guide.md"] },
  excludedFiles: ["bundle.map"], evidenceSources: [{ path: "main.py", revision: hash, startLine: 1, endLine: 20 }],
};
enableAutoUnmount(afterEach);
describe("skill review workbench", () => {
  let backend: BackendClient;
  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture();
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: root, name: "review", currentBranch: "main", headShortHash: "abc1234", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null };
    useSettingsStore().settings.apiKey = "fixture-key";
    useChangesStore().snapshot = { files: [], stagedCount: 1, unstagedCount: 0 };
  });

  it("requires the repository skill for review but not commit generation", async () => {
    vi.mocked(backend.aiReviewSkillStatus).mockResolvedValue({ state: "missing", info: null, error: null });
    await useAiStore().refreshReviewSkill();
    const wrapper = mount(AiTaskHome);
    await flushPromises();
    expect(wrapper.get('[aria-label="审查已暂存变更"]').attributes("disabled")).toBeDefined();
    expect(wrapper.get('[aria-label="生成已暂存提交信息"]').attributes("disabled")).toBeUndefined();
    expect(wrapper.text()).toContain("SKILL.md");
    expect(wrapper.text()).toContain("索引中的相关定义");
  });

  it("starts the selected historical commit with an empty index", async () => {
    useChangesStore().snapshot = { files: [], stagedCount: 0, unstagedCount: 0 };
    const history = useHistoryStore();
    history.loadedRootPath = root;
    history.selectedHash = hash;
    history.detail = { hash, shortHash: hash.slice(0, 7), parentHashes: [], message: "initial", authorName: "Test", authorEmail: "test@example.test", authoredAt: "2026-09-12", committerName: "Test", committerEmail: "test@example.test", committedAt: "2026-09-12", references: [], files: [] };
    vi.mocked(backend.aiStartReview).mockImplementation(async (_path, runId) => ({ runId, task: "reviewChanges", context: { stagedFileCount: 0, textFileCount: 1, skippedBinaryFiles: [], fingerprint: "frozen", review: context }, totalBatchCount: 1 }));
    const wrapper = mount(HistoryDetail);
    await flushPromises();
    await wrapper.get('[aria-label="AI 审查此提交"]').trigger("click");
    await flushPromises();
    expect(backend.aiStartReview).toHaveBeenCalledWith(root, expect.any(String), { kind: "commit", revision: hash });
    expect(useSettingsStore().settings.aiDrawerOpen).toBe(true);
  });

  it("shows the fixed commit, full evidence and uncovered scope, and copies a report", async () => {
    const ai = useAiStore();
    ai.runRoot = root;
    ai.reviewSource = context.source;
    ai.status = "completed";
    ai.partialIssues = [{ severity: "P1", path: "main.py", startLine: 8, endLine: 8, title: "确定异常", reason: "路径触发异常", impact: "请求失败", suggestedFix: "校验边界", evidence: "<script>literal evidence</script>", confidence: 8, contextMissing: [], changeRelation: "introduced", evidenceSources: context.evidenceSources }];
    ai.reviewResult = { context, summary: "发现 1 项", issues: ai.partialIssues, reviewedFiles: ["main.py"], skippedBinaryFiles: [], warnings: [], uncovered: ["外部依赖未获取"] };
    const copy = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
    const wrapper = mount(AiReviewView);
    expect(wrapper.text()).toContain(hash);
    expect(wrapper.text()).toContain("空树");
    expect(wrapper.text()).toContain("P1");
    expect(wrapper.text()).toContain("置信度");
    expect(wrapper.text()).toContain("外部依赖未获取");
    expect(wrapper.find("script").exists()).toBe(false);
    await wrapper.get('[aria-label="复制审查结果"]').trigger("click");
    await flushPromises();
    expect(copy).toHaveBeenCalledWith(expect.stringContaining("literal evidence"));
    expect(copy).toHaveBeenCalledWith(expect.stringContaining(hash));
  });

  it("opens a new historical review after returning to the task home", async () => {
    const ai = useAiStore();
    ai.currentTask = "reviewChanges";
    ai.runId = "old-run";
    ai.status = "completed";
    const wrapper = mount(AiDrawerShell, { props: { width: 360 } });
    await wrapper.get('[aria-label="返回 AI 任务"]').trigger("click");
    await flushPromises();
    vi.mocked(backend.aiStartReview).mockImplementation(async (_path, runId) => ({ runId, task: "reviewChanges", context: { stagedFileCount: 0, textFileCount: 1, skippedBinaryFiles: [], fingerprint: "new", review: context }, totalBatchCount: 1 }));
    await ai.startReview({ kind: "commit", revision: hash });
    await flushPromises();
    expect(wrapper.text()).toContain("历史提交审查");
  });

  it("displays and copies actual reviewed scope even when there are no findings", async () => {
    const ai = useAiStore();
    ai.status = "completed";
    ai.reviewResult = { context: { ...context, source: { kind: "commit", revision: "main" } }, summary: "未发现问题", issues: [], reviewedFiles: ["reviewed-only.py"], skippedBinaryFiles: ["image.png"], warnings: [], uncovered: [] };
    const copy = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
    const wrapper = mount(AiReviewView);
    expect(wrapper.text()).toContain("reviewed-only.py");
    await wrapper.get('[aria-label="复制审查结果"]').trigger("click");
    await flushPromises();
    const report = vi.mocked(navigator.clipboard.writeText).mock.calls[0]?.[0];
    expect(report).toContain("reviewed-only.py");
    expect(report).toContain("image.png");
    expect(report).toContain("main.py:1-20");
    expect(report).toContain(hash);
    expect(report).toContain("历史提交 " + hash);
  });

  it.each(["failed", "cancelled"] as const)("retains cumulative scope and policy after a partial %s review", async (status) => {
    const ai = useAiStore();
    ai.runRoot = root;
    ai.runId = "partial";
    ai.status = "running";
    ai.totalBatchCount = 2;
    ai.handleEvent({ runId: "partial", sequence: 1, event: {
      kind: "reviewBatchCompleted", batchIndex: 1, issues: [],
      result: { markdown: "## 首批报告\n\n**P1**：保留此问题", context: { ...context, skill: { ...context.skill, fingerprint: "expanded-rules" } }, summary: "完成首批", issues: [], reviewedFiles: ["first.py"], skippedBinaryFiles: [], warnings: ["首批提示"], uncovered: ["未读依赖"] },
    } });
    ai.handleEvent({ runId: "partial", sequence: 2, event: status === "cancelled"
      ? { kind: "cancelled", completedBatchCount: 1, totalBatchCount: 2 }
      : { kind: "failed", error: { code: "aiTransport", message: "连接中断" } } });
    const copy = vi.fn(async (_value: string) => undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
    const wrapper = mount(AiReviewView);
    expect(wrapper.text()).toContain("first.py");
    expect(wrapper.text()).toContain("未读依赖");
    expect(wrapper.get(".review-markdown h2").text()).toBe("首批报告");
    expect(wrapper.text()).not.toContain("未发现需要处理的问题");
    await wrapper.get('[aria-label="复制审查结果"]').trigger("click");
    await flushPromises();
    expect(copy.mock.calls[0]?.[0]).toContain("审查未完成（1/2 批）");
    expect(copy.mock.calls[0]?.[0]).toContain("expanded-rules");
    expect(copy.mock.calls[0]?.[0]).toContain("**P1**：保留此问题");
    expect(copy.mock.calls[0]?.[0]).toContain("first.py");
    vi.mocked(backend.aiStartReview).mockImplementation(async (_path, runId) => ({ runId, task: "reviewChanges", context: { stagedFileCount: 1, textFileCount: 1, skippedBinaryFiles: [], fingerprint: "retry" }, totalBatchCount: 1 }));
    await ai.startReview();
    expect(wrapper.text()).not.toContain("first.py");
  });

  it("displays and copies Markdown findings without inventing a clean conclusion", async () => {
    const ai = useAiStore();
    ai.status = "completed";
    const markdown = "## 审查摘要\n\n**P1**：请求可能失败\n\n### 未覆盖范围\n\n未读取依赖。";
    ai.reviewResult = { markdown, context, summary: "审查已完成", issues: [], reviewedFiles: ["main.py"], skippedBinaryFiles: [], warnings: [] };
    const copy = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
    const wrapper = mount(AiReviewView);
    expect(wrapper.get(".review-markdown h2").text()).toBe("审查摘要");
    expect(wrapper.get(".review-markdown strong").text()).toBe("P1");
    expect(wrapper.text()).not.toContain("未发现需要处理的问题");
    await wrapper.get('[aria-label="复制审查结果"]').trigger("click");
    await flushPromises();
    expect(copy).toHaveBeenCalledWith(expect.stringContaining(markdown));
    expect(copy).toHaveBeenCalledWith(expect.stringContaining(hash));
  });
});
