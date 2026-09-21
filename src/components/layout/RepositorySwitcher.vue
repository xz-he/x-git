<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Check, FolderGit2, FolderOpen, Search, X } from "@lucide/vue";
import { dialogs } from "@/lib/backend/dialogs";
import { normalizeBackendError } from "@/lib/backend/errors";
import { formatDisplayPath } from "@/lib/formatPath";
import { repositoryPathKey, uniqueRepositoryPaths } from "@/lib/repositoryPaths";
import { useRepositoryStore } from "@/stores/repository";
import { useSettingsStore } from "@/stores/settings";
import { useConsoleStore } from "@/stores/console";
import { useUpdatesStore } from "@/stores/updates";
import { useUiStore } from "@/stores/ui";

const ui = useUiStore(), repositories = useRepositoryStore(), settings = useSettingsStore();
const consoleStore = useConsoleStore(), updates = useUpdatesStore();
const query = ref(""), error = ref(""), pending = ref(false), selecting = ref(false);
const active = ref(0), search = ref<HTMLInputElement>(), panel = ref<HTMLElement>();
let previousFocus: HTMLElement | null = null;
const busy = computed(() => repositories.navigationBusy || consoleStore.busy || pending.value || selecting.value);
const currentKey = computed(() => repositories.snapshot ? repositoryPathKey(repositories.snapshot.rootPath) : "");
const rows = computed(() => uniqueRepositoryPaths([
  ...(repositories.snapshot ? [repositories.snapshot.rootPath] : []), ...settings.settings.recentRepoPaths,
]).map(path => ({ path, key: repositoryPathKey(path), displayPath: formatDisplayPath(path), name: formatDisplayPath(path).split(/[\\/]/).filter(Boolean).at(-1) ?? path }))
  .filter(row => `${row.name} ${row.displayPath.replace(/\\/g, "/")}`.toLowerCase().includes(query.value.trim().replace(/\\/g, "/").toLowerCase())));

function close() { ui.repositorySwitcherOpen = false; }
function scrollActive() { void nextTick(() => document.getElementById(`repository-option-${active.value}`)?.scrollIntoView?.({ block: "nearest" })); }
watch(query, () => { active.value = 0; scrollActive(); });
watch(rows, () => { active.value = Math.min(active.value, Math.max(0, rows.value.length - 1)); });
watch(() => ui.repositorySwitcherOpen, async open => {
  if (open) {
    if (updates.phase === "installing") { close(); return; }
    previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    query.value = ""; error.value = ""; active.value = 0;
    await nextTick(); search.value?.focus();
  } else {
    await nextTick();
    if (previousFocus?.isConnected && !previousFocus.closest("[inert]")) previousFocus.focus();
  }
});
watch(() => updates.phase, phase => { if (phase === "installing") close(); });

async function switchTo(path: string) {
  if (busy.value) return;
  if (repositoryPathKey(path) === currentKey.value) { ui.homeVisible = false; close(); return; }
  pending.value = true; error.value = "";
  const before = repositories.snapshot;
  try {
    await repositories.open(path);
    // The existing conflict-draft confirmation can cancel the operation.
    if (repositories.snapshot !== before) close();
  } catch (cause) { error.value = normalizeBackendError(cause).message; }
  finally { pending.value = false; }
}
async function browse() {
  if (busy.value) return;
  selecting.value = true; error.value = "";
  try {
    const path = await dialogs.selectDirectory(t('uiOpenAnotherRepositoryc2bae1'));
    selecting.value = false;
    if (path) await switchTo(path);
  } catch (cause) { error.value = normalizeBackendError(cause).message; }
  finally { selecting.value = false; }
}
function keydown(event: KeyboardEvent) {
  if (event.isComposing) return;
  if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); }
  if ((event.key === "ArrowDown" || event.key === "ArrowUp") && event.target === search.value) {
    event.preventDefault();
    if (rows.value.length) active.value = (active.value + (event.key === "ArrowDown" ? 1 : -1) + rows.value.length) % rows.value.length;
    scrollActive();
  }
  if (event.key === "Enter" && event.target === search.value) {
    event.preventDefault();
    const row = rows.value[active.value]; if (row) void switchTo(row.path);
  }
  if (event.key === "Tab") {
    const elements = [...(panel.value?.querySelectorAll<HTMLElement>('input:not(:disabled), button:not(:disabled), [tabindex="0"]') ?? [])];
    const first = elements[0], last = elements.at(-1);
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
  }
}
function shortcut(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey || event.key.toLowerCase() !== "p" || event.isComposing || event.repeat) return;
  // Leave terminal control keys and other dialogs to their owning view.
  if (updates.phase === "installing" || (event.target instanceof Element && event.target.closest('.xterm')) ||
      (!ui.repositorySwitcherOpen && document.querySelector('[role="dialog"], [role="alertdialog"]'))) return;
  event.preventDefault(); event.stopPropagation();
  ui.repositorySwitcherOpen = !ui.repositorySwitcherOpen;
}
onMounted(() => window.addEventListener("keydown", shortcut, true));
onBeforeUnmount(() => window.removeEventListener("keydown", shortcut, true));
</script>

<template>
  <Teleport to="body">
    <div v-if="ui.repositorySwitcherOpen" class="switcher-backdrop" @click.self="close" @keydown="keydown">
      <section ref="panel" class="repository-switcher" role="dialog" aria-modal="true" aria-labelledby="repository-switcher-title">
        <header><div><h2 id="repository-switcher-title">{{ t('uiSwitchRepository46c7e1') }}</h2><p>{{ t('uiSwitchYourWorkingRepositoryWithoutReturningHome7c9557') }}</p></div><button class="icon" :aria-label="t('uiCloseRepositorySwitcheraae721')" @click="close"><X :size="18" /></button></header>
        <div class="search"><Search :size="18" /><input ref="search" v-model="query" :aria-label="t('uiSearchRepositoryNameOrPathac9f8f')" role="combobox" aria-autocomplete="list" aria-expanded="true" aria-controls="repository-options" :aria-activedescendant="rows.length ? `repository-option-${active}` : undefined" :placeholder="t('uiSearchRepositoryNameOrPathd20157')" autocomplete="off" /></div>
        <p v-if="busy" class="notice" role="status">{{ pending ? t('uiSwitchingRepositoriesfcf862') : selecting ? t('uiChooseARepositoryDirectory8265d3') : t('uiAnOperationIsRunningWaitForItToFinishBeforeSwitching82fc7c') }}</p>
        <p v-if="error" class="error" role="alert">{{ error }}</p>
        <div id="repository-options" class="repositories" role="listbox" :aria-label="t('uiRecentRepositoriesd9de82')" :aria-busy="busy">
          <div v-for="(row, index) in rows" :id="`repository-option-${index}`" :key="row.key" role="option" :aria-selected="active === index" :aria-disabled="busy" :data-path="row.path" class="repository-row" :class="{ active: active === index, disabled: busy }" :title="row.displayPath" @pointermove="active = index" @pointerdown.prevent @click="switchTo(row.path)">
            <FolderGit2 :size="20" /><span><strong>{{ row.name }}</strong><small>{{ row.displayPath }}</small></span><span v-if="row.key === currentKey" class="current"><Check :size="13" />{{ t('uiCurrent25e74d') }}</span>
          </div>
          <p v-if="!rows.length" class="empty">{{ query ? t('uiNoMatchingRepositoriesOpenAnotherDirectory0c652b') : t('uiNoRecentRepositoriesOpenALocalRepositoryFirstb56f68') }}</p>
        </div>
        <footer><span>{{ t('uiSelectEnterSwitchEscClose5faf92') }}</span><button :disabled="busy" :aria-label="t('uiOpenAnotherRepositoryc2bae1')" @click="browse"><FolderOpen :size="15" />Open…</button></footer>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
.switcher-backdrop { position: fixed; z-index: 300; inset: 0; display: grid; place-items: start center; padding: min(14vh, 110px) 20px 20px; background: var(--overlay); }
.repository-switcher { display: flex; flex-direction: column; width: min(600px, 100%); max-height: 100%; min-height: 0; overflow: hidden; border: 1px solid var(--border); border-radius: 12px; background: var(--surface-panel); box-shadow: var(--shadow-window); color: var(--text); }
header { display: flex; align-items: center; justify-content: space-between; padding: 18px 20px 14px; }
h2, p { margin: 0; } h2 { font-size: 16px; } header p { margin-top: 5px; color: var(--text-muted); font-size: 12px; }
.icon { display: grid; place-items: center; width: 30px; height: 30px; border-radius: var(--radius-md); background: transparent; }
.icon:hover { background: var(--surface-muted); }
.search { display: flex; flex-shrink: 0; align-items: center; gap: 10px; margin: 0 20px 12px; padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-md); color: var(--text-muted); }
.search:focus-within { border-color: var(--primary); box-shadow: 0 0 0 2px var(--primary-soft); }
input { width: 100%; height: 40px; background: transparent; border: 0; outline: none; color: var(--text); font: inherit; }
.repositories { min-height: 0; max-height: 360px; overflow: auto; padding: 4px 10px; }
.repository-row { display: flex; align-items: center; gap: 12px; padding: 12px 10px; border-radius: var(--radius-md); cursor: pointer; }
.repository-row > svg { flex-shrink: 0; color: var(--text-muted); }
.repository-row > span:not(.current) { display: grid; min-width: 0; flex: 1; gap: 5px; }
.repository-row strong { font-size: 13px; overflow-wrap: anywhere; }
.repository-row small { color: var(--text-muted); font-size: 11px; overflow-wrap: anywhere; }
.repository-row.active { background: var(--primary-soft); color: var(--primary); }
.repository-row.disabled { cursor: wait; opacity: .6; }
.current { display: flex; gap: 3px; align-items: center; flex-shrink: 0; font-size: 11px; color: var(--primary); }
.notice, .error { margin: 0 20px 10px; font-size: 12px; line-height: 1.5; } .notice { color: var(--text-muted); } .error { color: var(--danger); }
.empty { padding: 24px 12px; color: var(--text-muted); text-align: center; }
footer { display: flex; align-items: center; justify-content: space-between; flex-shrink: 0; gap: 12px; padding: 12px 20px; margin-top: 8px; border-top: 1px solid var(--border); }
footer > span { color: var(--text-muted); font-size: 11px; }
footer button { display: flex; align-items: center; gap: 6px; padding: 7px 12px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); white-space: nowrap; }
</style>
