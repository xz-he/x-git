<script setup lang="ts">
import { t } from '@/lib/i18n';
import { FileSearch, RefreshCw, Copy } from "@lucide/vue";
import { computed, ref } from "vue";
import type { AiReviewIssue } from "@/lib/backend/types";
import { useAiStore } from "@/stores/ai";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";
import { useHistoryStore } from "@/stores/history";
import { useUiStore } from "@/stores/ui";
import ReviewFinding from "./ReviewFinding.vue";
import ReviewMarkdown from "./ReviewMarkdown.vue";
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
const reportSummary = computed(() => ai.status === "completed" ? result.value?.summary ?? "" : t('msgReviewIncompleteBatches52f716', { p0: ai.completedBatchCount, p1: ai.totalBatchCount, p2: result.value?.summary ?? "" }));
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
    navigationError.value = t('uiCouldNotOpenThisReviewLocationRefreshTheCommitOrFileAndTryAgd34792');
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
    await navigator.clipboard.writeText(reviewReport(issues.value, source.value, frozen.value, reportSummary.value, warnings.value, uncovered.value, result.value?.reviewedFiles, skippedBinaries.value, result.value?.markdown));
    copyStatus.value = t('uiReviewResultsCopied0c0353');
  } catch { copyStatus.value = t('uiCopyFailedPleaseTryAgain7bdd9c'); }
  finally { copying.value = false; }
}
</script>

<template>
  <div class="review-view">
    <section class="review-context" :aria-label="t('uiReviewScope00ba71')">
      <strong>{{ source?.kind === 'commit' ? t('uiCommitReview85cfa0') : source?.kind === 'stagedFiles' ? t('uiSelectedStagedFilesReview88649d') : t('uiStagedChangesReview95ae0d') }}</strong>
      <template v-if="source?.kind === 'commit'"><code>{{ frozen?.resolvedCommit ?? source.revision }}</code><span>{{ t('uiComparedWith2b1cdd') }}{{ frozen ? frozen.baseCommit ?? t('uiEmptyTreeInitialCommitb82617') : t('uiResolvingFirstParentbfa5cb') }}</span><small>{{ t('uiMergeCommitsAreComparedWithTheirFirstParentEvidenceComesFrom8544d6') }}</small></template>
      <small v-else>{{ t('uiReviewsStagedChangesOnlyRelatedDefinitionsAreReadFromTheInde826afc') }}</small>
      <details v-if="source?.kind === 'stagedFiles'" open><summary>{{ t('uiSelectedFiles6db64a') }}{{ source.paths.length }}）</summary><ul class="selected-review-paths"><li v-for="path in source.paths" :key="path">{{ path }}</li></ul><small>{{ t('uiReviewsOnlyStagedChangesInTheFilesAboveIncludingOriginalPath5542e4') }}</small></details>
      <template v-if="frozen"><span>{{ t('uiChangedFiles4e7b91') }}{{ frozen.changedFileCount }} {{ t('uiRulesde2be5') }}{{ frozen.skill.directory }}/SKILL.md</span><span>{{ frozen.skill.name }} {{ frozen.skill.version }}</span><details><summary>{{ t('uiRulesAndEvidenceSources31d499') }}</summary><code>{{ t('uiRulesFingerprint4c4f14') }}{{ frozen.skill.fingerprint }}</code><ul><li v-for="file in frozen.skill.files" :key="file">{{ file }}</li></ul><ul v-if="frozen.evidenceSources.length"><li v-for="entry in frozen.evidenceSources" :key="`${entry.path}:${entry.startLine}:${entry.revision}`"><code>{{ entry.path }}:{{ entry.startLine }}–{{ entry.endLine }} @ {{ entry.revision }}</code></li></ul></details></template>
    </section>

    <div v-if="ai.status === 'failed' && ai.error" class="state-banner error" role="alert"><span>{{ ai.error.message }}</span><button :aria-label="t('uiRetryReview658592')" :disabled="retrying || wrongRoot" @click="retry"><RefreshCw :size="14" />{{ t('uiRetrye2d53a') }}</button></div>
    <div v-else-if="ai.status === 'cancelled'" class="state-banner"><span>{{ t('uiStoppedCompletedReviewResultsHaveBeenKepte44f71') }}</span><button :aria-label="t('uiReviewAgain1d68a5')" :disabled="retrying || wrongRoot" @click="retry"><RefreshCw :size="14" />{{ t('uiReviewAgain1d68a5') }}</button></div>
    <details v-if="ai.status === 'failed' && ai.error?.diagnostics" class="error-details"><summary>{{ t('uiViewFailureDetailsc0f740') }}</summary><pre>{{ ai.error.diagnostics }}</pre></details>
    <p v-if="wrongRoot" class="error">{{ t('uiThisResultBelongsToAnotherRepositoryAndCannotBeLocatedHere1c423c') }}</p>
    <p v-if="navigationError || changes.navigationError" class="error" role="alert">{{ navigationError || changes.navigationError }}</p>

    <ReviewMarkdown v-if="result?.markdown" :content="result.markdown" />
    <section v-else-if="issues.length" class="findings" :aria-label="t('uiReviewFindings19460b')"><ReviewFinding v-for="(issue, index) in issues" :key="`${issue.path}:${issue.startLine}:${index}`" :issue="issue" :navigation-disabled="navigating || wrongRoot || repositories.navigationBusy" @reveal="reveal" /></section>
    <section v-else-if="ai.status === 'completed'" class="empty-state" :aria-label="t('uiReviewComplete544b13')"><FileSearch :size="24" /><strong>{{ t('uiReviewComplete544b13') }}</strong><span>{{ result?.summary }}</span><small>{{ t('uiThisConclusionAppliesOnlyToReviewedContentSeeUncoveredAreasBb75ba0') }}</small></section>
    <section v-else-if="ai.running" class="empty-state" :aria-label="t('uiReviewingb0742f')"><FileSearch :size="24" /><strong>{{ t('uiReviewingb0742f') }}{{ source?.kind === 'commit' ? t('uiHistoricalCommite60d56') : t('uiStagedChanges2fe2df') }}</strong><span>{{ ai.progressMessage || t('uiResultsAppearInBatches343095') }}</span></section>

    <p v-if="result && ai.status !== 'completed'" class="state-banner">{{ t('uiReviewIncomplete4ee5ac') }}{{ ai.completedBatchCount }}/{{ ai.totalBatchCount }} {{ t('uibatchesOnlyCompletedResultsAreShownBelowf58c0e') }}</p>
    <section v-if="result" class="compact-list" :aria-label="t('uiReviewedFilesff189d')"><h3>{{ t('uiReviewedFilesb08701') }}{{ result.reviewedFiles.length }}）</h3><ul v-if="result.reviewedFiles.length"><li v-for="path in result.reviewedFiles" :key="path">{{ path }}</li></ul><p v-else>{{ t('uiNoFilesReviewedcb936d') }}</p></section>
    <section v-if="result" class="compact-list"><h3>{{ t('uiUncoveredAreas02a9f5') }}</h3><ul v-if="uncovered.length"><li v-for="item in uncovered" :key="item">{{ item }}</li></ul><p v-else>{{ t('uiNothingElseReportedThisDoesNotImplyCoverageOfCodeThatWasNotPb4a470') }}</p></section>
    <section v-if="skippedBinaries.length" class="compact-list"><h3>{{ t('uiSkippedBinaryFiles1b81e2') }}</h3><ul><li v-for="path in skippedBinaries" :key="path">{{ path }}</li></ul></section>
    <section v-if="frozen?.excludedFiles.length" class="compact-list"><h3>{{ t('uiBuildArtifactsExcludedByTheSkillc9a33b') }}</h3><ul><li v-for="path in frozen.excludedFiles" :key="path">{{ path }}</li></ul></section>
    <section v-if="warnings.length" class="compact-list warnings"><h3>{{ t('uiReviewNotes3eb706') }}</h3><ul><li v-for="warning in warnings" :key="warning">{{ warning }}</li></ul></section>
    <div v-if="issues.length || result" class="copy-result"><button :aria-label="t('uiCopyReviewResults952fd9')" :disabled="copying" @click="copyReport"><Copy :size="14" />{{ t('uiCopyReviewResults952fd9') }}</button><small role="status">{{ copyStatus }}</small></div>
  </div>
</template>

<style scoped>
.error-details { font-size: 12px; color: var(--text-muted); }
.error-details pre { white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; }
.selected-review-paths { max-height: 160px; overflow: auto; padding-left: 18px; }
.review-view { display: grid; min-width: 0; align-content: start; gap: 12px; padding: 14px; overflow-wrap: anywhere; }.review-context { display: grid; gap: 7px; padding: 11px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); font-size: 11px; line-height: 1.5; }.review-context code { white-space: pre-wrap; user-select: text; }small { color: var(--text-muted); }.state-banner { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md); font-size: 11px; }.state-banner span { flex: 1 1 140px; }.error { color: var(--danger); }.findings { display: grid; gap: 9px; }.empty-state { display: grid; min-height: 160px; place-content: center; justify-items: center; gap: 7px; color: var(--text-muted); text-align: center; font-size: 11px; }.empty-state strong { color: var(--text); font-size: 13px; }.compact-list { padding: 10px 11px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); font-size: 11px; line-height: 1.5; }.compact-list h3 { margin: 0 0 7px; color: var(--text-muted); font-size: 11px; }.compact-list ul { display: grid; gap: 5px; margin: 0; padding-left: 16px; }.warnings { border-color: color-mix(in srgb, var(--warning) 28%, var(--border)); }button { display: inline-flex; align-items: center; gap: 5px; padding: 6px 8px; background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-sm); font-size: 11px; }.copy-result { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }summary { cursor: pointer; }
</style>
