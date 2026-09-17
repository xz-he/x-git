<script setup lang="ts">
import { computed } from "vue";
import { RefreshCw } from "@lucide/vue";
import { useAiStore } from "@/stores/ai";
import { useUiStore } from "@/stores/ui";
const ai = useAiStore();
const ui = useUiStore();
const message = computed(() => ai.skillLoading ? "正在读取仓库审查技能…" : ai.reviewSkill?.error?.message
  ?? (ai.reviewSkill?.state === "missing" ? "未找到仓库审查 SKILL.md。" : ai.reviewSkill?.state === "ambiguous" ? "存在两份审查技能，请在设置中指定目录。" : ai.reviewSkill?.state === "ready" ? "仓库审查技能已就绪" : "尚未读取仓库审查技能。"));
</script>
<template>
  <section class="skill-status" aria-label="仓库审查技能">
    <strong>{{ message }}</strong>
    <template v-if="ai.reviewSkill?.state === 'ready' && ai.reviewSkill.info">
      <span>{{ ai.reviewSkill.info.name }} {{ ai.reviewSkill.info.version }}</span>
      <code>{{ ai.reviewSkill.info.directory }}/SKILL.md</code>
      <small>使用当前仓库维护的规则；开始审查时重新校验。</small>
    </template>
    <small v-else>自动查找 code-review-expert/SKILL.md 或 code-review-export/SKILL.md。</small>
    <div><button :disabled="ai.skillLoading || ai.running" aria-label="重新读取审查技能" @click="ai.refreshReviewSkill()"><RefreshCw :size="12" />重新读取</button><button @click="ui.settingsDialogOpen = true">设置技能目录</button></div>
  </section>
</template>
<style scoped>
.skill-status { display: grid; gap: 6px; min-width: 0; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); font-size: 11px; overflow-wrap: anywhere; }
small { color: var(--text-muted); line-height: 1.5; }code { white-space: pre-wrap; user-select: text; }div { display: flex; flex-wrap: wrap; gap: 6px; }button { display: inline-flex; align-items: center; gap: 4px; padding: 5px 7px; border-radius: var(--radius-sm); background: var(--surface-panel); color: var(--primary); }
</style>
