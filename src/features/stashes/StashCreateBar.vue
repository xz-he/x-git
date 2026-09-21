<script setup lang="ts">
import { t } from '@/lib/i18n';
import { Archive, LoaderCircle } from "@lucide/vue";
import { computed } from "vue";
import { useStashesStore } from "@/stores/stashes";
import { useRepositoryStore } from "@/stores/repository";
const stashes = useStashesStore();
const repositories = useRepositoryStore();
const allSelected = computed(() => stashes.availableFiles.length > 0 && stashes.selectedPaths.length === stashes.availableFiles.length);
function toggleAll(): void { stashes.selectedPaths = allSelected.value ? [] : stashes.availableFiles.map(file => file.path); }
function create(): void { void stashes.create().catch(() => undefined); }
</script>

<template>
  <form class="stash-create" @submit.prevent="create">
    <input v-model="stashes.message" :aria-label="t('uiStashMessage020af1')" :placeholder="t('uiStashMessageOptionalc1141a')" :disabled="stashes.submitting" />
    <label><input v-model="stashes.includeUntracked" type="checkbox" :aria-label="t('uiIncludeUntrackedFilesbdbf87')" :disabled="stashes.submitting" />{{ t('uiIncludeUntrackedFilesbdbf87') }}</label>
    <label><input v-model="stashes.selectFiles" type="checkbox" :aria-label="t('uiSelectSpecificFiles409700')" :disabled="stashes.submitting" />{{ t('uiSelectSpecificFiles409700') }}</label>
    <div v-if="stashes.selectFiles" class="file-picker">
      <label class="select-all"><input type="checkbox" :aria-label="t('uiSelectAllStashFiles453a72')" :checked="allSelected" :indeterminate="stashes.selectedPaths.length > 0 && !allSelected" :disabled="stashes.submitting || !stashes.availableFiles.length" @change="toggleAll" />{{ t('uiSelectAll3e44b2') }} <span>{{ t('uiSelectedf24ddc') }} {{ stashes.selectedPaths.length }} / {{ stashes.availableFiles.length }}</span></label>
      <div class="file-options" :aria-label="t('uiChooseFilesToStash12e25c')">
        <label v-for="file in stashes.availableFiles" :key="file.path" class="file-option" :title="file.oldPath ? file.oldPath + ' → ' + file.path : file.path">
          <input v-model="stashes.selectedPaths" type="checkbox" :value="file.path" :aria-label="(t('uiStashFilebc7958') + ' ') + file.path" :disabled="stashes.submitting" />
          <span class="file-path">{{ file.oldPath ? file.oldPath + ' → ' : '' }}{{ file.path }}</span>
          <small>{{ file.indexStatus === '?' ? t('uiNotTracking2f345a') : file.staged && file.unstaged ? t('uiStagedAndUnstaged1a376c') : file.staged ? t('staged') : t('unstaged') }}</small>
        </label>
        <p v-if="!stashes.availableFiles.length" class="empty-files">{{ t('uiNoFilesToSelectd81f0e') }}</p>
      </div>
      <p class="selection-hint">{{ t('uiStashesBothStagedAndUnstagedChangesForTheSelectedFilesd812cf') }}</p>
    </div>
    <button type="submit" data-action="create-stash" :disabled="!repositories.snapshot || !stashes.canMutate || (stashes.selectFiles && !stashes.selectedPaths.length)">
      <LoaderCircle v-if="stashes.submitting" :size="14" class="spin" /><Archive v-else :size="14" />
      {{ stashes.submitting ? t('uiRunning0a7f07') : stashes.selectFiles ? t('msgStashSelectedFilese9be9d', { p0: stashes.selectedPaths.length }) : t('uiCreateStash59210b') }}
    </button>
  </form>
</template>

<style scoped>
.stash-create { display: grid; gap: 10px; padding: 12px 14px; border-bottom: 1px solid var(--border); }
.stash-create > input { width: 100%; min-width: 0; height: 32px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text); }
label { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-muted); }
label input { margin: 0; accent-color: var(--primary); }
button { display: flex; height: 32px; align-items: center; justify-content: center; gap: 6px; border-radius: var(--radius-md); background: var(--primary); color: white; }
.file-picker { min-width: 0; overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-md); }
.select-all { padding: 8px; background: var(--surface-muted); }
.select-all span { margin-left: auto; }
.file-options { max-height: 220px; overflow: auto; }
.file-option { min-height: 32px; padding: 6px 8px; }
.file-option:hover { background: var(--surface-muted); }
.file-option input { flex-shrink: 0; }
.file-path { flex: 1; min-width: 0; overflow-wrap: anywhere; color: var(--text); }
.file-option small { flex-shrink: 0; font-size: 10px; }
.empty-files, .selection-hint { margin: 0; padding: 8px; font-size: 11px; color: var(--text-muted); }
.selection-hint { border-top: 1px solid var(--border); }
</style>
