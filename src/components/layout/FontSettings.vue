<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import AppSelect from "@/components/common/AppSelect.vue";
import { backendClient } from "@/lib/backend/client";
import { normalizeBackendError } from "@/lib/backend/errors";
import { FONT_NAME_LIMIT, resolveFontFamilies } from "@/lib/fonts";
import { FALLBACK_FONT_FAMILIES, fontOptions } from "@/lib/fontCatalog";
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
async function loadFonts(): Promise<void> {
  if (loadingFonts.value) return;
  loadingFonts.value = true; fontLoadError.value = "";
  try {
    const families = await backendClient.settingsFonts();
    if (!families.length) throw new Error("empty font list");
    installedFonts.value = [...new Set(families)];
  } catch {
    fontLoadError.value = installedFonts.value ? "刷新失败，已保留上次读取的字体。" : "暂时无法读取系统字体，已显示常用候选字体；仍可手动输入。";
  } finally { loadingFonts.value = false; }
}
onMounted(loadFonts);
function edited(): void { feedback.value = ""; failed.value = false; }
async function save(reset = false): Promise<void> {
  if (saving.value) return;
  saving.value = true; edited();
  try {
    await settings.save({ ...settings.settings, ...(reset ? { fontFamily: "", codeFontFamily: "" } : draft) });
    Object.assign(draft, { fontFamily: settings.settings.fontFamily, codeFontFamily: settings.settings.codeFontFamily });
    feedback.value = reset ? "已恢复默认字体。" : "字体设置已保存，全局生效。";
  } catch (cause) {
    failed.value = true; feedback.value = normalizeBackendError(cause).message;
  } finally { saving.value = false; }
}
</script>

<template>
  <form class="font-settings" aria-label="全局字体设置" @submit.prevent="save()">
    <div class="section-title"><strong>字体</strong><span>保存后立即应用，下次启动自动恢复</span></div>
    <fieldset :disabled="saving || settings.saving">
      <div class="font-catalog"><span class="hint" role="status">{{ loadingFonts ? '正在读取本机字体…' : installedFonts ? `已读取 ${installedFonts.length} 种本机字体` : '常用候选字体' }}</span><button type="button" :disabled="loadingFonts" aria-label="刷新字体列表" @click="loadFonts">{{ loadingFonts ? 'Loading…' : 'Refresh' }}</button></div>
      <p v-if="fontLoadError" class="hint" role="status">{{ fontLoadError }}</p>
      <label><span>全局字体</span><AppSelect v-model="draft.fontFamily" :options="suggestions" editable :disabled="saving || settings.saving" :maxlength="FONT_NAME_LIMIT" aria-label="全局字体" placeholder="输入搜索或选择字体（留空使用默认）" @update:model-value="edited" /></label>
      <label><span>代码与终端字体（可选）</span><AppSelect v-model="draft.codeFontFamily" :options="suggestions" editable :disabled="saving || settings.saving" :maxlength="FONT_NAME_LIMIT" aria-label="代码与终端字体" placeholder="输入搜索或选择字体（留空跟随全局）" @update:model-value="edited" /></label>
      <p class="hint">输入中文或英文字体名称筛选，也可填写自定义字体名称。安装新字体后点击 Refresh；未安装的字体将使用系统回退字体。代码建议使用 Consolas 等等宽字体。两项留空恢复应用默认字体。</p>
      <div class="font-preview" aria-label="字体预览">
        <span class="sample-label">界面预览</span><p :style="{ fontFamily: fonts.ui }">你好，HQ Git · 分支与提交记录 AaBb 0123456789</p>
        <span class="sample-label">代码与终端预览</span><pre :style="{ fontFamily: fonts.code }">git status --short
const message = "代码审查";
0123456789  {} [] =&gt;</pre>
      </div>
      <p v-if="feedback" class="feedback" :class="{ error: failed }" :role="failed ? 'alert' : 'status'">{{ feedback }}</p>
      <div class="actions"><button type="button" aria-label="恢复默认字体" @click="save(true)">恢复默认</button><button type="submit" class="primary" aria-label="保存字体设置">{{ saving ? '保存中…' : '保存字体' }}</button></div>
    </fieldset>
  </form>
</template>

<style scoped>
.font-settings { margin-top: 22px; padding-top: 18px; border-top: 1px solid var(--border); }
.section-title { display: grid; gap: 4px; margin-bottom: 14px; }
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
