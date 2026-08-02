import type {
  WritingAgentAnswer,
  WritingDocumentRecord,
  WritingIssue,
  WritingPattern,
} from "./writingViewModel.ts";

export type WritingReviewIssueState = {
  status: "open" | "editing" | "modified" | "ignored" | "baseline";
  showDeeperHint: boolean;
  showReference: boolean;
};

export type WritingReviewTargetState = {
  targetKey: string;
  issues: WritingIssue[];
  patterns: WritingPattern[];
  answers: WritingAgentAnswer[];
  issueStates: Record<string, WritingReviewIssueState>;
  activeIssueId: string | null;
};

export function mergeWritingConversationAnswers(
  current: WritingAgentAnswer[],
  incoming: WritingAgentAnswer[],
) {
  const answersById = new Map(
    current.map((answer) => [answer.id, answer]),
  );
  for (const answer of incoming) {
    answersById.set(answer.id, answer);
  }
  return [...answersById.values()].sort(
    (first, second) =>
      first.createdAtUnixMs - second.createdAtUnixMs ||
      first.id - second.id,
  );
}

function createIssueStates(
  issues: WritingIssue[],
  readOnlyBaseline: boolean,
) {
  return Object.fromEntries(
    issues.map((issue) => [
      issue.id,
      {
        status: readOnlyBaseline ? "baseline" : "open",
        showDeeperHint: false,
        showReference: false,
      } satisfies WritingReviewIssueState,
    ]),
  ) as Record<string, WritingReviewIssueState>;
}

export function createWritingReviewTargetState(
  document: WritingDocumentRecord,
  versionId?: number,
): WritingReviewTargetState {
  const version =
    versionId === undefined
      ? undefined
      : document.versions.find((candidate) => candidate.id === versionId);
  if (versionId !== undefined && !version) {
    throw new Error("写作完成版本不存在，无法重建检查状态。");
  }
  const analysis = document.activeAnalysis ?? document.baselineAnalysis;
  const issues = version ? version.issues : analysis?.issues ?? [];
  const patterns = version ? version.patterns : analysis?.patterns ?? [];
  const latestCompletedVersion = document.versions.reduce<
    WritingDocumentRecord["versions"][number] | undefined
  >(
    (latest, candidate) =>
      !latest || candidate.sourceRevision > latest.sourceRevision
        ? candidate
        : latest,
    undefined,
  );
  const draftSessionStartRevision =
    latestCompletedVersion?.sourceRevision ?? -1;
  const answers = document.answers.filter((answer) =>
    version
      ? answer.versionId === version.id
      : answer.versionId === undefined &&
        answer.documentRevision > draftSessionStartRevision &&
        answer.documentRevision <= document.revision,
  );
  return {
    targetKey: version
      ? `${document.id}:version:${version.id}`
      : `${document.id}:draft:${latestCompletedVersion?.id ?? "initial"}`,
    issues,
    patterns,
    answers,
    issueStates: createIssueStates(issues, version !== undefined),
    activeIssueId: version ? null : issues[0]?.id ?? null,
  };
}

export class LatestWritingRequestSequence {
  private sequence = 0;

  begin() {
    this.sequence += 1;
    return this.sequence;
  }

  invalidate() {
    this.sequence += 1;
  }

  isCurrent(sequence: number) {
    return sequence === this.sequence;
  }

  requireCurrent(sequence: number) {
    if (!this.isCurrent(sequence)) {
      throw new Error("已有较新的问答请求，旧结果已过期。");
    }
  }
}
