import { beforeEach, describe, expect, it, vi } from "vitest";
import { createBackendFixture } from "@/test/backend";
import type { BackendClient } from "@/lib/backend/client";
import type { ConsoleEvent, GitRunEvent, RemoteOperationResult, TerminalEvent } from "@/lib/backend/types";
import { cancelFeedback, clearCompletedFeedback, gitFeedback, observeGitFeedback, resetGitFeedback } from "./gitFeedback";

const result: RemoteOperationResult = { workspace: { repository: { rootPath: "C:/repo", name: "repo", currentBranch: "main", headShortHash: "abc1234", isClean: true, changedFileCount: 0, conflictCount: 0, remotes: [], upstream: null }, changes: { files: [], stagedCount: 0, unstagedCount: 0 } }, refs: { localBranches: [], remoteBranches: [], tags: [] }, remotes: { remotes: [] }, operationState: { kind: "none", conflicts: [], abortAction: null } };
const push = { remote: "origin", localBranch: "feature/task", remoteBranch: "main", establishUpstream: false, forceWithLease: null };
describe("global Git feedback", () => {
  let backend: BackendClient, client: BackendClient, emit: (event: GitRunEvent) => void, terminal: (event: TerminalEvent) => void;
  beforeEach(async () => {
    resetGitFeedback();
    backend = createBackendFixture({
      gitRunListen: vi.fn(async listener => { emit = listener; return () => {}; }),
      terminalListen: vi.fn(async listener => { terminal = listener; return () => {}; }),
      remoteStartPush: vi.fn(async (_root, runId) => ({ runId, operation: "push" as const })),
      terminalStart: vi.fn(async (rootPath, runId) => ({ rootPath, runId })),
      changesStageFiles: vi.fn(async () => result), repositoryRefresh: vi.fn(async () => result.workspace.repository),
    });
    client = observeGitFeedback(backend);
    await client.gitRunListen(() => {}); await client.terminalListen(() => {});
  });
  it("reports a synchronous mutation from pending to success without changing its response", async () => {
    let complete!: (value: typeof result) => void;
    vi.mocked(backend.changesStageFiles).mockImplementation(() => new Promise(resolve => { complete = resolve; }));
    const request = client.changesStageFiles("C:/repo", ["a", "b"]);
    expect(gitFeedback.value[0]).toMatchObject({ title: "批量暂存", target: "2 个文件", status: "running" });
    clearCompletedFeedback(); expect(gitFeedback.value).toHaveLength(1);
    complete(result); expect(await request).toBe(result);
    expect(gitFeedback.value[0]).toMatchObject({ status: "success", message: "批量暂存成功：2 个文件" });
  });
  it("keeps background refresh quiet but reports manual refresh", async () => {
    await client.repositoryRefresh("C:/repo", true); expect(gitFeedback.value).toHaveLength(0);
    await client.repositoryRefresh("C:/repo"); expect(gitFeedback.value[0]?.status).toBe("success");
  });
  it("does not treat acceptance or a 100 percent transfer stage as push success", async () => {
    await client.remoteStartPush("C:/repo", "push-run", push);
    expect(gitFeedback.value[0]).toMatchObject({ status: "running", target: "feature/task → origin/main" });
    emit({ runId: "other-run", sequence: 8, event: { kind: "completed", result } });
    emit({ runId: "push-run", sequence: 2, event: { kind: "progress", phase: "writing", text: "Writing objects: 45% (9/20)" } });
    emit({ runId: "push-run", sequence: 1, event: { kind: "started", operation: "push" } });
    expect(gitFeedback.value[0]).toMatchObject({ percent: 45, phase: "发送对象", status: "running" });
    emit({ runId: "push-run", sequence: 3, event: { kind: "progress", phase: "writing", text: "Writing objects: 100%" } });
    expect(gitFeedback.value[0]?.status).toBe("running");
    emit({ runId: "push-run", sequence: 4, event: { kind: "completed", result } });
    emit({ runId: "push-run", sequence: 5, event: { kind: "progress", phase: "writing", text: "50%" } });
    expect(gitFeedback.value[0]).toMatchObject({ status: "success", message: "Push 推送成功：feature/task → origin/main" });
  });
  it("preserves a terminal event delivered before the acceptance response", async () => {
    vi.mocked(backend.remoteStartPush).mockImplementation(async (_root, runId) => { emit({ runId, sequence: 1, event: { kind: "failed", error: { code: "nonFastForward", message: "远程存在新提交，推送被拒绝" } } }); return { runId, operation: "push" }; });
    await client.remoteStartPush("C:/repo", "early", push);
    expect(gitFeedback.value[0]).toMatchObject({ status: "failed", message: "远程存在新提交，推送被拒绝" });
  });
  it("keeps actual errors and redacts diagnostics", async () => {
    const error = { code: "gitAuthentication" as const, message: "认证失败", diagnostics: "https://user:secret@example.test/repo password=secret" };
    vi.mocked(backend.changesStageFiles).mockRejectedValue(error);
    await expect(client.changesStageFiles("C:/repo", ["a"])).rejects.toBe(error);
    expect(gitFeedback.value[0]?.diagnostics).not.toContain("secret");
    expect(gitFeedback.value[0]?.message).toBe("认证失败");
  });
  it("waits for cancellation to finish and only sends one stop request", async () => {
    await client.remoteStartPush("C:/repo", "cancel", push);
    await Promise.all([cancelFeedback("cancel"), cancelFeedback("cancel")]);
    expect(backend.gitRunCancel).toHaveBeenCalledExactlyOnceWith("cancel");
    expect(gitFeedback.value[0]).toMatchObject({ status: "running", cancelling: true });
    emit({ runId: "cancel", sequence: 2, event: { kind: "cancelled", result } });
    expect(gitFeedback.value[0]).toMatchObject({ status: "cancelled", canCancel: false });
  });
  it.each([0, 141])("keeps terminal exit %i in the terminal without global feedback", async exitCode => {
    const listener = vi.fn();
    await client.terminalListen(listener);
    const accepted = await client.terminalStart("C:/repo", "terminal", "git log", 80, 24);
    expect(accepted).toEqual({ rootPath: "C:/repo", runId: "terminal" });
    expect(gitFeedback.value).toHaveLength(0);
    const event: TerminalEvent = { rootPath: "\\\\?\\C:\\repo", runId: "terminal", sequence: 1, event: { kind: "exited", exitCode, cancelled: false, error: null, durationMs: 20 } };
    terminal(event);
    expect(listener).toHaveBeenCalledExactlyOnceWith(event);
    expect(gitFeedback.value).toHaveLength(0);
  });
  it("passes console results and start errors through without global feedback", async () => {
    let consoleEvent!: (event: ConsoleEvent) => void;
    const listener = vi.fn();
    vi.mocked(backend.consoleListen).mockImplementation(async callback => { consoleEvent = callback; return () => {}; });
    vi.mocked(backend.consoleStart).mockResolvedValue({ rootPath: "C:/repo", runId: "console" });
    await client.consoleListen(listener);
    await client.consoleStart("C:/repo", "console", "git status");
    const event: ConsoleEvent = { rootPath: "C:/repo", runId: "console", sequence: 1, event: { kind: "terminal", outcome: "failed", exitCode: 128, durationMs: 20, stdoutTruncated: false, stderrTruncated: false } };
    consoleEvent(event);
    expect(listener).toHaveBeenCalledExactlyOnceWith(event);
    const failure = { code: "gitCommandFailed" as const, message: "Cannot start command" };
    vi.mocked(backend.terminalStart).mockRejectedValue(failure);
    vi.mocked(backend.consoleStart).mockRejectedValue(failure);
    await expect(client.terminalStart("C:/repo", "failed-terminal", "git log", 80, 24)).rejects.toBe(failure);
    await expect(client.consoleStart("C:/repo", "failed-console", "git status")).rejects.toBe(failure);
    expect(gitFeedback.value).toHaveLength(0);
  });
  it("distinguishes embedded operation errors and unresolved Git flows", async () => {
    const conflict = { ...result, operationState: { kind: "merge" as const, conflicts: [], abortAction: "merge" as const } };
    vi.mocked(backend.changesStageFiles).mockResolvedValue(conflict);
    await client.changesStageFiles("C:/repo", ["a"]);
    expect(gitFeedback.value[0]?.status).toBe("conflicted");
    vi.mocked(backend.filesExecute).mockResolvedValue({ applied: false, workspace: null, operationState: null, affectedDirectories: [], selectedPath: null, recoveryPath: null, error: { code: "staleFileOperation", message: "文件已经变化" } });
    await client.filesExecute("C:/repo", { intent: { kind: "delete", relativePath: "a" }, token: "old" });
    expect(gitFeedback.value[0]).toMatchObject({ status: "failed", message: "文件已经变化" });
  });
  it("separates successful Git execution from failed post-operation refresh", async () => {
    await client.remoteStartPush("C:/repo", "refresh-failed", push);
    emit({ runId: "refresh-failed", sequence: 1, event: { kind: "failed", error: { code: "gitRefreshFailed", message: "Git 同步已成功，但刷新失败" } } });
    expect(gitFeedback.value[0]).toMatchObject({ status: "warning", message: "Git 同步已成功，但刷新失败" });
  });
});
