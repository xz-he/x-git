<script setup lang="ts">
import { t } from '@/lib/i18n';
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
    <section class="scope-summary" :aria-label="t('uiAIDataScope21bbd7')">
      <span class="scope-icon"><ShieldCheck :size="17" /></span>
      <span>
        <strong>{{ t('uiReviewStagedChangesOnly08ac42') }}</strong>
        <small>{{ t('staged') }} {{ stagedCount }} {{ t('uifiles621862') }}</small>
        <small>{{ t('uiReviewsReadRelatedIndexedDefinitionsAsNeededForEvidenceCommi49be95') }}</small>
      </span>
    </section>

    <ReviewSkillStatus />

    <p v-if="!configured" class="configuration-error" role="alert">
      {{ t('uiConfigureAnAIServiceInSettingsFirstdf7408') }}
    </p>

    <section class="task-list" :aria-label="t('uiAITasks26be93')">
      <button class="task-row" :aria-label="t('uiAIConflictSuggestions657d89')"
        :disabled="repositories.navigationBusy || running || (!!conflicts.current && !!suggestion.startReason)"
        :title="suggestion.startReason" @click="startConflict">
        <span class="task-icon review"><ScanSearch :size="19" /></span>
        <span class="task-copy"><strong>{{ t('uiConflictSuggestionsa6130e') }}</strong><small>{{ conflicts.current ? suggestion.startReason || t('uiThreeVersionsAndOnDiskContentPreviewBeforeInsertinga04ced') : t('uiSelectAFileInTheConflictWorkbench657016') }}</small><small>{{ t('uiUnsavedDraftsAreExcludedNoReviewSKILLRequired1b3f00') }}</small></span>
        <ChevronRight :size="17" />
      </button>
      <button
        class="task-row"
        :aria-label="t('uiReviewStagedChanges4ac49e')"
        :disabled="actionsDisabled || !skill.ready.value"
        @click="emit('review')"
      >
        <span class="task-icon review"><ScanSearch :size="19" /></span>
        <span class="task-copy">
          <strong>{{ t('uiReviewStagedChanges4ac49e') }}</strong>
          <small>{{ t('uiUsesTheRepositorySKILLToReportSeverityAndEvidence8d9aae') }}</small>
        </span>
        <ChevronRight :size="17" />
      </button>
      <button
        class="task-row"
        :aria-label="t('uiGenerateMessageForStagedChangesf0e6d7')"
        :disabled="actionsDisabled"
        @click="emit('commit')"
      >
        <span class="task-icon commit"><MessageSquareText :size="19" /></span>
        <span class="task-copy">
          <strong>{{ t('uiGenerateCommitMessagefaa200') }}</strong>
          <small>{{ t('uiGenerateAConventionalCommitDraft7f0cd8') }}</small>
        </span>
        <ChevronRight :size="17" />
      </button>
    </section>

    <footer class="provider-summary">
      <span>{{ settingsStore.settings.aiProvider }}</span>
      <strong :title="settingsStore.settings.model">
        {{ settingsStore.settings.model || t('uiNoModelConfigured454f9e') }}
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
