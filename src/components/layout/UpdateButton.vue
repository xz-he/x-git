<script setup lang="ts">
import { ArrowUpCircle, LoaderCircle } from "@lucide/vue";
import { computed } from "vue";
import { useUpdatesStore } from "@/stores/updates";
import { useUiStore } from "@/stores/ui";

const updates = useUpdatesStore();
const ui = useUiStore();
const label = computed(() => {
  if (updates.phase === "downloading") return `正在下载更新${updates.progress === undefined ? '' : ` ${updates.progress}%`}`;
  if (updates.phase === "ready") return `更新 v${updates.latest?.version} 已下载，点击安装`;
  if (updates.latest) return `发现新版本 v${updates.latest.version}，查看更新`;
  if (updates.phase === "checking") return "正在检查版本更新";
  if (updates.error) return "更新检查失败，点击查看详情";
  return `版本更新 · 当前 v${updates.currentVersion}`;
});
function open() {
  ui.updateDialogOpen = true;
  updates.noticeVisible = false;
  if (!updates.latest) void updates.checkForUpdates();
}
</script>

<template>
  <button class="update-button" :class="{ available: updates.latest }" aria-label="版本更新" :title="label" @click="open">
    <LoaderCircle v-if="updates.busy" :size="17" class="spin" /><ArrowUpCircle v-else :size="17" />
    <span v-if="updates.latest" class="update-dot" aria-label="有可用更新" />
    <span v-else-if="updates.error" class="update-error" aria-label="更新检查失败">!</span>
  </button>
</template>

<style scoped>
.update-button { position: relative; display: grid; flex: 0 0 32px; width: 32px; height: 32px; place-items: center; border: 1px solid transparent; border-radius: var(--radius-md); background: transparent; color: var(--text-muted); }
.update-button:hover { border-color: var(--border); background: var(--surface-muted); }
.available { color: var(--primary); background: var(--primary-soft); }
.update-dot { position: absolute; top: 3px; right: 3px; width: 6px; height: 6px; border-radius: 50%; background: var(--primary); }
.update-error { position: absolute; top: 0; right: 2px; color: var(--danger); font-size: 11px; }
.spin { animation: update-button-spin 1s linear infinite; }
@keyframes update-button-spin { to { transform: rotate(360deg); } }
</style>
