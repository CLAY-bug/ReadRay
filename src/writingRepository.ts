import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  WritingAgentAnswer,
  WritingAnalysis,
  WritingDocumentSummary,
  WritingQuestionScope,
  WritingSnapshot,
  WritingVersion,
} from "./writingViewModel";

export type WritingQuestionCommand = {
  documentId: number;
  expectedRevision: number;
  versionId?: number;
  question: string;
  scope: WritingQuestionScope;
  selectionText?: string;
  parentAnswerId?: number;
};

/// 写作流式进度/终态事件（Rust 侧 `WritingStreamEvent` 的镜像）。检查/问答输出
/// 结构化 JSON，不逐字流式渲染；前端只展示友好进度与终态。
export type WritingStreamEvent =
  | { type: "status"; label: string }
  | { type: "done" }
  | { type: "stopped" }
  | { type: "error"; message: string };

export type WritingDocumentSummaryPayload = Omit<
  WritingDocumentSummary,
  | "lastOpenedAtUnixMs"
  | "draftUpdatedAtUnixMs"
  | "completedAtUnixMs"
  | "draftSnapshot"
  | "completedSnapshot"
> & {
  lastOpenedAtUnixMs?: number | null;
  draftUpdatedAtUnixMs?: number | null;
  completedAtUnixMs?: number | null;
  draftSnapshot?: WritingSnapshot | null;
  completedSnapshot?: WritingSnapshot | null;
};

export type WritingVersionPayload = Omit<
  WritingVersion,
  "analysisRevision" | "comparisonBaselineRevision"
> & {
  analysisRevision?: number | null;
  comparisonBaselineRevision?: number | null;
};

export type WritingAgentAnswerPayload = Omit<
  WritingAgentAnswer,
  "versionId" | "parentAnswerId" | "selectionText" | "map"
> & {
  versionId?: number | null;
  parentAnswerId?: number | null;
  selectionText?: string | null;
  map?: WritingAgentAnswer["map"] | null;
};

export type WritingDocumentPayload = WritingDocumentSummaryPayload & {
  comparisonBaseline: WritingSnapshot;
  comparisonBaselineRevision?: number | null;
  versions: WritingVersionPayload[];
  activeAnalysis?: WritingAnalysis | null;
  baselineAnalysis?: WritingAnalysis | null;
  answers: WritingAgentAnswerPayload[];
};

export interface WritingRepository {
  create(): Promise<WritingDocumentPayload>;
  list(query?: string): Promise<WritingDocumentSummaryPayload[]>;
  get(documentId: number): Promise<WritingDocumentPayload | null>;
  saveDraft(
    documentId: number,
    expectedRevision: number,
    snapshot: WritingSnapshot,
  ): Promise<WritingDocumentPayload>;
  delete(documentId: number, expectedRevision: number): Promise<boolean>;
  analyze(
    documentId: number,
    expectedRevision: number,
    onEvent: (event: WritingStreamEvent) => void,
  ): Promise<WritingDocumentPayload>;
  ask(
    request: WritingQuestionCommand,
    onEvent: (event: WritingStreamEvent) => void,
  ): Promise<WritingAgentAnswerPayload>;
  abort(documentId: number): Promise<void>;
  complete(
    documentId: number,
    expectedRevision: number,
  ): Promise<WritingDocumentPayload>;
  continueEditing(
    documentId: number,
    expectedRevision: number,
    versionId?: number,
  ): Promise<WritingDocumentPayload>;
}

export type WritingInvoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export class TauriWritingRepository implements WritingRepository {
  private readonly invokeCommand: WritingInvoke;

  constructor(invokeCommand: WritingInvoke = invoke) {
    this.invokeCommand = invokeCommand;
  }

  create() {
    return this.invokeCommand<WritingDocumentPayload>(
      "create_writing_document",
    );
  }

  list(query?: string) {
    return this.invokeCommand<WritingDocumentSummaryPayload[]>(
      "list_writing_documents",
      { query: query?.trim() || null },
    );
  }

  get(documentId: number) {
    return this.invokeCommand<WritingDocumentPayload | null>(
      "get_writing_document",
      { documentId },
    );
  }

  saveDraft(
    documentId: number,
    expectedRevision: number,
    snapshot: WritingSnapshot,
  ) {
    return this.invokeCommand<WritingDocumentPayload>(
      "save_writing_draft",
      { documentId, expectedRevision, snapshot },
    );
  }

  delete(documentId: number, expectedRevision: number) {
    return this.invokeCommand<boolean>("delete_writing_document", {
      documentId,
      expectedRevision,
    });
  }

  analyze(
    documentId: number,
    expectedRevision: number,
    onEvent: (event: WritingStreamEvent) => void,
  ) {
    const channel = new Channel<WritingStreamEvent>(onEvent);
    return this.invokeCommand<WritingDocumentPayload>(
      "analyze_writing_document",
      { documentId, expectedRevision, channel },
    );
  }

  ask(
    request: WritingQuestionCommand,
    onEvent: (event: WritingStreamEvent) => void,
  ) {
    const channel = new Channel<WritingStreamEvent>(onEvent);
    return this.invokeCommand<WritingAgentAnswerPayload>(
      "ask_writing_question",
      { request, channel },
    );
  }

  abort(documentId: number) {
    return this.invokeCommand<void>("abort_writing_analysis", {
      documentId,
    });
  }

  complete(documentId: number, expectedRevision: number) {
    return this.invokeCommand<WritingDocumentPayload>(
      "complete_writing_document",
      { documentId, expectedRevision },
    );
  }

  continueEditing(
    documentId: number,
    expectedRevision: number,
    versionId?: number,
  ) {
    return this.invokeCommand<WritingDocumentPayload>(
      "continue_writing_document",
      {
        documentId,
        expectedRevision,
        versionId: versionId ?? null,
      },
    );
  }
}
