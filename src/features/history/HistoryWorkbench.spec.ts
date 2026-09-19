import { flushPromises, mount } from "@vue/test-utils";
import { selectOption } from "@/test/select";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "@/App.vue";
import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import type {
  CommitDetail,
  CommitSummary,
  HistoryPage,
  RepositorySnapshot,
} from "@/lib/backend/types";
import { useHistoryStore } from "@/stores/history";
import { useRepositoryStore } from "@/stores/repository";
import { useUiStore } from "@/stores/ui";
import { createBackendFixture } from "@/test/backend";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

const repository: RepositorySnapshot = {
  rootPath: "C:/repo",
  name: "repo",
  currentBranch: "main",
  headShortHash: "2222222",
  isClean: true,
  changedFileCount: 0,
  conflictCount: 0,
  remotes: [],
  upstream: null,
};

function commit(hash: string, subject: string): CommitSummary {
  return {
    hash: hash.repeat(40),
    shortHash: hash.repeat(7),
    parentHashes: hash === "2" ? ["1".repeat(40)] : [],
    subject,
    authorName: "HQ Test",
    authorEmail: "hq@example.test",
    authoredAt: "2026-09-08T10:00:00+08:00",
    references: hash === "2" ? ["HEAD -> refs/heads/main"] : [],
    topology: {
      lane: 0,
      parents: hash === "2" ? [{ hash: "1".repeat(40), lane: 0 }] : [],
    },
  };
}

function page(commits: CommitSummary[], nextCursor: string | null): HistoryPage {
  return {
    commits,
    nextCursor,
    queryFingerprint: "history-main",
    continuationLanes: [],
  };
}

function detail(hash: string): CommitDetail {
  return {
    hash: hash.repeat(40),
    shortHash: hash.repeat(7),
    parentHashes: hash === "2" ? ["1".repeat(40)] : [],
    message: hash === "2" ? "newest\n\nbody" : "parent",
    authorName: "HQ Test",
    authorEmail: "hq@example.test",
    authoredAt: "2026-09-08T10:00:00+08:00",
    committerName: "HQ Test",
    committerEmail: "hq@example.test",
    committedAt: "2026-09-08T10:01:00+08:00",
    references: [],
    files: [
      {
        status: "M",
        path: "src/history.ts",
        oldPath: null,
        additions: 3,
        deletions: 1,
      },
    ],
  };
}

describe("history workbench", () => {
  let backend: BackendClient;
  let pinia: ReturnType<typeof createPinia>;

  beforeEach(() => {
    pinia = createPinia();
    setActivePinia(pinia);
    backend = createBackendFixture({
      historyPage: vi.fn(async () => page([commit("2", "newest")], null)),
      historyDetail: vi.fn(async (_path, hash) =>
        detail(hash.startsWith("2") ? "2" : "1"),
      ),
      historyFileDiff: vi.fn(async () => ({
        path: "src/history.ts",
        scope: "commit" as const,
        binary: true,
        hunks: [],
      })),
    });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = repository;
    useUiStore().activeView = "history";
  });

  function mountHistory() {
    return mount(App, { global: { plugins: [pinia] } });
  }

  it("shows detail loading immediately and replaces it with the loaded files", async () => {
    const pending = deferred<CommitDetail>();
    vi.mocked(backend.historyDetail).mockReturnValueOnce(pending.promise);
    const wrapper = mountHistory();
    await flushPromises();
    const row = wrapper.get('[data-commit-hash="' + "2".repeat(40) + '"]');
    await row.trigger("click");
    await row.trigger("click");
    expect(wrapper.text()).toContain("正在读取提交详情");
    expect(backend.historyDetail).toHaveBeenCalledTimes(1);
    pending.resolve(detail("2"));
    await flushPromises();
    expect(wrapper.text()).not.toContain("正在读取提交详情");
    expect(wrapper.find('[aria-label="查看提交文件 src/history.ts"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("offers a retry after detail loading fails", async () => {
    vi.mocked(backend.historyDetail).mockRejectedValueOnce({ code: "gitCommandFailed", message: "读取失败" });
    const wrapper = mountHistory();
    await flushPromises();
    await wrapper.get('[data-commit-hash="' + "2".repeat(40) + '"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('.history-detail [role="alert"]').text()).toContain("读取失败");
    await wrapper.get('.history-detail [role="alert"] button').trigger("click");
    await flushPromises();
    expect(wrapper.find('[aria-label="查看提交文件 src/history.ts"]').exists()).toBe(true);
    wrapper.unmount();
  });

  it("shows a searchable commit list without view switching", async () => {
    const wrapper = mountHistory();
    await flushPromises();

    await wrapper.get('input[aria-label="搜索提交历史"]').setValue("parser");

    expect(useHistoryStore().query.search).toBe("parser");
    expect(wrapper.find('[aria-label="历史显示方式"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="graph-lanes"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it("loads metadata, navigates to a parent, and shows binary file output", async () => {
    const wrapper = mountHistory();
    await flushPromises();

    await wrapper.get('[data-commit-hash="' + "2".repeat(40) + '"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("newest");
    expect(wrapper.text()).toContain("src/history.ts");

    await wrapper.get('[aria-label="查看父提交 1111111"]').trigger("click");
    await flushPromises();
    expect(backend.historyDetail).toHaveBeenLastCalledWith(
      "C:/repo",
      "1".repeat(40),
    );

    await wrapper.get('[data-commit-hash="' + "2".repeat(40) + '"]').trigger("click");
    await flushPromises();
    await wrapper.get('[aria-label="查看提交文件 src/history.ts"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("二进制文件无法显示文本差异");
  });

  it("exposes checkout cherry-pick and guarded reset controls", async () => {
    const wrapper = mountHistory();
    await flushPromises();
    await wrapper
      .get('[data-commit-hash="' + "2".repeat(40) + '"]')
      .trigger("click");
    await flushPromises();

    expect(wrapper.find('[aria-label="Checkout 提交 2222222"]').exists()).toBe(true);
    expect(wrapper.find('[aria-label="Cherry-pick 提交 2222222"]').exists()).toBe(true);
    expect(wrapper.find('[aria-label="Revert 提交 2222222"]').exists()).toBe(true);
    await wrapper.get('[aria-label="Reset 到提交 2222222"]').trigger("click");
    expect(wrapper.text()).toContain("软重置");
    expect(wrapper.text()).toContain("混合重置");
    expect(wrapper.text()).toContain("硬重置");
    await wrapper.get('[aria-label="硬重置"]').setValue(true);
    const confirm = wrapper.get('[aria-label="执行重置"]');
    expect(confirm.attributes()).toHaveProperty("disabled");
    await wrapper.get('[aria-label="输入短哈希确认"]').setValue("2222222");
    expect(confirm.attributes()).not.toHaveProperty("disabled");
  });

  it("only reverts after confirmation and refreshes history after success", async () => {
    vi.mocked(backend.historyRevert).mockResolvedValue({
      workspace: { repository: { ...repository, headShortHash: "3333333" }, changes: { files: [], stagedCount: 0, unstagedCount: 0 } },
      operationState: { kind: "none", conflicts: [], abortAction: null },
      history: page([commit("3", 'Revert "newest"'), commit("2", "newest")], null),
    });
    const wrapper = mountHistory();
    await flushPromises();
    await wrapper.get('[data-commit-hash="' + "2".repeat(40) + '"]').trigger("click");
    await flushPromises();
    await wrapper.get('[aria-label="Revert 提交 2222222"]').trigger("click");
    expect(backend.historyRevert).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("保留原有历史");
    await wrapper.get('[aria-label="取消"]').trigger("click");
    expect(backend.historyRevert).not.toHaveBeenCalled();
    await wrapper.get('[aria-label="Revert 提交 2222222"]').trigger("click");
    await wrapper.get('[aria-label="确认回滚提交"]').trigger("click");
    await flushPromises();
    expect(backend.historyRevert).toHaveBeenCalledWith("C:/repo", { commit: "2".repeat(40), mainline: null });
    expect(useHistoryStore().commits[0]?.subject).toBe('Revert "newest"');
    wrapper.unmount();
  });

  it("requires an explicit merge mainline and keeps the revert dialog on failure", async () => {
    vi.mocked(backend.historyDetail).mockResolvedValue({ ...detail("2"), parentHashes: ["1".repeat(40), "3".repeat(40)] });
    vi.mocked(backend.historyRevert).mockRejectedValue({ code: "dirtyWorktree", message: "请先提交或贮藏工作区修改" });
    const wrapper = mountHistory();
    await flushPromises();
    await wrapper.get('[data-commit-hash="' + "2".repeat(40) + '"]').trigger("click");
    await flushPromises();
    await wrapper.get('[aria-label="Revert 提交 2222222"]').trigger("click");
    const confirm = wrapper.get('[aria-label="确认回滚提交"]');
    expect(confirm.attributes()).toHaveProperty("disabled");
    await selectOption(wrapper, "Revert 主线父提交", 2);
    await confirm.trigger("click");
    await flushPromises();
    expect(backend.historyRevert).toHaveBeenCalledWith("C:/repo", { commit: "2".repeat(40), mainline: 2 });
    expect(wrapper.find('[aria-label="确认回滚提交"]').exists()).toBe(true);
    expect(wrapper.text()).toContain("请先提交或贮藏工作区修改");
    wrapper.unmount();
  });

  it("lazily expands multiple split diffs and preserves cache when collapsed", async () => {
    const twoFiles = detail("2");
    twoFiles.files.push({ status: "A", path: "docs/notes.md", oldPath: null, additions: 1, deletions: 0 });
    vi.mocked(backend.historyDetail).mockResolvedValue(twoFiles);
    vi.mocked(backend.historyFileDiff).mockImplementation(async (_root, _hash, path) => ({ path, scope: "commit", binary: false, hunks: [{ index: 0, header: "@@ -1 +1 @@", lines: [
      { kind: "deletion", content: "old text", oldLine: 1, newLine: null },
      { kind: "addition", content: "new text", oldLine: null, newLine: 1 },
    ] }] }));
    const wrapper = mountHistory();
    await flushPromises();
    await wrapper.get('[data-commit-hash="' + "2".repeat(40) + '"]').trigger("click");
    await flushPromises();
    expect(backend.historyFileDiff).not.toHaveBeenCalled();
    const first = wrapper.get('[aria-label="查看提交文件 src/history.ts"]');
    await first.trigger("click");
    await wrapper.get('[aria-label="查看提交文件 docs/notes.md"]').trigger("click");
    await flushPromises();
    expect(wrapper.findAll('.split-diff')).toHaveLength(2);
    expect(first.attributes("aria-expanded")).toBe("true");
    await first.trigger("click");
    expect(wrapper.findAll('.split-diff')).toHaveLength(1);
    await first.trigger("click");
    await flushPromises();
    expect(backend.historyFileDiff).toHaveBeenCalledTimes(2);
    // AI navigation can reveal the same file even after it has been collapsed.
    await first.trigger("click");
    await useHistoryStore().selectFile("src/history.ts");
    await flushPromises();
    expect(first.attributes("aria-expanded")).toBe("true");
    wrapper.unmount();
  });

  it("shows file loading and retry inline and does not reopen a collapsed pending file", async () => {
    let reject!: (error: unknown) => void;
    vi.mocked(backend.historyFileDiff).mockReturnValueOnce(new Promise((_resolve, fail) => { reject = fail; }));
    const wrapper = mountHistory();
    await flushPromises();
    await wrapper.get('[data-commit-hash="' + "2".repeat(40) + '"]').trigger("click");
    await flushPromises();
    const button = wrapper.get('[aria-label="查看提交文件 src/history.ts"]');
    await button.trigger("click");
    expect(wrapper.text()).toContain("正在读取文件差异");
    reject({ code: "gitCommandFailed", message: "文件读取失败" });
    await flushPromises();
    expect(wrapper.get('.history-diff-files [role="alert"]').text()).toContain("文件读取失败");
    await wrapper.get('[aria-label="重试文件 src/history.ts"]').trigger("click");
    await button.trigger("click");
    await flushPromises();
    expect(button.attributes("aria-expanded")).toBe("false");
    expect(wrapper.find('.split-diff').exists()).toBe(false);
    wrapper.unmount();
  });

  it("offers a local Cherry-pick target and return-after-success choice", async () => {
    vi.mocked(backend.refsSnapshot).mockResolvedValue({
      localBranches: [
        {
          name: "release",
          fullName: "refs/heads/release",
          kind: "local",
          current: false,
          tip: {
            fullHash: "1".repeat(40),
            shortHash: "1111111",
            subject: "release branch",
            author: "HQ Test",
            authoredAt: "2026-09-08T10:00:00+08:00",
          },
        },
      ],
      remoteBranches: [],
      tags: [],
    });
    const wrapper = mountHistory();
    await flushPromises();
    await wrapper
      .get('[data-commit-hash="' + "2".repeat(40) + '"]')
      .trigger("click");
    await flushPromises();

    await wrapper
      .get('[aria-label="Cherry-pick 提交 2222222"]')
      .trigger("click");
    const target = wrapper.get('[aria-label="Cherry-pick 目标分支"]');
    expect(target.text()).toContain("当前分支");
    expect(wrapper.find('input[type="checkbox"]').exists()).toBe(false);

    await selectOption(wrapper, "Cherry-pick 目标分支", "release");
    expect(target.text()).toBe("release");
    const returnChoice = wrapper.get('input[type="checkbox"]');
    expect(returnChoice.element).toHaveProperty("checked", true);
  });

  it("loads the next page when the intersection sentinel becomes visible", async () => {
    let callback: IntersectionObserverCallback | undefined;
    class Observer {
      constructor(next: IntersectionObserverCallback) {
        callback = next;
      }
      observe() {}
      disconnect() {}
      unobserve() {}
      takeRecords(): IntersectionObserverEntry[] {
        return [];
      }
      readonly root = null;
      readonly rootMargin = "0px";
      readonly thresholds = [0];
    }
    vi.stubGlobal("IntersectionObserver", Observer);
    vi.mocked(backend.historyPage)
      .mockResolvedValueOnce(page([commit("2", "newest")], "next"))
      .mockResolvedValueOnce(page([commit("1", "parent")], null));
    const wrapper = mountHistory();
    await flushPromises();

    callback?.(
      [{ isIntersecting: true } as IntersectionObserverEntry],
      {} as IntersectionObserver,
    );
    await flushPromises();

    expect(wrapper.findAll("[data-commit-hash]")).toHaveLength(2);
    vi.unstubAllGlobals();
  });

  it("renders loading, empty, no-match, and partial-error states", async () => {
    const pending = deferred<HistoryPage>();
    vi.mocked(backend.historyPage).mockReturnValueOnce(pending.promise);
    const wrapper = mountHistory();
    await flushPromises();
    expect(wrapper.text()).toContain("正在读取提交");

    pending.resolve(page([], null));
    await flushPromises();
    expect(wrapper.text()).toContain("仓库还没有提交");

    const history = useHistoryStore();
    history.query.search = "missing";
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain("没有匹配的提交");

    history.commits = [commit("2", "visible result")];
    history.error = { code: "gitCommandFailed", message: "后续页面读取失败。" };
    await wrapper.vm.$nextTick();
    expect(wrapper.get('[role="alert"]').text()).toContain("后续页面读取失败");
    expect(wrapper.findAll("[data-commit-hash]")).toHaveLength(1);
  });
});
