<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { GitCommitHorizontal, GitPullRequest, RotateCcw, Undo2 } from "@lucide/vue";

import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import AppSelect from "@/components/common/AppSelect.vue";
import type { CommitDetail, ResetMode } from "@/lib/backend/types";
import { useHistoryStore } from "@/stores/history";
import { useRefsStore } from "@/stores/refs";
import { useRepositoryStore } from "@/stores/repository";

type Action = "checkout" | "cherryPick" | "reset" | "revert";
const props = defineProps<{ detail: CommitDetail }>();
const history = useHistoryStore();
const refs = useRefsStore();
const repositories = useRepositoryStore();
const active = ref<Action>();
const targetBranch = ref("");
const returnAfterSuccess = ref(true);
const resetMode = ref<ResetMode>("mixed");
const resetConfirmation = ref("");
const mainline = ref<number | null>(null);
const shortHash = computed(() => props.detail.hash.slice(0, 7));
const resetDisabled = computed(
  () =>
    resetMode.value === "hard" &&
    resetConfirmation.value !== shortHash.value,
);

onMounted(() => {
  const root = repositories.snapshot?.rootPath;
  if (root) void refs.ensureLoaded(root, repositories.generation).catch(() => undefined);
});

function open(action: Action): void {
  active.value = action;
  if (action === "revert") history.error = undefined;
  mainline.value = null;
  if (action === "cherryPick") {
    targetBranch.value = "";
    returnAfterSuccess.value = true;
  } else if (action === "reset") {
    resetMode.value = "mixed";
    resetConfirmation.value = "";
  }
}

async function confirm(): Promise<void> {
  try {
    if (active.value === "checkout") {
      await history.checkout(props.detail.hash);
    } else if (active.value === "cherryPick") {
      await history.cherryPick({
        commit: props.detail.hash,
        targetBranch: targetBranch.value || null,
        returnAfterSuccess: returnAfterSuccess.value,
      });
    } else if (active.value === "revert") {
      await history.revert({ commit: props.detail.hash, mainline: mainline.value });
    } else if (active.value === "reset") {
      await history.reset({
        target: props.detail.hash,
        mode: resetMode.value,
        confirmation:
          resetMode.value === "hard" ? resetConfirmation.value : null,
      });
    }
    active.value = undefined;
  } catch {
    // Keep dialog values available for correction or retry.
  }
}
</script>

<template>
  <div class="history-actions" aria-label="提交操作" :inert="repositories.navigationBusy || undefined">
    <button :aria-label="`Checkout 提交 ${props.detail.shortHash}`" title="分离 HEAD 检出" :disabled="history.submitting" @click="open('checkout')"><GitCommitHorizontal :size="15" />Checkout</button>
    <button :aria-label="`Cherry-pick 提交 ${props.detail.shortHash}`" title="Cherry-pick" :disabled="history.submitting" @click="open('cherryPick')"><GitPullRequest :size="15" />Cherry-pick</button>
    <button :aria-label="`Revert 提交 ${props.detail.shortHash}`" title="生成反向提交，撤销这次提交的修改" :disabled="history.submitting" @click="open('revert')"><Undo2 :size="15" />Revert</button>
    <button :aria-label="`Reset 到提交 ${props.detail.shortHash}`" title="重置" :disabled="history.submitting" @click="open('reset')"><RotateCcw :size="15" />Reset</button>
  </div>

  <ConfirmDialog v-if="active === 'checkout'" title="以分离 HEAD 检出" :description="`工作区将检出提交 ${shortHash}，当前分支不会移动。`" confirm-label="检出提交" :busy="history.submitting" @cancel="active = undefined" @confirm="confirm" />
  <ConfirmDialog v-else-if="active === 'revert'" title="回滚此提交（Revert）" :description="`在当前分支 ${repositories.snapshot?.currentBranch ?? '未检出分支'} 上生成一个反向提交，撤销 ${shortHash} 的修改，保留原有历史。请先提交或贮藏工作区修改；遇到冲突后可继续解决或中止回滚。`" confirm-label="确认回滚提交" :confirm-disabled="props.detail.parentHashes.length > 1 && mainline === null" :busy="history.submitting" @cancel="active = undefined" @confirm="confirm">
    <p class="commit-subject">{{ props.detail.message.split('\n')[0] }}</p>
    <label v-if="props.detail.parentHashes.length > 1" class="field">合并提交的主线父提交<AppSelect v-model="mainline" aria-label="Revert 主线父提交" placeholder="请选择保留哪一侧作为主线" :disabled="history.submitting" :options="props.detail.parentHashes.map((parent, index) => ({ value: index + 1, label: `父提交 ${index + 1}：${parent.slice(0, 12)}` }))" /></label>
    <p v-if="props.detail.parentHashes.length > 1" class="field">撤销该合并相对所选父提交引入的修改。通常父提交 1 是合并前的目标分支。</p>
    <p v-if="history.error" role="alert" class="revert-error">{{ history.error.message }}</p>
  </ConfirmDialog>
  <ConfirmDialog v-else-if="active === 'cherryPick'" title="Cherry-pick 提交" :description="`应用提交 ${shortHash}。`" confirm-label="Cherry-pick" :busy="history.submitting" @cancel="active = undefined" @confirm="confirm">
    <label class="field">目标分支<AppSelect v-model="targetBranch" aria-label="Cherry-pick 目标分支" :disabled="history.submitting" :options="[{ value: '', label: '当前分支' }, ...(refs.snapshot?.localBranches ?? []).map(branch => ({ value: branch.name, label: branch.name }))]" /></label>
    <label v-if="targetBranch" class="check-field"><input v-model="returnAfterSuccess" type="checkbox" />成功后返回当前分支</label>
  </ConfirmDialog>
  <ConfirmDialog v-else-if="active === 'reset'" title="重置当前分支" :description="`将当前分支移动到 ${shortHash}。`" confirm-label="执行重置" :confirm-disabled="resetDisabled" :busy="history.submitting" :danger="resetMode === 'hard'" @cancel="active = undefined" @confirm="confirm">
    <fieldset class="reset-modes">
      <legend>重置模式</legend>
      <label><input v-model="resetMode" type="radio" value="soft" aria-label="软重置" />软重置</label>
      <label><input v-model="resetMode" type="radio" value="mixed" aria-label="混合重置" />混合重置</label>
      <label><input v-model="resetMode" type="radio" value="hard" aria-label="硬重置" />硬重置</label>
    </fieldset>
    <label v-if="resetMode === 'hard'" class="field">输入 {{ shortHash }} 确认<input v-model="resetConfirmation" aria-label="输入短哈希确认" autocomplete="off" /></label>
  </ConfirmDialog>
</template>

<style scoped>
.history-actions { display: flex; flex-wrap: wrap; gap: 7px; margin-top: 14px; }
.commit-subject { overflow-wrap: anywhere; }
.revert-error { color: var(--danger); }
.history-actions button { display: inline-flex; min-height: 32px; align-items: center; gap: 6px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }
.field { display: grid; gap: 6px; margin-top: 12px; color: var(--text-muted); font-size: 11px; }
.field input, .field select { width: 100%; height: 34px; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text); }
.check-field { display: flex; align-items: center; gap: 7px; margin-top: 12px; }
.reset-modes { display: flex; flex-wrap: wrap; gap: 12px; margin: 12px 0 0; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md); }
.reset-modes legend { padding: 0 4px; color: var(--text-muted); font-size: 11px; }
.reset-modes label { display: inline-flex; align-items: center; gap: 5px; }
</style>
