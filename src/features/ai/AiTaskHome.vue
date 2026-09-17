<script setup lang="ts">
import {
  ChevronRight,
  MessageSquareText,
  ScanSearch,
  ShieldCheck,
} from "@lucide/vue";
import { computed } from "vue";

import { useAiStore } from "@/stores/ai";
import AiChatPanel from "./AiChatPanel.vue";
import { useAiChatStore } from "@/stores/aiChat";
import { useChangesStore } from "@/stores/changes";
import { useSettingsStore } from "@/stores/settings";
import { useRepositoryStore } from "@/stores/repository";
import ReviewSkillStatus from "./ReviewSkillStatus.vue";
import { useReviewSkill } from "./useReviewSkill";
import { useConflictSuggestionStore } from "@/stores/conflictSuggestion";
import { useConflictsStore } from "@/stores/conflicts";
import { useUiStore } from "@/stores/ui";

const emit = defineEmits<{ review: []; commit: [] }>();
const ai = useAiStore();
const chat = useAiChatStore();
const changes = useChangesStore();
const settingsStore = useSettingsStore();
const repositories = useRepositoryStore();
const skill = useReviewSkill();
const suggestion = useConflictSuggestionStore();
const conflicts = useConflictsStore();
const ui = useUiStore();
function startConflict(): void {
  ui.openView("conflicts");
  if (conflicts.current) void suggestion.start().catch(() => undefined);
}

const stagedCount = computed(() => changes.snapshot?.stagedCount ?? 0);
const running = computed(
  () => ai.status === "starting" || ai.status === "running" || chat.running,
);
const configured = computed(
  () =>
    settingsStore.settings.apiKey.trim().length > 0 &&
    settingsStore.settings.baseUrl.trim().length > 0 &&
    settingsStore.settings.model.trim().length > 0,
);
const actionsDisabled = computed(
  () => stagedCount.value === 0 || running.value || !configured.value || repositories.navigationBusy,
);
</script>

<template>
  <div class="task-home">
    <section class="scope-summary" aria-label="AI 数据范围">
      <span class="scope-icon"><ShieldCheck :size="17" /></span>
      <span>
        <strong>仅审查已暂存变更</strong>
        <small>已暂存 {{ stagedCount }} 个文件</small>
        <small>审查时按需读取索引中的相关定义作为证据；提交信息仅发送已暂存变更。</small>
      </span>
    </section>

    <ReviewSkillStatus />

    <p v-if="!configured" class="configuration-error" role="alert">
      请先在设置中完成 AI 服务配置。
    </p>

    <section class="task-list" aria-label="AI 任务">
      <button class="task-row" aria-label="AI 冲突解决建议"
        :disabled="repositories.navigationBusy || running || (!!conflicts.current && !!suggestion.startReason)"
        :title="suggestion.startReason" @click="startConflict">
        <span class="task-icon review"><ScanSearch :size="19" /></span>
        <span class="task-copy"><strong>冲突解决建议</strong><small>{{ conflicts.current ? suggestion.startReason || '三方版本与磁盘内容 · 预览后填入草稿' : '前往冲突工作台选择文件' }}</small><small>未保存草稿不发送；无需审查 SKILL</small></span>
        <ChevronRight :size="17" />
      </button>
      <button
        class="task-row"
        aria-label="审查已暂存变更"
        :disabled="actionsDisabled || !skill.ready.value"
        @click="emit('review')"
      >
        <span class="task-icon review"><ScanSearch :size="19" /></span>
        <span class="task-copy">
          <strong>审查已暂存变更</strong>
          <small>使用目标仓库 SKILL，输出等级与证据</small>
        </span>
        <ChevronRight :size="17" />
      </button>
      <button
        class="task-row"
        aria-label="生成已暂存提交信息"
        :disabled="actionsDisabled"
        @click="emit('commit')"
      >
        <span class="task-icon commit"><MessageSquareText :size="19" /></span>
        <span class="task-copy">
          <strong>生成提交信息</strong>
          <small>生成 Conventional Commit 草稿</small>
        </span>
        <ChevronRight :size="17" />
      </button>
    </section>

    <footer class="provider-summary">
      <span>{{ settingsStore.settings.aiProvider }}</span>
      <strong :title="settingsStore.settings.model">
        {{ settingsStore.settings.model || "未配置模型" }}
      </strong>
    </footer>
    <AiChatPanel />
  </div>
</template>

<style scoped>
.task-home {
  display: grid;
  align-content: start;
  gap: 18px;
  min-height: 100%;
  padding: 20px 16px;
}

.scope-summary {
  display: grid;
  grid-template-columns: 34px minmax(0, 1fr);
  gap: 10px;
  align-items: center;
  padding: 12px;
  border: 1px solid var(--primary-border);
  border-radius: var(--radius-lg);
  background: var(--primary-soft);
}

.scope-icon {
  display: grid;
  width: 34px;
  height: 34px;
  place-items: center;
  border-radius: var(--radius-md);
  background: var(--surface-panel);
  color: var(--primary);
}

.scope-summary > span:last-child,
.task-copy {
  display: grid;
  min-width: 0;
  gap: 3px;
}

.scope-summary strong,
.task-copy strong {
  font-size: 13px;
  font-weight: 600;
}

.scope-summary small,
.task-copy small,
.provider-summary {
  color: var(--text-muted);
  font-size: 11px;
}

.configuration-error {
  margin: 0;
  padding: 9px 10px;
  border: 1px solid color-mix(in srgb, var(--warning) 35%, var(--border));
  border-radius: var(--radius-md);
  color: var(--warning);
  font-size: 12px;
}

.task-list {
  display: grid;
  gap: 8px;
}

.task-row {
  display: grid;
  grid-template-columns: 38px minmax(0, 1fr) auto;
  gap: 10px;
  min-height: 64px;
  align-items: center;
  padding: 9px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface-panel);
  text-align: left;
}

.task-row:hover:not(:disabled) {
  border-color: var(--primary-border);
  background: var(--primary-soft);
}

.task-row > svg {
  color: var(--text-muted);
}

.task-icon {
  display: grid;
  width: 38px;
  height: 38px;
  place-items: center;
  border-radius: var(--radius-md);
}

.task-icon.review {
  background: var(--primary-soft);
  color: var(--primary);
}

.task-icon.commit {
  background: var(--diff-add-bg);
  color: var(--success);
}

.task-row:disabled {
  cursor: not-allowed;
  opacity: 0.52;
}

.provider-summary {
  display: flex;
  min-width: 0;
  justify-content: space-between;
  gap: 12px;
  padding-top: 4px;
  border-top: 1px solid var(--border);
  line-height: 28px;
}

.provider-summary strong {
  overflow: hidden;
  color: var(--text);
  font-weight: 500;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
