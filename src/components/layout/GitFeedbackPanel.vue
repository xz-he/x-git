<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { CheckCircle2, ChevronDown, ChevronUp, CircleAlert, LoaderCircle, Square, X } from "@lucide/vue";
import { cancelFeedback, clearCompletedFeedback, dismissFeedback, feedbackExpanded, gitFeedback } from "@/lib/gitFeedback";

const CLOCK_INTERVAL_MS = 1000;
const MILLISECONDS_PER_SECOND = 1000;
const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | undefined;
const running = computed(() => gitFeedback.value.filter(item => item.status === "running").length);
const failed = computed(() => gitFeedback.value.filter(item => ["failed", "conflicted", "warning"].includes(item.status)).length);
const displayed = computed(() => [...gitFeedback.value.filter(item => item.status === "running"), ...gitFeedback.value.filter(item => item.status !== "running")]);
const labels = { running: "执行中", success: "成功", failed: "失败", cancelled: "已取消", conflicted: "需处理", warning: "已执行 · 刷新失败" };
onMounted(() => { timer = setInterval(() => { if (running.value) now.value = Date.now(); }, CLOCK_INTERVAL_MS); });
onBeforeUnmount(() => { if (timer) clearInterval(timer); });
function duration(start: number, end?: number): string { return `${Math.max(0, Math.floor(((end ?? now.value) - start) / MILLISECONDS_PER_SECOND))} 秒`; }
</script>
<template>
  <Teleport to="body">
    <aside v-if="gitFeedback.length" class="git-feedback" aria-label="Git 操作反馈">
      <header>
        <button class="heading" :aria-expanded="feedbackExpanded" aria-label="展开或收起 Git 操作反馈" @click="feedbackExpanded = !feedbackExpanded">
          <LoaderCircle v-if="running" :size="16" class="feedback-spin" /><CircleAlert v-else-if="failed" :size="16" /><CheckCircle2 v-else :size="16" />
          <strong>操作反馈</strong><span>{{ running ? `${running} 项进行中` : failed ? `${failed} 项需关注` : '操作已完成' }}</span>
          <ChevronDown v-if="feedbackExpanded" :size="15" /><ChevronUp v-else :size="15" />
        </button>
        <button v-if="feedbackExpanded" class="clear" :disabled="gitFeedback.every(item => item.status === 'running')" @click="clearCompletedFeedback">清除已结束</button>
      </header>
      <div v-if="feedbackExpanded" class="feedback-list">
        <article v-for="item in displayed" :key="item.id" :class="['feedback-item', item.status]" :data-feedback-id="item.id">
          <div class="item-title"><strong>{{ item.title }}</strong><span class="badge">{{ labels[item.status] }}</span><span class="duration">{{ duration(item.startedAt, item.finishedAt) }}</span>
            <button v-if="item.status !== 'running'" :aria-label="`关闭${item.title}反馈`" @click="dismissFeedback(item.id)"><X :size="14" /></button>
          </div>
          <p class="context" :title="item.root">{{ item.root }}</p><p v-if="item.target" class="target">{{ item.target }}</p>
          <p class="message" :role="item.status === 'failed' || item.status === 'conflicted' ? 'alert' : 'status'">{{ item.message }}</p>
          <template v-if="item.status === 'running'">
            <div class="progress-label"><span>{{ item.cancelling ? '正在停止' : item.phase ?? '等待 Git 返回进度' }}</span><span v-if="item.percent !== undefined">当前阶段 {{ item.percent }}%</span></div>
            <div class="progress-track" role="progressbar" :aria-label="`${item.title}进度${item.phase ? '：' + item.phase : ''}`" aria-valuemin="0" aria-valuemax="100" :aria-valuenow="item.percent" :aria-valuetext="item.percent === undefined ? '执行中，暂无百分比' : `当前阶段 ${item.percent}%`"><span :class="{ indeterminate: item.percent === undefined }" :style="item.percent === undefined ? undefined : { width: item.percent + '%' }" /></div>
            <p v-if="item.percent === 100" class="phase-note">当前阶段已完成，等待 Git 确认最终结果…</p>
            <button v-if="item.canCancel" class="cancel" :disabled="item.cancelling" :aria-label="`停止${item.title}`" @click="cancelFeedback(item.id)"><Square :size="12" />{{ item.cancelling ? '正在停止…' : '停止操作' }}</button>
          </template>
          <details v-if="item.diagnostics"><summary>查看诊断信息</summary><pre>{{ item.diagnostics }}</pre></details>
        </article>
      </div>
    </aside>
  </Teleport>
</template>
<style scoped>
.git-feedback { position: fixed; z-index: 120; right: 18px; bottom: 18px; width: min(420px, calc(100vw - 36px)); border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface-panel); box-shadow: var(--shadow-lg); color: var(--text); }
header { display: flex; align-items: center; gap: 6px; padding: 9px 10px; }
button { display: inline-flex; align-items: center; justify-content: center; gap: 6px; border-radius: var(--radius-sm); background: transparent; }
button:hover:not(:disabled) { background: var(--surface-muted); }
.heading { flex: 1; justify-content: flex-start; min-width: 0; padding: 4px; }
.heading strong { font-size: 13px; }.heading span, .clear { color: var(--text-muted); font-size: 11px; }.clear { padding: 5px; }
.feedback-list { max-height: min(460px, calc(100vh - 130px)); overflow-y: auto; border-top: 1px solid var(--border); }
.feedback-item { padding: 12px 14px; border-bottom: 1px solid var(--border); border-left: 3px solid var(--primary); }.feedback-item:last-child { border-bottom: 0; }
.feedback-item.success { border-left-color: #68ac87; }.feedback-item.failed, .feedback-item.conflicted { border-left-color: var(--danger); }.feedback-item.cancelled { border-left-color: var(--text-muted); }
.feedback-item.warning { border-left-color: #c69a45; }.warning .badge { color: #a77c2c; }
.item-title { display: flex; align-items: center; gap: 8px; }.item-title strong { font-size: 13px; }.item-title button { padding: 3px; }.duration { margin-left: auto; white-space: nowrap; font-size: 11px; color: var(--text-muted); }
.badge { font-size: 11px; color: var(--primary); }.success .badge { color: #509c77; }.failed .badge, .conflicted .badge { color: var(--danger); }.cancelled .badge { color: var(--text-muted); }
p { margin: 6px 0; line-height: 1.5; overflow-wrap: anywhere; }.context { font-size: 11px; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.target { font-size: 12px; }.message { font-size: 12px; }
.progress-label { display: flex; justify-content: space-between; gap: 10px; font-size: 11px; color: var(--text-muted); margin: 10px 0 5px; }
.progress-track { height: 5px; overflow: hidden; border-radius: 4px; background: var(--surface-muted); }.progress-track > span { display: block; height: 100%; background: var(--primary); transition: width .2s; }.progress-track > .indeterminate { width: 35%; animation: feedback-progress 1.4s ease-in-out infinite; }
.cancel { margin-top: 9px; padding: 5px 8px; border: 1px solid var(--border); color: var(--text-muted); font-size: 11px; }.phase-note, summary { color: var(--text-muted); font-size: 11px; }details { margin-top: 8px; }summary { cursor: pointer; }pre { max-height: 130px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font-family: var(--font-code); font-size: 11px; user-select: text; }
.feedback-spin { animation: feedback-spin .9s linear infinite; }@keyframes feedback-spin { to { transform: rotate(360deg); } }@keyframes feedback-progress { from { transform: translateX(-100%); } to { transform: translateX(390%); } }
</style>
