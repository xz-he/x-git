<script setup lang="ts">
import { ArrowDownToLine, ArrowUpFromLine, Braces, CloudDownload, Code2, GitBranch, GitCommitHorizontal, GitMerge, GitPullRequest, History, RadioTower, RotateCcw, Tags, Wrench } from "@lucide/vue";
import type { Component } from "vue";
import { PanelLeftClose, PanelLeftOpen, FolderGit2, ChevronDown } from "@lucide/vue";
import { formatDisplayPath } from "@/lib/formatPath";
import type { WorkspaceView } from "@/stores/ui";
import { useUiStore } from "@/stores/ui";
import { useRemotesStore, type RemoteDialogAction } from "@/stores/remotes";
import { useRepositoryStore } from "@/stores/repository";
import { useRefsStore, type IntegrationAction } from "@/stores/refs";

interface SidebarItem {
  label: string;
  icon: Component;
  view?: WorkspaceView;
  remoteAction?: RemoteDialogAction | "fetch";
  integrationAction?: IntegrationAction;
}

interface SidebarGroup {
  label: string;
  icon: Component;
  items: SidebarItem[];
}

const ui = useUiStore();
const remotes = useRemotesStore();
const repositories = useRepositoryStore();
const refs = useRefsStore();
const groups: SidebarGroup[] = [
  {
    label: "代码",
    icon: Code2,
    items: [
      { label: "仓库文件", icon: Code2, view: "files" },
      { label: "分支", icon: GitBranch, view: "branches" },
      { label: "标签", icon: Tags, view: "tags" },
      { label: "远程", icon: RadioTower, view: "remotes" },
      { label: "贮藏", icon: Braces, view: "stashes" },
    ],
  },
  {
    label: "变更",
    icon: GitPullRequest,
    items: [
      { label: "文件状态", icon: GitPullRequest, view: "changes" },
      { label: "提交记录", icon: History, view: "history" },
    ],
  },
  {
    label: "工具",
    icon: Wrench,
    items: [
      { label: "拉取", icon: ArrowDownToLine, remoteAction: "pull" },
      { label: "推送", icon: ArrowUpFromLine, remoteAction: "push" },
      { label: "获取", icon: CloudDownload, remoteAction: "fetch" },
      { label: "合并", icon: GitMerge, integrationAction: "merge" },
      { label: "变基", icon: RotateCcw, integrationAction: "rebase" },
      { label: "解决冲突", icon: GitPullRequest, view: "conflicts" },
      { label: "Git 命令", icon: Braces, view: "terminal" },
    ],
  },
];
const appIcon = "/app-icon.png";

async function activate(item: SidebarItem): Promise<void> {
  if (item.view ? repositories.viewNavigationBusy : repositories.navigationBusy) return;
  if (item.integrationAction) {
    refs.requestIntegration(item.integrationAction);
    return;
  }
  if (item.view) {
    ui.openView(item.view);
    return;
  }
  if (!item.remoteAction) {
    return;
  }
  ui.openView("remotes");
  const rootPath = repositories.snapshot?.rootPath;
  if (rootPath) {
    await remotes
      .ensureLoaded(rootPath, repositories.generation)
      .catch(() => undefined);
  }
  if (item.remoteAction === "fetch") {
    await remotes.fetchSelected().catch(() => undefined);
  } else {
    remotes.requestAction(item.remoteAction);
  }
}

function isDisabled(item: SidebarItem): boolean {
  if (item.view ? repositories.viewNavigationBusy : repositories.navigationBusy) return true;
  return item.integrationAction
    ? refs.integrationBlocked || !repositories.snapshot?.currentBranch
    : !item.view && !item.remoteAction;
}
</script>
<template>
  <aside class="sidebar" :class="{ collapsed: ui.sidebarCollapsed }" data-testid="app-sidebar">
    <div class="brand"><img :src="appIcon" alt="" /><strong>HQ Git</strong><button class="collapse-toggle" :aria-label="ui.sidebarCollapsed ? '展开侧边栏' : '折叠侧边栏'" :title="ui.sidebarCollapsed ? '展开侧边栏' : '折叠侧边栏'" :aria-expanded="!ui.sidebarCollapsed" @click="ui.sidebarCollapsed = !ui.sidebarCollapsed"><PanelLeftOpen v-if="ui.sidebarCollapsed" :size="17" /><PanelLeftClose v-else :size="17" /></button></div>
    <button class="repository-switch" aria-label="快速切换仓库" aria-haspopup="dialog" :aria-expanded="ui.repositorySwitcherOpen" :title="formatDisplayPath(repositories.snapshot?.rootPath) + ' · Ctrl+P 切换仓库'" @click="ui.repositorySwitcherOpen = true">
      <FolderGit2 :size="17" /><span v-if="!ui.sidebarCollapsed"><strong>{{ repositories.snapshot?.name ?? '选择仓库' }}</strong><small>Switch · Ctrl+P</small></span><ChevronDown v-if="!ui.sidebarCollapsed" :size="14" />
    </button>
    <nav aria-label="主导航">
      <section v-for="group in groups" :key="group.label">
        <h2><component :is="group.icon" :size="14" /><span>{{ group.label }}</span></h2>
        <button v-for="item in group.items" :key="item.label" class="nav-item" :data-view="item.view" :class="{ active: item.view === ui.activeView }" :disabled="isDisabled(item)" :aria-disabled="isDisabled(item)" :aria-label="item.remoteAction || item.integrationAction || item.view === 'conflicts' ? '打开' + item.label : undefined" :aria-current="item.view === ui.activeView ? 'page' : undefined" @click="activate(item)">
          <component :is="item.icon" :size="15" /><span :class="{ 'collapsed-label': ui.sidebarCollapsed }">{{ item.label }}</span><span v-if="ui.sidebarCollapsed" class="nav-tooltip" aria-hidden="true">{{ item.label }}</span>
        </button>
      </section>
    </nav>
  </aside>
</template>
<style scoped>
.sidebar { grid-row: 1 / -1; padding: 12px 10px; border-right: 1px solid var(--border); background: var(--surface-panel); overflow: auto; }
.brand { display: flex; align-items: center; gap: 9px; height: 36px; padding: 0 8px 12px; font-size: 15px; }
.brand img { width: 24px; height: 24px; border-radius: var(--radius-sm); }
.repository-switch { display: flex; width: 100%; align-items: center; gap: 8px; min-height: 44px; margin-top: 10px; padding: 8px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); text-align: left; }
.repository-switch:hover { border-color: var(--primary); }
.repository-switch > svg { flex-shrink: 0; }
.repository-switch span { display: grid; flex: 1; min-width: 0; gap: 3px; }
.repository-switch strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
.repository-switch small { color: var(--text-muted); font-size: 10px; }
.collapsed .repository-switch { justify-content: center; padding: 6px; }
section { margin-top: 14px; }
h2 { display: flex; align-items: center; gap: 6px; margin: 0 8px 5px; color: var(--text-muted); font-size: 11px; font-weight: 600; }
.nav-item { display: flex; align-items: center; gap: 9px; width: 100%; height: 32px; padding: 0 10px; border-radius: var(--radius-md); background: transparent; text-align: left; }
.nav-item.active { color: var(--primary); background: var(--primary-soft); font-weight: 600; }
.collapse-toggle { display: grid; place-items: center; width: 28px; height: 28px; margin-left: auto; padding: 0; background: transparent; border-radius: var(--radius-sm); flex-shrink: 0; }
.collapse-toggle:hover { background: var(--surface-muted); }
.brand { padding-left: 2px; padding-right: 0; }
.collapsed { padding-left: 8px; padding-right: 8px; }
.collapsed .brand { justify-content: center; }
.collapsed .brand img, .collapsed .brand strong, .collapsed h2 { display: none; }
.collapsed .collapse-toggle { margin: 0; }
.collapsed .nav-item { justify-content: center; padding: 0; }
.collapsed-label { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
.nav-tooltip { display: none; position: fixed; left: 50px; z-index: 120; padding: 5px 8px; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface-panel); color: var(--text); white-space: nowrap; pointer-events: none; }
.nav-item:hover .nav-tooltip, .nav-item:focus-visible .nav-tooltip { display: block; }
</style>
