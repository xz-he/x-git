<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, ref, watch } from "vue";

import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import BranchInput from "@/components/common/BranchInput.vue";
import AppSelect from "@/components/common/AppSelect.vue";
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
    pushBranch.value = localBranch.value;
    establishUpstream.value = !currentLocal?.upstream;
    forceWithLease.value = false;
  },
  { immediate: true },
);

watch(localBranch, (name) => {
  if (remotes.requestedAction !== "push") return;
  pushBranch.value = name;
  establishUpstream.value = !localBranches.value.find(branch => branch.name === name)?.upstream;
  forceWithLease.value = false;
});

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
    :title="t('uiPullRemoteBranchb188c0')"
    :description="selectedRemote ? t('msgPullIntoLocalBranchThenStayOnTheTargetBranchGitRep1b89e3', { p0: selectedRemote.name, p1: pullBranch || t('uiRemoteBranch9072f8'), p2: pullLocalBranch || t('uiTargetBranch55b297') }) : t('uiNoRemoteRepositoryAvailable33fe14')"
    :confirm-label="t('uiConfirmPull4e5514')"
    :confirm-disabled="pullDisabled"
    :busy="remotes.running"
    @cancel="remotes.dismissAction()"
    @confirm="confirmPull"
  >
    <label class="field">
      {{ t('uiRemoteRepository36ecf0') }}
      <AppSelect v-model="remotes.selectedRemoteName" :aria-label="t('uiPullRemote899926')" :disabled="remotes.running" :options="(remotes.snapshot?.remotes ?? []).map(remote => ({ value: remote.name, label: remote.name }))" />
    </label>
    <label class="field">{{ t('uiSourceRemoteBranche6b299') }}
      <BranchInput v-model="pullBranch" :label="t('uiRemoteBranch9072f8')" :options="selectedRemote?.branches.map(branch => branch.name) ?? []" :disabled="remotes.running" />
    </label>
    <label class="field">{{ t('uiTargetLocalBranchcaec31') }}
      <BranchInput v-model="pullLocalBranch" :label="t('uiPullTargetLocalBranchc944dc')" :options="localBranches.map(branch => branch.name)" :disabled="remotes.running" />
    </label>
    <p class="target-hint">{{ t('uiTypeForSuggestionsTheLocalTargetBranchMustAlreadyExistCommitfa0537') }}</p>
    <p v-if="remotes.error" class="lease-warning" role="alert">{{ remotes.error.message }}</p>
  </ConfirmDialog>

  <ConfirmDialog
    v-else-if="remotes.requestedAction === 'push'"
    :title="t('uiPushLocalBranch9fd0a6')"
    :description="selectedRemote ? (t('uiPushTheSelectedLocalBranchTo870749') + ' ') + selectedRemote.name + '。' : t('uiNoRemoteRepositoryAvailable33fe14')"
    :confirm-label="t('uiConfirmPush13123f')"
    :confirm-disabled="pushDisabled"
    :busy="remotes.running"
    :danger="forceWithLease"
    @cancel="remotes.dismissAction()"
    @confirm="confirmPush"
  >
    <div class="target-grid">
      <label class="field">
        {{ t('uiLocalBranch9fdfe9') }}
        <AppSelect v-model="localBranch" :aria-label="t('uiLocalBranch9fdfe9')" :disabled="remotes.running" :options="localBranches.map(branch => ({ value: branch.name, label: branch.name }))" />
      </label>
      <label class="field">
        {{ t('uiRemoteBranch9072f8') }}
        <BranchInput
          v-model="pushBranch"
          :label="t('uiPushRemoteBranch1c1302')"
          :options="selectedRemote?.branches.map(branch => branch.name) ?? []"
          :disabled="remotes.running"
        />
      </label>
    </div>
    <label class="check-field">
      <input v-model="establishUpstream" type="checkbox" />
      {{ t('uiSetUpstreamTracking78264b') }}
    </label>
    <label class="check-field force-choice">
      <input
        v-model="forceWithLease"
        type="checkbox"
        :aria-label="t('uiUseForceWithLeasea0315d')"
      />
      {{ t('uiUseForceWithLeasea0315d') }}
    </label>
    <p v-if="forceWithLease" class="lease-warning">
      {{ t('uiOverwriteTheRemoteBranchOnlyIfe023f7') }} {{ selectedRemote?.name }}/{{ pushBranch }} {{ t('uistillPointsTo668a22') }}
      <code>{{ leaseBranch?.objectId ?? t('uiUnknownObjectda5e88') }}</code> {{ t('uia2b2b4') }}
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
