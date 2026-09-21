<script setup lang="ts">
import { t } from '@/lib/i18n';
import AppSelect from "./AppSelect.vue";

defineProps<{ modelValue: string; options: string[]; label: string; disabled?: boolean }>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();
</script>

<template>
  <AppSelect :model-value="modelValue" :options="options.map(name => ({ value: name, label: name }))" :aria-label="label" :disabled="disabled" editable :placeholder="t('uiSelectOrEnterABranchName460fa5')" @update:model-value="emit('update:modelValue', $event)" />
</template>

<style scoped>
input { width: 100%; min-width: 0; height: 34px; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text); }
</style>
