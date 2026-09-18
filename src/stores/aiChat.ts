import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type { AiChatMessage, BackendError } from "@/lib/backend/types";
import { cleanGitErrorText, resetGitFailureScope, type GitFailure } from "@/lib/gitFailure";
import { formatDisplayPath } from "@/lib/formatPath";
import { useAiStore } from "@/stores/ai";
import { useRepositoryStore } from "@/stores/repository";
import { useSettingsStore } from "@/stores/settings";

const MAX_CONTEXT_BYTES = 64 * 1024;
const MAX_ERROR_CHARS = 12_000;
export const useAiChatStore = defineStore("ai-chat", () => {
  const draft = ref("");
  const messages = ref<AiChatMessage[]>([]);
  const pendingQuestion = ref("");
  const running = ref(false);
  const error = ref<BackendError>();
  const notice = ref("");
  const revealVersion = ref(0);
  const openRequested = ref(false);
  const configured = computed(() => {
    const settings = useSettingsStore().settings;
    return !!(settings.apiKey.trim() && settings.baseUrl.trim() && settings.model.trim());
  });
  let runId: string | undefined;
  let version = 0;
  let lastFailure = "";

  function captureFailure(failure: GitFailure): void {
    const root = useRepositoryStore().snapshot?.rootPath;
    const normalize = (path: string) => formatDisplayPath(path).replace(/\\/g, "/").replace(/\/$/, "").toLowerCase();
    if (!root || normalize(root) !== normalize(failure.root)) return;
    const source = cleanGitErrorText(`请帮我分析以下 Git 命令报错，并给出解决步骤：\n\n操作：${failure.command}\n错误：${failure.error.message}\n${failure.error.diagnostics ?? ""}\n${failure.output ?? ""}`).trim();
    const text = source.length > MAX_ERROR_CHARS ? `${source.slice(0, MAX_ERROR_CHARS)}\n[报错内容过长，已截取前 ${MAX_ERROR_CHARS} 字符]` : source;
    if (lastFailure === text && draft.value.includes(text)) return;
    lastFailure = text;
    draft.value = draft.value.trim() ? `${draft.value}\n\n${text}` : text;
    notice.value = "Git 报错已填入，可编辑后发送。";
    useSettingsStore().settings.aiDrawerOpen = true;
    openRequested.value = true;
    revealVersion.value++;
  }

  async function send(): Promise<void> {
    const submittedDraft = draft.value;
    const question = submittedDraft.trim();
    if (!question || running.value || useAiStore().running || useRepositoryStore().navigationBusy) return;
    error.value = undefined;
    if (!configured.value) { error.value = { code: "aiConfiguration", message: "请先在设置中完成 AI 服务配置。" }; return; }
    const outgoing: AiChatMessage[] = [...messages.value.map(message => ({ ...message })), { role: "user", content: question }];
    let trimmed = false;
    while ((outgoing.length > 32 || new TextEncoder().encode(JSON.stringify(outgoing)).length > MAX_CONTEXT_BYTES) && outgoing.length > 1) {
      outgoing.splice(0, 2); trimmed = true;
    }
    if (new TextEncoder().encode(JSON.stringify(outgoing)).length > MAX_CONTEXT_BYTES) {
      error.value = { code: "aiContextTooLarge", message: "问题超过 64 KiB，请缩短后发送。" }; return;
    }
    notice.value = trimmed ? "本次仅携带容量范围内的最近对话。" : "";
    const id = crypto.randomUUID();
    const current = version;
    runId = id; running.value = true; pendingQuestion.value = question;
    draft.value = "";
    try {
      const reply = await backendClient.aiChat(id, outgoing);
      if (version !== current) return;
      messages.value.push({ role: "user", content: question }, { role: "assistant", content: reply });
    } catch (cause) {
      if (version === current) {
        error.value = normalizeBackendError(cause);
        if (!draft.value) draft.value = submittedDraft;
      }
    } finally {
      if (runId === id) { running.value = false; runId = undefined; pendingQuestion.value = ""; }
    }
  }
  async function cancel(): Promise<void> {
    if (!runId) return;
    try { await backendClient.aiCancel(runId); }
    catch (cause) { error.value = normalizeBackendError(cause); }
  }
  function clear(): void {
    version++;
    void cancel();
    messages.value = []; draft.value = ""; pendingQuestion.value = ""; error.value = undefined; notice.value = ""; lastFailure = ""; openRequested.value = false;
  }
  function resetForRepository(): void { resetGitFailureScope(); clear(); }
  return { draft, messages, pendingQuestion, running, error, notice, revealVersion, openRequested, configured, captureFailure, send, cancel, clear, resetForRepository };
});
