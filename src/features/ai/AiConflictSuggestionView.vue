<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, ref } from "vue";
import ConflictEditor from "@/features/conflicts/ConflictEditor.vue";
import ConflictSuggestionPreview from "./ConflictSuggestionPreview.vue";
import { conflictSideLabels } from "@/lib/aiConflict";
import { useAiStore } from "@/stores/ai";
import { useConflictSuggestionStore } from "@/stores/conflictSuggestion";
import { useConflictsStore } from "@/stores/conflicts";
const ai = useAiStore();
const suggestion = useConflictSuggestionStore();
const conflicts = useConflictsStore();
const copyMessage = ref("");
const context = computed(() => suggestion.result?.context ?? ai.context?.conflict);
const labels = computed(() => conflictSideLabels(context.value?.operationKind));
const operationLabel = computed(() => ({ get none() { return t('uiIndexConflict109085'); }, get merge() { return t('merge'); }, get rebase() { return t('rebase'); }, get cherryPick() { return t('uiCherryPickd62eda'); }, get revert() { return t('uiRevertfcca84'); } })[context.value?.operationKind ?? "none"]);
const canRetry = computed(() => suggestion.owned && !suggestion.startReason
  && conflicts.current?.detail.path === ai.conflictTarget?.relativePath
  && conflicts.current?.detail.token === ai.conflictTarget?.token);
function retry(): void { if (canRetry.value) void suggestion.start().catch(() => undefined); }
async function copy(): Promise<void> {
  const result = suggestion.result;
  if (!result) return;
  try {
    await navigator.clipboard.writeText(result.kind === "text" ? result.resolvedText! : [result.summary, result.explanation, ...result.risks, ...result.contextMissing].join("\n\n"));
    copyMessage.value = t('uiCopiede381a5');
  } catch { copyMessage.value = t('uiCopyFailedCheckClipboardPermissionsfe1f1f'); }
}
</script>
<template>
  <div class="conflict-suggestion-view">
    <p class="scope">{{ t('uiSendsOnlyTheSelectedFileSThreeVersionsAndOnDiskContentUnsave114d38') }}</p>
    <strong class="path">{{ context?.path ?? ai.conflictTarget?.relativePath }}</strong>
    <p v-if="ai.running" role="status">{{ t('uiAnalyzingConflictsPreviewIsAvailableWhenTheFullResultReturns7743f0') }}</p>
    <p v-else-if="ai.status === 'cancelled'" role="status">{{ t('uiTaskStoppedIncompleteResultsCannotBeApplied6f26cb') }}</p>
    <p v-if="ai.error" class="error" role="alert">{{ ai.error.message }}</p>
    <template v-if="suggestion.result">
      <section><h3>{{ suggestion.result.summary }}</h3><p class="prose">{{ suggestion.result.explanation }}</p></section>
      <section v-if="suggestion.result.risks.length"><h3>{{ t('uiRisksAndChecks286c4e') }}</h3><ul><li v-for="(risk, index) in suggestion.result.risks" :key="index">{{ risk }}</li></ul></section>
      <section v-if="suggestion.result.contextMissing.length"><h3>{{ t('uiMissingContexteb4958') }}</h3><ul><li v-for="(item, index) in suggestion.result.contextMissing" :key="index">{{ item }}</li></ul></section>
      <p v-if="suggestion.result.kind === 'adviceOnly'" class="scope">{{ t('uiAnalysisOnlyNoCandidateContentIsAvailableToApply639c2a') }}</p>
      <section v-else class="candidate"><h3>{{ t('uiFullCandidateContent389fb5') }}</h3><p v-if="suggestion.result.resolvedText === ''">{{ t('uiEmptyTextCandidateKeepsAnEmptyFileInsteadOfDeletingItc8e30b') }}</p><ConflictEditor :model-value="suggestion.result.resolvedText ?? ''" readonly :label="t('uiAIConflictCandidateContent72421e')" /></section>
      <button :aria-label="t('uiCopyConflictSuggestion76b9ab')" @click="copy">{{ suggestion.result.kind === 'text' ? t('uiCopyCandidateContent5f0b08') : t('uiCopyAnalysis4e9b17') }}</button>
      <p v-if="copyMessage" role="status">{{ copyMessage }}</p>
      <button v-if="suggestion.result.kind === 'text'" class="primary" :aria-label="t('uiPreviewSuggestionfb757d')" :disabled="!suggestion.canPreview || suggestion.busy" @click="suggestion.requestPreview">{{ suggestion.busy ? t('uiValidatingSourceFile007a46') : t('uiPreviewSuggestionfb757d') }}</button>
      <p v-if="suggestion.result.kind === 'text' && !suggestion.canPreview" class="scope">{{ t('uiReturnToTheOriginalConflictFileToPreviewIfTheFileOrRepositor3d986c') }}</p>
    </template>
    <p v-if="suggestion.error" class="error" role="alert">{{ suggestion.error.message }}</p>
    <p v-if="suggestion.appliedMessage" class="success" role="status">{{ suggestion.appliedMessage }}</p>
    <div class="actions"><button :aria-label="t('uiLocateSourceConflict8e0d06')" :disabled="!suggestion.owned || conflicts.busy" @click="suggestion.navigateToTarget">{{ t('uiLocateSourceFile579ebf') }}</button><button :aria-label="t('uiRegenerateConflictSuggestion5e5f17')" :disabled="!canRetry" @click="retry">{{ t('uiRegenerate2e1905') }}</button></div>
    <p v-if="!ai.running && !canRetry" class="scope">{{ t('uiSelectTheOriginalConflictVersionToRegenerateIfTheFileChangedcdd000') }}</p>
    <details v-if="context" class="metadata"><summary>{{ t('uiSourceVersionsForThisSuggestion1e5392') }}</summary><dl>
      <dt>{{ t('uiOperationf3ea6d') }}</dt><dd>{{ operationLabel }}</dd>
      <dt>{{ t('uiCommonAncestora4dcb8') }}</dt><dd>{{ context.baseOid ?? t('uiDoesNotExist0d864b') }}</dd>
      <dt>{{ labels.ours }}{{ t('uiIndexStage2ea441a') }}</dt><dd>{{ context.oursOid ?? t('uiDoesNotExist0d864b') }}</dd>
      <dt>{{ labels.theirs }}{{ t('uiIndexStage37865b3') }}</dt><dd>{{ context.theirsOid ?? t('uiDoesNotExist0d864b') }}</dd>
      <dt>{{ t('uiFileVerificationToken4c5747') }}</dt><dd>{{ context.token }}</dd><dt>{{ t('uiContextFingerprint87cc16') }}</dt><dd>{{ context.fingerprint }}</dd>
    </dl></details>
    <ConflictSuggestionPreview />
  </div>
</template>
<style scoped>
.conflict-suggestion-view { display: grid; align-content: start; gap: 12px; padding: 16px; font-size: 12px; overflow-wrap: anywhere; }
p, h3 { margin: 0; }h3 { margin-bottom: 8px; font-size: 13px; }.scope { color: var(--text-muted); font-size: 11px; line-height: 1.6; }.prose { white-space: pre-wrap; line-height: 1.6; }.path { font-family: var(--font-code); }
ul { margin: 0; padding-left: 18px; }li { margin: 5px 0; line-height: 1.5; }
.candidate { min-width: 0; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden; }.candidate h3, .candidate p { padding: 9px; margin: 0; }.candidate h3 { background: var(--surface-muted); }.candidate :deep(.conflict-editor) { height: 260px; }
button { min-height: 34px; padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }button.primary { background: var(--primary); color: white; }button:disabled { opacity: .48; cursor: not-allowed; }.actions { display: flex; flex-wrap: wrap; gap: 8px; }
.error, .success { padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md); line-height: 1.5; }.error { color: var(--danger); }.success { color: var(--success); }
.metadata { color: var(--text-muted); font-size: 11px; }.metadata summary { cursor: pointer; }.metadata dl { display: grid; gap: 4px; }.metadata dd { margin: 0 0 8px; font-family: var(--font-code); }
</style>
