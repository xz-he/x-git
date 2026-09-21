import { t } from '@/lib/i18n';
import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";
import type { AiConflictSuggestionResult, BackendError, ConflictDetail } from "@/lib/backend/types";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import { conflictSuggestionUnavailable } from "@/lib/aiConflict";
import { useAiStore } from "./ai";
import { useAiChatStore } from "./aiChat";
import { useConflictsStore } from "./conflicts";
import { useRepositoryStore } from "./repository";
import { useSettingsStore } from "./settings";
import { useUiStore } from "./ui";

interface SuggestionPreview {
  stamp: string;
  path: string;
  root: string;
  token: string;
  before: string;
  beforeDeleted: boolean;
  candidate: string;
}
const stale = (): BackendError => ({ code: "staleConflict", get message() { return t('uiConflictContentDraftOrTaskChangedCheckAgainAndPreviewYourOri3d997d'); } });

export const useConflictSuggestionStore = defineStore("conflictSuggestion", () => {
  const ai = useAiStore();
  const conflicts = useConflictsStore();
  const repository = useRepositoryStore();
  const settings = useSettingsStore();
  const ui = useUiStore();
  const preview = ref<SuggestionPreview>();
  const error = ref<BackendError>();
  const appliedMessage = ref("");
  const pending = ref<number>();
  let requestVersion = 0;
  const busy = computed(() => pending.value !== undefined);
  const result = computed(() => ai.currentTask === "resolveConflict" && ai.status === "completed" ? ai.conflictResult : undefined);
  const owned = computed(() => !!ai.runRoot && ai.runRoot === repository.snapshot?.rootPath && ai.runGeneration === repository.generation);
  const readyTarget = computed(() => conflicts.loadedRootPath === repository.snapshot?.rootPath
    && conflicts.generation === repository.generation
    && !!conflicts.snapshot?.files.some(file => file.path === conflicts.current?.detail.path));
  const startReason = computed(() => {
    if (!settings.settings.apiKey.trim() || !settings.settings.baseUrl.trim() || !settings.settings.model.trim()) return t('uiConfigureAnAIServiceFirsted3727');
    if (ai.running || useAiChatStore().running) return t('uiWaitForOrStopTheCurrentAITask3c0649');
    if (conflicts.busy || conflicts.detailLoading || conflicts.loading || conflicts.confirmation) return t('uiWaitForTheCurrentOperationToFinish799cb7');
    if (!readyTarget.value) return t('uiSelectAnUnresolvedFileInTheConflictWorkbenchb63cde');
    return conflictSuggestionUnavailable(conflicts.current?.detail);
  });
  const canPreview = computed(() => !!result.value && result.value.kind === "text" && result.value.resolvedText !== null
    && owned.value && readyTarget.value && conflicts.selectedPath === result.value.context.path
    && conflicts.current?.detail.token === result.value.context.token
    && !conflictSuggestionUnavailable(conflicts.current?.detail)
    && !conflicts.busy && !conflicts.detailLoading && !conflicts.loading && !conflicts.confirmation
    && ui.activeView === "conflicts" && !ui.homeVisible);

  function stamp(): string {
    return JSON.stringify([repository.snapshot?.rootPath, repository.generation, repository.snapshot?.currentBranch,
      ai.runId, ai.runGeneration, ai.currentTask, ai.status, result.value?.context.fingerprint, result.value?.resolvedText,
      conflicts.loadedRootPath, conflicts.generation, conflicts.selectedPath, conflicts.current?.detail.token,
      conflicts.draftRevision, conflicts.snapshot?.operationToken, conflicts.busy, conflicts.detailLoading, conflicts.loading,
      !!conflicts.confirmation, ui.activeView, ui.homeVisible]);
  }
  watch(stamp, () => {
    const hadIntent = !!preview.value || pending.value !== undefined;
    requestVersion++;
    preview.value = undefined;
    pending.value = undefined;
    appliedMessage.value = "";
    if (hadIntent) error.value = stale();
    else error.value = undefined;
  }, { flush: "sync" });

  async function start(): Promise<void> {
    if (startReason.value) return;
    const detail = conflicts.current!.detail;
    settings.settings.aiDrawerOpen = true;
    await ai.startConflictSuggestion(detail.path, detail.token);
  }
  async function navigateToTarget(): Promise<void> {
    const path = result.value?.context.path ?? ai.conflictTarget?.relativePath;
    const root = ai.runRoot;
    const run = ai.runId;
    if (!path || !root || !owned.value || conflicts.busy) return;
    ui.openView("conflicts");
    await conflicts.ensureLoaded(root, repository.generation);
    if (owned.value && ai.runId === run) await conflicts.selectFile(path);
  }
  function verify(detail: ConflictDetail, expected: AiConflictSuggestionResult): void {
    if (detail.token !== expected.context.token || detail.path !== expected.context.path
      || detail.operationKind !== expected.context.operationKind || conflictSuggestionUnavailable(detail)) throw stale();
  }
  async function requestPreview(): Promise<void> {
    if (!canPreview.value || busy.value) return;
    const expected = result.value!;
    const root = ai.runRoot!;
    const captured = stamp();
    const id = ++requestVersion;
    pending.value = id;
    error.value = undefined; appliedMessage.value = "";
    try {
      const detail = await backendClient.conflictsDetail(root, expected.context.path);
      if (id !== requestVersion || stamp() !== captured || !canPreview.value) return;
      verify(detail, expected);
      const draft = conflicts.current!.resolution;
      const before = draft.kind === "text" ? draft.text
        : draft.kind === "ours" || draft.kind === "theirs" ? conflicts.current!.detail[draft.kind].text ?? "" : "";
      preview.value = { stamp: captured, root, path: expected.context.path, token: expected.context.token,
        before, beforeDeleted: draft.kind === "delete", candidate: expected.resolvedText! };
    } catch (cause) {
      if (id === requestVersion) error.value = normalizeBackendError(cause);
    } finally { if (pending.value === id) pending.value = undefined; }
  }
  async function confirmApply(): Promise<void> {
    const intent = preview.value;
    if (!intent || busy.value || !canPreview.value || intent.stamp !== stamp()) return;
    const expected = result.value!;
    const id = ++requestVersion;
    pending.value = id; error.value = undefined;
    try {
      const detail = await backendClient.conflictsDetail(intent.root, intent.path);
      if (id !== requestVersion || preview.value !== intent || stamp() !== intent.stamp || !canPreview.value) return;
      verify(detail, expected);
      preview.value = undefined;
      pending.value = undefined;
      conflicts.edit(intent.candidate);
      appliedMessage.value = t('uiSuggestionInsertedIntoTheResolutionDraftNotSavedOrStagedChec3ffa08');
    } catch (cause) {
      if (id === requestVersion) { preview.value = undefined; error.value = normalizeBackendError(cause); }
    } finally { if (pending.value === id) pending.value = undefined; }
  }
  function dismissPreview(): void {
    requestVersion++; pending.value = undefined; preview.value = undefined;
  }
  return { preview, error, busy, result, owned, startReason, canPreview, appliedMessage,
    start, navigateToTarget, requestPreview, confirmApply, dismissPreview };
});
