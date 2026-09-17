import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "@/App.vue";
import DiffViewer from "./DiffViewer.vue";
import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import type {
  ChangesSnapshot,
  FileDiff,
  MutationWorkspace,
  RepositorySnapshot,
} from "@/lib/backend/types";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { createBackendFixture } from "@/test/backend";

enableAutoUnmount(afterEach);

const repository: RepositorySnapshot = {
  rootPath: "D:\\work\\hq-git",
  name: "hq-git",
  currentBranch: "main",
  headShortHash: "abc1234",
  isClean: false,
  changedFileCount: 2,
  conflictCount: 0,
  remotes: [],
  upstream: null,
};

const changes: ChangesSnapshot = {
  stagedCount: 1,
  unstagedCount: 1,
  files: [
    {
      path: "package-lock.json",
      oldPath: null,
      indexStatus: "M",
      worktreeStatus: " ",
      staged: true,
      unstaged: false,
      conflict: false,
    },
    {
      path: "src/main.ts",
      oldPath: null,
      indexStatus: " ",
      worktreeStatus: "M",
      staged: false,
      unstaged: true,
      conflict: false,
    },
  ],
};

const diff: FileDiff = {
  path: "src/main.ts",
  scope: "unstaged",
  binary: false,
  hunks: [
    {
      index: 0,
      header: "@@ -1,2 +1,2 @@",
      lines: [
        {
          kind: "deletion",
          oldLine: 1,
          newLine: null,
          content: "const oldValue = 1;",
        },
        {
          kind: "addition",
          oldLine: null,
          newLine: 1,
          content: "const newValue = 1;",
        },
        {
          kind: "context",
          oldLine: 2,
          newLine: 2,
          content: "export default newValue;",
        },
      ],
    },
  ],
};

const stagedDiff: FileDiff = {
  path: "package-lock.json",
  scope: "staged",
  binary: false,
  hunks: [
    {
      index: 0,
      header: "@@ -8 +8 @@",
      lines: [
        {
          kind: "deletion",
          oldLine: 8,
          newLine: null,
          content: '"version": "3.0.0"',
        },
        {
          kind: "addition",
          oldLine: null,
          newLine: 8,
          content: '"version": "4.0.0"',
        },
      ],
    },
  ],
};

function workspace(nextChanges: ChangesSnapshot = changes): MutationWorkspace {
  return {
    workspace: {
      repository: {
        ...repository,
        changedFileCount: nextChanges.files.length,
        isClean: nextChanges.files.length === 0,
      },
      changes: nextChanges,
    },
    operationState: {
      kind: "none",
      conflicts: [],
      abortAction: null,
    },
  };
}

describe("changes workbench", () => {
  let backend: BackendClient;
  let pinia: ReturnType<typeof createPinia>;

  beforeEach(() => {
    document.body.innerHTML = "";
    pinia = createPinia();
    setActivePinia(pinia);
    backend = createBackendFixture();
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = repository;
    useChangesStore().snapshot = structuredClone(changes);
  });

  function mountWorkbench() {
    return mount(App, {
      attachTo: document.body,
      global: { plugins: [pinia] },
    });
  }

  it("renders grouped changes and stages a changed file", async () => {
    const stagedChanges: ChangesSnapshot = {
      stagedCount: 2,
      unstagedCount: 0,
      files: changes.files.map((file) => ({
        ...file,
        indexStatus: "M",
        worktreeStatus: " ",
        staged: true,
        unstaged: false,
      })),
    };
    vi.mocked(backend.changesStageFile).mockResolvedValue(
      workspace(stagedChanges),
    );
    const wrapper = mountWorkbench();

    expect(wrapper.text()).toContain("已暂存 1");
    expect(wrapper.text()).toContain("未暂存 1");
    await wrapper.get('[aria-label="暂存 src/main.ts"]').trigger("click");
    await flushPromises();

    expect(backend.changesStageFile).toHaveBeenCalledWith(
      repository.rootPath,
      "src/main.ts",
    );
    expect(wrapper.text()).toContain("已暂存 2");
    expect(wrapper.text()).toContain("未暂存 0");
  });

  it("loads a diff and exposes hunk and selected-line actions", async () => {
    vi.mocked(backend.changesFileDiff).mockResolvedValue(diff);
    vi.mocked(backend.changesStageHunk).mockResolvedValue(workspace());
    vi.mocked(backend.changesStageLines).mockResolvedValue(workspace());
    const wrapper = mountWorkbench();

    await wrapper
      .get('[data-change-key="unstaged:src/main.ts"]')
      .trigger("click");
    await flushPromises();

    expect(backend.changesFileDiff).toHaveBeenCalledWith(
      repository.rootPath,
      "src/main.ts",
      "unstaged",
    );
    expect(wrapper.text()).toContain("@@ -1,2 +1,2 @@");
    const before = wrapper.get('[role="region"][aria-label="修改前"]');
    const after = wrapper.get('[role="region"][aria-label="修改后"]');
    expect(before.text()).toContain("const oldValue = 1;");
    expect(before.text()).not.toContain("const newValue = 1;");
    expect(after.text()).toContain("const newValue = 1;");
    expect(after.text()).not.toContain("const oldValue = 1;");
    await wrapper.get('[aria-label="暂存块 1"]').trigger("click");
    await flushPromises();
    expect(backend.changesStageHunk).toHaveBeenCalledWith(
      repository.rootPath,
      "src/main.ts",
      0,
    );

    await wrapper
      .get('[aria-label="选择变更行 1：const newValue = 1;"]')
      .setValue(true);
    await wrapper.get('[aria-label="暂存所选行"]').trigger("click");
    expect(backend.changesStageLines).toHaveBeenCalledWith(
      repository.rootPath,
      "src/main.ts",
      1,
      1,
    );
  });

  it("keeps virtual scroll in place while selecting and deselecting changed lines", async () => {
    useChangesStore().selectedDiff = { ...diff, hunks: [{ index: 0, header: "@@ -0,0 +1,1000 @@", lines: Array.from({ length: 1000 }, (_, i) => ({ kind: "addition", content: `added ${i + 1}`, oldLine: null, newLine: i + 1 })) }] };
    const wrapper = mount(DiffViewer, { global: { plugins: [pinia] } });
    const right = wrapper.get('[role="region"][aria-label="修改后"]');
    Object.assign(right.element, { scrollTop: 14000, scrollLeft: 120 });
    await right.trigger("scroll");
    const checkbox = wrapper.get('[aria-label="选择变更行 501：added 501"]');
    await checkbox.trigger("pointerdown");
    await checkbox.setValue(true);
    await flushPromises();
    expect(right.element.scrollTop).toBe(14000);
    expect(right.element.scrollLeft).toBe(120);
    expect(wrapper.findAll('.diff-cell').length).toBeLessThan(160);
    await checkbox.trigger("pointerdown");
    await checkbox.setValue(false);
    expect(wrapper.find('[aria-label="暂存所选行"]').exists()).toBe(false);
    wrapper.unmount();
  });

  it("reveals a virtualized review addition far below the first viewport", async () => {
    useChangesStore().selectedDiff = { ...stagedDiff, hunks: [{ index: 0, header: "@@ -0,0 +1,1000 @@", lines: Array.from({ length: 1000 }, (_, i) => ({ kind: "addition", content: `added ${i + 1}`, oldLine: null, newLine: i + 1 })) }] };
    const wrapper = mount(DiffViewer, { global: { plugins: [pinia] } });
    expect(wrapper.find('[data-diff-line="900"]').exists()).toBe(false);
    useChangesStore().highlightedLine = 900;
    await flushPromises();
    expect(wrapper.get('[data-diff-line="900"].review-highlight').text()).toContain("added 900");
    const panes = wrapper.findAll('.diff-pane');
    expect(panes[0]!.element.scrollTop).toBeGreaterThan(24000);
    expect(panes[0]!.element.scrollTop).toBe(panes[1]!.element.scrollTop);
    wrapper.unmount();
  });

  it("unstages a deleted line selected from the left pane", async () => {
    useChangesStore().selectedDiff = stagedDiff;
    vi.mocked(backend.changesUnstageLines).mockResolvedValue(workspace());
    vi.mocked(backend.changesFileDiff).mockResolvedValue(stagedDiff);
    const wrapper = mountWorkbench();
    await wrapper.get('[role="region"][aria-label="修改前"] input').setValue(true);
    await wrapper.get('[aria-label="取消暂存所选行"]').trigger("click");
    await flushPromises();
    expect(backend.changesUnstageLines).toHaveBeenCalledWith(repository.rootPath, "package-lock.json", 8, 8);
    wrapper.unmount();
  });

  it("renders commit diffs without workspace mutation controls", () => {
    useChangesStore().selectedDiff = { ...diff, scope: "commit" };

    const wrapper = mountWorkbench();

    expect(wrapper.text()).toContain("@@ -1,2 +1,2 @@");
    expect(wrapper.find('[aria-label="暂存块 1"]').exists()).toBe(false);
    expect(
      wrapper.find('[aria-label="选择变更行 1：const newValue = 1;"]').exists(),
    ).toBe(false);
  });

  it("reveals a staged review line and clears it on unrelated selection", async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;
    vi.mocked(backend.changesFileDiff)
      .mockResolvedValueOnce(stagedDiff)
      .mockResolvedValueOnce(diff);
    const wrapper = mountWorkbench();

    await useChangesStore().revealStagedLine("package-lock.json", 8);
    await flushPromises();

    expect(backend.changesFileDiff).toHaveBeenCalledWith(
      repository.rootPath,
      "package-lock.json",
      "staged",
    );
    expect(
      wrapper.get('[data-diff-line="8"].review-highlight').classes(),
    ).toContain("addition");
    expect(
      wrapper.findAll('[data-diff-line="8"].review-highlight'),
    ).toHaveLength(1);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "center" });

    await wrapper.get('[data-change-key="unstaged:src/main.ts"]').trigger("click");
    await flushPromises();
    expect(useChangesStore().highlightedLine).toBeUndefined();
  });

  it("requires exact-path confirmation before discarding", async () => {
    vi.mocked(backend.changesDiscardFile).mockResolvedValue(
      workspace({
        stagedCount: 1,
        unstagedCount: 0,
        files: [changes.files[0]!],
      }),
    );
    const wrapper = mountWorkbench();

    await wrapper.get('[aria-label="丢弃 src/main.ts"]').trigger("click");
    await flushPromises();
    const dialog = wrapper.get('[role="alertdialog"]');
    expect(dialog.text()).toContain("hq-git");
    expect(dialog.text()).toContain("src/main.ts");
    expect(document.activeElement?.getAttribute("aria-label")).toBe(
      "取消丢弃",
    );
    await dialog.get('[aria-label="取消丢弃"]').trigger("click");
    expect(backend.changesDiscardFile).not.toHaveBeenCalled();

    await wrapper.get('[aria-label="丢弃 src/main.ts"]').trigger("click");
    await wrapper.get('[aria-label="确认丢弃 src/main.ts"]').trigger("click");
    await flushPromises();
    expect(backend.changesDiscardFile).toHaveBeenCalledWith(
      repository.rootPath,
      "src/main.ts",
    );
  });

  it("guards commit input and reports the new hash", async () => {
    const cleanWorkspace = workspace({
      stagedCount: 0,
      unstagedCount: 0,
      files: [],
    });
    vi.mocked(backend.changesCommit).mockResolvedValue({
      shortHash: "def5678",
      subject: "feat: save changes",
      workspace: cleanWorkspace,
    });
    const wrapper = mountWorkbench();
    const submit = wrapper.get('[aria-label="创建提交"]');

    expect(submit.attributes()).toHaveProperty("disabled");
    await wrapper
      .get('[aria-label="提交说明"]')
      .setValue("feat: save changes");
    expect(submit.attributes()).not.toHaveProperty("disabled");
    await submit.trigger("click");
    await flushPromises();

    expect(backend.changesCommit).toHaveBeenCalledWith(
      repository.rootPath,
      "feat: save changes",
    );
    expect(wrapper.text()).toContain("def5678");
    expect(wrapper.get('[aria-label="提交说明"]').element).toHaveProperty(
      "value",
      "",
    );
  });

  it("shows structured operation errors without losing the workbench", async () => {
    vi.mocked(backend.changesStageFile).mockRejectedValue({
      code: "gitLocked",
      message: "Git 正在执行其他操作。",
    });
    const wrapper = mountWorkbench();

    await wrapper.get('[aria-label="暂存 src/main.ts"]').trigger("click");
    await flushPromises();

    expect(wrapper.get('[role="alert"]').text()).toContain(
      "Git 正在执行其他操作。",
    );
    expect(wrapper.text()).toContain("src/main.ts");
  });
});
