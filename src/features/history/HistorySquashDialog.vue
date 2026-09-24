<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from "vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type { SquashPreview } from "@/lib/backend/types";
import { t } from "@/lib/i18n";
import { useHistoryStore } from "@/stores/history";
import { useOperationStore } from "@/stores/operation";
import { useRepositoryStore } from "@/stores/repository";
import { useUiStore } from "@/stores/ui";

const props = defineProps<{ commits: string[] }>();
const emit = defineEmits<{ close: [] }>();
const history = useHistoryStore();
const repositories = useRepositoryStore();
const operations = useOperationStore();
const preview = ref<SquashPreview>();
const message = ref("");
const acknowledged = ref(false);
const loading = ref(true);
const error = ref("");
const root = repositories.snapshot?.rootPath;
const generation = repositories.generation;
let alive = true;
onBeforeUnmount(() => { alive = false; });
watch(() => [repositories.snapshot?.rootPath, repositories.generation, repositories.snapshot?.currentBranch], () => {
  if (!history.submitting) emit("close");
});
onMounted(async () => {
  try {
    if (!root) return;
    const result = await backendClient.historySquashPreview(root, [...props.commits]);
    if (!alive || repositories.snapshot?.rootPath !== root || repositories.generation !== generation) return;
    preview.value = result;
    message.value = result.commits.map(commit => commit.subject).join("\n\n");
  } catch (cause) {
    if (alive) error.value = normalizeBackendError(cause).message;
  } finally {
    loading.value = false;
  }
});

function close(): void { if (!history.submitting) emit("close"); }
async function confirm(): Promise<void> {
  if (!preview.value || !acknowledged.value || !message.value.trim() || history.submitting || repositories.navigationBusy) return;
  if (repositories.snapshot?.rootPath !== root || repositories.generation !== generation) { close(); return; }
  error.value = "";
  try {
    await history.squash({
      commits: preview.value.commits.map(commit => commit.hash),
      message: message.value,
      expectedHead: preview.value.head,
      expectedBranch: preview.value.branch,
    });
    emit("close");
  } catch (cause) {
    error.value = normalizeBackendError(cause).message;
    if (operations.state.kind === "rebase") {
      emit("close");
      useUiStore().activeView = "conflicts";
    }
  }
}
</script>

<template>
  <Teleport to="body">
    <ConfirmDialog :title="t('squashTitle')" :description="t('squashDescription')" confirm-label="Squash"
      :busy="history.submitting" danger :confirm-disabled="loading || !preview || !acknowledged || !message.trim() || repositories.navigationBusy"
      @cancel="close" @confirm="confirm">
      <div class="squash-content">
        <p v-if="loading" role="status">{{ t('squashLoading') }}</p>
        <template v-if="preview">
          <p>{{ t('squashSummary', { branch: preview.branch, selected: preview.commits.length, rewritten: preview.rewrittenCount }) }}</p>
          <ol class="squash-commits"><li v-for="commit in preview.commits" :key="commit.hash"><code>{{ commit.hash.slice(0, 7) }}</code> {{ commit.subject }}</li></ol>
          <label class="squash-message">{{ t('squashMessage') }}<textarea v-model="message" :aria-label="t('squashMessage')" :disabled="history.submitting" rows="5" /></label>
          <p class="squash-warning">{{ t('squashWarning') }}</p>
        </template>
        <p v-if="error" role="alert" class="squash-error">{{ error }}</p>
      </div>
      <label v-if="preview" class="squash-ack"><input v-model="acknowledged" type="checkbox" :disabled="history.submitting" />{{ t('squashAcknowledge') }}</label>
    </ConfirmDialog>
  </Teleport>
</template>

<style scoped>
.squash-content { max-height: 48vh; overflow: auto; overflow-wrap: anywhere; }
.squash-commits { max-height: 130px; overflow: auto; padding-left: 24px; }
.squash-commits li { margin-bottom: 5px; }
.squash-commits code { color: var(--primary); }
.squash-message { display: grid; gap: 6px; }
.squash-message textarea { width: 100%; resize: vertical; min-height: 90px; padding: 8px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text); }
.squash-warning { color: var(--text-muted); }
.squash-ack { display: flex; align-items: flex-start; gap: 7px; margin-top: 12px; }
.squash-error { color: var(--danger); }
</style>
