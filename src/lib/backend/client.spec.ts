import { beforeEach, describe, expect, it, vi } from "vitest";

import { backendClient } from "./client";
import type { AiConnectionConfig, AiRunEvent, GitRunEvent } from "./types";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauri.listen,
}));

const config: AiConnectionConfig = {
  provider: "gemini",
  apiKey: "test-key",
  baseUrl: "https://generativelanguage.googleapis.com/v1beta",
  model: "gemini-test",
};

describe("backend AI transport", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.listen.mockReset();
  });

  it("delegates AI commands with exact command arguments", async () => {
    tauri.invoke.mockResolvedValue(undefined);

    await backendClient.aiStartReview("D:\\repo", "review-run");
    await backendClient.aiStartCommitMessage("D:\\repo", "commit-run");
    await backendClient.aiCancel("review-run");
    await backendClient.aiTestConnection(config);

    expect(tauri.invoke).toHaveBeenNthCalledWith(1, "ai_start_review", {
      path: "D:\\repo",
      runId: "review-run",
    });
    expect(tauri.invoke).toHaveBeenNthCalledWith(2, "ai_start_commit_message", {
      path: "D:\\repo",
      runId: "commit-run",
    });
    expect(tauri.invoke).toHaveBeenNthCalledWith(3, "ai_cancel", {
      runId: "review-run",
    });
    expect(tauri.invoke).toHaveBeenNthCalledWith(4, "ai_test_connection", {
      config,
    });
  });

  it("subscribes to one event and returns the unlisten function", async () => {
    const listener = vi.fn();
    const unlisten = vi.fn();
    let eventHandler: ((event: { payload: AiRunEvent }) => void) | undefined;
    tauri.listen.mockImplementation(
      async (
        _eventName: string,
        handler: (event: { payload: AiRunEvent }) => void,
      ) => {
        eventHandler = handler;
        return unlisten;
      },
    );
    const event: AiRunEvent = {
      runId: "run-1",
      sequence: 1,
      event: {
        kind: "delta",
        text: "feat",
      },
    };

    const result = await backendClient.aiListen(listener);
    eventHandler?.({ payload: event });

    expect(result).toBe(unlisten);
    expect(tauri.listen).toHaveBeenCalledTimes(1);
    expect(tauri.listen).toHaveBeenCalledWith(
      "ai://run-event",
      expect.any(Function),
    );
    expect(listener).toHaveBeenCalledWith(event);
  });

  it("delegates branch and history mutations with exact arguments", async () => {
    tauri.invoke.mockResolvedValue({ value: undefined, error: null, warning: null });
    const createRequest = {
      name: "topic",
      startPoint: "main",
      switch: true,
    };
    const deleteRequest = {
      name: "topic",
      force: true,
      confirmation: "topic",
    };
    const cherryPickRequest = {
      commit: "a".repeat(40),
      targetBranch: "release",
      returnAfterSuccess: true,
    };
    const resetRequest = {
      target: "b".repeat(40),
      mode: "hard" as const,
      confirmation: "bbbbbbb",
    };

    await backendClient.refsCreate("D:\\repo", createRequest);
    await backendClient.refsSwitch("D:\\repo", "topic");
    await backendClient.refsDelete("D:\\repo", deleteRequest);
    await backendClient.refsMerge("D:\\repo", "topic");
    await backendClient.refsRebase("D:\\repo", "topic");
    await backendClient.refsAbort("D:\\repo", "merge");
    await backendClient.historyCheckout("D:\\repo", "abc1234");
    await backendClient.historyCherryPick("D:\\repo", cherryPickRequest);
    await backendClient.historyReset("D:\\repo", resetRequest);

    expect(tauri.invoke.mock.calls).toEqual([
      ["refs_create_branch", { path: "D:\\repo", request: createRequest }],
      ["refs_switch_branch", { path: "D:\\repo", name: "topic" }],
      ["refs_delete_branch", { path: "D:\\repo", request: deleteRequest }],
      ["refs_merge", { path: "D:\\repo", target: "topic" }],
      ["refs_rebase", { path: "D:\\repo", target: "topic" }],
      ["refs_abort", { path: "D:\\repo", action: "merge" }],
      ["history_checkout", { path: "D:\\repo", commit: "abc1234" }],
      [
        "history_cherry_pick",
        { path: "D:\\repo", request: cherryPickRequest },
      ],
      ["history_reset", { path: "D:\\repo", request: resetRequest }],
    ].map(([action, args]) => ["activity_execute", { action, args }]));
  });

  it("delegates remote synchronization commands and events exactly", async () => {
    tauri.invoke.mockResolvedValue(undefined);
    const listener = vi.fn();
    const unlisten = vi.fn();
    let eventHandler: ((event: { payload: GitRunEvent }) => void) | undefined;
    tauri.listen.mockImplementation(
      async (
        _eventName: string,
        handler: (event: { payload: GitRunEvent }) => void,
      ) => {
        eventHandler = handler;
        return unlisten;
      },
    );
    const pushRequest = {
      remote: "origin",
      localBranch: "main",
      remoteBranch: "main",
      establishUpstream: true,
      forceWithLease: null,
    };

    await backendClient.remotesSnapshot("D:\\repo");
    await backendClient.remoteStartFetch("D:\\repo", "fetch-run", {
      remote: "origin",
    });
    await backendClient.remoteStartPull("D:\\repo", "pull-run", {
      remote: "origin",
      remoteBranch: "main",
    });
    await backendClient.remoteStartPush("D:\\repo", "push-run", pushRequest);
    await backendClient.gitRunCancel("push-run");
    const result = await backendClient.gitRunListen(listener);
    const event = {
      runId: "push-run",
      sequence: 1,
      event: { kind: "started", operation: "push" },
    } satisfies GitRunEvent;
    eventHandler?.({ payload: event });

    expect(tauri.invoke.mock.calls).toEqual([
      ["remotes_snapshot", { path: "D:\\repo" }],
      [
        "remote_start_fetch",
        { path: "D:\\repo", runId: "fetch-run", request: { remote: "origin" } },
      ],
      [
        "remote_start_pull",
        {
          path: "D:\\repo",
          runId: "pull-run",
          request: { remote: "origin", remoteBranch: "main" },
        },
      ],
      [
        "remote_start_push",
        { path: "D:\\repo", runId: "push-run", request: pushRequest },
      ],
      ["git_run_cancel", { runId: "push-run" }],
    ]);
    expect(tauri.listen).toHaveBeenCalledWith(
      "git://run-event",
      expect.any(Function),
    );
    expect(result).toBe(unlisten);
    expect(listener).toHaveBeenCalledWith(event);
  });
});
