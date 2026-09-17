<script setup lang="ts">
import { FileSearch } from "@lucide/vue";
import type { AiReviewIssue } from "@/lib/backend/types";
import { locationLabel, relationLabel, severityLabel } from "./reviewPresentation";
defineProps<{ issue: AiReviewIssue; navigationDisabled: boolean }>();
defineEmits<{ reveal: [issue: AiReviewIssue] }>();
</script>
<template>
  <article class="finding" :data-review-severity="issue.severity">
    <header><span class="severity" :class="issue.severity">{{ severityLabel[issue.severity] }}</span><button class="location" :aria-label="`查看 ${locationLabel(issue)}`" :title="locationLabel(issue)" :disabled="navigationDisabled || (!issue.startLine && !issue.endLine)" @click="$emit('reveal', issue)"><FileSearch :size="13" /><span>{{ locationLabel(issue) }}</span></button></header>
    <h3 v-if="issue.title">{{ issue.title }}</h3>
    <p>{{ issue.impact ?? issue.reason }}</p>
    <div class="suggested-fix"><strong>建议修复</strong><span>{{ issue.suggestedFix }}</span></div>
    <details v-if="issue.evidence || issue.confidence != null || issue.contextMissing?.length" class="evidence"><summary>证据与置信度 <span v-if="issue.confidence != null">{{ issue.confidence }}/10</span></summary><p v-if="issue.evidence">{{ issue.evidence }}</p><p v-if="issue.changeRelation">变更关系：{{ relationLabel[issue.changeRelation] ?? issue.changeRelation }}</p><ul v-if="issue.contextMissing?.length"><li v-for="missing in issue.contextMissing" :key="missing">缺失上下文：{{ missing }}</li></ul><ul v-if="issue.evidenceSources?.length"><li v-for="source in issue.evidenceSources" :key="`${source.path}:${source.startLine}:${source.revision}`"><code>{{ source.path }}:{{ source.startLine }}–{{ source.endLine }} @ {{ source.revision }}</code></li></ul></details>
  </article>
</template>
<style scoped>
.finding { min-width: 0; overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--surface-panel); font-size: 12px; overflow-wrap: anywhere; }
header { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; padding: 9px 10px; border-bottom: 1px solid var(--border); background: var(--surface-muted); }.severity { padding: 4px 6px; border-radius: var(--radius-sm); font-size: 10px; font-weight: 600; }.P0,.P1,.critical { background: var(--danger-soft); color: var(--danger); }.P2,.warning { color: var(--warning); }.P3,.suggestion { background: var(--primary-soft); color: var(--primary); }
.location { display: inline-flex; align-items: center; gap: 5px; min-width: 0; flex: 1 1 120px; background: transparent; color: var(--primary); font-size: 10px; text-align: left; }.location span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }h3 { margin: 10px 12px 0; font-size: 12px; }p { margin: 0; padding: 10px 12px; line-height: 1.6; white-space: pre-wrap; }.suggested-fix { display: grid; gap: 5px; margin: 0 12px 12px; padding: 9px; border-radius: var(--radius-md); background: var(--primary-soft); font-size: 11px; line-height: 1.5; white-space: pre-wrap; }.suggested-fix strong { color: var(--primary); }.evidence { border-top: 1px solid var(--border); padding: 9px 12px; font-size: 11px; }.evidence summary { cursor: pointer; color: var(--text-muted); }.evidence p { padding: 8px 0; }.evidence ul { padding-left: 15px; }code { user-select: text; }
</style>
