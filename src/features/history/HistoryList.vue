<script setup lang="ts">
import { t } from '@/lib/i18n';
import { GitCommitHorizontal, LoaderCircle, Search } from "@lucide/vue";
import { onBeforeUnmount, ref, watch } from "vue";

import type { CommitSummary } from "@/lib/backend/types";
import { useHistoryStore } from "@/stores/history";
import { useRepositoryStore } from "@/stores/repository";

const history = useHistoryStore();
const repositories = useRepositoryStore();
const sentinel = ref<HTMLElement>();
let observer: IntersectionObserver | undefined;

function selectCommit(commit: CommitSummary): void {
  void history.selectCommit(commit.hash).catch(() => undefined);
}

function observeSentinel(element?: HTMLElement): void {
  observer?.disconnect();
  observer = undefined;
  if (!element || typeof IntersectionObserver === "undefined") return;
  observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) {
      void history.loadNextPage().catch(() => undefined);
    }
  });
  observer.observe(element);
}

watch(sentinel, observeSentinel, { flush: "post" });
onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <div class="history-list">
    <div class="history-toolbar">
      <div class="history-scope" :title="history.query.reference || repositories.snapshot?.currentBranch || 'HEAD'">{{ history.query.reference ? t('uiSelectedRef7b028e') + history.query.reference : t('uiCurrentBranchcb6e0f') + (repositories.snapshot?.currentBranch || t('uiHEADDetached97456c')) }}<button v-if="history.query.reference" @click="history.setReference(null)">{{ t('uiFollowCurrentBranch50e350') }}</button></div>
      <label class="search-field">
        <Search :size="14" />
        <input :aria-label="t('uiSearchCommitHistory1c1dc1')" :value="history.query.search" :placeholder="t('uiSearchCommitsAuthorsOrHashes818a73')" @input="history.setSearch(($event.target as HTMLInputElement).value)" />
      </label>
    </div>
    <div v-if="history.error && history.commits.length" class="inline-error" role="alert">{{ history.error.message }}</div>
    <div v-if="history.loading && history.commits.length === 0" class="module-state"><LoaderCircle :size="18" class="spin" />{{ t('uiLoadingCommitsc684a1') }}</div>
    <div v-else-if="history.error && history.commits.length === 0" class="module-state error" role="alert">{{ history.error.message }}</div>
    <div v-else-if="history.commits.length === 0" class="module-state"><GitCommitHorizontal :size="20" />{{ history.query.search ? t('uiNoMatchingCommits3dcac9') : t('uiThisRepositoryHasNoCommitsYetfcc894') }}</div>
    <div v-else class="history-rows">
      <button
        v-for="commit in history.commits"
        :key="commit.hash"
        class="history-row"
        :class="{ selected: history.selectedHash === commit.hash }"
        :data-commit-hash="commit.hash"
        @click="selectCommit(commit)"
      >
        <span class="commit-copy">
          <strong :title="commit.subject">{{ commit.subject }}</strong>
          <small>{{ commit.authorName }} · {{ commit.authoredAt }}</small>
          <span v-if="commit.references.length" class="reference-labels"><em v-for="reference in commit.references" :key="reference">{{ reference }}</em></span>
        </span>
        <code>{{ commit.shortHash }}</code>
      </button>
      <div v-if="history.nextCursor" ref="sentinel" class="history-sentinel" data-testid="history-sentinel">
        <LoaderCircle v-if="history.loading" :size="16" class="spin" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.history-scope { min-width: 0; overflow-wrap: anywhere; color: var(--text-muted); font-size: 11px; }
.history-scope button { margin-left: 8px; background: transparent; color: var(--primary); }
.history-list { min-height: 0; overflow: auto; }
.history-toolbar { position: sticky; z-index: 2; top: 0; display: grid; gap: 8px; padding: 10px; border-bottom: 1px solid var(--border); background: var(--surface-panel); }
.search-field { display: flex; min-width: 0; flex: 1; height: 32px; align-items: center; gap: 7px; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text-muted); }
.search-field input { min-width: 0; flex: 1; border: 0; outline: 0; background: transparent; }
.history-row { width: 100%; border-radius: 0; border-bottom: 1px solid var(--border); background: transparent; text-align: left; }
.history-row:hover, .history-row.selected { background: var(--surface-muted); }
.history-row.selected { box-shadow: inset 2px 0 var(--primary); }
.commit-copy { display: grid; min-width: 0; gap: 3px; padding: 8px 0; }
.commit-copy strong, .commit-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.commit-copy small { color: var(--text-muted); font-size: 10px; }
.history-row > code { padding-right: 10px; color: var(--text-muted); font-size: 10px; }
.reference-labels { display: flex; min-width: 0; gap: 4px; overflow: hidden; }
.reference-labels em { overflow: hidden; padding: 1px 5px; border-radius: var(--radius-sm); background: var(--primary-soft); color: var(--primary); font-size: 9px; font-style: normal; text-overflow: ellipsis; white-space: nowrap; }
.history-sentinel { display: grid; height: 34px; place-items: center; color: var(--text-muted); }
.inline-error { margin: 8px; padding: 7px; border-radius: var(--radius-md); background: var(--danger-soft); color: var(--danger); font-size: 11px; }
</style>
