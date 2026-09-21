<script setup lang="ts">
import { t } from '@/lib/i18n';
import { formatDisplayPath } from "@/lib/formatPath";
import { repositoryPathKey } from "@/lib/repositoryPaths";
import UpdateButton from "@/components/layout/UpdateButton.vue";
import { ArrowRight, FolderGit2, GitFork, Plus, Settings, Trash2, X } from "@lucide/vue";
import { computed, ref } from "vue";

import { dialogs } from "@/lib/backend/dialogs";
import { useRepositoryStore } from "@/stores/repository";
import { useSettingsStore } from "@/stores/settings";
import { useUiStore } from "@/stores/ui";

const repositories = useRepositoryStore();
const settingsStore = useSettingsStore();
const ui = useUiStore();
const appIcon = "/app-icon.png";
const cloneOpen = ref(false);
const cloneUrl = ref("");
const cloneTarget = ref("");
const formError = ref("");
const busy = computed(() => repositories.navigationBusy);

async function openRepository() {
  if (busy.value) return;
  const path = await dialogs.selectDirectory(t('uiOpenLocalRepositoryfb19d9'));
  if (path && !busy.value) await repositories.open(path).catch(() => undefined);
}

async function initRepository() {
  if (busy.value) return;
  const path = await dialogs.selectDirectory(t('uiChooseDirectoryToInitialize080d9f'));
  if (!path || busy.value) return;
  if (!window.confirm(t('uiInitializeAGitRepositoryInTheSelectedDirectory170995'))) return;
  await repositories.init(path).catch(() => undefined);
}

async function chooseCloneTarget() {
  const path = await dialogs.selectDirectory(t('uiChooseCloneDestinationc63ef6'));
  if (path) cloneTarget.value = path;
}

async function submitClone() {
  if (busy.value) return;
  formError.value = "";
  if (!cloneUrl.value.trim()) {
    formError.value = t('uiEnterARepositoryURL822b7f');
    return;
  }
  if (!cloneTarget.value) {
    formError.value = t('uiChooseADestinationDirectorye3cb62');
    return;
  }
  await repositories
    .clone(cloneUrl.value.trim(), cloneTarget.value)
    .then(() => {
      cloneOpen.value = false;
    })
    .catch(() => undefined);
}

async function removeRecent(path: string) {
  const recentRepoPaths = settingsStore.settings.recentRepoPaths.filter(
    (entry) => repositoryPathKey(entry) !== repositoryPathKey(path),
  );
  await settingsStore
    .save({ ...settingsStore.settings, recentRepoPaths })
    .catch(() => undefined);
}
</script>

<template>
  <main class="welcome">
    <header class="welcome-header">
      <div class="welcome-brand">
        <img :src="appIcon" alt="" />
        <div><h1>HQ Git</h1><p>{{ t('uiChooseARepositoryToGetStarted0b8ca8') }}</p></div>
      </div>
      <div class="welcome-actions"><UpdateButton /><button class="icon-button settings-button" :aria-label="t('uiOpenSettings857329')" :title="t('uiOpenSettings857329')" @click="ui.settingsDialogOpen = true"><Settings :size="18" /></button></div>
    </header>

    <section v-if="repositories.snapshot" class="current-repository" :aria-label="t('uiCurrentRepositorye2a5d1')">
      <div><strong :title="repositories.snapshot.name">{{ repositories.snapshot.name }}</strong><span :title="formatDisplayPath(repositories.snapshot.rootPath)">{{ formatDisplayPath(repositories.snapshot.rootPath) }}</span></div>
      <button :aria-label="t('uiContinueWithCurrentRepository7a449d')" @click="ui.homeVisible = false">{{ t('uiContinueWithCurrentRepository7a449d') }}<ArrowRight :size="16" /></button>
    </section>

    <div v-if="settingsStore.migrationWarning" class="notice">
      <span>{{ settingsStore.migrationWarning }}</span>
      <button @click="ui.settingsDialogOpen = true"><Settings :size="15" />{{ t('uiGoToSettings9ea2c9') }}</button>
    </div>

    <section class="entry-actions" :aria-label="t('uiRepositoryActions3d47ce')">
      <button class="primary-action" :aria-label="t('uiOpenLocalRepositoryfb19d9')" :disabled="busy" @click="openRepository">
        <FolderGit2 :size="20" /><span><strong>{{ t('uiOpenRepository47f538') }}</strong><small>{{ t('uiChooseALocalGitWorkingDirectory3a5061') }}</small></span>
      </button>
      <button :aria-label="t('uiCloneRemoteRepository319bf1')" :disabled="busy" @click="cloneOpen = true">
        <GitFork :size="20" /><span><strong>{{ t('uiCloneRepository2d9186') }}</strong><small>{{ t('uiCreateALocalCopyFromARemoteURLe5b181') }}</small></span>
      </button>
      <button :aria-label="t('uiInitializeLocalRepository25fb4c')" :disabled="busy" @click="initRepository">
        <Plus :size="20" /><span><strong>{{ t('uiInitializeRepositoryc681d9') }}</strong><small>{{ t('uiCreateAGitRepositoryInALocalDirectoryf1c17b') }}</small></span>
      </button>
    </section>

    <section v-if="settingsStore.settings.recentRepoPaths.length" class="recent">
      <h2>{{ t('uiRecentRepositoriesd9de82') }}</h2>
      <div class="recent-list">
        <div v-for="path in settingsStore.settings.recentRepoPaths" :key="path" class="recent-row">
          <button class="recent-open" :disabled="busy" :title="formatDisplayPath(path)" :aria-label="(t('uiOpenRecentRepository4c5b8b') + ' ') + formatDisplayPath(path)" @click="repositories.open(path).catch(() => undefined)">
            <FolderGit2 :size="16" /><span><strong>{{ path.split(/[\\/]/).filter(Boolean).at(-1) }}</strong><small>{{ formatDisplayPath(path) }}</small></span>
          </button>
          <button class="icon-button" :aria-label="(t('uiRemoveRecentRepositoryc0fd02') + ' ') + formatDisplayPath(path)" :title="t('uiRemove2f752c')" @click="removeRecent(path)"><Trash2 :size="15" /></button>
        </div>
      </div>
    </section>

    <div v-if="repositories.error" class="error-message" role="alert">
      <strong>{{ repositories.error.message }}</strong>
      <details v-if="repositories.error.diagnostics"><summary>{{ t('uiDiagnostics0b673e') }}</summary><pre>{{ repositories.error.diagnostics }}</pre></details>
    </div>

    <div v-if="cloneOpen" class="modal-backdrop" @click.self="cloneOpen = false">
      <section class="modal" role="dialog" aria-modal="true" aria-labelledby="clone-title">
        <header><h2 id="clone-title">{{ t('uiCloneRepository2d9186') }}</h2><button class="icon-button" :aria-label="t('uiCloseCloneDialoge5dc17')" @click="cloneOpen = false"><X :size="17" /></button></header>
        <label>{{ t('uiRepositoryURL6b470c') }}<input v-model="cloneUrl" type="text" placeholder="https://example.com/team/repository.git" /></label>
        <label>{{ t('uiDestinationDirectory4b86a8') }}<div class="target-input"><input v-model="cloneTarget" type="text" readonly /><button @click="chooseCloneTarget">{{ t('uiSelect70b208') }}</button></div></label>
        <p v-if="formError" class="form-error">{{ formError }}</p>
        <footer><button @click="cloneOpen = false">{{ t('uiCancel4d0b46') }}</button><button class="primary" :aria-label="t('uiStartClone38554c')" :disabled="busy" @click="submitClone">{{ busy ? t('uiCloningf68114') : t('uiClone6e6045') }}</button></footer>
      </section>
    </div>
  </main>
</template>

<style scoped>
.welcome { min-width: 1100px; min-height: 700px; height: 100vh; padding: 72px max(32px, calc((100vw - 860px) / 2)); background: var(--surface-app); overflow: auto; }
.welcome-header, .welcome-brand { display: flex; align-items: center; }
.welcome-header { justify-content: space-between; margin-bottom: 34px; }
.welcome-brand { gap: 14px; }
.welcome-actions { display: flex; align-items: center; gap: 8px; }
.welcome-header img { width: 44px; height: 44px; border-radius: var(--radius-md); }
h1, h2, p { margin: 0; }
h1 { font-size: 24px; }
.welcome-header p { margin-top: 4px; color: var(--text-muted); }
.current-repository { display: flex; min-width: 0; align-items: center; justify-content: space-between; gap: 16px; padding: 0 0 22px; margin-bottom: 22px; border-bottom: 1px solid var(--border); }
.current-repository > div { display: grid; min-width: 0; gap: 6px; }
.current-repository strong, .current-repository span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.current-repository span { color: var(--text-muted); font-size: 11px; }
.current-repository button { display: inline-flex; flex: 0 0 auto; align-items: center; gap: 8px; min-height: 34px; padding: 0 12px; border-radius: var(--radius-md); background: var(--primary); color: white; }
.entry-actions { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; }
.entry-actions > button { display: flex; min-height: 82px; align-items: center; gap: 12px; padding: 16px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); text-align: left; }
.entry-actions > button:hover:not(:disabled) { border-color: var(--primary); }
.entry-actions .primary-action { border-color: var(--primary); color: var(--primary); background: var(--primary-soft); }
.entry-actions span, .recent-open span { display: grid; gap: 5px; min-width: 0; }
small { color: var(--text-muted); font-size: 11px; }
.recent { margin-top: 36px; }
.recent h2 { margin-bottom: 10px; font-size: 13px; }
.recent-list { border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); overflow: hidden; }
.recent-row { display: flex; align-items: center; border-bottom: 1px solid var(--border); }
.recent-row:last-child { border-bottom: 0; }
.recent-open { display: flex; flex: 1; min-width: 0; align-items: center; gap: 10px; padding: 11px 14px; background: transparent; text-align: left; }
.recent-open small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.icon-button { display: grid; width: 32px; height: 32px; place-items: center; border-radius: var(--radius-md); background: transparent; }
.settings-button { border: 1px solid var(--border); background: var(--surface-panel); }
.notice, .error-message { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 18px; padding: 11px 13px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }
.notice button { display: inline-flex; align-items: center; gap: 5px; background: transparent; color: var(--primary); }
.error-message { display: block; margin-top: 18px; border-color: color-mix(in srgb, var(--danger), transparent 55%); color: var(--danger); }
details { margin-top: 8px; }
pre { max-height: 140px; overflow: auto; white-space: pre-wrap; color: var(--text-muted); font-size: 11px; }
.modal-backdrop { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(18 24 33 / 35%); }
.modal { width: min(480px, calc(100vw - 48px)); padding: 18px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); box-shadow: var(--shadow-window); }
.modal header, .modal footer, .target-input { display: flex; align-items: center; }
.modal header { justify-content: space-between; margin-bottom: 18px; }
.modal h2 { font-size: 15px; }
.modal label { display: grid; gap: 7px; margin-top: 13px; color: var(--text-muted); font-size: 12px; }
.modal input { width: 100%; height: 36px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); outline: none; }
.modal input:focus { border-color: var(--primary); }
.target-input { gap: 8px; }
.target-input button, .modal footer button { height: 34px; padding: 0 14px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); }
.modal footer { justify-content: flex-end; gap: 8px; margin-top: 20px; }
.modal footer .primary { border-color: var(--primary); background: var(--primary); color: white; }
.form-error { margin-top: 8px; color: var(--danger); font-size: 12px; }
</style>
