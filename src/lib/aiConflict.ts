import type { ConflictDetail, RepositoryOperationKind } from "./backend/types";

export const MAX_AI_CONFLICT_VERSION_BYTES = 2 * 1024 * 1024;
export function conflictSuggestionUnavailable(detail?: ConflictDetail): string | undefined {
  if (!detail) return "请先选择一个冲突文件。";
  if (!detail.editable) return detail.unsupportedReason ?? "此文件不支持 AI 文本解决建议。";
  for (const version of [detail.base, detail.ours, detail.theirs, detail.working]) {
    if (version.kind === "missing" && !version.exists) continue;
    if (version.kind !== "text" || version.text === null) return "AI 建议只支持可完整读取的 UTF-8 文本及一致的换行符。";
    if (version.byteLength > MAX_AI_CONFLICT_VERSION_BYTES) return "任一版本超过 2 MiB，暂不支持 AI 冲突建议。";
  }
  return undefined;
}
export function conflictSideLabels(kind?: RepositoryOperationKind): { ours: string; theirs: string } {
  return kind === "rebase"
    ? { ours: "变基目标 + 已重放提交", theirs: "正在重放的提交" }
    : { ours: "当前版本", theirs: "传入版本" };
}
