<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Copy, Eraser, Square, Terminal, Zap } from "@lucide/vue";
import { formatDisplayPath } from "@/lib/formatPath";
import { useRepositoryStore } from "@/stores/repository";
import { useTerminalStore } from "@/stores/terminal";
import TerminalInput from "./TerminalInput.vue";
import { consoleStatusLabel } from "./presentation";

const terminal = useTerminalStore();
const repository = useRepositoryStore();
const host = ref<HTMLElement>();
const copyStatus = ref("");
const screenError = ref("");
const examples = ["git status", "git log --oneline -n 20", "git diff", "git add -p"];
const statusText = computed(() => terminal.resetting ? "正在结束旧会话" : terminal.refreshing ? "正在刷新工作台" : consoleStatusLabel(terminal.status));
let disposed = false;
onMounted(async () => {
  try { if (host.value) await terminal.attach(host.value); }
  catch (cause) { if (!disposed) screenError.value = cause instanceof Error ? cause.message : "无法初始化终端"; }
});
onBeforeUnmount(() => { disposed = true; terminal.detach(); });
async function copyOutput(): Promise<void> {
  try { await navigator.clipboard.writeText(terminal.outputText()); copyStatus.value = "已复制"; }
  catch { copyStatus.value = "复制失败，请在终端中选择文本后复制。"; }
}
</script>

<template>
  <section class="console-detail" aria-label="Git 命令控制台">
    <header class="console-header">
      <div class="title"><Terminal :size="19" /><h1>Git 终端</h1><span class="badge">{{ repository.snapshot?.currentBranch || '分离 HEAD' }}</span></div>
      <p :title="formatDisplayPath(repository.snapshot?.rootPath)">{{ formatDisplayPath(repository.snapshot?.rootPath) || '请先打开 Git 仓库' }}</p>
    </header>
    <div class="output-toolbar">
      <span role="status">{{ statusText }}<template v-if="terminal.result"> · {{ terminal.result.durationMs }} ms<template v-if="terminal.result.exitCode !== null"> · 退出码 {{ terminal.result.exitCode }}</template></template></span>
      <div class="tools">
        <button v-if="terminal.running" aria-label="中断命令" title="发送 Ctrl+C" @click="terminal.sendInput('\x03')"><Zap :size="14" />中断</button>
        <button v-if="terminal.running" class="danger" aria-label="终止命令" :disabled="terminal.cancelRequested" @click="terminal.terminate()"><Square :size="13" />{{ terminal.cancelRequested ? '终止中…' : '终止' }}</button>
        <button aria-label="复制输出" :disabled="!terminal.screenReady" @click="copyOutput"><Copy :size="14" />复制</button>
        <button aria-label="清屏" :disabled="terminal.busy" @click="terminal.clear()"><Eraser :size="14" />清屏</button>
      </div>
    </div>
    <div v-if="terminal.error || terminal.refreshError || screenError || copyStatus" class="notices">
      <p v-if="terminal.error" role="alert">{{ terminal.error.message }}</p>
      <p v-if="terminal.refreshError" role="alert">命令已结束，工作台刷新失败：{{ terminal.refreshError.message }} <button :disabled="terminal.refreshing" @click="terminal.retryRefresh()">重新刷新</button></p>
      <p v-if="screenError" role="alert">{{ screenError }}</p>
      <p v-if="copyStatus" role="status">{{ copyStatus }}</p>
    </div>
    <div ref="host" class="terminal-host" aria-label="命令输出" @click="terminal.focus()" />
    <div v-if="!terminal.history.length && !terminal.busy" class="terminal-help">
      <span>支持 Git 子命令、参数和别名；可在终端内交互。管道和重定向请使用系统 Shell。</span>
      <div><button v-for="command in examples" :key="command" :data-command="command" @click="terminal.draft = command">{{ command }}</button></div>
    </div>
    <TerminalInput />
  </section>
</template>

<style scoped>
.console-detail { display: flex; flex-direction: column; min-width: 0; min-height: 0; grid-row: 1 / -1; height: 100%; background: var(--surface-panel); }
.console-header { padding: 16px 20px 12px; border-bottom: 1px solid var(--border); }
.title { display: flex; align-items: center; gap: 9px; flex-wrap: wrap; } h1 { margin: 0; font-size: 17px; }
.badge { max-width: 70%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--primary); background: var(--primary-soft); padding: 3px 8px; border-radius: 10px; font-size: 11px; }
.console-header p { margin: 7px 0 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: var(--text-muted); }
.output-toolbar { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; padding: 8px 16px; border-bottom: 1px solid var(--border); font-size: 11px; color: var(--text-muted); }
.tools { display: flex; gap: 5px; } button { display: inline-flex; align-items: center; gap: 5px; padding: 5px 8px; background: var(--surface-muted); border-radius: var(--radius-sm); font-size: 11px; }
.danger, [role="alert"] { color: var(--danger); }
.terminal-host { flex: 1; min-height: 100px; min-width: 0; overflow: hidden; padding: 10px 8px 0 12px; }
.notices { flex-shrink: 0; max-height: 25%; overflow: auto; padding: 0 16px; font-size: 12px; background: var(--surface-muted); } .notices p { margin: 7px 0; overflow-wrap: anywhere; }
.terminal-help { padding: 8px 16px 12px; font-size: 11px; color: var(--text-muted); line-height: 1.7; }
.terminal-help div { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 7px; } .terminal-help button { font-family: var(--font-code); }
</style>
