<script setup lang="ts">
import { onBeforeUnmount } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import ConflictEditor from "@/features/conflicts/ConflictEditor.vue";
import { useConflictSuggestionStore } from "@/stores/conflictSuggestion";
const suggestion = useConflictSuggestionStore();
onBeforeUnmount(() => suggestion.dismissPreview());
</script>
<template>
  <ConfirmDialog v-if="suggestion.preview" class="suggestion-preview" title="预览 AI 建议：仅替换草稿"
    description="确认后替换当前解决草稿，尚未写入或暂存。请检查完整内容，再使用“保存并标记已解决”。"
    confirm-label="确认填入解决草稿" :busy="suggestion.busy" :confirm-disabled="!suggestion.canPreview"
    @cancel="suggestion.dismissPreview" @confirm="suggestion.confirmApply" @keydown.esc="suggestion.dismissPreview">
    <p class="path">{{ suggestion.preview.path }}</p>
    <div class="comparison">
      <section><h3>现有解决草稿</h3><p v-if="suggestion.preview.beforeDeleted">当前草稿选择删除文件</p><ConflictEditor v-else :model-value="suggestion.preview.before" readonly label="应用前草稿" /></section>
      <section><h3>AI 建议替换内容</h3><p v-if="suggestion.preview.candidate === ''">候选为空文本，将保留空文件，不会删除文件。</p><ConflictEditor :model-value="suggestion.preview.candidate" :compare-text="suggestion.preview.before" comparison-label="对比现有草稿" readonly label="建议替换内容" /></section>
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
