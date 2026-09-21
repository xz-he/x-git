import { t } from '@/lib/i18n';
import type { BackendError, BackendErrorCode } from "./types";

const knownCodes = new Set<BackendErrorCode>([
  "invalidConsoleCommand",
  "staleFileOperation", "fileAlreadyExists", "unsupportedFileOperation",
  "gitNotFound",
  "invalidRepository",
  "gitAuthentication",
  "gitNetwork",
  "gitRefreshFailed",
  "gitConflict",
  "gitLocked",
  "gitCommandFailed",
  "dirtyWorktree",
  "invalidReference",
  "branchUnavailable",
  "currentBranchDeletion",
  "unmergedBranchDeletion",
  "missingUpstream",
  "nonFastForward",
  "leaseRejected",
  "gitOperationInProgress",
  "staleStash",
  "staleConflict",
  "unsupportedConflict",
  "invalidHistoryCursor",
  "invalidPath",
  "unsupportedEncoding",
  "io",
  "settingsMigration",
  "aiConfiguration",
  "aiAuthentication",
  "aiRateLimited",
  "aiTransport",
  "aiTimeout",
  "aiInvalidResponse",
  "aiContextTooLarge",
  "aiNoStagedChanges",
  "cancelled",
  "unexpected",
]);

export function normalizeBackendError(error: unknown): BackendError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof error.code === "string" &&
    knownCodes.has(error.code as BackendErrorCode) &&
    typeof error.message === "string"
  ) {
    const normalized: BackendError = {
      code: error.code as BackendErrorCode,
      message: error.message,
    };
    if ("diagnostics" in error && typeof error.diagnostics === "string") {
      normalized.diagnostics = error.diagnostics;
    }
    return normalized;
  }

  return {
    code: "unexpected",
    get message() { return t('uiAnUnknownErrorOccurred211ee5'); },
  };
}
