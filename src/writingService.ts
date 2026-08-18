import type {
  WritingAgentAnswerPayload,
  WritingDocumentPayload,
  WritingDocumentSummaryPayload,
  WritingQuestionCommand,
  WritingRepository,
  WritingStreamEvent,
  WritingVersionPayload,
} from "./writingRepository";
import {
  cloneWritingSnapshot,
  type WritingAgentAnswer,
  type WritingAnalysis,
  type WritingDocumentRecord,
  type WritingDocumentSummary,
  type WritingIssue,
  type WritingPattern,
  type WritingQuestionScope,
  type WritingSnapshot,
  type WritingVersion,
} from "./writingViewModel.ts";

export interface WritingService {
  listDocuments(query?: string): Promise<WritingDocumentSummary[]>;
  createDocument(): Promise<WritingDocumentRecord>;
  loadDocument(documentId: number): Promise<WritingDocumentRecord>;
  saveDraft(
    documentId: number,
    expectedRevision: number,
    snapshot: WritingSnapshot,
  ): Promise<WritingDocumentRecord>;
  deleteDocument(
    documentId: number,
    expectedRevision: number,
  ): Promise<boolean>;
  analyzeDocument(
    documentId: number,
    expectedRevision: number,
    onEvent: (event: WritingStreamEvent) => void,
  ): Promise<WritingDocumentRecord>;
  askQuestion(
    request: WritingQuestionCommand,
    onEvent: (event: WritingStreamEvent) => void,
  ): Promise<WritingAgentAnswer>;
  abortAnalysis(documentId: number): Promise<void>;
  completeDocument(
    documentId: number,
    expectedRevision: number,
  ): Promise<WritingDocumentRecord>;
  continueEditing(
    documentId: number,
    expectedRevision: number,
    versionId?: number,
  ): Promise<WritingDocumentRecord>;
}

function requirePositiveInteger(value: number, label: string) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${label}无效。`);
  }
}

function requireRevision(value: number) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error("写作文章 revision 无效。");
  }
}

function requireTimestamp(
  value: number | null | undefined,
  label: string,
) {
  if (
    value != null &&
    (!Number.isSafeInteger(value) || value <= 0)
  ) {
    throw new Error(`${label}无效。`);
  }
}

function mapSnapshot(
  snapshot: WritingSnapshot | null | undefined,
): WritingSnapshot | undefined {
  if (!snapshot) {
    return undefined;
  }
  if (
    typeof snapshot.title !== "string" ||
    !Array.isArray(snapshot.paragraphs) ||
    snapshot.paragraphs.length === 0 ||
    snapshot.paragraphs.some((paragraph) => typeof paragraph !== "string")
  ) {
    throw new Error("写作正文快照无效。");
  }
  return cloneWritingSnapshot(snapshot);
}

function requireMappedSnapshot(
  snapshot: WritingSnapshot | null | undefined,
  label: string,
) {
  const mapped = mapSnapshot(snapshot);
  if (!mapped) {
    throw new Error(`${label}缺失。`);
  }
  return mapped;
}

function mapPattern(pattern: WritingPattern): WritingPattern {
  if (
    !pattern.id?.trim() ||
    !pattern.title?.trim() ||
    !pattern.description?.trim()
  ) {
    throw new Error("写作模式结果无效。");
  }
  return { ...pattern };
}

function mapIssue(issue: WritingIssue): WritingIssue {
  if (
    !issue.id?.trim() ||
    !issue.category?.trim() ||
    !issue.source?.trim() ||
    !issue.targetText?.trim() ||
    !issue.explanation?.trim() ||
    !issue.hint?.trim() ||
    !issue.deeperHint?.trim() ||
    !issue.reference?.trim()
  ) {
    throw new Error("写作问题结果无效。");
  }
  return { ...issue };
}

function mapAnalysis(
  analysis: WritingAnalysis | null | undefined,
  documentId: number,
): WritingAnalysis | undefined {
  if (!analysis) {
    return undefined;
  }
  requirePositiveInteger(analysis.id, "写作分析 ID");
  requirePositiveInteger(analysis.documentId, "写作分析文章 ID");
  requireRevision(analysis.documentRevision);
  requirePositiveInteger(analysis.round, "写作分析轮次");
  requireTimestamp(analysis.createdAtUnixMs, "写作分析时间");
  if (analysis.documentId !== documentId) {
    throw new Error("写作分析结果串写到其他文章。");
  }
  return {
    ...analysis,
    issues: analysis.issues.map(mapIssue),
    patterns: analysis.patterns.map(mapPattern),
  };
}

function mapVersion(
  version: WritingVersionPayload,
  documentId: number,
): WritingVersion {
  requirePositiveInteger(version.id, "写作版本 ID");
  requirePositiveInteger(version.documentId, "写作版本文章 ID");
  requirePositiveInteger(version.ordinal, "写作版本序号");
  requireRevision(version.sourceRevision);
  if (version.comparisonBaselineRevision != null) {
    requireRevision(version.comparisonBaselineRevision);
    if (version.comparisonBaselineRevision > version.sourceRevision) {
      throw new Error("写作版本对比基线晚于正文来源 revision。");
    }
  }
  if (version.analysisRevision != null) {
    requireRevision(version.analysisRevision);
    if (version.analysisRevision > version.sourceRevision) {
      throw new Error("写作版本分析 revision 晚于正文来源。");
    }
    if (
      version.comparisonBaselineRevision != null &&
      version.analysisRevision !== version.comparisonBaselineRevision
    ) {
      throw new Error("写作版本分析 revision 与对比基线不一致。");
    }
    if (
      version.comparisonBaselineRevision == null &&
      version.analysisRevision !== version.sourceRevision
    ) {
      throw new Error("旧写作版本分析 revision 与正文来源不一致。");
    }
  }
  requireTimestamp(version.completedAtUnixMs, "写作版本完成时间");
  if (version.documentId !== documentId) {
    throw new Error("写作版本串写到其他文章。");
  }
  return {
    ...version,
    analysisRevision: version.analysisRevision ?? undefined,
    comparisonBaselineRevision:
      version.comparisonBaselineRevision ?? undefined,
    snapshot: requireMappedSnapshot(version.snapshot, "写作版本正文"),
    comparisonBaseline: requireMappedSnapshot(
      version.comparisonBaseline,
      "写作版本对比基线",
    ),
    issues: version.issues.map(mapIssue),
    patterns: version.patterns.map(mapPattern),
  };
}

function mapAnswer(
  answer: WritingAgentAnswerPayload,
  documentId: number,
): WritingAgentAnswer {
  requirePositiveInteger(answer.id, "写作辅助回答 ID");
  requirePositiveInteger(answer.documentId, "写作辅助文章 ID");
  requireRevision(answer.documentRevision);
  if (answer.versionId != null) {
    requirePositiveInteger(answer.versionId, "写作辅助目标版本 ID");
  }
  requireTimestamp(answer.createdAtUnixMs, "写作辅助回答时间");
  if (answer.documentId !== documentId) {
    throw new Error("写作辅助回答串写到其他文章。");
  }
  if (!answer.question?.trim() || !answer.title?.trim() || !answer.copy?.trim()) {
    throw new Error("写作辅助回答内容无效。");
  }
  if (!["document", "paragraph", "selection"].includes(answer.scope)) {
    throw new Error("写作辅助回答 scope 无效。");
  }
  return {
    ...answer,
    versionId: answer.versionId ?? undefined,
    parentAnswerId: answer.parentAnswerId ?? undefined,
    selectionText: answer.selectionText ?? undefined,
    map: answer.map
      ? {
          core: answer.map.core,
          questions: [...answer.map.questions],
          phrases: [...answer.map.phrases],
          starters: [...answer.map.starters],
        }
      : undefined,
  };
}

export function mapWritingDocumentSummary(
  summary: WritingDocumentSummaryPayload,
): WritingDocumentSummary {
  requirePositiveInteger(summary.id, "写作文章 ID");
  requireRevision(summary.revision);
  requireTimestamp(summary.createdAtUnixMs, "写作文章创建时间");
  requireTimestamp(summary.updatedAtUnixMs, "写作文章更新时间");
  requireTimestamp(summary.lastOpenedAtUnixMs, "写作文章最近打开时间");
  requireTimestamp(summary.draftUpdatedAtUnixMs, "写作草稿更新时间");
  requireTimestamp(summary.completedAtUnixMs, "写作完成时间");
  const draftSnapshot = mapSnapshot(summary.draftSnapshot);
  const completedSnapshot = mapSnapshot(summary.completedSnapshot);
  if (!draftSnapshot && !completedSnapshot) {
    throw new Error("写作文章同时缺少草稿和完成稿。");
  }
  return {
    ...summary,
    lastOpenedAtUnixMs: summary.lastOpenedAtUnixMs ?? undefined,
    draftUpdatedAtUnixMs: summary.draftUpdatedAtUnixMs ?? undefined,
    completedAtUnixMs: summary.completedAtUnixMs ?? undefined,
    draftSnapshot,
    completedSnapshot,
  };
}

export function mapWritingDocument(
  document: WritingDocumentPayload,
): WritingDocumentRecord {
  const summary = mapWritingDocumentSummary(document);
  if (document.comparisonBaselineRevision != null) {
    requireRevision(document.comparisonBaselineRevision);
  }
  const activeAnalysis = mapAnalysis(
    document.activeAnalysis,
    summary.id,
  );
  const baselineAnalysis = mapAnalysis(
    document.baselineAnalysis,
    summary.id,
  );
  return {
    ...summary,
    comparisonBaselineRevision:
      document.comparisonBaselineRevision ?? undefined,
    comparisonBaseline: requireMappedSnapshot(
      document.comparisonBaseline,
      "写作对比基线",
    ),
    versions: document.versions.map((version) =>
      mapVersion(version, summary.id),
    ),
    activeAnalysis:
      activeAnalysis?.documentRevision === summary.revision
        ? activeAnalysis
        : undefined,
    baselineAnalysis:
      baselineAnalysis?.documentRevision ===
      (document.comparisonBaselineRevision ?? undefined)
        ? baselineAnalysis
        : undefined,
    answers: document.answers.map((answer) =>
      mapAnswer(answer, summary.id),
    ),
  };
}

function requireDocumentId(documentId: number) {
  requirePositiveInteger(documentId, "写作文章 ID");
  return documentId;
}

function requireQuestionScope(scope: WritingQuestionScope) {
  if (!["document", "paragraph", "selection"].includes(scope)) {
    throw new Error("写作辅助问题 scope 无效。");
  }
}

export class RepositoryWritingService implements WritingService {
  private readonly repository: WritingRepository;

  constructor(repository: WritingRepository) {
    this.repository = repository;
  }

  async listDocuments(query?: string) {
    return (await this.repository.list(query)).map(mapWritingDocumentSummary);
  }

  async createDocument() {
    return mapWritingDocument(await this.repository.create());
  }

  async loadDocument(documentId: number) {
    const document = await this.repository.get(requireDocumentId(documentId));
    if (!document) {
      throw new Error("这篇写作文章不存在或已被删除。");
    }
    return mapWritingDocument(document);
  }

  async saveDraft(
    documentId: number,
    expectedRevision: number,
    snapshot: WritingSnapshot,
  ) {
    requireDocumentId(documentId);
    requireRevision(expectedRevision);
    const mappedSnapshot = requireMappedSnapshot(snapshot, "写作草稿");
    return mapWritingDocument(
      await this.repository.saveDraft(
        documentId,
        expectedRevision,
        mappedSnapshot,
      ),
    );
  }

  async deleteDocument(documentId: number, expectedRevision: number) {
    requireDocumentId(documentId);
    requireRevision(expectedRevision);
    return this.repository.delete(documentId, expectedRevision);
  }

  async analyzeDocument(
    documentId: number,
    expectedRevision: number,
    onEvent: (event: WritingStreamEvent) => void,
  ) {
    requireDocumentId(documentId);
    requireRevision(expectedRevision);
    return mapWritingDocument(
      await this.repository.analyze(documentId, expectedRevision, onEvent),
    );
  }

  async askQuestion(
    request: WritingQuestionCommand,
    onEvent: (event: WritingStreamEvent) => void,
  ) {
    requireDocumentId(request.documentId);
    requireRevision(request.expectedRevision);
    requireQuestionScope(request.scope);
    if (!request.question.trim()) {
      throw new Error("写作辅助问题不能为空。");
    }
    return mapAnswer(
      await this.repository.ask({ ...request }, onEvent),
      request.documentId,
    );
  }

  abortAnalysis(documentId: number) {
    requireDocumentId(documentId);
    return this.repository.abort(documentId);
  }

  async completeDocument(documentId: number, expectedRevision: number) {
    requireDocumentId(documentId);
    requireRevision(expectedRevision);
    return mapWritingDocument(
      await this.repository.complete(documentId, expectedRevision),
    );
  }

  async continueEditing(
    documentId: number,
    expectedRevision: number,
    versionId?: number,
  ) {
    requireDocumentId(documentId);
    requireRevision(expectedRevision);
    if (versionId !== undefined) {
      requirePositiveInteger(versionId, "写作版本 ID");
    }
    return mapWritingDocument(
      await this.repository.continueEditing(
        documentId,
        expectedRevision,
        versionId,
      ),
    );
  }
}
