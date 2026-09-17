import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "@/App.vue";
import RemoteDetail from "@/features/remotes/RemoteDetail.vue";
import RemoteSyncDialogs from "@/features/remotes/RemoteSyncDialogs.vue";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { RemoteSnapshot, RepositorySnapshot } from "@/lib/backend/types";
import { useRefsStore } from "@/stores/refs";
import { useRemotesStore } from "@/stores/remotes";
import { useRepositoryStore } from "@/stores/repository";
import { useUiStore } from "@/stores/ui";
import { createBackendFixture } from "@/test/backend";

const repository: RepositorySnapshot = {
  rootPath: "C:/repo",
  name: "repo",
  currentBranch: "main",
  headShortHash: "abc1234",
  isClean: true,
  changedFileCount: 0,
  conflictCount: 0,
  remotes: [{ name: "origin", fetchUrl: "C:/fetch.git" }],
  upstream: { name: "origin/main", ahead: 2, behind: 1 },
};

const remotes: RemoteSnapshot = {
  remotes: [
    {
      name: "origin",
      fetchUrl: "C:/fetch repo.git",
      pushUrl: "C:/push repo.git",
      branches: [
        {
          name: "main",
          fullName: "refs/remotes/origin/main",
          objectId: "a".repeat(40),
          trackingLocal: "main",
          ahead: 2,
          behind: 1,
        },
      ],
    },
  ],
};

describe("remote workbench", () => {
  let pinia: ReturnType<typeof createPinia>;
  let backend: BackendClient;

  beforeEach(() => {
    pinia = createPinia();
    setActivePinia(pinia);
    backend = createBackendFixture({
      remotesSnapshot: vi.fn(async () => remotes),
      remoteStartFetch: vi.fn(async (_path, runId) => ({
        runId,
        operation: "fetch" as const,
      })),
    });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = repository;
    useRepositoryStore().generation = 1;
    useUiStore().activeView = "remotes";
  });

  it("shows read-only URLs, tracking, divergence, and direct fetch", async () => {
    const wrapper = mount(App, { global: { plugins: [pinia] } });
    await vi.waitFor(() => expect(wrapper.text()).toContain("C:/fetch repo.git"));

    expect(wrapper.text()).toContain("C:/push repo.git");
    expect(wrapper.text()).toContain("main");
    expect(wrapper.text()).toContain("领先 2");
    expect(wrapper.text()).toContain("落后 1");
    expect(wrapper.find('input[value="C:/fetch repo.git"]').exists()).toBe(false);

    await wrapper.get('[aria-label="获取 origin"]').trigger("click");
    expect(backend.remoteStartFetch).toHaveBeenCalledWith(
      "C:/repo",
      expect.any(String),
      { remote: "origin" },
    );
  });

  it("requires a current remote branch selection for pull", async () => {
    const store = useRemotesStore();
    store.resetForRepository("C:/repo", 1);
    store.applySnapshot(remotes);
    store.selectedRemoteName = "origin";
    store.requestAction("pull");
    const wrapper = mount(RemoteSyncDialogs, {
      attachTo: document.body,
      global: { plugins: [pinia] },
    });

    expect(wrapper.get('[aria-label="确认拉取"]').attributes("disabled")).toBeUndefined();
    store.applySnapshot({ remotes: [] });
    await wrapper.vm.$nextTick();
    expect(wrapper.get('[aria-label="确认拉取"]').attributes("disabled")).toBeDefined();
    wrapper.unmount();
  });

  it("names the force-with-lease target and defaults focus to cancel", async () => {
    const store = useRemotesStore();
    store.resetForRepository("C:/repo", 1);
    store.applySnapshot(remotes);
    store.selectedRemoteName = "origin";
    useRefsStore().applySnapshot(
      {
        localBranches: [
          {
            name: "main",
            fullName: "refs/heads/main",
            kind: "local",
            current: true,
            tip: {
              fullHash: "b".repeat(40),
              shortHash: "bbbbbbb",
              subject: "local",
              author: "HQ",
              authoredAt: "2026-09-10T00:00:00Z",
            },
          },
        ],
        remoteBranches: [],
        tags: [],
      },
      "C:/repo",
    );
    store.requestAction("push");
    const wrapper = mount(RemoteSyncDialogs, {
      attachTo: document.body,
      global: { plugins: [pinia] },
    });

    await wrapper.get('[aria-label="使用 Force With Lease"]').setValue(true);

    expect(wrapper.text()).toContain("origin/main");
    expect(document.activeElement).toBe(
      wrapper.get('[data-action="cancel"]').element,
    );
    wrapper.unmount();
  });

  it("keeps a fixed progress surface and coalesces Stop requests", async () => {
    const store = useRemotesStore();
    store.runId = "run-1";
    store.status = "running";
    store.progress = { phase: "receiving", text: "Receiving objects: 50%" };
    const wrapper = mount(RemoteDetail, { global: { plugins: [pinia] } });

    const progress = wrapper.get('[data-testid="git-progress"]');
    expect(progress.text()).toContain("Receiving objects: 50%");
    await Promise.all([
      progress.get('[aria-label="停止远程同步"]').trigger("click"),
      progress.get('[aria-label="停止远程同步"]').trigger("click"),
    ]);
    expect(backend.gitRunCancel).toHaveBeenCalledTimes(1);
  });

  it("keeps bounded failure diagnostics available for expansion", () => {
    const store = useRemotesStore();
    store.status = "failed";
    store.error = {
      code: "gitNetwork",
      message: "网络失败。",
      diagnostics: "unable to access remote",
    };
    const wrapper = mount(RemoteDetail, { global: { plugins: [pinia] } });

    expect(wrapper.text()).toContain("网络失败。");
    expect(wrapper.get("details").text()).toContain("unable to access remote");
  });
});
