import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { DEFAULT_CODE_FONT } from "@/lib/fonts";
import { TerminalPrompt, type PromptCallbacks } from "./terminalPrompt";

export interface TerminalScreen {
  readonly cols: number; readonly rows: number;
  write(data: string | Uint8Array): Promise<void>;
  attach(host: HTMLElement): void; detach(): void; focus(): void;
  reset(): void; clear(): void; text(): string; dispose(): void;
  setPrompt(label: string | null, draft: string): void;
  commitPrompt(label: string, command: string): Promise<void>;
  restoreInput(): Promise<void>;
}

// Owned by the session, not its Vue page: the parser keeps consuming output off-screen.
export async function createTerminalScreen(onInput: (data: string) => void, onResize: (cols: number, rows: number) => void, callbacks: PromptCallbacks): Promise<TerminalScreen> {
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
  terminal.options.theme = { background: "#141627", foreground: "#d8e1eb", cursor: "#23c17b", selectionBackground: "#354462", green: "#23c17b", brightGreen: "#53df9f" };
  terminal.options.cursorStyle = "block";
  const write = (data: string | Uint8Array): Promise<void> => {
    if (disposed) return Promise.resolve();
    return new Promise(resolve => { const done = () => { pending.delete(done); resolve(); }; pending.add(done); terminal.write(data, done); });
  };
  const prompt = new TerminalPrompt(terminal, write, callbacks, onInput);
  const updateFont = () => {
    const next = fontFamily();
    if (disposed || terminal.options.fontFamily === next) return;
    terminal.options.fontFamily = next;
    resize();
    void document.fonts?.ready.then(resize);
  };
  const fontObserver = new MutationObserver(updateFont);
  fontObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["style"] });
  terminal.onData(data => { if (!prompt.input(data)) onInput(data); });
  terminal.onResize(({ cols, rows }) => { prompt.resized(); onResize(cols, rows); });
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
    write,
    setPrompt(label, draft) { prompt.set(label, draft); },
    commitPrompt(label, command) { return prompt.commit(label, command); },
    async restoreInput() {
      // Leaving an inactive alternate buffer with 1049l restores an unrelated saved
      // cursor and overwrites earlier output. Switch only when a pager still owns it.
      const normalCursor = terminal.buffer.active.type === "alternate"
        ? { x: terminal.buffer.normal.cursorX, y: terminal.buffer.normal.cursorY } : undefined;
      if (normalCursor) await write("\x1b[?1047l");
      await write("\x1b[!p\x1b[?25h\x1b[?1l\x1b>\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b(B\x1b[0m\x1b[0 q");
      // xterm copies the alternate cursor into the normal buffer on exit.
      if (normalCursor) await write(`\x1b[${normalCursor.y + 1};${normalCursor.x + 1}H`);
      if (terminal.buffer.active.cursorX) await write("\r\n");
    },
    attach(next) { host = next; next.append(element); observer.disconnect(); observer.observe(next); resize(); },
    detach() { host = undefined; observer.disconnect(); element.remove(); },
    focus() { if (!disposed) terminal.focus(); },
    reset() { if (!disposed) terminal.reset(); },
    clear() { if (!disposed) prompt.clear(); },
    text() {
      if (disposed) return "";
      if (terminal.hasSelection()) return terminal.getSelection();
      const buffer = terminal.buffer.active;
      const lines: string[] = [];
      for (let i = 0; i < buffer.length; i++) lines.push(buffer.getLine(i)?.translateToString(true) ?? "");
      return lines.join("\n").trimEnd();
    },
    dispose() { disposed = true; prompt.dispose(); observer.disconnect(); fontObserver.disconnect(); terminal.dispose(); element.remove(); for (const done of pending) done(); pending.clear(); },
  };
}
