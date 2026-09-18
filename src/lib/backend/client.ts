import { invoke, recordAsyncEvent, type ActivityEntry } from "./activity";
import { observeGitFailures } from "@/lib/gitFailure";
import { observeGitFeedback, resetGitFeedback } from "@/lib/gitFeedback";
import type { AiChatMessage } from "./types";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  IgnoreRequest, IgnorePreview,
  ChangeLineStat,
  NoiseCandidate, NoiseScan, NoiseRestoreResult,
  TerminalAccepted, TerminalEvent, TerminalCompletion,
  CreateTaskBranchRequest, TaskBranchBinding, TaskBranchRunRequest, TaskBranchResult,
  ReviewSource,
  ReviewSkillStatus,
  ConsoleAccepted, ConsoleEvent,
  FileDirectoryPage, RepositoryFilePreview, FileOperationIntent, PreparedFileOperation, ExecuteFileOperationRequest, FileMutationResult,
  ConflictDetail, ConflictSnapshot, ConflictMutationResult, ResolveConflictRequest,
  AiConnectionConfig,
  AiConnectionTestResult,
  AiRunAccepted,
  AiRunEvent,
  AppSettings,
  ChangeScope,
  ChangesSnapshot,
  CherryPickRequest,
  RevertRequest,
  CommitDetail,
  CommitResult,
  CreateBranchRequest,
  DeleteBranchRequest,
  FileDiff,
  FetchRequest,
  GitRunAccepted,
  GitRunEvent,
  HistoryMutationResult,
  HistoryPage,
  HistoryQuery,
  MutationWorkspace,
  AbortAction,
  RefsMutationResult,
  RefsSnapshot,
  RemoteSnapshot,
  RepositorySnapshot,
  SettingsLoadResult,
  ResetRequest,
  PullRequest,
  PushRequest,
  StashCreateRequest,
  StashDetail,
  StashMutationResult,
  StashSelection,
  StashSnapshot,
} from "./types";

export interface BackendClient {
  filesOpen(path: string, relativePath: string, reveal: boolean): Promise<void>;
  filesInspect(path: string, relativePath: string, kind: "history" | "blame"): Promise<string>;
  filesIgnorePreview(path: string, request: IgnoreRequest): Promise<IgnorePreview>;
  filesIgnore(path: string, request: IgnoreRequest): Promise<MutationWorkspace>;
  filesUntrack(path: string, relativePath: string): Promise<MutationWorkspace>;
  filesLfsTrack(path: string, relativePath: string): Promise<MutationWorkspace>;
  activityList(path: string): Promise<ActivityEntry[]>;
  activityRollback(path: string, id: string): Promise<MutationWorkspace>;
  repositoryWatchSnapshot(path: string): Promise<{ watchId: string; worktreeVersion: number; metadataVersion: number }>;
  repositoryWatchStop(): Promise<void>;
  aiChat(runId: string, messages: AiChatMessage[]): Promise<string>;
  taskBranchesSnapshot(path: string): Promise<TaskBranchBinding[]>;
  taskBranchesUnlink(path: string, id: string): Promise<TaskBranchBinding[]>;
  taskBranchesCreate(path: string, request: CreateTaskBranchRequest): Promise<TaskBranchResult>;
  taskBranchesRun(path: string, request: TaskBranchRunRequest): Promise<TaskBranchResult>;
  consoleStart(path: string, runId: string, command: string): Promise<ConsoleAccepted>;
  terminalStart(path: string, runId: string, command: string, cols: number, rows: number): Promise<TerminalAccepted>;
  terminalWrite(path: string, runId: string, data: string): Promise<void>;
  terminalResize(path: string, runId: string, cols: number, rows: number): Promise<void>;
  terminalTerminate(path: string, runId: string): Promise<void>;
  terminalAck(path: string, runId: string, sequence: number): Promise<void>;
  terminalComplete(path: string, command: string, cursor: number): Promise<TerminalCompletion>;
  terminalListen(listener: (event: TerminalEvent) => void): Promise<UnlistenFn>;
  consoleCancel(path: string, runId: string): Promise<void>;
  consoleListen(listener: (event: ConsoleEvent) => void): Promise<UnlistenFn>;
  filesList(path: string, relativeDir: string, cursor?: string): Promise<FileDirectoryPage>;
  filesPreview(path: string, relativePath: string): Promise<RepositoryFilePreview>;
  filesPrepare(path: string, intent: FileOperationIntent): Promise<PreparedFileOperation>;
  filesExecute(path: string, request: ExecuteFileOperationRequest): Promise<FileMutationResult>;
  conflictsSnapshot(path: string): Promise<ConflictSnapshot>;
  conflictsDetail(path: string, relativePath: string): Promise<ConflictDetail>;
  conflictsResolve(path: string, request: ResolveConflictRequest): Promise<ConflictMutationResult>;
  conflictsContinue(path: string, operationToken: string): Promise<ConflictMutationResult>;
  stashSnapshot(path: string): Promise<StashSnapshot>;
  stashDetail(path: string, selection: StashSelection): Promise<StashDetail>;
  stashFileDiff(path: string, selection: StashSelection, relativePath: string, untracked?: boolean): Promise<FileDiff>;
  stashCreate(path: string, request: StashCreateRequest): Promise<StashMutationResult>;
  stashApply(path: string, selection: StashSelection): Promise<StashMutationResult>;
  stashPop(path: string, selection: StashSelection): Promise<StashMutationResult>;
  aiReviewSkillStatus(path: string): Promise<ReviewSkillStatus>;
  aiStartReview(path: string, runId: string, source?: ReviewSource): Promise<AiRunAccepted>;
  aiStartConflictSuggestion(path: string, runId: string, relativePath: string, token: string): Promise<AiRunAccepted>;
  aiStartCommitMessage(path: string, runId: string): Promise<AiRunAccepted>;
  aiCancel(runId: string): Promise<void>;
  aiTestConnection(
    config: AiConnectionConfig,
  ): Promise<AiConnectionTestResult>;
  aiListen(listener: (event: AiRunEvent) => void): Promise<UnlistenFn>;
  repositoryOpen(path: string): Promise<RepositorySnapshot>;
  repositoryInit(path: string): Promise<RepositorySnapshot>;
  repositoryClone(url: string, path: string): Promise<RepositorySnapshot>;
  repositoryRefresh(path: string, quiet?: boolean): Promise<RepositorySnapshot>;
  changesSnapshot(path: string): Promise<ChangesSnapshot>;
  changesFileDiff(
    path: string,
    relativePath: string,
    scope: ChangeScope,
  ): Promise<FileDiff>;
  changesStageFiles(path: string, relativePaths: string[]): Promise<MutationWorkspace>;
  changesUnstageFiles(path: string, relativePaths: string[]): Promise<MutationWorkspace>;
  changesStageFile(
    path: string,
    relativePath: string,
  ): Promise<MutationWorkspace>;
  changesUnstageFile(
    path: string,
    relativePath: string,
  ): Promise<MutationWorkspace>;
  changesStageHunk(
    path: string,
    relativePath: string,
    hunkIndex: number,
  ): Promise<MutationWorkspace>;
  changesUnstageHunk(
    path: string,
    relativePath: string,
    hunkIndex: number,
  ): Promise<MutationWorkspace>;
  changesStageLines(
    path: string,
    relativePath: string,
    startLine: number,
    endLine: number,
  ): Promise<MutationWorkspace>;
  changesUnstageLines(
    path: string,
    relativePath: string,
    startLine: number,
    endLine: number,
  ): Promise<MutationWorkspace>;
  changesScanNoise(path: string): Promise<NoiseScan>;
  changesLineStats(path: string): Promise<ChangeLineStat[]>;
  changesRestoreNoise(path: string, selected: NoiseCandidate[]): Promise<NoiseRestoreResult>;
  changesDiscardFile(
    path: string,
    relativePath: string,
  ): Promise<MutationWorkspace>;
  changesCommit(path: string, message: string): Promise<CommitResult>;
  refsSnapshot(path: string): Promise<RefsSnapshot>;
  refsCreate(path: string, request: CreateBranchRequest): Promise<RefsMutationResult>;
  refsSwitch(path: string, name: string): Promise<RefsMutationResult>;
  refsDelete(path: string, request: DeleteBranchRequest): Promise<RefsMutationResult>;
  refsMerge(path: string, target: string, destination?: string): Promise<RefsMutationResult>;
  refsRebase(path: string, target: string): Promise<RefsMutationResult>;
  refsAbort(path: string, action: AbortAction): Promise<RefsMutationResult>;
  remotesSnapshot(path: string): Promise<RemoteSnapshot>;
  remoteStartFetch(
    path: string,
    runId: string,
    request: FetchRequest,
  ): Promise<GitRunAccepted>;
  remoteStartPull(
    path: string,
    runId: string,
    request: PullRequest,
  ): Promise<GitRunAccepted>;
  remoteStartPush(
    path: string,
    runId: string,
    request: PushRequest,
  ): Promise<GitRunAccepted>;
  gitRunCancel(runId: string): Promise<void>;
  gitRunListen(listener: (event: GitRunEvent) => void): Promise<UnlistenFn>;
  historyPage(path: string, query: HistoryQuery): Promise<HistoryPage>;
  historyDetail(path: string, commit: string): Promise<CommitDetail>;
  historyFileDiff(
    path: string,
    commit: string,
    relativePath: string,
  ): Promise<FileDiff>;
  historyCheckout(path: string, commit: string): Promise<HistoryMutationResult>;
  historyRevert(path: string, request: RevertRequest): Promise<HistoryMutationResult>;
  historyCherryPick(
    path: string,
    request: CherryPickRequest,
  ): Promise<HistoryMutationResult>;
  historyReset(
    path: string,
    request: ResetRequest,
  ): Promise<HistoryMutationResult>;
  settingsLoad(): Promise<SettingsLoadResult>;
  settingsSave(settings: AppSettings): Promise<AppSettings>;
}

const tauriBackendClient: BackendClient = {
  activityList: path => invoke("activity_list", { path }),
  activityRollback: (path, id) => invoke("activity_rollback", { path, id }),
  aiChat: (runId, messages) => invoke("ai_chat", { runId, messages }),
  terminalStart: (path, runId, command, cols, rows) => invoke("terminal_start", { path, runId, command, cols, rows }),
  terminalWrite: (path, runId, data) => invoke("terminal_write", { path, runId, data }),
  terminalResize: (path, runId, cols, rows) => invoke("terminal_resize", { path, runId, cols, rows }),
  terminalTerminate: (path, runId) => invoke("terminal_terminate", { path, runId }),
  terminalAck: (path, runId, sequence) => invoke("terminal_ack", { path, runId, sequence }),
  terminalComplete: (path, command, cursor) => invoke("terminal_complete", { path, command, cursor }),
  terminalListen: (listener) => listen<TerminalEvent>("git://terminal-event", ({ payload }) => { recordAsyncEvent(payload); listener(payload); }),
  taskBranchesSnapshot: (path) => invoke("task_branches_snapshot", { path }),
  taskBranchesUnlink: (path, id) => invoke("task_branches_unlink", { path, id }),
  taskBranchesCreate: (path, request) => invoke("task_branches_create", { path, request }),
  taskBranchesRun: (path, request) => invoke("task_branches_run", { path, request }),
  aiStartConflictSuggestion: (path, runId, relativePath, token) => invoke("ai_start_conflict_suggestion", { path, runId, relativePath, token }),
  consoleStart: (path, runId, command) => invoke("console_start", { path, runId, command }),
  consoleCancel: (path, runId) => invoke("console_cancel", { path, runId }),
  consoleListen: (listener) => listen<ConsoleEvent>("git://console-event", ({ payload }) => { recordAsyncEvent(payload); listener(payload); }),
  filesList: (path, relativeDir, cursor) => invoke("files_list", { path, relativeDir, cursor: cursor ?? null }),
  filesPreview: (path, relativePath) => invoke("files_preview", { path, relativePath }),
  filesOpen: (path, relativePath, reveal) => invoke("files_open", { path, relativePath, reveal }),
  filesInspect: (path, relativePath, kind) => invoke("files_inspect", { path, relativePath, kind }),
  filesIgnorePreview: (path, request) => invoke("files_ignore_preview", { path, request }),
  filesIgnore: (path, request) => invoke("files_ignore", { path, request }),
  filesUntrack: (path, relativePath) => invoke("files_untrack", { path, relativePath }),
  filesLfsTrack: (path, relativePath) => invoke("files_lfs_track", { path, relativePath }),
  filesPrepare: (path, intent) => invoke("files_prepare", { path, intent }),
  filesExecute: (path, request) => invoke("files_execute", { path, request }),
  conflictsSnapshot: (path) => invoke("conflicts_snapshot", { path }),
  conflictsDetail: (path, relativePath) => invoke("conflicts_detail", { path, relativePath }),
  conflictsResolve: (path, request) => invoke("conflicts_resolve", { path, request }),
  conflictsContinue: (path, operationToken) => invoke("conflicts_continue", { path, operationToken }),
  stashSnapshot: (path) => invoke("stash_snapshot", { path }),
  stashDetail: (path, selection) => invoke("stash_detail", { path, selection }),
  stashFileDiff: (path, selection, relativePath, untracked = false) => invoke("stash_file_diff", { path, selection, relativePath, untracked }),
  stashCreate: (path, request) => invoke("stash_create", { path, request }),
  stashApply: (path, selection) => invoke("stash_apply", { path, selection }),
  stashPop: (path, selection) => invoke("stash_pop", { path, selection }),
  aiReviewSkillStatus: (path) => invoke("ai_review_skill_status", { path }),
  aiStartReview: (path, runId, source) =>
    invoke("ai_start_review", source ? { path, runId, source } : { path, runId }),
  aiStartCommitMessage: (path, runId) =>
    invoke("ai_start_commit_message", { path, runId }),
  aiCancel: (runId) => invoke("ai_cancel", { runId }),
  aiTestConnection: (config) => invoke("ai_test_connection", { config }),
  aiListen: (listener) =>
    listen<AiRunEvent>("ai://run-event", ({ payload }) => listener(payload)),
  repositoryOpen: (path) => invoke("repository_open", { path }),
  repositoryWatchSnapshot: (path) => invoke("repository_watch_snapshot", { path }),
  repositoryWatchStop: () => invoke("repository_watch_stop"),
  repositoryInit: (path) => invoke("repository_init", { path }),
  repositoryClone: (url, path) => invoke("repository_clone", { url, path }),
  repositoryRefresh: (path) => invoke("repository_refresh", { path }),
  changesSnapshot: (path) => invoke("changes_snapshot", { path }),
  changesFileDiff: (path, relativePath, scope) =>
    invoke("changes_file_diff", { path, relativePath, scope }),
  changesStageFiles: (path, relativePaths) => invoke("changes_stage_files", { path, relativePaths }),
  changesUnstageFiles: (path, relativePaths) => invoke("changes_unstage_files", { path, relativePaths }),
  changesStageFile: (path, relativePath) =>
    invoke("changes_stage_file", { path, relativePath }),
  changesUnstageFile: (path, relativePath) =>
    invoke("changes_unstage_file", { path, relativePath }),
  changesStageHunk: (path, relativePath, hunkIndex) =>
    invoke("changes_stage_hunk", { path, relativePath, hunkIndex }),
  changesUnstageHunk: (path, relativePath, hunkIndex) =>
    invoke("changes_unstage_hunk", { path, relativePath, hunkIndex }),
  changesStageLines: (path, relativePath, startLine, endLine) =>
    invoke("changes_stage_lines", {
      path,
      relativePath,
      startLine,
      endLine,
    }),
  changesUnstageLines: (path, relativePath, startLine, endLine) =>
    invoke("changes_unstage_lines", {
      path,
      relativePath,
      startLine,
      endLine,
    }),
  changesScanNoise: (path) => invoke("changes_scan_noise", { path }),
  changesLineStats: (path) => invoke("changes_line_stats", { path }),
  changesRestoreNoise: (path, selected) => invoke("changes_restore_noise", { path, selected }),
  changesDiscardFile: (path, relativePath) =>
    invoke("changes_discard_file", { path, relativePath }),
  changesCommit: (path, message) =>
    invoke("changes_commit", { path, message }),
  refsSnapshot: (path) => invoke("refs_snapshot", { path }),
  refsCreate: (path, request) =>
    invoke("refs_create_branch", { path, request }),
  refsSwitch: (path, name) =>
    invoke("refs_switch_branch", { path, name }),
  refsDelete: (path, request) =>
    invoke("refs_delete_branch", { path, request }),
  refsMerge: (path, target, destination) => invoke("refs_merge", { path, target, ...(destination ? { destination } : {}) }),
  refsRebase: (path, target) => invoke("refs_rebase", { path, target }),
  refsAbort: (path, action) => invoke("refs_abort", { path, action }),
  remotesSnapshot: (path) => invoke("remotes_snapshot", { path }),
  remoteStartFetch: (path, runId, request) =>
    invoke("remote_start_fetch", { path, runId, request }),
  remoteStartPull: (path, runId, request) =>
    invoke("remote_start_pull", { path, runId, request }),
  remoteStartPush: (path, runId, request) =>
    invoke("remote_start_push", { path, runId, request }),
  gitRunCancel: (runId) => invoke("git_run_cancel", { runId }),
  gitRunListen: (listener) =>
    listen<GitRunEvent>("git://run-event", ({ payload }) => { recordAsyncEvent(payload); listener(payload); }),
  historyPage: (path, query) => invoke("history_page", { path, query }),
  historyDetail: (path, commit) =>
    invoke("history_detail", { path, commit }),
  historyFileDiff: (path, commit, relativePath) =>
    invoke("history_file_diff", { path, commit, relativePath }),
  historyCheckout: (path, commit) =>
    invoke("history_checkout", { path, commit }),
  historyRevert: (path, request) => invoke("history_revert", { path, request }),
  historyCherryPick: (path, request) =>
    invoke("history_cherry_pick", { path, request }),
  historyReset: (path, request) =>
    invoke("history_reset", { path, request }),
  settingsLoad: () => invoke("settings_load"),
  settingsSave: (settings) => invoke("settings_save", { settings }),
};

let activeBackendClient = observeGitFailures(observeGitFeedback(tauriBackendClient));

export const backendClient: BackendClient = {
  activityList: path => activeBackendClient.activityList(path),
  activityRollback: (path, id) => activeBackendClient.activityRollback(path, id),
  aiChat: (runId, messages) => activeBackendClient.aiChat(runId, messages),
  terminalStart: (path, runId, command, cols, rows) => activeBackendClient.terminalStart(path, runId, command, cols, rows),
  terminalWrite: (path, runId, data) => activeBackendClient.terminalWrite(path, runId, data),
  terminalResize: (path, runId, cols, rows) => activeBackendClient.terminalResize(path, runId, cols, rows),
  terminalTerminate: (path, runId) => activeBackendClient.terminalTerminate(path, runId),
  terminalAck: (path, runId, sequence) => activeBackendClient.terminalAck(path, runId, sequence),
  terminalComplete: (path, command, cursor) => activeBackendClient.terminalComplete(path, command, cursor),
  terminalListen: (listener) => activeBackendClient.terminalListen(listener),
  taskBranchesSnapshot: (path) => activeBackendClient.taskBranchesSnapshot(path),
  taskBranchesUnlink: (path, id) => activeBackendClient.taskBranchesUnlink(path, id),
  taskBranchesCreate: (path, request) => activeBackendClient.taskBranchesCreate(path, request),
  taskBranchesRun: (path, request) => activeBackendClient.taskBranchesRun(path, request),
  consoleStart: (path, runId, command) => activeBackendClient.consoleStart(path, runId, command),
  consoleCancel: (path, runId) => activeBackendClient.consoleCancel(path, runId),
  consoleListen: (listener) => activeBackendClient.consoleListen(listener),
  filesList: (path, relativeDir, cursor) => activeBackendClient.filesList(path, relativeDir, cursor),
  filesPreview: (path, relativePath) => activeBackendClient.filesPreview(path, relativePath),
  filesOpen: (path, relativePath, reveal) => activeBackendClient.filesOpen(path, relativePath, reveal),
  filesInspect: (path, relativePath, kind) => activeBackendClient.filesInspect(path, relativePath, kind),
  filesIgnorePreview: (path, request) => activeBackendClient.filesIgnorePreview(path, request),
  filesIgnore: (path, request) => activeBackendClient.filesIgnore(path, request),
  filesUntrack: (path, relativePath) => activeBackendClient.filesUntrack(path, relativePath),
  filesLfsTrack: (path, relativePath) => activeBackendClient.filesLfsTrack(path, relativePath),
  filesPrepare: (path, intent) => activeBackendClient.filesPrepare(path, intent),
  filesExecute: (path, request) => activeBackendClient.filesExecute(path, request),
  conflictsSnapshot: (path) => activeBackendClient.conflictsSnapshot(path),
  conflictsDetail: (path, relativePath) => activeBackendClient.conflictsDetail(path, relativePath),
  conflictsResolve: (path, request) => activeBackendClient.conflictsResolve(path, request),
  conflictsContinue: (path, operationToken) => activeBackendClient.conflictsContinue(path, operationToken),
  stashSnapshot: (path) => activeBackendClient.stashSnapshot(path),
  stashDetail: (path, selection) => activeBackendClient.stashDetail(path, selection),
  stashFileDiff: (path, selection, relativePath, untracked = false) => activeBackendClient.stashFileDiff(path, selection, relativePath, untracked),
  stashCreate: (path, request) => activeBackendClient.stashCreate(path, request),
  stashApply: (path, selection) => activeBackendClient.stashApply(path, selection),
  stashPop: (path, selection) => activeBackendClient.stashPop(path, selection),
  aiReviewSkillStatus: (path) => activeBackendClient.aiReviewSkillStatus(path),
  aiStartConflictSuggestion: (path, runId, relativePath, token) => activeBackendClient.aiStartConflictSuggestion(path, runId, relativePath, token),
  aiStartReview: (path, runId, source) => source
    ? activeBackendClient.aiStartReview(path, runId, source)
    : activeBackendClient.aiStartReview(path, runId),
  aiStartCommitMessage: (path, runId) =>
    activeBackendClient.aiStartCommitMessage(path, runId),
  aiCancel: (runId) => activeBackendClient.aiCancel(runId),
  aiTestConnection: (config) =>
    activeBackendClient.aiTestConnection(config),
  aiListen: (listener) => activeBackendClient.aiListen(listener),
  repositoryOpen: (path) => activeBackendClient.repositoryOpen(path),
  repositoryWatchSnapshot: (path) => activeBackendClient.repositoryWatchSnapshot(path),
  repositoryWatchStop: () => activeBackendClient.repositoryWatchStop(),
  repositoryInit: (path) => activeBackendClient.repositoryInit(path),
  repositoryClone: (url, path) =>
    activeBackendClient.repositoryClone(url, path),
  repositoryRefresh: (path, quiet) => quiet ? activeBackendClient.repositoryRefresh(path, true) : activeBackendClient.repositoryRefresh(path),
  changesSnapshot: (path) => activeBackendClient.changesSnapshot(path),
  changesFileDiff: (path, relativePath, scope) =>
    activeBackendClient.changesFileDiff(path, relativePath, scope),
  changesStageFiles: (path, relativePaths) => activeBackendClient.changesStageFiles(path, relativePaths),
  changesUnstageFiles: (path, relativePaths) => activeBackendClient.changesUnstageFiles(path, relativePaths),
  changesStageFile: (path, relativePath) =>
    activeBackendClient.changesStageFile(path, relativePath),
  changesUnstageFile: (path, relativePath) =>
    activeBackendClient.changesUnstageFile(path, relativePath),
  changesStageHunk: (path, relativePath, hunkIndex) =>
    activeBackendClient.changesStageHunk(path, relativePath, hunkIndex),
  changesUnstageHunk: (path, relativePath, hunkIndex) =>
    activeBackendClient.changesUnstageHunk(path, relativePath, hunkIndex),
  changesStageLines: (path, relativePath, startLine, endLine) =>
    activeBackendClient.changesStageLines(
      path,
      relativePath,
      startLine,
      endLine,
    ),
  changesUnstageLines: (path, relativePath, startLine, endLine) =>
    activeBackendClient.changesUnstageLines(
      path,
      relativePath,
      startLine,
      endLine,
    ),
  changesScanNoise: (path) => activeBackendClient.changesScanNoise(path),
  changesLineStats: (path) => activeBackendClient.changesLineStats(path),
  changesRestoreNoise: (path, selected) => activeBackendClient.changesRestoreNoise(path, selected),
  changesDiscardFile: (path, relativePath) =>
    activeBackendClient.changesDiscardFile(path, relativePath),
  changesCommit: (path, message) =>
    activeBackendClient.changesCommit(path, message),
  refsSnapshot: (path) => activeBackendClient.refsSnapshot(path),
  refsCreate: (path, request) =>
    activeBackendClient.refsCreate(path, request),
  refsSwitch: (path, name) =>
    activeBackendClient.refsSwitch(path, name),
  refsDelete: (path, request) =>
    activeBackendClient.refsDelete(path, request),
  refsMerge: (path, target, destination) =>
    destination === undefined ? activeBackendClient.refsMerge(path, target) : activeBackendClient.refsMerge(path, target, destination),
  refsRebase: (path, target) =>
    activeBackendClient.refsRebase(path, target),
  refsAbort: (path, action) =>
    activeBackendClient.refsAbort(path, action),
  remotesSnapshot: (path) => activeBackendClient.remotesSnapshot(path),
  remoteStartFetch: (path, runId, request) =>
    activeBackendClient.remoteStartFetch(path, runId, request),
  remoteStartPull: (path, runId, request) =>
    activeBackendClient.remoteStartPull(path, runId, request),
  remoteStartPush: (path, runId, request) =>
    activeBackendClient.remoteStartPush(path, runId, request),
  gitRunCancel: (runId) => activeBackendClient.gitRunCancel(runId),
  gitRunListen: (listener) => activeBackendClient.gitRunListen(listener),
  historyPage: (path, query) =>
    activeBackendClient.historyPage(path, query),
  historyDetail: (path, commit) =>
    activeBackendClient.historyDetail(path, commit),
  historyFileDiff: (path, commit, relativePath) =>
    activeBackendClient.historyFileDiff(path, commit, relativePath),
  historyCheckout: (path, commit) =>
    activeBackendClient.historyCheckout(path, commit),
  historyRevert: (path, request) => activeBackendClient.historyRevert(path, request),
  historyCherryPick: (path, request) =>
    activeBackendClient.historyCherryPick(path, request),
  historyReset: (path, request) =>
    activeBackendClient.historyReset(path, request),
  settingsLoad: () => activeBackendClient.settingsLoad(),
  settingsSave: (settings) => activeBackendClient.settingsSave(settings),
};

export function setBackendClientForTests(client: BackendClient): void {
  resetGitFeedback();
  activeBackendClient = observeGitFailures(observeGitFeedback(client));
}
