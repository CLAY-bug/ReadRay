import type { WritingQuestionCommand } from "./writingRepository";
import type { WritingService } from "./writingService";
import {
  emptyWritingSnapshot,
  normalizeWritingText,
  type WritingAgentAnswer,
  type WritingAnalysis,
  type WritingDocumentRecord,
  type WritingDocumentSummary,
  type WritingPattern,
  type WritingSnapshot,
  type WritingVersion,
} from "./writingViewModel";

const STORAGE_KEY = "readray:writing-preview:v2";

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function now() {
  return Date.now();
}

function previewFixtures(): WritingDocumentRecord[] {
  const timestamp = new Date("2026-07-18T23:04:00+08:00").getTime();
  const snapshot: WritingSnapshot = {
    title: "When Technology Learns to Stay Quiet",
    paragraphs: [
      "Digital tools often promise to make us more productive, but many of them ask for our attention before they earn it.",
      "Last semester, I tried several writing applications. This was useful at first, but it also made me to wait for the machine's opinion.",
      "A helpful writing assistant should not replace my voice. Revision should become a form of learning.",
    ],
  };
  return [
    {
      id: 1,
      revision: 1,
      createdAtUnixMs: timestamp,
      updatedAtUnixMs: timestamp,
      lastOpenedAtUnixMs: timestamp,
      draftUpdatedAtUnixMs: timestamp,
      draftSnapshot: snapshot,
      comparisonBaseline: clone(snapshot),
      comparisonBaselineRevision: 1,
      versions: [],
      answers: [],
    },
  ];
}

function summary(record: WritingDocumentRecord): WritingDocumentSummary {
  const {
    comparisonBaseline: _comparisonBaseline,
    comparisonBaselineRevision: _comparisonBaselineRevision,
    versions: _versions,
    activeAnalysis: _activeAnalysis,
    baselineAnalysis: _baselineAnalysis,
    answers: _answers,
    ...value
  } = record;
  return clone(value);
}

function searchableText(record: WritingDocumentRecord) {
  return normalizeWritingText(
    [
      record.draftSnapshot?.title ?? "",
      ...(record.draftSnapshot?.paragraphs ?? []),
      record.completedSnapshot?.title ?? "",
      ...(record.completedSnapshot?.paragraphs ?? []),
    ].join(" "),
  ).toLocaleLowerCase("zh-CN");
}

class BrowserPreviewWritingService implements WritingService {
  private records: WritingDocumentRecord[];
  private nextId: number;

  constructor() {
    this.records = this.read();
    this.nextId =
      Math.max(
        1,
        ...this.records.flatMap((record) => [
          record.id,
          ...record.versions.map((version) => version.id),
          ...record.answers.map((answer) => answer.id),
          record.activeAnalysis?.id ?? 0,
          record.baselineAnalysis?.id ?? 0,
        ]),
      ) + 1;
  }

  async listDocuments(query?: string) {
    const normalized = normalizeWritingText(query ?? "").toLocaleLowerCase(
      "zh-CN",
    );
    return this.records
      .filter((record) => !normalized || searchableText(record).includes(normalized))
      .sort((first, second) => second.updatedAtUnixMs - first.updatedAtUnixMs)
      .map(summary);
  }

  async createDocument() {
    const timestamp = now();
    const record: WritingDocumentRecord = {
      id: this.nextId++,
      revision: 0,
      createdAtUnixMs: timestamp,
      updatedAtUnixMs: timestamp,
      lastOpenedAtUnixMs: timestamp,
      draftUpdatedAtUnixMs: timestamp,
      draftSnapshot: clone(emptyWritingSnapshot),
      comparisonBaseline: clone(emptyWritingSnapshot),
      comparisonBaselineRevision: 0,
      versions: [],
      answers: [],
    };
    this.records.unshift(record);
    this.write();
    return clone(record);
  }

  async loadDocument(documentId: number) {
    const record = this.required(documentId);
    record.lastOpenedAtUnixMs = now();
    this.write();
    return clone(record);
  }

  async saveDraft(
    documentId: number,
    expectedRevision: number,
    snapshot: WritingSnapshot,
  ) {
    const record = this.requiredRevision(documentId, expectedRevision);
    const timestamp = now();
    record.revision += 1;
    record.updatedAtUnixMs = timestamp;
    record.draftUpdatedAtUnixMs = timestamp;
    record.draftSnapshot = clone(snapshot);
    record.activeAnalysis = undefined;
    this.write();
    return clone(record);
  }

  async deleteDocument(documentId: number, expectedRevision: number) {
    this.requiredRevision(documentId, expectedRevision);
    const index = this.records.findIndex((record) => record.id === documentId);
    this.records.splice(index, 1);
    this.write();
    return true;
  }

  async analyzeDocument(documentId: number, expectedRevision: number) {
    const record = this.requiredRevision(documentId, expectedRevision);
    const snapshot = record.draftSnapshot;
    if (!snapshot) {
      throw new Error("完成稿需要先继续修改。");
    }
    const body = snapshot.paragraphs.join(" ");
    const issues = body.includes("made me to wait")
      ? [
          {
            id: "preview-verb",
            category: "动词结构",
            source: "made me to wait",
            targetText: "made me to wait",
            explanation: "make + 人 后面的动作通常直接使用动词原形。",
            hint: "提示：去掉动作前多余的连接词。",
            deeperHint: "保留 made me，然后直接接真正执行的动作。",
            reference: "It made me wait for the machine's opinion.",
          },
        ]
      : [];
    const patterns: WritingPattern[] = issues.length
      ? [
          {
            id: "01",
            title: "make + 人 + 动词原形",
            description: "使役动词后直接接动作。",
          },
        ]
      : [];
    record.revision += 1;
    const analysis: WritingAnalysis = {
      id: this.nextId++,
      documentId,
      documentRevision: record.revision,
      round: (record.activeAnalysis?.round ?? 0) + 1,
      issues,
      patterns,
      createdAtUnixMs: now(),
    };
    record.activeAnalysis = analysis;
    record.baselineAnalysis = clone(analysis);
    record.comparisonBaseline = clone(snapshot);
    record.comparisonBaselineRevision = record.revision;
    record.updatedAtUnixMs = now();
    this.write();
    return clone(record);
  }

  async askQuestion(request: WritingQuestionCommand) {
    const record = this.requiredRevision(
      request.documentId,
      request.expectedRevision,
    );
    const answer: WritingAgentAnswer = {
      id: this.nextId++,
      documentId: request.documentId,
      documentRevision:
        request.versionId === undefined
          ? request.expectedRevision
          : record.versions.find(
              (version) => version.id === request.versionId,
            )?.sourceRevision ?? request.expectedRevision,
      versionId: request.versionId,
      parentAnswerId: request.parentAnswerId,
      question: request.question,
      scope: request.scope,
      scopeLabel:
        request.scope === "selection"
          ? "所选内容"
          : request.scope === "document"
            ? "整篇文章"
            : "当前段落",
      selectionText: request.selectionText,
      title: "先处理最小的障碍",
      copy: "保留原意，先确认句子的主语和核心动词，再只调整一个影响理解的结构点。",
      createdAtUnixMs: now(),
    };
    record.answers.push(answer);
    this.write();
    return clone(answer);
  }

  async completeDocument(documentId: number, expectedRevision: number) {
    const record = this.requiredRevision(documentId, expectedRevision);
    if (!record.draftSnapshot) {
      throw new Error("当前没有可完成的草稿。");
    }
    const timestamp = now();
    const version: WritingVersion = {
      id: this.nextId++,
      documentId,
      ordinal: record.versions.length + 1,
      sourceRevision: record.revision,
      analysisRevision: record.baselineAnalysis?.documentRevision,
      comparisonBaselineRevision:
        record.comparisonBaselineRevision,
      snapshot: clone(record.draftSnapshot),
      comparisonBaseline: clone(record.comparisonBaseline),
      issues: clone(record.baselineAnalysis?.issues ?? []),
      patterns: clone(record.baselineAnalysis?.patterns ?? []),
      completedAtUnixMs: timestamp,
    };
    record.versions.push(version);
    record.completedSnapshot = clone(record.draftSnapshot);
    record.completedAtUnixMs = timestamp;
    record.draftSnapshot = undefined;
    record.draftUpdatedAtUnixMs = undefined;
    record.revision += 1;
    record.activeAnalysis = undefined;
    record.updatedAtUnixMs = timestamp;
    this.write();
    return clone(record);
  }

  async continueEditing(
    documentId: number,
    expectedRevision: number,
    versionId?: number,
  ) {
    const record = this.requiredRevision(documentId, expectedRevision);
    if (record.draftSnapshot) {
      throw new Error("当前文章已有修改中草稿，请返回现有草稿。");
    }
    const version = versionId
      ? record.versions.find((candidate) => candidate.id === versionId)
      : record.versions[record.versions.length - 1];
    if (!version) {
      throw new Error("没有可继续修改的完成版本。");
    }
    const timestamp = now();
    record.draftSnapshot = clone(version.snapshot);
    record.draftUpdatedAtUnixMs = timestamp;
    record.comparisonBaseline = clone(version.snapshot);
    record.revision += 1;
    record.comparisonBaselineRevision = record.revision;
    record.activeAnalysis = undefined;
    record.baselineAnalysis = undefined;
    record.updatedAtUnixMs = timestamp;
    this.write();
    return clone(record);
  }

  private required(documentId: number) {
    const record = this.records.find((candidate) => candidate.id === documentId);
    if (!record) {
      throw new Error("预览文章不存在。");
    }
    return record;
  }

  private requiredRevision(documentId: number, expectedRevision: number) {
    const record = this.required(documentId);
    if (record.revision !== expectedRevision) {
      throw new Error("预览文章版本冲突。");
    }
    return record;
  }

  private read() {
    try {
      const raw = window.localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as WritingDocumentRecord[];
        if (Array.isArray(parsed)) {
          return parsed.map((record) => ({
            ...record,
            comparisonBaselineRevision:
              record.comparisonBaselineRevision ?? record.revision,
            baselineAnalysis:
              record.baselineAnalysis ??
              (record.activeAnalysis?.documentRevision ===
              (record.comparisonBaselineRevision ?? record.revision)
                ? record.activeAnalysis
                : undefined),
            versions: record.versions.map((version) => ({
              ...version,
              comparisonBaselineRevision:
                version.comparisonBaselineRevision ??
                version.sourceRevision,
            })),
          }));
        }
      }
    } catch {
      // 浏览器预览无法使用 localStorage 时退回当前会话内存。
    }
    return previewFixtures();
  }

  private write() {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(this.records));
    } catch {
      // 浏览器预览无法使用 localStorage 时保留当前会话内存。
    }
  }
}

export function createBrowserPreviewWritingService(): WritingService {
  return new BrowserPreviewWritingService();
}
