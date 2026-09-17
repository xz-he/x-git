import { open } from "@tauri-apps/plugin-dialog";

export interface DialogAdapter {
  selectDirectory(title: string): Promise<string | null>;
}

const tauriDialogAdapter: DialogAdapter = {
  async selectDirectory(title) {
    const selected = await open({ directory: true, multiple: false, title });
    return typeof selected === "string" ? selected : null;
  },
};

let activeDialogAdapter = tauriDialogAdapter;

export const dialogs: DialogAdapter = {
  selectDirectory: (title) => activeDialogAdapter.selectDirectory(title),
};

export function setDialogAdapterForTests(adapter: DialogAdapter): void {
  activeDialogAdapter = adapter;
}
