<script setup lang="ts">
import { formatDisplayPath } from "@/lib/formatPath";
import { computed, ref } from "vue";
import { GitBranchPlus, GitMerge, GitPullRequest, RefreshCcw, Trash2 } from "@lucide/vue";

import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import BranchInput from "@/components/common/BranchInput.vue";
import type { BranchSummary } from "@/lib/backend/types";
import { useRefsStore } from "@/stores/refs";
import { useRepositoryStore } from "@/stores/repository";

type Action = "create" | "switch" | "merge" | "rebase" | "delete";
const props = defineProps<{ branch?: BranchSummary }>();
const refs = useRefsStore();
const repositories = useRepositoryStore();
const active = ref<Action>();
const branchName = ref("");
const switchAfterCreate = ref(true);
const forceDelete = ref(false);
const deleteConfirmation = ref("");
const isMutableTarget = computed(
  () => props.branch?.kind === "local" && !props.branch.current,
);
const deleteDisabled = computed(
  () => forceDelete.value && deleteConfirmation.value !== props.branch?.name,
);
const integrationTarget = computed(() => refs.integrationTargets.find(
  (branch) => branch.fullName === refs.integrationRequest?.target || branch.name === refs.integrationRequest?.target,
));
const localNames = computed(() => refs.snapshot?.localBranches.map(branch => branch.name) ?? []);
const invalidIntegration = computed(() => !integrationTarget.value || (refs.integrationRequest?.action === "merge" &&
  (!localNames.value.includes(refs.integrationRequest.destination) || integrationTarget.value.name === refs.integrationRequest.destination)));

function open(action: Action): void {
  if (action === "merge" || action === "rebase") {
    refs.requestIntegration(action, props.branch?.fullName);
    return;
  }
  active.value = action;
  if (action === "create") {
    branchName.value = "";
    switchAfterCreate.value = true;
  }
  if (action === "delete") {
    forceDelete.value = false;
    deleteConfirmation.value = "";
  }
}

async function confirm(): Promise<void> {
  if (!props.branch) return;
  try {
    if (active.value === "create") {
      await refs.createBranch({
        name: branchName.value,
        startPoint: props.branch.fullName,
        switch: switchAfterCreate.value,
      });
    } else if (active.value === "switch") {
      await refs.switchBranch(props.branch.name);
    } else if (active.value === "delete") {
      await refs.deleteBranch({
        name: props.branch.name,
        force: forceDelete.value,
        confirmation: forceDelete.value ? deleteConfirmation.value : null,
      });
    }
    active.value = undefined;
  } catch {
    // Keep dialog values available for correction or retry.
  }
}
</script>

<template>
  <div v-if="props.branch" class="branch-actions" aria-label="分支操作" :inert="repositories.navigationBusy || undefined">
    <button :aria-label="`创建分支，起点 ${props.branch.name}`" title="创建分支" :disabled="refs.submitting" @click="open('create')"><GitBranchPlus :size="15" />创建</button>
    <button v-if="props.branch.kind === 'local'" :aria-label="`合并分支 ${props.branch.name}`" title="合并分支" :disabled="refs.integrationBlocked" @click="open('merge')"><GitMerge :size="15" />合并</button>
    <template v-if="isMutableTarget">
      <button :aria-label="`切换到分支 ${props.branch.name}`" title="切换分支" :disabled="refs.submitting" @click="open('switch')"><RefreshCcw :size="15" />切换</button>
      <button :aria-label="`变基到分支 ${props.branch.name}`" title="变基" :disabled="refs.integrationBlocked" @click="open('rebase')"><GitPullRequest :size="15" />变基</button>
      <button class="danger-action" :aria-label="`删除分支 ${props.branch.name}`" title="删除分支" :disabled="refs.submitting" @click="open('delete')"><Trash2 :size="15" />删除</button>
    </template>
  </div>

  <ConfirmDialog v-if="props.branch && active === 'create'" title="创建本地分支" description="分支将从当前选择的引用开始。" confirm-label="创建分支" :confirm-disabled="!branchName.trim()" :busy="refs.submitting" @cancel="active = undefined" @confirm="confirm">
    <label class="field">分支名称<input v-model="branchName" aria-label="新分支名称" autocomplete="off" /></label>
    <label class="check-field"><input v-model="switchAfterCreate" type="checkbox" />创建后切换</label>
  </ConfirmDialog>
  <ConfirmDialog v-else-if="props.branch && active === 'switch'" title="切换分支" :description="`切换到 ${props.branch.name}。安全的未提交更改会由 Git 保留。`" confirm-label="切换分支" :busy="refs.submitting" @cancel="active = undefined" @confirm="confirm" />
  <ConfirmDialog v-else-if="props.branch && active === 'delete'" title="删除本地分支" :description="`删除 ${props.branch.name}。未合并分支需要明确强制确认。`" confirm-label="删除分支" :confirm-disabled="deleteDisabled" :busy="refs.submitting" :danger="true" @cancel="active = undefined" @confirm="confirm">
    <label class="check-field"><input v-model="forceDelete" type="checkbox" aria-label="强制删除" />强制删除未合并分支</label>
    <label v-if="forceDelete" class="field">输入完整分支名称确认<input v-model="deleteConfirmation" aria-label="输入分支名称确认" autocomplete="off" /></label>
  </ConfirmDialog>
  <ConfirmDialog v-if="refs.integrationRequest" :title="refs.integrationRequest.action === 'merge' ? '合并分支' : '变基当前分支'" :description="refs.integrationRequest.action === 'merge' ? `将 ${integrationTarget?.name ?? '所选源分支'} 合并到 ${refs.integrationRequest.destination || '目标分支'}，执行后停留在目标分支。` : `将 ${refs.integrationRequest.source} 变基到 ${integrationTarget?.name ?? '所选目标分支'}。`" :confirm-label="refs.integrationRequest.action === 'merge' ? '合并分支' : '开始变基'" :confirm-disabled="refs.integrationBlocked || refs.loading || invalidIntegration" :busy="refs.submitting" @cancel="refs.cancelIntegration" @confirm="refs.confirmIntegration">
    <p class="repository-context">仓库：{{ formatDisplayPath(refs.integrationRequest.rootPath) }}<br />当前分支：{{ refs.integrationRequest.source }}</p>
    <div v-if="refs.loading" role="status">正在读取分支...</div>
    <template v-else-if="refs.snapshot && refs.integrationRequest.action === 'merge'">
      <label class="field">源本地分支
        <BranchInput :model-value="integrationTarget?.name ?? refs.integrationRequest.target" label="合并源分支" :options="localNames" :disabled="refs.submitting" @update:model-value="refs.integrationRequest.target = $event" />
      </label>
      <label class="field">目标本地分支
        <BranchInput v-model="refs.integrationRequest.destination" label="合并目标分支" :options="localNames" :disabled="refs.submitting" />
      </label>
      <p v-if="integrationTarget?.name === refs.integrationRequest.destination" class="inline-error">源分支和目标分支不能相同。</p>
      <p class="repository-context">支持输入联想；跨分支合并前请先提交或贮藏未提交修改。</p>
    </template>
    <label v-else-if="refs.snapshot" class="field">目标分支
      <select v-model="refs.integrationRequest.target" aria-label="目标分支" :disabled="refs.submitting">
        <option value="">选择目标分支</option>
        <option v-for="target in refs.integrationTargets" :key="target.fullName" :value="target.fullName">{{ target.name }}</option>
      </select>
    </label>
    <p v-if="!refs.loading && refs.snapshot && (refs.integrationRequest.action === 'merge' ? localNames.length < 2 : !refs.integrationTargets.length)">没有可用的目标分支</p>
    <p v-if="refs.integrationBlocked && !refs.submitting" role="status">请先完成当前 Git 操作或解决冲突。</p>
    <div v-if="refs.error" class="inline-error" role="alert">{{ refs.error.message }}
      <button v-if="!refs.snapshot" class="retry-button" aria-label="重试读取分支" :disabled="refs.loading || refs.integrationBlocked" @click="refs.loadIntegrationTargets"><RefreshCcw :size="14" />重试</button>
    </div>
  </ConfirmDialog>
</template>

<style scoped>
:deep(.dialog-content) { overflow-wrap: anywhere; }
.branch-actions { display: flex; flex-wrap: wrap; gap: 7px; margin-top: 16px; }
.branch-actions button { display: inline-flex; min-height: 32px; align-items: center; gap: 6px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }
.branch-actions .danger-action { color: var(--danger); }
.field { display: grid; gap: 6px; margin-top: 12px; color: var(--text-muted); font-size: 11px; }
.field input, .field select { width: 100%; min-width: 0; height: 34px; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text); }
.repository-context { overflow-wrap: anywhere; color: var(--text-muted); line-height: 1.6; }
.inline-error { margin-top: 12px; color: var(--danger); overflow-wrap: anywhere; }
.retry-button { display: inline-flex; align-items: center; gap: 6px; margin-left: 8px; padding: 6px 8px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); color: var(--text); }
.check-field { display: flex; align-items: center; gap: 7px; margin-top: 12px; }
</style>
