<script setup lang="ts">
import { computed } from "vue";
import { GitMerge, RotateCcw, TriangleAlert } from "@lucide/vue";

import type { AbortAction } from "@/lib/backend/types";
import { useOperationStore } from "@/stores/operation";
import { useConflictsStore } from "@/stores/conflicts";
import { useRepositoryStore } from "@/stores/repository";
import { useUiStore } from "@/stores/ui";

const operation = useOperationStore();
const conflicts = useConflictsStore();
const repositories = useRepositoryStore();
const ui = useUiStore();
const count = computed(() => operation.state.conflicts.length);
const title = computed(() => {
  if (operation.state.kind === "revert") return count.value ? `Revert 冲突：${count.value} 个文件` : "Revert 等待继续";
  if (!count.value) return operation.state.kind === "merge" ? "合并等待完成" : operation.state.kind === "rebase" ? "变基等待继续" : "Cherry-pick 等待继续";
  const suffix = `：${count.value} 个文件`;
  if (operation.state.kind === "merge") return `合并冲突${suffix}`;
  if (operation.state.kind === "rebase") return `变基冲突${suffix}`;
  if (operation.state.kind === "cherryPick") return `Cherry-pick 冲突${suffix}`;
  return `未解决冲突${suffix}`;
});
const abortLabel: Record<AbortAction, string> = {
  merge: "中止合并",
  rebase: "中止变基",
  cherryPick: "中止 Cherry-pick",
  revert: "中止 Revert",
};

async function abort(): Promise<void> {
  if (!repositories.snapshot || conflicts.busy) return;
  await conflicts.ensureLoaded(repositories.snapshot.rootPath, repositories.generation);
  await conflicts.refresh();
  conflicts.requestAbort();
}
</script>

<template>
  <aside v-if="operation.isBlocked" class="conflict-banner" role="alert">
    <TriangleAlert :size="17" />
    <div class="conflict-summary">
      <strong>{{ title }}</strong>
      <details v-if="count">
        <summary>查看冲突文件</summary>
        <code v-for="file in operation.state.conflicts" :key="file.path">
          {{ file.status }} {{ file.path }}
        </code>
      </details>
    </div>
    <button class="abort-button" @click="ui.openView('conflicts')">解决冲突</button>
    <button
      v-if="operation.state.abortAction"
      class="abort-button"
      :aria-label="abortLabel[operation.state.abortAction]"
      :disabled="conflicts.busy"
      @click="abort()"
    >
      <GitMerge v-if="operation.state.abortAction === 'merge'" :size="15" />
      <RotateCcw v-else :size="15" />
      {{ abortLabel[operation.state.abortAction] }}
    </button>
  </aside>
</template>

<style scoped>
.conflict-banner { grid-column: 2 / -1; grid-row: 2; display: flex; min-width: 0; align-items: center; gap: 10px; padding: 9px 16px; border-bottom: 1px solid color-mix(in srgb, var(--danger) 28%, var(--border)); background: var(--danger-soft); color: var(--danger); }
.conflict-summary { display: flex; min-width: 0; flex: 1; align-items: center; gap: 12px; }
.conflict-summary strong { white-space: nowrap; }
details { color: var(--text-muted); font-size: 11px; }
details code { display: block; margin-top: 5px; color: var(--text); overflow-wrap: anywhere; }
.abort-button { display: inline-flex; flex: 0 0 auto; align-items: center; gap: 6px; min-height: 32px; padding: 0 10px; border: 1px solid color-mix(in srgb, var(--danger) 35%, var(--border)); border-radius: var(--radius-md); background: var(--surface-panel); color: var(--danger); }
</style>
