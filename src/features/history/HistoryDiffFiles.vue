<script setup lang="ts">
import { ChevronRight, Copy, FileCode, FileQuestion, LoaderCircle } from "@lucide/vue";
import { nextTick, ref, useId, watch } from "vue";
import { useHistoryStore } from "@/stores/history";
import SplitDiff from "./SplitDiff.vue";

const history = useHistoryStore();
const expanded = ref(new Set<string>());
const container = ref<HTMLElement>();
const copyStatus = ref("");
const id = useId();

function load(path: string): void {
  void history.selectFile(path, false).catch(() => undefined);
}
function toggle(path: string): void {
  if (expanded.value.has(path)) expanded.value.delete(path);
  else { expanded.value.add(path); load(path); }
}
async function copyPath(path: string): Promise<void> {
  try { await navigator.clipboard.writeText(path); copyStatus.value = `已复制 ${path}`; }
  catch { copyStatus.value = "复制失败，请重试。"; }
}

watch(() => history.fileRevealVersion, async () => {
  const path = history.selectedFilePath;
  if (!path) return;
  expanded.value.add(path);
  await nextTick();
  const card = Array.from(container.value?.querySelectorAll<HTMLElement>("[data-file-path]") ?? []).find(element => element.dataset.filePath === path);
  card?.scrollIntoView?.({ block: "nearest" });
});

// Also support an already selected file when returning to the history page.
if (history.selectedFilePath && history.fileDiffs.has(history.selectedFilePath)) expanded.value.add(history.selectedFilePath);
</script>

<template>
  <section ref="container" class="history-diff-files" aria-label="提交文件差异">
    <div class="files-title"><h2>变更文件 <span>{{ history.detail?.files.length ?? 0 }}</span></h2><small>左右对比 · 点击文件展开</small></div>
    <p v-if="copyStatus" class="copy-status" role="status">{{ copyStatus }}</p>
    <article v-for="(file, index) in history.detail?.files" :key="file.path" class="file-card" :data-file-path="file.path">
      <header class="file-header">
        <button class="file-toggle" :aria-label="`查看提交文件 ${file.path}`" :aria-expanded="expanded.has(file.path)" :aria-controls="`${id}-${index}`" :title="file.oldPath ? `${file.oldPath} → ${file.path}` : file.path" @click="toggle(file.path)">
          <ChevronRight :size="14" class="chevron" :class="{ expanded: expanded.has(file.path) }" />
          <FileCode :size="15" />
          <b class="file-status">{{ file.status }}</b>
          <span class="file-path">{{ file.path }}</span>
        </button>
        <div class="file-stats"><span v-if="file.additions !== null" class="added">+{{ file.additions }}</span><span v-if="file.deletions !== null" class="removed">−{{ file.deletions }}</span></div>
        <button class="copy-path" :aria-label="`复制文件路径 ${file.path}`" title="复制文件路径" @click="copyPath(file.path)"><Copy :size="14" /></button>
      </header>
      <div v-if="expanded.has(file.path)" :id="`${id}-${index}`" class="file-body">
        <div v-if="file.oldPath && file.oldPath !== file.path" class="rename-path">重命名：{{ file.oldPath }} → {{ file.path }}</div>
        <div v-if="history.fileLoadingPaths.has(file.path)" class="file-state" role="status"><LoaderCircle :size="18" class="spin" />正在读取文件差异…</div>
        <div v-else-if="history.fileErrors.has(file.path)" class="file-state error" role="alert"><span>{{ history.fileErrors.get(file.path)?.message }}</span><button :aria-label="`重试文件 ${file.path}`" @click="load(file.path)">重试</button></div>
        <template v-else-if="history.fileDiffs.has(file.path)">
          <div v-if="history.fileDiffs.get(file.path)!.binary" class="file-state"><FileQuestion :size="20" />二进制文件无法显示文本差异</div>
          <SplitDiff v-else-if="history.fileDiffs.get(file.path)!.hunks.length" :hunks="history.fileDiffs.get(file.path)!.hunks" />
          <div v-else class="file-state">无文本差异（可能仅文件名或权限发生变化）</div>
        </template>
      </div>
    </article>
    <div v-if="!history.detail?.files.length" class="file-state">此提交没有变更文件</div>
  </section>
</template>

<style scoped>
.history-diff-files { min-width: 0; padding: 14px 16px 24px; }
.files-title { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; margin-bottom: 12px; }
h2 { margin: 0; font-size: 12px; } h2 span { margin-left: 6px; color: var(--text-muted); }
.files-title small, .copy-status { color: var(--text-muted); font-size: 11px; }
.file-card { min-width: 0; margin-bottom: 12px; border: 1px solid var(--border); border-radius: var(--radius-md); overflow: hidden; background: var(--surface-panel); }
.file-header { display: flex; align-items: center; min-height: 40px; background: var(--surface-muted); }
.file-toggle { display: flex; flex: 1; min-width: 0; align-items: center; gap: 7px; padding: 10px; background: transparent; text-align: left; }
.file-toggle > svg { flex-shrink: 0; color: var(--text-muted); }
.chevron { transition: transform 120ms; }.chevron.expanded { transform: rotate(90deg); }
.file-status { font-size: 11px; color: var(--primary); }
.file-path { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; font-weight: 600; }
.file-stats { display: flex; flex-shrink: 0; gap: 8px; padding: 0 6px; font: 11px var(--font-code); }
.added { color: var(--success); }.removed { color: var(--danger); }
.copy-path { display: grid; place-items: center; flex-shrink: 0; width: 30px; height: 28px; margin-right: 6px; border-radius: var(--radius-sm); background: transparent; color: var(--text-muted); }
.file-toggle:hover, .copy-path:hover { background: var(--primary-soft); }
.file-body { border-top: 1px solid var(--border); }
.file-state { display: flex; align-items: center; justify-content: center; flex-wrap: wrap; gap: 8px; min-height: 84px; padding: 18px; color: var(--text-muted); }
.file-state.error { color: var(--danger); }
.file-state button { padding: 5px 12px; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface-panel); }
.rename-path { padding: 7px 12px; color: var(--text-muted); font-size: 11px; overflow-wrap: anywhere; border-bottom: 1px solid var(--border); }
</style>
