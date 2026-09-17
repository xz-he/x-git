import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "@/App.vue";
import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import type {
  AiRunEvent,
  AiRunAccepted,
  ChangesSnapshot,
  FileDiff,
  RepositorySnapshot,
} from "@/lib/backend/types";
import { useChangesStore } from "@/stores/changes";
import { useAiStore } from "@/stores/ai";
import { useRepositoryStore } from "@/stores/repository";
import { defaultSettings } from "@/stores/settings";
import { createBackendFixture } from "@/test/backend";

const runId = "11111111-1111-4111-8111-111111111111";
const repository: RepositorySnapshot = {
  rootPath: "D:\\work\\repo",
  name: "repo",
  currentBranch: "main",
  headShortHash: "abc1234",
  isClean: false,
  changedFileCount: 1,
  conflictCount: 0,
  remotes: [],
  upstream: null,
};
const context = {
  stagedFileCount: 1,
  textFileCount: 1,
  skippedBinaryFiles: [],
  fingerprint: "fingerprint",
};
const stagedDiff: FileDiff = {
  path: "src/main.ts",
  scope: "staged",
  binary: false,
  hunks: [
    {
      index: 0,
      header: "@@ -8 +8 @@",
      lines: [
        {
          kind: "addition",
          oldLine: null,
          newLine: 8,
          content: "const changed = true;",
        },
      ],
    },
  ],
};

enableAutoUnmount(afterEach);
const changes: ChangesSnapshot = {
  stagedCount: 1,
  unstagedCount: 0,
  files: [
    {
      path: "src/main.ts",
      oldPath: null,
      indexStatus: "M",
      worktreeStatus: " ",
      staged: true,
      unstaged: false,
      conflict: false,
    },
  ],
};

function accepted(
  requestedRunId: string,
  task: AiRunAccepted["task"],
): AiRunAccepted {
  return { runId: requestedRunId, task, context, totalBatchCount: 1 };
}

function createAiBackendFixture(): BackendClient {
  return createBackendFixture({
    aiStartReview: vi.fn(async (_path, requestedRunId) =>
      accepted(requestedRunId, "reviewChanges"),
    ),
    aiStartCommitMessage: vi.fn(async (_path, requestedRunId) =>
      accepted(requestedRunId, "generateCommitMessage"),
    ),
    aiListen: vi.fn(),
    changesSnapshot: vi.fn(async () => structuredClone(changes)),
    changesFileDiff: vi.fn(async () => stagedDiff),
    settingsLoad: vi.fn(async () => ({
      settings: defaultSettings({
        aiDrawerOpen: true,
        lastRepoPath: repository.rootPath,
        apiKey: "test-key",
      }),
    })),
  });
}

function message(sequence: number, event: AiRunEvent["event"]): AiRunEvent {
  return { runId, sequence, event };
}

describe("AI drawer workflows", () => {
  let backend: BackendClient;
  let emitAi: (event: AiRunEvent) => void;
  let pinia: ReturnType<typeof createPinia>;

  beforeEach(() => {
    document.body.innerHTML = "";
    pinia = createPinia();
    setActivePinia(pinia);
    backend = createAiBackendFixture();
    vi.mocked(backend.aiListen).mockImplementation(async (listener) => {
      emitAi = listener;
      return () => undefined;
    });
    setBackendClientForTests(backend);
    vi.stubGlobal("crypto", { randomUUID: vi.fn(() => runId) });
    useRepositoryStore().snapshot = repository;
    useChangesStore().snapshot = structuredClone(changes);
  });

  async function mountDrawer() {
    const wrapper = mount(App, {
      attachTo: document.body,
      global: { plugins: [pinia] },
    });
    await flushPromises();
    return wrapper;
  }

  it("starts staged review and navigates a finding to staged diff", async () => {
    const wrapper = await mountDrawer();
    expect(wrapper.text()).toContain("仅发送已暂存变更");

    await wrapper.get('[aria-label="审查已暂存变更"]').trigger("click");
    await flushPromises();
    expect(backend.aiStartReview).toHaveBeenCalledWith(repository.rootPath, runId);
    emitAi(
      message(2, {
        kind: "reviewCompleted",
        result: {
          summary: "发现 1 个问题",
          issues: [
            {
              severity: "warning",
              path: "src/main.ts",
              startLine: 8,
              endLine: 8,
              reason: "可能为空。",
              suggestedFix: "增加检查。",
            },
          ],
          reviewedFiles: ["src/main.ts"],
          skippedBinaryFiles: [],
          warnings: [],
        },
      }),
    );
    await flushPromises();
    await wrapper
      .get('[aria-label="查看 src/main.ts 第 8 行"]')
      .trigger("click");
    await flushPromises();

    expect(backend.changesFileDiff).toHaveBeenCalledWith(
      repository.rootPath,
      "src/main.ts",
      "staged",
    );
    expect(useChangesStore().highlightedLine).toBe(8);
  });

  it("disables staged-only tasks when no files are staged", async () => {
    useChangesStore().snapshot = { stagedCount: 0, unstagedCount: 1, files: [] };
    const wrapper = await mountDrawer();

    expect(
      wrapper.get('[aria-label="审查已暂存变更"]').attributes(),
    ).toHaveProperty("disabled");
    expect(
      wrapper.get('[aria-label="生成已暂存提交信息"]').attributes(),
    ).toHaveProperty("disabled");
  });

  it("shows progress, stops explicitly, and retains partial findings", async () => {
    const wrapper = await mountDrawer();
    await wrapper.get('[aria-label="审查已暂存变更"]').trigger("click");
    await flushPromises();
    emitAi(
      message(2, {
        kind: "reviewBatchCompleted",
        batchIndex: 1,
        issues: [
          {
            severity: "suggestion",
            path: "src/main.ts",
            startLine: 8,
            endLine: 8,
            reason: "可简化。",
            suggestedFix: "合并表达式。",
          },
        ],
      }),
    );
    emitAi(
      message(3, {
        kind: "cancelled",
        completedBatchCount: 1,
        totalBatchCount: 2,
      }),
    );
    await flushPromises();

    expect(wrapper.text()).toContain("已停止");
    expect(wrapper.text()).toContain("可简化。");
    await useAiStore().startReview();
    await wrapper.get('[aria-label="停止 AI 任务"]').trigger("click");
    expect(backend.aiCancel).toHaveBeenCalledWith(runId);
  });

  it("starts only one review when retry is triggered twice rapidly", async () => {
    const wrapper = await mountDrawer();
    await wrapper.get('[aria-label="审查已暂存变更"]').trigger("click");
    await flushPromises();
    emitAi(
      message(2, {
        kind: "failed",
        error: { code: "aiTransport", message: "连接失败。" },
      }),
    );
    await flushPromises();

    const retry = wrapper.get('[aria-label="重试审查"]');
    await Promise.all([retry.trigger("click"), retry.trigger("click")]);
    await flushPromises();

    expect(backend.aiStartReview).toHaveBeenCalledTimes(2);
  });

  it("orders findings by severity and renders warnings and skipped binaries", async () => {
    const wrapper = await mountDrawer();
    await wrapper.get('[aria-label="审查已暂存变更"]').trigger("click");
    await flushPromises();
    emitAi(
      message(2, {
        kind: "reviewCompleted",
        result: {
          summary: "发现 3 个问题",
          issues: [
            { severity: "suggestion", path: "c.ts", startLine: 3, endLine: 3, reason: "建议", suggestedFix: "调整" },
            { severity: "critical", path: "a.ts", startLine: 1, endLine: 1, reason: "严重", suggestedFix: "修复" },
            { severity: "warning", path: "b.ts", startLine: 2, endLine: 2, reason: "警告", suggestedFix: "检查" },
          ],
          reviewedFiles: ["a.ts", "b.ts", "c.ts"],
          skippedBinaryFiles: ["assets/logo.png"],
          warnings: ["规则文件未找到"],
        },
      }),
    );
    await flushPromises();

    expect(
      wrapper.findAll('[data-review-severity]').map((item) => item.attributes("data-review-severity")),
    ).toEqual(["critical", "warning", "suggestion"]);
    expect(wrapper.text()).toContain("assets/logo.png");
    expect(wrapper.text()).toContain("规则文件未找到");
  });

  it("keeps a frozen finding visible when its staged path no longer exists", async () => {
    const wrapper = await mountDrawer();
    await wrapper.get('[aria-label="审查已暂存变更"]').trigger("click");
    await flushPromises();
    emitAi(
      message(2, {
        kind: "reviewCompleted",
        result: {
          summary: "发现 1 个问题",
          issues: [{ severity: "warning", path: "removed.ts", startLine: 4, endLine: 4, reason: "问题", suggestedFix: "修复" }],
          reviewedFiles: ["removed.ts"],
          skippedBinaryFiles: [],
          warnings: [],
        },
      }),
    );
    await flushPromises();
    await wrapper.get('[aria-label="查看 removed.ts 第 4 行"]').trigger("click");

    expect(wrapper.text()).toContain("该位置已不在当前已暂存变更中。");
    expect(wrapper.text()).toContain("问题");
  });

  it("streams a commit message and explicitly fills an empty commit form", async () => {
    const wrapper = await mountDrawer();
    await wrapper.get('[aria-label="生成已暂存提交信息"]').trigger("click");
    await flushPromises();
    emitAi(message(2, { kind: "delta", text: "feat(ui): add review" }));
    emitAi(
      message(3, {
        kind: "commitMessageCompleted",
        result: {
          message: "feat(ui): add review",
          contextFingerprint: "fingerprint",
        },
      }),
    );
    await flushPromises();
    await wrapper.get('[aria-label="填入提交框"]').trigger("click");

    expect(useChangesStore().commitMessage).toBe("feat(ui): add review");
  });

  it("does not apply an invalid or cancelled commit preview", async () => {
    const wrapper = await mountDrawer();
    await wrapper.get('[aria-label="生成已暂存提交信息"]').trigger("click");
    await flushPromises();
    emitAi(
      message(2, {
        kind: "commitMessageCompleted",
        result: { message: "not conventional", contextFingerprint: "fingerprint" },
      }),
    );
    await flushPromises();

    expect(wrapper.text()).toContain("不符合 Conventional Commit 格式");
    expect(wrapper.get('[aria-label="填入提交框"]').attributes()).toHaveProperty("disabled");
    expect(useChangesStore().commitMessage).toBe("");
  });

  it("starts only one commit generation when retry is triggered twice rapidly", async () => {
    const wrapper = await mountDrawer();
    await wrapper
      .get('[aria-label="生成已暂存提交信息"]')
      .trigger("click");
    await flushPromises();
    emitAi(
      message(2, {
        kind: "failed",
        error: { code: "aiTransport", message: "连接失败。" },
      }),
    );
    await flushPromises();

    const retry = wrapper.get('[aria-label="重新生成提交信息"]');
    await Promise.all([retry.trigger("click"), retry.trigger("click")]);
    await flushPromises();

    expect(backend.aiStartCommitMessage).toHaveBeenCalledTimes(2);
  });

  it("confirms replacement and closing the drawer never cancels", async () => {
    const wrapper = await mountDrawer();
    useChangesStore().commitMessage = "fix: keep draft";
    await wrapper.get('[aria-label="生成已暂存提交信息"]').trigger("click");
    await flushPromises();
    emitAi(
      message(2, {
        kind: "commitMessageCompleted",
        result: {
          message: "feat(ui): generated",
          contextFingerprint: "fingerprint",
        },
      }),
    );
    await flushPromises();
    await wrapper.get('[aria-label="填入提交框"]').trigger("click");
    expect(wrapper.get('[role="alertdialog"]').text()).toContain(
      "fix: keep draft",
    );
    expect(document.activeElement?.getAttribute("aria-label")).toBe(
      "保留原内容",
    );
    await wrapper.get('[aria-label="替换提交信息"]').trigger("click");
    expect(useChangesStore().commitMessage).toBe("feat(ui): generated");

    await wrapper.get('[aria-label="关闭 AI 助手"]').trigger("click");
    expect(backend.aiCancel).not.toHaveBeenCalled();
  });
});
