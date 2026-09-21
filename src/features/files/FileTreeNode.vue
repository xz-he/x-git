<script setup lang="ts">
import { t } from '@/lib/i18n';
import { ChevronDown, ChevronRight, File, Folder, LockKeyhole } from "@lucide/vue";
import type { RepositoryFileEntry } from "@/lib/backend/types";
import { useFilesStore } from "@/stores/files";
defineProps<{ entry: RepositoryFileEntry }>();
const files = useFilesStore();
</script>
<template>
  <li>
    <div class="row" :class="{ selected: files.selectedEntry?.relativePath === entry.relativePath }">
      <button v-if="entry.kind === 'directory'" class="expand" :aria-label="(files.expanded.has(entry.relativePath) ? (t('uiCollapseDirectory9922af') + ' ') : (t('uiExpandDirectory000701') + ' ')) + entry.relativePath" :aria-expanded="files.expanded.has(entry.relativePath)" :disabled="files.busy" @click="files.toggleDirectory(entry.relativePath)"><ChevronDown v-if="files.expanded.has(entry.relativePath)" :size="13" /><ChevronRight v-else :size="13" /></button><span v-else class="spacer" />
      <button class="entry" :aria-label="t('uiSelect70b208') + (entry.kind === 'directory' ? (t('uiDirectory41e524') + ' ') : entry.kind === 'file' ? (t('uifiles49deaf') + ' ') : (t('uiRestrictedItem29e8e7') + ' ')) + entry.relativePath" :title="entry.relativePath + (entry.reason ? '\n' + entry.reason : '')" :disabled="files.busy" @click="files.selectEntry(entry)"><Folder v-if="entry.kind === 'directory'" :size="15" /><LockKeyhole v-else-if="entry.kind === 'restricted'" :size="14" /><File v-else :size="14" /><span>{{ entry.name }}</span><small v-if="entry.gitStatus">{{ entry.gitStatus }}</small><small v-if="entry.kind === 'restricted'">{{ t('uiRestricted936333') }}</small></button>
    </div>
    <ul v-if="entry.kind === 'directory' && files.expanded.has(entry.relativePath)">
      <li v-if="files.directories[entry.relativePath]?.loading" class="state" role="status">{{ t('uiLoadingfcabad') }}</li>
      <li v-if="files.directories[entry.relativePath]?.error" class="state error" role="alert">{{ files.directories[entry.relativePath]?.error?.message }}<button :disabled="files.busy" @click="files.loadDirectory(entry.relativePath)">{{ t('uiRetrye2d53a') }}</button></li>
      <FileTreeNode v-for="child in files.directories[entry.relativePath]?.page?.entries" :key="child.relativePath" :entry="child" />
      <li v-if="files.directories[entry.relativePath]?.page?.totalEntries === 0" class="state">{{ t('uiEmptyDirectorye878e3') }}</li>
      <li v-if="files.directories[entry.relativePath]?.page?.nextCursor" class="state"><button :disabled="files.busy || files.directories[entry.relativePath]?.loading" @click="files.loadMore(entry.relativePath)">{{ t('uiLoadMore3a0fab') }}</button></li>
    </ul>
  </li>
</template>
<style scoped>
ul { list-style: none; margin: 0; padding-left: 14px; }li { min-width: 0; }.row { display: flex; min-width: 0; min-height: 30px; border-radius: 4px; }.row:hover { background: var(--surface-muted); }.row.selected { background: var(--primary-soft); color: var(--primary); }.expand, .spacer { flex: 0 0 20px; width: 20px; }.expand { display: grid; place-items: center; background: transparent; }.entry { display: flex; align-items: center; gap: 6px; min-width: 0; flex: 1; padding: 5px 6px 5px 0; background: transparent; text-align: left; }.entry span { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.entry svg, small { flex-shrink: 0; }small { font-size: 10px; color: var(--text-muted); }.state { padding: 6px 10px; font-size: 11px; color: var(--text-muted); overflow-wrap: anywhere; }.state button { background: transparent; color: var(--primary); padding: 4px; }.error { color: var(--danger); }
</style>
