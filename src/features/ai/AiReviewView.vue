<script setup lang="ts">
import { FileSearch, RefreshCw, Copy } from "@lucide/vue";
import { computed, ref } from "vue";
import type { AiReviewIssue } from "@/lib/backend/types";
import { useAiStore } from "@/stores/ai";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { useHistoryStore } from "@/stores/history";
import { useUiStore } from "@/stores/ui";
import ReviewFinding from "./ReviewFinding.vue";
import { reviewReport, severityRank } from "./reviewPresentation";

const ai = useAiStore();
const changes = useChangesStore();
const repositories = useRepositoryStore();
const history = useHistoryStore();
const ui = useUiStore();
const retrying = ref(false);
const navigating = ref(false);
const navigationError = ref("");
const copyStatus = ref("");
const copying = ref(false);
const result = computed(() => ai.status === "completed" ? ai.reviewResult : ai.partialReviewResult);
const reportSummary = computed(() => ai.status === "completed" ? result.value?.summary ?? "" : `审查未完成（${ai.completedBatchCount}/${ai.totalBatchCount} 批）\n${result.value?.summary ?? ""}`);
const frozen = computed(() => result.value?.context ?? ai.context?.review ?? undefined);
const source = computed(() => frozen.value?.source ?? ai.reviewSource);
const issues = computed(() => ai.partialIssues.map((issue, index) => ({ issue, index }))
  .sort((a, b) => severityRank[a.issue.severity] - severityRank[b.issue.severity] || a.index - b.index).map(({ issue }) => issue));
const warnings = computed(() => result.value?.warnings ?? []);
const skippedBinaries = computed(() => result.value?.skippedBinaryFiles ?? ai.context?.skippedBinaryFiles ?? []);
const uncovered = computed(() => result.value?.uncovered ?? []);
const wrongRoot = computed(() => !!ai.runRoot && ai.runRoot !== repositories.snapshot?.rootPath);

async function reveal(issue: AiReviewIssue): Promise<void> {
  const line = issue.startLine ?? issue.endLine;
  if (!line || navigating.value || wrongRoot.value || repositories.navigationBusy) return;
  navigationError.value = "";
  navigating.value = true;
  const root = repositories.snapshot?.rootPath;
  const generation = repositories.generation;
  const target = source.value;
  try {
    if (target?.kind === "commit" && root) {
      const revision = frozen.value?.resolvedCommit ?? target.revision;
      await history.ensureLoaded(root, generation);
      if (repositories.snapshot?.rootPath !== root || repositories.generation !== generation) return;
      ui.openView("history");
      await history.selectCommit(revision);
      if (repositories.snapshot?.rootPath !== root || repositories.generation !== generation || history.detail?.hash !== revision) return;
      await history.selectFile(issue.path);
    } else {
      await changes.revealStagedLine(issue.path, line);
    }
  } catch {
    navigationError.value = "无法打开该审查位置，请刷新提交或文件后重试。";
  } finally { navigating.value = false; }
}

async function retry(): Promise<void> {
  if (retrying.value || ai.running || wrongRoot.value) return;
  retrying.value = true;
  try { await ai.startReview(source.value); } catch { /* Store exposes the error. */ }
  finally { retrying.value = false; }
}

async function copyReport(): Promise<void> {
  if (copying.value) return;
  copying.value = true;
  copyStatus.value = "";
  try {
    await navigator.clipboard.writeText(reviewReport(issues.value, source.value, frozen.value, reportSummary.value, warnings.value, uncovered.value, result.value?.reviewedFiles, skippedBinaries.value));
    copyStatus.value = "审查结果已复制。";
  } catch { copyStatus.value = "复制失败，请重试。"; }
  finally { copying.value = false; }
}
</script>

<template>
  <div class="review-view">
    <section class="review-context" aria-label="本次审查范围">
      <strong>{{ source?.kind === 'commit' ? '历史提交审查' : source?.kind === 'stagedFiles' ? '所选暂存文件审查' : '已暂存变更审查' }}</strong>
      <template v-if="source?.kind === 'commit'"><code>{{ frozen?.resolvedCommit ?? source.revision }}</code><span>比较基准：{{ frozen ? frozen.baseCommit ?? '空树（首次提交）' : '正在解析第一父提交' }}</span><small>合并提交按第一父提交比较；相关证据取自历史快照。</small></template>
      <small v-else>仅审查已暂存变更；按需读取索引中的相关定义作为证据。</small>
      <details v-if="source?.kind === 'stagedFiles'" open><summary>所选文件（{{ source.paths.length }}）</summary><ul class="selected-review-paths"><li v-for="path in source.paths" :key="path">{{ path }}</li></ul><small>只审查上述文件的暂存变更；重命名包含对应旧路径。其他索引文件仅按需作为相关证据。</small></details>
      <template v-if="frozen"><span>变更文件：{{ frozen.changedFileCount }} · 规则：{{ frozen.skill.directory }}/SKILL.md</span><span>{{ frozen.skill.name }} {{ frozen.skill.version }}</span><details><summary>本次规则与证据来源</summary><code>规则指纹：{{ frozen.skill.fingerprint }}</code><ul><li v-for="file in frozen.skill.files" :key="file">{{ file }}</li></ul><ul v-if="frozen.evidenceSources.length"><li v-for="entry in frozen.evidenceSources" :key="`${entry.path}:${entry.startLine}:${entry.revision}`"><code>{{ entry.path }}:{{ entry.startLine }}–{{ entry.endLine }} @ {{ entry.revision }}</code></li></ul></details></template>
    </section>

    <div v-if="ai.status === 'failed' && ai.error" class="state-banner error" role="alert"><span>{{ ai.error.message }}</span><button aria-label="重试审查" :disabled="retrying || wrongRoot" @click="retry"><RefreshCw :size="14" />重试</button></div>
    <div v-else-if="ai.status === 'cancelled'" class="state-banner"><span>已停止，保留已完成的审查结果。</span><button aria-label="重新审查" :disabled="retrying || wrongRoot" @click="retry"><RefreshCw :size="14" />重新审查</button></div>
    <details v-if="ai.status === 'failed' && ai.error?.diagnostics" class="error-details"><summary>查看失败原因</summary><pre>{{ ai.error.diagnostics }}</pre></details>
    <p v-if="wrongRoot" class="error">此结果属于其他仓库，无法在当前仓库定位。</p>
    <p v-if="navigationError || changes.navigationError" class="error" role="alert">{{ navigationError || changes.navigationError }}</p>

    <section v-if="issues.length" class="findings" aria-label="审查问题"><ReviewFinding v-for="(issue, index) in issues" :key="`${issue.path}:${issue.startLine}:${index}`" :issue="issue" :navigation-disabled="navigating || wrongRoot || repositories.navigationBusy" @reveal="reveal" /></section>
    <section v-else-if="ai.status === 'completed'" class="empty-state" aria-label="审查完成"><FileSearch :size="24" /><strong>未发现需要处理的问题</strong><span>{{ result?.summary }}</span><small>结论仅适用于已审范围，未覆盖部分见下方。</small></section>
    <section v-else-if="ai.running" class="empty-state" aria-label="正在审查"><FileSearch :size="24" /><strong>正在审查{{ source?.kind === 'commit' ? '历史提交' : '已暂存变更' }}</strong><span>{{ ai.progressMessage || '结果会按批次显示' }}</span></section>

    <p v-if="result && ai.status !== 'completed'" class="state-banner">审查未完成（{{ ai.completedBatchCount }}/{{ ai.totalBatchCount }} 批），以下仅包含已完成部分。</p>
    <section v-if="result" class="compact-list" aria-label="实际已审文件"><h3>实际已审文件（{{ result.reviewedFiles.length }}）</h3><ul v-if="result.reviewedFiles.length"><li v-for="path in result.reviewedFiles" :key="path">{{ path }}</li></ul><p v-else>无已审文件。</p></section>
    <section v-if="result" class="compact-list"><h3>未覆盖范围</h3><ul v-if="uncovered.length"><li v-for="item in uncovered" :key="item">{{ item }}</li></ul><p v-else>无额外报告；不代表覆盖未提供的代码。</p></section>
    <section v-if="skippedBinaries.length" class="compact-list"><h3>已跳过二进制文件</h3><ul><li v-for="path in skippedBinaries" :key="path">{{ path }}</li></ul></section>
    <section v-if="frozen?.excludedFiles.length" class="compact-list"><h3>按技能排除的构建产物</h3><ul><li v-for="path in frozen.excludedFiles" :key="path">{{ path }}</li></ul></section>
    <section v-if="warnings.length" class="compact-list warnings"><h3>审查提示</h3><ul><li v-for="warning in warnings" :key="warning">{{ warning }}</li></ul></section>
    <div v-if="issues.length || result" class="copy-result"><button aria-label="复制审查结果" :disabled="copying" @click="copyReport"><Copy :size="14" />复制审查结果</button><small role="status">{{ copyStatus }}</small></div>
  </div>
</template>

<style scoped>
.error-details { font-size: 12px; color: var(--text-muted); }
.error-details pre { white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; }
.selected-review-paths { max-height: 160px; overflow: auto; padding-left: 18px; }
.review-view { display: grid; min-width: 0; align-content: start; gap: 12px; padding: 14px; overflow-wrap: anywhere; }.review-context { display: grid; gap: 7px; padding: 11px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); font-size: 11px; line-height: 1.5; }.review-context code { white-space: pre-wrap; user-select: text; }small { color: var(--text-muted); }.state-banner { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md); font-size: 11px; }.state-banner span { flex: 1 1 140px; }.error { color: var(--danger); }.findings { display: grid; gap: 9px; }.empty-state { display: grid; min-height: 160px; place-content: center; justify-items: center; gap: 7px; color: var(--text-muted); text-align: center; font-size: 11px; }.empty-state strong { color: var(--text); font-size: 13px; }.compact-list { padding: 10px 11px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); font-size: 11px; line-height: 1.5; }.compact-list h3 { margin: 0 0 7px; color: var(--text-muted); font-size: 11px; }.compact-list ul { display: grid; gap: 5px; margin: 0; padding-left: 16px; }.warnings { border-color: color-mix(in srgb, var(--warning) 28%, var(--border)); }button { display: inline-flex; align-items: center; gap: 5px; padding: 6px 8px; background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-sm); font-size: 11px; }.copy-result { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }summary { cursor: pointer; }
</style>
