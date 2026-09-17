import type { IMarker, Terminal } from "@xterm/xterm";
import type { TerminalCompletion } from "@/lib/backend/types";
import { commonPrefix, replaceCompletion } from "./completion";

export interface PromptCallbacks {
  change(command: string): void;
  submit(): void;
  complete(command: string, cursor: number): Promise<TerminalCompletion>;
  history(): string[];
  blocked(): boolean;
}
const MAX_INPUT = 8192;
const PROMPT_COLOR = "\x1b[38;2;35;193;123m";
const safeText = (value: string) => value.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "").replace(/[\x00-\x1f\x7f-\x9f]/g, " ");

/** Local command editing only while no PTY owns the keyboard. */
export class TerminalPrompt {
  private active = false;
  private label = "";
  private draft = "";
  private cursor = 0;
  private marker?: IMarker;
  private revision = 0;
  private historyIndex = -1;
  private savedDraft = "";
  private pending: Promise<void> = Promise.resolve();
  private disposed = false;
  private localInput?: string[];

  constructor(private terminal: Terminal, private write: (text: string) => Promise<void>, private callbacks: PromptCallbacks, private forward: (data: string) => void) {}

  private finishLocalInput(): void {
    if (this.disposed) return;
    this.draft = ""; this.cursor = 0; this.active = !this.callbacks.blocked(); this.edited();
    const queued = this.localInput ?? []; this.localInput = undefined;
    for (const data of queued) if (!this.input(data)) this.forward(data);
  }

  set(label: string | null, draft: string): void {
    if (label === null) { this.active = false; this.revision++; return; }
    const next = safeText(draft).slice(0, MAX_INPUT);
    if (this.active && this.label === label && this.draft === next) return;
    this.active = true; this.label = safeText(label); this.draft = next; this.cursor = next.length; this.historyIndex = -1; this.revision++;
    this.render();
  }
  private enqueue(action: () => Promise<void>): Promise<void> {
    this.pending = this.pending.then(async () => { if (!this.disposed) await action(); });
    return this.pending;
  }
  private async erase(): Promise<void> {
    const buffer = this.terminal.buffer.active;
    if (this.marker && !this.marker.isDisposed && this.marker.line >= buffer.baseY) {
      const up = buffer.baseY + buffer.cursorY - this.marker.line;
      await this.write(`${up > 0 ? `\x1b[${up}A` : ""}\r\x1b[J`);
    } else {
      if (buffer.cursorX) await this.write("\r\n");
      this.marker?.dispose();
      this.marker = this.terminal.registerMarker();
    }
  }
  private async draw(): Promise<void> {
    if (this.disposed) return;
    await this.erase();
    if (this.disposed) return;
    const text = this.label + this.draft;
    await this.write(`\x1b[?2004h${PROMPT_COLOR}${this.label}\x1b[0m${this.draft}`);
    if (this.disposed) return;
    // Use rendered cells, so CJK, emoji, and wrapped lines position the cursor correctly.
    if (this.cursor < this.draft.length && this.marker && !this.marker.isDisposed) {
      const buffer = this.terminal.buffer.active;
      const target = this.label.length + this.cursor;
      let consumed = 0;
      for (let row = this.marker.line; row <= buffer.baseY + buffer.cursorY; row++) {
        const line = buffer.getLine(row);
        for (let col = 0; col < this.terminal.cols; col++) {
          const cell = line?.getCell(col);
          if (!cell || cell.getWidth() === 0) continue;
          if (consumed >= target) {
            const up = buffer.baseY + buffer.cursorY - row;
            await this.write(`${up > 0 ? `\x1b[${up}A` : ""}\x1b[${col + 1}G`);
            return;
          }
          const chars = cell.getChars();
          // Empty padding before a wide wrapped character is not part of the input.
          consumed += chars.length;
          if (consumed >= text.length) return;
        }
      }
    }
  }
  private render(): void { void this.enqueue(async () => { if (this.active) await this.draw(); }); }
  private edited(): void { this.revision++; this.callbacks.change(this.draft); this.render(); }
  async commit(label: string, command: string): Promise<void> {
    this.active = false; this.revision++; this.historyIndex = -1;
    await this.enqueue(async () => {
      this.label = safeText(label); this.draft = safeText(command); this.cursor = this.draft.length;
      await this.draw(); await this.write("\x1b[?2004l\r\n"); this.marker?.dispose(); this.marker = undefined;
    });
  }
  clear(): void {
    void this.enqueue(async () => {
      this.marker?.dispose(); this.marker = undefined;
      this.terminal.clear(); await this.write("\x1b[2J\x1b[H");
      if (this.active) await this.draw();
    });
  }
  resized(): void { if (this.active) this.render(); }
  private recall(direction: number): void {
    const history = this.callbacks.history();
    if (!history.length) return;
    if (this.historyIndex < 0 && direction > 0) this.savedDraft = this.draft;
    this.historyIndex = Math.max(-1, Math.min(history.length - 1, this.historyIndex + direction));
    this.draft = this.historyIndex < 0 ? this.savedDraft : history[this.historyIndex]!;
    this.cursor = this.draft.length; this.edited();
  }
  private async complete(): Promise<void> {
    const version = ++this.revision, command = this.draft, cursor = this.cursor;
    try {
      const result = await this.callbacks.complete(command, cursor);
      if (!this.active || version !== this.revision || this.callbacks.blocked()) return;
      if (result.start < 0 || result.end < result.start || result.end > command.length || !result.items.length) return;
      const single = result.items.length === 1;
      const value = single ? result.items[0]!.value : commonPrefix(result.items.map(item => item.value));
      if (single || value.length > cursor - result.start) {
        const replaced = replaceCompletion(command, result, value, single);
        this.draft = safeText(replaced.command).slice(0, MAX_INPUT); this.cursor = Math.min(replaced.cursor, this.draft.length); this.edited();
      }
      const completedRevision = this.revision;
      if (!single) await this.enqueue(async () => {
        if (!this.active || completedRevision !== this.revision || this.callbacks.blocked()) return;
        this.cursor = this.draft.length;
        await this.draw(); await this.write("\r\n");
        await this.write(result.items.map(item => `${safeText(item.label)}  ${safeText(item.description)}`).join("\r\n") + "\r\n");
        if (result.hasMore) await this.write("还有更多匹配，请继续输入缩小范围。\r\n");
        this.marker?.dispose(); this.marker = undefined; await this.draw();
      });
    } catch { /* Completion is optional; command editing stays available. */ }
  }
  input(data: string): boolean {
    if (this.localInput) { this.localInput.push(data); return true; }
    if (!this.active) return false;
    if (this.callbacks.blocked()) return true;
    if (data === "\r") {
      if (this.draft.trim()) this.callbacks.submit();
      else { this.localInput = []; void this.commit(this.label, "").then(() => this.finishLocalInput()); }
      return true;
    }
    if (data === "\t") { void this.complete(); return true; }
    if (data === "\x1b[A") { this.recall(1); return true; }
    if (data === "\x1b[B") { this.recall(-1); return true; }
    if (data === "\x03") {
      this.localInput = [];
      void this.commit(this.label, this.draft + "^C").then(() => this.finishLocalInput());
      return true;
    }
    if (data === "\x0c") { this.clear(); return true; }
    const before = Array.from(this.draft.slice(0, this.cursor));
    const after = Array.from(this.draft.slice(this.cursor));
    if (data === "\x7f" || data === "\b") { before.pop(); this.draft = before.join("") + after.join(""); this.cursor = before.join("").length; }
    else if (data === "\x1b[3~" || data === "\x04") { after.shift(); this.draft = before.join("") + after.join(""); }
    else if (data === "\x1b[D") this.cursor -= before.at(-1)?.length ?? 0;
    else if (data === "\x1b[C") this.cursor += after[0]?.length ?? 0;
    else if (["\x01", "\x1b[H", "\x1b[1~"].includes(data)) this.cursor = 0;
    else if (["\x05", "\x1b[F", "\x1b[4~"].includes(data)) this.cursor = this.draft.length;
    else if (data === "\x15") { this.draft = after.join(""); this.cursor = 0; }
    else if (data === "\x0b") this.draft = before.join("");
    else if (data === "\x17") { const prefix = before.join("").replace(/\s*\S+\s*$/, ""); this.draft = prefix + after.join(""); this.cursor = prefix.length; }
    else if (data === "\x1b" || (/^\x1b/.test(data) && !data.startsWith("\x1b[200~"))) return true;
    else if (data.length === 1 && data.charCodeAt(0) < 32) return true;
    else {
      // Multiline paste becomes a single editable command; it never auto-executes.
      const inserted = safeText(data.replace(/^\x1b\[200~/, "").replace(/\x1b\[201~$/, "")).slice(0, MAX_INPUT - this.draft.length);
      this.draft = before.join("") + inserted + after.join(""); this.cursor += inserted.length;
    }
    this.historyIndex = -1; this.edited(); return true;
  }
  dispose(): void { this.disposed = true; this.active = false; this.revision++; this.marker?.dispose(); }
}
