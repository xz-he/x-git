import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, expect, it, vi } from "vitest";

import App from "@/App.vue";
import {
  setBackendClientForTests,
  type BackendClient,
} from "@/lib/backend/client";
import { useOperationStore } from "@/stores/operation";
import { useRepositoryStore } from "@/stores/repository";
import { useUiStore } from "@/stores/ui";
import { createBackendFixture } from "@/test/backend";

it("persists across modules and exposes only the matching abort", async () => {
  const pinia = createPinia();
  setActivePinia(pinia);
  const repository = {
    rootPath: "C:/repo",
    name: "repo",
    currentBranch: "main",
    headShortHash: "aaaaaaa",
    isClean: false,
    changedFileCount: 1,
    conflictCount: 1,
    remotes: [],
    upstream: null,
  };
  const backend: BackendClient = createBackendFixture();
  vi.mocked(backend.refsAbort).mockResolvedValue({
    workspace: {
      repository: { ...repository, isClean: true, changedFileCount: 0, conflictCount: 0 },
      changes: { files: [], stagedCount: 0, unstagedCount: 0 },
    },
    refs: { localBranches: [], remoteBranches: [], tags: [] },
    operationState: { kind: "none", conflicts: [], abortAction: null },
  });
  setBackendClientForTests(backend);
  useRepositoryStore().snapshot = repository;
  useOperationStore().state = {
    kind: "merge",
    conflicts: [{ path: "README.md", status: "UU" }],
    abortAction: "merge",
  };
  vi.mocked(backend.conflictsSnapshot).mockImplementation(async () => ({ operationState: useOperationStore().state, operationToken: "token", files: [], continueAction: null, stagedFiles: [] }));
  const wrapper = mount(App, { global: { plugins: [pinia] } });

  expect(wrapper.text()).toContain("合并冲突：1 个文件");
  expect(wrapper.find('[aria-label="中止合并"]').exists()).toBe(true);
  expect(wrapper.find('[aria-label="中止变基"]').exists()).toBe(false);
  useUiStore().openView("history");
  await flushPromises();
  expect(wrapper.text()).toContain("合并冲突：1 个文件");

  await wrapper.get('[aria-label="中止合并"]').trigger("click");
  await flushPromises();
  expect(backend.refsAbort).not.toHaveBeenCalled();
  expect(wrapper.get('[role="alertdialog"]').text()).toContain("草稿");
  await wrapper.get('[aria-label="确认中止"]').trigger("click");
  await flushPromises();
  expect(backend.refsAbort).toHaveBeenCalledWith("C:/repo", "merge");
  expect(wrapper.text()).not.toContain("合并冲突：1 个文件");
});
