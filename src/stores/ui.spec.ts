import { createPinia, disposePinia, setActivePinia, type Pinia } from "pinia";
import { flushPromises } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useUiStore } from "./ui";
import capabilities from "../../src-tauri/capabilities/default.json";

const native = vi.hoisted(() => ({ isTauri: vi.fn(), setTheme: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: native.isTauri }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ setTheme: native.setTheme }) }));

describe("page and native title bar themes", () => {
  let pinia: Pinia;
  let media: MediaQueryList;
  beforeEach(() => {
    pinia = createPinia(); setActivePinia(pinia);
    native.isTauri.mockReturnValue(true); native.setTheme.mockReset().mockResolvedValue(undefined);
    media = Object.assign(new EventTarget(), { matches: false, media: "(prefers-color-scheme: dark)" }) as MediaQueryList;
    vi.stubGlobal("matchMedia", vi.fn(() => media));
  });
  afterEach(() => { disposePinia(pinia); vi.unstubAllGlobals(); });
  it("applies saved or selected dark and light preferences to the title bar as well as the page", async () => {
    const ui = useUiStore();
    ui.applyTheme("dark"); await flushPromises();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(native.setTheme).toHaveBeenLastCalledWith("dark");
    ui.applyTheme("light"); await flushPromises();
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(native.setTheme).toHaveBeenLastCalledWith("light");
  });
  it("follows system changes and stops following them for an explicit preference", async () => {
    const ui = useUiStore(); ui.applyTheme("system"); await flushPromises();
    expect(native.setTheme).toHaveBeenLastCalledWith(null);
    Object.assign(media, { matches: true }); media.dispatchEvent(new Event("change"));
    expect(ui.resolvedTheme).toBe("dark");
    ui.applyTheme("light"); await flushPromises(); media.dispatchEvent(new Event("change"));
    expect(ui.resolvedTheme).toBe("light");
    ui.applyTheme("system"); await flushPromises();
    expect(ui.resolvedTheme).toBe("dark"); expect(native.setTheme).toHaveBeenLastCalledWith(null);
  });
  it("keeps browser preview independent of the desktop bridge", async () => {
    native.isTauri.mockReturnValue(false);
    const ui = useUiStore(); ui.applyTheme("dark"); await flushPromises();
    expect(ui.resolvedTheme).toBe("dark"); expect(native.setTheme).not.toHaveBeenCalled();
  });
  it("grants the native theme command to the main window", () => {
    expect(capabilities.windows).toContain("main");
    expect(capabilities.permissions).toContain("core:window:allow-set-theme");
  });
  it("serializes rapid changes and recovers after a native theme error", async () => {
    let finish!: () => void;
    native.setTheme.mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve; }));
    const ui = useUiStore(); ui.applyTheme("dark"); await flushPromises();
    ui.applyTheme("system"); ui.applyTheme("light");
    finish(); await flushPromises();
    expect(native.setTheme.mock.calls).toEqual([["dark"], ["light"]]);
    native.setTheme.mockRejectedValueOnce(new Error("permission denied"));
    ui.applyTheme("dark"); await flushPromises();
    expect(ui.nativeThemeError).toContain("同步失败");
    ui.applyTheme("light"); await flushPromises();
    expect(ui.nativeThemeError).toBe("");
    expect(native.setTheme).toHaveBeenLastCalledWith("light");
  });
  it("removes the system theme listener when its store is disposed", async () => {
    const ui = useUiStore(); ui.applyTheme("system"); await flushPromises();
    disposePinia(pinia);
    Object.assign(media, { matches: true }); media.dispatchEvent(new Event("change"));
    expect(ui.resolvedTheme).toBe("light");
  });
});
