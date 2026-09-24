import { t } from '@/lib/i18n';
import { computed, ref, watch } from "vue";
import { defineStore } from "pinia";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type { BackendError, FileDirectoryPage, FileMutationResult, FileOperationIntent, PreparedFileOperation, RepositoryFileEntry, RepositoryFilePreview } from "@/lib/backend/types";
import { useRepositoryStore } from "./repository";
import { useChangesStore } from "./changes";
import { useConflictsStore } from "./conflicts";
import { useOperationStore } from "./operation";
import { useUiStore } from "./ui";

interface DirectoryState { page?: FileDirectoryPage; loading: boolean; error?: BackendError }
interface Identity { rootPath: string; generation: number; branch: string | null }
interface FileConfirmation extends Identity { intent: FileOperationIntent; selectedPath?: string; prepared?: PreparedFileOperation }
export const parentDirectory = (path: string): string => path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
export function fileNameError(name: string): string | undefined {
  if (!name || /[\\/:*?"<>|\u0000-\u001f]/.test(name) || /[. ]$/.test(name) || name === "." || name === "..") return t('uiEnterASingleValidNameWithoutPathSeparatorsInvalidCharactersT3a2863');
  if (/^(?:\.git|git~1)$/i.test(name) || /^(?:con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)/i.test(name)) return t('uiThisNameIsReservedByTheSystemOrGit5c5533');
}

export const useFilesStore = defineStore("files", () => {
  const loadedRootPath = ref<string>(); const generation = ref(0);
  const directories = ref<Record<string, DirectoryState>>(Object.create(null));
  const expanded = ref(new Set<string>([""]));
  const selectedEntry = ref<RepositoryFileEntry>(); const preview = ref<RepositoryFilePreview>();
  const previewLoading = ref(false); const previewError = ref<BackendError>();
  const submitting = ref(false); const preparing = ref(false); const refreshing = ref(false);
  const error = ref<BackendError>(); const confirmation = ref<FileConfirmation>(); const result = ref<FileMutationResult>();
  let epoch = 0; let previewVersion = 0; let prepareVersion = 0;
  const directoryVersions = new Map<string, number>();
  // Browsing can proceed while the terminal owns the write lock; writes cannot.
  const busy = computed<boolean>((): boolean => submitting.value || refreshing.value || useRepositoryStore().viewNavigationBusy);
  const canMutate = computed<boolean>(() => !!identity() && !busy.value && !useRepositoryStore().navigationBusy && !useOperationStore().isBlocked && !useConflictsStore().hasDirtyDrafts);
  const currentDirectory = computed(() => selectedEntry.value?.kind === "directory" ? selectedEntry.value.relativePath : parentDirectory(selectedEntry.value?.relativePath ?? ""));
  const nameError = computed(() => {
    const intent = confirmation.value?.intent;
    return !intent || intent.kind === "delete" ? undefined : fileNameError(intent.kind === "rename" ? intent.newName : intent.name);
  });
  function identity(): Identity | undefined {
    const repo = useRepositoryStore();
    if (!loadedRootPath.value || repo.snapshot?.rootPath !== loadedRootPath.value || repo.generation !== generation.value) return;
    return { rootPath: loadedRootPath.value, generation: generation.value, branch: repo.snapshot.currentBranch };
  }
  function accepts(id: Identity): boolean {
    const current = identity(); return !!current && current.rootPath === id.rootPath && current.generation === id.generation && current.branch === id.branch;
  }
  function invalidateReads(): void {
    epoch++; previewVersion++; directoryVersions.clear(); previewLoading.value = false;
    for (const dir of Object.values(directories.value)) dir.loading = false;
  }
  function cancelConfirmation(): void {
    if (submitting.value) return;
    prepareVersion++; preparing.value = false; confirmation.value = undefined;
  }
  function invalidate(): void {
    invalidateReads(); cancelConfirmation(); directories.value = Object.create(null); preview.value = undefined; previewError.value = undefined;
  }
  function resetForRepository(root: string, nextGeneration: number): void {
    invalidate(); loadedRootPath.value = root; generation.value = nextGeneration;
    expanded.value = new Set([""]); selectedEntry.value = undefined; result.value = undefined; error.value = undefined;
  }
  async function ensureLoaded(root: string, nextGeneration: number, acceptsRefresh: () => boolean = () => true): Promise<void> {
    if (loadedRootPath.value !== root) resetForRepository(root, nextGeneration);
    else if (generation.value !== nextGeneration) { invalidate(); generation.value = nextGeneration; await refresh(); return; }
    if (!directories.value[""]?.page && !directories.value[""]?.loading) await loadDirectory("", false, acceptsRefresh);
  }
  async function loadDirectory(dir: string, append = false, acceptsRefresh: () => boolean = () => true): Promise<void> {
    const id = identity(); if (!id || submitting.value) return;
    const previous = directories.value[dir]?.page; const cursor = append ? previous?.nextCursor ?? undefined : undefined;
    if (append && (!cursor || directories.value[dir]?.loading)) return;
    const version = (directoryVersions.get(dir) ?? 0) + 1; directoryVersions.set(dir, version); const startedEpoch = epoch;
    directories.value[dir] = { page: append ? previous : undefined, loading: true };
    const valid = () => accepts(id) && epoch === startedEpoch && directoryVersions.get(dir) === version;
    try {
      const next = await backendClient.filesList(id.rootPath, dir, cursor);
      if (!valid() || !acceptsRefresh()) return;
      if (append && previous && next.token !== previous.token) { await loadDirectory(dir); return; }
      directories.value[dir] = { loading: false, page: { ...next, entries: append && previous ? [...previous.entries, ...next.entries] : next.entries } };
    } catch (cause) {
      if (!valid() || !acceptsRefresh()) return;
      const failure = normalizeBackendError(cause);
      if (append && failure.code === "staleFileOperation") { await loadDirectory(dir); return; }
      directories.value[dir] = { page: previous, loading: false, error: failure };
    } finally { if (valid()) directories.value[dir]!.loading = false; }
  }
  function loadMore(dir: string): Promise<void> { return loadDirectory(dir, true); }
  async function toggleDirectory(dir: string): Promise<void> {
    if (busy.value) return;
    if (expanded.value.has(dir)) { expanded.value.delete(dir); return; }
    expanded.value.add(dir); if (!directories.value[dir]?.page) await loadDirectory(dir);
  }
  async function selectEntry(entry?: RepositoryFileEntry): Promise<void> {
    const id = identity(); if (!id || busy.value) return;
    if (selectedEntry.value?.relativePath !== entry?.relativePath) cancelConfirmation();
    selectedEntry.value = entry; await readPreview(entry, id);
  }
  async function readPreview(entry: RepositoryFileEntry | undefined, id: Identity): Promise<void> {
    preview.value = undefined; previewError.value = undefined;
    const version = ++previewVersion; previewLoading.value = false;
    if (entry?.kind !== "file") return;
    previewLoading.value = true;
    try {
      const next = await backendClient.filesPreview(id.rootPath, entry.relativePath);
      if (accepts(id) && version === previewVersion) preview.value = next;
    } catch (cause) { if (accepts(id) && version === previewVersion) previewError.value = normalizeBackendError(cause); }
    finally { if (version === previewVersion) previewLoading.value = false; }
  }
  async function refresh(): Promise<void> {
    if (submitting.value) return;
    const id = identity(); if (!id) return;
    const selected = selectedEntry.value; invalidate();
    const startedEpoch = epoch; const selectionVersion = previewVersion;
    const valid = () => accepts(id) && epoch === startedEpoch && previewVersion === selectionVersion;
    await Promise.all([...expanded.value].map(dir => loadDirectory(dir)));
    if (valid() && selected) {
      const parent = parentDirectory(selected.relativePath);
      if (!directories.value[parent]?.page) await loadDirectory(parent);
      const findSelected = () => directories.value[parent]?.page?.entries.find(item => item.relativePath === selected.relativePath);
      // A selected item may sort beyond page one. Walk the bounded cursor chain,
      // stopping on errors or a changed snapshot rather than declaring it absent.
      const seenCursors = new Set<string>();
      while (valid() && !findSelected() && directories.value[parent]?.page?.nextCursor && !directories.value[parent]?.error) {
        const cursor = directories.value[parent]!.page!.nextCursor!;
        if (seenCursors.has(cursor)) break;
        seenCursors.add(cursor); await loadMore(parent);
      }
      if (valid() && !directories.value[parent]?.error && !directories.value[parent]?.page?.nextCursor) {
        selectedEntry.value = findSelected(); await readPreview(selectedEntry.value, id);
      } else if (valid() && findSelected()) {
        selectedEntry.value = findSelected(); await readPreview(selectedEntry.value, id);
      }
    }
  }
  function requestOperation(intent: FileOperationIntent): void {
    const id = identity(); if (!id || !canMutate.value || useUiStore().activeView !== "files" || useUiStore().homeVisible) return;
    cancelConfirmation(); error.value = undefined;
    confirmation.value = { ...id, intent: { ...intent }, selectedPath: selectedEntry.value?.relativePath };
  }
  function changeName(name: string): void {
    const current = confirmation.value; if (!current || submitting.value || current.intent.kind === "delete") return;
    prepareVersion++; preparing.value = false; current.prepared = undefined; error.value = undefined;
    current.intent = current.intent.kind === "rename" ? { ...current.intent, newName: name } : { ...current.intent, name };
  }
  function acceptsConfirmation(current: FileConfirmation): boolean {
    return accepts(current) && current.selectedPath === selectedEntry.value?.relativePath && useUiStore().activeView === "files" && !useUiStore().homeVisible;
  }
  async function prepare(): Promise<void> {
    const current = confirmation.value; if (!current || !canMutate.value || nameError.value || preparing.value) return;
    if (!acceptsConfirmation(current)) { cancelConfirmation(); return; }
    const version = ++prepareVersion; const intent = { ...current.intent }; preparing.value = true; error.value = undefined; current.prepared = undefined;
    try {
      const prepared = await backendClient.filesPrepare(current.rootPath, intent);
      if (version === prepareVersion && confirmation.value === current && acceptsConfirmation(current)) current.prepared = prepared;
    } catch (cause) { if (version === prepareVersion && acceptsConfirmation(current)) error.value = normalizeBackendError(cause); }
    finally { if (version === prepareVersion) preparing.value = false; }
  }
  async function confirm(): Promise<void> {
    const current = confirmation.value; if (!current?.prepared || !canMutate.value || preparing.value) return;
    if (!acceptsConfirmation(current)) { cancelConfirmation(); return; }
    const prepared = current.prepared; submitting.value = true; error.value = undefined; invalidateReads();
    let appliedResult: FileMutationResult | undefined;
    try {
      const next = await backendClient.filesExecute(current.rootPath, { intent: { ...current.intent }, token: prepared.token });
      if (!accepts(current)) return;
      result.value = next; error.value = next.error ?? undefined;
      if (next.workspace) useChangesStore().applyWorkspace(next.workspace);
      if (next.operationState) useOperationStore().state = next.operationState;
      if (next.applied) {
        appliedResult = next;
        confirmation.value = undefined; directories.value = Object.create(null); preview.value = undefined;
        const kind = prepared.entryKind; selectedEntry.value = next.selectedPath ? { name: next.selectedPath.split("/").pop()!, relativePath: next.selectedPath, kind, byteLength: null, reason: null, gitStatus: null } : undefined;
        // Drop descendant expansion after rename/delete; never reload the old subtree.
        if (prepared.sourcePath) for (const dir of expanded.value) if (dir === prepared.sourcePath || dir.startsWith(prepared.sourcePath + "/")) expanded.value.delete(dir);
      } else current.prepared = undefined;
    } catch (cause) { if (accepts(current)) { error.value = normalizeBackendError(cause); current.prepared = undefined; } }
    finally { submitting.value = false; }
    if (accepts(current) && appliedResult) {
      for (const dir of appliedResult.affectedDirectories) expanded.value.add(dir);
      await refresh();
    }
  }
  async function retryRefresh(): Promise<void> {
    if (busy.value || useRepositoryStore().navigationBusy || !identity()) return;
    const id = identity()!; refreshing.value = true;
    try {
      await useRepositoryStore().refresh();
      if (accepts(id)) { error.value = undefined; if (result.value) result.value = { ...result.value, error: null }; }
    } catch (cause) { if (accepts(id)) error.value = normalizeBackendError(cause); }
    finally { refreshing.value = false; }
    // Repository refresh already reloads the visible file module.
  }
  watch(() => [useRepositoryStore().snapshot?.rootPath, useRepositoryStore().generation, useRepositoryStore().snapshot?.currentBranch, useRepositoryStore().snapshot?.headShortHash, useUiStore().activeView, useUiStore().homeVisible], () => {
    if (!submitting.value) cancelConfirmation();
  }, { flush: "sync" });
  watch(() => useRepositoryStore().operation.kind, kind => { if (kind !== "idle" && !submitting.value) invalidate(); }, { flush: "sync" });
  return { loadedRootPath, generation, directories, expanded, selectedEntry, currentDirectory, preview, previewLoading, previewError, submitting, preparing, refreshing, busy, canMutate, error, confirmation, result, nameError, invalidate, resetForRepository, ensureLoaded, loadDirectory, loadMore, toggleDirectory, selectEntry, refresh, requestOperation, changeName, prepare, confirm, cancelConfirmation, retryRefresh };
});
