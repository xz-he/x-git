<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Copy, Eraser, Square, Terminal, Zap } from "@lucide/vue";
import { formatDisplayPath } from "@/lib/formatPath";
import { useRepositoryStore } from "@/stores/repository";
import { useTerminalStore } from "@/stores/terminal";
import { consoleStatusLabel } from "./presentation";

const terminal = useTerminalStore();
const repository = useRepositoryStore();
const host = ref<HTMLElement>();
const copyStatus = ref("");
const screenError = ref("");
const statusText = computed(() => terminal.resetting ? t('uiEndingPreviousSession2b67f5') : terminal.refreshing ? t('uiRefreshingWorkbench07d483') : consoleStatusLabel(terminal.status));
let disposed = false;
onMounted(async () => {
  try { if (host.value) await terminal.attach(host.value); }
  catch (cause) { if (!disposed) screenError.value = cause instanceof Error ? cause.message : t('uiCouldNotInitializeTerminal7c63ac'); }
});
onBeforeUnmount(() => { disposed = true; terminal.detach(); });
async function copyOutput(): Promise<void> {
  try { await navigator.clipboard.writeText(terminal.outputText()); copyStatus.value = t('uiCopiede381a5'); }
  catch { copyStatus.value = t('uiCopyFailedSelectTextInTheTerminalAndCopyItManually265d5f'); }
}
</script>

<template>
  <section class="console-detail" :aria-label="t('uiGitCommandConsole73f165')">
    <header class="console-header">
      <div class="title"><Terminal :size="19" /><h1>{{ t('uiGitTerminalabeaee') }}</h1><span class="badge">{{ repository.snapshot?.currentBranch || t('uiDetachedHEADddb9e0') }}</span></div>
      <p :title="formatDisplayPath(repository.snapshot?.rootPath)">{{ formatDisplayPath(repository.snapshot?.rootPath) || t('uiOpenAGitRepositoryFirstca5538') }}</p>
    </header>
    <div class="output-toolbar">
      <span role="status">{{ statusText }}<template v-if="terminal.result"> · {{ terminal.result.durationMs }} ms<template v-if="terminal.result.exitCode !== null"> {{ t('uiExitCoded1cbfc') }} {{ terminal.result.exitCode }}</template></template></span>
      <div class="tools">
        <button v-if="terminal.running" :aria-label="t('uiInterruptCommand91c075')" :title="t('uiSendCtrlC60c5ca')" @click="terminal.sendInput('\x03')"><Zap :size="14" />{{ t('uiInterrupt44e681') }}</button>
        <button v-if="terminal.running" class="danger" :aria-label="t('uiTerminateCommand680944')" :disabled="terminal.cancelRequested" @click="terminal.terminate()"><Square :size="13" />{{ terminal.cancelRequested ? t('uiTerminating024aa3') : t('uiTerminate2eee57') }}</button>
        <button :aria-label="t('uiCopyOutputce4702')" :disabled="!terminal.screenReady" @click="copyOutput"><Copy :size="14" />{{ t('uiCopy4edd1d') }}</button>
        <button :aria-label="t('uiClearScreene8db7c')" :disabled="terminal.busy" @click="terminal.clear()"><Eraser :size="14" />{{ t('uiClearScreene8db7c') }}</button>
      </div>
    </div>
    <div v-if="terminal.error || terminal.refreshError || screenError || copyStatus" class="notices">
      <p v-if="terminal.error" role="alert">{{ terminal.error.message }}</p>
      <p v-if="terminal.refreshError" role="alert">{{ t('uiCommandFinishedButWorkbenchRefreshFailed23a0cb') }}{{ terminal.refreshError.message }} <button :disabled="terminal.refreshing" @click="terminal.retryRefresh()">{{ t('uiRefreshAgain8924dc') }}</button></p>
      <p v-if="screenError" role="alert">{{ screenError }}</p>
      <p v-if="copyStatus" role="status">{{ copyStatus }}</p>
    </div>
    <div ref="host" class="terminal-host" :aria-label="t('uiGitTerminalInputAndOutputd165be')" :title="t('uiEnterRunTabCompleteHistoryCtrlCInterruptCtrlLClear2a9daa')" @click="terminal.focus()" />
  </section>
</template>

<style scoped>
.console-detail { --surface-panel: #141627; --surface-muted: #20243a; --border: #2b3047; --text: #d8e1eb; --text-muted: #99a5ba; --primary: #23c17b; --primary-soft: #1d3935; --danger: #ff8993; display: flex; flex-direction: column; min-width: 0; min-height: 0; grid-row: 1 / -1; height: 100%; background: var(--surface-panel); color: var(--text); }
.console-header { padding: 10px 14px 8px; border-bottom: 1px solid var(--border); }
.title { display: flex; align-items: center; gap: 9px; flex-wrap: wrap; } h1 { margin: 0; font-size: 17px; }
.badge { max-width: 70%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--primary); background: var(--primary-soft); padding: 3px 8px; border-radius: 10px; font-size: 11px; }
.console-header p { margin: 7px 0 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: var(--text-muted); }
.output-toolbar { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; padding: 8px 16px; border-bottom: 1px solid var(--border); font-size: 11px; color: var(--text-muted); }
.tools { display: flex; gap: 5px; } button { display: inline-flex; align-items: center; gap: 5px; padding: 5px 8px; background: var(--surface-muted); border-radius: var(--radius-sm); font-size: 11px; }
.danger, [role="alert"] { color: var(--danger); }
.terminal-host { flex: 1; min-height: 0; min-width: 0; overflow: hidden; padding: 8px 8px 8px 10px; }
.notices { flex-shrink: 0; max-height: 25%; overflow: auto; padding: 0 16px; font-size: 12px; background: var(--surface-muted); } .notices p { margin: 7px 0; overflow-wrap: anywhere; }
</style>
