import type { AiIssueSeverity, AiReviewIssue, ReviewContext, ReviewSource } from "@/lib/backend/types";

export const severityRank: Record<AiIssueSeverity, number> = { P0: 0, P1: 1, P2: 2, P3: 3, critical: 0, warning: 1, suggestion: 3 };
export const severityLabel: Record<AiIssueSeverity, string> = { P0: "P0 严重", P1: "P1 高", P2: "P2 中", P3: "P3 低 / 需确认", critical: "严重", warning: "警告", suggestion: "建议" };
export const relationLabel: Record<string, string> = { introduced: "本次引入", triggered: "本次触发", exposed: "本次暴露", amplified: "本次放大", unclear: "关系待确认" };
export function locationLabel(issue: AiReviewIssue): string {
  return issue.startLine ? `${issue.path} 第 ${issue.startLine} 行` : issue.path;
}
export function reviewScopeLabel(source?: ReviewSource): string {
  return source?.kind === "commit" ? `历史提交 ${source.revision}` : source?.kind === "stagedFiles" ? `所选暂存文件（${source.paths.length}）` : "已暂存变更";
}
export function reviewReport(issues: AiReviewIssue[], source: ReviewSource | undefined, context: ReviewContext | undefined, summary: string, warnings: string[], uncovered: string[], reviewedFiles: string[] = [], skippedBinaryFiles: string[] = []): string {
  const originalSource = context?.source ?? source;
  const frozenSource = originalSource?.kind === "commit" && context?.resolvedCommit ? { kind: "commit" as const, revision: context.resolvedCommit } : originalSource;
  const lines = ["代码审查", reviewScopeLabel(frozenSource), summary, `已审文件（${reviewedFiles.length}）：${reviewedFiles.length ? reviewedFiles.join(", ") : "无"}`];
  if (frozenSource?.kind === "stagedFiles") lines.push("所选路径：", ...frozenSource.paths.map(path => `- ${path}`));
  if (context) lines.push(`规则：${context.skill.directory}/SKILL.md ${context.skill.version ?? ""}`, `规则指纹：${context.skill.fingerprint}`, `规则文件：${context.skill.files.join(", ")}`, `比较基准：${context.baseCommit ?? "空树"}`);
  for (const evidence of context?.evidenceSources ?? []) lines.push(`取证来源：${evidence.path}:${evidence.startLine}-${evidence.endLine} @ ${evidence.revision}`);
  if (skippedBinaryFiles.length) lines.push(`已跳过二进制文件：${skippedBinaryFiles.join(", ")}`);
  for (const issue of issues) {
    lines.push("", `${severityLabel[issue.severity]} · ${locationLabel(issue)} · ${issue.title ?? issue.reason}`, `影响：${issue.impact ?? issue.reason}`, `建议：${issue.suggestedFix}`);
    if (issue.evidence) lines.push(`证据：${issue.evidence}`);
    if (issue.confidence != null) lines.push(`置信度：${issue.confidence}/10`);
    if (issue.changeRelation) lines.push(`变更关系：${relationLabel[issue.changeRelation] ?? issue.changeRelation}`);
    for (const missing of issue.contextMissing ?? []) lines.push(`缺失上下文：${missing}`);
    for (const evidence of issue.evidenceSources ?? []) lines.push(`来源：${evidence.path}:${evidence.startLine}-${evidence.endLine} @ ${evidence.revision}`);
  }
  lines.push("", `未覆盖范围：${uncovered.length ? uncovered.join("；") : "无额外报告"}`);
  for (const warning of warnings) lines.push(`提示：${warning}`);
  if (context?.excludedFiles.length) lines.push(`排除文件：${context.excludedFiles.join(", ")}`);
  return lines.join("\n");
}
