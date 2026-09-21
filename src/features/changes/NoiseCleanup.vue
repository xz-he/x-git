<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, ref, watch } from "vue";
import { ScanLine, LoaderCircle } from "@lucide/vue";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";
import type { NoiseScan } from "@/lib/backend/types";
import { useChangesStore } from "@/stores/changes";
import { useRepositoryStore } from "@/stores/repository";

const changes = useChangesStore();
const repositories = useRepositoryStore();
const opened = ref(false);
const scanning = ref(false);
const restoring = ref(false);
const report = ref<NoiseScan>();
const selected = ref(new Set<string>());
const message = ref("");
const busy = computed(() => repositories.navigationBusy || scanning.value || restoring.value);
const selectedFiles = computed(() => report.value?.candidates.filter(file => selected.value.has(file.path)) ?? []);
let revision = 0;
watch([() => repositories.snapshot?.rootPath, () => repositories.generation], () => {
  revision += 1;
  opened.value = false;
  report.value = undefined;
  selected.value = new Set();
  message.value = "";
});

async function scan(): Promise<void> {
  if (busy.value) return;
  const version = ++revision;
  scanning.value = true;
  message.value = "";
  report.value = undefined;
  try {
    const result = await changes.scanNoise();
    if (version !== revision) return;
    report.value = result;
    selected.value = new Set(result.candidates.map(file => file.path));
    opened.value = true;
  } catch { /* ChangesList displays the store error. */ }
  finally { scanning.value = false; }
}

function toggle(path: string, event: Event): void {
  if (restoring.value) return;
  if ((event.target as HTMLInputElement).checked) selected.value.add(path);
  else selected.value.delete(path);
}

async function restore(): Promise<void> {
  if (busy.value || !selectedFiles.value.length) return;
  restoring.value = true;
  const version = revision;
  try {
    const result = await changes.restoreNoise(selectedFiles.value.map(file => ({ ...file })));
    if (version !== revision) return;
    message.value = result.skipped.length
      ? t('noiseRestorePartial', { restored: result.restored.length, skipped: result.skipped.length })
      : t('noiseRestoreComplete', { count: result.restored.length });
    // Fingerprints are single-use: index changes invalidate the previous scan.
    report.value = { candidates: [], skipped: result.skipped };
    selected.value = new Set();
    if (!result.skipped.length) opened.value = false;
  } catch {
    // Require a fresh scan after stale results or an execution error.
    report.value = undefined;
    selected.value = new Set();
  } finally { restoring.value = false; }
}
</script>

<template>
  <div class="noise-cleanup">
    <button class="scan-button" :aria-label="t('uiDetectNonSubstantiveChangesdeff45')" :disabled="busy || !changes.snapshot?.files.length" @click="scan">
      <LoaderCircle v-if="scanning" :size="14" class="spinning" /><ScanLine v-else :size="14" />
      {{ scanning ? t('uiScanning0eda54') : t('uiDetectNonSubstantiveChangesdeff45') }}
    </button>
    <small v-if="message" role="status">{{ message }}</small>
  </div>
  <ConfirmDialog v-if="opened" :title="t('uiRestoreLineEndingOnlyChangesae95d3')" :description="t('uiOnlyHandlesFilesWhoseEntireContentIsUnchangedExceptForCRLFLFe2b3bd')" :confirm-label="t('uiRestoreSelectedFiles06a343')" :confirm-disabled="!selectedFiles.length || repositories.navigationBusy" :busy="restoring" danger @cancel="!restoring && (opened = false)" @confirm="restore">
    <div class="noise-report">
      <p>{{ t('uiRestorable479b76') }} {{ report?.candidates.length ?? 0 }} {{ t('uiSelected8f8c98') }} {{ selectedFiles.length }} {{ t('uiitemsf7b2a6') }}</p>
      <p v-if="report && !report.candidates.length">{{ t('uiNoFilesCanBeSafelyRestoredInBulk5af8c3') }}</p>
      <label v-for="file in report?.candidates ?? []" :key="file.path" class="noise-file">
        <input type="checkbox" :aria-label="(t('uiRestore457d44') + ' ') + file.path" :checked="selected.has(file.path)" :disabled="restoring" @change="toggle(file.path, $event)" />
        <span>{{ file.path }}<small>{{ file.staged ? t('uiIndexWorkingTreeHEAD44627f') : t('uiWorkingTreeIndex3684fc') }} {{ t('uiOnlyLineEndingsDifferOrBytesAreIdenticald8e918') }}</small></span>
      </label>
      <details v-if="report?.skipped.length"><summary>{{ t('uiNotRestoredAutomatically507a45') }}{{ report.skipped.length }}）</summary><p v-for="file in report.skipped" :key="file.path"><strong>{{ file.path }}</strong><br />{{ file.reason }}</p></details>
      <p>{{ t('uiWhitespaceIndentationTrailingBlankLinesAndEncodingChangesAre839c5e') }}</p>
      <p v-if="changes.error" role="alert" class="noise-error">{{ changes.error.message }} {{ t('uiCloseAndScanAgaind59275') }}</p>
    </div>
  </ConfirmDialog>
</template>

<style scoped>
.noise-cleanup { display: grid; gap: 6px; padding: 10px 12px 4px; }
.scan-button { display: inline-flex; justify-content: center; align-items: center; gap: 6px; min-height: 30px; padding: 5px 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); font-size: 12px; }
.scan-button:disabled { opacity: .55; cursor: not-allowed; }
.noise-report { max-height: min(55vh, 460px); overflow-y: auto; overflow-wrap: anywhere; font-size: 12px; line-height: 1.6; }
.noise-file { display: flex; align-items: flex-start; gap: 8px; padding: 7px 0; border-bottom: 1px solid var(--border); }
.noise-file input { margin-top: 4px; flex-shrink: 0; }
.noise-file span { min-width: 0; }
.noise-file small { display: block; color: var(--text-muted); }
.noise-error { color: var(--danger); }
summary { cursor: pointer; }
.spinning { animation: noise-spin 1s linear infinite; }
@keyframes noise-spin { to { transform: rotate(360deg); } }
</style>
