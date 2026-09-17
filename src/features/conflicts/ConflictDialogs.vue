<script setup lang="ts">
import { formatDisplayPath } from "@/lib/formatPath";
import { computed } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import { hasConflictMarkers, useConflictsStore } from "@/stores/conflicts";
const conflicts = useConflictsStore();
const intent = computed(() => conflicts.confirmation);
const title = computed(() => intent.value?.kind === "save" ? "保存并标记已解决" : intent.value?.kind === "reload" ? "丢弃草稿并重新读取" : intent.value?.kind === "abort" ? "中止当前操作" : "继续当前操作");
const label = computed(() => intent.value?.kind === "save" ? "确认保存解决结果" : intent.value?.kind === "reload" ? "确认丢弃草稿" : intent.value?.kind === "abort" ? "确认中止" : "确认继续");
const markers = computed(() => intent.value?.kind === "save" && intent.value.resolution.kind === "text" && hasConflictMarkers(intent.value.resolution.text));
const needsAcknowledgement = computed(() => markers.value && intent.value?.kind === "save" && intent.value.resolution.kind === "text" && !intent.value.resolution.acknowledgeMarkers);
const preview = computed(() => {
  if (intent.value?.kind !== "save") return "";
  const resolution = intent.value.resolution;
  if (resolution.kind === "text") return resolution.text;
  if (resolution.kind === "delete") return "删除文件";
  const version = conflicts.drafts[intent.value.path]?.detail[resolution.kind];
  return version?.exists ? version.text ?? `${version.kind} · ${version.byteLength} bytes` : "所选版本不存在，将删除文件";
});
</script>
<template>
  <ConfirmDialog v-if="intent" :title="title" :confirm-label="label" :busy="conflicts.submitting" :confirm-disabled="needsAcknowledgement || (intent.kind === 'continue' && !conflicts.canContinue)" :danger="intent.kind === 'abort' || intent.kind === 'reload'" @cancel="conflicts.cancelConfirmation" @confirm="conflicts.confirm">
    <div class="conflict-confirmation"><code>{{ formatDisplayPath(intent.rootPath) }}</code><p>分支：{{ intent.branch ?? '分离 HEAD' }}</p>
      <template v-if="intent.kind === 'save'"><strong>{{ intent.path }}</strong><p>确认后写入结果，并仅暂存这个文件。原文件将保留恢复副本。</p><pre>{{ preview.slice(0, 4000) }}{{ preview.length > 4000 ? '\n[预览已截断]' : '' }}</pre><label v-if="markers && intent.resolution.kind === 'text'" class="marker-warning"><input v-model="intent.resolution.acknowledgeMarkers" type="checkbox" aria-label="确认保留冲突标记" />结果仍含冲突标记，确认保留</label></template>
      <p v-else-if="intent.kind === 'reload'">{{ intent.path }} 的未保存草稿将被丢弃。</p>
      <template v-else-if="intent.kind === 'continue'"><p>继续 {{ conflicts.snapshot?.continueAction }}，将使用以下已暂存内容：</p><ul><li v-for="path in conflicts.snapshot?.stagedFiles" :key="path">{{ path }}</li></ul><p v-if="!conflicts.snapshot?.stagedFiles.length">没有已暂存文件。Git 可能拒绝空提交。</p></template>
      <p v-else>中止 {{ intent.action }}。工作区将由 Git 恢复，所有未保存的解决草稿将被丢弃。</p>
      <div v-if="conflicts.error" role="alert" class="error"><p>{{ conflicts.error.message }}</p><details v-if="conflicts.error.diagnostics"><summary>诊断信息</summary><pre>{{ conflicts.error.diagnostics }}</pre></details></div><p v-if="conflicts.recoveryPath">原文件恢复副本：<code>{{ conflicts.recoveryPath }}</code></p>
    </div>
  </ConfirmDialog>
</template>
<style scoped>
.conflict-confirmation { max-height: 65vh; overflow: auto; min-width: 0; overflow-wrap: anywhere; font-size: 12px; }code, strong { display: block; margin-top: 8px; }code { color: var(--text-muted); font-size: 11px; }p { line-height: 1.6; }pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 180px; overflow: auto; padding: 8px; background: var(--surface-muted); }ul { padding-left: 20px; max-height: 160px; overflow: auto; }.marker-warning { display: flex; gap: 6px; align-items: flex-start; color: var(--warning); }.error { color: var(--danger); }
</style>
