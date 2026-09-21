<script setup lang="ts">
import { t } from '@/lib/i18n';
import { X } from "@lucide/vue";
import { onMounted, ref } from "vue";
import UpdateSettings from "./UpdateSettings.vue";
import { useUiStore } from "@/stores/ui";
const ui = useUiStore();
const closeButton = ref<HTMLButtonElement>();
onMounted(() => closeButton.value?.focus());
</script>

<template>
  <div class="update-backdrop" @click.self="ui.updateDialogOpen = false" @keydown.esc.stop="ui.updateDialogOpen = false">
    <section class="update-dialog" role="dialog" aria-modal="true" aria-labelledby="update-dialog-title">
      <header><h2 id="update-dialog-title">{{ t('uiUpdatesca9576') }}</h2><button ref="closeButton" :aria-label="t('uiCloseUpdates9ec3e8')" @click="ui.updateDialogOpen = false"><X :size="17" /></button></header>
      <div class="update-content"><UpdateSettings /></div>
    </section>
  </div>
</template>

<style scoped>
.update-backdrop { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; background: var(--overlay); }
.update-dialog { display: flex; flex-direction: column; width: min(560px, calc(100vw - 32px)); max-height: calc(100vh - 48px); border: 1px solid var(--border); border-radius: var(--radius-lg); background: var(--surface-panel); box-shadow: var(--shadow-window); }
header { display: flex; flex-shrink: 0; align-items: center; justify-content: space-between; padding: 14px 18px; border-bottom: 1px solid var(--border); }
h2 { margin: 0; font-size: 15px; }
header button { display: grid; place-items: center; width: 30px; height: 30px; border-radius: var(--radius-md); background: transparent; }
header button:hover { background: var(--surface-muted); }
.update-content { min-height: 0; overflow-y: auto; padding: 18px; font-size: 12px; }
</style>
