<script setup lang="ts">
import { t } from '@/lib/i18n';
import { Archive, GitBranch, LoaderCircle, RefreshCw } from "@lucide/vue";
import { computed } from "vue";
import StashCreateBar from "@/features/stashes/StashCreateBar.vue";
import { useStashesStore } from "@/stores/stashes";
import type { StashEntry } from "@/lib/backend/types";
const stashes = useStashesStore();
const outcomeText = computed(() => {
  switch (stashes.outcome) {
    case "created": return t('uiStashCreatedc788c0');
    case "noChanges": return t('uiNoChangesToStash4ecb3d');
    case "applied": return t('uiStashAppliedEntryKept8873fd');
    case "removed": return t('uiStashAppliedAndRemoved60ef44');
    case "retained": return t('uiStashEntryKept8d188f');
    default: return "";
  }
});
function refresh(): void { void stashes.refresh().catch(() => undefined); }
function select(entry: StashEntry): void { void stashes.selectEntry(entry).catch(() => undefined); }
function date(timestamp: string): string {
  const value = new Date(timestamp);
  return Number.isNaN(value.getTime()) ? timestamp : value.toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
}
</script>

<template>
  <div class="stash-list">
    <header><span><Archive :size="14" />{{ t('stashes') }} <small>{{ stashes.snapshot?.entries.length ?? 0 }}</small></span><button class="icon-button" :title="t('uiRefreshStashes7c8025')" :aria-label="t('uiRefreshStashes7c8025')" :disabled="stashes.loading || stashes.submitting" @click="refresh"><RefreshCw :size="14" /></button></header>
    <StashCreateBar />
    <div v-if="stashes.error" class="stash-error" role="alert">
      <span>{{ stashes.error.message }}</span>
      <details v-if="stashes.error.diagnostics"><summary>{{ t('uiDiagnostics0b673e') }}</summary><pre>{{ stashes.error.diagnostics }}</pre></details>
      <button :aria-label="t('uiReloadStash5665ff')" :disabled="stashes.submitting" @click="refresh"><RefreshCw :size="13" />{{ t('uiRefreshListbd82e4') }}</button>
    </div>
    <p v-if="outcomeText" class="outcome" role="status">{{ outcomeText }}</p>
    <div v-if="stashes.loading" class="module-state"><LoaderCircle :size="18" class="spin" />{{ t('uiLoadingStashesf1e7b7') }}</div>
    <div v-else-if="!stashes.error && !stashes.snapshot?.entries.length" class="module-state"><Archive :size="20" />{{ t('uiNoStashes82670d') }}</div>
    <div v-else class="entries" :aria-label="t('uiStashList7c4db7')">
      <button v-for="entry in stashes.snapshot?.entries" :key="entry.selector + entry.objectId" class="stash-row" :class="{ selected: stashes.selectedEntry?.objectId === entry.objectId && stashes.selectedEntry?.selector === entry.selector }" :aria-label="(t('uiViewStasha793ce') + ' ') + entry.selector" :aria-pressed="stashes.selectedEntry?.selector === entry.selector" :disabled="stashes.submitting" @click="select(entry)">
        <span class="row-meta"><code>{{ entry.selector }}</code><time :datetime="entry.timestamp" :title="entry.timestamp">{{ date(entry.timestamp) }}</time></span>
        <strong class="stash-description" :title="entry.description">{{ entry.description }}</strong>
        <span class="branch"><GitBranch :size="12" /><span :title="entry.branch ?? t('uiUnknownBranch433d75')">{{ entry.branch ?? t('uiUnknownBranch433d75') }}</span></span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.stash-list { height: 100%; min-height: 0; overflow: auto; }
header { position: sticky; z-index: 1; top: 0; display: flex; height: 44px; align-items: center; justify-content: space-between; padding: 0 14px; border-bottom: 1px solid var(--border); background: var(--surface-panel); }
header > span { display: flex; align-items: center; gap: 7px; font-weight: 600; }
small { color: var(--text-muted); font-size: 11px; }
.icon-button { display: grid; width: 28px; height: 28px; place-items: center; border-radius: var(--radius-md); background: transparent; }
.icon-button:hover { background: var(--surface-muted); }
.entries { padding: 6px; }
.stash-row { display: grid; width: 100%; min-width: 0; height: 86px; align-content: center; gap: 7px; padding: 9px; border-radius: var(--radius-md); background: transparent; text-align: left; }
.stash-row:hover, .stash-row.selected { background: var(--surface-muted); }
.stash-row.selected { box-shadow: inset 2px 0 var(--primary); }
.row-meta { display: flex; min-width: 0; justify-content: space-between; gap: 6px; color: var(--text-muted); font-size: 10px; }
.row-meta code { color: var(--primary); }
.row-meta time { flex-shrink: 0; }
.stash-description, .branch span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.stash-description { font-size: 12px; }
.branch { display: flex; min-width: 0; align-items: center; gap: 5px; color: var(--text-muted); font-size: 10px; }
.branch svg { flex-shrink: 0; }
.module-state { flex-direction: column; }
.stash-error { display: grid; gap: 8px; margin: 10px; padding: 9px; background: var(--danger-soft); color: var(--danger); overflow-wrap: anywhere; font-size: 11px; }
.stash-error pre { max-height: 160px; overflow: auto; white-space: pre-wrap; }
.stash-error button { display: flex; align-items: center; justify-content: center; gap: 5px; min-height: 28px; background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-md); }
.outcome { margin: 10px 14px; color: var(--text-muted); font-size: 11px; }
</style>
