<script setup lang="ts">
import type { RepositorySnapshot } from "@/lib/backend/types";
defineProps<{ repository: RepositorySnapshot }>();
</script>
<template>
  <div class="summary">
    <dl><dt>当前分支</dt><dd>{{ repository.currentBranch ?? "分离 HEAD" }}</dd></dl>
    <dl><dt>HEAD</dt><dd>{{ repository.headShortHash ?? "尚无提交" }}</dd></dl>
    <dl><dt>工作区</dt><dd>{{ repository.isClean ? "干净" : repository.changedFileCount + " 个文件有变更" }}</dd></dl>
    <dl><dt>冲突</dt><dd>{{ repository.conflictCount }}</dd></dl>
    <dl><dt>上游</dt><dd>{{ repository.upstream?.name ?? "未设置" }}</dd></dl>
    <dl><dt>远程</dt><dd>{{ repository.remotes.map((remote) => remote.name).join(", ") || "未配置" }}</dd></dl>
  </div>
</template>
<style scoped>
.summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 1px; margin: 24px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--border); overflow: hidden; }
dl { min-height: 82px; margin: 0; padding: 16px; background: var(--surface-panel); }
dt { margin-bottom: 10px; color: var(--text-muted); font-size: 11px; }
dd { margin: 0; overflow-wrap: anywhere; font-weight: 600; }
</style>
