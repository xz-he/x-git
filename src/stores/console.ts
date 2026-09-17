import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import { reportGitFailure } from "@/lib/gitFailure";
import type { BackendError, ConsoleEvent, ConsoleOutcome } from "@/lib/backend/types";
import { useRepositoryStore } from "@/stores/repository";

const DEFAULT_COMMAND = "git status";
const HISTORY_LIMIT = 20;
// UTF-8 replacement/redaction can expand the raw 512 KiB stream budget.
const DISPLAY_CHARACTER_LIMIT = 3 * 512 * 1024;
export type ConsoleStatus = "idle" | "starting" | "running" | ConsoleOutcome;
type Terminal = Extract<ConsoleEvent["event"], { kind: "terminal" }>;
export interface ConsoleHistoryEntry {
  runId: string; command: string; startedAt: number; status: ConsoleStatus; durationMs: number | null;
}
interface Owner {
  runId: string; rootPath: string; generation: number; command: string; startedAt: number;
  dispatched: boolean; accepted: boolean; rejected: boolean; terminal: boolean;
  cancellation?: Promise<void>;
}

export const useConsoleStore = defineStore("console", () => {
  const draft = ref(DEFAULT_COMMAND);
  const runId = ref<string>();
  const status = ref<ConsoleStatus>("idle");
  const stdout = ref("");
  const stderr = ref("");
  const error = ref<BackendError>();
  const result = ref<Terminal>();
  const history = ref<ConsoleHistoryEntry[]>([]);
  const cancelRequested = ref(false);
  const settling = ref(false);
  const displayTruncated = ref(false);
  const running = computed(() => status.value === "starting" || status.value === "running");
  const busy = computed(() => running.value || settling.value);
  const outputText = computed(() => stdout.value + stderr.value);
  let owner: Owner | undefined;
  let retired: Owner | undefined;
  let sequence = 0;
  let unlisten: (() => void) | undefined;
  let listening: Promise<void> | undefined;
  let lifetime = 0;

  function initialize(): Promise<void> {
    if (unlisten) return Promise.resolve();
    if (listening) return listening;
    const epoch = lifetime;
    const pending = backendClient.consoleListen(handleEvent).then(stop => {
      if (epoch !== lifetime) stop();
      else unlisten = stop;
    }).finally(() => {
      if (listening === pending) listening = undefined;
    });
    listening = pending;
    return pending;
  }

  function owns(candidate: Owner): boolean {
    const repository = useRepositoryStore();
    return owner === candidate && candidate.rootPath === repository.snapshot?.rootPath &&
      candidate.generation === repository.generation;
  }

  // Repository generation advances when an open is attempted. The retained
  // console remains owned until that open succeeds, including events in flight.
  function preserveForOpen(generation: number): void {
    if (owner) owner.generation = generation;
  }

  function accepted(candidate: Owner): void {
    if (candidate.accepted) return;
    candidate.accepted = true;
    history.value.unshift({
      runId: candidate.runId, command: candidate.command, startedAt: candidate.startedAt,
      status: candidate.terminal ? status.value : "running", durationMs: result.value?.durationMs ?? null,
    });
    history.value = history.value.slice(0, HISTORY_LIMIT);
  }

  async function run(): Promise<void> {
    const repository = useRepositoryStore();
    if (busy.value || repository.operation.kind !== "idle") return;
    if (!repository.snapshot) {
      error.value = { code: "invalidRepository", message: "请先打开 Git 仓库。" };
      return;
    }
    const candidate: Owner = {
      runId: crypto.randomUUID(), rootPath: repository.snapshot.rootPath,
      generation: repository.generation, command: draft.value, startedAt: Date.now(),
      dispatched: false, accepted: false, rejected: false, terminal: false,
    };
    owner = candidate;
    runId.value = candidate.runId;
    status.value = "starting";
    stdout.value = "";
    stderr.value = "";
    error.value = undefined;
    result.value = undefined;
    displayTruncated.value = false;
    cancelRequested.value = false;
    sequence = 0;
    try {
      await initialize();
      if (!owns(candidate) || candidate.terminal) return;
      candidate.dispatched = true;
      const response = await backendClient.consoleStart(candidate.rootPath, candidate.runId, candidate.command);
      if (!owns(candidate)) return;
      if (response.runId !== candidate.runId || !sameRoot(response.rootPath, candidate.rootPath)) {
        throw { code: "unexpected", message: "Git 控制台返回了不匹配的任务信息。" };
      }
      accepted(candidate);
      if (!candidate.terminal) status.value = "running";
    } catch (cause) {
      if (!owns(candidate) || candidate.terminal) return;
      error.value = normalizeBackendError(cause);
      status.value = error.value.code === "cancelled" ? "cancelled" : "failed";
      reportGitFailure({ root: candidate.rootPath, command: candidate.command, error: error.value });
      // A rejected start may still have a registered worker's terminal in flight.
      // Retain terminal authority without accepting late execution events.
      candidate.rejected = true;
      cancelRequested.value = false;
      updateHistory(candidate);
    }
  }

  function updateHistory(candidate: Owner): void {
    const entry = history.value.find(item => item.runId === candidate.runId);
    if (entry) {
      entry.status = status.value;
      entry.durationMs = result.value?.durationMs ?? null;
    }
  }

  function handleEvent(message: ConsoleEvent): void {
    const candidate = owner;
    if (!candidate || !owns(candidate) || candidate.terminal || message.runId !== candidate.runId ||
      !sameRoot(message.rootPath, candidate.rootPath) || message.sequence <= sequence) return;
    if (candidate.rejected && message.event.kind !== "terminal") return;
    sequence = message.sequence;
    const payload = message.event;
    // A registered attempt may terminate before root validation rejects start.
    // Only execution events or an acceptance response establish history.
    if (payload.kind !== "terminal") accepted(candidate);
    if (payload.kind === "started") {
      status.value = "running";
    } else if (payload.kind === "output") {
      const target = payload.stream === "stdout" ? stdout : stderr;
      const remaining = Math.max(0, DISPLAY_CHARACTER_LIMIT - target.value.length);
      target.value += payload.text.slice(0, remaining);
      displayTruncated.value ||= payload.text.length > remaining;
      status.value = "running";
    } else {
      result.value = payload;
      status.value = payload.outcome;
      error.value = payload.error ?? undefined;
      if (payload.outcome === "failed") reportGitFailure({ root: candidate.rootPath, command: candidate.command,
        error: payload.error ?? { code: "gitCommandFailed", message: `Git 命令执行失败，退出码：${payload.exitCode}` },
        output: (stderr.value || stdout.value).slice(-12_000) });
      candidate.terminal = true;
      cancelRequested.value = false;
      updateHistory(candidate);
    }
  }

  function cancelOwner(candidate: Owner): Promise<void> {
    if (candidate.cancellation) return candidate.cancellation;
    if (!candidate.dispatched || candidate.terminal) {
      candidate.terminal = true;
      return Promise.resolve();
    }
    const pending = backendClient.consoleCancel(candidate.rootPath, candidate.runId).finally(() => {
      if (candidate.cancellation === pending) candidate.cancellation = undefined;
    });
    candidate.cancellation = pending;
    return pending;
  }

  async function cancel(): Promise<void> {
    if (retired) {
      await finishRetired(retired);
      return;
    }
    const candidate = owner;
    if (!candidate || !running.value) return;
    cancelRequested.value = true;
    try {
      await cancelOwner(candidate);
      if (owns(candidate) && !candidate.dispatched) {
        status.value = "cancelled";
        cancelRequested.value = false;
      }
    } catch (cause) {
      if (owns(candidate) && !candidate.terminal) {
        error.value = normalizeBackendError(cause);
        cancelRequested.value = false;
      }
    }
  }

  async function finishRetired(candidate: Owner): Promise<void> {
    try {
      await cancelOwner(candidate);
      if (retired === candidate) {
        retired = undefined;
        settling.value = false;
        cancelRequested.value = false;
        error.value = undefined;
      }
    } catch (cause) {
      if (retired === candidate) {
        error.value = normalizeBackendError(cause);
        cancelRequested.value = false;
      }
    }
  }

  async function resetForRepository(): Promise<void> {
    const previous = owner;
    owner = undefined;
    runId.value = undefined;
    status.value = "idle";
    sequence = 0;
    draft.value = DEFAULT_COMMAND;
    history.value = [];
    stdout.value = "";
    stderr.value = "";
    result.value = undefined;
    error.value = undefined;
    displayTruncated.value = false;
    if (previous && !previous.terminal) retired = previous;
    if (retired) {
      settling.value = true;
      cancelRequested.value = true;
      await finishRetired(retired);
    }
  }

  function recall(entry: ConsoleHistoryEntry): void {
    if (!busy.value) draft.value = entry.command;
  }
  function clearHistory(): void {
    if (!busy.value) history.value = [];
  }
  function dispose(): void {
    lifetime += 1;
    unlisten?.();
    unlisten = undefined;
    listening = undefined;
    void resetForRepository();
  }
  return { draft, runId, status, stdout, stderr, error, result, history, cancelRequested, settling,
    displayTruncated, running, busy, outputText, initialize, run, handleEvent, cancel,
    preserveForOpen, resetForRepository, recall, clearHistory, dispose };
});

// Tauri canonical Windows paths may carry a verbatim prefix and backslashes.
function sameRoot(left: string, right: string): boolean {
  const normalize = (value: string) => value.replace(/^\\\\\?\\/, "").replace(/\\/g, "/").replace(/\/$/, "");
  const a = normalize(left), b = normalize(right);
  return /^[a-z]:\//i.test(a) || a.startsWith("//") ? a.toLowerCase() === b.toLowerCase() : a === b;
}
