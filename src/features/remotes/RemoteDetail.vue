<script setup lang="ts">
import { t } from '@/lib/i18n';
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  CloudDownload,
  LoaderCircle,
  RadioTower,
  Square,
  TriangleAlert,
} from "@lucide/vue";
import { computed, watch } from "vue";

import RemoteSyncDialogs from "@/features/remotes/RemoteSyncDialogs.vue";
import { useRefsStore } from "@/stores/refs";
import { useRemotesStore } from "@/stores/remotes";
import { useRepositoryStore } from "@/stores/repository";

const remotes = useRemotesStore();
const refs = useRefsStore();
const repositories = useRepositoryStore();
const selected = computed(
  () =>
    remotes.snapshot?.remotes.find(
      (remote) => remote.name === remotes.selectedRemoteName,
    ) ?? remotes.snapshot?.remotes[0],
);
const statusLabel = computed(() => {
  if (remotes.error?.code === "gitRefreshFailed") return t('uiSyncSucceededRefreshFailed2d44eb');
  const labels = {
    get idle() { return t('uiWaitingForASyncOperation87fd7f'); },
    get starting() { return t('uiStarting6017dc'); },
    get running() { return t('uiSyncInProgress573137'); },
    get completed() { return t('uiSyncCompleted2870ba'); },
    get conflicted() { return t('uiSyncHasConflictsd75da9'); },
    get cancelled() { return t('uiSyncCancelleddd26f7'); },
    get failed() { return t('uiSyncFaileda8f14c'); },
  };
  return labels[remotes.status];
});

watch(
  () => [repositories.snapshot?.rootPath, repositories.generation] as const,
  ([rootPath, generation]) => {
    if (rootPath && !refs.snapshot) {
      void refs.ensureLoaded(rootPath, generation).catch(() => undefined);
    }
  },
  { immediate: true },
);

function fetchSelected(): void {
  if (selected.value) {
    void remotes.fetch(selected.value.name).catch(() => undefined);
  }
}
</script>

<template>
  <div class="detail-body remote-detail">
    <template v-if="selected">
      <header class="remote-title">
        <RadioTower :size="19" />
        <div>
          <span>{{ t('uiRemoteRepository36ecf0') }}</span>
          <h1>{{ selected.name }}</h1>
        </div>
      </header>

      <dl class="remote-metadata">
        <div>
          <dt>{{ t('uiFetchURLa1cda4') }}</dt>
          <dd><code tabindex="0">{{ selected.fetchUrl }}</code></dd>
        </div>
        <div>
          <dt>{{ t('uiPushURL8ec0e9') }}</dt>
          <dd><code tabindex="0">{{ selected.pushUrl }}</code></dd>
        </div>
        <div>
          <dt>{{ t('uiRemoteBranch9072f8') }}</dt>
          <dd>{{ selected.branches.length }}</dd>
        </div>
      </dl>

      <div class="remote-actions" :aria-label="t('uiRemoteSyncActions080b45')" :inert="repositories.navigationBusy || undefined">
        <button
          :aria-label="(t('fetch') + ' ') + selected.name"
          :disabled="remotes.running"
          @click="fetchSelected"
        >
          <CloudDownload :size="15" />{{ t('fetch') }}
        </button>
        <button
          :aria-label="t('uiOpenPullDialog8f1445')"
          :disabled="remotes.running || selected.branches.length === 0"
          @click="remotes.requestAction('pull')"
        >
          <ArrowDownToLine :size="15" />{{ t('pull') }}
        </button>
        <button
          :aria-label="t('uiOpenPushDialogae84e4')"
          :disabled="remotes.running"
          @click="remotes.requestAction('push')"
        >
          <ArrowUpFromLine :size="15" />{{ t('push') }}
        </button>
      </div>
    </template>
    <div v-else class="module-state">{{ t('uiSelectARemoteRepositoryc010e0') }}</div>

    <section
      class="sync-surface"
      data-testid="git-progress"
      aria-live="polite"
    >
      <div class="sync-status">
        <LoaderCircle v-if="remotes.running" :size="17" class="spin" />
        <TriangleAlert
          v-else-if="remotes.status === 'failed' || remotes.status === 'conflicted'"
          :size="17"
        />
        <span>
          <strong>{{ statusLabel }}</strong>
          <small v-if="remotes.progress">{{ remotes.progress.text }}</small>
          <small v-else-if="remotes.status === 'cancelled'">{{ t('uiRepositoryStateRefreshed69de4c') }}</small>
        </span>
      </div>
      <button
        v-if="remotes.running"
        class="stop-button"
        :aria-label="t('uiStopRemoteSync7629b3')"
        :title="t('uiStopRemoteSync7629b3')"
        :disabled="remotes.cancelRequested"
        @click="void remotes.cancel()"
      >
        <Square :size="13" />
        {{ remotes.cancelRequested ? t('uiStoppingcaa14e') : t('uiStopa17f70') }}
      </button>
    </section>

    <section v-if="remotes.error" class="failure" role="alert">
      <strong>{{ remotes.error.message }}</strong>
      <details v-if="remotes.error.diagnostics">
        <summary>{{ t('uiDiagnostics0b673e') }}</summary>
        <pre>{{ remotes.error.diagnostics }}</pre>
      </details>
    </section>
  </div>
  <RemoteSyncDialogs />
</template>

<style scoped>
.remote-detail { padding: 24px; }
.remote-title { display: flex; align-items: flex-start; gap: 10px; padding-bottom: 18px; border-bottom: 1px solid var(--border); }
.remote-title span { color: var(--text-muted); font-size: 11px; }
h1 { margin: 3px 0 0; overflow-wrap: anywhere; font-size: 18px; letter-spacing: 0; }
.remote-metadata { display: grid; margin: 0; }
.remote-metadata > div { display: grid; grid-template-columns: 100px minmax(0, 1fr); gap: 14px; padding: 12px 0; border-bottom: 1px solid var(--border); }
dt { color: var(--text-muted); }
dd { min-width: 0; margin: 0; overflow-wrap: anywhere; }
code { user-select: text; overflow-wrap: anywhere; color: var(--text); font-family: var(--font-code); font-size: 12px; }
.remote-actions { display: flex; flex-wrap: wrap; gap: 7px; margin-top: 16px; }
.remote-actions button, .stop-button { display: inline-flex; min-height: 32px; align-items: center; gap: 6px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }
.remote-actions button:first-child { border-color: var(--primary-border); color: var(--primary); }
.sync-surface { display: flex; min-height: 58px; align-items: center; justify-content: space-between; gap: 12px; margin-top: 20px; padding: 10px 12px; border-top: 1px solid var(--border); border-bottom: 1px solid var(--border); background: var(--surface-muted); }
.sync-status { display: flex; min-width: 0; align-items: center; gap: 9px; }
.sync-status > span { display: grid; min-width: 0; gap: 3px; }
.sync-status small { overflow: hidden; color: var(--text-muted); text-overflow: ellipsis; white-space: nowrap; }
.stop-button { flex: 0 0 auto; color: var(--danger); }
.failure { margin-top: 14px; padding: 12px; border-left: 3px solid var(--danger); background: var(--danger-soft); color: var(--danger); }
.failure details { margin-top: 8px; color: var(--text); }
.failure summary { cursor: pointer; color: var(--text-muted); font-size: 11px; }
.failure pre { max-height: 150px; margin: 8px 0 0; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 11px; }
</style>
