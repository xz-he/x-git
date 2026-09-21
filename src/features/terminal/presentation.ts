import { t } from '@/lib/i18n';
import type { ConsoleStatus } from "@/stores/console";

const STATUS_LABELS: Record<ConsoleStatus, string> = {
  get idle() { return t('uiReadyb796f2'); }, get starting() { return t('uiStarting6017dc'); }, get running() { return t('uiRunning594249'); }, get completed() { return t('uiCompletede99b48'); },
  get failed() { return t('uiExecutionFailed9746cf'); }, get cancelled() { return t('uiCancelleda5ffdc'); }, get timedOut() { return t('uiTimedOutf2c3ab'); },
};
export function consoleStatusLabel(status: ConsoleStatus): string { return STATUS_LABELS[status]; }
