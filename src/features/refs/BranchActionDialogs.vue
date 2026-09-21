<script setup lang="ts">
import { t } from '@/lib/i18n';
import { formatDisplayPath } from "@/lib/formatPath";
import { computed, ref } from "vue";
import { GitBranchPlus, GitMerge, GitPullRequest, RefreshCcw, Trash2 } from "@lucide/vue";

import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import BranchInput from "@/components/common/BranchInput.vue";
import AppSelect from "@/components/common/AppSelect.vue";
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
  <div v-if="props.branch" class="branch-actions" :aria-label="t('uiBranchActions44800b')" :inert="repositories.navigationBusy || undefined">
    <button :aria-label="t('msgCreateBranchStartingAt0ea8d8', { p0: props.branch.name })" :title="t('uiCreateBranchcd7ca0')" :disabled="refs.submitting" @click="open('create')"><GitBranchPlus :size="15" />{{ t('uiCreatefcbd09') }}</button>
    <button v-if="props.branch.kind === 'local'" :aria-label="t('msgMergeBranchb83ee2', { p0: props.branch.name })" :title="t('uiMergeBranches6821b6')" :disabled="refs.integrationBlocked" @click="open('merge')"><GitMerge :size="15" />{{ t('merge') }}</button>
    <template v-if="isMutableTarget">
      <button :aria-label="t('msgSwitchToBranch0e0bc5', { p0: props.branch.name })" :title="t('uiSwitchBranchcfeb33')" :disabled="refs.submitting" @click="open('switch')"><RefreshCcw :size="15" />{{ t('uiSwitch2f116b') }}</button>
      <button :aria-label="t('msgRebaseOntoBranch49d3cd', { p0: props.branch.name })" :title="t('rebase')" :disabled="refs.integrationBlocked" @click="open('rebase')"><GitPullRequest :size="15" />{{ t('rebase') }}</button>
      <button class="danger-action" :aria-label="t('msgDeleteBranch01dd06', { p0: props.branch.name })" :title="t('uiDeleteBranch6203f5')" :disabled="refs.submitting" @click="open('delete')"><Trash2 :size="15" />{{ t('uiDelete3755f5') }}</button>
    </template>
  </div>

  <ConfirmDialog v-if="props.branch && active === 'create'" :title="t('uiCreateLocalBranche689c5')" :description="t('uiTheBranchStartsFromTheCurrentlySelectedRef8eb1f8')" :confirm-label="t('uiCreateBranchcd7ca0')" :confirm-disabled="!branchName.trim()" :busy="refs.submitting" @cancel="active = undefined" @confirm="confirm">
    <label class="field">{{ t('uiBranchName01571f') }}<input v-model="branchName" :aria-label="t('uiNewBranchName99f235')" autocomplete="off" /></label>
    <label class="check-field"><input v-model="switchAfterCreate" type="checkbox" />{{ t('uiSwitchAfterCreating77d27c') }}</label>
  </ConfirmDialog>
  <ConfirmDialog v-else-if="props.branch && active === 'switch'" :title="t('uiSwitchBranchcfeb33')" :description="t('msgSwitchToGitWillPreserveUncommittedChangesWhenSafe910b23', { p0: props.branch.name })" :confirm-label="t('uiSwitchBranchcfeb33')" :busy="refs.submitting" @cancel="active = undefined" @confirm="confirm" />
  <ConfirmDialog v-else-if="props.branch && active === 'delete'" :title="t('uiDeleteLocalBrancha76f61')" :description="t('msgDeleteUnmergedBranchesRequireExplicitForceConfirmacd08f3', { p0: props.branch.name })" :confirm-label="t('uiDeleteBranch6203f5')" :confirm-disabled="deleteDisabled" :busy="refs.submitting" :danger="true" @cancel="active = undefined" @confirm="confirm">
    <label class="check-field"><input v-model="forceDelete" type="checkbox" :aria-label="t('uiForceDelete53ddf2')" />{{ t('uiForceDeleteAnUnmergedBranch9d8290') }}</label>
    <label v-if="forceDelete" class="field">{{ t('uiEnterTheFullBranchNameToConfirmbd9d97') }}<input v-model="deleteConfirmation" :aria-label="t('uiEnterBranchNameToConfirm1104f9')" autocomplete="off" /></label>
  </ConfirmDialog>
  <ConfirmDialog v-if="refs.integrationRequest" :title="refs.integrationRequest.action === 'merge' ? t('uiMergeBranches6821b6') : t('uiRebaseCurrentBranchd05aa7')" :description="refs.integrationRequest.action === 'merge' ? t('msgMergeIntoThenStayOnTheTargetBranchae33f2', { p0: integrationTarget?.name ?? t('uiSelectedSourceBranchc12eef'), p1: refs.integrationRequest.destination || t('uiTargetBranch55b297') }) : t('msgRebaseOntob80f63', { p0: refs.integrationRequest.source, p1: integrationTarget?.name ?? t('uiSelectedTargetBranch5b3dd5') })" :confirm-label="refs.integrationRequest.action === 'merge' ? t('uiMergeBranches6821b6') : t('uiStartRebasecd5184')" :confirm-disabled="refs.integrationBlocked || refs.loading || invalidIntegration" :busy="refs.submitting" @cancel="refs.cancelIntegration" @confirm="refs.confirmIntegration">
    <p class="repository-context">{{ t('uiRepository2a9fcc') }}{{ formatDisplayPath(refs.integrationRequest.rootPath) }}<br />{{ t('uiCurrentBranchcb6e0f') }}{{ refs.integrationRequest.source }}</p>
    <div v-if="refs.loading" role="status">{{ t('uiLoadingBranches4e7093') }}</div>
    <template v-else-if="refs.snapshot && refs.integrationRequest.action === 'merge'">
      <label class="field">{{ t('uiSourceLocalBranch0c7a18') }}
        <BranchInput :model-value="integrationTarget?.name ?? refs.integrationRequest.target" :label="t('uiMergeSourceBranchb971ca')" :options="localNames" :disabled="refs.submitting" @update:model-value="refs.integrationRequest.target = $event" />
      </label>
      <label class="field">{{ t('uiTargetLocalBranchb5af52') }}
        <BranchInput v-model="refs.integrationRequest.destination" :label="t('uiMergeTargetBranch9d470a')" :options="localNames" :disabled="refs.submitting" />
      </label>
      <p v-if="integrationTarget?.name === refs.integrationRequest.destination" class="inline-error">{{ t('uiSourceAndTargetBranchesMustBeDifferentae9b4b') }}</p>
      <p class="repository-context">{{ t('uiTypeForSuggestionsCommitOrStashUncommittedChangesBeforeMergi5d4db3') }}</p>
    </template>
    <label v-else-if="refs.snapshot" class="field">{{ t('uiTargetBranch55b297') }}
      <AppSelect v-model="refs.integrationRequest.target" :aria-label="t('uiTargetBranch55b297')" :placeholder="t('uiSelectTargetBranch489806')" :disabled="refs.submitting" :options="refs.integrationTargets.map(target => ({ value: target.fullName, label: target.name }))" />
    </label>
    <p v-if="!refs.loading && refs.snapshot && (refs.integrationRequest.action === 'merge' ? localNames.length < 2 : !refs.integrationTargets.length)">{{ t('uiNoTargetBranchesAvailable8614a0') }}</p>
    <p v-if="refs.integrationBlocked && !refs.submitting" role="status">{{ t('uiFinishTheCurrentGitOperationOrResolveConflictsFirst020dd7') }}</p>
    <div v-if="refs.error" class="inline-error" role="alert">{{ refs.error.message }}
      <button v-if="!refs.snapshot" class="retry-button" :aria-label="t('uiRetryLoadingBranches2b9e4c')" :disabled="refs.loading || refs.integrationBlocked" @click="refs.loadIntegrationTargets"><RefreshCcw :size="14" />{{ t('uiRetrye2d53a') }}</button>
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
