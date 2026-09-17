import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke as nativeInvoke } from "@tauri-apps/api/core";
import { activityWarning, invoke, recordAsyncEvent } from "./activity";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
describe("operation history IPC", () => {
  beforeEach(() => { vi.mocked(nativeInvoke).mockReset(); activityWarning.value = ""; });
  it("routes mutations through native history without changing returned results", async () => {
    const result = { workspace: { repository: {} } };
    vi.mocked(nativeInvoke).mockResolvedValue({ value: result, error: null, warning: null });
    expect(await invoke("changes_stage_files", { path: "C:/repo", relativePaths: ["a.ts", "b.ts"] })).toEqual(result);
    expect(nativeInvoke).toHaveBeenCalledExactlyOnceWith("activity_execute", { action: "changes_stage_files", args: { path: "C:/repo", relativePaths: ["a.ts", "b.ts"] } });
  });
  it("does not record refresh queries or recursively record history reads", async () => {
    vi.mocked(nativeInvoke).mockResolvedValue([]);
    await invoke("repository_refresh", { path: "C:/repo" }); await invoke("activity_list", { path: "C:/repo" });
    expect(nativeInvoke).toHaveBeenNthCalledWith(1, "repository_refresh", { path: "C:/repo" });
    expect(nativeInvoke).toHaveBeenNthCalledWith(2, "activity_list", { path: "C:/repo" });
  });
  it("preserves Git errors and reports journal completion failure separately", async () => {
    const error = { code: "gitLocked", message: "index is locked" };
    vi.mocked(nativeInvoke).mockResolvedValueOnce({ value: null, error, warning: "历史保存失败" });
    await expect(invoke("changes_commit", { path: "C:/repo", message: "test" })).rejects.toEqual(error);
    expect(activityWarning.value).toBe("历史保存失败");
  });
  it("records terminal completion once and omits arbitrary command arguments", async () => {
    vi.mocked(nativeInvoke).mockResolvedValue({});
    await invoke("terminal_start", { path: "C:/repo", runId: "run-a", command: "git -c secret=value status" });
    recordAsyncEvent({ runId: "run-a", event: { kind: "output" } });
    expect(nativeInvoke).toHaveBeenCalledTimes(1);
    recordAsyncEvent({ runId: "run-a", event: { kind: "exited", exitCode: 0, cancelled: false } });
    recordAsyncEvent({ runId: "run-a", event: { kind: "exited", exitCode: 0, cancelled: false } });
    await Promise.resolve();
    expect(nativeInvoke).toHaveBeenCalledTimes(2);
    expect(nativeInvoke).toHaveBeenLastCalledWith("activity_record_external", { path: "C:/repo", title: "Git 终端命令", success: true, message: "操作完成" });
  });
  it("records completed sync with a refresh warning as successful execution", async () => {
    vi.mocked(nativeInvoke).mockResolvedValue({});
    await invoke("remote_start_push", { path: "C:/repo", runId: "refresh-warning" });
    recordAsyncEvent({ runId: "refresh-warning", event: { kind: "failed", error: { code: "gitRefreshFailed", message: "同步成功，刷新失败" } } });
    await Promise.resolve();
    expect(nativeInvoke).toHaveBeenLastCalledWith("activity_record_external", { path: "C:/repo", title: "Push 远程", success: true, message: "同步成功，刷新失败" });
  });
});
