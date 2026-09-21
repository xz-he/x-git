<script setup lang="ts" generic="T extends string | number | null | undefined">
import { t } from '@/lib/i18n';
import { computed, nextTick, onBeforeUnmount, ref, useAttrs, useId, watch, type CSSProperties } from "vue";
import { Check, ChevronDown, Search } from "@lucide/vue";

defineOptions({ inheritAttrs: false });
const props = withDefaults(defineProps<{
  modelValue: T; options: readonly { value: T; label: string; disabled?: boolean }[];
  disabled?: boolean; editable?: boolean; placeholder?: string;
  fontFamily?: string;
}>(), { placeholder: undefined });
const emit = defineEmits<{ "update:modelValue": [value: T]; change: [value: T] }>();
const attrs = useAttrs();
const id = `select-${useId()}`;
const trigger = ref<HTMLInputElement | HTMLButtonElement>();
const panel = ref<HTMLElement>();
const search = ref<HTMLInputElement>();
const open = ref(false);
const query = ref("");
const active = ref(-1);
const placement = ref<Record<string, string>>({});
const selected = computed(() => props.options.find(option => option.value === props.modelValue));
const placeholder = computed(() => props.placeholder ?? t('uiSelectAnOption382f4b'));
const filtered = computed(() => props.options.filter(option => option.label.toLocaleLowerCase().includes(query.value.trim().toLocaleLowerCase())));
const searchable = computed(() => !props.editable && props.options.length > 8);
const activeId = computed(() => open.value && active.value >= 0 ? `${id}-${active.value}` : undefined);
let observer: ResizeObserver | undefined;
let ancestorObserver: MutationObserver | undefined;
let frame = 0;
const blocked = () => props.disabled || trigger.value?.matches(":disabled") || !!trigger.value?.closest("[inert]");

function position() {
  if (!open.value || !trigger.value) return;
  const box = trigger.value.getBoundingClientRect();
  const viewport = window.visualViewport;
  const leftEdge = viewport?.offsetLeft ?? 0, topEdge = viewport?.offsetTop ?? 0;
  const width = viewport?.width ?? window.innerWidth, height = viewport?.height ?? window.innerHeight;
  if (box.bottom < topEdge || box.top > topEdge + height) { close(); return; }
  const margin = 8, gap = 5, preferredHeight = 320;
  const below = topEdge + height - box.bottom - margin - gap, above = box.top - topEdge - margin - gap;
  const upwards = below < Math.min(preferredHeight, panel.value?.scrollHeight ?? preferredHeight) && above > below;
  const available = Math.max(0, upwards ? above : below);
  const panelHeight = Math.min(preferredHeight, available, panel.value?.scrollHeight ?? preferredHeight);
  const panelWidth = Math.min(box.width, width - margin * 2);
  placement.value = {
    left: `${Math.max(leftEdge + margin, Math.min(box.left, leftEdge + width - margin - panelWidth))}px`,
    top: `${upwards ? box.top - gap - panelHeight : box.bottom + gap}px`,
    width: `${panelWidth}px`, maxHeight: `${Math.min(preferredHeight, available)}px`,
  };
}
function schedulePosition() { cancelAnimationFrame(frame); frame = requestAnimationFrame(position); }
function scrollActive() { void nextTick(() => document.getElementById(`${id}-${active.value}`)?.scrollIntoView?.({ block: "nearest" })); }
function close(restoreFocus = false) {
  open.value = false;
  if (restoreFocus) trigger.value?.focus();
}
async function show(filter = "") {
  if (blocked()) return;
  query.value = filter;
  active.value = filtered.value.findIndex(option => !option.disabled && option.value === props.modelValue);
  // Free-text input is never implicitly replaced by the first suggestion.
  if (active.value < 0 && !props.editable) active.value = filtered.value.findIndex(option => !option.disabled);
  open.value = true;
  await nextTick();
  if (!open.value) return;
  position(); scrollActive();
  await nextTick(); position();
  if (searchable.value) search.value?.focus();
}
function choose(index: number) {
  const option = filtered.value[index];
  if (blocked() || !option || option.disabled) return;
  emit("update:modelValue", option.value); emit("change", option.value);
  close(true);
}
function move(direction: number) {
  const enabled = filtered.value.flatMap((option, index) => option.disabled ? [] : [index]);
  if (!enabled.length) { active.value = -1; return; }
  const index = enabled.indexOf(active.value);
  active.value = enabled[(index < 0 ? direction > 0 ? 0 : enabled.length - 1 : (index + direction + enabled.length) % enabled.length)]!;
  scrollActive();
}
function keydown(event: KeyboardEvent) {
  if (event.isComposing || blocked()) return;
  if (event.key === "Escape" && open.value) { event.preventDefault(); event.stopPropagation(); close(true); }
  else if (event.key === "Tab") { if (open.value) close(!props.editable); }
  else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    if (!open.value) void show(); else move(event.key === "ArrowDown" ? 1 : -1);
  } else if (event.key === "Enter" || (!props.editable && event.target === trigger.value && event.key === " ")) {
    if (open.value) { event.preventDefault(); if (active.value < 0) close(); else choose(active.value); }
    else if (!props.editable) { event.preventDefault(); void show(); }
  } else if (open.value && !props.editable && (event.key === "Home" || event.key === "End")) {
    event.preventDefault(); active.value = -1; move(event.key === "Home" ? 1 : -1);
  } else if (!props.editable && event.target === trigger.value && event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
    event.preventDefault(); void show(event.key);
  }
}
function edit(event: Event) {
  if (blocked()) return;
  const value = (event.target as HTMLInputElement).value;
  emit("update:modelValue", value as T);
  query.value = value; active.value = -1;
  if (!open.value) void show(value);
}
function outside(event: Event) {
  const target = event.target as Node;
  if (!trigger.value?.parentElement?.contains(target) && !panel.value?.contains(target)) close();
}
function onScroll(event: Event) { if (!(event.target instanceof Node) || !panel.value?.contains(event.target)) schedulePosition(); }
function searchChanged() { active.value = filtered.value.findIndex(option => !option.disabled); schedulePosition(); }
watch(query, schedulePosition);
watch(() => props.options, () => { active.value = -1; schedulePosition(); }, { deep: true });
watch(() => props.disabled, disabled => { if (disabled) close(); });
watch(open, value => {
  if (!value) {
    document.removeEventListener("pointerdown", outside, true); document.removeEventListener("focusin", outside);
    window.removeEventListener("scroll", onScroll, true); window.removeEventListener("resize", schedulePosition);
    window.visualViewport?.removeEventListener("resize", schedulePosition);
    observer?.disconnect(); ancestorObserver?.disconnect(); cancelAnimationFrame(frame);
    return;
  }
  document.addEventListener("pointerdown", outside, true); document.addEventListener("focusin", outside);
  window.addEventListener("scroll", onScroll, true); window.addEventListener("resize", schedulePosition);
  window.visualViewport?.addEventListener("resize", schedulePosition);
  if (typeof ResizeObserver !== "undefined" && trigger.value) { observer = new ResizeObserver(schedulePosition); observer.observe(trigger.value); }
  ancestorObserver = new MutationObserver(() => { if (blocked()) close(); });
  for (let node = trigger.value?.parentElement; node; node = node.parentElement) ancestorObserver.observe(node, { attributes: true, attributeFilter: ["disabled", "inert"] });
});
onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", outside, true); document.removeEventListener("focusin", outside);
  window.removeEventListener("scroll", onScroll, true); window.removeEventListener("resize", schedulePosition);
  window.visualViewport?.removeEventListener("resize", schedulePosition);
  observer?.disconnect(); ancestorObserver?.disconnect(); cancelAnimationFrame(frame);
});
</script>

<template>
  <div class="app-select" :class="attrs.class" :style="[attrs.style as CSSProperties, fontFamily ? { fontFamily } : undefined]">
    <input v-if="editable" ref="trigger" v-bind="{ ...attrs, class: undefined, style: undefined }" class="select-trigger select-input" :value="modelValue ?? ''" :placeholder="placeholder" :disabled="disabled" role="combobox" aria-autocomplete="list" aria-haspopup="listbox" :aria-expanded="open" :aria-controls="open ? id : undefined" :aria-activedescendant="activeId" autocomplete="off" spellcheck="false" @input="edit" @click="open ? close() : show()" @keydown="keydown" />
    <button v-else ref="trigger" v-bind="{ ...attrs, class: undefined, style: undefined }" type="button" class="select-trigger" :class="{ 'is-placeholder': !selected }" :disabled="disabled" role="combobox" aria-haspopup="listbox" :aria-expanded="open" :aria-controls="open ? id : undefined" :aria-activedescendant="activeId" :title="selected?.label" @click="open ? close() : show()" @keydown="keydown"><span>{{ selected?.label ?? placeholder }}</span></button>
    <ChevronDown class="select-chevron" :class="{ expanded: open }" :size="15" aria-hidden="true" />
    <Teleport to="body">
      <div v-if="open" ref="panel" class="select-popover" :style="{ ...placement, fontFamily }" @keydown="keydown">
        <div v-if="searchable" class="select-search"><Search :size="14" aria-hidden="true" /><input ref="search" v-model="query" :aria-label="t('msgSearchc32673', { p0: attrs['aria-label'] ?? t('uiOptions221ee0') })" :aria-controls="id" :aria-activedescendant="activeId" role="combobox" aria-expanded="true" aria-autocomplete="list" :placeholder="t('uiSearchOptionsb55e53')" @input="searchChanged" /></div>
        <div :id="id" class="select-options" role="listbox" :aria-label="attrs['aria-label'] as string | undefined">
          <div v-for="(option, index) in filtered" :id="`${id}-${index}`" :key="String(option.value)" role="option" :data-value="String(option.value)" :aria-selected="option.value === modelValue" :aria-disabled="option.disabled || undefined" class="select-option" :class="{ active: index === active, selected: option.value === modelValue, disabled: option.disabled }" :title="option.label" @pointermove="!option.disabled && (active = index)" @pointerdown.prevent @click="choose(index)"><span>{{ option.label }}</span><Check v-if="option.value === modelValue" :size="15" aria-hidden="true" /></div>
          <div v-if="!filtered.length" class="select-empty" role="status">{{ editable ? t('uiNoMatchesKeepTypingToEnterAValue34f7c2') : options.length ? t('uiNoMatchingOptionsb8859e') : t('uiNoOptionsAvailable187f0d') }}</div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.app-select { position: relative; width: 100%; min-width: 0; }
.app-select .select-trigger { display: flex; align-items: center; width: 100%; min-width: 0; height: 36px; padding: 0 34px 0 11px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); color: var(--text); font: inherit; font-size: 12px; text-align: left; cursor: pointer; transition: border-color .15s, box-shadow .15s; }
.select-trigger span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.app-select .select-input { cursor: text; }
.app-select .select-trigger:hover:not(:disabled) { border-color: var(--primary-border); }.app-select .select-trigger:focus-visible, .app-select .select-trigger[aria-expanded=true] { outline: none; border-color: var(--primary); box-shadow: 0 0 0 3px var(--primary-soft); }
.app-select .is-placeholder { color: var(--text-muted); }.select-trigger:disabled { cursor: not-allowed; opacity: .5; }
.select-chevron { position: absolute; right: 11px; top: 50%; transform: translateY(-50%); color: var(--text-muted); pointer-events: none; transition: transform .15s; }.select-chevron.expanded { transform: translateY(-50%) rotate(180deg); }
.select-popover { position: fixed; z-index: 2000; display: flex; flex-direction: column; padding: 5px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); color: var(--text); box-shadow: var(--shadow-window); font-family: var(--font-ui); font-size: 12px; overflow: hidden; }
.select-options { overflow: auto; min-height: 0; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--border) transparent; }
.select-option { display: flex; align-items: center; gap: 10px; min-height: 34px; padding: 8px 10px; border-radius: 4px; cursor: pointer; line-height: 1.5; }.select-option span { flex: 1; min-width: 0; overflow-wrap: anywhere; }.select-option svg { flex-shrink: 0; }
.select-option.selected { color: var(--primary); background: var(--primary-soft); }.select-option.active:not(.disabled) { background: var(--primary-soft); outline: 1px solid var(--primary-border); outline-offset: -1px; }.select-option.disabled { cursor: not-allowed; opacity: .45; }
.select-search { display: flex; align-items: center; gap: 7px; padding: 5px 7px 9px; margin-bottom: 4px; border-bottom: 1px solid var(--border); color: var(--text-muted); }.select-search input { width: 100%; min-width: 0; height: 28px; padding: 0; border: 0; outline: none; background: transparent; color: var(--text); font: inherit; }
.select-empty { padding: 16px 10px; color: var(--text-muted); text-align: center; }
</style>
