<script setup lang="ts">
import { t } from '@/lib/i18n';
import { formatDisplayPath } from "@/lib/formatPath";
import { computed } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import { hasConflictMarkers, useConflictsStore } from "@/stores/conflicts";
const conflicts = useConflictsStore();
const intent = computed(() => conflicts.confirmation);
const title = computed(() => intent.value?.kind === "save" ? t('uiSaveAndMarkResolvedf0e27c') : intent.value?.kind === "reload" ? t('uiDiscardDraftAndReloadd6dbef') : intent.value?.kind === "abort" ? t('uiAbortCurrentOperationeca84c') : t('uiContinueCurrentOperationda3683'));
const label = computed(() => intent.value?.kind === "save" ? t('uiConfirmSavingResolution8770d9') : intent.value?.kind === "reload" ? t('uiConfirmDiscardingDraft50fee3') : intent.value?.kind === "abort" ? t('uiConfirmAbort44be9b') : t('uiConfirmContinue55ceac'));
const markers = computed(() => intent.value?.kind === "save" && intent.value.resolution.kind === "text" && hasConflictMarkers(intent.value.resolution.text));
const needsAcknowledgement = computed(() => markers.value && intent.value?.kind === "save" && intent.value.resolution.kind === "text" && !intent.value.resolution.acknowledgeMarkers);
const preview = computed(() => {
  if (intent.value?.kind !== "save") return "";
  const resolution = intent.value.resolution;
  if (resolution.kind === "text") return resolution.text;
  if (resolution.kind === "delete") return t('uiDeleteFile935cd5');
  const version = conflicts.drafts[intent.value.path]?.detail[resolution.kind];
  return version?.exists ? version.text ?? `${version.kind} · ${version.byteLength} bytes` : t('uiSelectedVersionDoesNotExistTheFileWillBeDeleted211988');
});
</script>
<template>
  <ConfirmDialog v-if="intent" :title="title" :confirm-label="label" :busy="conflicts.submitting" :confirm-disabled="needsAcknowledgement || (intent.kind === 'continue' && !conflicts.canContinue)" :danger="intent.kind === 'abort' || intent.kind === 'reload'" @cancel="conflicts.cancelConfirmation" @confirm="conflicts.confirm">
    <div class="conflict-confirmation"><code>{{ formatDisplayPath(intent.rootPath) }}</code><p>{{ t('uiBranch309e33') }}{{ intent.branch ?? t('uiDetachedHEADddb9e0') }}</p>
      <template v-if="intent.kind === 'save'"><strong>{{ intent.path }}</strong><p>{{ t('uiWritesTheResultAndStagesOnlyThisFileARecoveryCopyOfTheOrigind59e98') }}</p><pre>{{ preview.slice(0, 4000) }}{{ preview.length > 4000 ? ("\n" + t('uiPreviewTruncatedae4056')) : '' }}</pre><label v-if="markers && intent.resolution.kind === 'text'" class="marker-warning"><input v-model="intent.resolution.acknowledgeMarkers" type="checkbox" :aria-label="t('uiConfirmKeepingConflictMarkers7b7b8f')" />{{ t('uiTheResultStillContainsConflictMarkersKeepThemAnyway35d98a') }}</label></template>
      <p v-else-if="intent.kind === 'reload'">{{ intent.path }} {{ t('uiunsavedDraftWillBeDiscarded8cc8af') }}</p>
      <template v-else-if="intent.kind === 'continue'"><p>{{ t('uiContinue1fc1af') }} {{ conflicts.snapshot?.continueAction }}{{ t('uiusingTheFollowingStagedContent147b33') }}</p><ul><li v-for="path in conflicts.snapshot?.stagedFiles" :key="path">{{ path }}</li></ul><p v-if="!conflicts.snapshot?.stagedFiles.length">{{ t('uiNoStagedFilesGitMayRejectAnEmptyCommit26ba36') }}</p></template>
      <p v-else>{{ t('uiAbort67847e') }} {{ intent.action }}{{ t('uiGitWillRestoreTheWorkingTreeDiscardingAllUnsavedResolutionDr1cbcd4') }}</p>
      <div v-if="conflicts.error" role="alert" class="error"><p>{{ conflicts.error.message }}</p><details v-if="conflicts.error.diagnostics"><summary>{{ t('uiDiagnostics0b673e') }}</summary><pre>{{ conflicts.error.diagnostics }}</pre></details></div><p v-if="conflicts.recoveryPath">{{ t('uiOriginalFileRecoveryCopy558e23') }}<code>{{ conflicts.recoveryPath }}</code></p>
    </div>
  </ConfirmDialog>
</template>
<style scoped>
.conflict-confirmation { max-height: 65vh; overflow: auto; min-width: 0; overflow-wrap: anywhere; font-size: 12px; }code, strong { display: block; margin-top: 8px; }code { color: var(--text-muted); font-size: 11px; }p { line-height: 1.6; }pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 180px; overflow: auto; padding: 8px; background: var(--surface-muted); }ul { padding-left: 20px; max-height: 160px; overflow: auto; }.marker-warning { display: flex; gap: 6px; align-items: flex-start; color: var(--warning); }.error { color: var(--danger); }
</style>
