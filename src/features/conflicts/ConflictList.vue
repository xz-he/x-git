<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed } from "vue";
import { FileWarning, RefreshCw, Play } from "@lucide/vue";
import { useConflictsStore } from "@/stores/conflicts";
const conflicts = useConflictsStore();
const retained = computed(() => Object.keys(conflicts.drafts).filter(path => conflicts.drafts[path]?.dirty && !conflicts.snapshot?.files.some(file => file.path === path)));
</script>
<template>
  <div class="conflict-list">
    <header><strong>{{ t('conflicts') }} <span>{{ conflicts.snapshot?.files.length ?? 0 }}</span></strong><button :title="t('uiRefreshConflictsffe69c')" :aria-label="t('uiRefreshConflictsffe69c')" :disabled="conflicts.busy || conflicts.loading" @click="conflicts.refresh"><RefreshCw :size="15" /></button></header>
    <div v-if="conflicts.loading" class="state" role="status">{{ t('uiLoadingConflicts8530a3') }}</div>
    <div v-if="conflicts.error" class="state error" role="alert">{{ conflicts.error.message }}<details v-if="conflicts.error.diagnostics"><summary>{{ t('uiDiagnostics0b673e') }}</summary><pre>{{ conflicts.error.diagnostics }}</pre></details></div>
    <details v-if="conflicts.recoveryPath" class="recovery"><summary>{{ t('uiOriginalFileRecoveryCopy7aa41f') }}</summary><code>{{ conflicts.recoveryPath }}</code></details>
    <div class="files">
      <button v-for="file in conflicts.snapshot?.files" :key="file.path" :aria-label="(t('uiViewConflictb33027') + ' ') + file.path" :aria-current="conflicts.selectedPath === file.path ? 'true' : undefined" :disabled="conflicts.busy" :class="{ selected: conflicts.selectedPath === file.path }" @click="conflicts.selectFile(file.path)">
        <FileWarning :size="16" /><span class="path">{{ file.path }}<small v-if="!file.supported">{{ file.reason }}</small></span><span class="badge">{{ conflicts.drafts[file.path]?.dirty ? t('uiDraft0f4368') : file.status }}</span>
      </button>
      <button v-for="path in retained" :key="path" :aria-label="(t('uiViewPreservedDraft9ebded') + ' ') + path" :disabled="conflicts.busy" @click="conflicts.selectFile(path)"><FileWarning :size="16" /><span class="path">{{ path }}</span><span class="badge">{{ t('uiPreservedDraft5a0045') }}</span></button>
      <div v-if="!conflicts.loading && conflicts.snapshot && !conflicts.snapshot.files.length" class="state">{{ t('uiNoUnresolvedConflicts8fb25d') }}</div>
    </div>
    <footer v-if="conflicts.snapshot?.continueAction"><button class="continue" :disabled="!conflicts.canContinue" @click="conflicts.requestContinue"><Play :size="15" />{{ t('uiContinue1fc1af') }}{{ conflicts.snapshot.continueAction === 'merge' ? t('merge') : conflicts.snapshot.continueAction === 'rebase' ? t('rebase') : conflicts.snapshot.continueAction === 'revert' ? ' Revert' : ' Cherry-pick' }}</button><p v-if="conflicts.hasDirtyDrafts">{{ t('uiUnsavedResolutionDraftsRemain52a3c3') }}</p></footer>
  </div>
</template>
<style scoped>
.conflict-list { display: flex; flex-direction: column; height: 100%; min-height: 0; }
header { display: flex; align-items: center; justify-content: space-between; min-height: 44px; padding: 0 12px; border-bottom: 1px solid var(--border); }
header span, .badge { color: var(--text-muted); font-size: 11px; }
header button { display: grid; place-items: center; width: 28px; height: 28px; background: transparent; }
.files { flex: 1; overflow: auto; }
.files button { width: 100%; min-height: 48px; display: flex; align-items: center; gap: 8px; text-align: left; padding: 10px 12px; background: transparent; border-bottom: 1px solid var(--border); }
.files button.selected { background: var(--primary-soft); }
.files svg { flex-shrink: 0; color: var(--warning); }.path { flex: 1; min-width: 0; overflow-wrap: anywhere; }small { display: block; color: var(--text-muted); margin-top: 4px; }.badge { flex-shrink: 0; }
.state { padding: 16px; color: var(--text-muted); overflow-wrap: anywhere; }.error { color: var(--danger); }
.recovery { padding: 10px 12px; font-size: 11px; overflow-wrap: anywhere; border-bottom: 1px solid var(--border); }.recovery code { display: block; margin-top: 6px; }pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 160px; overflow: auto; }
footer { border-top: 1px solid var(--border); padding: 12px; }footer p { font-size: 11px; color: var(--warning); }.continue { display: flex; gap: 6px; align-items: center; min-height: 32px; padding: 6px 10px; background: var(--primary); color: white; border-radius: 4px; }
</style>
