<script setup lang="ts">
import { useId } from "vue";

defineProps<{ modelValue: string; options: string[]; label: string; disabled?: boolean }>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();
const listId = useId();
</script>

<template>
  <input :value="modelValue" :list="listId" :aria-label="label" :disabled="disabled"
    placeholder="选择或输入分支名" autocomplete="off" spellcheck="false"
    @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)" />
  <datalist :id="listId"><option v-for="name in options" :key="name" :value="name" /></datalist>
</template>

<style scoped>
input { width: 100%; min-width: 0; height: 34px; padding: 0 9px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-app); color: var(--text); }
</style>
