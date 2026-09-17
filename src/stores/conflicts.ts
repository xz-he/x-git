import { computed, ref, watch } from "vue";
import { defineStore } from "pinia";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import type { AbortAction, BackendError, ConflictDetail, ConflictMutationResult, ConflictResolution, ConflictSnapshot } from "@/lib/backend/types";
import { useRepositoryStore } from "./repository";
import { useChangesStore } from "./changes";
import { useOperationStore } from "./operation";
import { useRefsStore } from "./refs";
import { useHistoryStore } from "./history";
import { useUiStore } from "./ui";

export interface ConflictDraft { detail: ConflictDetail; resolution: ConflictResolution; dirty: boolean }
interface ConfirmationIdentity { rootPath: string; generation: number; branch: string | null }
type Confirmation = ConfirmationIdentity & (
  { kind: "save"; path: string; token: string; resolution: ConflictResolution } |
  { kind: "continue"; token: string } | { kind: "abort"; action: AbortAction; token: string } | { kind: "reload"; path: string }
);
export const hasConflictMarkers = (text: string): boolean => /^(?:<{7}|={7}|>{7}|\|{7})/m.test(text);

export const useConflictsStore = defineStore("conflicts", () => {
  const snapshot = ref<ConflictSnapshot>();
  const loadedRootPath = ref<string>();
  const generation = ref(0);
  const selectedPath = ref<string>();
  const drafts = ref<Record<string, ConflictDraft>>(Object.create(null));
  const loading = ref(false);
  const detailLoading = ref(false);
  const submitting = ref(false);
  const error = ref<BackendError>();
  const detailError = ref<BackendError>();
  const draftRevision = ref(0);
  const recoveryPath = ref<string | null>(null);
  const confirmation = ref<Confirmation>();
  let listVersion = 0;
  let detailVersion = 0;
  let pending: Promise<void> | undefined;
  const current = computed<ConflictDraft | undefined>(() => selectedPath.value ? drafts.value[selectedPath.value] : undefined);
  const hasDirtyDrafts = computed<boolean>(() => Object.values(drafts.value).some(draft => draft.dirty));
  const busy = computed<boolean>((): boolean => submitting.value || useRepositoryStore().navigationBusy);
  const canContinue = computed<boolean>(() => !!snapshot.value?.continueAction && !snapshot.value.files.length && !hasDirtyDrafts.value && !busy.value);
  const canSave = computed<boolean>(() => {
    const draft = current.value;
    if (!draft?.dirty || busy.value) return false;
    if (draft.resolution.kind === "text") return draft.detail.editable;
    if (draft.resolution.kind === "ours") return draft.detail.canChooseOurs;
    if (draft.resolution.kind === "theirs") return draft.detail.canChooseTheirs;
    return draft.detail.canDelete;
  });
  function identity(): ConfirmationIdentity | undefined {
    const repositories = useRepositoryStore();
    if (!loadedRootPath.value || repositories.snapshot?.rootPath !== loadedRootPath.value || repositories.generation !== generation.value) return;
    return { rootPath: loadedRootPath.value, generation: generation.value, branch: repositories.snapshot.currentBranch };
  }
  function accepts(id: ConfirmationIdentity, checkBranch = true): boolean {
    const currentId = identity();
    return !!currentId && currentId.rootPath === id.rootPath && currentId.generation === id.generation && (!checkBranch || currentId.branch === id.branch);
  }
  function resetForRepository(rootPath: string, nextGeneration: number): void {
    draftRevision.value++;
    listVersion++; detailVersion++; pending = undefined;
    loadedRootPath.value = rootPath; generation.value = nextGeneration;
    snapshot.value = undefined; selectedPath.value = undefined; drafts.value = Object.create(null);
    loading.value = false; detailLoading.value = false; error.value = undefined; detailError.value = undefined;
    confirmation.value = undefined; recoveryPath.value = null;
  }
  function applySnapshot(next: ConflictSnapshot): void {
    if (confirmation.value && snapshot.value?.operationToken !== next.operationToken && !submitting.value) confirmation.value = undefined;
    snapshot.value = next;
    useOperationStore().state = next.operationState;
  }
  function ensureLoaded(rootPath: string, nextGeneration: number): Promise<void> {
    if (loadedRootPath.value !== rootPath || generation.value !== nextGeneration) resetForRepository(rootPath, nextGeneration);
    if (snapshot.value) return Promise.resolve();
    if (pending) return pending;
    pending = refresh();
    return pending;
  }
  async function refresh(): Promise<void> {
    const id = identity(); if (!id || submitting.value) return;
    const version = ++listVersion; loading.value = true; error.value = undefined;
    try {
      const next = await backendClient.conflictsSnapshot(id.rootPath);
      if (accepts(id) && version === listVersion) applySnapshot(next);
    } catch (cause) {
      if (accepts(id) && version === listVersion) error.value = normalizeBackendError(cause);
    } finally {
      if (version === listVersion) { loading.value = false; pending = undefined; }
    }
  }
  async function selectFile(path: string): Promise<void> {
    const id = identity(); if (!id || submitting.value) return;
    if (selectedPath.value !== path) { draftRevision.value++; confirmation.value = undefined; }
    selectedPath.value = path; detailError.value = undefined;
    const version = ++detailVersion;
    detailLoading.value = false;
    if (drafts.value[path]) return;
    detailLoading.value = true;
    try {
      const detail = await backendClient.conflictsDetail(id.rootPath, path);
      if (!accepts(id) || version !== detailVersion) return;
      drafts.value[path] = { detail, resolution: { kind: "text", text: detail.working.text ?? "", acknowledgeMarkers: false }, dirty: false };
    } catch (cause) {
      if (accepts(id) && version === detailVersion) detailError.value = normalizeBackendError(cause);
    } finally { if (version === detailVersion) detailLoading.value = false; }
  }
  async function refreshCleanDetails(): Promise<void> {
    const id = identity(); if (!id || submitting.value) return;
    const path = selectedPath.value;
    for (const [key, draft] of Object.entries(drafts.value)) {
      if (!draft.dirty && key !== path) delete drafts.value[key];
    }
    if (!path || !current.value || current.value.dirty) return;
    const version = ++detailVersion, revision = draftRevision.value;
    if (!snapshot.value?.files.some(file => file.path === path)) {
      delete drafts.value[path]; selectedPath.value = undefined; draftRevision.value++; return;
    }
    try {
      const detail = await backendClient.conflictsDetail(id.rootPath, path);
      if (!accepts(id) || version !== detailVersion || revision !== draftRevision.value || selectedPath.value !== path || current.value?.dirty) return;
      detailError.value = undefined;
      if (current.value?.detail.token !== detail.token) {
        drafts.value[path] = { detail, resolution: { kind: "text", text: detail.working.text ?? "", acknowledgeMarkers: false }, dirty: false };
        draftRevision.value++;
      }
    } catch (cause) {
      if (accepts(id) && version === detailVersion) detailError.value = normalizeBackendError(cause);
    }
  }
  function edit(text: string): void {
    if (!current.value?.detail.editable || submitting.value) return;
    draftRevision.value++;
    current.value.resolution = { kind: "text", text, acknowledgeMarkers: false }; current.value.dirty = true;
  }
  function choose(kind: "ours" | "theirs" | "delete"): void {
    const draft = current.value; if (!draft || submitting.value) return;
    if ((kind === "ours" && !draft.detail.canChooseOurs) || (kind === "theirs" && !draft.detail.canChooseTheirs) || (kind === "delete" && !draft.detail.canDelete)) return;
    draftRevision.value++;
    draft.resolution = { kind }; draft.dirty = true;
  }
  function requestSave(): void {
    const id = identity(); const draft = current.value;
    if (!id || !draft || !canSave.value) return;
    confirmation.value = { ...id, kind: "save", path: draft.detail.path, token: draft.detail.token, resolution: { ...draft.resolution } };
    error.value = undefined;
  }
  function requestReload(): void {
    const id = identity(); if (!id || !selectedPath.value || busy.value) return;
    if (current.value?.dirty) confirmation.value = { ...id, kind: "reload", path: selectedPath.value };
    else void reload(selectedPath.value);
  }
  async function reload(path: string): Promise<void> {
    const id = identity(); if (!id) return;
    draftRevision.value++;
    delete drafts.value[path]; await refresh();
    if (accepts(id) && selectedPath.value === path) await selectFile(path);
  }
  function requestContinue(): void {
    const id = identity(); if (id && canContinue.value && snapshot.value) confirmation.value = { ...id, kind: "continue", token: snapshot.value.operationToken };
  }
  function requestAbort(): void {
    const id = identity(); if (id && !busy.value && snapshot.value?.operationState.abortAction) confirmation.value = { ...id, kind: "abort", action: snapshot.value.operationState.abortAction, token: snapshot.value.operationToken };
  }
  function cancelConfirmation(): void { if (!submitting.value) confirmation.value = undefined; }
  function applyResult(result: ConflictMutationResult): void {
    useChangesStore().applyWorkspace(result.workspace);
    useOperationStore().state = result.operationState;
    if (result.refs) useRefsStore().applySnapshot(result.refs, result.workspace.repository.rootPath);
    applySnapshot(result.conflicts);
    if (result.recoveryPath) recoveryPath.value = result.recoveryPath;
    error.value = result.error ?? undefined;
  }
  async function confirm(): Promise<void> {
    const intent = confirmation.value;
    if (!intent || busy.value) return;
    if (!accepts(intent)) { confirmation.value = undefined; return; }
    if (intent.kind === "reload") { confirmation.value = undefined; await reload(intent.path); return; }
    if (intent.kind === "continue" && (!canContinue.value || snapshot.value?.operationToken !== intent.token)) return;
    if (intent.kind === "abort" && snapshot.value?.operationToken !== intent.token) return;
    if (intent.kind === "save" && intent.resolution.kind === "text" && hasConflictMarkers(intent.resolution.text) && !intent.resolution.acknowledgeMarkers) return;
    submitting.value = true; error.value = undefined;
    // Responses begun before this mutation must not overwrite its authoritative result.
    listVersion++; detailVersion++; pending = undefined; loading.value = false; detailLoading.value = false;
    try {
      if (intent.kind === "abort") {
        await useRefsStore().abort(intent.action);
        if (!accepts(intent, false)) return;
        const next = await backendClient.conflictsSnapshot(intent.rootPath);
        if (!accepts(intent, false)) return;
        drafts.value = Object.create(null); selectedPath.value = undefined; applySnapshot(next); confirmation.value = undefined;
      } else {
        const result = intent.kind === "save"
          ? await backendClient.conflictsResolve(intent.rootPath, { relativePath: intent.path, token: intent.token, resolution: intent.resolution })
          : await backendClient.conflictsContinue(intent.rootPath, intent.token);
        if (!accepts(intent)) return;
        applyResult(result);
        if (intent.kind === "continue") useHistoryStore().resetForRepository(intent.rootPath, intent.generation);
        if (!result.error) {
          confirmation.value = undefined;
          if (intent.kind === "save" && result.resolved) { delete drafts.value[intent.path]; selectedPath.value = undefined; }
        }
      }
    } catch (cause) { if (accepts(intent, intent.kind !== "abort")) error.value = normalizeBackendError(cause); }
    finally { submitting.value = false; }
  }
  watch(() => [useRepositoryStore().snapshot?.rootPath, useRepositoryStore().generation], () => {
    if (confirmation.value && !accepts(confirmation.value)) confirmation.value = undefined;
  }, { flush: "sync" });
  watch(() => [useUiStore().activeView, useUiStore().homeVisible, useRepositoryStore().snapshot?.currentBranch], () => {
    if (!submitting.value) confirmation.value = undefined;
  }, { flush: "sync" });
  return { snapshot, loadedRootPath, generation, selectedPath, drafts, draftRevision, current, loading, detailLoading, submitting, busy, error, detailError, recoveryPath, confirmation, hasDirtyDrafts, canSave, canContinue, resetForRepository, ensureLoaded, refresh, refreshCleanDetails, selectFile, edit, choose, requestSave, requestReload, requestContinue, requestAbort, cancelConfirmation, confirm };
});
