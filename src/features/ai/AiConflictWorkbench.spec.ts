import { createPinia, setActivePinia } from "pinia";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AiDrawerShell from "@/components/layout/AiDrawerShell.vue";
import ConflictDetail from "@/features/conflicts/ConflictDetail.vue";
import { conflictDetail, conflictResult, setupConflictSuggestion } from "@/test/conflictSuggestion";
import { useUiStore } from "@/stores/ui";
import { useAiStore } from "@/stores/ai";

const editorStub = { props: ["modelValue", "label"], template: '<pre :aria-label="label">{{ modelValue }}</pre>' };
const mountDrawer = () => mount(AiDrawerShell, { props: { width: 460 }, global: { stubs: { ConflictEditor: editorStub } } });
enableAutoUnmount(afterEach);
describe("AI conflict suggestion workbench", () => {
  beforeEach(() => setActivePinia(createPinia()));
  it("allows readable large conflicts to request chunked suggestions", async () => {
    const { conflicts, backend } = await setupConflictSuggestion();
    conflicts.current!.detail.ours = { ...conflicts.current!.detail.ours, byteLength: 1_799_081, text: "shared line\n".repeat(150_000) };
    vi.mocked(backend.aiStartConflictSuggestion).mockClear();
    const wrapper = mount(ConflictDetail, { global: { stubs: { ConflictEditor: editorStub } } });
    expect(wrapper.get('[aria-label="AI 解决建议"]').attributes("disabled")).toBeUndefined();
    await wrapper.get('[aria-label="AI 解决建议"]').trigger("click");
    await flushPromises();
    expect(backend.aiStartConflictSuggestion).toHaveBeenCalledOnce();
  });
  it("starts from selected conflict independently of review rules and discloses draft scope", async () => {
    const { backend, ai } = await setupConflictSuggestion();
    ai.reviewSkill = { state: "missing", info: null, error: null };
    vi.mocked(backend.aiStartConflictSuggestion).mockClear();
    const wrapper = mount(ConflictDetail, { global: { stubs: { ConflictEditor: editorStub } } });
    expect(wrapper.text()).toContain("未保存草稿不发送");
    await wrapper.get('[aria-label="AI 解决建议"]').trigger("click");
    await flushPromises();
    expect(backend.aiStartConflictSuggestion).toHaveBeenCalledWith("C:/repo", expect.any(String), conflictDetail.path, conflictDetail.token);
  });
  it("shows a candidate as text and confirms only a draft replacement", async () => {
    const { ai, backend, conflicts } = await setupConflictSuggestion();
    ai.conflictResult = conflictResult("<script>literal</script>\n");
    const wrapper = mountDrawer();
    expect(wrapper.text()).toContain("保留双方修改");
    expect(wrapper.find("script").exists()).toBe(false);
    await wrapper.get('[aria-label="预览应用建议"]').trigger("click");
    await flushPromises();
    const dialog = wrapper.get('[role="dialog"]');
    expect(dialog.text()).toContain("仅替换草稿");
    expect(dialog.text()).toContain("original");
    expect(conflicts.current?.dirty).toBe(false);
    await dialog.get('[aria-label="确认填入解决草稿"]').trigger("click");
    await flushPromises();
    expect(conflicts.current?.resolution).toMatchObject({ kind: "text", text: "<script>literal</script>\n" });
    expect(backend.conflictsResolve).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("尚未写入或暂存");
  });
  it("renders adviceOnly without application controls and uses accurate rebase labels", async () => {
    const { ai } = await setupConflictSuggestion();
    ai.conflictResult = { ...conflictResult(null), context: { ...conflictResult().context, operationKind: "rebase" }, contextMissing: ["外部调用定义"] };
    const wrapper = mountDrawer();
    expect(wrapper.text()).toContain("变基目标 + 已重放提交");
    expect(wrapper.text()).toContain("正在重放的提交");
    expect(wrapper.text()).toContain("外部调用定义");
    expect(wrapper.find('[aria-label="预览应用建议"]').exists()).toBe(false);
  });
  it("opens the conflict workbench from the shortcut without inventing a target", async () => {
    const { backend, conflicts } = await setupConflictSuggestion();
    useAiStore().currentTask = undefined;
    conflicts.selectedPath = undefined;
    useUiStore().openView("history");
    vi.mocked(backend.aiStartConflictSuggestion).mockClear();
    const wrapper = mountDrawer();
    await wrapper.get('[aria-label="AI 冲突解决建议"]').trigger("click");
    expect(useUiStore().activeView).toBe("conflicts");
    expect(backend.aiStartConflictSuggestion).not.toHaveBeenCalled();
  });
});
