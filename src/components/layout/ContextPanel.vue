<script setup lang="ts">
import { formatDisplayPath } from "@/lib/formatPath";
import { watch } from "vue";
import FileTree from "@/features/files/FileTree.vue";
import ConsoleHistory from "@/features/terminal/ConsoleHistory.vue";
import { useFilesStore } from "@/stores/files";
import ConflictList from "@/features/conflicts/ConflictList.vue";
import { useConflictsStore } from "@/stores/conflicts";
import type { RepositorySnapshot } from "@/lib/backend/types";
import ChangesList from "@/features/changes/ChangesList.vue";
import HistoryList from "@/features/history/HistoryList.vue";
import RefsList from "@/features/refs/RefsList.vue";
import RemoteList from "@/features/remotes/RemoteList.vue";
import StashList from "@/features/stashes/StashList.vue";
import { useStashesStore } from "@/stores/stashes";
import { useChangesStore } from "@/stores/changes";
import { useHistoryStore } from "@/stores/history";
import { useRefsStore } from "@/stores/refs";
import { useRemotesStore } from "@/stores/remotes";
import { useRepositoryStore } from "@/stores/repository";
import { useUiStore } from "@/stores/ui";
import { useTerminalStore } from "@/stores/terminal";
import ContextResizeHandle from "./ContextResizeHandle.vue";

const props = withDefaults(defineProps<{ repository?: RepositorySnapshot; width?: number; maxWidth?: number }>(), { width: 320, maxWidth: 1000 });
const changesStore = useChangesStore();
const historyStore = useHistoryStore();
const refsStore = useRefsStore();
const remotesStore = useRemotesStore();
const stashesStore = useStashesStore();
const repositories = useRepositoryStore();
const ui = useUiStore();
const conflicts = useConflictsStore();
const files = useFilesStore();
const terminal = useTerminalStore();

watch(
  [() => ui.activeView, () => props.repository?.rootPath, () => repositories.generation, () => terminal.busy],
  ([view, rootPath, generation, terminalBusy]) => {
    if (!rootPath || (terminalBusy && view === "conflicts")) return;
    if (view === "files") {
      void files.ensureLoaded(rootPath, generation);
    } else if (view === "conflicts") {
      void conflicts.ensureLoaded(rootPath, generation).then(() => conflicts.refresh());
    } else if (view === "branches" || view === "tags") {
      void refsStore.ensureLoaded(rootPath, generation).catch(() => undefined);
    } else if (view === "history") {
      void historyStore.ensureLoaded(rootPath, generation).catch(() => undefined);
    } else if (view === "remotes") {
      void remotesStore.ensureLoaded(rootPath, generation).catch(() => undefined);
    } else if (view === "stashes") {
      void stashesStore.ensureLoaded(rootPath, generation).catch(() => undefined);
    }
  },
  { immediate: true },
);
</script>
<template>
  <section class="context" data-testid="context-panel">
    <ContextResizeHandle :width="props.width" :max-width="props.maxWidth" @resize="ui.contextWidth = $event" />
    <div v-if="ui.activeView === 'changes'" class="heading">
      <span>文件状态</span>
      <span class="count">{{ changesStore.snapshot?.files.length ?? 0 }}</span>
    </div>
    <div v-if="ui.activeView === 'changes' && repository" class="repo"><strong>{{ repository.name }}</strong><span :title="formatDisplayPath(repository.rootPath)">{{ formatDisplayPath(repository.rootPath) }}</span></div>
    <ChangesList v-if="ui.activeView === 'changes'" />
    <RefsList v-else-if="ui.activeView === 'branches' || ui.activeView === 'tags'" :mode="ui.activeView" />
    <HistoryList v-else-if="ui.activeView === 'history'" />
    <RemoteList v-else-if="ui.activeView === 'remotes'" />
    <StashList v-else-if="ui.activeView === 'stashes'" />
    <ConflictList v-else-if="ui.activeView === 'conflicts'" />
    <FileTree v-else-if="ui.activeView === 'files'" />
    <ConsoleHistory v-else-if="ui.activeView === 'terminal'" />
    <div v-else class="module-state">该模块将在后续增量中提供</div>
  </section>
</template>
<style scoped>
.context { position: relative; display: flex; flex-direction: column; grid-column: 2; grid-row: 3; min-width: 0; min-height: 0; border-right: 1px solid var(--border); background: var(--surface-panel); overflow: hidden; }
/* The active module is the final child; constrain it to the remaining panel height. */
.context > :last-child { flex: 1; min-height: 0; }
.heading { display: flex; flex-shrink: 0; align-items: center; justify-content: space-between; height: 44px; padding: 0 14px; border-bottom: 1px solid var(--border); font-weight: 600; }
.count { min-width: 22px; padding: 2px 6px; border-radius: 10px; background: var(--surface-muted); color: var(--text-muted); text-align: center; font-size: 11px; }
.repo { display: grid; flex-shrink: 0; gap: 4px; padding: 14px; border-bottom: 1px solid var(--border); }
.repo span { overflow: hidden; color: var(--text-muted); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
</style>
