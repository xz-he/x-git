import { t } from '@/lib/i18n';
import type { AiIssueSeverity, AiReviewIssue, ReviewContext, ReviewSource } from "@/lib/backend/types";

export const severityRank: Record<AiIssueSeverity, number> = { P0: 0, P1: 1, P2: 2, P3: 3, critical: 0, warning: 1, suggestion: 3 };
export const severityLabel: Record<AiIssueSeverity, string> = { get P0() { return t('uiP0Critical92392e'); }, get P1() { return t('uiP1High33f55c'); }, get P2() { return t('uiP2Mediumb6d61f'); }, get P3() { return t('uiP3LowNeedsConfirmationa630de'); }, get critical() { return t('uiCritical81ffc6'); }, get warning() { return t('uiWarning5521e3'); }, get suggestion() { return t('uiSuggestionc5134e'); } };
export const relationLabel: Record<string, string> = { get introduced() { return t('uiIntroduced8fca00'); }, get triggered() { return t('uiTriggered7f7b7c'); }, get exposed() { return t('uiExposed0f2a58'); }, get amplified() { return t('uiAmplified8b1509'); }, get unclear() { return t('uiRelationUnconfirmedc5f1ea'); } };
export function locationLabel(issue: AiReviewIssue): string {
  return issue.startLine ? t('msgLineefaad9', { p0: issue.path, p1: issue.startLine }) : issue.path;
}
export function reviewScopeLabel(source?: ReviewSource): string {
  return source?.kind === "commit" ? t('msgHistoricalCommit060926', { p0: source.revision }) : source?.kind === "stagedFiles" ? t('msgSelectedStagedFiles0e02c7', { p0: source.paths.length }) : t('uiStagedChanges2fe2df');
}
export function reviewReport(issues: AiReviewIssue[], source: ReviewSource | undefined, context: ReviewContext | undefined, summary: string, warnings: string[], uncovered: string[], reviewedFiles: string[] = [], skippedBinaryFiles: string[] = [], markdown?: string): string {
  const originalSource = context?.source ?? source;
  const frozenSource = originalSource?.kind === "commit" && context?.resolvedCommit ? { kind: "commit" as const, revision: context.resolvedCommit } : originalSource;
  const lines = [t('uiCodeReviewda6d88'), reviewScopeLabel(frozenSource), summary, t('msgReviewedFiles1da2e2', { p0: reviewedFiles.length, p1: reviewedFiles.length ? reviewedFiles.join(", ") : t('uiNone720777') })];
  if (frozenSource?.kind === "stagedFiles") lines.push(t('uiSelectedPaths84fce2'), ...frozenSource.paths.map(path => `- ${path}`));
  if (context) lines.push(t('msgRulesSKILLMd7d2073', { p0: context.skill.directory, p1: context.skill.version ?? "" }), t('msgRulesFingerprint664029', { p0: context.skill.fingerprint }), t('msgRuleFiles3e96dd', { p0: context.skill.files.join(", ") }), t('msgComparedWith90ec7f', { p0: context.baseCommit ?? t('uiEmptyTree65442d') }));
  for (const evidence of context?.evidenceSources ?? []) lines.push(t('msgEvidenceSourcea4afb1', { p0: evidence.path, p1: evidence.startLine, p2: evidence.endLine, p3: evidence.revision }));
  if (skippedBinaryFiles.length) lines.push(t('msgSkippedBinaryFilese535ac', { p0: skippedBinaryFiles.join(", ") }));
  if (markdown) lines.push("", markdown);
  for (const issue of issues) {
    lines.push("", `${severityLabel[issue.severity]} · ${locationLabel(issue)} · ${issue.title ?? issue.reason}`, t('msgImpact639a81', { p0: issue.impact ?? issue.reason }), t('msgSuggestion42a0d9', { p0: issue.suggestedFix }));
    if (issue.evidence) lines.push(t('msgEvidencef0ebac', { p0: issue.evidence }));
    if (issue.confidence != null) lines.push(t('msgConfidence10de3afa', { p0: issue.confidence }));
    if (issue.changeRelation) lines.push(t('msgRelationToChangec7ec07', { p0: relationLabel[issue.changeRelation] ?? issue.changeRelation }));
    for (const missing of issue.contextMissing ?? []) lines.push(t('msgMissingContextec8708', { p0: missing }));
    for (const evidence of issue.evidenceSources ?? []) lines.push(t('msgSource10b80f', { p0: evidence.path, p1: evidence.startLine, p2: evidence.endLine, p3: evidence.revision }));
  }
  lines.push("", t('msgUncoveredAreas753cf7', { p0: uncovered.length ? uncovered.join("；") : t('uiNothingElseReported13643c') }));
  for (const warning of warnings) lines.push(t('msgNote8b200d', { p0: warning }));
  if (context?.excludedFiles.length) lines.push(t('msgExcludedFiles103fd9', { p0: context.excludedFiles.join(", ") }));
  return lines.join("\n");
}
