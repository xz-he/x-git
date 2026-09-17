import { flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setBackendClientForTests } from "@/lib/backend/client";
import type { TerminalAccepted, TerminalEvent } from "@/lib/backend/types";
import { createBackendFixture } from "@/test/backend";
import { useTerminalStore } from "./terminal";
import { useRepositoryStore } from "./repository";
import { onGitFailure } from "@/lib/gitFailure";

const screen = vi.hoisted(() => ({ write: vi.fn(async (): Promise<void> => {}), attach: vi.fn(), detach: vi.fn(), focus: vi.fn(), reset: vi.fn(), clear: vi.fn(), dispose: vi.fn(), text: vi.fn(() => "output"), cols: 80, rows: 24 }));
vi.mock("@/features/terminal/terminalScreen", () => ({ createTerminalScreen: vi.fn(async () => screen) }));
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (cause: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const exited: TerminalEvent["event"] = { kind: "exited", exitCode: 0, durationMs: 10, cancelled: false, error: null };

describe("interactive terminal lifecycle", () => {
  let backend: ReturnType<typeof createBackendFixture>;
  let emit: (event: TerminalEvent) => void;
  let refresh: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    setActivePinia(createPinia());
    screen.write.mockReset().mockResolvedValue(undefined);
    backend = createBackendFixture({ terminalListen: vi.fn(async listener => { emit = listener; return vi.fn(); }), terminalStart: vi.fn(async (rootPath, runId) => ({ rootPath, runId })) });
    setBackendClientForTests(backend);
    const repo = useRepositoryStore();
    repo.snapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abc", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null };
    refresh = vi.spyOn(repo, "refreshAfterTerminal").mockResolvedValue(undefined);
  });
  function send(event: TerminalEvent["event"], sequence = 1, runId = useTerminalStore().runId!, rootPath = "C:/repo") { emit({ event, sequence, runId, rootPath }); }

  it("subscribes before starting and disallows duplicate runs", async () => {
    const pending = deferred<() => void>();
    vi.mocked(backend.terminalListen).mockReturnValueOnce(pending.promise);
    const store = useTerminalStore();
    const starting = store.run();
    await store.run();
    expect(backend.terminalStart).not.toHaveBeenCalled();
    pending.resolve(() => undefined);
    await starting;
    expect(backend.terminalStart).toHaveBeenCalledTimes(1);
    expect(store.running).toBe(true);
  });

  it("reports only the failed command output and does not report success or cancellation", async () => {
    const listener = vi.fn(); const stop = onGitFailure(listener);
    try {
      const store = useTerminalStore(); await store.run();
      send({ kind: "output", data: btoa("previous success") }); send(exited, 2); await flushPromises();
      expect(listener).not.toHaveBeenCalled();
      store.draft = "git bad-command"; await store.run();
      send({ kind: "output", data: btoa("fatal: bad command") }); send({ ...exited, kind: "exited", exitCode: 1 }, 2); await flushPromises();
      expect(listener).toHaveBeenCalledWith(expect.objectContaining({ command: "git bad-command", output: "fatal: bad command" }));
      listener.mockClear(); await store.run(); send({ ...exited, kind: "exited", exitCode: 1, cancelled: true }); await flushPromises();
      expect(listener).not.toHaveBeenCalled();
    } finally { stop(); }
  });

  it("acknowledges output only after consumption and refreshes after exit", async () => {
    const store = useTerminalStore(); await store.run();
    const consumed = deferred<void>(); screen.write.mockReturnValueOnce(consumed.promise);
    send({ kind: "output", data: btoa("hello") });
    await flushPromises();
    expect(backend.terminalAck).not.toHaveBeenCalled();
    send(exited, 2);
    expect(store.busy).toBe(true);
    consumed.resolve(); await flushPromises();
    expect(backend.terminalAck).toHaveBeenCalledWith("C:/repo", store.runId, 1);
    expect(store.status).toBe("completed");
    expect(store.busy).toBe(false);
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("does not let late acceptance or output override a terminal result", async () => {
    const accepted = deferred<TerminalAccepted>(); vi.mocked(backend.terminalStart).mockReturnValueOnce(accepted.promise);
    const store = useTerminalStore(); const starting = store.run(); await flushPromises();
    send(exited); send({ kind: "output", data: btoa("late") }, 2);
    accepted.resolve({ rootPath: "C:/repo", runId: store.runId! }); await starting; await flushPromises();
    expect(store.status).toBe("completed"); expect(store.history).toHaveLength(1);
    expect(backend.terminalAck).not.toHaveBeenCalled();
  });

  it("ignores wrong owners and duplicate sequences", async () => {
    const store = useTerminalStore(); await store.run();
    send({ kind: "output", data: btoa("wrong") }, 1, "old");
    send({ kind: "output", data: btoa("wrong") }, 1, store.runId, "C:/other");
    send({ kind: "output", data: btoa("ok") }, 1);
    send({ kind: "output", data: btoa("duplicate") }, 1);
    await flushPromises(); expect(backend.terminalAck).toHaveBeenCalledTimes(1);
  });
  it("matches canonical Windows UNC event paths to the requested repository", async () => {
    useRepositoryStore().snapshot!.rootPath = String.raw`\\server\share\repo`;
    const store = useTerminalStore(); await store.run();
    send({ kind: "output", data: btoa("UNC") }, 1, store.runId, String.raw`\\?\UNC\server\share\repo`);
    await flushPromises(); expect(backend.terminalAck).toHaveBeenCalledOnce();
  });

  it("keeps refresh failures separate from Git success and permits retry", async () => {
    const store = useTerminalStore(); await store.run();
    refresh.mockRejectedValueOnce(new Error("refresh failed")); send(exited); await flushPromises();
    expect(store.status).toBe("completed"); expect(store.refreshError).toBeDefined();
    await store.retryRefresh(); expect(store.refreshError).toBeUndefined();
  });

  it("does not pretend termination succeeded when backend cleanup fails", async () => {
    const store = useTerminalStore(); await store.run();
    vi.mocked(backend.terminalTerminate).mockRejectedValueOnce(new Error("still running"));
    await store.terminate();
    expect(store.running).toBe(true); expect(store.cancelRequested).toBe(false); expect(store.error).toBeDefined();
    await store.terminate(); send({ ...exited, kind: "exited", cancelled: true }, 2); await flushPromises();
    expect(store.status).toBe("cancelled");
  });

  it("cleans up on repository replacement and refuses stale output", async () => {
    const store = useTerminalStore(); await store.run(); const previous = store.runId!;
    await store.resetForRepository();
    send({ kind: "output", data: btoa("late") }, 1, previous); await flushPromises();
    expect(backend.terminalTerminate).toHaveBeenCalledWith("C:/repo", previous);
    expect(store.history).toEqual([]); expect(store.status).toBe("idle");
    expect(backend.terminalAck).not.toHaveBeenCalled();
  });

  it("does not resurrect a rejected start on a late event", async () => {
    vi.mocked(backend.terminalStart).mockRejectedValueOnce({ code: "invalidConsoleCommand", message: "invalid" });
    const store = useTerminalStore(); await store.run();
    send({ kind: "started" });
    expect(store.status).toBe("failed"); expect(store.history).toHaveLength(0);
  });

  it("cancels initialization before dispatch without starting a process", async () => {
    const pending = deferred<() => void>();
    vi.mocked(backend.terminalListen).mockReturnValueOnce(pending.promise);
    const store = useTerminalStore(); const starting = store.run();
    await store.terminate(); pending.resolve(() => undefined); await starting;
    expect(store.status).toBe("cancelled"); expect(backend.terminalStart).not.toHaveBeenCalled();
  });
});
