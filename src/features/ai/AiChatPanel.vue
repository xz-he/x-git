<script setup lang="ts">
import { t } from '@/lib/i18n';
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
  <section class="chat-panel" :aria-label="t('uiAIChat37f435')">
    <header><strong><MessageCircle :size="16" />{{ t('uiAIChat37f435') }}</strong><button class="clear-chat" :title="t('uiClearChat35c974')" :aria-label="t('uiClearAIChatd8ebf2')" :disabled="chat.running" @click="chat.clear"><Trash2 :size="14" /></button></header>
    <p class="chat-hint">{{ t('uiAskAboutGitOrSendAnAutomaticallyFilledErrorReportOnlyChatCon8e56d1') }}</p>
    <div v-if="chat.messages.length || chat.running" ref="transcript" class="transcript" role="log" :aria-label="t('uiAIChatHistoryac7215')" aria-live="polite">
      <article v-for="(message, index) in chat.messages" :key="index" :class="message.role"><small>{{ message.role === 'user' ? t('uiYou5630b8') : t('uiAIAssistant5341ec') }}</small><div>{{ message.content }}</div></article>
      <article v-if="chat.running" class="user"><small>{{ t('uiYou5630b8') }}</small><div>{{ chat.pendingQuestion }}</div></article>
      <p v-if="chat.running" class="chat-hint" role="status">{{ t('uiAIIsReplying5cede0') }}</p>
    </div>
    <p v-if="chat.notice" class="chat-hint" role="status">{{ chat.notice }}</p>
    <p v-if="chat.error" class="chat-error" role="alert">{{ chat.error.message }}</p>
    <textarea ref="input" v-model="chat.draft" :aria-label="t('uiAIChatInputa84121')" :placeholder="t('uiEnterAQuestionGitCommandErrorsWillAppearHereAutomatically0108d9')" rows="5" @keydown.ctrl.enter.prevent="!disabled && chat.send()" @keydown.meta.enter.prevent="!disabled && chat.send()" />
    <footer><small>{{ ai.running ? t('uiWaitForTheCurrentAITaskToFinish433d97') : t('uiCtrlEnterToSend0a99f9') }}</small><button v-if="chat.running" class="send-chat" @click="chat.cancel"><Square :size="13" />{{ t('uiStopa17f70') }}</button><button v-else class="send-chat" :disabled="disabled" @click="chat.send"><Send :size="14" />{{ t('uiSend1214d6') }}</button></footer>
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
