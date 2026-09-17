<script setup lang="ts">
import { ArrowLeft, Bot, Square, X } from "@lucide/vue";
import { computed, onBeforeUnmount, ref, watch } from "vue";

import AiCommitMessageView from "@/features/ai/AiCommitMessageView.vue";
import AiReviewView from "@/features/ai/AiReviewView.vue";
import AiTaskHome from "@/features/ai/AiTaskHome.vue";
import AiConflictSuggestionView from "@/features/ai/AiConflictSuggestionView.vue";
import { useAiStore } from "@/stores/ai";
import { useAiChatStore } from "@/stores/aiChat";

type DrawerView = "home" | "review" | "commit" | "conflict";

const props = defineProps<{ width: number }>();
const emit = defineEmits<{ close: []; resize: [width: number]; resizeEnd: [] }>();
const ai = useAiStore();
const chat = useAiChatStore();
const view = ref<DrawerView>(
  ai.currentTask === "reviewChanges"
    ? "review"
    : ai.currentTask === "generateCommitMessage"
      ? "commit"
      : ai.currentTask === "resolveConflict" ? "conflict" : "home",
);
const running = computed(
  () => ai.status === "starting" || ai.status === "running",
);
const title = computed(() => {
  if (view.value === "review") return "AI 代码审查";
  if (view.value === "commit") return "AI 提交信息";
  if (view.value === "conflict") return "AI 冲突解决建议";
  return "AI 助手";
});
const progressText = computed(() => {
  if (ai.progressMessage) return ai.progressMessage;
  if (ai.currentTask === "resolveConflict" && ai.status === "starting") return "准备冲突三方版本与磁盘内容";
  if (ai.status === "starting") return ai.reviewSource?.kind === "commit" ? "准备历史提交与审查技能" : "准备已暂存上下文";
  if (ai.totalBatchCount > 0) {
    return `${ai.completedBatchCount} / ${ai.totalBatchCount} 批`;
  }
  return "正在处理";
});

let startX = 0;
watch(() => chat.revealVersion, () => {
  if (chat.openRequested) { view.value = "home"; chat.openRequested = false; }
}, { immediate: true });
let startWidth = 0;

watch(
  () => [ai.currentTask, ai.runId] as const,
  ([task]) => {
    if (task === "reviewChanges") view.value = "review";
    if (task === "generateCommitMessage") view.value = "commit";
    if (task === "resolveConflict") view.value = "conflict";
  },
);

function startResize(event: PointerEvent): void {
  startX = event.clientX;
  startWidth =
    (event.currentTarget as HTMLElement).parentElement?.getBoundingClientRect()
      .width ?? props.width;
  window.addEventListener("pointermove", resize);
  window.addEventListener("pointerup", stopResize, { once: true });
}

function resize(event: PointerEvent): void {
  emit(
    "resize",
    Math.min(560, Math.max(300, startWidth + startX - event.clientX)),
  );
}

function stopResize(): void {
  window.removeEventListener("pointermove", resize);
  emit("resizeEnd");
}

function startReview(): void {
  view.value = "review";
  void ai.startReview().catch(() => undefined);
}

function startCommitMessage(): void {
  view.value = "commit";
  void ai.startCommitMessage().catch(() => undefined);
}

function stopRun(): void {
  void ai.cancelActive().catch(() => undefined);
}

onBeforeUnmount(() => window.removeEventListener("pointermove", resize));
</script>

<template>
  <aside class="drawer" data-testid="ai-drawer" :style="{ width: width + 'px' }">
    <div
      class="resize-handle"
      role="separator"
      aria-orientation="vertical"
      aria-label="调整 AI 助手宽度"
      title="调整 AI 助手宽度"
      @pointerdown="startResize"
    />
    <header class="drawer-header">
      <button
        v-if="view !== 'home'"
        class="icon-button"
        aria-label="返回 AI 任务"
        title="返回 AI 任务"
        @click="view = 'home'"
      >
        <ArrowLeft :size="17" />
      </button>
      <span class="drawer-title" :class="{ home: view === 'home' }">
        <Bot v-if="view === 'home'" :size="17" />
        {{ title }}
      </span>
      <button
        class="icon-button close-button"
        aria-label="关闭 AI 助手"
        title="关闭 AI 助手"
        @click="emit('close')"
      >
        <X :size="17" />
      </button>
    </header>

    <div v-if="running" class="progress-header" aria-live="polite">
      <span class="progress-dot" />
      <span>{{ progressText }}</span>
      <button
        class="stop-button"
        aria-label="停止 AI 任务"
        title="停止 AI 任务"
        @click="stopRun"
      >
        <Square :size="12" fill="currentColor" />
      </button>
    </div>

    <div class="drawer-body">
      <AiTaskHome
        v-if="view === 'home'"
        @review="startReview"
        @commit="startCommitMessage"
      />
      <AiReviewView v-else-if="view === 'review'" />
      <AiConflictSuggestionView v-else-if="view === 'conflict'" />
      <AiCommitMessageView v-else />
    </div>
  </aside>
</template>

<style scoped>
.drawer {
  position: relative;
  grid-column: 4;
  grid-row: 3 / 5;
  display: grid;
  grid-template-rows: auto auto minmax(0, 1fr);
  min-width: 300px;
  max-width: min(560px, calc(100vw - 800px));
  min-height: 0;
  border-left: 1px solid var(--border);
  background: var(--surface-panel);
  box-shadow: var(--shadow-window);
}

.resize-handle {
  position: absolute;
  z-index: 4;
  top: 0;
  bottom: 0;
  left: -4px;
  width: 8px;
  cursor: ew-resize;
}

.resize-handle:hover {
  background: color-mix(in srgb, var(--primary) 25%, transparent);
}

.drawer-header {
  display: grid;
  grid-template-columns: 30px minmax(0, 1fr) 30px;
  gap: 6px;
  height: 48px;
  align-items: center;
  padding: 0 10px;
  border-bottom: 1px solid var(--border);
}

.drawer-title {
  min-width: 0;
  overflow: hidden;
  font-size: 13px;
  font-weight: 600;
  text-align: center;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.drawer-title.home {
  grid-column: 1 / 3;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  color: var(--primary);
  text-align: left;
}

.close-button {
  grid-column: 3;
}

.icon-button,
.stop-button {
  display: grid;
  width: 30px;
  height: 30px;
  place-items: center;
  border-radius: var(--radius-md);
  background: transparent;
}

.icon-button:hover,
.stop-button:hover {
  background: var(--surface-muted);
}

.progress-header {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  gap: 8px;
  height: 40px;
  align-items: center;
  padding: 0 9px 0 14px;
  border-bottom: 1px solid var(--border);
  background: var(--primary-soft);
  color: var(--primary);
  font-size: 11px;
  font-weight: 500;
}

.progress-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--primary);
  animation: pulse 1.2s ease-in-out infinite;
}

.stop-button {
  width: 28px;
  height: 28px;
  color: var(--primary);
}

.drawer-body {
  grid-row: 3;
  min-width: 0;
  min-height: 0;
  overflow: auto;
}

@keyframes pulse {
  50% {
    opacity: 0.35;
  }
}
</style>
