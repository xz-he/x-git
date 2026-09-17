import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { isTauri } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { setBackendClientForTests } from "@/lib/backend/client";
import { createBackendFixture } from "@/test/backend";
import { useUpdatesStore } from "./updates";
import { useRepositoryStore } from "./repository";
import { useConflictsStore } from "./conflicts";
import { useSettingsStore } from "./settings";
import { useUiStore } from "./ui";
import UpdateSettings from "@/components/layout/UpdateSettings.vue";
import SettingsDialog from "@/components/layout/SettingsDialog.vue";
import UpdateNotice from "@/components/layout/UpdateNotice.vue";

vi.mock("@tauri-apps/api/core", async original => ({ ...await original<object>(), isTauri: vi.fn(() => true) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(async () => "4.0.0") }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn(async () => {}) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function updateFixture() {
  return { currentVersion: "4.0.0", version: "4.0.1", body: "修复与优化", date: "2026-09-17T00:00:00Z",
    close: vi.fn(async () => {}), download: vi.fn(async (_listener?: (event: DownloadEvent) => void) => {}),
    install: vi.fn(async () => {}) };
}

describe("signed app updates", () => {
  let update: ReturnType<typeof updateFixture>;
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(isTauri).mockReturnValue(true);
    vi.mocked(getVersion).mockResolvedValue("4.0.0");
    vi.mocked(relaunch).mockResolvedValue(undefined);
    setActivePinia(createPinia());
    setBackendClientForTests(createBackendFixture());
    update = updateFixture();
    vi.mocked(check).mockResolvedValue(update as unknown as Update);
  });
  afterEach(() => vi.unstubAllEnvs());

  it("only checks once at startup and never downloads automatically", async () => {
    vi.stubEnv("DEV", false);
    const store = useUpdatesStore();
    await store.initialize(); await store.initialize();
    expect(check).toHaveBeenCalledTimes(1);
    expect(store.phase).toBe("available");
    expect(store.noticeVisible).toBe(true);
    expect(update.download).not.toHaveBeenCalled();
  });

  it("respects the startup preference and browser preview", async () => {
    vi.stubEnv("DEV", false);
    useSettingsStore().settings.checkUpdatesOnStartup = false;
    await useUpdatesStore().initialize();
    expect(check).not.toHaveBeenCalled();
    setActivePinia(createPinia()); vi.mocked(isTauri).mockReturnValue(false);
    const store = useUpdatesStore();
    await store.initialize(); await store.checkForUpdates();
    expect(check).not.toHaveBeenCalled();
    const wrapper = mount(UpdateSettings);
    expect(wrapper.text()).toContain("浏览器预览不支持安装更新");
    wrapper.unmount();
  });

  it("deduplicates checks and reports latest only on a successful empty response", async () => {
    const pending = deferred<Update | null>(); vi.mocked(check).mockReturnValueOnce(pending.promise);
    const store = useUpdatesStore(); const checking = store.checkForUpdates();
    await flushPromises(); await store.checkForUpdates();
    expect(check).toHaveBeenCalledTimes(1); expect(store.phase).toBe("checking");
    pending.resolve(null); await checking;
    expect(store.phase).toBe("current"); expect(store.checkedAt).toBeTruthy();
    vi.mocked(check).mockRejectedValueOnce(new Error("404")); await store.checkForUpdates();
    expect(store.phase).toBe("idle"); expect(store.error).toContain("latest.json");
    expect(store.noticeVisible).toBe(false);
    await store.checkForUpdates(); expect(store.phase).toBe("available");
  });

  it("keeps download pending until signature verification finishes, and allows retry", async () => {
    const pending = deferred<void>();
    update.download.mockImplementationOnce(async listener => {
      listener?.({ event: "Started", data: { contentLength: 200 } });
      listener?.({ event: "Progress", data: { chunkLength: 50 } });
      return pending.promise;
    });
    const store = useUpdatesStore(); await store.checkForUpdates();
    const downloading = store.download(); await store.download(); await store.checkForUpdates();
    expect(update.download).toHaveBeenCalledTimes(1); expect(check).toHaveBeenCalledTimes(1);
    expect(store.progress).toBe(25);
    update.download.mock.calls[0]?.[0]?.({ event: "Finished" });
    expect(store.phase).toBe("downloading");
    await store.install(); expect(update.install).not.toHaveBeenCalled();
    pending.reject(new Error("signature verification failed")); await downloading;
    expect(store.phase).toBe("available"); expect(store.error).toContain("signature verification failed");
    await store.download(); expect(store.phase).toBe("ready");
    await store.checkForUpdates(); expect(check).toHaveBeenCalledTimes(1);
  });

  it("shows bytes without inventing a percentage when length is unknown", async () => {
    update.download.mockImplementationOnce(async listener => {
      listener?.({ event: "Started", data: {} });
      listener?.({ event: "Progress", data: { chunkLength: 1024 } });
    });
    const store = useUpdatesStore(); await store.checkForUpdates(); await store.download();
    expect(store.downloadedBytes).toBe(1024); expect(store.progress).toBeUndefined();
    expect(store.phase).toBe("ready"); expect(update.install).not.toHaveBeenCalled();
  });

  it("blocks installation during Git work and with unsaved conflict drafts", async () => {
    const store = useUpdatesStore(); await store.checkForUpdates(); await store.download();
    useRepositoryStore().operation = { kind: "clone" };
    await store.install(); expect(update.install).not.toHaveBeenCalled();
    expect(store.error).toContain("Git / AI");
    useRepositoryStore().operation = { kind: "idle" };
    const conflicts = useConflictsStore();
    conflicts.drafts["a.ts"] = { dirty: true } as (typeof conflicts.drafts)[string];
    await store.install(); expect(update.install).not.toHaveBeenCalled();
    expect(store.error).toContain("草稿");
    delete conflicts.drafts["a.ts"];
    await store.install(); expect(update.install).toHaveBeenCalledTimes(1);
  });

  it("requires the separate confirmation click, rechecks busy state, and prevents duplicate install", async () => {
    const store = useUpdatesStore(); await store.checkForUpdates(); await store.download();
    const wrapper = mount(UpdateSettings);
    await wrapper.get('button.primary').trigger("click");
    expect(wrapper.find('[role="alertdialog"]').exists()).toBe(true);
    expect(update.install).not.toHaveBeenCalled();
    useRepositoryStore().operation = { kind: "clone" }; await flushPromises();
    expect(wrapper.get('button.primary').attributes("disabled")).toBeDefined();
    useRepositoryStore().operation = { kind: "idle" }; await flushPromises();
    const pending = deferred<void>(); update.install.mockReturnValueOnce(pending.promise);
    await wrapper.get('button.primary').trigger("click"); await store.install();
    expect(store.phase).toBe("installing"); expect(update.install).toHaveBeenCalledTimes(1);
    pending.resolve(); await flushPromises();
    expect(relaunch).toHaveBeenCalledTimes(1); wrapper.unmount();
  });

  it("allows a new download after installation fails and a restart retry after relaunch fails", async () => {
    const store = useUpdatesStore(); await store.checkForUpdates(); await store.download();
    update.install.mockRejectedValueOnce(new Error("access denied")); await store.install();
    expect(store.phase).toBe("available"); expect(store.error).toContain("access denied");
    expect(relaunch).not.toHaveBeenCalled();
    await store.download();
    vi.mocked(relaunch).mockRejectedValueOnce(new Error("restart unavailable")); await store.install();
    expect(store.phase).toBe("installed"); expect(store.error).toContain("手动退出");
    await store.restart(); expect(relaunch).toHaveBeenCalledTimes(2);
    expect(update.install).toHaveBeenCalledTimes(2);
  });

  it("keeps the update preference when an already open AI form saves", async () => {
    const wrapper = mount(SettingsDialog);
    useUiStore().settingsTab = "updates"; await flushPromises();
    await wrapper.get('input[type="checkbox"]').setValue(false); await flushPromises();
    useUiStore().settingsTab = "ai"; await flushPromises();
    await wrapper.get('[aria-label="保存 AI 设置"]').trigger("click"); await flushPromises();
    expect(useSettingsStore().settings.checkUpdatesOnStartup).toBe(false);
    wrapper.unmount();
  });

  it("opens the update tab from the notice without installing anything", async () => {
    const store = useUpdatesStore(); await store.checkForUpdates();
    const wrapper = mount(UpdateNotice);
    await wrapper.get('button').trigger("click");
    expect(useUiStore().settingsTab).toBe("updates");
    expect(useUiStore().settingsDialogOpen).toBe(true);
    expect(store.noticeVisible).toBe(false);
    expect(update.install).not.toHaveBeenCalled(); wrapper.unmount();
  });
});
