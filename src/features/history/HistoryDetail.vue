<script setup lang="ts">
import { t } from '@/lib/i18n';
import { GitCommitHorizontal, ScanSearch, LoaderCircle } from "@lucide/vue";
import { computed } from "vue";

import { useHistoryStore } from "@/stores/history";
import HistoryActionDialogs from "@/features/history/HistoryActionDialogs.vue";
import HistoryDiffFiles from "@/features/history/HistoryDiffFiles.vue";
import { useAiStore } from "@/stores/ai";
import { useSettingsStore } from "@/stores/settings";
import { useRepositoryStore } from "@/stores/repository";
import { useReviewSkill } from "@/features/ai/useReviewSkill";

const history = useHistoryStore();
const ai = useAiStore();
const settings = useSettingsStore();
const repositories = useRepositoryStore();
const skill = useReviewSkill();
const canReview = computed(() => skill.ready.value && !ai.running && !repositories.navigationBusy && !!settings.settings.apiKey.trim() && !!settings.settings.baseUrl.trim() && !!settings.settings.model.trim());

function reviewCommit(): void {
  const revision = history.detail?.hash;
  if (!revision || !canReview.value) return;
  settings.settings.aiDrawerOpen = true;
  void ai.startReview({ kind: "commit", revision }).catch(() => undefined);
}

function selectParent(hash: string): void {
  void history.selectCommit(hash).catch(() => undefined);
}

</script>

<template>
  <div class="detail-body history-detail">
    <div v-if="history.detailLoading" class="module-state" role="status"><LoaderCircle :size="22" class="spin" />{{ t('uiReadingCommitDetails912152') }}</div>
    <div v-else-if="!history.detail && history.error && history.selectedHash" class="module-state error" role="alert">{{ history.error.message }}<button @click="selectParent(history.selectedHash!)">{{ t('uiRetryLoadingCommit2c6316') }}</button></div>
    <div v-else-if="!history.detail" class="module-state"><GitCommitHorizontal :size="22" />{{ t('uiSelectACommitToViewDetails426466') }}</div>
    <template v-else>
      <section class="commit-metadata">
        <div class="detail-title"><GitCommitHorizontal :size="18" /><div><span>{{ t('commit') }} {{ history.detail.shortHash }}</span><h1>{{ history.detail.message.split("\n")[0] }}</h1></div></div>
        <pre v-if="history.detail.message.includes('\n')">{{ history.detail.message }}</pre>
        <dl>
          <div><dt>{{ t('uiAuthor698bea') }}</dt><dd>{{ history.detail.authorName }} &lt;{{ history.detail.authorEmail }}&gt;</dd></div>
          <div><dt>{{ t('uiCommitter293371') }}</dt><dd>{{ history.detail.committerName }} &lt;{{ history.detail.committerEmail }}&gt;</dd></div>
          <div v-if="history.detail.parentHashes.length"><dt>{{ t('uiParents5ffcd8') }}</dt><dd class="parent-list"><button v-for="parent in history.detail.parentHashes" :key="parent" :aria-label="t('msgViewParentCommit0466e1', { p0: parent.slice(0, 7) })" @click="selectParent(parent)"><code>{{ parent.slice(0, 7) }}</code></button></dd></div>
        </dl>
        <HistoryActionDialogs :detail="history.detail" />
        <div class="review-action"><button :aria-label="t('uiAIReviewThisCommitf8d013')" :disabled="!canReview" @click="reviewCommit"><ScanSearch :size="15" />{{ t('uiAIReviewThisCommitf8d013') }}</button><small>{{ history.detail.parentHashes.length ? t('uiCompareWithFirstParentfaa8eb') : t('uiInitialCommitCompareWithEmptyTreea60d53') }} {{ t('uiUsesCurrentRepositorySKILL9e241e') }}</small><small v-if="!skill.ready.value">{{ ai.skillLoading ? t('uiReadingReviewSkilla2b209') : ai.reviewSkill?.error?.message ?? t('uiConfigureTheRepositoryReviewSkillInAISettings89d26c') }}</small></div>
      </section>
      <HistoryDiffFiles :key="`${history.loadedRootPath}:${history.generation}:${history.detail.hash}`" />
    </template>
  </div>
</template>

<style scoped>
.history-detail { display: grid; align-content: start; }
.review-action { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-top: 12px; }.review-action button { display: inline-flex; align-items: center; gap: 5px; padding: 7px 10px; border-radius: var(--radius-md); color: var(--primary); background: var(--primary-soft); }.review-action small { color: var(--text-muted); }
.commit-metadata { padding: 20px 24px; border-bottom: 1px solid var(--border); }
.detail-title { display: flex; gap: 10px; }
.detail-title span { color: var(--text-muted); font-size: 11px; }
h1 { margin: 3px 0 0; font-size: 17px; letter-spacing: 0; }
.commit-metadata > pre { margin: 14px 0 0; padding: 12px; border-radius: var(--radius-md); background: var(--surface-muted); font-family: inherit; white-space: pre-wrap; }
dl { margin: 14px 0 0; }
dl div { display: grid; grid-template-columns: 74px minmax(0, 1fr); gap: 10px; padding: 5px 0; }
dt { color: var(--text-muted); }
dd { min-width: 0; margin: 0; overflow-wrap: anywhere; }
.parent-list { display: flex; gap: 5px; }
.parent-list button { padding: 2px 5px; border-radius: var(--radius-sm); background: var(--primary-soft); color: var(--primary); }
</style>
