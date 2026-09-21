import { t } from '@/lib/i18n';
import { computed, ref, watch } from "vue";
import { defineStore } from "pinia";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import { reportGitFailure } from "@/lib/gitFailure";
import { formatDisplayPath } from "@/lib/formatPath";
import type { BackendError, TerminalEvent } from "@/lib/backend/types";
import type { TerminalScreen } from "@/features/terminal/terminalScreen";
import { useRepositoryStore } from "./repository";
import { useConflictsStore } from "./conflicts";

export type TerminalStatus = "idle" | "starting" | "running" | "completed" | "failed" | "cancelled";
export interface TerminalHistoryEntry { runId: string; command: string; startedAt: number; status: TerminalStatus; durationMs: number | null }
type Exit = Extract<TerminalEvent["event"], { kind: "exited" }>;
interface Owner { runId: string; root: string; generation: number; command: string; startedAt: number; dispatched: boolean; accepted: boolean; terminal: boolean; sequence: number; output: Promise<void>; input: Promise<void>; cancellation?: Promise<void>; decoder?: TextDecoder; diagnosticOutput?: string }
const HISTORY_LIMIT = 20;
const INPUT_CHUNK = 4096;
function sameRoot(a: string, b: string): boolean {
  const normalize = (value: string) => formatDisplayPath(value).replace(/\\/g, "/").replace(/\/$/, "");
  const left = normalize(a), right = normalize(b);
  return /^[a-z]:\//i.test(left) || left.startsWith("//") ? left.toLowerCase() === right.toLowerCase() : left === right;
}

export const useTerminalStore = defineStore("terminal", () => {
  const draft = ref("");
  const runId = ref<string>();
  const status = ref<TerminalStatus>("idle");
  const error = ref<BackendError>();
  const refreshError = ref<BackendError>();
  const result = ref<Exit>();
  const history = ref<TerminalHistoryEntry[]>([]);
  const cancelRequested = ref(false);
  const refreshing = ref(false);
  const resetting = ref(false);
  const screenReady = ref(false);
  const running = computed(() => status.value === "starting" || status.value === "running");
  const busy = computed(() => running.value || refreshing.value || resetting.value);
  let owner: Owner | undefined;
  let screen: TerminalScreen | undefined;
  let creatingScreen: Promise<TerminalScreen> | undefined;
  let unlisten: (() => void) | undefined;
  let listening: Promise<void> | undefined;
  let lifetime = 0;

  function promptLabel(): string {
    const repository = useRepositoryStore().snapshot;
    return `[${repository?.name ?? "repository"} ${repository?.currentBranch || "detached"}]$ `;
  }
  function syncPrompt(): void { screen?.setPrompt(busy.value ? null : promptLabel(), draft.value); }
  watch([busy, draft, () => useRepositoryStore().snapshot?.rootPath, () => useRepositoryStore().snapshot?.currentBranch], syncPrompt, { flush: "sync" });

  function owns(candidate: Owner): boolean {
    const repo = useRepositoryStore();
    return owner === candidate && candidate.generation === repo.generation && !!repo.snapshot && sameRoot(candidate.root, repo.snapshot.rootPath);
  }
  function initialize(): Promise<void> {
    if (unlisten) return Promise.resolve();
    if (listening) return listening;
    const epoch = lifetime;
    const pending = backendClient.terminalListen(handleEvent).then(stop => { if (epoch !== lifetime) stop(); else unlisten = stop; }).finally(() => { if (listening === pending) listening = undefined; });
    listening = pending; return pending;
  }
  async function ensureScreen(): Promise<TerminalScreen> {
    if (screen) return screen;
    if (creatingScreen) return creatingScreen;
    const epoch = lifetime;
    const pending = import("@/features/terminal/terminalScreen").then(module => module.createTerminalScreen(data => { void sendInput(data); }, (cols, rows) => { void resize(cols, rows); }, {
      change: command => { draft.value = command; }, submit: () => { void run(); },
      blocked: () => busy.value || useRepositoryStore().otherOperationBusy || !useRepositoryStore().snapshot,
      history: () => history.value.map(entry => entry.command),
      complete: async (command, cursor) => {
        const repo = useRepositoryStore(), root = repo.snapshot?.rootPath, generation = repo.generation;
        if (!root) throw new Error(t('uiOpenARepositoryFirst4f776e'));
        const completion = await backendClient.terminalComplete(root, command, cursor);
        if (repo.snapshot?.rootPath !== root || repo.generation !== generation) throw new Error(t('uiRepositoryChangedf8a15c'));
        return completion;
      },
    })).then(value => {
      if (epoch !== lifetime) { value.dispose(); throw new Error(t('uiTerminalSessionClosed7cff3e')); }
      screen = value; screenReady.value = true; syncPrompt(); return value;
    }).finally(() => { if (creatingScreen === pending) creatingScreen = undefined; });
    creatingScreen = pending; return pending;
  }
  async function attach(host: HTMLElement): Promise<void> { const current = await ensureScreen(); if (host.isConnected) { current.attach(host); current.focus(); } }
  function detach(): void { screen?.detach(); }
  function focus(): void { screen?.focus(); }
  function clear(): void { if (!busy.value) screen?.clear(); }
  function outputText(): string { return screen?.text() ?? ""; }
  function accept(candidate: Owner): void {
    if (candidate.accepted) return;
    candidate.accepted = true;
    history.value.unshift({ runId: candidate.runId, command: candidate.command, startedAt: candidate.startedAt, status: candidate.terminal ? status.value : "running", durationMs: result.value?.durationMs ?? null });
    history.value = history.value.slice(0, HISTORY_LIMIT);
  }
  function updateHistory(candidate: Owner): void {
    const entry = history.value.find(item => item.runId === candidate.runId);
    if (entry) { entry.status = status.value; entry.durationMs = result.value?.durationMs ?? null; }
  }
  async function run(): Promise<void> {
    const repo = useRepositoryStore();
    if (busy.value || repo.navigationBusy || !draft.value.trim()) return;
    if (!repo.snapshot) { error.value = { code: "invalidRepository", get message() { return t('uiOpenAGitRepositoryFirsta00a3e'); } }; return; }
    if (useConflictsStore().hasDirtyDrafts) { error.value = { code: "gitOperationInProgress", get message() { return t('uiSaveOrDiscardConflictDraftsBeforeRunningGitCommandsa785fb'); } }; return; }
    const candidate: Owner = { runId: crypto.randomUUID(), root: repo.snapshot.rootPath, generation: repo.generation, command: draft.value, startedAt: Date.now(), dispatched: false, accepted: false, terminal: false, sequence: 0, output: Promise.resolve(), input: Promise.resolve() };
    owner = candidate; runId.value = candidate.runId; status.value = "starting"; error.value = undefined; refreshError.value = undefined; result.value = undefined; cancelRequested.value = false;
    draft.value = "";
    try {
      await initialize(); const current = await ensureScreen();
      if (!owns(candidate) || candidate.terminal) return;
      await current.commitPrompt(promptLabel(), candidate.command);
      if (!owns(candidate) || candidate.terminal) return;
      candidate.dispatched = true;
      const accepted = await backendClient.terminalStart(candidate.root, candidate.runId, candidate.command, current.cols, current.rows);
      if (!owns(candidate)) return;
      if (accepted.runId !== candidate.runId || !sameRoot(accepted.rootPath, candidate.root)) throw new Error(t('uiTheTerminalReturnedAMismatchedSessionfbbe5e'));
      accept(candidate); if (!candidate.terminal) { status.value = "running"; current.focus(); }
    } catch (cause) {
      if (!owns(candidate) || candidate.terminal) return;
      if (candidate.accepted) {
        error.value = normalizeBackendError(cause);
        await terminate();
        return;
      }
      candidate.terminal = true;
      error.value = normalizeBackendError(cause); status.value = error.value.code === "cancelled" ? "cancelled" : "failed"; cancelRequested.value = false; updateHistory(candidate);
      reportGitFailure({ root: candidate.root, command: candidate.command, error: error.value });
    }
  }
  function handleEvent(message: TerminalEvent): void {
    const candidate = owner;
    if (!candidate || !owns(candidate) || candidate.terminal || message.runId !== candidate.runId || !sameRoot(message.rootPath, candidate.root) || message.sequence <= candidate.sequence) return;
    candidate.sequence = message.sequence;
    const event = message.event;
    if (event.kind === "started") { accept(candidate); status.value = "running"; }
    else if (event.kind === "output") {
      accept(candidate);
      candidate.output = candidate.output.then(async () => {
        if (!owns(candidate)) return;
        const bytes = Uint8Array.from(atob(event.data), char => char.charCodeAt(0));
        candidate.decoder ??= new TextDecoder();
        candidate.diagnosticOutput = ((candidate.diagnosticOutput ?? "") + candidate.decoder.decode(bytes, { stream: true })).slice(-12_000);
        await (await ensureScreen()).write(bytes);
        if (owns(candidate)) await backendClient.terminalAck(candidate.root, candidate.runId, message.sequence);
      }).catch(async cause => {
        if (!owns(candidate)) return;
        error.value = normalizeBackendError(cause);
        await terminate();
      });
    } else {
      candidate.terminal = true;
      candidate.output = candidate.output.then(async () => {
        if (!owns(candidate)) return;
        refreshing.value = true;
        result.value = event; status.value = event.cancelled ? "cancelled" : event.error || event.exitCode !== 0 ? "failed" : "completed";
        error.value = event.error ?? undefined; cancelRequested.value = false; refreshing.value = true; updateHistory(candidate);
        if (status.value === "failed") reportGitFailure({ root: candidate.root, command: candidate.command,
          error: event.error ?? { code: "gitCommandFailed", message: t('msgGitCommandFailedExitCode9ed91f', { p0: event.exitCode ?? t('uiUnknownd9c32a') }) },
          output: candidate.diagnosticOutput });
        await screen?.restoreInput();
        if (owns(candidate)) await refreshOwner(candidate);
      }).catch(cause => { if (owns(candidate)) { refreshing.value = false; refreshError.value = normalizeBackendError(cause); } });
    }
  }
  async function refreshOwner(candidate: Owner): Promise<void> {
    refreshing.value = true; refreshError.value = undefined;
    try { await useRepositoryStore().refreshAfterTerminal(candidate.root, candidate.generation); }
    catch (cause) { if (owns(candidate)) refreshError.value = normalizeBackendError(cause); }
    finally { if (owns(candidate)) refreshing.value = false; }
  }
  async function retryRefresh(): Promise<void> { if (owner && !running.value && !refreshing.value) await refreshOwner(owner); }
  async function sendInput(data: string): Promise<void> {
    const candidate = owner;
    if (!candidate || !owns(candidate) || !candidate.dispatched || candidate.terminal) return;
    const bytes = new TextEncoder().encode(data);
    candidate.input = candidate.input.then(async () => {
      for (let offset = 0; offset < bytes.length; offset += INPUT_CHUNK) {
        if (!owns(candidate) || candidate.terminal) return;
        await backendClient.terminalWrite(candidate.root, candidate.runId, btoa(String.fromCharCode(...bytes.subarray(offset, offset + INPUT_CHUNK))));
      }
    }).catch(cause => { if (owns(candidate) && !candidate.terminal) error.value = normalizeBackendError(cause); });
    await candidate.input;
  }
  async function resize(cols: number, rows: number): Promise<void> {
    const candidate = owner;
    if (!candidate || !owns(candidate) || !candidate.dispatched || candidate.terminal) return;
    try { await backendClient.terminalResize(candidate.root, candidate.runId, cols, rows); }
    catch (cause) { if (owns(candidate) && !candidate.terminal) error.value = normalizeBackendError(cause); }
  }
  async function terminateOwner(candidate: Owner): Promise<void> {
    if (candidate.terminal || !candidate.dispatched) { candidate.terminal = true; return; }
    if (candidate.cancellation) return candidate.cancellation;
    const pending = backendClient.terminalTerminate(candidate.root, candidate.runId).finally(() => { if (candidate.cancellation === pending) candidate.cancellation = undefined; });
    candidate.cancellation = pending; await pending;
  }
  async function terminate(): Promise<void> {
    const candidate = owner; if (!candidate || !running.value) return;
    cancelRequested.value = true;
    try { await terminateOwner(candidate); if (owns(candidate) && !candidate.dispatched) { status.value = "cancelled"; cancelRequested.value = false; } }
    catch (cause) { if (owns(candidate)) { error.value = normalizeBackendError(cause); cancelRequested.value = false; } }
  }
  async function resetForRepository(): Promise<void> {
    const previous = owner;
    resetting.value = true;
    try {
      if (previous && !previous.terminal) await terminateOwner(previous);
      owner = undefined; lifetime++; screen?.dispose(); screen = undefined; creatingScreen = undefined; screenReady.value = false;
      unlisten?.(); unlisten = undefined; listening = undefined;
      status.value = "idle"; runId.value = undefined; result.value = undefined; error.value = undefined; refreshError.value = undefined; history.value = []; draft.value = ""; refreshing.value = false; cancelRequested.value = false;
    } finally { resetting.value = false; }
  }
  function recall(entry: TerminalHistoryEntry): void { if (!busy.value) { draft.value = entry.command; focus(); } }
  function clearHistory(): void { if (!busy.value) history.value = []; }
  function dispose(): void { lifetime++; unlisten?.(); unlisten = undefined; listening = undefined; void resetForRepository().catch(cause => { error.value = normalizeBackendError(cause); }); }
  return { draft, runId, status, error, refreshError, result, history, cancelRequested, refreshing, resetting, screenReady, running, busy, initialize, attach, detach, focus, clear, outputText, run, handleEvent, sendInput, resize, terminate, resetForRepository, retryRefresh, recall, clearHistory, dispose };
});
