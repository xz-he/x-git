import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/App.vue";
import { createBackendFixture } from "@/test/backend";
import { setBackendClientForTests } from "@/lib/backend/client";
import type { ConflictDetail, ConflictSnapshot } from "@/lib/backend/types";
import { useRepositoryStore } from "@/stores/repository";
import { useConflictsStore } from "@/stores/conflicts";

const version = { exists: true, oid: null, mode: "100644", kind: "text" as const, text: "content\n", byteLength: 8, bom: false, lineEnding: "lf" as const };
const detail: ConflictDetail = { path: "note.txt", token: "token", operationKind: "rebase", base: version, ours: version, theirs: version, working: version, editable: true, canChooseOurs: true, canChooseTheirs: true, canDelete: true, unsupportedReason: null };
const snapshot: ConflictSnapshot = { operationState: { kind: "rebase", conflicts: [{ path: "note.txt", status: "UU" }], abortAction: "rebase" }, operationToken: "op", files: [{ path: "note.txt", status: "UU", supported: true, reason: null }], continueAction: null, stagedFiles: [] };
describe("conflict workbench", () => {
  let pinia: ReturnType<typeof createPinia>;
  beforeEach(() => { pinia = createPinia(); setActivePinia(pinia); });
  async function open() {
    const backend = createBackendFixture({ conflictsSnapshot: vi.fn(async () => snapshot), conflictsDetail: vi.fn(async () => detail) });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abcdef0", upstream: null, isClean: false, conflictCount: 1, changedFileCount: 1, remotes: [] };
    const wrapper = mount(App, { global: { plugins: [pinia], stubs: { ConflictEditor: { props: ["modelValue"], template: '<textarea :value="modelValue" @input="$emit(\'update:modelValue\', $event.target.value)" />' } } } });
    await flushPromises();
    return { wrapper, backend };
  }
  it("opens conflicts, labels rebase sides and adopts a draft before explicit save", async () => {
    const { wrapper, backend } = await open();
    await wrapper.get('[aria-label="打开解决冲突"]').trigger("click"); await flushPromises();
    await wrapper.get('[aria-label="查看冲突 note.txt"]').trigger("click"); await flushPromises();
    expect(wrapper.text()).toContain("正在重放的提交");
    await wrapper.get('[aria-label="采用索引版本 3"]').trigger("click");
    expect(backend.conflictsResolve).not.toHaveBeenCalled();
    await wrapper.get('[aria-label="保存并标记已解决"]').trigger("click");
    expect(wrapper.get('[role="dialog"]').text()).toContain("note.txt");
    expect(backend.conflictsResolve).not.toHaveBeenCalled();
    wrapper.unmount();
  });
  it("requires acknowledgement for remaining conflict markers", async () => {
    const { wrapper } = await open();
    await wrapper.get('[aria-label="打开解决冲突"]').trigger("click"); await flushPromises();
    await wrapper.get('[aria-label="查看冲突 note.txt"]').trigger("click"); await flushPromises();
    useConflictsStore().edit("<<<<<<< ours\ncontent\n=======\nother\n>>>>>>> theirs");
    await flushPromises();
    await wrapper.get('[aria-label="保存并标记已解决"]').trigger("click");
    expect(wrapper.get('[aria-label="确认保存解决结果"]').attributes("disabled")).toBeDefined();
    await wrapper.get('[aria-label="确认保留冲突标记"]').setValue(true);
    expect(wrapper.get('[aria-label="确认保存解决结果"]').attributes("disabled")).toBeUndefined();
    wrapper.unmount();
  });
  it("previews binary whole-side adoption without enabling manual editing", async () => {
    const { wrapper, backend } = await open();
    vi.mocked(backend.conflictsDetail).mockResolvedValue({ ...detail, editable: false, working: { ...version, kind: "binary", text: null }, unsupportedReason: "Only whole-side adoption is supported." });
    await wrapper.get('[aria-label="打开解决冲突"]').trigger("click"); await flushPromises();
    await wrapper.get('[aria-label="查看冲突 note.txt"]').trigger("click"); await flushPromises();
    await wrapper.get('[aria-label="采用索引版本 2"]').trigger("click");
    expect(wrapper.get('[aria-label="保存并标记已解决"]').attributes("disabled")).toBeUndefined();
    await wrapper.get('[aria-label="保存并标记已解决"]').trigger("click");
    expect(wrapper.get('[role="dialog"]').text()).toContain("note.txt"); expect(backend.conflictsResolve).not.toHaveBeenCalled();
    wrapper.unmount();
  });
  it("shows branch, staged paths and expandable diagnostics for Continue failures", async () => {
    const { wrapper, backend } = await open(); const store = useConflictsStore();
    await store.ensureLoaded("C:/repo", 0);
    store.snapshot = { ...snapshot, files: [], continueAction: "rebase", stagedFiles: ["unrelated-staged.txt"], operationState: { kind: "rebase", conflicts: [], abortAction: "rebase" } };
    store.requestContinue(); await flushPromises();
    const dialog = wrapper.get('[role="dialog"]');
    expect(dialog.text()).toContain("main"); expect(dialog.text()).toContain("unrelated-staged.txt");
    vi.mocked(backend.conflictsContinue).mockRejectedValue({ code: "gitCommandFailed", message: "Continue failed", diagnostics: "Signing failed: key unavailable" });
    await dialog.get('[aria-label="确认继续"]').trigger("click"); await flushPromises();
    expect(dialog.get("details").text()).toContain("Signing failed");
    wrapper.unmount();
  });
  it("allows acknowledging marker prefixes without a whitespace suffix", async () => {
    const { wrapper } = await open();
    await wrapper.get('[aria-label="打开解决冲突"]').trigger("click"); await flushPromises();
    await wrapper.get('[aria-label="查看冲突 note.txt"]').trigger("click"); await flushPromises();
    useConflictsStore().edit("=======literal\n"); await flushPromises();
    await wrapper.get('[aria-label="保存并标记已解决"]').trigger("click");
    expect(wrapper.find('[aria-label="确认保留冲突标记"]').exists()).toBe(true);
    await wrapper.get('[aria-label="确认保留冲突标记"]').setValue(true);
    expect(wrapper.get('[aria-label="确认保存解决结果"]').attributes("disabled")).toBeUndefined();
    wrapper.unmount();
  });
});
