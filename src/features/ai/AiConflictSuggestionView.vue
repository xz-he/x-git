<script setup lang="ts">
import { computed, ref } from "vue";
import ConflictEditor from "@/features/conflicts/ConflictEditor.vue";
import ConflictSuggestionPreview from "./ConflictSuggestionPreview.vue";
import { conflictSideLabels } from "@/lib/aiConflict";
import { useAiStore } from "@/stores/ai";
import { useConflictSuggestionStore } from "@/stores/conflictSuggestion";
import { useConflictsStore } from "@/stores/conflicts";
const ai = useAiStore();
const suggestion = useConflictSuggestionStore();
const conflicts = useConflictsStore();
const copyMessage = ref("");
const context = computed(() => suggestion.result?.context ?? ai.context?.conflict);
const labels = computed(() => conflictSideLabels(context.value?.operationKind));
const operationLabel = computed(() => ({ none: "索引冲突", merge: "合并", rebase: "变基", cherryPick: "拣选提交（Cherry-pick）", revert: "回滚提交（Revert）" })[context.value?.operationKind ?? "none"]);
const canRetry = computed(() => suggestion.owned && !suggestion.startReason
  && conflicts.current?.detail.path === ai.conflictTarget?.relativePath
  && conflicts.current?.detail.token === ai.conflictTarget?.token);
function retry(): void { if (canRetry.value) void suggestion.start().catch(() => undefined); }
async function copy(): Promise<void> {
  const result = suggestion.result;
  if (!result) return;
  try {
    await navigator.clipboard.writeText(result.kind === "text" ? result.resolvedText! : [result.summary, result.explanation, ...result.risks, ...result.contextMissing].join("\n\n"));
    copyMessage.value = "已复制";
  } catch { copyMessage.value = "复制失败，请检查剪贴板权限。"; }
}
</script>
<template>
  <div class="conflict-suggestion-view">
    <p class="scope">仅发送选中文件的三方版本与磁盘工作内容，未保存草稿不发送。建议需预览后填入草稿。</p>
    <strong class="path">{{ context?.path ?? ai.conflictTarget?.relativePath }}</strong>
    <p v-if="ai.running" role="status">正在分析冲突，完整结果返回后才能预览。关闭助手不会停止任务。</p>
    <p v-else-if="ai.status === 'cancelled'" role="status">任务已停止，未完成的结果不能应用。</p>
    <p v-if="ai.error" class="error" role="alert">{{ ai.error.message }}</p>
    <template v-if="suggestion.result">
      <section><h3>{{ suggestion.result.summary }}</h3><p class="prose">{{ suggestion.result.explanation }}</p></section>
      <section v-if="suggestion.result.risks.length"><h3>风险与检查项</h3><ul><li v-for="(risk, index) in suggestion.result.risks" :key="index">{{ risk }}</li></ul></section>
      <section v-if="suggestion.result.contextMissing.length"><h3>缺少的上下文</h3><ul><li v-for="(item, index) in suggestion.result.contextMissing" :key="index">{{ item }}</li></ul></section>
      <p v-if="suggestion.result.kind === 'adviceOnly'" class="scope">当前仅提供分析建议，没有可应用的候选内容。</p>
      <section v-else class="candidate"><h3>完整候选内容</h3><p v-if="suggestion.result.resolvedText === ''">空文本候选：保留空文件，不删除文件。</p><ConflictEditor :model-value="suggestion.result.resolvedText ?? ''" readonly label="AI 冲突候选内容" /></section>
      <button aria-label="复制冲突建议" @click="copy">{{ suggestion.result.kind === 'text' ? '复制候选内容' : '复制分析建议' }}</button>
      <p v-if="copyMessage" role="status">{{ copyMessage }}</p>
      <button v-if="suggestion.result.kind === 'text'" class="primary" aria-label="预览应用建议" :disabled="!suggestion.canPreview || suggestion.busy" @click="suggestion.requestPreview">{{ suggestion.busy ? '正在校验源文件…' : '预览应用建议' }}</button>
      <p v-if="suggestion.result.kind === 'text' && !suggestion.canPreview" class="scope">请返回原冲突文件预览；源文件或仓库已变化时，请重新读取并生成建议。</p>
    </template>
    <p v-if="suggestion.error" class="error" role="alert">{{ suggestion.error.message }}</p>
    <p v-if="suggestion.appliedMessage" class="success" role="status">{{ suggestion.appliedMessage }}</p>
    <div class="actions"><button aria-label="定位建议来源冲突" :disabled="!suggestion.owned || conflicts.busy" @click="suggestion.navigateToTarget">定位源文件</button><button aria-label="重新生成冲突建议" :disabled="!canRetry" @click="retry">重新生成</button></div>
    <p v-if="!ai.running && !canRetry" class="scope">重新生成需选中原冲突版本；文件有变化时，请在工作台重新读取后发起新建议。</p>
    <details v-if="context" class="metadata"><summary>本次建议的源版本</summary><dl>
      <dt>操作</dt><dd>{{ operationLabel }}</dd>
      <dt>共同祖先</dt><dd>{{ context.baseOid ?? '不存在' }}</dd>
      <dt>{{ labels.ours }}（索引版本 2）</dt><dd>{{ context.oursOid ?? '不存在' }}</dd>
      <dt>{{ labels.theirs }}（索引版本 3）</dt><dd>{{ context.theirsOid ?? '不存在' }}</dd>
      <dt>文件校验标识</dt><dd>{{ context.token }}</dd><dt>上下文指纹</dt><dd>{{ context.fingerprint }}</dd>
    </dl></details>
    <ConflictSuggestionPreview />
  </div>
</template>
<style scoped>
.conflict-suggestion-view { display: grid; align-content: start; gap: 12px; padding: 16px; font-size: 12px; overflow-wrap: anywhere; }
p, h3 { margin: 0; }h3 { margin-bottom: 8px; font-size: 13px; }.scope { color: var(--text-muted); font-size: 11px; line-height: 1.6; }.prose { white-space: pre-wrap; line-height: 1.6; }.path { font-family: var(--font-code); }
ul { margin: 0; padding-left: 18px; }li { margin: 5px 0; line-height: 1.5; }
.candidate { min-width: 0; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden; }.candidate h3, .candidate p { padding: 9px; margin: 0; }.candidate h3 { background: var(--surface-muted); }.candidate :deep(.conflict-editor) { height: 260px; }
button { min-height: 34px; padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }button.primary { background: var(--primary); color: white; }button:disabled { opacity: .48; cursor: not-allowed; }.actions { display: flex; flex-wrap: wrap; gap: 8px; }
.error, .success { padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md); line-height: 1.5; }.error { color: var(--danger); }.success { color: var(--success); }
.metadata { color: var(--text-muted); font-size: 11px; }.metadata summary { cursor: pointer; }.metadata dl { display: grid; gap: 4px; }.metadata dd { margin: 0 0 8px; font-family: var(--font-code); }
</style>
