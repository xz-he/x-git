<script setup lang="ts">
import { t } from '@/lib/i18n';
import { formatDisplayPath } from "@/lib/formatPath";
import { FilePlus2, FolderPlus, RefreshCw } from "@lucide/vue";
import { useFilesStore } from "@/stores/files";
import { useRepositoryStore } from "@/stores/repository";
import FileTreeNode from "./FileTreeNode.vue";
const files = useFilesStore(); const repositories = useRepositoryStore();
</script>
<template>
  <section class="file-tree" :aria-label="t('uiRepositoryFileTreee8bbc8')">
    <header><strong>{{ t('files') }}</strong><button :title="t('uiNewFiled4526e')" :aria-label="t('uiNewFiled4526e')" :disabled="!files.canMutate" @click="files.requestOperation({ kind: 'createFile', parentDir: files.currentDirectory, name: '' })"><FilePlus2 :size="16" /></button><button :title="t('uiNewFolder95cf3c')" :aria-label="t('uiNewFolder95cf3c')" :disabled="!files.canMutate" @click="files.requestOperation({ kind: 'createDirectory', parentDir: files.currentDirectory, name: '' })"><FolderPlus :size="16" /></button><button :title="t('uiRefreshFileTree60676c')" :aria-label="t('uiRefreshFileTree60676c')" :disabled="files.busy" @click="files.refresh"><RefreshCw :size="15" /></button></header>
    <button class="root" :aria-label="t('uiSelectRepositoryRoot5b46b2')" :title="formatDisplayPath(repositories.snapshot?.rootPath)" :disabled="files.busy" @click="files.selectEntry()"><strong>{{ repositories.snapshot?.name }}</strong><span>{{ formatDisplayPath(repositories.snapshot?.rootPath) }}</span></button>
    <p class="hint">{{ t('uiWorkingDirectoryIncludesHiddenUntrackedAndIgnoredItems8b5778') }}</p>
    <div class="tree-scroll"><p v-if="files.directories['']?.loading" class="state" role="status">{{ t('uiReadingDirectorye3d062') }}</p><div v-if="files.directories['']?.error" class="state error" role="alert">{{ files.directories['']?.error?.message }}<button :disabled="files.busy" @click="files.loadDirectory('')">{{ t('uiRetrye2d53a') }}</button><details v-if="files.directories['']?.error?.diagnostics"><summary>{{ t('uiDiagnostics0b673e') }}</summary><pre>{{ files.directories['']?.error?.diagnostics }}</pre></details></div><ul><FileTreeNode v-for="entry in files.directories['']?.page?.entries" :key="entry.relativePath" :entry="entry" /></ul><p v-if="files.directories['']?.page?.totalEntries === 0" class="state">{{ t('uiWorkingDirectoryIsEmpty6d9d61') }}</p><button v-if="files.directories['']?.page?.nextCursor" class="more" :disabled="files.busy || files.directories['']?.loading" @click="files.loadMore('')">{{ t('uiLoadMoreShowing10dfff') }} {{ files.directories['']?.page?.entries.length }} / {{ files.directories['']?.page?.totalEntries }}）</button></div>
  </section>
</template>
<style scoped>
.file-tree { height: 100%; min-height: 0; display: flex; flex-direction: column; }header { display: flex; align-items: center; gap: 4px; padding: 8px 12px; border-bottom: 1px solid var(--border); min-height: 44px; }header strong { flex: 1; }header button { width: 28px; height: 28px; display: grid; place-items: center; background: transparent; border-radius: 4px; }header button:hover { background: var(--surface-muted); }.root { display: grid; gap: 5px; padding: 12px 14px 8px; min-width: 0; text-align: left; background: transparent; }.root span { color: var(--text-muted); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.hint { font-size: 10px; color: var(--text-muted); margin: 4px 14px 10px; }.tree-scroll { flex: 1; min-height: 0; overflow: auto; padding: 4px 8px; }ul { list-style: none; padding: 0; margin: 0; }.state { color: var(--text-muted); padding: 8px; font-size: 12px; overflow-wrap: anywhere; }.error { color: var(--danger); }.more, .state button { padding: 8px; color: var(--primary); background: transparent; }pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
