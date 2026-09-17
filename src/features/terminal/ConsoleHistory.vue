<script setup lang="ts">
import { History, Trash2 } from "@lucide/vue";
import { useTerminalStore } from "@/stores/terminal";
import { consoleStatusLabel } from "./presentation";
const consoleStore = useTerminalStore();
function time(value: number): string { return new Date(value).toLocaleTimeString("zh-CN", { hour12: false }); }
</script>
<template>
  <section class="console-history" aria-label="命令历史">
    <header><span><History :size="15" />本次会话</span><button aria-label="清空命令历史" title="清空命令历史" :disabled="consoleStore.busy || !consoleStore.history.length" @click="consoleStore.clearHistory()"><Trash2 :size="14" /></button></header>
    <p class="hint">最近 20 条命令 · 点击填入，不自动执行</p>
    <div class="history-list">
      <p v-if="!consoleStore.history.length" class="empty">还没有执行记录</p>
      <button v-for="entry in consoleStore.history" :key="entry.runId" class="entry" data-testid="console-history-entry" :disabled="consoleStore.busy" @click="consoleStore.recall(entry)">
        <code>{{ entry.command }}</code>
        <span>{{ time(entry.startedAt) }} · {{ consoleStatusLabel(entry.status) }}<template v-if="entry.durationMs !== null"> · {{ entry.durationMs }} ms</template></span>
      </button>
    </div>
    <footer>历史仅保留在当前会话，切换仓库后清空。终端保留最近 10,000 行输出。</footer>
  </section>
</template>
<style scoped>
.console-history { display: flex; flex-direction: column; min-height: 0; height: 100%; }
header { display: flex; align-items: center; justify-content: space-between; height: 48px; flex-shrink: 0; padding: 0 14px; border-bottom: 1px solid var(--border); }
header span { display: flex; align-items: center; gap: 7px; font-weight: 600; }
header button { display: flex; padding: 6px; background: transparent; }
.hint, footer { color: var(--text-muted); font-size: 11px; line-height: 1.6; padding: 0 14px; }
.history-list { flex: 1; min-height: 0; overflow: auto; }
.entry { display: grid; gap: 6px; width: 100%; padding: 12px 14px; border-bottom: 1px solid var(--border); background: transparent; text-align: left; }
.entry:hover { background: var(--surface-muted); }
.entry code { font-family: var(--font-code); font-size: 12px; overflow-wrap: anywhere; }
.entry span { color: var(--text-muted); font-size: 11px; overflow-wrap: anywhere; }
.empty { padding: 24px 14px; color: var(--text-muted); font-size: 12px; }
footer { padding: 12px 14px; border-top: 1px solid var(--border); }
</style>
