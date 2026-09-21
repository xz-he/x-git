<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed } from "vue";
import { ArrowRight, GitBranch, Tag } from "@lucide/vue";

import { useHistoryStore } from "@/stores/history";
import { useRefsStore } from "@/stores/refs";
import { useRepositoryStore } from "@/stores/repository";
import { useUiStore } from "@/stores/ui";
import BranchActionDialogs from "@/features/refs/BranchActionDialogs.vue";

const props = defineProps<{ mode: "branches" | "tags" }>();
const refsStore = useRefsStore();
const historyStore = useHistoryStore();
const repositories = useRepositoryStore();
const ui = useUiStore();
const branch = computed(() => {
  const branches = [
    ...(refsStore.snapshot?.localBranches ?? []),
    ...(refsStore.snapshot?.remoteBranches ?? []),
  ];
  return (
    branches.find((item) => item.fullName === refsStore.selectedFullName) ??
    branches[0]
  );
});
const tag = computed(() => {
  const tags = refsStore.snapshot?.tags ?? [];
  return (
    tags.find((item) => `refs/tags/${item.name}` === refsStore.selectedFullName) ??
    tags[0]
  );
});

function openHistory(reference: string): void {
  const rootPath = repositories.snapshot?.rootPath;
  if (!rootPath) return;
  historyStore.resetForRepository(rootPath, repositories.generation);
  void historyStore.setReference(reference).catch(() => undefined);
  ui.openView("history");
}
</script>

<template>
  <div class="detail-body ref-detail">
    <template v-if="props.mode === 'branches' && branch">
      <div class="detail-title"><GitBranch :size="18" /><div><span>{{ branch.kind === "local" ? t('uiLocalBranch9fdfe9') : t('uiRemoteBranch9072f8') }}</span><h1>{{ branch.name }}</h1></div></div>
      <dl>
        <div><dt>{{ t('uiLatestCommitdfcc7b') }}</dt><dd><code>{{ branch.tip.shortHash }}</code> {{ branch.tip.subject }}</dd></div>
        <div><dt>{{ t('uiAuthor698bea') }}</dt><dd>{{ branch.tip.author }}</dd></div>
        <div><dt>{{ t('uiCommitDatee2ecfd') }}</dt><dd>{{ branch.tip.authoredAt }}</dd></div>
        <div v-if="branch.upstream"><dt>{{ t('uiUpstreamed38f4') }}</dt><dd>{{ branch.upstream }}</dd></div>
        <div v-if="branch.ahead !== undefined || branch.behind !== undefined"><dt>{{ t('uiSyncStatus015020') }}</dt><dd>{{ t('uiAheada6d477') }} {{ branch.ahead ?? 0 }} {{ t('uiBehindbe32a3') }} {{ branch.behind ?? 0 }}</dd></div>
      </dl>
      <BranchActionDialogs :branch="branch" />
      <button class="primary-action" :aria-label="t('msgViewInHistorydfc020', { p0: branch.name })" @click="openHistory(branch.fullName)">{{ t('uiViewInHistorya60753') }}<ArrowRight :size="15" /></button>
    </template>
    <template v-else-if="props.mode === 'tags' && tag">
      <div class="detail-title"><Tag :size="18" /><div><span>{{ tag.annotated ? t('uiAnnotatedTagd0bb33') : t('uiLightweightTag9f0a5e') }}</span><h1>{{ tag.name }}</h1></div></div>
      <p v-if="tag.annotation" class="annotation">{{ tag.annotation }}</p>
      <dl>
        <div><dt>{{ t('commit') }}</dt><dd><code>{{ tag.peeledCommitHash.slice(0, 7) }}</code> {{ tag.commitSubject }}</dd></div>
        <div v-if="tag.tagger"><dt>{{ t('uiCreator40f51f') }}</dt><dd>{{ tag.tagger }}</dd></div>
        <div v-if="tag.taggedAt"><dt>{{ t('uiCreatedAt84e380') }}</dt><dd>{{ tag.taggedAt }}</dd></div>
      </dl>
      <button class="primary-action" :aria-label="t('msgViewInHistorydfc020', { p0: tag.name })" @click="openHistory(`refs/tags/${tag.name}`)">{{ t('uiViewInHistorya60753') }}<ArrowRight :size="15" /></button>
    </template>
    <div v-else class="module-state">{{ t('uiSelectA791164') }}{{ props.mode === "tags" ? t('tags') : t('branches') }}</div>
    <BranchActionDialogs v-if="props.mode === 'branches' && !branch" />
  </div>
</template>

<style scoped>
.ref-detail { padding: 24px; }
.detail-title { display: flex; align-items: flex-start; gap: 10px; padding-bottom: 18px; border-bottom: 1px solid var(--border); }
.detail-title span { color: var(--text-muted); font-size: 11px; }
h1 { margin: 3px 0 0; overflow-wrap: anywhere; font-size: 18px; letter-spacing: 0; }
dl { display: grid; margin: 0; }
dl div { display: grid; grid-template-columns: 100px minmax(0, 1fr); gap: 14px; padding: 12px 0; border-bottom: 1px solid var(--border); }
dt { color: var(--text-muted); }
dd { min-width: 0; margin: 0; overflow-wrap: anywhere; }
.annotation { margin: 16px 0 4px; padding: 12px; border-left: 3px solid var(--primary); background: var(--surface-muted); white-space: pre-wrap; }
.primary-action { display: inline-flex; align-items: center; gap: 7px; margin-top: 18px; padding: 8px 12px; border-radius: var(--radius-md); background: var(--primary); color: white; }
</style>
