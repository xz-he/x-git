import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import GitFeedbackPanel from "./GitFeedbackPanel.vue";
import { feedbackExpanded, gitFeedback, resetGitFeedback } from "@/lib/gitFeedback";

enableAutoUnmount(afterEach);
describe("Git feedback panel", () => {
  beforeEach(() => { resetGitFeedback(); });
  function panel() { return mount(GitFeedbackPanel, { global: { stubs: { Teleport: true } } }); }
  function start() { gitFeedback.value = [{ id: "a", root: "C:/repo", title: "Push 推送", target: "main → origin/main", status: "running", message: "正在连接远端", startedAt: Date.now(), canCancel: false, cancelling: false }]; }
  it.each([
    [String.raw`\\?\D:\hq-project\fancyqube`, String.raw`D:\hq-project\fancyqube`],
    [String.raw`\\?\UNC\server\share\repo`, String.raw`\\server\share\repo`],
    ["D:/hq-project/fancyqube", "D:/hq-project/fancyqube"],
  ])("formats the feedback path and tooltip without changing the operation path (%s)", (root, displayed) => {
    start(); gitFeedback.value[0]!.root = root;
    const wrapper = panel();
    expect(wrapper.get(".context").text()).toBe(displayed);
    expect(wrapper.get(".context").attributes("title")).toBe(displayed);
    expect(gitFeedback.value[0]!.root).toBe(root);
  });
  it("shows indeterminate progress and only displays real stage percentages", async () => {
    start(); const wrapper = panel();
    expect(wrapper.get('[role="progressbar"]').attributes("aria-valuenow")).toBeUndefined();
    expect(wrapper.find(".feedback-spin").exists()).toBe(true);
    gitFeedback.value[0]!.percent = 100; gitFeedback.value[0]!.phase = "发送对象"; await flushPromises();
    expect(wrapper.get('[role="progressbar"]').attributes("aria-valuenow")).toBe("100");
    expect(wrapper.text()).toContain("等待 Git 确认最终结果"); expect(wrapper.text()).not.toContain("推送成功");
  });
  it("keeps results until dismissed and supports collapse without losing ongoing work", async () => {
    start(); const wrapper = panel();
    await wrapper.get('[aria-label="展开或收起 Git 操作反馈"]').trigger("click");
    expect(feedbackExpanded.value).toBe(false); expect(wrapper.find(".feedback-list").exists()).toBe(false);
    await wrapper.get('[aria-label="展开或收起 Git 操作反馈"]').trigger("click");
    Object.assign(gitFeedback.value[0]!, { status: "failed", message: "推送失败", diagnostics: "远端拒绝更新", finishedAt: Date.now() }); await flushPromises();
    expect(wrapper.get('[role="alert"]').text()).toBe("推送失败");
    expect(wrapper.get("details").text()).toContain("远端拒绝更新");
    await wrapper.get('[aria-label="关闭Push 推送反馈"]').trigger("click");
    expect(wrapper.find("aside").exists()).toBe(false);
  });
  it("keeps running operations visible when clearing completed notices", async () => {
    start(); gitFeedback.value.unshift({ ...gitFeedback.value[0]!, id: "b", status: "success", title: "暂存文件" });
    const wrapper = panel();
    expect(wrapper.findAll(".feedback-item")[0]!.text()).toContain("Push 推送");
    await wrapper.get(".clear").trigger("click"); expect(gitFeedback.value).toHaveLength(1); expect(gitFeedback.value[0]!.id).toBe("a");
  });
});
