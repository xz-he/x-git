import type { ConsoleStatus } from "@/stores/console";

const STATUS_LABELS: Record<ConsoleStatus, string> = {
  idle: "就绪", starting: "正在启动", running: "运行中", completed: "已完成",
  failed: "执行失败", cancelled: "已取消", timedOut: "执行超时",
};
export function consoleStatusLabel(status: ConsoleStatus): string { return STATUS_LABELS[status]; }
