<script setup lang="ts">
import { nextTick, onMounted, ref } from "vue";
import { TriangleAlert, X } from "@lucide/vue";

const props = withDefaults(
  defineProps<{
    title: string;
    description?: string;
    confirmLabel: string;
    cancelLabel?: string;
    confirmDisabled?: boolean;
    busy?: boolean;
    danger?: boolean;
  }>(),
  {
    description: undefined,
    cancelLabel: "取消",
    confirmDisabled: false,
    busy: false,
    danger: false,
  },
);
const emit = defineEmits<{ cancel: []; confirm: [] }>();
const cancelButton = ref<HTMLButtonElement>();

onMounted(async () => {
  await nextTick();
  cancelButton.value?.focus();
});
</script>

<template>
  <div class="dialog-backdrop" role="presentation" @click.self="emit('cancel')">
    <section
      class="confirm-dialog"
      :role="props.danger ? 'alertdialog' : 'dialog'"
      aria-modal="true"
      aria-labelledby="confirm-dialog-title"
      :aria-describedby="props.description ? 'confirm-dialog-description' : undefined"
    >
      <div v-if="props.danger" class="dialog-icon">
        <TriangleAlert :size="20" />
      </div>
      <div class="dialog-content">
        <header>
          <h2 id="confirm-dialog-title">{{ props.title }}</h2>
          <button class="icon-button" aria-label="关闭" title="关闭" @click="emit('cancel')">
            <X :size="16" />
          </button>
        </header>
        <p v-if="props.description" id="confirm-dialog-description">
          {{ props.description }}
        </p>
        <slot />
      </div>
      <footer>
        <button
          ref="cancelButton"
          data-action="cancel"
          class="secondary-button"
          :aria-label="props.cancelLabel"
          :disabled="props.busy"
          @click="emit('cancel')"
        >
          {{ props.cancelLabel }}
        </button>
        <button
          :class="props.danger ? 'danger-button' : 'primary-button'"
          :aria-label="props.confirmLabel"
          :disabled="props.confirmDisabled || props.busy"
          @click="emit('confirm')"
        >
          {{ props.busy ? "正在执行" : props.confirmLabel }}
        </button>
      </footer>
    </section>
  </div>
</template>

<style scoped>
.dialog-backdrop {
  position: fixed;
  z-index: 100;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 24px;
  background: var(--overlay);
}
.confirm-dialog {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 12px;
  width: min(460px, calc(100vw - 48px));
  padding: 18px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--surface-panel);
  box-shadow: var(--shadow-lg);
}
.dialog-content { min-width: 0; }
.dialog-content > header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
h2 { margin: 0; font-size: 15px; letter-spacing: 0; }
p { margin: 10px 0; color: var(--text-muted); font-size: 12px; line-height: 1.55; }
.dialog-icon { display: grid; width: 36px; height: 36px; place-items: center; border-radius: var(--radius-md); background: var(--danger-soft); color: var(--danger); }
.icon-button { display: grid; width: 28px; height: 28px; place-items: center; border-radius: var(--radius-sm); background: transparent; }
.icon-button:hover { background: var(--surface-muted); }
footer { grid-column: 1 / -1; display: flex; justify-content: flex-end; gap: 8px; padding-top: 6px; }
footer button { min-height: 34px; padding: 0 12px; border-radius: var(--radius-md); }
.secondary-button { border: 1px solid var(--border); background: var(--surface-panel); }
.primary-button { background: var(--primary); color: white; }
.danger-button { background: var(--danger); color: white; }
</style>
