<script setup lang="ts">
import { t } from '@/lib/i18n';
import { Archive, ArchiveRestore, ArrowUpFromLine, FileQuestion, LoaderCircle, RefreshCw } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import type { StashEntry } from "@/lib/backend/types";
import { useStashesStore } from "@/stores/stashes";
const stashes = useStashesStore();
const pendingPop = ref<StashEntry>();
const disabled = computed(() => !stashes.canMutate || !!stashes.detailError || !stashes.detail || stashes.detailLoading);
watch([() => stashes.loadedRootPath, () => stashes.generation, () => stashes.selectedEntry?.objectId, () => stashes.selectedEntry?.selector], () => { pendingPop.value = undefined; });
function refresh(): void { void stashes.refresh().catch(() => undefined); }
function selectFile(path: string, untracked = false): void { void stashes.selectFile(path, untracked).catch(() => undefined); }
function apply(): void { if (stashes.selectedEntry && !disabled.value) void stashes.apply(stashes.selectedEntry).catch(() => undefined); }
function askPop(): void { if (stashes.selectedEntry && !disabled.value) pendingPop.value = { ...stashes.selectedEntry }; }
async function pop(): Promise<void> {
  const entry = pendingPop.value;
  if (!entry || disabled.value) return;
  try { await stashes.pop(entry); } catch { /* The list retains the operation error. */ }
  finally { pendingPop.value = undefined; }
}
function cancelPop(): void { if (!stashes.submitting) pendingPop.value = undefined; }
</script>

<template>
  <div class="detail-body stash-detail">
    <div v-if="!stashes.selectedEntry" class="module-state"><Archive :size="22" />{{ t('uiNoStashSelected3b3ff9') }}</div>
    <template v-else>
      <section class="stash-metadata">
        <div class="detail-title"><Archive :size="18" /><div><span>{{ stashes.selectedEntry.selector }} · {{ stashes.selectedEntry.branch ?? t('uiUnknownBranch433d75') }}</span><h1 :title="stashes.selectedEntry.description">{{ stashes.selectedEntry.description }}</h1></div></div>
        <div class="metadata"><code :title="stashes.selectedEntry.objectId">{{ stashes.selectedEntry.objectId }}</code><time :datetime="stashes.selectedEntry.timestamp">{{ stashes.selectedEntry.timestamp }}</time></div>
        <div class="stash-actions">
          <button data-action="apply-stash" :disabled="disabled" @click="apply"><ArchiveRestore :size="14" />{{ t('uiApplyac4f76') }}</button>
          <button data-action="pop-stash" :disabled="disabled" @click="askPop"><ArrowUpFromLine :size="14" />{{ t('uiPop6fa24a') }}</button>
        </div>
      </section>
      <div v-if="stashes.detailLoading" class="module-state"><LoaderCircle :size="18" class="spin" />{{ t('uiLoadingStashDetails8a58cc') }}</div>
      <div v-else-if="stashes.detailError" class="local-error" role="alert"><span>{{ stashes.detailError.message }}</span><button :aria-label="t('uiReloadStash5665ff')" @click="refresh"><RefreshCw :size="14" />{{ t('uiReload778497') }}</button></div>
      <section v-else-if="stashes.detail" class="stash-files">
        <h2>{{ t('uiChangedFiles33bb4a') }} <span>{{ stashes.detail.files.length }}</span></h2>
        <div v-if="stashes.detail.files.length === 0" class="module-state">{{ t('uiNoFileChanges42ef47') }}</div>
        <button v-for="file in stashes.detail.files" :key="file.path + ':' + file.untracked" :class="{ selected: stashes.selectedFilePath === file.path && stashes.selectedFileUntracked === file.untracked }" :aria-label="(t('uiViewStashedFile99e1cc') + ' ') + file.path + (file.untracked ? t('uiUntrackedfd295b') : '')" :disabled="stashes.submitting" @click="selectFile(file.path, file.untracked)">
          <b>{{ file.status }}</b><span :title="file.oldPath ? file.oldPath + ' → ' + file.path : file.path">{{ file.path }}</span><small v-if="file.untracked">{{ t('uiNotTracking2f345a') }}</small><small v-if="file.binary">{{ t('uiBinary78ff74') }}</small><small v-else>+{{ file.additions ?? 0 }} / -{{ file.deletions ?? 0 }}</small>
        </button>
      </section>
      <section class="stash-diff" :aria-label="t('uiStashFileDiff783967')">
        <div v-if="stashes.diffLoading" class="module-state"><LoaderCircle :size="18" class="spin" />{{ t('uiLoadingDiff59e762') }}</div>
        <div v-else-if="stashes.diffError" class="local-error" role="alert"><span>{{ stashes.diffError.message }}</span><button :aria-label="t('uiRetryStashDiff6bc16a')" @click="stashes.selectedFilePath && selectFile(stashes.selectedFilePath, stashes.selectedFileUntracked)"><RefreshCw :size="14" />{{ t('uiRetrye2d53a') }}</button></div>
        <div v-else-if="stashes.fileDiff?.binary" class="module-state"><FileQuestion :size="20" />{{ t('uiBinaryFilesCannotDisplayATextDiff6deddb') }}</div>
        <div v-else-if="stashes.fileDiff && !stashes.fileDiff.hunks.length" class="module-state">{{ t('uiNoTextDiff064f06') }}</div>
        <template v-else-if="stashes.fileDiff">
          <div v-for="hunk in stashes.fileDiff.hunks" :key="hunk.index" class="readonly-hunk">
            <header>{{ hunk.header }}</header>
            <div v-for="(line, index) in hunk.lines" :key="index" :class="line.kind"><code>{{ line.oldLine ?? "" }}</code><code>{{ line.newLine ?? "" }}</code><pre>{{ line.kind === "addition" ? "+" : line.kind === "deletion" ? "-" : " " }}{{ line.content }}</pre></div>
          </div>
        </template>
      </section>
    </template>
    <ConfirmDialog v-if="pendingPop" :title="t('uiPopStash0d142f')" :description="t('uiRemovesThisStashOnlyAfterASuccessfulApplicationWithoutConfli0dcc1d')" :confirm-label="t('uiConfirmPop7f4037')" :busy="stashes.submitting" :confirm-disabled="disabled" @cancel="cancelPop" @confirm="pop">
      <p class="pop-target">{{ pendingPop.selector }} · {{ pendingPop.description }}</p>
    </ConfirmDialog>
  </div>
</template>

<style scoped>
.stash-detail { display: grid; align-content: start; }
.stash-metadata { min-width: 0; padding: 18px 24px; border-bottom: 1px solid var(--border); }
.detail-title { display: flex; gap: 10px; min-width: 0; }
.detail-title > svg { flex: 0 0 auto; margin-top: 2px; }
.detail-title > div { min-width: 0; }
.detail-title span { font-size: 11px; color: var(--text-muted); overflow-wrap: anywhere; }
h1 { margin: 4px 0 0; font-size: 16px; letter-spacing: 0; overflow-wrap: anywhere; }
.metadata { display: flex; flex-wrap: wrap; gap: 6px 16px; margin-top: 12px; font-size: 11px; color: var(--text-muted); }
.metadata code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.metadata time { overflow-wrap: anywhere; }
.stash-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 14px; }
.stash-actions button, .local-error button { display: flex; align-items: center; justify-content: center; gap: 6px; min-height: 32px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }
.stash-actions button:first-child { background: var(--primary); color: white; border-color: var(--primary); }
.stash-files { min-width: 0; padding: 12px 16px; border-bottom: 1px solid var(--border); }
h2 { margin: 0 8px 7px; font-size: 12px; }
h2 span { color: var(--text-muted); }
.stash-files > button { display: flex; width: 100%; min-width: 0; min-height: 34px; align-items: center; gap: 7px; padding: 0 8px; border-radius: var(--radius-md); background: transparent; text-align: left; }
.stash-files > button:hover, .stash-files > button.selected { background: var(--surface-muted); }
.stash-files b { width: 20px; flex: 0 0 auto; color: var(--primary); }
.stash-files button span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.stash-files small { flex: 0 0 auto; color: var(--text-muted); font-size: 10px; }
.stash-diff { min-width: 0; overflow: auto; }
.readonly-hunk { min-width: 440px; font-family: var(--font-code); font-size: 11px; }
.readonly-hunk > header { padding: 7px 12px; background: var(--primary-soft); color: var(--primary); }
.readonly-hunk > div { display: grid; grid-template-columns: 46px 46px minmax(0, 1fr); min-height: 22px; }
.readonly-hunk code { padding: 3px 7px; border-right: 1px solid var(--border); color: var(--text-muted); text-align: right; }
.readonly-hunk pre { margin: 0; padding: 3px 8px; white-space: pre-wrap; overflow-wrap: anywhere; }
.readonly-hunk .addition { background: var(--diff-add-bg); }
.readonly-hunk .deletion { background: var(--diff-delete-bg); }
.local-error { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; padding: 14px 24px; color: var(--danger); overflow-wrap: anywhere; }
.pop-target { max-height: 120px; overflow: auto; overflow-wrap: anywhere; color: var(--text-muted); }
@container (max-width: 450px) { .stash-metadata { padding: 14px; } .stash-files { padding: 10px 8px; } }
</style>
