<script setup lang="ts">
import { MessageCircle, Send, Square, Trash2 } from "@lucide/vue";
import { computed, nextTick, ref, watch } from "vue";
import { useAiChatStore } from "@/stores/aiChat";
import { useAiStore } from "@/stores/ai";
import { useRepositoryStore } from "@/stores/repository";

const chat = useAiChatStore();
const ai = useAiStore();
const repository = useRepositoryStore();
const input = ref<HTMLTextAreaElement>();
const transcript = ref<HTMLElement>();
const disabled = computed(() => chat.running || ai.running || repository.navigationBusy || !chat.configured || !chat.draft.trim());
watch(() => chat.revealVersion, async () => {
  await nextTick(); input.value?.scrollIntoView?.({ block: "nearest" });
}, { immediate: true });
watch(() => [chat.messages.length, chat.running], async () => {
  await nextTick();
  if (transcript.value) transcript.value.scrollTop = transcript.value.scrollHeight;
});
</script>

<template>
  <section class="chat-panel" aria-label="AI 对话">
    <header><strong><MessageCircle :size="16" />AI 对话</strong><button class="clear-chat" title="清空对话" aria-label="清空 AI 对话" :disabled="chat.running" @click="chat.clear"><Trash2 :size="14" /></button></header>
    <p class="chat-hint">询问 Git 问题，或发送自动填入的报错。仅发送对话内容，不会自动执行命令。</p>
    <div v-if="chat.messages.length || chat.running" ref="transcript" class="transcript" role="log" aria-label="AI 对话记录" aria-live="polite">
      <article v-for="(message, index) in chat.messages" :key="index" :class="message.role"><small>{{ message.role === 'user' ? '你' : 'AI 助手' }}</small><div>{{ message.content }}</div></article>
      <article v-if="chat.running" class="user"><small>你</small><div>{{ chat.pendingQuestion }}</div></article>
      <p v-if="chat.running" class="chat-hint" role="status">AI 正在回复…</p>
    </div>
    <p v-if="chat.notice" class="chat-hint" role="status">{{ chat.notice }}</p>
    <p v-if="chat.error" class="chat-error" role="alert">{{ chat.error.message }}</p>
    <textarea ref="input" v-model="chat.draft" aria-label="AI 对话输入" placeholder="输入问题，Git 命令报错会自动填入这里…" rows="5" @keydown.ctrl.enter.prevent="!disabled && chat.send()" @keydown.meta.enter.prevent="!disabled && chat.send()" />
    <footer><small>{{ ai.running ? '请等待当前 AI 任务结束' : 'Ctrl + Enter 发送' }}</small><button v-if="chat.running" class="send-chat" @click="chat.cancel"><Square :size="13" />停止</button><button v-else class="send-chat" :disabled="disabled" @click="chat.send"><Send :size="14" />发送</button></footer>
  </section>
</template>

<style scoped>
.chat-panel { display: grid; gap: 10px; min-width: 0; padding-top: 4px; }
header, footer, header strong { display: flex; align-items: center; gap: 7px; }
header, footer { justify-content: space-between; }
header strong { font-size: 13px; }
.clear-chat { display: grid; place-items: center; width: 26px; height: 26px; border-radius: var(--radius-md); background: transparent; color: var(--text-muted); }
.clear-chat:hover { background: var(--surface-muted); }
.chat-hint, footer small { margin: 0; color: var(--text-muted); font-size: 11px; line-height: 1.6; }
.chat-error { margin: 0; padding: 8px; border-radius: var(--radius-md); background: var(--danger-soft); color: var(--danger); font-size: 12px; overflow-wrap: anywhere; }
.transcript { min-height: 0; max-height: 360px; overflow: auto; display: grid; gap: 10px; }
article { min-width: 0; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); }
article.user { background: var(--primary-soft); }
article small { color: var(--text-muted); font-size: 11px; }
article div { margin-top: 5px; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 13px; line-height: 1.65; user-select: text; }
textarea { box-sizing: border-box; width: 100%; min-height: 112px; max-height: 320px; resize: vertical; padding: 10px; border: 1px solid var(--border-strong); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text); font: inherit; font-size: 13px; line-height: 1.6; }
textarea:focus { outline: 2px solid var(--primary-border); border-color: var(--primary); }
.send-chat { display: flex; align-items: center; gap: 6px; padding: 7px 12px; border-radius: var(--radius-md); background: var(--primary); color: white; font-size: 12px; }
button:disabled { opacity: .5; cursor: not-allowed; }
</style>
