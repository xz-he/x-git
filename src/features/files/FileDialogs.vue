<script setup lang="ts">
import { formatDisplayPath } from "@/lib/formatPath";
import { computed } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import { parentDirectory, useFilesStore } from "@/stores/files";
const files = useFilesStore();
const current = computed(() => files.confirmation);
const labels = { createFile: "新建文件", createDirectory: "新建文件夹", rename: "重命名", delete: "移入恢复区" };
const title = computed(() => current.value ? labels[current.value.intent.kind] : "文件操作");
const name = computed(() => { const intent = current.value?.intent; return !intent || intent.kind === "delete" ? "" : intent.kind === "rename" ? intent.newName : intent.name; });
const target = computed(() => {
  const intent = current.value?.intent; if (!intent || intent.kind === "delete") return null;
  const parent = intent.kind === "rename" ? parentDirectory(intent.relativePath) : intent.parentDir;
  return [parent, name.value].filter(Boolean).join("/");
});
</script>
<template>
  <ConfirmDialog v-if="current" :title="title" :danger="current.intent.kind === 'delete'" :confirm-label="current.prepared ? '确认' + title : '预检查'" :confirm-disabled="!!files.nameError || files.preparing || !files.canMutate" :busy="files.submitting" @cancel="files.cancelConfirmation" @confirm="current.prepared ? files.confirm() : files.prepare()">
    <div class="file-confirmation"><p>仓库：<code>{{ formatDisplayPath(current.rootPath) }}</code></p><p>分支：{{ current.branch ?? '分离 HEAD' }}</p><p v-if="'relativePath' in current.intent">原路径：<strong>{{ current.intent.relativePath }}</strong></p><p v-else>父目录：<strong>{{ current.intent.parentDir || '仓库根目录' }}</strong></p>
      <label v-if="current.intent.kind !== 'delete'">{{ current.intent.kind === 'rename' ? '新名称' : '名称' }}<input :value="name" aria-label="文件名称" :disabled="files.submitting" autocomplete="off" spellcheck="false" @input="files.changeName(($event.target as HTMLInputElement).value)" /></label><p v-if="target">目标路径：<strong>{{ target }}</strong></p><p v-if="files.nameError" class="error">{{ files.nameError }}</p>
      <p v-if="current.intent.kind === 'delete'">确认后将整个选中项移入仓库恢复区，包含未跟踪和忽略内容。这不是系统回收站；不会自动清理恢复副本。</p><p v-else>仅改变工作目录，不自动暂存。新建内容为空；重命名限同一目录，已有目标不会被覆盖。</p>
      <div v-if="current.prepared" class="summary"><strong>预检查已完成</strong><p>{{ current.prepared.entryKind === 'directory' ? '目录' : '文件' }} · {{ current.prepared.fileCount }} 个文件 · {{ current.prepared.directoryCount }} 个目录</p><p>{{ current.prepared.totalBytes.toLocaleString() }} bytes · {{ current.prepared.nodeCount }} 个节点</p><p>确认前请暂停外部程序写入；内容或仓库状态变化后需重新预检查。</p></div><p v-else-if="files.preparing" role="status">正在检查路径和内容…</p>
      <div v-if="files.error" class="error" role="alert"><p>{{ files.error.message }}</p><details v-if="files.error.diagnostics"><summary>诊断信息</summary><pre>{{ files.error.diagnostics }}</pre></details></div>
    </div>
  </ConfirmDialog>
</template>
<style scoped>
.file-confirmation { max-height: 65vh; min-width: 0; overflow: auto; overflow-wrap: anywhere; font-size: 12px; }p { margin: 9px 0; line-height: 1.6; }code, strong { overflow-wrap: anywhere; }code { display: block; color: var(--text-muted); }label { display: grid; gap: 6px; }input { width: 100%; min-width: 0; padding: 8px; color: var(--text); background: var(--surface-app); border: 1px solid var(--border); border-radius: 4px; }.summary { margin-top: 12px; padding: 10px; background: var(--surface-muted); border-radius: 4px; }.error { color: var(--danger); }pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
