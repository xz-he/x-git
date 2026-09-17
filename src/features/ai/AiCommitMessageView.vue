<script setup lang="ts">
import {
  Check,
  CircleAlert,
  FileInput,
  Fingerprint,
  RefreshCw,
} from "@lucide/vue";
import { computed, nextTick, ref } from "vue";

import { isConventionalCommit, useAiStore } from "@/stores/ai";
import { useChangesStore } from "@/stores/changes";

const ai = useAiStore();
const changes = useChangesStore();
const confirmingReplacement = ref(false);
const keepButton = ref<HTMLButtonElement>();
const retrying = ref(false);

const validPreview = computed(() => isConventionalCommit(ai.commitPreview));
const canApply = computed(
  () =>
    ai.status === "completed" &&
    ai.currentTask === "generateCommitMessage" &&
    !!ai.commitResult &&
    validPreview.value,
);
const fingerprint = computed(
  () => ai.commitResult?.contextFingerprint ?? ai.context?.fingerprint ?? "",
);
const generatedSubject = computed(
  () => ai.commitPreview.trim().split(/\r?\n/, 1)[0] ?? "",
);
const existingSubject = computed(
  () => changes.commitMessage.trim().split(/\r?\n/, 1)[0] ?? "",
);

async function requestApply(): Promise<void> {
  if (!canApply.value) return;
  const existing = changes.commitMessage.trim();
  const generated = ai.commitPreview.trim();
  if (!existing || existing === generated) {
    ai.applyCommitMessage();
    return;
  }
  confirmingReplacement.value = true;
  await nextTick();
  keepButton.value?.focus();
}

function replaceCommitMessage(): void {
  if (ai.applyCommitMessage()) {
    confirmingReplacement.value = false;
  }
}

function retry(): void {
  if (retrying.value) return;
  confirmingReplacement.value = false;
  retrying.value = true;
  void ai
    .startCommitMessage()
    .catch(() => undefined)
    .finally(() => {
      retrying.value = false;
    });
}
</script>

<template>
  <div class="commit-message-view">
    <div v-if="ai.status === 'failed' && ai.error" class="state-banner error" role="alert">
      <CircleAlert :size="16" />
      <span>{{ ai.error.message }}</span>
      <button
        aria-label="重新生成提交信息"
        :disabled="retrying"
        @click="retry"
      >
        <RefreshCw :size="14" />重试
      </button>
    </div>
    <div v-else-if="ai.status === 'cancelled'" class="state-banner">
      <CircleAlert :size="16" />
      <span>生成已停止，当前草稿不会填入提交框。</span>
      <button
        aria-label="重新生成提交信息"
        :disabled="retrying"
        @click="retry"
      >
        <RefreshCw :size="14" />重新生成
      </button>
    </div>

    <section class="preview-section" aria-labelledby="commit-preview-heading">
      <header>
        <div>
          <strong id="commit-preview-heading">提交信息草稿</strong>
          <span v-if="ai.status === 'starting' || ai.status === 'running'">生成中</span>
          <span v-else-if="ai.status === 'completed'" class="complete"><Check :size="12" />已完成</span>
        </div>
        <small>{{ ai.context?.stagedFileCount ?? 0 }} 个已暂存文件</small>
      </header>
      <textarea
        v-model="ai.commitPreview"
        aria-label="AI 提交信息草稿"
        maxlength="500"
        rows="8"
        spellcheck="false"
        placeholder="正在生成提交信息..."
      />
      <p
        v-if="ai.status === 'completed' && ai.commitPreview.trim() && !validPreview"
        class="validation-error"
        role="alert"
      >
        不符合 Conventional Commit 格式，请修改后再填入。
      </p>
    </section>

    <dl class="metadata">
      <div>
        <dt><Fingerprint :size="13" />暂存指纹</dt>
        <dd :title="fingerprint">{{ fingerprint || "等待上下文" }}</dd>
      </div>
    </dl>

    <button
      class="apply-button"
      aria-label="填入提交框"
      :disabled="!canApply"
      @click="requestApply"
    >
      <FileInput :size="16" />
      填入提交框
    </button>

    <div
      v-if="confirmingReplacement"
      class="modal-backdrop"
      role="presentation"
      @click.self="confirmingReplacement = false"
    >
      <section
        class="replace-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="replace-title"
        aria-describedby="replace-description"
      >
        <div class="dialog-icon"><FileInput :size="19" /></div>
        <div class="dialog-copy">
          <h2 id="replace-title">替换现有提交信息？</h2>
          <p id="replace-description">提交框中已有草稿，替换后原内容将被覆盖。</p>
          <dl>
            <div>
              <dt>现有草稿</dt>
              <dd>{{ existingSubject }}</dd>
            </div>
            <div>
              <dt>生成内容</dt>
              <dd>{{ generatedSubject }}</dd>
            </div>
          </dl>
        </div>
        <footer>
          <button
            ref="keepButton"
            class="secondary-button"
            aria-label="保留原内容"
            @click="confirmingReplacement = false"
          >
            保留原内容
          </button>
          <button
            class="primary-button"
            aria-label="替换提交信息"
            @click="replaceCommitMessage"
          >
            替换提交信息
          </button>
        </footer>
      </section>
    </div>
  </div>
</template>

<style scoped>
.commit-message-view {
  display: grid;
  align-content: start;
  gap: 14px;
  padding: 16px;
}

.state-banner {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  gap: 8px;
  align-items: center;
  padding: 9px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  color: var(--text-muted);
  font-size: 11px;
}

.state-banner.error {
  border-color: color-mix(in srgb, var(--danger) 35%, var(--border));
  background: var(--danger-soft);
  color: var(--danger);
}

.state-banner button {
  display: inline-flex;
  height: 26px;
  align-items: center;
  gap: 4px;
  padding: 0 7px;
  border-radius: var(--radius-sm);
  background: var(--surface-panel);
  color: inherit;
}

.preview-section {
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface-panel);
}

.preview-section > header {
  display: flex;
  min-height: 44px;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 0 11px;
  border-bottom: 1px solid var(--border);
  background: var(--surface-muted);
}

.preview-section header div {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 7px;
}

.preview-section header strong {
  font-size: 12px;
}

.preview-section header span,
.preview-section header small {
  color: var(--text-muted);
  font-size: 10px;
}

.preview-section header span.complete {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  color: var(--success);
}

.preview-section textarea {
  display: block;
  width: 100%;
  min-height: 176px;
  max-height: 280px;
  resize: vertical;
  padding: 12px;
  border: 0;
  outline: 0;
  background: var(--surface-panel);
  color: var(--text);
  font-family: var(--font-code);
  font-size: 12px;
  line-height: 1.6;
}

.preview-section textarea:focus {
  box-shadow: inset 0 0 0 2px var(--primary-border);
}

.validation-error {
  margin: 0;
  padding: 8px 11px;
  border-top: 1px solid color-mix(in srgb, var(--danger) 30%, var(--border));
  background: var(--danger-soft);
  color: var(--danger);
  font-size: 10px;
}

.metadata {
  margin: 0;
  padding: 0 2px;
}

.metadata div {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 10px;
  align-items: center;
}

.metadata dt {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  color: var(--text-muted);
  font-size: 10px;
}

.metadata dd {
  overflow: hidden;
  margin: 0;
  color: var(--text-muted);
  font-family: var(--font-code);
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.apply-button {
  display: inline-flex;
  width: 100%;
  height: 36px;
  align-items: center;
  justify-content: center;
  gap: 7px;
  border-radius: var(--radius-md);
  background: var(--primary);
  color: white;
  font-size: 12px;
  font-weight: 600;
}

.apply-button:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}

.modal-backdrop {
  position: fixed;
  z-index: 30;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 24px;
  background: var(--overlay);
}

.replace-dialog {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 12px;
  width: min(440px, calc(100vw - 48px));
  padding: 20px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface-panel);
  box-shadow: var(--shadow-window);
}

.dialog-icon {
  display: grid;
  width: 36px;
  height: 36px;
  place-items: center;
  border-radius: var(--radius-md);
  background: var(--primary-soft);
  color: var(--primary);
}

.dialog-copy h2 {
  margin: 1px 0 7px;
  font-size: 15px;
}

.dialog-copy p {
  margin: 0 0 11px;
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.5;
}

.dialog-copy dl {
  display: grid;
  gap: 7px;
  margin: 0;
}

.dialog-copy dl div {
  display: grid;
  gap: 3px;
}

.dialog-copy dt {
  color: var(--text-muted);
  font-size: 10px;
}

.dialog-copy dd {
  overflow-wrap: anywhere;
  margin: 0;
  padding: 7px 8px;
  border-radius: var(--radius-sm);
  background: var(--surface-muted);
  font-family: var(--font-code);
  font-size: 11px;
}

.replace-dialog footer {
  grid-column: 1 / -1;
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 7px;
}

.secondary-button,
.primary-button {
  height: 32px;
  padding: 0 12px;
  border-radius: var(--radius-md);
  font-size: 12px;
}

.secondary-button {
  border: 1px solid var(--border);
  background: var(--surface-panel);
}

.primary-button {
  background: var(--primary);
  color: white;
}
</style>
