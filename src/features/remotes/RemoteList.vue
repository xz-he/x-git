<script setup lang="ts">
import { Cloud, GitBranch, LoaderCircle, RadioTower, RefreshCw } from "@lucide/vue";
import { computed } from "vue";

import type { RemoteDetail } from "@/lib/backend/types";
import { useRemotesStore } from "@/stores/remotes";
import { useRepositoryStore } from "@/stores/repository";

const remotes = useRemotesStore();
const repositories = useRepositoryStore();
const items = computed(() => remotes.snapshot?.remotes ?? []);

function select(remote: RemoteDetail): void {
  remotes.selectedRemoteName = remote.name;
}

function retry(): void {
  const rootPath = repositories.snapshot?.rootPath;
  if (rootPath) {
    void remotes
      .ensureLoaded(rootPath, repositories.generation)
      .catch(() => undefined);
  }
}
</script>

<template>
  <div class="remote-list">
    <header class="list-heading">
      <span><RadioTower :size="14" />远程仓库</span>
      <strong>{{ items.length }}</strong>
    </header>
    <div v-if="remotes.loading && !remotes.snapshot" class="module-state">
      <LoaderCircle :size="18" class="spin" />
      <span>正在读取远程仓库</span>
    </div>
    <div v-else-if="remotes.error && !remotes.snapshot" class="module-state error" role="alert">
      <span>{{ remotes.error.message }}</span>
      <button aria-label="重试读取远程仓库" @click="retry">
        <RefreshCw :size="14" />重试
      </button>
    </div>
    <div v-else-if="items.length === 0" class="module-state">
      <Cloud :size="20" />
      <span>没有已配置的远程仓库</span>
    </div>
    <section
      v-for="remote in items"
      v-else
      :key="remote.name"
      class="remote-group"
    >
      <button
        class="remote-row"
        :class="{ selected: remotes.selectedRemoteName === remote.name }"
        :aria-label="'查看远程仓库 ' + remote.name"
        @click="select(remote)"
      >
        <RadioTower :size="15" />
        <span class="remote-name" :title="remote.name">{{ remote.name }}</span>
        <strong>{{ remote.branches.length }}</strong>
      </button>
      <button
        v-for="branch in remote.branches"
        :key="branch.fullName"
        class="branch-row"
        :aria-label="'查看远程分支 ' + remote.name + '/' + branch.name"
        @click="select(remote)"
      >
        <GitBranch :size="13" />
        <span class="branch-copy">
          <span class="ref-name" :title="branch.name">{{ branch.name }}</span>
          <small v-if="branch.trackingLocal">跟踪 {{ branch.trackingLocal }}</small>
          <small v-else>未跟踪</small>
        </span>
        <span class="divergence">
          <span v-if="branch.ahead">领先 {{ branch.ahead }}</span>
          <span v-if="branch.behind">落后 {{ branch.behind }}</span>
        </span>
      </button>
    </section>
    <div v-if="remotes.error && remotes.snapshot" class="inline-error" role="alert">
      {{ remotes.error.message }}
    </div>
  </div>
</template>

<style scoped>
.remote-list { min-height: 0; height: 100%; overflow: auto; }
.list-heading { position: sticky; z-index: 1; top: 0; display: flex; height: 44px; align-items: center; justify-content: space-between; padding: 0 14px; border-bottom: 1px solid var(--border); background: var(--surface-panel); }
.list-heading span { display: flex; align-items: center; gap: 7px; font-weight: 600; }
.list-heading strong { min-width: 22px; padding: 2px 6px; border-radius: 10px; background: var(--surface-muted); color: var(--text-muted); text-align: center; font-size: 11px; }
.remote-group { padding: 8px 10px 10px; border-bottom: 1px solid var(--border); }
.remote-row, .branch-row { display: flex; width: 100%; min-width: 0; align-items: center; border-radius: var(--radius-md); background: transparent; text-align: left; }
.remote-row { height: 38px; gap: 8px; padding: 0 8px; }
.remote-row:hover, .remote-row.selected { background: var(--surface-muted); }
.remote-row.selected { box-shadow: inset 2px 0 var(--primary); color: var(--primary); }
.remote-name { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
.remote-row strong { color: var(--text-muted); font-size: 11px; }
.branch-row { min-height: 42px; gap: 8px; padding: 6px 8px 6px 28px; color: var(--text-muted); }
.branch-row:hover { background: var(--surface-muted); color: var(--text); }
.branch-copy { display: grid; min-width: 0; flex: 1; gap: 2px; }
.branch-copy > span { color: var(--text); }
.branch-copy small { font-size: 10px; }
.divergence { display: flex; flex: 0 0 auto; gap: 4px; color: var(--primary); font-size: 10px; }
.module-state { flex-direction: column; }
.module-state button { display: inline-flex; height: 30px; align-items: center; gap: 6px; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }
.inline-error { margin: 10px; padding: 8px; border-radius: var(--radius-md); background: var(--danger-soft); color: var(--danger); font-size: 11px; }
</style>
