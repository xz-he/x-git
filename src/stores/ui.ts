import { defineStore } from "pinia";
import { onScopeDispose, ref, watch } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

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
  const updateDialogOpen = ref(false);
  const settingsTab = ref<"appearance" | "ai" | "updates">("appearance");
  const diffFullscreen = ref(false);
  const nativeThemeError = ref("");
  const systemTheme = typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)") : undefined;
  let themePreference: ThemePreference = "system";
  let themeQueue = Promise.resolve();
  let themeVersion = 0;

  function applyPageTheme(): void {
    resolvedTheme.value = resolveTheme(themePreference, systemTheme?.matches ?? false);
    document.documentElement.dataset.theme = resolvedTheme.value;
  }
  function systemThemeChanged(): void {
    if (themePreference === "system") applyPageTheme();
  }
  systemTheme?.addEventListener("change", systemThemeChanged);
  onScopeDispose(() => {
    themeVersion++;
    systemTheme?.removeEventListener("change", systemThemeChanged);
  });

  function applyTheme(preference: ThemePreference): void {
    themePreference = preference;
    applyPageTheme();
    nativeThemeError.value = "";
    if (!isTauri()) return;
    const version = ++themeVersion;
    // Serialize native updates so rapid toggles cannot leave the title bar on an older theme.
    themeQueue = themeQueue.then(async () => {
      if (version !== themeVersion) return;
      await getCurrentWindow().setTheme(preference === "system" ? null : preference);
    }).catch(() => {
      if (version === themeVersion) nativeThemeError.value = "标题栏主题同步失败，请重新切换主题重试。";
    });
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
    updateDialogOpen,
    settingsTab,
    diffFullscreen,
    nativeThemeError,
    applyTheme,
    openView,
  };
});
