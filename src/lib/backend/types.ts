export interface AiChatMessage { role: "user" | "assistant"; content: string; }
export type TaskBranchKind = "feature" | "hotfix";
export type TaskBranchMode = "current" | "remoteMaster";
export type TaskBranchPhase = "ready" | "committing" | "pendingPick" | "picking" | "conflict" | "pendingReturn" | "completed" | "needsAttention";
export interface CreateTaskBranchRequest {
  kind: TaskBranchKind; ticket: string; slug: string; description: string;
  mode: TaskBranchMode; remote?: string; sourceBranch: string; expectedHead: string;
}
export interface TaskBranchBinding {
  id: string; rootPath: string; sourceBranch: string; targetBranch: string;
  ticket: string; description: string; mode: TaskBranchMode; phase: TaskBranchPhase;
  sourceCommit: string | null; targetCommit: string | null;
  returnAfterSuccess: boolean; message: string | null;
}
export interface TaskBranchRunRequest {
  id: string; action: "commit" | "pick" | "return" | "reconcile";
  message?: string; returnAfterSuccess: boolean; expectedHead: string;
}
export interface TaskBranchResult {
  bindings: TaskBranchBinding[]; workspace: RefsMutationResult | null; error: BackendError | null;
}

export type BackendErrorCode =
  | "invalidConsoleCommand"
  | "staleFileOperation"
  | "fileAlreadyExists"
  | "unsupportedFileOperation"
  | "gitNotFound"
  | "invalidRepository"
  | "gitAuthentication"
  | "gitNetwork"
  | "gitRefreshFailed"
  | "gitConflict"
  | "gitLocked"
  | "staleConflict"
  | "unsupportedConflict"
  | "gitCommandFailed"
  | "dirtyWorktree"
  | "invalidReference"
  | "branchUnavailable"
  | "currentBranchDeletion"
  | "unmergedBranchDeletion"
  | "missingUpstream"
  | "nonFastForward"
  | "leaseRejected"
  | "gitOperationInProgress"
  | "staleStash"
  | "invalidHistoryCursor"
  | "invalidPath"
  | "unsupportedEncoding"
  | "io"
  | "settingsMigration"
  | "aiConfiguration"
  | "aiAuthentication"
  | "aiRateLimited"
  | "aiTransport"
  | "aiTimeout"
  | "aiInvalidResponse"
  | "aiContextTooLarge"
  | "aiNoStagedChanges"
  | "cancelled"
  | "unexpected";

export interface BackendError {
  code: BackendErrorCode;
  message: string;
  diagnostics?: string;
}

export type ConsoleOutcome = "completed" | "failed" | "cancelled" | "timedOut";
export interface ConsoleAccepted { runId: string; rootPath: string }
export interface ConsoleEvent {
  runId: string;
  rootPath: string;
  sequence: number;
  event:
    | { kind: "started" }
    | { kind: "output"; stream: "stdout" | "stderr"; text: string }
    | { kind: "terminal"; outcome: ConsoleOutcome; exitCode: number | null;
        durationMs: number; stdoutTruncated: boolean; stderrTruncated: boolean;
        error?: BackendError | null };
}

export type FileEntryKind = "file" | "directory" | "restricted";
export interface RepositoryFileEntry {
  name: string; relativePath: string; kind: FileEntryKind;
  byteLength: number | null; reason: string | null; gitStatus: string | null;
}
export interface FileDirectoryPage {
  relativeDir: string; token: string; entries: RepositoryFileEntry[];
  nextCursor: string | null; totalEntries: number;
}
export interface RepositoryFilePreview {
  relativePath: string; token: string;
  kind: "text" | "binary" | "unsupportedEncoding" | "tooLarge" | "unsupported";
  text: string | null; byteLength: number; bom: boolean;
  lineEnding: "lf" | "crlf" | "none" | "mixed"; reason: string | null;
}
export type FileOperationIntent =
  | { kind: "createFile" | "createDirectory"; parentDir: string; name: string }
  | { kind: "rename"; relativePath: string; newName: string }
  | { kind: "delete"; relativePath: string };
export interface PreparedFileOperation {
  intent: FileOperationIntent; token: string; sourcePath: string | null; targetPath: string | null;
  entryKind: "file" | "directory"; nodeCount: number; fileCount: number; directoryCount: number; totalBytes: number;
}
export interface ExecuteFileOperationRequest { intent: FileOperationIntent; token: string }
export interface FileMutationResult {
  applied: boolean; workspace: WorkingTreeSnapshot | null; operationState: RepositoryOperationState | null;
  affectedDirectories: string[]; selectedPath: string | null; recoveryPath: string | null; error: BackendError | null;
}

export interface RemoteSummary {
  name: string;
  fetchUrl: string;
}

export interface UpstreamSummary {
  name: string;
  ahead: number;
  behind: number;
}

export interface RepositorySnapshot {
  rootPath: string;
  name: string;
  currentBranch: string | null;
  headShortHash: string | null;
  isClean: boolean;
  changedFileCount: number;
  conflictCount: number;
  remotes: RemoteSummary[];
  upstream: UpstreamSummary | null;
}

export interface TerminalAccepted { runId: string; rootPath: string }
export interface TerminalCompletionItem { value: string; label: string; description: string; kind: string }
export interface TerminalCompletion { start: number; end: number; items: TerminalCompletionItem[]; hasMore: boolean }
export interface TerminalEvent {
  runId: string; rootPath: string; sequence: number;
  event: { kind: "started" } | { kind: "output"; data: string } |
    { kind: "exited"; exitCode: number | null; durationMs: number; cancelled: boolean; error: BackendError | null };
}

export type ChangeScope = "staged" | "unstaged";
export type DiffScope = ChangeScope | "commit";
export type DiffLineKind = "context" | "addition" | "deletion" | "meta";

export interface FileChange {
  path: string;
  oldPath: string | null;
  indexStatus: string;
  worktreeStatus: string;
  staged: boolean;
  unstaged: boolean;
  conflict: boolean;
}

export interface ChangesSnapshot {
  files: FileChange[];
  stagedCount: number;
  unstagedCount: number;
}

export interface DiffLine {
  kind: DiffLineKind;
  oldLine: number | null;
  newLine: number | null;
  content: string;
}

export interface DiffHunk {
  index: number;
  header: string;
  lines: DiffLine[];
}

export interface FileDiff {
  path: string;
  scope: DiffScope;
  binary: boolean;
  hunks: DiffHunk[];
}

export type BranchKind = "local" | "remote";

export interface BranchTip {
  fullHash: string;
  shortHash: string;
  subject: string;
  author: string;
  authoredAt: string;
}

export interface BranchSummary {
  name: string;
  fullName: string;
  kind: BranchKind;
  current: boolean;
  upstream?: string;
  ahead?: number;
  behind?: number;
  tip: BranchTip;
}

export interface TagSummary {
  name: string;
  objectHash: string;
  peeledCommitHash: string;
  annotated: boolean;
  tagger?: string;
  taggedAt?: string;
  annotation?: string;
  commitSubject: string;
}

export interface RefsSnapshot {
  localBranches: BranchSummary[];
  remoteBranches: BranchSummary[];
  tags: TagSummary[];
}

export interface RemoteBranchSummary {
  name: string;
  fullName: string;
  objectId: string;
  trackingLocal?: string;
  ahead?: number;
  behind?: number;
}

export interface RemoteDetail {
  name: string;
  fetchUrl: string;
  pushUrl: string;
  branches: RemoteBranchSummary[];
}

export interface RemoteSnapshot {
  remotes: RemoteDetail[];
}

export interface FetchRequest {
  remote: string;
}

export interface PullRequest {
  remote: string;
  remoteBranch: string;
  localBranch?: string;
}

export type IgnoreRule = { kind: "exact" } | { kind: "extension" } | { kind: "directory"; directory: string } | { kind: "custom"; pattern: string };
export interface IgnoreRequest { relativePath: string; rule: IgnoreRule; scope: "repository" | "local" | "global" }
export interface IgnorePreview { pattern: string; targetPath: string; tracked: boolean }

export interface ForceWithLease {
  expectedRemoteOid: string;
}

export interface PushRequest {
  remote: string;
  localBranch: string;
  remoteBranch: string;
  establishUpstream: boolean;
  forceWithLease: ForceWithLease | null;
}

export type GitRunOperation = "fetch" | "pull" | "push";
export type GitRunProgressPhase =
  | "enumerating"
  | "counting"
  | "compressing"
  | "receiving"
  | "resolving"
  | "writing"
  | "updating";

export interface RemoteOperationResult extends MutationWorkspace {
  refs: RefsSnapshot;
  remotes: RemoteSnapshot;
}

export interface GitRunAccepted {
  runId: string;
  operation: GitRunOperation;
}

export interface GitRunEvent {
  runId: string;
  sequence: number;
  event:
    | { kind: "started"; operation: GitRunOperation }
    | { kind: "progress"; phase: GitRunProgressPhase; text: string }
    | { kind: "completed"; result: RemoteOperationResult }
    | { kind: "conflicted"; result: RemoteOperationResult }
    | { kind: "cancelled"; result: RemoteOperationResult }
    | {
        kind: "failed";
        error: BackendError;
        result?: RemoteOperationResult;
      };
}

export type RepositoryOperationKind =
  | "none"
  | "merge"
  | "rebase"
  | "cherryPick"
  | "revert";
export type AbortAction = "merge" | "rebase" | "cherryPick" | "revert";

export interface ConflictFileSummary {
  path: string;
  status: string;
}

export interface RepositoryOperationState {
  kind: RepositoryOperationKind;
  conflicts: ConflictFileSummary[];
  abortAction: AbortAction | null;
}

export interface CreateBranchRequest {
  name: string;
  startPoint: string | null;
  switch: boolean;
}

export interface DeleteBranchRequest {
  name: string;
  force: boolean;
  confirmation: string | null;
}

export interface HistoryQuery {
  reference: string | null;
  search: string;
  cursor: string | null;
}

export interface TopologyParent {
  hash: string;
  lane: number;
}

export interface CommitTopology {
  lane: number;
  parents: TopologyParent[];
}

export interface CommitSummary {
  hash: string;
  shortHash: string;
  parentHashes: string[];
  subject: string;
  authorName: string;
  authorEmail: string;
  authoredAt: string;
  references: string[];
  topology: CommitTopology;
}

export interface HistoryPage {
  commits: CommitSummary[];
  nextCursor: string | null;
  queryFingerprint: string;
  continuationLanes: string[];
}

export interface CommitFileSummary {
  status: string;
  path: string;
  oldPath: string | null;
  additions: number | null;
  deletions: number | null;
}

export interface CommitDetail {
  hash: string;
  shortHash: string;
  parentHashes: string[];
  message: string;
  authorName: string;
  authorEmail: string;
  authoredAt: string;
  committerName: string;
  committerEmail: string;
  committedAt: string;
  references: string[];
  files: CommitFileSummary[];
}

export interface WorkingTreeSnapshot {
  repository: RepositorySnapshot;
  changes: ChangesSnapshot;
}

export interface StashEntry {
  selector: string;
  objectId: string;
  branch: string | null;
  description: string;
  timestamp: string;
}
export interface StashSelection { selector: string; expectedObjectId: string }
export interface StashSnapshot { entries: StashEntry[] }
export interface StashFileSummary extends CommitFileSummary { binary: boolean; untracked: boolean }

export type ConflictContentKind = "missing" | "text" | "binary" | "unsupportedEncoding" | "tooLarge" | "mixedLineEndings" | "unsupported";
export interface ConflictVersion {
  exists: boolean; oid: string | null; mode: string | null; kind: ConflictContentKind;
  text: string | null; byteLength: number; bom: boolean; lineEnding: "lf" | "crlf" | "none" | "mixed";
}
export interface ConflictFile { path: string; status: string; supported: boolean; reason: string | null }
export interface ConflictSnapshot {
  operationState: RepositoryOperationState; operationToken: string; files: ConflictFile[];
  continueAction: AbortAction | null; stagedFiles: string[];
}
export interface ConflictDetail {
  path: string; token: string; operationKind: RepositoryOperationState["kind"];
  base: ConflictVersion; ours: ConflictVersion; theirs: ConflictVersion; working: ConflictVersion;
  editable: boolean; canChooseOurs: boolean; canChooseTheirs: boolean; canDelete: boolean; unsupportedReason: string | null;
}
export type ConflictResolution = { kind: "text"; text: string; acknowledgeMarkers: boolean } | { kind: "ours" | "theirs" | "delete" };
export interface ResolveConflictRequest { relativePath: string; token: string; resolution: ConflictResolution }
export interface ConflictMutationResult {
  workspace: WorkingTreeSnapshot; operationState: RepositoryOperationState; conflicts: ConflictSnapshot;
  refs: RefsSnapshot | null; error: BackendError | null; recoveryPath: string | null; resolved: boolean;
}
export interface StashDetail { entry: StashEntry; files: StashFileSummary[] }
export interface StashCreateRequest { message?: string | null; includeUntracked: boolean; paths?: string[] }
export interface StashMutationResult extends MutationWorkspace {
  outcome: "created" | "noChanges" | "applied" | "removed" | "retained";
  stashes: StashSnapshot;
  error?: BackendError | null;
}

export interface MutationWorkspace {
  workspace: WorkingTreeSnapshot;
  operationState: RepositoryOperationState;
}

export interface RefsMutationResult extends MutationWorkspace {
  refs: RefsSnapshot;
}

export type ResetMode = "soft" | "mixed" | "hard";

export interface ResetRequest {
  target: string;
  mode: ResetMode;
  confirmation: string | null;
}

export interface CherryPickRequest {
  commit: string;
  targetBranch: string | null;
  returnAfterSuccess: boolean;
}

export interface NoiseCandidate { path: string; staged: boolean; fingerprint: string }
export interface ChangeLineStat { path: string; scope: ChangeScope; additions: number | null; deletions: number | null; binary: boolean }
export interface NoiseSkipped { path: string; reason: string }
export interface NoiseScan { candidates: NoiseCandidate[]; skipped: NoiseSkipped[] }
export interface NoiseRestoreResult { workspace: MutationWorkspace; restored: string[]; skipped: NoiseSkipped[] }

export interface RevertRequest {
  commit: string;
  mainline: number | null;
}

export interface HistoryMutationResult extends MutationWorkspace {
  history: HistoryPage;
  error?: BackendError | null;
}

export interface CommitResult {
  shortHash: string;
  subject: string;
  workspace: MutationWorkspace;
}

export type ThemePreference = "light" | "dark" | "system";
export type AiProvider = "openAi" | "qwen" | "gemini" | "custom";
export type AiApiFormat = "chatCompletions" | "responses";
export type AiTaskKind = "reviewChanges" | "generateCommitMessage" | "resolveConflict";
export type AiIssueSeverity = "P0" | "P1" | "P2" | "P3" | "critical" | "warning" | "suggestion";

export type ReviewSource = { kind: "staged" } | { kind: "stagedFiles"; paths: string[] } | { kind: "commit"; revision: string };
export interface ReviewSkillInfo {
  directory: string; name: string; version: string | null; fingerprint: string; files: string[];
}
export interface ReviewSkillStatus {
  state: "ready" | "missing" | "ambiguous" | "error";
  info: ReviewSkillInfo | null; error: BackendError | null;
}
export interface ReviewEvidenceSource {
  path: string; revision: string; startLine: number; endLine: number;
}
export interface ReviewContext {
  source: ReviewSource; resolvedCommit: string | null; baseCommit: string | null;
  changedFileCount: number; skill: ReviewSkillInfo; excludedFiles: string[];
  evidenceSources: ReviewEvidenceSource[];
}

export interface AiConnectionConfig {
  provider: AiProvider;
  apiFormat?: AiApiFormat;
  apiKey: string;
  baseUrl: string;
  model: string;
}

export interface AiContextSummary {
  conflict?: AiConflictContext | null;
  review?: ReviewContext | null;
  stagedFileCount: number;
  textFileCount: number;
  skippedBinaryFiles: string[];
  fingerprint: string;
}

export interface AiRunAccepted {
  runId: string;
  task: AiTaskKind;
  context: AiContextSummary;
  totalBatchCount: number;
}

export interface AiReviewIssue {
  title?: string | null;
  impact?: string | null;
  evidence?: string | null;
  confidence?: number | null;
  contextMissing?: string[];
  changeRelation?: string | null;
  evidenceSources?: ReviewEvidenceSource[];
  severity: AiIssueSeverity;
  path: string;
  startLine: number | null;
  endLine: number | null;
  reason: string;
  suggestedFix: string;
}

export interface AiReviewResult {
  markdown?: string;
  context?: ReviewContext | null;
  uncovered?: string[];
  summary: string;
  issues: AiReviewIssue[];
  reviewedFiles: string[];
  skippedBinaryFiles: string[];
  warnings: string[];
}

export interface AiCommitMessageResult {
  message: string;
  contextFingerprint: string;
}

export interface AiConflictContext {
  path: string;
  token: string;
  operationKind: RepositoryOperationKind;
  baseOid: string | null;
  oursOid: string | null;
  theirsOid: string | null;
  fingerprint: string;
}

export interface AiConflictSuggestionResult {
  kind: "text" | "adviceOnly";
  summary: string;
  explanation: string;
  resolvedText: string | null;
  risks: string[];
  contextMissing: string[];
  context: AiConflictContext;
}

export interface AiConnectionTestResult {
  provider: AiProvider;
  model: string;
  message: string;
}

export interface AiRunEvent {
  runId: string;
  sequence: number;
  event:
    | {
        kind: "started";
        context: AiContextSummary;
        totalBatchCount: number;
      }
    | { kind: "batchStarted"; batchIndex: number; filePaths: string[] }
    | { kind: "reviewProgress"; phase: string; message: string }
    | { kind: "delta"; text: string }
    | {
        kind: "reviewBatchCompleted";
        batchIndex: number;
        issues: AiReviewIssue[];
        result?: AiReviewResult | null;
      }
    | { kind: "reviewCompleted"; result: AiReviewResult }
    | { kind: "commitMessageCompleted"; result: AiCommitMessageResult }
    | { kind: "conflictSuggestionCompleted"; result: AiConflictSuggestionResult }
    | { kind: "failed"; error: BackendError }
    | {
        kind: "cancelled";
        completedBatchCount: number;
        totalBatchCount: number;
      };
}

export interface AppSettings {
  schemaVersion: number;
  language: "zh-CN" | "en" | "bilingual";
  theme: ThemePreference;
  fontFamily: string;
  codeFontFamily: string;
  launchAtLogin: boolean;
  checkUpdatesOnStartup: boolean;
  lastRepoPath: string | null;
  recentRepoPaths: string[];
  reviewRuleFiles: string[];
  reviewSkillDirectory: string;
  useReviewRuleFilesInReview: boolean;
  aiDrawerOpen: boolean;
  aiDrawerWidth: number;
  aiProvider: AiProvider;
  aiApiFormat: AiApiFormat;
  apiKey: string;
  baseUrl: string;
  model: string;
}

export interface SettingsLoadResult {
  settings: AppSettings;
  migrationWarning?: string;
}
