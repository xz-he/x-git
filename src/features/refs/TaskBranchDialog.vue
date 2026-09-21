<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, ref, watch } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import AppSelect from "@/components/common/AppSelect.vue";
import type { TaskBranchKind, TaskBranchMode } from "@/lib/backend/types";
import { taskBranchPreview } from "@/lib/taskBranch";
import { useTaskBranchSlug } from "./useTaskBranchSlug";
import { useRepositoryStore } from "@/stores/repository";
import { useTaskBranchesStore } from "@/stores/taskBranches";
import { formatDisplayPath } from "@/lib/formatPath";

const emit = defineEmits<{ close: []; created: [name: string, title: string] }>();
const repositories = useRepositoryStore();
const tasks = useTaskBranchesStore();
const initial = { root: repositories.snapshot?.rootPath, generation: repositories.generation, branch: repositories.snapshot?.currentBranch, head: repositories.snapshot?.headShortHash };
const kind = ref<TaskBranchKind>("feature");
const mode = ref<TaskBranchMode>("remoteMaster");
const ticket = ref("");
const { slug, description, translating, translationError, updateDescription, updateSlug, regenerate, stop } = useTaskBranchSlug();
const remoteNames = computed(() => repositories.snapshot?.remotes.map((item) => item.name) ?? []);
const remote = ref(remoteNames.value.includes("origin") ? "origin" : remoteNames.value.length === 1 ? remoteNames.value[0]! : "");
const copied = ref(false);
const copyError = ref("");
const preview = computed(() => taskBranchPreview(kind.value, ticket.value, slug.value, description.value));
const invalid = computed(() => !!preview.value.error || tasks.blocked || !initial.branch || !initial.head || (mode.value === "remoteMaster" && !remoteNames.value.includes(remote.value)));
watch([() => repositories.snapshot?.rootPath, () => repositories.generation, () => repositories.snapshot?.currentBranch, () => repositories.snapshot?.headShortHash], () => {
  close();
});
function close() { if (!tasks.submitting) { stop(); emit("close"); } }
async function copyTitle() {
  copied.value = false; copyError.value = "";
  try { await navigator.clipboard.writeText(preview.value.title); copied.value = true; }
  catch { copyError.value = t('uiCopyFailedSelectTheTitleAboveAndCopyItManually6976d1'); }
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
  <ConfirmDialog class="task-dialog" :title="t('uiQuickBranch2c5f2e')" :confirm-label="t('uiConfirmTaskBranchCreation0bec86')" :confirm-disabled="invalid" :busy="tasks.submitting" @cancel="close" @confirm="create">
    <p class="task-context">{{ formatDisplayPath(initial.root ?? '') }} · {{ initial.branch }}</p>
    <fieldset class="task-fields" :disabled="tasks.submitting">
      <label>{{ t('uiTaskType4a6f41') }}<AppSelect v-model="kind" :aria-label="t('uiTaskType4a6f41')" :disabled="tasks.submitting" :options="[{ value: 'feature', label: t('uiFeatureEnhancementFeature573e6f') }, { value: 'hotfix', label: t('uiBugFixHotfixeb5e14') }] as const" /></label>
      <label>{{ t('uiFullTaskIDa2cb26') }}<input v-model="ticket" :aria-label="t('uiFullTaskIDa2cb26')" :placeholder="kind === 'feature' ? 'R2026082681825' : 'B2026082681825'" /></label>
      <label>{{ t('uiChineseDescription02d25c') }}<input :value="description" :aria-label="t('uiChineseDescription02d25c')" :placeholder="t('uiAddPurchaseOrdersEnterInChinese95bf31')" @input="updateDescription(($event.target as HTMLInputElement).value, ($event as InputEvent).isComposing)" @compositionstart="stop" @compositionend="updateDescription(($event.target as HTMLInputElement).value)" /></label>
      <label>{{ t('uiShortEnglishDescription3ae64a') }}<input :value="slug" :aria-label="t('uiEnglishDescription3bbed2')" :title="t('uiAIGeneratesThisFromTheChineseDescriptionYouCanAlsoEditItManu0cdfb3')" :placeholder="t('uiGeneratedAfterEnteringAChineseDescriptiona28972')" @input="updateSlug(($event.target as HTMLInputElement).value)" /></label>
      <div class="task-translation">
        <span class="task-hint" role="status">{{ translating ? t('uiGeneratingEnglishDescriptionec0df9') : t('uiTranslatedByYourConfiguredAIServiceManualEditsArePreservedc36e1a') }}</span>
        <button type="button" :disabled="!description.trim() || translating" @click="regenerate">{{ t('uiRegenerate2e1905') }}</button>
      </div>
      <p v-if="translationError" role="alert" class="task-error">{{ translationError }}</p>
      <label>{{ t('uiCreationMode32d8a7') }}<AppSelect v-model="mode" :aria-label="t('uiCreationMode32d8a7')" :disabled="tasks.submitting" :options="[{ value: 'current', label: t('uiCreateFromCurrentBranchAndSwitch67bac5') }, { value: 'remoteMaster', label: t('uiCreateFromRemoteMasterStayOnDevelopmentBranch8619cb') }] as const" /></label>
      <label v-if="mode === 'remoteMaster'">{{ t('uiRemotee28c4a') }}<AppSelect v-model="remote" :aria-label="t('uiTaskBranchRemote1dec25')" :placeholder="t('uiSelectRemote01c506')" :disabled="tasks.submitting" :options="remoteNames.map(name => ({ value: name, label: name }))" /></label>
    </fieldset>
    <p class="task-hint">{{ mode === 'current' ? t('uiCreatesAndSwitchesToABranchAtTheCurrentHEADContinueDevelopmed6011c') : t('uiFetchesTheLatestRemoteMasterAndCreatesTheTaskBranchKeepDevela06a94') }}</p>
    <div class="task-preview"><small>{{ t('uiBranchName01571f') }}</small><code>{{ preview.name }}</code><small>{{ t('uiMRTitle41774b') }}</small><span>{{ preview.title }}</span><button type="button" :disabled="!!preview.error" @click="copyTitle">{{ copied ? t('uiCopiede381a5') : t('uiCopyMRTitledb55cf') }}</button></div>
    <p v-if="preview.error && (ticket || slug || description)" class="task-hint">{{ preview.error }}</p>
    <p v-if="mode === 'remoteMaster' && !remoteNames.length" class="task-error">{{ t('uiNoRemotesConfiguredInThisRepository848758') }}</p>
    <p v-if="copyError" role="alert" class="task-error">{{ copyError }}</p>
    <p v-if="tasks.error" role="alert" class="task-error">{{ tasks.error.message }}</p>
    <button v-if="tasks.uncertain" :disabled="tasks.loading || tasks.submitting" :aria-label="t('uiRefreshTaskStatus7ac37a')" @click="tasks.refreshState">{{ t('uiRefreshTaskStatus7ac37a') }}</button>
  </ConfirmDialog>
</template>

<style scoped>
@import "./taskBranches.css";
.task-dialog :deep(.confirm-dialog) { display: block; width: min(560px, calc(100vw - 48px)); max-height: calc(100vh - 48px); overflow-y: auto; }
.task-dialog :deep(footer) { margin-top: 12px; }
.task-translation { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
.task-translation button { flex-shrink: 0; padding: 4px; color: var(--primary); background: transparent; font-size: 12px; }
.task-translation button:disabled { opacity: .5; }
</style>
