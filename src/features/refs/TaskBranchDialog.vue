<script setup lang="ts">
import { computed, ref, watch } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import type { TaskBranchKind, TaskBranchMode } from "@/lib/backend/types";
import { taskBranchPreview } from "@/lib/taskBranch";
import { useRepositoryStore } from "@/stores/repository";
import { useTaskBranchesStore } from "@/stores/taskBranches";
import { formatDisplayPath } from "@/lib/formatPath";

const emit = defineEmits<{ close: []; created: [name: string, title: string] }>();
const repositories = useRepositoryStore();
const tasks = useTaskBranchesStore();
const initial = { root: repositories.snapshot?.rootPath, generation: repositories.generation, branch: repositories.snapshot?.currentBranch, head: repositories.snapshot?.headShortHash };
const kind = ref<TaskBranchKind>("feature");
const mode = ref<TaskBranchMode>("current");
const ticket = ref("");
const slug = ref("");
const description = ref("");
const remoteNames = computed(() => repositories.snapshot?.remotes.map((item) => item.name) ?? []);
const remote = ref(remoteNames.value.includes("origin") ? "origin" : remoteNames.value.length === 1 ? remoteNames.value[0]! : "");
const copied = ref(false);
const copyError = ref("");
const preview = computed(() => taskBranchPreview(kind.value, ticket.value, slug.value, description.value));
const invalid = computed(() => !!preview.value.error || tasks.blocked || !initial.branch || !initial.head || (mode.value === "remoteMaster" && !remoteNames.value.includes(remote.value)));
watch([() => repositories.snapshot?.rootPath, () => repositories.generation, () => repositories.snapshot?.currentBranch, () => repositories.snapshot?.headShortHash], () => {
  if (!tasks.submitting) emit("close");
});
function close() { if (!tasks.submitting) emit("close"); }
async function copyTitle() {
  copied.value = false; copyError.value = "";
  try { await navigator.clipboard.writeText(preview.value.title); copied.value = true; }
  catch { copyError.value = "复制失败，请选择上方标题手动复制。"; }
}
async function create() {
  if (invalid.value || !initial.branch || !initial.head || initial.root !== repositories.snapshot?.rootPath || initial.generation !== repositories.generation) return;
  const result = await tasks.create({
    kind: kind.value, ticket: preview.value.ticket, slug: preview.value.slug, description: preview.value.description,
    mode: mode.value, ...(mode.value === "remoteMaster" ? { remote: remote.value } : {}),
    sourceBranch: initial.branch, expectedHead: initial.head,
  });
  if (result && !result.error) emit("created", preview.value.name, preview.value.title);
}
</script>

<template>
  <ConfirmDialog class="task-dialog" title="快速建分支" confirm-label="确认创建任务分支" :confirm-disabled="invalid" :busy="tasks.submitting" @cancel="close" @confirm="create">
    <p class="task-context">{{ formatDisplayPath(initial.root ?? '') }} · {{ initial.branch }}</p>
    <fieldset class="task-fields" :disabled="tasks.submitting">
      <label>任务类型<select v-model="kind" aria-label="任务类型"><option value="feature">需求 / 优化（feature）</option><option value="hotfix">Bug 修复（hotfix）</option></select></label>
      <label>完整任务单号<input v-model="ticket" aria-label="完整任务单号" :placeholder="kind === 'feature' ? 'R2026082681825' : 'B2026082681825'" /></label>
      <label>简短英文描述<input v-model="slug" aria-label="英文描述" placeholder="purchase-orders" /></label>
      <label>中文说明<input v-model="description" aria-label="中文说明" placeholder="新增采购订单" /></label>
      <label>创建方式<select v-model="mode" aria-label="创建方式"><option value="current">从当前分支创建并切换</option><option value="remoteMaster">从远端 master 创建，留在开发分支</option></select></label>
      <label v-if="mode === 'remoteMaster'">远端<select v-model="remote" aria-label="任务分支远端"><option value="" disabled>选择远端</option><option v-for="name in remoteNames" :key="name" :value="name">{{ name }}</option></select></label>
    </fieldset>
    <p class="task-hint">{{ mode === 'current' ? '基于当前 HEAD 创建并切换，后续在新分支开发。' : '先获取最新远端 master，再创建任务分支。继续在当前分支开发，在提交面板选择“提交并移植”。' }}</p>
    <div class="task-preview"><small>分支名称</small><code>{{ preview.name }}</code><small>MR 标题</small><span>{{ preview.title }}</span><button type="button" :disabled="!!preview.error" @click="copyTitle">{{ copied ? '已复制' : '复制 MR 标题' }}</button></div>
    <p v-if="preview.error && (ticket || slug || description)" class="task-hint">{{ preview.error }}</p>
    <p v-if="mode === 'remoteMaster' && !remoteNames.length" class="task-error">当前仓库没有配置远端。</p>
    <p v-if="copyError" role="alert" class="task-error">{{ copyError }}</p>
    <p v-if="tasks.error" role="alert" class="task-error">{{ tasks.error.message }}</p>
    <button v-if="tasks.uncertain" :disabled="tasks.loading || tasks.submitting" aria-label="刷新任务状态" @click="tasks.refreshState">刷新任务状态</button>
  </ConfirmDialog>
</template>

<style scoped>
@import "./taskBranches.css";
.task-dialog :deep(.confirm-dialog) { display: block; width: min(560px, calc(100vw - 48px)); max-height: calc(100vh - 48px); overflow-y: auto; }
.task-dialog :deep(footer) { margin-top: 12px; }
</style>
