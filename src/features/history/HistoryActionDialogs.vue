<script setup lang="ts">
import { t } from '@/lib/i18n';
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
  <div class="history-actions" :aria-label="t('uiCommitActions999f4c')" :inert="repositories.navigationBusy || undefined">
    <button :aria-label="t('msgCheckoutCommit0fcd13', { p0: props.detail.shortHash })" :title="t('uiCheckoutWithDetachedHEAD5900ae')" :disabled="history.submitting" @click="open('checkout')"><GitCommitHorizontal :size="15" />Checkout</button>
    <button :aria-label="t('msgCherryPickCommita65640', { p0: props.detail.shortHash })" title="Cherry-pick" :disabled="history.submitting" @click="open('cherryPick')"><GitPullRequest :size="15" />Cherry-pick</button>
    <button :aria-label="t('msgRevertCommit95ca02', { p0: props.detail.shortHash })" :title="t('uiCreateAnInverseCommitToUndoThisCommitSChanges3f9a97')" :disabled="history.submitting" @click="open('revert')"><Undo2 :size="15" />Revert</button>
    <button :aria-label="t('msgResetToCommit9e15f7', { p0: props.detail.shortHash })" :title="t('uiReset3d8134')" :disabled="history.submitting" @click="open('reset')"><RotateCcw :size="15" />Reset</button>
  </div>

  <ConfirmDialog v-if="active === 'checkout'" :title="t('uiCheckoutWithDetachedHEAD2ce873')" :description="t('msgCheckOutCommitInTheWorkingTreeWithoutMovingTheCurr62b9d1', { p0: shortHash })" :confirm-label="t('uiCheckoutCommit45ff42')" :busy="history.submitting" @cancel="active = undefined" @confirm="confirm" />
  <ConfirmDialog v-else-if="active === 'revert'" :title="t('uiRevertThisCommit8403af')" :description="t('msgCreateAnInverseCommitOnTheCurrentBranchToUndoPrese3ab534', { p0: repositories.snapshot?.currentBranch ?? t('uiNoBranchCheckedOut5130ff'), p1: shortHash })" :confirm-label="t('uiConfirmRevert27cfa9')" :confirm-disabled="props.detail.parentHashes.length > 1 && mainline === null" :busy="history.submitting" @cancel="active = undefined" @confirm="confirm">
    <p class="commit-subject">{{ props.detail.message.split('\n')[0] }}</p>
    <label v-if="props.detail.parentHashes.length > 1" class="field">{{ t('uiMainlineParentOfMergeCommit510ec3') }}<AppSelect v-model="mainline" :aria-label="t('uiRevertMainlineParent3bc689')" :placeholder="t('uiSelectTheParentToKeepAsMainlineaadf67')" :disabled="history.submitting" :options="props.detail.parentHashes.map((parent, index) => ({ value: index + 1, label: t('msgParent2b7399', { p0: index + 1, p1: parent.slice(0, 12) }) }))" /></label>
    <p v-if="props.detail.parentHashes.length > 1" class="field">{{ t('uiUndoesChangesIntroducedByTheMergeRelativeToTheSelectedParentd1a899') }}</p>
    <p v-if="history.error" role="alert" class="revert-error">{{ history.error.message }}</p>
  </ConfirmDialog>
  <ConfirmDialog v-else-if="active === 'cherryPick'" :title="t('uiCherryPickCommit1c6e85')" :description="t('msgApplyCommit554462', { p0: shortHash })" confirm-label="Cherry-pick" :busy="history.submitting" @cancel="active = undefined" @confirm="confirm">
    <label class="field">{{ t('uiTargetBranch55b297') }}<AppSelect v-model="targetBranch" :aria-label="t('uiCherryPickTargetBrancheab1fc')" :disabled="history.submitting" :options="[{ value: '', label: t('uiCurrentBranch0eb05c') }, ...(refs.snapshot?.localBranches ?? []).map(branch => ({ value: branch.name, label: branch.name }))]" /></label>
    <label v-if="targetBranch" class="check-field"><input v-model="returnAfterSuccess" type="checkbox" />{{ t('uiReturnToCurrentBranchOnSuccessf715b9') }}</label>
    <p class="field">{{ t('cherryPickPreserveUnstaged') }}</p>
    <p v-if="history.error" role="alert" class="revert-error">{{ history.error.message }}</p>
  </ConfirmDialog>
  <ConfirmDialog v-else-if="active === 'reset'" :title="t('uiResetCurrentBranchf212b1')" :description="t('msgMoveTheCurrentBranchTo7f37ca', { p0: shortHash })" :confirm-label="t('uiPerformReset94f347')" :confirm-disabled="resetDisabled" :busy="history.submitting" :danger="resetMode === 'hard'" @cancel="active = undefined" @confirm="confirm">
    <fieldset class="reset-modes">
      <legend>{{ t('uiResetMode1c08fe') }}</legend>
      <label><input v-model="resetMode" type="radio" value="soft" :aria-label="t('uiSoftReset39643f')" />{{ t('uiSoftReset39643f') }}</label>
      <label><input v-model="resetMode" type="radio" value="mixed" :aria-label="t('uiMixedReset5bced1')" />{{ t('uiMixedReset5bced1') }}</label>
      <label><input v-model="resetMode" type="radio" value="hard" :aria-label="t('uiHardResetc2d210')" />{{ t('uiHardResetc2d210') }}</label>
    </fieldset>
    <label v-if="resetMode === 'hard'" class="field">{{ t('uiEntere88504') }} {{ shortHash }} {{ t('uiConfirmb56d9a') }}<input v-model="resetConfirmation" :aria-label="t('uiEnterShortHashToConfirmd3a6a4')" autocomplete="off" /></label>
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
