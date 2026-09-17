import type { TerminalCompletion } from "@/lib/backend/types";

export function commonPrefix(values: string[]): string {
  let prefix = values[0] ?? "";
  for (const value of values.slice(1)) {
    let length = 0;
    while (length < prefix.length && prefix[length] === value[length]) length++;
    prefix = prefix.slice(0, length);
  }
  return prefix;
}
export function replaceCompletion(command: string, completion: TerminalCompletion, value: string, complete: boolean): { command: string; cursor: number } {
  const suffix = command.slice(completion.end);
  const continues = /[\\/=]["']?$/.test(value);
  const insert = value + (complete && !suffix && !continues ? " " : "");
  return { command: command.slice(0, completion.start) + insert + suffix, cursor: completion.start + insert.length };
}
