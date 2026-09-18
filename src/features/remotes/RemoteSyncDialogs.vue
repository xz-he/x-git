<script setup lang="ts">
import { computed, ref, watch } from "vue";

import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import BranchInput from "@/components/common/BranchInput.vue";
import type { PushRequest } from "@/lib/backend/types";
import { useRefsStore } from "@/stores/refs";
import { useRemotesStore } from "@/stores/remotes";
import { useRepositoryStore } from "@/stores/repository";

const remotes = useRemotesStore();
const refs = useRefsStore();
const repositories = useRepositoryStore();
const pullBranch = ref("");
const pullLocalBranch = ref("");
const localBranch = ref("");
const pushBranch = ref("");
const establishUpstream = ref(false);
const forceWithLease = ref(false);
const selectedRemote = computed(
  () =>
    remotes.snapshot?.remotes.find(
      (remote) => remote.name === remotes.selectedRemoteName,
    ),
);
const localBranches = computed(() => refs.snapshot?.localBranches ?? []);
const leaseBranch = computed(() =>
  selectedRemote.value?.branches.find(
    (branch) => branch.name === pushBranch.value,
  ),
);
const pullDisabled = computed(
  () =>
    remotes.running || repositories.navigationBusy || refs.loading || !selectedRemote.value ||
    !pullBranch.value.trim() ||
    !localBranches.value.some(branch => branch.name === pullLocalBranch.value),
);
const pushDisabled = computed(
  () =>
    remotes.running ||
    !localBranches.value.some((branch) => branch.name === localBranch.value) ||
    !pushBranch.value.trim() ||
    (forceWithLease.value && !leaseBranch.value),
);

watch(
  [() => remotes.requestedAction, () => remotes.selectedRemoteName],
  ([action]) => {
    if (!action) {
      return;
    }
    const currentLocal =
      localBranches.value.find((branch) => branch.current) ??
      localBranches.value[0];
    pullBranch.value = selectedRemote.value?.branches.find(branch => branch.trackingLocal === currentLocal?.name)?.name
      ?? selectedRemote.value?.branches[0]?.name ?? "";
    pullLocalBranch.value = currentLocal?.name ?? repositories.snapshot?.currentBranch ?? "";
    localBranch.value = currentLocal?.name ?? "";
    pushBranch.value =
      selectedRemote.value?.branches.find(
        (branch) => branch.trackingLocal === currentLocal?.name,
      )?.name ??
      currentLocal?.name ??
      "";
    establishUpstream.value = !currentLocal?.upstream;
    forceWithLease.value = false;
  },
  { immediate: true },
);

async function confirmPull(): Promise<void> {
  const remote = selectedRemote.value;
  if (!remote || pullDisabled.value) {
    return;
  }
  try {
    await remotes.pull(remote.name, pullBranch.value.trim(), pullLocalBranch.value);
    remotes.dismissAction();
  } catch {
    // Preserve the selected target for retry.
  }
}

async function confirmPush(): Promise<void> {
  const remote = selectedRemote.value;
  if (!remote || pushDisabled.value) {
    return;
  }
  const request: PushRequest = {
    remote: remote.name,
    localBranch: localBranch.value,
    remoteBranch: pushBranch.value.trim(),
    establishUpstream: establishUpstream.value,
    forceWithLease: forceWithLease.value
      ? { expectedRemoteOid: leaseBranch.value!.objectId }
      : null,
  };
  try {
    await remotes.push(request);
    remotes.dismissAction();
  } catch {
    // Preserve all choices and diagnostics for correction or retry.
  }
}
</script>

<template>
  <ConfirmDialog
    v-if="remotes.requestedAction === 'pull'"
    title="拉取远程分支"
    :description="selectedRemote ? `将 ${selectedRemote.name}/${pullBranch || '远程分支'} 拉取到本地 ${pullLocalBranch || '目标分支'}。执行后停留在目标分支，Git 的仓库配置决定合并或变基策略。` : '当前没有可用的远程仓库。'"
    confirm-label="确认拉取"
    :confirm-disabled="pullDisabled"
    :busy="remotes.running"
    @cancel="remotes.dismissAction()"
    @confirm="confirmPull"
  >
    <label class="field">
      远程仓库
      <select v-model="remotes.selectedRemoteName" aria-label="拉取远程仓库" :disabled="remotes.running">
        <option v-for="remote in remotes.snapshot?.remotes ?? []" :key="remote.name" :value="remote.name">{{ remote.name }}</option>
      </select>
    </label>
    <label class="field">远程源分支
      <BranchInput v-model="pullBranch" label="远程分支" :options="selectedRemote?.branches.map(branch => branch.name) ?? []" :disabled="remotes.running" />
    </label>
    <label class="field">本地目标分支
      <BranchInput v-model="pullLocalBranch" label="拉取目标本地分支" :options="localBranches.map(branch => branch.name)" :disabled="remotes.running" />
    </label>
    <p class="target-hint">支持输入联想；本地目标分支必须已存在。跨分支拉取前请先提交或贮藏未提交修改。</p>
    <p v-if="remotes.error" class="lease-warning" role="alert">{{ remotes.error.message }}</p>
  </ConfirmDialog>

  <ConfirmDialog
    v-else-if="remotes.requestedAction === 'push'"
    title="推送本地分支"
    :description="selectedRemote ? '将指定本地分支推送到 ' + selectedRemote.name + '。' : '当前没有可用的远程仓库。'"
    confirm-label="确认推送"
    :confirm-disabled="pushDisabled"
    :busy="remotes.running"
    :danger="forceWithLease"
    @cancel="remotes.dismissAction()"
    @confirm="confirmPush"
  >
    <div class="target-grid">
      <label class="field">
        本地分支
        <select v-model="localBranch" aria-label="本地分支">
          <option
            v-for="branch in localBranches"
            :key="branch.fullName"
            :value="branch.name"
          >
            {{ branch.name }}
          </option>
        </select>
      </label>
      <label class="field">
        远程分支
        <input
          v-model="pushBranch"
          aria-label="推送远程分支"
          autocomplete="off"
        />
      </label>
    </div>
    <label class="check-field">
      <input v-model="establishUpstream" type="checkbox" />
      建立上游跟踪
    </label>
    <label class="check-field force-choice">
      <input
        v-model="forceWithLease"
        type="checkbox"
        aria-label="使用 Force With Lease"
      />
      使用 Force With Lease
    </label>
    <p v-if="forceWithLease" class="lease-warning">
      仅当 {{ selectedRemote?.name }}/{{ pushBranch }} 仍指向
      <code>{{ leaseBranch?.objectId ?? "未知对象" }}</code> 时覆盖远程分支。
    </p>
  </ConfirmDialog>
</template>

<style scoped>
.target-hint { color: var(--text-muted); font-size: 11px; line-height: 1.6; margin-top: 12px; }
.target-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 10px; }
.field { display: grid; gap: 6px; margin-top: 12px; color: var(--text-muted); font-size: 11px; }
.field input, .field select { width: 100%; height: 34px; min-width: 0; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text); }
.check-field { display: flex; align-items: center; gap: 7px; margin-top: 12px; }
.force-choice { color: var(--danger); }
.lease-warning { margin: 10px 0 0; padding: 9px; border-left: 3px solid var(--danger); background: var(--danger-soft); color: var(--text); font-size: 11px; line-height: 1.5; }
.lease-warning code { overflow-wrap: anywhere; color: var(--danger); }
</style>
