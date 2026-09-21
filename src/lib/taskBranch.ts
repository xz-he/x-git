import { t } from '@/lib/i18n';
import type { TaskBranchKind, TaskBranchPhase } from "@/lib/backend/types";

export function taskBranchPreview(kind: TaskBranchKind, ticketInput: string, slugInput: string, descriptionInput: string) {
  const ticket = ticketInput.trim().toUpperCase();
  const slug = slugInput.trim().toLowerCase().replace(/\s+/g, "-");
  const description = descriptionInput.trim();
  const prefix = kind === "feature" ? "R" : "B";
  const error = !new RegExp(`^${prefix}[0-9]+$`).test(ticket)
    ? t('msgEnterTheCompleteNumericTaskIDStartingWithd22dd2', { p0: prefix })
    : !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug)
      ? t('uiUseLettersNumbersAndHyphensForTheEnglishDescriptionEGPurchasc4b8ed')
      : !description || /[\r\n]/.test(description) ? t('uiEnterASingleLineChineseDescriptione882b6') : undefined;
  return { ticket, slug, description, name: `${kind}/${ticket}-${slug}`, title: `${ticket} ${description}`, error };
}

export const taskPhaseLabel: Record<TaskBranchPhase, string> = {
  get ready() { return t('uiAwaitingCommit1bf4a2'); }, get committing() { return t('uiCommitResultNeedsVerificationc01693'); }, get pendingPick() { return t('uiCommittedAwaitingCherryPickf5d8e3'); },
  get picking() { return t('uiCherryPickResultNeedsVerificationb6ecf3'); }, get conflict() { return t('uiConflictsNeedResolution933bc3'); }, get pendingReturn() { return t('uiCherryPickedAwaitingReturn6b3098'); },
  get completed() { return t('uiCompletede99b48'); }, get needsAttention() { return t('uiStatusNeedsVerificationa1eb55'); },
};
