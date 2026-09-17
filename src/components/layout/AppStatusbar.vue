<script setup lang="ts">
import { GitBranch, GitCommitHorizontal } from "@lucide/vue";
import type { RepositorySnapshot } from "@/lib/backend/types";
import { useChangesStore } from "@/stores/changes";

defineProps<{ repository?: RepositorySnapshot }>();
const changesStore = useChangesStore();
</script>
<template>
  <footer class="statusbar">
    <span><GitBranch :size="14" />{{ repository?.currentBranch ?? "未打开仓库" }}</span>
    <span v-if="repository?.upstream">↑ {{ repository.upstream.ahead }} ↓ {{ repository.upstream.behind }}</span>
    <span>已暂存 {{ changesStore.snapshot?.stagedCount ?? 0 }}</span>
    <span>未暂存 {{ changesStore.snapshot?.unstagedCount ?? 0 }}</span>
    <button disabled><GitCommitHorizontal :size="14" />提交</button>
  </footer>
</template>
<style scoped>
.statusbar { grid-column: 2 / 4; grid-row: 4; display: flex; align-items: center; gap: 18px; padding: 0 14px; border-top: 1px solid var(--border); background: var(--surface-panel); color: var(--text-muted); font-size: 12px; }
.statusbar span, button { display: inline-flex; align-items: center; gap: 5px; }
button { margin-left: auto; padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }
</style>
