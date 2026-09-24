import { DOMWrapper, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import HistoryList from "./HistoryList.vue";
import { setBackendClientForTests } from "@/lib/backend/client";
import type { CommitSummary, SquashPreview } from "@/lib/backend/types";
import { createBackendFixture } from "@/test/backend";
import { useHistoryStore } from "@/stores/history";
import { useRepositoryStore } from "@/stores/repository";
import { useOperationStore } from "@/stores/operation";
import { useUiStore } from "@/stores/ui";

const commits: CommitSummary[] = ["3", "2", "1"].map(n => ({
  hash: n.repeat(40), shortHash: n.repeat(7), subject: `commit ${n}`, parentHashes: [],
  authorName: "HQ", authorEmail: "hq@example.test", authoredAt: "2026-09-23", references: [], topology: { lane: 0, parents: [] },
}));
const repository = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "3333333", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null };
const page = { commits, nextCursor: null, queryFingerprint: "main", continuationLanes: [] };
const preview: SquashPreview = { head: "3".repeat(40), branch: "main", commits: [commits[2]!, commits[0]!], rewrittenCount: 3 };
const result = { workspace: { repository, changes: { files: [], stagedCount: 0, unstagedCount: 0 } }, history: { ...page, commits: [commits[1]!] }, operationState: { kind: "none" as const, conflicts: [], abortAction: null }, notice: "backup saved" };

describe("history squash", () => {
  let wrapper: ReturnType<typeof mount>;
  let backend: ReturnType<typeof createBackendFixture>;
  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture({ historySquashPreview: vi.fn(async () => preview), historySquash: vi.fn(async () => result) });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { ...repository };
    useHistoryStore().applyPage(page, repository.rootPath);
    wrapper = mount(HistoryList, { attachTo: document.body });
  });
  afterEach(() => { wrapper.unmount(); document.body.innerHTML = ""; });
  const dialog = () => new DOMWrapper(document.body.querySelector('[role="alertdialog"]')!);
  async function open() {
    await wrapper.get('[aria-label="选择提交 3333333"]').trigger("click");
    await wrapper.get('[aria-label="选择提交 1111111"]').trigger("click");
    await wrapper.get('[aria-label="合并所选提交"]').trigger("click");
    await flushPromises();
  }

  it("squashes only checked nonconsecutive commits after preview, message and explicit confirmation", async () => {
    await open();
    expect(backend.historySquashPreview).toHaveBeenCalledWith("C:/repo", ["3".repeat(40), "1".repeat(40)]);
    expect(backend.historySquash).not.toHaveBeenCalled();
    expect(dialog().get('[aria-label="Squash"]').attributes()).toHaveProperty("disabled");
    await dialog().get('input[type="checkbox"]').setValue(true);
    await dialog().get("textarea").setValue(" ");
    expect(dialog().get('[aria-label="Squash"]').attributes()).toHaveProperty("disabled");
    await dialog().get("textarea").setValue("combined");
    await dialog().get('[aria-label="Squash"]').trigger("click");
    await flushPromises();
    expect(backend.historySquash).toHaveBeenCalledWith("C:/repo", { commits: ["1".repeat(40), "3".repeat(40)], message: "combined", expectedHead: "3".repeat(40), expectedBranch: "main" });
    expect(useHistoryStore().checkedHashes).toEqual([]);
    expect(useHistoryStore().commits).toHaveLength(1);
    expect(useHistoryStore().notice).toBe("backup saved");
    expect(document.body.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it("supports Ctrl and Shift selection and clears it when searching or switching repositories", async () => {
    await wrapper.get('[data-commit-hash="' + "3".repeat(40) + '"]').trigger("click", { ctrlKey: true });
    await wrapper.get('[data-commit-hash="' + "1".repeat(40) + '"]').trigger("click", { shiftKey: true });
    expect(useHistoryStore().checkedHashes).toHaveLength(3);
    useHistoryStore().setSearch("query");
    expect(useHistoryStore().checkedHashes).toEqual([]);
    useHistoryStore().checkedHashes = ["1".repeat(40)];
    useHistoryStore().resetForRepository("C:/other", 2);
    expect(useHistoryStore().checkedHashes).toEqual([]);
  });

  it("keeps failed previews read-only and cancel performs no mutation", async () => {
    vi.mocked(backend.historySquashPreview).mockRejectedValue({ code: "dirtyWorktree", message: "请先贮藏" });
    await open();
    expect(dialog().get('[role="alert"]').text()).toBe("请先贮藏");
    expect(dialog().get('[aria-label="Squash"]').attributes()).toHaveProperty("disabled");
    await dialog().get('[data-action="cancel"]').trigger("click");
    expect(backend.historySquash).not.toHaveBeenCalled();
  });

  it("keeps inputs on stale-head failure and opens the conflict workbench for a paused rebase", async () => {
    vi.mocked(backend.historySquash).mockRejectedValueOnce({ code: "invalidReference", message: "HEAD 已变化" });
    await open();
    await dialog().get('input[type="checkbox"]').setValue(true);
    await dialog().get('[aria-label="Squash"]').trigger("click");
    await flushPromises();
    expect(dialog().text()).toContain("HEAD 已变化");
    vi.mocked(backend.historySquash).mockResolvedValueOnce({ ...result, operationState: { kind: "rebase", conflicts: [{ path: "f", status: "UU" }], abortAction: "rebase" }, error: { code: "gitConflict", message: "conflict; backup saved" } });
    await dialog().get('[aria-label="Squash"]').trigger("click");
    await flushPromises();
    expect(useOperationStore().state.kind).toBe("rebase");
    expect(useUiStore().activeView).toBe("conflicts");
    expect(document.body.querySelector('[role="alertdialog"]')).toBeNull();
  });
});
