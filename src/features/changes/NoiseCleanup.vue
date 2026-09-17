<script setup lang="ts">
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
    message.value = `已还原 ${result.restored.length} 个文件${result.skipped.length ? `，${result.skipped.length} 个未还原，请重新检测。` : '。'}`;
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
    <button class="scan-button" aria-label="检测无实质变更" :disabled="busy || !changes.snapshot?.files.length" @click="scan">
      <LoaderCircle v-if="scanning" :size="14" class="spinning" /><ScanLine v-else :size="14" />
      {{ scanning ? '正在检测…' : '检测无实质变更' }}
    </button>
    <small v-if="message" role="status">{{ message }}</small>
  </div>
  <ConfirmDialog v-if="opened" title="还原仅换行符变更" description="仅处理整份文件内容一致、只存在 CRLF/LF 换行差异的变更。已暂存候选会同时还原暂存区和工作区到 HEAD；未暂存候选还原到暂存版本。不生成新提交。" confirm-label="还原所选文件" :confirm-disabled="!selectedFiles.length || repositories.navigationBusy" :busy="restoring" danger @cancel="!restoring && (opened = false)" @confirm="restore">
    <div class="noise-report">
      <p>可还原 {{ report?.candidates.length ?? 0 }} 个 · 已选 {{ selectedFiles.length }} 个</p>
      <p v-if="report && !report.candidates.length">没有可安全批量还原的文件。</p>
      <label v-for="file in report?.candidates ?? []" :key="file.path" class="noise-file">
        <input type="checkbox" :aria-label="'还原 ' + file.path" :checked="selected.has(file.path)" :disabled="restoring" @change="toggle(file.path, $event)" />
        <span>{{ file.path }}<small>{{ file.staged ? '暂存区 + 工作区 → HEAD' : '工作区 → 暂存版本' }} · 仅换行符或字节一致</small></span>
      </label>
      <details v-if="report?.skipped.length"><summary>不自动还原（{{ report.skipped.length }}）</summary><p v-for="file in report.skipped" :key="file.path"><strong>{{ file.path }}</strong><br />{{ file.reason }}</p></details>
      <p>空格、缩进、末尾空行和编码变化不等于无效改动，均不会自动丢弃。确认前请停止外部编辑器写入。</p>
      <p v-if="changes.error" role="alert" class="noise-error">{{ changes.error.message }} 请关闭后重新检测。</p>
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
