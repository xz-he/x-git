import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "@/App.vue";
import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import type { RefsMutationResult, RefsSnapshot, RepositorySnapshot } from "@/lib/backend/types";
import { useHistoryStore } from "@/stores/history";
import { useRepositoryStore } from "@/stores/repository";
import { useUiStore } from "@/stores/ui";
import { createBackendFixture } from "@/test/backend";

const repository: RepositorySnapshot = {
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

function refsFixture(): RefsSnapshot {
  const tip = {
    fullHash: "a".repeat(40),
    shortHash: "aaaaaaa",
    subject: "add history parser",
    author: "HQ Test",
    authoredAt: "2026-09-08T10:00:00+08:00",
  };
  return {
    localBranches: [
      {
        name: "feature/a-very-long-branch-name-that-must-truncate",
        fullName: "refs/heads/feature/a-very-long-branch-name-that-must-truncate",
        kind: "local",
        current: false,
        upstream: "origin/feature/long",
        ahead: 2,
        behind: 1,
        tip,
      },
    ],
    remoteBranches: [
      {
        name: "origin/main",
        fullName: "refs/remotes/origin/main",
        kind: "remote",
        current: false,
        tip,
      },
    ],
    tags: [
      {
        name: "v4.0.0",
        objectHash: "b".repeat(40),
        peeledCommitHash: "a".repeat(40),
        annotated: true,
        tagger: "Release Bot",
        taggedAt: "2026-09-08T11:00:00+08:00",
        annotation: "stable release",
        commitSubject: "add history parser",
      },
    ],
  };
}

describe("refs workbench", () => {
  let backend: BackendClient;
  let pinia: ReturnType<typeof createPinia>;

  beforeEach(() => {
    pinia = createPinia();
    setActivePinia(pinia);
    backend = createBackendFixture({
      refsSnapshot: vi.fn(async () => refsFixture()),
    });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = repository;
  });

  function mountView(view: "branches" | "tags") {
    useUiStore().activeView = view;
    return mount(App, { global: { plugins: [pinia] } });
  }

  it("groups local and remote branches and keeps long names inside the row", async () => {
    const wrapper = mountView("branches");
    await flushPromises();

    expect(wrapper.text()).toContain("本地分支");
    expect(wrapper.text()).toContain("远程分支");
    expect(wrapper.get('[data-testid="ref-name"]').attributes("title")).toContain(
      "feature/",
    );
    expect(wrapper.find('[data-testid="ref-actions"]').exists()).toBe(true);
  });

  it("shows annotated tag fields and navigates the tag into history", async () => {
    const wrapper = mountView("tags");
    await flushPromises();

    expect(wrapper.text()).toContain("stable release");
    expect(wrapper.text()).toContain("Release Bot");
    await wrapper.get('[aria-label="在提交记录中查看 v4.0.0"]').trigger("click");
    await flushPromises();

    expect(useUiStore().activeView).toBe("history");
    expect(useHistoryStore().query.reference).toBe("refs/tags/v4.0.0");
    expect(backend.historyPage).toHaveBeenCalledTimes(1);
  });

  it("shows safe local actions and requires the exact name for force delete", async () => {
    const wrapper = mountView("branches");
    await flushPromises();

    expect(wrapper.find('[aria-label^="切换到分支"]').exists()).toBe(true);
    expect(wrapper.find('[aria-label^="合并分支"]').exists()).toBe(true);
    expect(wrapper.find('[aria-label^="变基到分支"]').exists()).toBe(true);
    await wrapper.get('[aria-label^="删除分支"]').trigger("click");
    await wrapper.get('[aria-label="强制删除"]').setValue(true);
    const confirm = wrapper.get('[aria-label="删除分支"]');
    expect(confirm.attributes()).toHaveProperty("disabled");
    await wrapper.get('[aria-label="输入分支名称确认"]').setValue(
      "feature/a-very-long-branch-name-that-must-truncate",
    );
    expect(confirm.attributes()).not.toHaveProperty("disabled");
  });

  it("does not expose local mutations for a remote branch", async () => {
    const wrapper = mountView("branches");
    await flushPromises();
    await wrapper.get('[aria-label="查看分支 origin/main"]').trigger("click");
    await wrapper.get('[aria-label="查看分支 origin/main"]').trigger("dblclick");
    expect(backend.refsSwitch).not.toHaveBeenCalled();

    expect(wrapper.find('[aria-label^="切换到分支"]').exists()).toBe(false);
    expect(wrapper.find('[aria-label^="删除分支"]').exists()).toBe(false);
  });

  it("does not expose switch merge rebase or delete for the current branch", async () => {
    const fixture = refsFixture();
    const branch = fixture.localBranches[0]!;
    vi.mocked(backend.refsSnapshot).mockResolvedValue({
      ...fixture,
      localBranches: [
        {
          ...branch,
          name: "main",
          fullName: "refs/heads/main",
          current: true,
        },
        ...fixture.localBranches,
      ],
    });
    const wrapper = mountView("branches");
    await flushPromises();
    await wrapper.get('[aria-label="查看分支 main"]').trigger("dblclick");
    expect(backend.refsSwitch).not.toHaveBeenCalled();

    expect(wrapper.find('[aria-label^="创建分支"]').exists()).toBe(true);
    expect(wrapper.find('[aria-label^="切换到分支"]').exists()).toBe(false);
    expect(wrapper.find('[aria-label^="合并分支"]').exists()).toBe(false);
    expect(wrapper.find('[aria-label^="变基到分支"]').exists()).toBe(false);
    expect(wrapper.find('[aria-label^="删除分支"]').exists()).toBe(false);
  });

  it("keeps branch input available after a backend failure", async () => {
    vi.mocked(backend.refsCreate).mockRejectedValue({
      code: "invalidBranchName",
      message: "分支名称无效。",
    });
    const wrapper = mountView("branches");
    await flushPromises();

    await wrapper.get('[aria-label^="创建分支"]').trigger("click");
    await wrapper.get('[aria-label="新分支名称"]').setValue("feature/retry-me");
    await wrapper.get('[aria-label="创建分支"]').trigger("click");
    await flushPromises();

    expect(backend.refsCreate).toHaveBeenCalledWith("C:/repo", {
      name: "feature/retry-me",
      startPoint:
        "refs/heads/feature/a-very-long-branch-name-that-must-truncate",
      switch: true,
    });
    expect(wrapper.get('[aria-label="新分支名称"]').element).toHaveProperty(
      "value",
      "feature/retry-me",
    );
  });

  it("selects on click, switches on double-click and prevents duplicate switches", async () => {
    const fixture = refsFixture();
    const branch = fixture.localBranches[0]!;
    let resolve!: (result: RefsMutationResult) => void;
    vi.mocked(backend.refsSwitch).mockImplementation(() => new Promise(next => { resolve = next; }));
    const wrapper = mountView("branches");
    await flushPromises();
    const name = wrapper.get('[data-testid="ref-name"]');
    await name.trigger("click");
    expect(backend.refsSwitch).not.toHaveBeenCalled();
    await name.trigger("dblclick");
    await name.trigger("dblclick");
    expect(backend.refsSwitch).toHaveBeenCalledExactlyOnceWith("C:/repo", branch.name);
    resolve({
      workspace: { repository: { ...repository, currentBranch: branch.name }, changes: { files: [], stagedCount: 0, unstagedCount: 0 } },
      operationState: { kind: "none", conflicts: [], abortAction: null },
      refs: { ...fixture, localBranches: [{ ...branch, current: true }] },
    });
    await flushPromises();
    expect(useRepositoryStore().snapshot?.currentBranch).toBe(branch.name);
    expect(wrapper.get('.ref-row').text()).toContain("当前");
    await wrapper.get('[data-testid="ref-name"]').trigger("dblclick");
    expect(backend.refsSwitch).toHaveBeenCalledTimes(1);
    wrapper.unmount();
  });

  it("blocks double-click while Git is busy and shows protected local-change errors", async () => {
    const wrapper = mountView("branches");
    await flushPromises();
    useRepositoryStore().operation = { kind: "refresh" };
    await wrapper.get('[data-testid="ref-name"]').trigger("dblclick");
    expect(backend.refsSwitch).not.toHaveBeenCalled();
    useRepositoryStore().operation = { kind: "idle" };
    vi.mocked(backend.refsSwitch).mockRejectedValue({ code: "dirtyWorktree", message: "请先提交或贮藏会被覆盖的修改。" });
    await wrapper.get('[data-testid="ref-name"]').trigger("dblclick");
    await flushPromises();
    expect(wrapper.text()).toContain("请先提交或贮藏会被覆盖的修改。");
    expect(useRepositoryStore().snapshot?.currentBranch).toBe("main");
    wrapper.unmount();
  });
});
