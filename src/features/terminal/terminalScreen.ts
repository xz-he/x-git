import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { DEFAULT_CODE_FONT } from "@/lib/fonts";

export interface TerminalScreen {
  readonly cols: number; readonly rows: number;
  write(data: string | Uint8Array): Promise<void>;
  attach(host: HTMLElement): void; detach(): void; focus(): void;
  reset(): void; clear(): void; text(): string; dispose(): void;
}

// Owned by the session, not its Vue page: the parser keeps consuming output off-screen.
export async function createTerminalScreen(onInput: (data: string) => void, onResize: (cols: number, rows: number) => void): Promise<TerminalScreen> {
  const element = document.createElement("div");
  element.className = "terminal-surface";
  Object.assign(element.style, { width: "100%", height: "100%", minHeight: "0" });
  const fontFamily = () => getComputedStyle(document.documentElement).getPropertyValue("--font-code").trim() || DEFAULT_CODE_FONT;
  const terminal = new Terminal({ cols: 80, rows: 24, scrollback: 10000, cursorBlink: true, fontSize: 13, lineHeight: 1.2, fontFamily: fontFamily(), allowProposedApi: false });
  const fit = new FitAddon(); terminal.loadAddon(fit); terminal.open(element);
  const pending = new Set<() => void>();
  let disposed = false;
  let host: HTMLElement | undefined;
  const resize = () => { if (!disposed && host?.isConnected && host.clientWidth > 0 && host.clientHeight > 0) fit.fit(); };
  const observer = new ResizeObserver(resize);
  const theme = () => {
    const dark = document.documentElement.dataset.theme === "dark";
    terminal.options.theme = dark ? { background: "#11151b", foreground: "#dce5ef", cursor: "#78aaff", selectionBackground: "#355482" } : { background: "#ffffff", foreground: "#263445", cursor: "#1677ff", selectionBackground: "#b5d7ff" };
  };
  const themeObserver = new MutationObserver(theme);
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] }); theme();
  const updateFont = () => {
    const next = fontFamily();
    if (disposed || terminal.options.fontFamily === next) return;
    terminal.options.fontFamily = next;
    resize();
    void document.fonts?.ready.then(resize);
  };
  const fontObserver = new MutationObserver(updateFont);
  fontObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["style"] });
  terminal.onData(onInput);
  terminal.onResize(({ cols, rows }) => onResize(cols, rows));
  // Consume OSC 52 without allowing terminal output to write the user's clipboard.
  terminal.parser.registerOscHandler(52, () => true);
  terminal.attachCustomKeyEventHandler(event => {
    if (event.type === "keydown" && event.ctrlKey && event.key.toLowerCase() === "c" && terminal.hasSelection()) {
      void navigator.clipboard?.writeText(terminal.getSelection()).catch(() => undefined);
      return false;
    }
    return true;
  });
  return {
    get cols() { return terminal.cols; }, get rows() { return terminal.rows; },
    write(data) {
      if (disposed) return Promise.resolve();
      return new Promise(resolve => { const done = () => { pending.delete(done); resolve(); }; pending.add(done); terminal.write(data, done); });
    },
    attach(next) { host = next; next.append(element); observer.disconnect(); observer.observe(next); resize(); },
    detach() { host = undefined; observer.disconnect(); element.remove(); },
    focus() { if (!disposed) terminal.focus(); },
    reset() { if (!disposed) terminal.reset(); },
    clear() { if (!disposed) terminal.clear(); },
    text() {
      if (disposed) return "";
      if (terminal.hasSelection()) return terminal.getSelection();
      const buffer = terminal.buffer.active;
      const lines: string[] = [];
      for (let i = 0; i < buffer.length; i++) lines.push(buffer.getLine(i)?.translateToString(true) ?? "");
      return lines.join("\n").trimEnd();
    },
    dispose() { disposed = true; observer.disconnect(); themeObserver.disconnect(); fontObserver.disconnect(); terminal.dispose(); element.remove(); for (const done of pending) done(); pending.clear(); },
  };
}
