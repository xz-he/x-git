<script setup lang="ts">
import { t } from '@/lib/i18n';
import { computed, onMounted, reactive, ref } from "vue";
import AppSelect from "@/components/common/AppSelect.vue";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import { DEFAULT_UI_FONT, FONT_NAME_LIMIT, resolveFontFamilies } from "@/lib/fonts";
import { FALLBACK_FONT_FAMILIES, clearFontPreferences, fontOptions, hasFineStrokes } from "@/lib/fontCatalog";
import { useSettingsStore } from "@/stores/settings";

const settings = useSettingsStore();
const draft = reactive({ fontFamily: settings.settings.fontFamily, codeFontFamily: settings.settings.codeFontFamily });
const fonts = computed(() => resolveFontFamilies(draft));
const saving = ref(false);
const feedback = ref("");
const failed = ref(false);
const installedFonts = ref<string[] | null>(null);
const loadingFonts = ref(false);
const fontLoadError = ref("");
const suggestions = computed(() => fontOptions(installedFonts.value ?? FALLBACK_FONT_FAMILIES));
const fineStrokes = computed(() => hasFineStrokes(draft.fontFamily) || hasFineStrokes(draft.codeFontFamily));
const PREVIEW_SIZES = [11, 13, 16];
async function loadFonts(): Promise<void> {
  if (loadingFonts.value) return;
  loadingFonts.value = true; fontLoadError.value = "";
  try {
    const families = await backendClient.settingsFonts();
    if (!families.length) throw new Error("empty font list");
    installedFonts.value = [...new Set(families)];
  } catch {
    fontLoadError.value = installedFonts.value ? t('uiRefreshFailedThePreviousFontListWasKeptc619f5') : t('uiSystemFontsCouldNotBeReadCommonFontsAreListedYouCanAlsoEnterb60be5');
  } finally { loadingFonts.value = false; }
}
onMounted(loadFonts);
function edited(): void { feedback.value = ""; failed.value = false; }
async function save(reset = false, clear = false): Promise<void> {
  if (saving.value) return;
  saving.value = true; edited();
  try {
    const preferences = clear ? clearFontPreferences(installedFonts.value ?? []) : reset ? { fontFamily: "", codeFontFamily: "" } : draft;
    await settings.save({ ...settings.settings, ...preferences });
    Object.assign(draft, { fontFamily: settings.settings.fontFamily, codeFontFamily: settings.settings.codeFontFamily });
    feedback.value = clear ? t('uiClearFontsAppliedThroughoutTheAppdae5f4') : reset ? t('uiDefaultFontsRestored836dc1') : t('uiFontSettingsSavedAndAppliedThroughoutTheAppb567f8');
  } catch (cause) {
    failed.value = true; feedback.value = normalizeBackendError(cause).message;
  } finally { saving.value = false; }
}
</script>

<template>
  <form class="font-settings" :style="{ fontFamily: DEFAULT_UI_FONT }" :aria-label="t('uiGlobalFontSettings9566c5')" @submit.prevent="save()">
    <div class="section-title"><strong>{{ t('uiFontsb50d4d') }}</strong><span>{{ t('uiAppliesWhenSavedAndIsRememberedNextTime0e81fa') }}</span></div>
    <fieldset :disabled="saving || settings.saving">
      <div class="clarity-preset"><div><strong>{{ t('uiClearScreenFontsa3b3ed') }}</strong><p class="hint">{{ t('uiUsesMicrosoftYaHeiForTheInterfaceAndAMonospaceFontForCodeWhedb0f31') }}</p></div><button type="button" :aria-label="t('uiUseClearFonts6ef945')" @click="save(false, true)">Use Clear Fonts</button></div>
      <div class="font-catalog"><span class="hint" role="status">{{ loadingFonts ? t('uiReadingInstalledFonts04f891') : installedFonts ? t('msgLoadedInstalledFontsd6f5bb', { p0: installedFonts.length }) : t('uiCommonFonts1913ac') }}</span><button type="button" :disabled="loadingFonts" :aria-label="t('uiRefreshFontListf972ed')" @click="loadFonts">{{ loadingFonts ? 'Loading…' : 'Refresh' }}</button></div>
      <p v-if="fontLoadError" class="hint" role="status">{{ fontLoadError }}</p>
      <label><span>{{ t('uiInterfaceFont1a396c') }}</span><AppSelect v-model="draft.fontFamily" :font-family="DEFAULT_UI_FONT" :options="suggestions" editable :disabled="saving || settings.saving" :maxlength="FONT_NAME_LIMIT" :aria-label="t('uiInterfaceFont1a396c')" :placeholder="t('uiSearchOrSelectAFontBlankForDefault275680')" @update:model-value="edited" /></label>
      <label><span>{{ t('uiCodeAndTerminalFontOptional0805dc') }}</span><AppSelect v-model="draft.codeFontFamily" :font-family="DEFAULT_UI_FONT" :options="suggestions" editable :disabled="saving || settings.saving" :maxlength="FONT_NAME_LIMIT" :aria-label="t('uiCodeAndTerminalFont40c20f')" :placeholder="t('uiSearchOrSelectAFontBlankToFollowInterface446f28')" @update:model-value="edited" /></label>
      <p v-if="fineStrokes" class="font-advice" role="status">{{ t('uiThisFontHasThinStrokesAtSmallSizesAndMayLookBlurryOrHardToRe2d9f06') }}</p>
      <p class="hint">{{ t('uiSearchByChineseOrEnglishFontNameOrEnterACustomNameClickRefreeb2445') }}</p>
      <div class="font-preview" :aria-label="t('uiFontPreview01a2b8')">
        <span class="sample-label">{{ t('uiInterfacePreviewSmallBodyLargeebd30a') }}</span><p v-for="size in PREVIEW_SIZES" :key="size" data-font-sample :style="{ fontFamily: fonts.ui, fontSize: size + 'px' }">{{ size }}{{ t('uipxFileStatusAndHistoryAaBb0123456789eb69f1') }}</p>
        <span class="sample-label">{{ t('uiCodeAndTerminalPreviewf31052') }}</span><pre :style="{ fontFamily: fonts.code }">git status --short
const message = "代码审查";
0123456789  {} [] =&gt;</pre>
      </div>
      <p v-if="feedback" class="feedback" :class="{ error: failed }" :role="failed ? 'alert' : 'status'">{{ feedback }}</p>
      <div class="actions"><button type="button" :aria-label="t('uiRestoreDefaultFonts5a781b')" @click="save(true)">{{ t('uiResetToDefaulta19193') }}</button><button type="submit" class="primary" :aria-label="t('uiSaveFontSettings213c40')">{{ saving ? t('uiSaving6644f0') : t('uiSaveFonts655c3a') }}</button></div>
    </fieldset>
  </form>
</template>

<style scoped>
.font-settings { margin-top: 22px; padding-top: 18px; border-top: 1px solid var(--border); }
.section-title { display: grid; gap: 4px; margin-bottom: 14px; }
.clarity-preset { display: flex; align-items: center; gap: 12px; padding: 12px; border: 1px solid var(--primary-border); border-radius: var(--radius-md); background: var(--primary-soft); }
.clarity-preset > div { flex: 1; min-width: 0; }.clarity-preset strong { font-size: 13px; }.clarity-preset .hint { margin-top: 5px; }.clarity-preset button { flex-shrink: 0; color: var(--primary); }
.font-advice { padding: 10px 12px; background: var(--surface-muted); color: var(--text); border-left: 3px solid var(--warning); font-size: 12px; line-height: 1.6; }
.font-catalog { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
.font-catalog button { min-height: 28px; padding: 3px 10px; font-size: 11px; }
.section-title span, .hint, .sample-label { color: var(--text-muted); font-size: 11px; line-height: 1.6; }
fieldset { display: grid; gap: 12px; border: 0; padding: 0; margin: 0; min-width: 0; }
label { display: grid; gap: 6px; } label > span { font-size: 12px; }
input { width: 100%; min-width: 0; height: 34px; border: 1px solid var(--border); border-radius: var(--radius-md); padding: 0 9px; background: var(--surface-panel); color: var(--text); }
input:focus { border-color: var(--primary); outline: 2px solid var(--primary-soft); }
p, pre { margin: 0; } .hint { overflow-wrap: anywhere; }
.font-preview { display: grid; gap: 7px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-muted); overflow: auto; }
.font-preview p, pre { font-size: 13px; line-height: 1.7; } pre { white-space: pre-wrap; }
.feedback { color: var(--success); font-size: 12px; } .feedback.error { color: var(--danger); }
.actions { display: flex; justify-content: flex-end; gap: 8px; }
button { min-height: 34px; padding: 7px 13px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); } .primary { background: var(--primary); color: white; border-color: var(--primary); }
</style>
