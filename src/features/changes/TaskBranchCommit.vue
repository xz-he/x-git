<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { GitBranchPlus, LoaderCircle, Unlink } from "@lucide/vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import { taskPhaseLabel } from "@/lib/taskBranch";
import { useTaskBranchesStore } from "@/stores/taskBranches";
import { useRepositoryStore } from "@/stores/repository";
import { useChangesStore } from "@/stores/changes";
import { useUiStore } from "@/stores/ui";

const tasks = useTaskBranchesStore();
const repositories = useRepositoryStore();
const changes = useChangesStore();
const selectedId = ref("");
const copied = ref(false);
const copyError = ref("");
const candidates = computed(() => tasks.relevantBindings);
const selected = computed(() => candidates.value.find((item) => item.id === selectedId.value) ?? candidates.value[0]);
const canCommit = computed(() => !!selected.value && selected.value.mode === "remoteMaster" && ["ready", "completed"].includes(selected.value.phase) && repositories.snapshot?.currentBranch === selected.value.sourceBranch);
const commitDisabled = computed(() => tasks.blocked || !changes.commitMessage.trim() || !(changes.snapshot?.stagedCount ?? 0));
const returnAfterSuccess = ref<boolean>(true);
const confirmation = ref<{ action: "commit" | "pick"; id: string; source: string; target: string; message: string; head: string; root: string; generation: number; branch: string | null; staged: number }>();
const confirmLabel = computed(() => confirmation.value?.action === "commit" ? "确认提交并移植" : "确认移植此提交");

watch([() => repositories.snapshot?.rootPath, () => repositories.generation, () => repositories.snapshot?.currentBranch, () => repositories.snapshot?.headShortHash], () => {
  if (!tasks.submitting) confirmation.value = undefined;
});
watch(() => selected.value?.id, () => { copied.value = false; copyError.value = ""; });
function open(action: "commit" | "pick") {
  const item = selected.value; const repo = repositories.snapshot;
  if (!item || !repo?.headShortHash || tasks.blocked || (action === "commit" && commitDisabled.value)) return;
  returnAfterSuccess.value = item.returnAfterSuccess;
  confirmation.value = { action, id: item.id, source: item.sourceBranch, target: item.targetBranch, message: changes.commitMessage, head: repo.headShortHash, root: repo.rootPath, generation: repositories.generation, branch: repo.currentBranch, staged: changes.snapshot?.stagedCount ?? 0 };
}
async function confirm() {
  const request = confirmation.value;
  if (!request || tasks.blocked || repositories.snapshot?.rootPath !== request.root || repositories.generation !== request.generation || repositories.snapshot.currentBranch !== request.branch || repositories.snapshot.headShortHash !== request.head) return;
  await tasks.run({ id: request.id, action: request.action, ...(request.action === "commit" ? { message: request.message } : {}), returnAfterSuccess: returnAfterSuccess.value, expectedHead: request.head });
  confirmation.value = undefined;
}
async function act(action: "return" | "reconcile") {
  const item = selected.value; const head = repositories.snapshot?.headShortHash;
  if (!item || !head) return;
  await tasks.run({ id: item.id, action, returnAfterSuccess: item.returnAfterSuccess, expectedHead: head });
}
async function copyTitle() {
  if (!selected.value) return;
  copyError.value = "";
  try { await navigator.clipboard.writeText(`${selected.value.ticket} ${selected.value.description}`); copied.value = true; }
  catch { copyError.value = "复制失败，请手动复制任务标题。"; }
}
async function unlink() {
  const item = selected.value;
  if (!item) return;
  confirmation.value = undefined;
  await tasks.unlink(item.id);
  if (!candidates.value.some((candidate) => candidate.id === selectedId.value)) {
    selectedId.value = selected.value?.id ?? "";
  }
}
</script>

<template>
  <div v-if="selected || tasks.loading || tasks.error" class="task-commit" aria-label="任务分支提交">
    <div v-if="tasks.loading" class="task-hint" role="status"><LoaderCircle :size="13" class="spin" /> 正在读取任务分支</div>
    <template v-if="selected">
      <div class="task-heading"><span><GitBranchPlus :size="14" />任务分支</span><small>{{ taskPhaseLabel[selected.phase] }}</small></div>
      <select v-if="candidates.length > 1" v-model="selectedId" class="task-selector" aria-label="提交目标任务分支" :disabled="tasks.submitting" @change="confirmation = undefined"><option v-if="!selectedId" :value="''">{{ candidates[0]?.targetBranch }}</option><option v-for="item in candidates" :key="item.id" :value="item.id">{{ item.targetBranch }}</option></select>
      <code v-else class="task-target">{{ selected.targetBranch }}</code>
      <div class="task-title">{{ selected.ticket }} {{ selected.description }} <button @click="copyTitle">{{ copied ? '已复制' : '复制 MR 标题' }}</button></div>
      <p v-if="selected.sourceCommit" class="task-hint">开发分支提交：<code :title="selected.sourceCommit">{{ selected.sourceCommit.slice(0, 8) }}</code></p>
      <p v-if="selected.message" class="task-hint" role="status">{{ selected.message }}</p>
      <div class="task-actions">
        <button v-if="canCommit" class="task-primary" aria-label="提交并移植" :disabled="commitDisabled" @click="open('commit')">提交并移植</button>
        <button v-if="selected.phase === 'pendingPick'" class="task-primary" aria-label="移植此提交" :disabled="tasks.blocked" @click="open('pick')">移植此提交</button>
        <button v-if="selected.phase === 'pendingReturn'" aria-label="返回开发分支" :disabled="tasks.blocked" @click="act('return')">返回开发分支</button>
        <button v-if="selected.phase === 'conflict'" :disabled="tasks.submitting" @click="useUiStore().openView('conflicts')">打开冲突工作台</button>
        <button v-if="!['ready', 'completed'].includes(selected.phase)" aria-label="核对任务状态" :disabled="tasks.submitting || tasks.loading || repositories.navigationBusy" @click="act('reconcile')">核对任务状态</button>
        <button aria-label="刷新任务状态" :disabled="tasks.submitting || tasks.loading" @click="tasks.refreshState">刷新任务状态</button>
        <button class="task-unlink" aria-label="解除关联" title="仅移除当前任务关联，保留 Git 分支、提交和未提交修改；未完成的 Git 操作仍需自行处理。" :disabled="tasks.submitting || tasks.loading || repositories.navigationBusy" @click="unlink"><Unlink :size="13" />解除关联</button>
      </div>
      <p v-if="selected.phase === 'conflict'" class="task-hint">解决并继续或中止 Cherry-pick 后，点击“核对任务状态”继续。</p>
    </template>
    <p v-if="tasks.error" class="task-error" role="alert">{{ tasks.error.message }}</p>
    <p v-if="copyError" class="task-error" role="alert">{{ copyError }}</p>
    <button v-if="tasks.error && !selected" aria-label="刷新任务状态" :disabled="tasks.submitting || tasks.loading" @click="tasks.refreshState">刷新任务状态</button>
    <ConfirmDialog v-if="confirmation" class="task-pick-dialog" :title="confirmation.action === 'commit' ? '提交并移植' : '移植已有提交'" :confirm-label="confirmLabel" :confirm-disabled="tasks.blocked" :busy="tasks.submitting" @cancel="!tasks.submitting && (confirmation = undefined)" @confirm="confirm">
      <div class="task-preview"><small>开发分支</small><code>{{ confirmation.source }}</code><small>目标任务分支</small><code>{{ confirmation.target }}</code><template v-if="confirmation.action === 'commit'"><small>提交 {{ confirmation.staged }} 个已暂存文件</small><span class="task-message">{{ confirmation.message }}</span></template><template v-else><small>仅移植已创建的提交</small><code>{{ selected?.sourceCommit }}</code></template></div>
      <fieldset class="task-destination" :disabled="tasks.submitting"><legend>移植成功后</legend><label><input v-model="returnAfterSuccess" type="radio" name="task-destination" :value="true" aria-label="成功后返回开发分支" />返回开发分支继续开发</label><label><input v-model="returnAfterSuccess" type="radio" name="task-destination" :value="false" aria-label="成功后留在新分支" />留在新分支</label></fieldset>
      <p class="task-hint">发生冲突时停在新分支，进入冲突工作台处理。</p>
    </ConfirmDialog>
  </div>
</template>

<style scoped>
@import "../refs/taskBranches.css";
.task-commit { grid-column: 1 / -1; min-width: 0; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); font-size: 12px; }
.task-heading { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 6px; }
.task-heading span { display: inline-flex; gap: 6px; align-items: center; font-weight: 600; }
.task-heading small { color: var(--text-muted); }
.task-target { display: block; overflow-wrap: anywhere; color: var(--primary); }
.task-title { margin-top: 6px; overflow-wrap: anywhere; }
.task-title button { padding-left: 8px; background: transparent; color: var(--primary); }
.task-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 9px; }
.task-actions button, .task-commit > button { padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }
.task-actions .task-primary { background: var(--primary); color: white; border-color: var(--primary); }
.task-unlink { display: inline-flex; align-items: center; gap: 5px; }
.task-pick-dialog :deep(.confirm-dialog) { display: block; width: min(540px, calc(100vw - 48px)); max-height: calc(100vh - 48px); overflow-y: auto; }
.task-pick-dialog .task-preview { margin-top: 12px; }
.task-destination { display: grid; gap: 12px; padding: 12px; margin: 14px 0 0; border: 1px solid var(--border); border-radius: var(--radius-md); }
.task-destination label { display: flex; align-items: center; gap: 8px; }
.task-destination input { accent-color: var(--primary); }
.task-message { white-space: pre-wrap; }
</style>
