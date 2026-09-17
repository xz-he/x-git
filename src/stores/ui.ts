import { defineStore } from "pinia";
import { ref, watch } from "vue";

import type { ThemePreference } from "@/lib/backend/types";

export type ResolvedTheme = "light" | "dark";
export type WorkspaceView =
  | "terminal"
  | "files"
  | "changes"
  | "history"
  | "branches"
  | "tags"
  | "remotes"
  | "stashes"
  | "conflicts";

export function resolveTheme(
  preference: ThemePreference,
  systemPrefersDark: boolean,
): ResolvedTheme {
  return preference === "system"
    ? systemPrefersDark
      ? "dark"
      : "light"
    : preference;
}

export const useUiStore = defineStore("ui", () => {
  const layoutKey = "hq-git.workspace-layout.v1";
  let saved: Record<string, unknown> = {};
  try { saved = JSON.parse(localStorage.getItem(layoutKey) ?? "{}"); } catch { /* Storage can be unavailable. */ }
  const sidebarCollapsed = ref(saved?.sidebarCollapsed === true);
  const contextWidth = ref(typeof saved?.contextWidth === "number" && Number.isFinite(saved.contextWidth) ? Math.min(1000, Math.max(270, saved.contextWidth)) : 320);
  watch([sidebarCollapsed, contextWidth], () => {
    try { localStorage.setItem(layoutKey, JSON.stringify({ sidebarCollapsed: sidebarCollapsed.value, contextWidth: contextWidth.value })); } catch { /* Layout remains usable without storage. */ }
  }, { immediate: true });
  const activeView = ref<WorkspaceView>("changes");
  const homeVisible = ref(false);
  const resolvedTheme = ref<ResolvedTheme>("light");
  const settingsDialogOpen = ref(false);
  const diffFullscreen = ref(false);

  function applyTheme(preference: ThemePreference): void {
    const systemPrefersDark =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches;
    resolvedTheme.value = resolveTheme(preference, systemPrefersDark);
    document.documentElement.dataset.theme = resolvedTheme.value;
  }

  function openView(view: WorkspaceView): void {
    activeView.value = view;
  }

  return {
    sidebarCollapsed,
    contextWidth,
    activeView,
    homeVisible,
    resolvedTheme,
    settingsDialogOpen,
    diffFullscreen,
    applyTheme,
    openView,
  };
});
