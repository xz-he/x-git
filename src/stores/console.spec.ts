import { flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setBackendClientForTests, type BackendClient } from "@/lib/backend/client";
import type { ConsoleAccepted, ConsoleEvent, RepositorySnapshot } from "@/lib/backend/types";
import { createBackendFixture } from "@/test/backend";
import { useConsoleStore } from "@/stores/console";
import { useRepositoryStore } from "@/stores/repository";
import { useUiStore } from "@/stores/ui";

const repository: RepositorySnapshot = { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abc1234", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null };
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const terminal: ConsoleEvent["event"] = { kind: "terminal", outcome: "completed", exitCode: 0, durationMs: 12, stdoutTruncated: false, stderrTruncated: false };

describe("console session", () => {
  let backend: BackendClient;
  let emit: (event: ConsoleEvent) => void;
  beforeEach(() => {
    setActivePinia(createPinia());
    backend = createBackendFixture({
      consoleListen: vi.fn(async listener => { emit = listener; return vi.fn(); }),
      consoleStart: vi.fn(async (path, runId) => ({ runId, rootPath: path })),
    });
    setBackendClientForTests(backend);
    useRepositoryStore().snapshot = { ...repository };
    useRepositoryStore().generation = 1;
  });
  function send(event: ConsoleEvent["event"], sequence = 1, runId = useConsoleStore().runId!, rootPath = repository.rootPath) {
    emit({ runId, rootPath, sequence, event });
  }
  it("subscribes before start and prevents duplicate runs", async () => {
    const store = useConsoleStore();
    const subscribed = deferred<() => void>();
    vi.mocked(backend.consoleListen).mockReturnValueOnce(subscribed.promise);
    store.draft = "git status";
    const start = store.run();
    await store.run();
    expect(backend.consoleStart).not.toHaveBeenCalled();
    subscribed.resolve(() => undefined);
    await start;
    expect(backend.consoleStart).toHaveBeenCalledTimes(1);
    expect(store.running).toBe(true);
  });
  it("retains stream text and terminal data, ignores duplicate/late/wrong owner events", async () => {
    const store = useConsoleStore();
    await store.run();
    send({ kind: "output", stream: "stdout", text: "中文\n\n" });
    send({ kind: "output", stream: "stderr", text: "warning\n" }, 2);
    send({ kind: "output", stream: "stdout", text: "duplicate" }, 2);
    send({ kind: "output", stream: "stdout", text: "wrong" }, 3, "other");
    send({ kind: "output", stream: "stdout", text: "wrong root" }, 3, store.runId, "C:/other");
    send(terminal, 4);
    send({ kind: "started" }, 5);
    send({ kind: "output", stream: "stdout", text: "late" }, 6);
    expect(store.outputText).toBe("中文\n\nwarning\n");
    expect(store.status).toBe("completed");
    expect(store.result?.exitCode).toBe(0);
    expect(store.history).toHaveLength(1);
    expect(store.history[0]?.status).toBe("completed");
  });
  it("terminal wins over a delayed start acceptance", async () => {
    const pending = deferred<ConsoleAccepted>();
    vi.mocked(backend.consoleStart).mockReturnValueOnce(pending.promise);
    const store = useConsoleStore();
    const start = store.run();
    await flushPromises();
    send(terminal);
    pending.resolve({ runId: store.runId!, rootPath: repository.rootPath });
    await start;
    expect(store.status).toBe("completed");
    expect(store.history).toHaveLength(1);
  });
  it("listener and policy failures never create accepted history", async () => {
    const store = useConsoleStore();
    vi.mocked(backend.consoleListen).mockRejectedValueOnce({ code: "unexpected", message: "无法订阅" });
    await store.run();
    expect(backend.consoleStart).not.toHaveBeenCalled();
    expect(store.error?.message).toBe("无法订阅");
    vi.mocked(backend.consoleStart).mockRejectedValueOnce({ code: "invalidConsoleCommand", message: "不支持写命令" });
    await store.run();
    expect(store.error?.message).toBe("不支持写命令");
    expect(store.history).toEqual([]);
  });
  it("a terminal preceding a rejected start does not create accepted history", async () => {
    const pending = deferred<ConsoleAccepted>();
    vi.mocked(backend.consoleStart).mockReturnValueOnce(pending.promise);
    const store = useConsoleStore();
    const start = store.run();
    await flushPromises();
    const failure = { code: "invalidRepository" as const, message: "仓库目录已失效" };
    send({ ...terminal, kind: "terminal", outcome: "failed", exitCode: null, error: failure });
    pending.reject(failure);
    await start;
    expect(store.status).toBe("failed");
    expect(store.error).toEqual(failure);
    expect(store.history).toEqual([]);
  });
  it("cancellation is idempotent, retryable on rejection, and waits for terminal", async () => {
    const store = useConsoleStore();
    await store.run();
    vi.mocked(backend.consoleCancel).mockRejectedValueOnce({ code: "io", message: "取消失败" });
    await store.cancel();
    expect(store.running).toBe(true);
    expect(store.cancelRequested).toBe(false);
    const pending = deferred<void>();
    vi.mocked(backend.consoleCancel).mockReturnValueOnce(pending.promise);
    const cancelled = store.cancel();
    void store.cancel();
    expect(backend.consoleCancel).toHaveBeenCalledTimes(2);
    pending.resolve();
    await cancelled;
    expect(store.running).toBe(true);
    send({ ...terminal, kind: "terminal", outcome: "cancelled", exitCode: null });
    expect(store.status).toBe("cancelled");
  });
  it("shows cancellation when cancel wins before start registration and no terminal exists", async () => {
    const pending = deferred<ConsoleAccepted>();
    vi.mocked(backend.consoleStart).mockReturnValueOnce(pending.promise);
    const store = useConsoleStore();
    const starting = store.run();
    await flushPromises();
    await store.cancel();
    pending.reject({ code: "cancelled", message: "任务在登记前已取消" });
    await starting;
    expect(store.status).toBe("cancelled");
    expect(store.running).toBe(false);
    expect(store.history).toEqual([]);
  });
  it("accepts authoritative timeout after startup rejection without reviving execution or history", async () => {
    const pending = deferred<ConsoleAccepted>();
    vi.mocked(backend.consoleStart).mockReturnValueOnce(pending.promise);
    const store = useConsoleStore();
    const starting = store.run();
    await flushPromises();
    pending.reject({ code: "cancelled", message: "根路径校验超时" });
    await starting;
    send({ kind: "started" }, 1);
    send({ kind: "output", stream: "stdout", text: "late" }, 2);
    send({ ...terminal, kind: "terminal", outcome: "timedOut", exitCode: null }, 3);
    expect(store.status).toBe("timedOut");
    expect(store.result?.durationMs).toBe(12);
    expect(store.outputText).toBe("");
    expect(store.history).toEqual([]);
  });
  it("caps history at 20, retains only latest output and recalls without running", async () => {
    const store = useConsoleStore();
    for (let i = 0; i < 22; i++) {
      store.draft = i % 2 ? "git log --oneline" : "git status";
      await store.run();
      send({ kind: "output", stream: "stdout", text: String(i) });
      send(terminal, 2);
    }
    expect(store.history).toHaveLength(20);
    expect(store.outputText).toBe("21");
    const calls = vi.mocked(backend.consoleStart).mock.calls.length;
    store.recall(store.history[1]!);
    expect(store.draft).toBe("git status");
    expect(backend.consoleStart).toHaveBeenCalledTimes(calls);
    store.clearHistory();
    expect(store.history).toEqual([]);
  });
  it("preserves session across navigation and failed open including events during the attempt", async () => {
    const store = useConsoleStore();
    await store.run();
    store.draft = "git diff";
    useUiStore().openView("history");
    useUiStore().homeVisible = true;
    const pending = deferred<RepositorySnapshot>();
    vi.mocked(backend.repositoryOpen).mockReturnValueOnce(pending.promise);
    const opening = useRepositoryStore().open("C:/missing");
    await flushPromises();
    send({ kind: "output", stream: "stdout", text: "still owned" });
    pending.reject({ code: "invalidRepository", message: "不存在" });
    await expect(opening).rejects.toMatchObject({ code: "invalidRepository" });
    send(terminal, 2);
    expect(store.outputText).toBe("still owned");
    expect(store.draft).toBe("git diff");
    expect(store.status).toBe("completed");
    expect(backend.consoleCancel).not.toHaveBeenCalled();
  });
  it.each(["C:/other", "C:/repo"])("successful reopen %s invalidates before cancellation and clears the session", async nextRoot => {
    const store = useConsoleStore();
    await store.run();
    const oldRun = store.runId!;
    const pending = deferred<void>();
    vi.mocked(backend.consoleCancel).mockReturnValueOnce(pending.promise);
    vi.mocked(backend.repositoryOpen).mockResolvedValueOnce({ ...repository, rootPath: nextRoot });
    const opening = useRepositoryStore().open(nextRoot);
    await flushPromises();
    expect(backend.consoleCancel).toHaveBeenCalledWith(repository.rootPath, oldRun);
    send({ kind: "output", stream: "stdout", text: "late old text" }, 1, oldRun);
    expect(store.outputText).toBe("");
    expect(store.history).toEqual([]);
    await store.run();
    expect(backend.consoleStart).toHaveBeenCalledTimes(1);
    pending.resolve();
    await opening;
    expect(store.status).toBe("idle");
    expect(store.draft).toBe("git status");
  });
  it("keeps history during running and disables execution with no repository", async () => {
    const store = useConsoleStore();
    await store.run();
    store.clearHistory();
    expect(store.history).toHaveLength(1);
    send(terminal);
    useRepositoryStore().snapshot = undefined;
    await store.run();
    expect(store.error?.code).toBe("invalidRepository");
    expect(backend.consoleStart).toHaveBeenCalledTimes(1);
  });
  it("disposes a pending listener and never launches after app unmount", async () => {
    const deferredListener = deferred<() => void>();
    const unlisten = vi.fn();
    vi.mocked(backend.consoleListen).mockReturnValueOnce(deferredListener.promise);
    const store = useConsoleStore();
    const start = store.run();
    store.dispose();
    deferredListener.resolve(unlisten);
    await start;
    expect(unlisten).toHaveBeenCalledOnce();
    expect(backend.consoleStart).not.toHaveBeenCalled();
  });
});
