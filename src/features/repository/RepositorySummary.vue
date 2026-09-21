<script setup lang="ts">
import { t } from '@/lib/i18n';
import type { RepositorySnapshot } from "@/lib/backend/types";
defineProps<{ repository: RepositorySnapshot }>();
</script>
<template>
  <div class="summary">
    <dl><dt>{{ t('uiCurrentBranch0eb05c') }}</dt><dd>{{ repository.currentBranch ?? t('uiDetachedHEADddb9e0') }}</dd></dl>
    <dl><dt>HEAD</dt><dd>{{ repository.headShortHash ?? t('uiNoCommitsYeta9b427') }}</dd></dl>
    <dl><dt>{{ t('uiWorkingTreea1ff8d') }}</dt><dd>{{ repository.isClean ? t('uiCleanf598d3') : repository.changedFileCount + (' ' + t('uichangedFiles1ac00e')) }}</dd></dl>
    <dl><dt>{{ t('uiConflictsdc6013') }}</dt><dd>{{ repository.conflictCount }}</dd></dl>
    <dl><dt>{{ t('uiUpstreamed38f4') }}</dt><dd>{{ repository.upstream?.name ?? t('uiNotSet55a04b') }}</dd></dl>
    <dl><dt>{{ t('remotes') }}</dt><dd>{{ repository.remotes.map((remote) => remote.name).join(", ") || t('uiNotConfigured63595e') }}</dd></dl>
  </div>
</template>
<style scoped>
.summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 1px; margin: 24px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--border); overflow: hidden; }
dl { min-height: 82px; margin: 0; padding: 16px; background: var(--surface-panel); }
dt { margin-bottom: 10px; color: var(--text-muted); font-size: 11px; }
dd { margin: 0; overflow-wrap: anywhere; font-weight: 600; }
</style>
