<script setup lang="ts">
import { computed, reactive, ref, useId } from "vue";
import { normalizeBackendError } from "@/lib/backend/errors";
import { FONT_NAME_LIMIT, resolveFontFamilies } from "@/lib/fonts";
import { useSettingsStore } from "@/stores/settings";

const settings = useSettingsStore();
const draft = reactive({ fontFamily: settings.settings.fontFamily, codeFontFamily: settings.settings.codeFontFamily });
const fonts = computed(() => resolveFontFamilies(draft));
const saving = ref(false);
const feedback = ref("");
const failed = ref(false);
const id = useId();
const suggestions = ["Microsoft YaHei", "Microsoft YaHei UI", "Segoe UI", "SimSun", "SimHei", "KaiTi", "Arial", "Consolas", "Cascadia Code", "Cascadia Mono", "JetBrains Mono", "Source Han Sans SC", "Noto Sans CJK SC"];
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
      <label><span>全局字体</span><input v-model="draft.fontFamily" :list="id" :maxlength="FONT_NAME_LIMIT" aria-label="全局字体" placeholder="默认字体（留空）" autocomplete="off" spellcheck="false" @input="edited" /></label>
      <label><span>代码与终端字体（可选）</span><input v-model="draft.codeFontFamily" :list="id" :maxlength="FONT_NAME_LIMIT" aria-label="代码与终端字体" placeholder="跟随全局字体" autocomplete="off" spellcheck="false" @input="edited" /></label>
      <datalist :id="id"><option v-for="font in suggestions" :key="font" :value="font" /></datalist>
      <p class="hint">可选择常用字体或输入本机已安装的字体名称；未安装时使用系统回退字体。代码建议使用 Consolas 等等宽字体。两项留空恢复应用默认字体。</p>
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
