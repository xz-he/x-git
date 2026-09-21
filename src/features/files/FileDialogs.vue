<script setup lang="ts">
import { t } from '@/lib/i18n';
import { formatDisplayPath } from "@/lib/formatPath";
import { computed } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import { parentDirectory, useFilesStore } from "@/stores/files";
const files = useFilesStore();
const current = computed(() => files.confirmation);
const labels = { get createFile() { return t('uiNewFiled4526e'); }, get createDirectory() { return t('uiNewFolder95cf3c'); }, get rename() { return t('uiRename1cd80f'); }, get delete() { return t('uiMoveToRecoveryArea301cc4'); } };
const title = computed(() => current.value ? labels[current.value.intent.kind] : t('uiFileActionsfd1cf9'));
const name = computed(() => { const intent = current.value?.intent; return !intent || intent.kind === "delete" ? "" : intent.kind === "rename" ? intent.newName : intent.name; });
const target = computed(() => {
  const intent = current.value?.intent; if (!intent || intent.kind === "delete") return null;
  const parent = intent.kind === "rename" ? parentDirectory(intent.relativePath) : intent.parentDir;
  return [parent, name.value].filter(Boolean).join("/");
});
</script>
<template>
  <ConfirmDialog v-if="current" :title="title" :danger="current.intent.kind === 'delete'" :confirm-label="current.prepared ? t('uiConfirmb56d9a') + title : t('uiPreflightCheck54f847')" :confirm-disabled="!!files.nameError || files.preparing || !files.canMutate" :busy="files.submitting" @cancel="files.cancelConfirmation" @confirm="current.prepared ? files.confirm() : files.prepare()">
    <div class="file-confirmation"><p>{{ t('uiRepository2a9fcc') }}<code>{{ formatDisplayPath(current.rootPath) }}</code></p><p>{{ t('uiBranch309e33') }}{{ current.branch ?? t('uiDetachedHEADddb9e0') }}</p><p v-if="'relativePath' in current.intent">{{ t('uiOriginalPathdc7bc3') }}<strong>{{ current.intent.relativePath }}</strong></p><p v-else>{{ t('uiParentDirectory3ad373') }}<strong>{{ current.intent.parentDir || t('uiRepositoryRoot4c3dc5') }}</strong></p>
      <label v-if="current.intent.kind !== 'delete'">{{ current.intent.kind === 'rename' ? t('uiNewNameb86598') : t('uiName1be7ae') }}<input :value="name" :aria-label="t('uiFileName572c5c')" :disabled="files.submitting" autocomplete="off" spellcheck="false" @input="files.changeName(($event.target as HTMLInputElement).value)" /></label><p v-if="target">{{ t('uiTargetPath64eff8') }}<strong>{{ target }}</strong></p><p v-if="files.nameError" class="error">{{ files.nameError }}</p>
      <p v-if="current.intent.kind === 'delete'">{{ t('uiMovesTheEntireSelectedItemIncludingUntrackedAndIgnoredConten7292b9') }}</p><p v-else>{{ t('uiChangesOnlyTheWorkingDirectoryDoesNotStageAutomaticallyNewIt7ef6e4') }}</p>
      <div v-if="current.prepared" class="summary"><strong>{{ t('uiPreflightCheckComplete9ca79a') }}</strong><p>{{ current.prepared.entryKind === 'directory' ? t('uiDirectory41e524') : t('uifiles49deaf') }} · {{ current.prepared.fileCount }} {{ t('uifilesce4733') }} {{ current.prepared.directoryCount }} {{ t('uidirectoriesac05ce') }}</p><p>{{ current.prepared.totalBytes.toLocaleString() }} bytes · {{ current.prepared.nodeCount }} {{ t('uientriesdf2dd9') }}</p><p>{{ t('uiPauseExternalWritesBeforeConfirmingChangesToContentOrReposit9481da') }}</p></div><p v-else-if="files.preparing" role="status">{{ t('uiCheckingPathsAndContent4daee2') }}</p>
      <div v-if="files.error" class="error" role="alert"><p>{{ files.error.message }}</p><details v-if="files.error.diagnostics"><summary>{{ t('uiDiagnostics0b673e') }}</summary><pre>{{ files.error.diagnostics }}</pre></details></div>
    </div>
  </ConfirmDialog>
</template>
<style scoped>
.file-confirmation { max-height: 65vh; min-width: 0; overflow: auto; overflow-wrap: anywhere; font-size: 12px; }p { margin: 9px 0; line-height: 1.6; }code, strong { overflow-wrap: anywhere; }code { display: block; color: var(--text-muted); }label { display: grid; gap: 6px; }input { width: 100%; min-width: 0; padding: 8px; color: var(--text); background: var(--surface-app); border: 1px solid var(--border); border-radius: 4px; }.summary { margin-top: 12px; padding: 10px; background: var(--surface-muted); border-radius: 4px; }.error { color: var(--danger); }pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
