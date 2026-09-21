import { t } from '@/lib/i18n';
import type { ConflictDetail, RepositoryOperationKind } from "./backend/types";

export const MAX_AI_CONFLICT_VERSION_BYTES = 2 * 1024 * 1024;
export function conflictSuggestionUnavailable(detail?: ConflictDetail): string | undefined {
  if (!detail) return t('uiSelectAConflictedFileFirst4c6df6');
  if (!detail.editable) return detail.unsupportedReason ?? t('uiAITextResolutionSuggestionsAreUnavailableForThisFile4a3f3b');
  for (const version of [detail.base, detail.ours, detail.theirs, detail.working]) {
    if (version.kind === "missing" && !version.exists) continue;
    if (version.kind !== "text" || version.text === null) return t('uiAISuggestionsRequireCompleteUTF8TextWithConsistentLineEndingb71f70');
    if (version.byteLength > MAX_AI_CONFLICT_VERSION_BYTES) return t('uiAIConflictSuggestionsAreUnavailableIfAnyVersionExceeds2MiBa5fd4b');
  }
  return undefined;
}
export function conflictSideLabels(kind?: RepositoryOperationKind): { ours: string; theirs: string } {
  return kind === "rebase"
    ? { get ours() { return t('uiRebaseTargetReplayedCommits7b96b5'); }, get theirs() { return t('uiCommitBeingReplayedb4080d'); } }
    : { get ours() { return t('uiCurrentVersion46e66f'); }, get theirs() { return t('uiIncomingVersion931ad5'); } };
}
