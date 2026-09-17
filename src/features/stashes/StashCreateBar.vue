<script setup lang="ts">
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
    <input v-model="stashes.message" aria-label="贮藏说明" placeholder="贮藏说明（可选）" :disabled="stashes.submitting" />
    <label><input v-model="stashes.includeUntracked" type="checkbox" aria-label="包含未跟踪文件" :disabled="stashes.submitting" />包含未跟踪文件</label>
    <label><input v-model="stashes.selectFiles" type="checkbox" aria-label="选择部分文件" :disabled="stashes.submitting" />选择部分文件</label>
    <div v-if="stashes.selectFiles" class="file-picker">
      <label class="select-all"><input type="checkbox" aria-label="全选贮藏文件" :checked="allSelected" :indeterminate="stashes.selectedPaths.length > 0 && !allSelected" :disabled="stashes.submitting || !stashes.availableFiles.length" @change="toggleAll" />全选 <span>已选 {{ stashes.selectedPaths.length }} / {{ stashes.availableFiles.length }}</span></label>
      <div class="file-options" aria-label="选择要贮藏的文件">
        <label v-for="file in stashes.availableFiles" :key="file.path" class="file-option" :title="file.oldPath ? file.oldPath + ' → ' + file.path : file.path">
          <input v-model="stashes.selectedPaths" type="checkbox" :value="file.path" :aria-label="'贮藏文件 ' + file.path" :disabled="stashes.submitting" />
          <span class="file-path">{{ file.oldPath ? file.oldPath + ' → ' : '' }}{{ file.path }}</span>
          <small>{{ file.indexStatus === '?' ? '未跟踪' : file.staged && file.unstaged ? '暂存及未暂存' : file.staged ? '已暂存' : '未暂存' }}</small>
        </label>
        <p v-if="!stashes.availableFiles.length" class="empty-files">没有可选择的文件</p>
      </div>
      <p class="selection-hint">按文件贮藏，包含所选文件的已暂存及未暂存变更。</p>
    </div>
    <button type="submit" data-action="create-stash" :disabled="!repositories.snapshot || !stashes.canMutate || (stashes.selectFiles && !stashes.selectedPaths.length)">
      <LoaderCircle v-if="stashes.submitting" :size="14" class="spin" /><Archive v-else :size="14" />
      {{ stashes.submitting ? "正在执行" : stashes.selectFiles ? `贮藏选中的 ${stashes.selectedPaths.length} 个文件` : "创建贮藏" }}
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
