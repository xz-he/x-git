<script setup lang="ts">
import { t } from '@/lib/i18n';
import { onBeforeUnmount } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import ConflictEditor from "@/features/conflicts/ConflictEditor.vue";
import { useConflictSuggestionStore } from "@/stores/conflictSuggestion";
const suggestion = useConflictSuggestionStore();
onBeforeUnmount(() => suggestion.dismissPreview());
</script>
<template>
  <ConfirmDialog v-if="suggestion.preview" class="suggestion-preview" :title="t('uiPreviewAISuggestionReplaceDraftOnly557a5a')"
    :description="t('uiReplacesTheCurrentResolutionDraftWithoutSavingOrStagingItChee926d2')"
    :confirm-label="t('uiInsertIntoResolutionDraft406c6d')" :busy="suggestion.busy" :confirm-disabled="!suggestion.canPreview"
    @cancel="suggestion.dismissPreview" @confirm="suggestion.confirmApply" @keydown.esc="suggestion.dismissPreview">
    <p class="path">{{ suggestion.preview.path }}</p>
    <div class="comparison">
      <section><h3>{{ t('uiCurrentResolutionDraftc0b800') }}</h3><p v-if="suggestion.preview.beforeDeleted">{{ t('uiCurrentDraftDeletesTheFilec10491') }}</p><ConflictEditor v-else :model-value="suggestion.preview.before" readonly :label="t('uiDraftBeforeApplying3ea944')" /></section>
      <section><h3>{{ t('uiAIReplacementContentfbf74c') }}</h3><p v-if="suggestion.preview.candidate === ''">{{ t('uiAnEmptyCandidateKeepsAnEmptyFileItDoesNotDeleteTheFileb47588') }}</p><ConflictEditor :model-value="suggestion.preview.candidate" :compare-text="suggestion.preview.before" :comparison-label="t('uiCompareWithCurrentDraft8433ee')" readonly :label="t('uiSuggestedReplacementdf1fac')" /></section>
    </div>
  </ConfirmDialog>
</template>
<style scoped>
.suggestion-preview :deep(.confirm-dialog) { width: min(920px, calc(100vw - 48px)); grid-template-columns: minmax(0, 1fr); max-height: calc(100vh - 48px); overflow: auto; }
.path { overflow-wrap: anywhere; font-family: var(--font-code); font-size: 12px; }
.comparison { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
.comparison section { min-width: 0; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden; }
h3 { margin: 0; padding: 9px; font-size: 12px; background: var(--surface-muted); }
.comparison p { padding: 0 9px; font-size: 12px; color: var(--text-muted); }
.comparison :deep(.conflict-editor) { height: min(360px, 45vh); }
@media (max-width: 700px) { .comparison { grid-template-columns: minmax(0, 1fr); }.comparison :deep(.conflict-editor) { height: 170px; } }
</style>
