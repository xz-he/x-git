<script setup lang="ts">
import { CheckCircle2, GitCommitHorizontal, LoaderCircle } from "@lucide/vue";
import { computed, ref } from "vue";

import { useChangesStore } from "@/stores/changes";
import { useTaskBranchesStore } from "@/stores/taskBranches";
import TaskBranchCommit from "./TaskBranchCommit.vue";
import { useTerminalStore } from "@/stores/terminal";

const changesStore = useChangesStore();
const taskBranches = useTaskBranchesStore();
const terminal = useTerminalStore();
const successHash = ref<string>();
const isCommitting = computed(
  () => changesStore.operation.kind === "commit",
);
const canCommit = computed(
  () =>
    changesStore.commitMessage.trim().length > 0 &&
    (changesStore.snapshot?.stagedCount ?? 0) > 0 &&
    !isCommitting.value && !taskBranches.submitting && !terminal.busy,
);

async function createCommit(): Promise<void> {
  successHash.value = undefined;
  try {
    const result = await changesStore.commit();
    successHash.value = result.shortHash;
  } catch {
    // The shared changes panel renders the structured error.
  }
}
</script>

<template>
  <section class="commit-panel" aria-labelledby="commit-heading">
    <div class="commit-copy">
      <div class="commit-title">
        <span>
          <GitCommitHorizontal :size="16" />
          <strong id="commit-heading">创建提交</strong>
        </span>
        <small>已暂存 {{ changesStore.snapshot?.stagedCount ?? 0 }}</small>
      </div>
      <textarea
        v-model="changesStore.commitMessage"
        :disabled="taskBranches.submitting"
        aria-label="提交说明"
        maxlength="500"
        rows="2"
        placeholder="输入提交说明"
        @keydown.ctrl.enter.prevent="canCommit && createCommit()"
      />
    </div>
    <div class="commit-actions">
      <div v-if="successHash" class="commit-success" role="status">
        <CheckCircle2 :size="14" />
        已创建 {{ successHash }}
      </div>
      <button
        class="commit-button"
        aria-label="创建提交"
        :disabled="!canCommit"
        @click="createCommit"
      >
        <LoaderCircle v-if="isCommitting" :size="15" class="spin" />
        <GitCommitHorizontal v-else :size="15" />
        {{ isCommitting ? "提交中" : "提交" }}
      </button>
    </div>
    <TaskBranchCommit />
  </section>
</template>

<style scoped>
.commit-panel {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 14px;
  width: 100%;
  min-width: 0;
  min-height: 112px;
  padding: 12px 16px;
  border-top: 1px solid var(--border);
  background: var(--surface-panel);
}

.commit-copy {
  display: grid;
  min-width: 0;
  gap: 8px;
}

.commit-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.commit-title span {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

.commit-title strong {
  font-size: 12px;
}

.commit-title small {
  color: var(--text-muted);
  font-size: 11px;
}

textarea {
  width: 100%;
  min-width: 0;
  min-height: 48px;
  max-height: 86px;
  resize: vertical;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--surface-app);
  color: var(--text);
  font: inherit;
  line-height: 1.4;
}

textarea:focus {
  border-color: var(--primary);
  outline: 2px solid var(--primary-soft);
}

.commit-actions {
  display: flex;
  width: 132px;
  flex-direction: column;
  align-items: stretch;
  justify-content: flex-end;
  gap: 8px;
}

.commit-success {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 5px;
  color: var(--success);
  font-size: 11px;
}

.commit-button {
  display: inline-flex;
  height: 34px;
  align-items: center;
  justify-content: center;
  gap: 7px;
  border-radius: var(--radius-md);
  background: var(--primary);
  color: white;
  font-weight: 600;
}

.spin {
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

@container (max-width: 480px) {
  .commit-panel {
    grid-template-columns: minmax(0, 1fr);
  }

  .commit-actions {
    width: 100%;
    flex-direction: row;
    align-items: center;
    justify-content: flex-end;
  }

  .commit-button {
    width: 112px;
  }
}
</style>
