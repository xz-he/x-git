<script setup lang="ts">
import { computed, ref, watch } from "vue";

import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import type { PushRequest } from "@/lib/backend/types";
import { useRefsStore } from "@/stores/refs";
import { useRemotesStore } from "@/stores/remotes";

const remotes = useRemotesStore();
const refs = useRefsStore();
const pullBranch = ref("");
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
    remotes.running ||
    !selectedRemote.value?.branches.some(
      (branch) => branch.name === pullBranch.value,
    ),
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
    pullBranch.value = selectedRemote.value?.branches[0]?.name ?? "";
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
    await remotes.pull(remote.name, pullBranch.value);
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
    :description="selectedRemote ? '将所选远程分支拉取到当前分支。Git 的仓库配置决定合并或变基策略。' : '当前没有可用的远程仓库。'"
    confirm-label="确认拉取"
    :confirm-disabled="pullDisabled"
    :busy="remotes.running"
    @cancel="remotes.dismissAction()"
    @confirm="confirmPull"
  >
    <label class="field">
      远程分支
      <select v-model="pullBranch" aria-label="远程分支">
        <option
          v-for="branch in selectedRemote?.branches ?? []"
          :key="branch.fullName"
          :value="branch.name"
        >
          {{ selectedRemote?.name }}/{{ branch.name }}
        </option>
      </select>
    </label>
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
.target-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 10px; }
.field { display: grid; gap: 6px; margin-top: 12px; color: var(--text-muted); font-size: 11px; }
.field input, .field select { width: 100%; height: 34px; min-width: 0; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text); }
.check-field { display: flex; align-items: center; gap: 7px; margin-top: 12px; }
.force-choice { color: var(--danger); }
.lease-warning { margin: 10px 0 0; padding: 9px; border-left: 3px solid var(--danger); background: var(--danger-soft); color: var(--text); font-size: 11px; line-height: 1.5; }
.lease-warning code { overflow-wrap: anywhere; color: var(--danger); }
</style>
