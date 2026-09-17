<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from "vue";
import { Play, LoaderCircle } from "@lucide/vue";
import { backendClient } from "@/lib/backend/client";
import type { TerminalCompletion } from "@/lib/backend/types";
import { useRepositoryStore } from "@/stores/repository";
import { useTerminalStore } from "@/stores/terminal";
import { commonPrefix, replaceCompletion } from "./completion";

const terminal = useTerminalStore();
const repository = useRepositoryStore();
const input = ref<HTMLInputElement>();
const completion = ref<TerminalCompletion>();
const selected = ref(0);
const querying = ref(false);
const id = useId();
const blocked = computed(() => terminal.busy || repository.otherOperationBusy || !repository.snapshot);
const popup = computed(() => !blocked.value && !!completion.value?.items.length);
let version = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let historyIndex = -1;
let historyDraft = "";
let lastQuery = "";
let applying = false;
let tabEngaged = false;

function close(): void { version++; completion.value = undefined; querying.value = false; lastQuery = ""; tabEngaged = false; if (timer) clearTimeout(timer); }
function edited(): void { close(); historyIndex = -1; timer = setTimeout(() => { void query(false); }, 160); }
async function setDraft(value: string, cursor = value.length): Promise<void> {
  applying = true; terminal.draft = value; await nextTick(); input.value?.setSelectionRange(cursor, cursor); applying = false;
}
async function apply(index: number): Promise<void> {
  const result = completion.value; const item = result?.items[index]; if (!result || !item) return;
  const replaced = replaceCompletion(terminal.draft, result, item.value, true);
  close(); await setDraft(replaced.command, replaced.cursor); input.value?.focus();
}
async function query(tab: boolean): Promise<void> {
  if (blocked.value || !input.value) return;
  if (timer) clearTimeout(timer);
  const command = terminal.draft;
  const cursor = input.value.selectionStart ?? command.length;
  const root = repository.snapshot!.rootPath;
  const generation = repository.generation;
  const request = ++version;
  querying.value = true;
  try {
    const result = await backendClient.terminalComplete(root, command, cursor);
    if (request !== version || blocked.value || terminal.draft !== command || repository.snapshot?.rootPath !== root || repository.generation !== generation || input.value?.selectionStart !== cursor) return;
    if (result.start < 0 || result.end < result.start || result.end > command.length) return;
    completion.value = result; selected.value = 0; lastQuery = command; tabEngaged = tab;
    if (tab && result.items.length === 1) await apply(0);
    else if (tab && result.items.length > 1) {
      const prefix = commonPrefix(result.items.map(item => item.value));
      if (prefix.length > cursor - result.start) {
        const replaced = replaceCompletion(command, result, prefix, false);
        await setDraft(replaced.command, replaced.cursor);
        completion.value = { ...result, end: result.start + prefix.length }; lastQuery = replaced.command;
      }
    }
  } catch { if (request === version) completion.value = undefined; }
  finally { if (request === version) querying.value = false; }
}
function moveHistory(direction: number): void {
  close();
  if (!terminal.history.length) return;
  if (historyIndex < 0 && direction > 0) historyDraft = terminal.draft;
  historyIndex = Math.max(-1, Math.min(terminal.history.length - 1, historyIndex + direction));
  void setDraft(historyIndex < 0 ? historyDraft : terminal.history[historyIndex]!.command);
}
async function keydown(event: KeyboardEvent): Promise<void> {
  if (event.isComposing || blocked.value) return;
  if (event.key === "Tab") {
    event.preventDefault();
    if (popup.value && completion.value!.items.length === 1 && lastQuery === terminal.draft) await apply(0);
    else if (popup.value && tabEngaged && lastQuery === terminal.draft) selected.value = (selected.value + (event.shiftKey ? -1 : 1) + completion.value!.items.length) % completion.value!.items.length;
    else await query(true);
  } else if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) close();
  else if (event.key === "Escape") { event.preventDefault(); close(); }
  else if (event.key === "ArrowUp" || event.key === "ArrowDown") {
    event.preventDefault();
    const direction = event.key === "ArrowUp" ? -1 : 1;
    if (popup.value) selected.value = (selected.value + direction + completion.value!.items.length) % completion.value!.items.length;
    else { if (timer) clearTimeout(timer); moveHistory(-direction); }
  } else if (event.key === "Enter") {
    event.preventDefault();
    if (popup.value) await apply(selected.value);
    else { close(); historyIndex = -1; await terminal.run(); }
  }
}
watch(() => terminal.draft, () => { if (!applying) close(); }, { flush: "sync" });
watch(blocked, async value => { close(); if (!value) { await nextTick(); input.value?.focus(); } });
watch(selected, async () => { await nextTick(); document.getElementById(`${id}-option-${selected.value}`)?.scrollIntoView?.({ block: "nearest" }); });
onBeforeUnmount(close);
</script>

<template>
  <div class="terminal-command">
    <div v-if="popup" :id="`${id}-list`" class="completion-menu" role="listbox" aria-label="Git 命令提示">
      <button v-for="(item, index) in completion!.items" :id="`${id}-option-${index}`" :key="`${item.value}:${index}`" type="button" role="option" :aria-selected="index === selected" :class="{ selected: index === selected }" @mousedown.prevent @click="apply(index)"><code>{{ item.label }}</code><span>{{ item.description }}</span></button>
      <small v-if="completion!.hasMore">还有更多匹配项，请继续输入缩小范围</small>
    </div>
    <div class="input-row">
      <span class="prompt" aria-hidden="true">❯</span>
      <input ref="input" v-model="terminal.draft" role="combobox" aria-label="Git 命令" :aria-expanded="popup" :aria-controls="popup ? `${id}-list` : undefined" :aria-activedescendant="popup ? `${id}-option-${selected}` : undefined" aria-autocomplete="list" :disabled="blocked" spellcheck="false" autocomplete="off" placeholder="git status" @input="edited" @keydown="keydown" @blur="close" @click="close" />
      <LoaderCircle v-if="querying" :size="15" class="spin" />
      <button class="run" aria-label="运行命令" :disabled="blocked || !terminal.draft.trim()" @click="close(); terminal.run()"><Play :size="14" />运行</button>
    </div>
    <p>Enter 执行 · Tab 补全 · ↑↓ 历史 · 运行时在终端内输入，Ctrl+C 中断</p>
  </div>
</template>

<style scoped>
.terminal-command { position: relative; flex-shrink: 0; padding: 12px 16px 8px; background: var(--surface-panel); border-top: 1px solid var(--border); }
.input-row { display: flex; gap: 10px; align-items: center; }
.prompt { color: var(--primary); font: bold 20px var(--font-code); }
input { width: 100%; min-width: 0; flex: 1; padding: 8px 2px; border: 0; outline: 0; background: transparent; color: var(--text); font: 13px var(--font-code); }
.run { display: inline-flex; align-items: center; gap: 5px; padding: 7px 12px; background: var(--primary); border-radius: var(--radius-md); color: white; }
p { margin: 5px 0 0 20px; color: var(--text-muted); font-size: 11px; line-height: 1.6; }
.completion-menu { position: absolute; bottom: 100%; left: 16px; right: 16px; max-height: min(260px, 35vh); overflow: auto; z-index: 10; padding: 5px; border: 1px solid var(--border); border-radius: var(--radius-md); box-shadow: var(--shadow-window); background: var(--surface-panel); }
.completion-menu button { display: flex; align-items: baseline; justify-content: space-between; gap: 14px; width: 100%; padding: 8px; border-radius: var(--radius-sm); background: transparent; text-align: left; }
.completion-menu button.selected, .completion-menu button:hover { background: var(--primary-soft); color: var(--primary); }
.completion-menu code { min-width: 0; overflow-wrap: anywhere; font: 12px var(--font-code); }
.completion-menu span, .completion-menu small { color: var(--text-muted); font-size: 11px; }
</style>
