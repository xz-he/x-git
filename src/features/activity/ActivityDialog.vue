<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { History, RefreshCw, Undo2, X } from "@lucide/vue";
import { useActivityStore } from "@/stores/activity";
import { useRepositoryStore } from "@/stores/repository";
import { ACTIVITY_UPDATED, activityWarning } from "@/lib/backend/activity";
import ConfirmDialog from "@/components/common/ConfirmDialog.vue";

const emit = defineEmits<{ close: [] }>();
const activity = useActivityStore(), repositories = useRepositoryStore();
const selectedId = ref<string>(), confirming = ref(false);
const closeButton = ref<HTMLButtonElement>();
const selected = computed(() => activity.entries.find(entry => entry.id === selectedId.value));
const labels = { get pending() { return t('uiCompletionUnconfirmed80f917'); }, get success() { return t('uiSucceeded51991a'); }, get failed() { return t('uiFailed3e3c80'); } };
function close(): void { if (!activity.submitting) emit("close"); }
function updated(event: Event): void { if ((event as CustomEvent).detail === repositories.snapshot?.rootPath) void activity.load(); }
function keydown(event: KeyboardEvent): void { if (event.key === "Escape" && !activity.submitting) { if (confirming.value) confirming.value = false; else close(); } }
async function rollback(): Promise<void> { if (selected.value && await activity.rollback(selected.value)) confirming.value = false; }
watch(() => [repositories.snapshot?.rootPath, repositories.generation], () => { selectedId.value = undefined; confirming.value = false; void activity.load(); }, { immediate: true });
onMounted(() => { closeButton.value?.focus(); window.addEventListener(ACTIVITY_UPDATED, updated); window.addEventListener("keydown", keydown); });
onBeforeUnmount(() => { window.removeEventListener(ACTIVITY_UPDATED, updated); window.removeEventListener("keydown", keydown); });
</script>
<template>
  <Teleport to="body">
    <div class="activity-backdrop" @click.self="close">
      <section class="activity-dialog" role="dialog" aria-modal="true" aria-labelledby="activity-title" :inert="confirming || undefined">
        <header><h2 id="activity-title"><History :size="19" /> {{ t('uiOperationHistory56833a') }}</h2><button ref="closeButton" :aria-label="t('uiCloseOperationHistoryc252eb')" :disabled="activity.submitting" @click="close"><X :size="18" /></button></header>
        <div class="toolbar"><span>{{ repositories.snapshot?.name }} {{ t('uiLatest200Entries161804') }}</span><button :disabled="activity.loading || activity.submitting" @click="activity.load"><RefreshCw :size="14" />{{ t('uiRefreshHistory7b0ae4') }}</button></div>
        <p class="hint">{{ t('uiClickAnEntryToViewRollbackOptionsOnlyOperationsPerformedInHQba702f') }}</p>
        <p v-if="activity.error" role="alert" class="error">{{ activity.error }}</p>
        <p v-if="activityWarning" role="alert" class="error">{{ activityWarning }}</p>
        <div class="records" :aria-busy="activity.loading">
          <p v-if="!activity.entries.length" class="empty">{{ activity.loading ? t('uiLoadingOperationHistory9dbfa9') : t('uiNoOperationsRecorded33dbc8') }}</p>
          <article v-for="entry in activity.entries" :key="entry.id" :class="{ selected: entry.id === selectedId }">
            <button class="record" :aria-expanded="entry.id === selectedId" :disabled="activity.submitting" @click="selectedId = selectedId === entry.id ? undefined : entry.id; confirming = false">
              <span class="record-main"><strong>{{ entry.title }}</strong><time>{{ new Date(entry.createdAt).toLocaleString() }}</time></span><span class="status" :class="entry.status">{{ labels[entry.status] }}</span>
            </button>
            <div v-if="entry.id === selectedId" class="details">
              <p>{{ entry.message }}</p><p>{{ entry.rollbackReason }}</p>
              <button class="rollback" :disabled="activity.loading || !entry.rollbackKind || !!entry.rollbackId || repositories.navigationBusy" @click="confirming = true"><Undo2 :size="15" />{{ entry.rollbackId ? t('uiRollbackRequestede84d34') : t('uiRollBackThisOperation8c9e17') }}</button>
            </div>
          </article>
        </div>
      </section>
      <ConfirmDialog v-if="confirming && selected" :title="t('uiRollBackThisOperation72dced')" :description="selected.title + '：' + selected.rollbackReason" :confirm-label="t('uiConfirmRollback2a814f')" :busy="activity.submitting" :confirm-disabled="activity.loading || repositories.navigationBusy || !selected.rollbackKind || !!selected.rollbackId" @cancel="!activity.submitting && (confirming = false)" @confirm="rollback">
        <p v-if="activity.error" role="alert" class="error">{{ activity.error }}</p>
      </ConfirmDialog>
    </div>
  </Teleport>
</template>
<style scoped>
.activity-backdrop { position: fixed; inset: 0; z-index: 90; display: grid; place-items: center; padding: 24px; background: var(--overlay); }
.activity-dialog { display: flex; flex-direction: column; width: min(760px, 92vw); max-height: 85vh; min-height: 260px; padding: 20px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); box-shadow: var(--shadow-lg); }
header, .toolbar, h2 { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
h2 { margin: 0; font-size: 17px; justify-content: flex-start; }
button { display: inline-flex; align-items: center; gap: 6px; padding: 7px 10px; border-radius: var(--radius-sm); background: var(--surface-muted); }
button:disabled { opacity: .5; cursor: not-allowed; }
.toolbar { margin-top: 18px; color: var(--text-muted); }
.hint, time { color: var(--text-muted); font-size: 12px; }
.hint { line-height: 1.6; }
.records { min-height: 0; overflow-y: auto; margin-top: 8px; }
article { border: 1px solid var(--border); border-radius: var(--radius-sm); margin-bottom: 8px; overflow: hidden; }
article.selected { border-color: var(--primary); }
.record { width: 100%; padding: 12px; justify-content: space-between; text-align: left; background: transparent; }
.record:hover { background: var(--surface-muted); }
.record-main { display: grid; gap: 6px; min-width: 0; overflow-wrap: anywhere; }
.record-main strong { font-weight: 500; }
.status { white-space: nowrap; color: var(--text-muted); font-size: 12px; }
.status.success { color: #509c77; }.status.failed, .error { color: var(--danger); }
.details { border-top: 1px solid var(--border); padding: 4px 12px 12px; line-height: 1.7; overflow-wrap: anywhere; }
.details p { color: var(--text-muted); }
.rollback { color: var(--primary); border: 1px solid var(--border); background: var(--primary-soft); }
.empty { padding: 30px; text-align: center; color: var(--text-muted); }
.error { font-size: 13px; overflow-wrap: anywhere; }
</style>
