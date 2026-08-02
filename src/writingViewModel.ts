export type WritingMode = "draft" | "review" | "compare" | "completed" | "library";

export type WritingDocumentStatus = "draft" | "completed";

export type WritingSnapshot = {
  title: string;
  paragraphs: string[];
};

export type WritingDocumentSummary = {
  id: number;
  revision: number;
  createdAtUnixMs: number;
  updatedAtUnixMs: number;
  lastOpenedAtUnixMs?: number;
  draftUpdatedAtUnixMs?: number;
  completedAtUnixMs?: number;
  draftSnapshot?: WritingSnapshot;
  completedSnapshot?: WritingSnapshot;
};

export type WritingIssue = {
  id: string;
  category: string;
  source: string;
  targetText: string;
  explanation: string;
  hint: string;
  deeperHint: string;
  reference: string;
};

export type WritingPattern = {
  id: string;
  title: string;
  description: string;
};

export type WritingAnalysis = {
  id: number;
  documentId: number;
  documentRevision: number;
  round: number;
  issues: WritingIssue[];
  patterns: WritingPattern[];
  createdAtUnixMs: number;
};

export type WritingVersion = {
  id: number;
  documentId: number;
  ordinal: number;
  sourceRevision: number;
  analysisRevision?: number;
  comparisonBaselineRevision?: number;
  snapshot: WritingSnapshot;
  comparisonBaseline: WritingSnapshot;
  issues: WritingIssue[];
  patterns: WritingPattern[];
  completedAtUnixMs: number;
};

export type WritingAnswerMap = {
  core: string;
  questions: string[];
  phrases: string[];
  starters: string[];
};

export type WritingQuestionScope = "document" | "paragraph" | "selection";

export type WritingAgentAnswer = {
  id: number;
  documentId: number;
  documentRevision: number;
  versionId?: number;
  parentAnswerId?: number;
  question: string;
  scope: WritingQuestionScope;
  scopeLabel: string;
  selectionText?: string;
  title: string;
  copy: string;
  map?: WritingAnswerMap;
  createdAtUnixMs: number;
};

export type WritingDocumentRecord = WritingDocumentSummary & {
  comparisonBaseline: WritingSnapshot;
  comparisonBaselineRevision?: number;
  versions: WritingVersion[];
  activeAnalysis?: WritingAnalysis;
  baselineAnalysis?: WritingAnalysis;
  answers: WritingAgentAnswer[];
};

export const emptyWritingSnapshot: WritingSnapshot = {
  title: "",
  paragraphs: [""],
};

export function normalizeWritingText(value: string) {
  return value.replace(/\s+/g, " ").trim();
}

export function cloneWritingSnapshot(snapshot: WritingSnapshot): WritingSnapshot {
  return {
    title: snapshot.title,
    paragraphs: [...snapshot.paragraphs],
  };
}

export function writingSnapshotsEqual(
  first: WritingSnapshot,
  second: WritingSnapshot,
) {
  return (
    first.title === second.title &&
    first.paragraphs.length === second.paragraphs.length &&
    first.paragraphs.every(
      (paragraph, index) => paragraph === second.paragraphs[index],
    )
  );
}

export function countWritingWords(snapshot: WritingSnapshot) {
  return (
    snapshot.paragraphs
      .join(" ")
      .match(/[A-Za-z]+(?:['’-][A-Za-z]+)*/g) ?? []
  ).length;
}

export function getRecordStatus(
  record: WritingDocumentSummary,
): WritingDocumentStatus {
  return record.draftSnapshot ? "draft" : "completed";
}

export function getRecordSnapshot(
  record: WritingDocumentSummary,
): WritingSnapshot {
  return cloneWritingSnapshot(
    record.draftSnapshot ??
      record.completedSnapshot ??
      emptyWritingSnapshot,
  );
}
