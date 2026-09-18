import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { backendClient, setBackendClientForTests } from "@/lib/backend/client";
import { cleanGitErrorText, onGitFailure } from "@/lib/gitFailure";
import { createBackendFixture, createTestSettings } from "@/test/backend";
import { useAiChatStore } from "./aiChat";
import { useAiStore } from "./ai";
import { useRepositoryStore } from "./repository";
import { useSettingsStore } from "./settings";
import AiChatPanel from "@/features/ai/AiChatPanel.vue";

function deferred<T>() { let resolve!: (value: T) => void; let reject!: (cause: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const failure = { root: "C:/repo", command: "git push", error: { code: "gitCommandFailed" as const, message: "push failed", diagnostics: "non-fast-forward" } };
describe("AI chat and Git failure drafts", () => {
  let backend: ReturnType<typeof createBackendFixture>;
  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture({ aiChat: vi.fn(async () => "先运行 git status") });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abc", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null };
    useSettingsStore().settings = createTestSettings({ apiKey: "key" });
  });
  it("automatically fills actual failed Git operations without sending or overwriting a draft", async () => {
    const chat = useAiChatStore(); chat.draft = "我的补充问题";
    const stop = onGitFailure(chat.captureFailure);
    try {
      vi.mocked(backend.refsSwitch).mockRejectedValueOnce(failure.error);
      await expect(backendClient.refsSwitch("C:/repo", "feature/test")).rejects.toBe(failure.error);
      expect(chat.draft).toContain("我的补充问题"); expect(chat.draft).toContain("git switch");
      expect(chat.draft).toContain("feature/test"); expect(chat.draft).toContain("non-fast-forward");
      expect(useSettingsStore().settings.aiDrawerOpen).toBe(true);
      expect(backend.aiChat).not.toHaveBeenCalled();
    } finally { stop(); }
  });
  it("ignores duplicate drafts, other repositories and cancelled operations", async () => {
    const chat = useAiChatStore(); chat.captureFailure(failure); const draft = chat.draft;
    chat.captureFailure(failure); chat.captureFailure({ ...failure, root: "C:/other" });
    expect(chat.draft).toBe(draft);
    const stop = onGitFailure(chat.captureFailure);
    try {
      vi.mocked(backend.refsSwitch).mockRejectedValueOnce({ code: "cancelled", message: "stop" });
      await backendClient.refsSwitch("C:/repo", "x").catch(() => undefined);
      expect(chat.draft).toBe(draft);
    } finally { stop(); }
  });
  it("sends conversation history and preserves edits made while waiting", async () => {
    const chat = useAiChatStore(); chat.draft = "如何处理 push 失败？"; await chat.send();
    expect(chat.messages).toHaveLength(2); expect(chat.draft).toBe("");
    const pending = deferred<string>(); vi.mocked(backend.aiChat).mockReturnValueOnce(pending.promise);
    chat.draft = "下一步呢？"; const sending = chat.send(); chat.draft = "另一个问题";
    await chat.send(); expect(backend.aiChat).toHaveBeenCalledTimes(2);
    expect(vi.mocked(backend.aiChat).mock.calls[1]![1]).toEqual([...chat.messages, { role: "user", content: "下一步呢？" }]);
    pending.resolve("检查远程分支"); await sending;
    expect(chat.draft).toBe("另一个问题"); expect(chat.messages).toHaveLength(4);
  });
  it("clears the composer as soon as a question is sent and preserves an identical new draft", async () => {
    const pending = deferred<string>(); vi.mocked(backend.aiChat).mockReturnValueOnce(pending.promise);
    const wrapper = mount(AiChatPanel);
    try {
      const input = wrapper.get("textarea");
      await input.setValue("建立上游跟踪是什么意思");
      await wrapper.get("button.send-chat").trigger("click");
      expect(wrapper.get('[aria-label="AI 对话记录"]').text()).toContain("建立上游跟踪是什么意思");
      expect(wrapper.text()).toContain("AI 正在回复");
      expect((input.element as HTMLTextAreaElement).value).toBe("");
      await input.setValue("建立上游跟踪是什么意思");
      pending.resolve("上游分支用于关联本地分支和远端分支。"); await flushPromises();
      expect((input.element as HTMLTextAreaElement).value).toBe("建立上游跟踪是什么意思");
      expect(wrapper.findAll("article.user")).toHaveLength(1);
    } finally { wrapper.unmount(); }
  });
  it("keeps the question on transport failure and supports retry", async () => {
    const chat = useAiChatStore(); chat.draft = "help";
    vi.mocked(backend.aiChat).mockRejectedValueOnce({ code: "aiTransport", message: "连接失败" });
    await chat.send(); expect(chat.draft).toBe("help"); expect(chat.error?.message).toBe("连接失败");
    await chat.send(); expect(chat.messages).toHaveLength(2); expect(chat.error).toBeUndefined();
  });
  it.each(["aiTransport", "cancelled"])("restores an unanswered question after %s without overwriting new input", async code => {
    const chat = useAiChatStore();
    const first = deferred<string>(); vi.mocked(backend.aiChat).mockReturnValueOnce(first.promise);
    chat.draft = "  原问题  "; const sending = chat.send();
    expect(chat.draft).toBe("");
    first.reject({ code, message: "请求未完成" }); await sending;
    expect(chat.draft).toBe("  原问题  ");
    expect(chat.messages).toEqual([]);
    const retry = deferred<string>(); vi.mocked(backend.aiChat).mockReturnValueOnce(retry.promise);
    const retrying = chat.send(); chat.draft = "下一条问题";
    retry.reject({ code, message: "请求未完成" }); await retrying;
    expect(chat.draft).toBe("下一条问题");
  });
  it("preserves newly captured Git errors without repeating the sent question", async () => {
    const pending = deferred<string>(); vi.mocked(backend.aiChat).mockReturnValueOnce(pending.promise);
    const chat = useAiChatStore(); chat.draft = "原问题"; const sending = chat.send();
    chat.captureFailure(failure);
    expect(chat.draft).toContain("non-fast-forward"); expect(chat.draft).not.toContain("原问题");
    pending.resolve("回复"); await sending;
    expect(chat.draft).toContain("non-fast-forward");
  });
  it("does not restore a failed question after the conversation is cleared", async () => {
    const pending = deferred<string>(); vi.mocked(backend.aiChat).mockReturnValueOnce(pending.promise);
    const chat = useAiChatStore(); chat.draft = "原问题"; const sending = chat.send();
    chat.clear(); pending.reject({ code: "cancelled", message: "已停止" }); await sending;
    expect(chat.draft).toBe(""); expect(chat.messages).toEqual([]); expect(chat.error).toBeUndefined();
  });
  it("keeps unsent input when AI configuration is missing", async () => {
    const chat = useAiChatStore(); chat.draft = "原问题";
    useSettingsStore().settings.apiKey = "";
    await chat.send();
    expect(chat.draft).toBe("原问题"); expect(backend.aiChat).not.toHaveBeenCalled();
  });
  it("cancels on repository replacement and discards late replies and errors", async () => {
    const pending = deferred<string>(); vi.mocked(backend.aiChat).mockReturnValueOnce(pending.promise);
    const chat = useAiChatStore(); chat.draft = "old repo"; const sending = chat.send();
    chat.resetForRepository(); chat.draft = "new repo";
    expect(backend.aiCancel).toHaveBeenCalledWith(vi.mocked(backend.aiChat).mock.calls[0]![0]);
    pending.resolve("late response"); await sending;
    expect(chat.messages).toEqual([]); expect(chat.draft).toBe("new repo"); expect(chat.running).toBe(false);
  });
  it("drops late Git failures after repository scope changes", async () => {
    const pending = deferred<never>(); vi.mocked(backend.refsSwitch).mockReturnValueOnce(pending.promise);
    const chat = useAiChatStore(); const stop = onGitFailure(chat.captureFailure);
    try {
      const request = backendClient.refsSwitch("C:/repo", "x").catch(() => undefined);
      chat.resetForRepository(); pending.reject(failure.error); await request;
      expect(chat.draft).toBe("");
    } finally { stop(); }
  });
  it("bounds UTF-8 context by dropping old turns and rejects an oversized question", async () => {
    const chat = useAiChatStore();
    chat.messages = [{ role: "user", content: "old question" }, { role: "assistant", content: "中".repeat(25000) }];
    chat.draft = "new question"; await chat.send();
    expect(vi.mocked(backend.aiChat).mock.calls[0]![1]).toEqual([{ role: "user", content: "new question" }]);
    expect(chat.notice).toContain("最近对话");
    chat.draft = "中".repeat(25000); await chat.send();
    expect(chat.error?.code).toBe("aiContextTooLarge"); expect(backend.aiChat).toHaveBeenCalledTimes(1);
  });
  it("blocks chat during another AI task and renders answers as text", async () => {
    const chat = useAiChatStore(); chat.draft = "help"; useAiStore().status = "running";
    await chat.send(); expect(backend.aiChat).not.toHaveBeenCalled();
    useAiStore().status = "idle";
    const wrapper = mount(AiChatPanel);
    vi.mocked(backend.aiChat).mockResolvedValueOnce('<img src=x onerror="alert(1)">');
    await wrapper.get("button.send-chat").trigger("click"); await flushPromises();
    expect(wrapper.find("img").exists()).toBe(false); expect(wrapper.text()).toContain("<img");
    wrapper.unmount();
  });
  it("removes terminal controls and common credentials before drafting", () => {
    const text = cleanGitErrorText("\x1b[31merror\x1b[0m https://user:secret@host/repo?token=abc\nAuthorization: Bearer private\napi_key=hidden");
    expect(text).toContain("error"); expect(text).not.toContain("secret"); expect(text).not.toContain("abc");
    expect(text).not.toContain("private"); expect(text).not.toContain("hidden"); expect(text).not.toContain("\x1b");
  });
});
