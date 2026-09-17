<script setup lang="ts">
import { GitBranch, LoaderCircle, Tag, Wifi } from "@lucide/vue";
import { computed, ref } from "vue";
import TaskBranchDialog from "./TaskBranchDialog.vue";
import { useTaskBranchesStore } from "@/stores/taskBranches";
import { useRepositoryStore } from "@/stores/repository";

import type { BranchSummary, TagSummary } from "@/lib/backend/types";
import { useRefsStore } from "@/stores/refs";

const props = defineProps<{ mode: "branches" | "tags" }>();
const refsStore = useRefsStore();
const tasks = useTaskBranchesStore();
const repositories = useRepositoryStore();
const taskDialogOpen = ref(false);
const createdTask = ref("");
function taskCreated(name: string, title: string) { taskDialogOpen.value = false; createdTask.value = `已创建 ${name} · MR 标题：${title}`; }
const localBranches = computed(() => refsStore.snapshot?.localBranches ?? []);
const remoteBranches = computed(
  () => refsStore.snapshot?.remoteBranches ?? [],
);
const tags = computed(() => refsStore.snapshot?.tags ?? []);

function selectBranch(branch: BranchSummary): void {
  refsStore.selectedFullName = branch.fullName;
}

function selectTag(tag: TagSummary): void {
  refsStore.selectedFullName = `refs/tags/${tag.name}`;
}
</script>

<template>
  <div class="refs-list">
    <div v-if="props.mode === 'branches'" class="task-entry">
      <button aria-label="快速建分支" :disabled="tasks.blocked || !repositories.snapshot?.currentBranch || !repositories.snapshot?.headShortHash" @click="taskDialogOpen = true">快速建分支</button>
      <p v-if="createdTask" role="status">{{ createdTask }}</p>
      <p v-if="tasks.error" role="alert">{{ tasks.error.message }}</p>
      <button v-if="tasks.error" aria-label="重试读取任务分支" :disabled="tasks.loading || tasks.submitting" @click="tasks.refreshState">重试读取任务分支</button>
    </div>
    <TaskBranchDialog v-if="taskDialogOpen && props.mode === 'branches'" @close="taskDialogOpen = false" @created="taskCreated" />
    <div v-if="refsStore.loading && !refsStore.snapshot" class="module-state">
      <LoaderCircle :size="18" class="spin" />
      <span>正在读取引用</span>
    </div>
    <div v-else-if="refsStore.error && !refsStore.snapshot" class="module-state error" role="alert">
      {{ refsStore.error.message }}
    </div>
    <template v-else-if="props.mode === 'branches'">
      <section class="ref-group" aria-labelledby="local-branches-heading">
        <header id="local-branches-heading">
          <span><GitBranch :size="14" />本地分支</span>
          <strong>{{ localBranches.length }}</strong>
        </header>
        <button
          v-for="branch in localBranches"
          :key="branch.fullName"
          class="ref-row"
          :class="{ selected: refsStore.selectedFullName === branch.fullName }"
          :aria-label="`查看分支 ${branch.name}`"
          @click="selectBranch(branch)"
        >
          <span class="ref-main">
            <span class="ref-name" data-testid="ref-name" :title="branch.name">{{ branch.name }}</span>
            <small>{{ branch.tip.shortHash }} · {{ branch.tip.subject }}</small>
          </span>
          <span class="ref-actions" data-testid="ref-actions">
            <span v-if="branch.current" class="status-chip">当前</span>
            <span v-if="branch.ahead" class="divergence">↑{{ branch.ahead }}</span>
            <span v-if="branch.behind" class="divergence">↓{{ branch.behind }}</span>
          </span>
        </button>
        <div v-if="localBranches.length === 0" class="group-empty">没有本地分支</div>
      </section>
      <section class="ref-group" aria-labelledby="remote-branches-heading">
        <header id="remote-branches-heading">
          <span><Wifi :size="14" />远程分支</span>
          <strong>{{ remoteBranches.length }}</strong>
        </header>
        <button
          v-for="branch in remoteBranches"
          :key="branch.fullName"
          class="ref-row"
          :class="{ selected: refsStore.selectedFullName === branch.fullName }"
          :aria-label="`查看分支 ${branch.name}`"
          @click="selectBranch(branch)"
        >
          <span class="ref-main">
            <span class="ref-name" data-testid="ref-name" :title="branch.name">{{ branch.name }}</span>
            <small>{{ branch.tip.shortHash }} · {{ branch.tip.subject }}</small>
          </span>
          <span class="ref-actions" data-testid="ref-actions" />
        </button>
        <div v-if="remoteBranches.length === 0" class="group-empty">没有远程分支</div>
      </section>
    </template>
    <section v-else class="ref-group" aria-labelledby="tags-heading">
      <header id="tags-heading">
        <span><Tag :size="14" />标签</span>
        <strong>{{ tags.length }}</strong>
      </header>
      <button
        v-for="tag in tags"
        :key="tag.name"
        class="ref-row"
        :class="{ selected: refsStore.selectedFullName === `refs/tags/${tag.name}` }"
        :aria-label="`查看标签 ${tag.name}`"
        @click="selectTag(tag)"
      >
        <span class="ref-main">
          <span class="ref-name" data-testid="ref-name" :title="tag.name">{{ tag.name }}</span>
          <small>{{ tag.commitSubject }}</small>
        </span>
        <span class="ref-actions" data-testid="ref-actions">
          <span v-if="tag.annotated" class="status-chip">附注</span>
        </span>
      </button>
      <div v-if="tags.length === 0" class="group-empty">没有标签</div>
    </section>
    <div v-if="refsStore.error && refsStore.snapshot" class="inline-error" role="alert">
      {{ refsStore.error.message }}
    </div>
  </div>
</template>

<style scoped>
.refs-list { min-height: 0; overflow: auto; }
.task-entry { padding: 10px; border-bottom: 1px solid var(--border); }
.task-entry button { width: 100%; padding: 8px; border-radius: var(--radius-md); background: var(--primary); color: white; }
.task-entry p { color: var(--text-muted); overflow-wrap: anywhere; font-size: 11px; line-height: 1.5; }
.ref-group { padding: 10px; border-bottom: 1px solid var(--border); }
.ref-group > header { display: flex; align-items: center; justify-content: space-between; height: 28px; padding: 0 6px; color: var(--text-muted); font-size: 11px; }
.ref-group > header span { display: flex; align-items: center; gap: 6px; }
.ref-group > header strong { font-size: 11px; }
.ref-row { display: flex; width: 100%; min-width: 0; min-height: 50px; align-items: center; justify-content: space-between; gap: 8px; padding: 8px; border-radius: var(--radius-md); background: transparent; text-align: left; }
.ref-row:hover, .ref-row.selected { background: var(--surface-muted); }
.ref-row.selected { box-shadow: inset 2px 0 var(--primary); }
.ref-main { display: grid; min-width: 0; gap: 4px; }
.ref-main small { overflow: hidden; color: var(--text-muted); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
.ref-actions { display: flex; flex: 0 0 auto; align-items: center; gap: 5px; }
.status-chip, .divergence { padding: 2px 5px; border-radius: var(--radius-sm); background: var(--primary-soft); color: var(--primary); font-size: 10px; }
.group-empty { padding: 14px 6px; color: var(--text-muted); font-size: 11px; }
.inline-error { margin: 10px; padding: 8px; border-radius: var(--radius-md); background: var(--danger-soft); color: var(--danger); font-size: 11px; }
</style>
