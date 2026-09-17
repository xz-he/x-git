import type { TaskBranchKind, TaskBranchPhase } from "@/lib/backend/types";

export function taskBranchPreview(kind: TaskBranchKind, ticketInput: string, slugInput: string, descriptionInput: string) {
  const ticket = ticketInput.trim().toUpperCase();
  const slug = slugInput.trim().toLowerCase().replace(/\s+/g, "-");
  const description = descriptionInput.trim();
  const prefix = kind === "feature" ? "R" : "B";
  const error = !new RegExp(`^${prefix}[0-9]+$`).test(ticket)
    ? `请输入 ${prefix} 开头的完整数字任务单号`
    : !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug)
      ? "英文描述请使用字母、数字和连字符，如 purchase-orders"
      : !description || /[\r\n]/.test(description) ? "请输入单行中文说明" : undefined;
  return { ticket, slug, description, name: `${kind}/${ticket}-${slug}`, title: `${ticket} ${description}`, error };
}

export const taskPhaseLabel: Record<TaskBranchPhase, string> = {
  ready: "等待提交", committing: "提交结果待核对", pendingPick: "已提交，等待移植",
  picking: "移植结果待核对", conflict: "冲突待处理", pendingReturn: "已移植，等待返回",
  completed: "已完成", needsAttention: "需要核对状态",
};
