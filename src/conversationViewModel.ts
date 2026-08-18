import type { ConversationOrigin } from "./types/quickAi";

export type ConversationInline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong"; text: string };

export type ConversationAnswerBlock =
  | {
      kind: "paragraph";
      content: ConversationInline[];
      tone?: "default" | "lead";
    }
  | {
      kind: "list";
      items: ConversationInline[][];
    }
  | {
      kind: "example";
      english: string;
      translation: string;
    };

export type ConversationMemoryCitation = {
  title: string;
  typeLabel: string;
  sourceApp: string;
  recordedAt: string;
  excerpt: string;
};

/** Agent 工具来源（任务 3）：与 Rust SourceMetadata 同构，驱动来源卡片。 */
export type AgentSource = {
  sourceId: string;
  title: string;
  url: string;
  siteName?: string | null;
  publishedAt?: string | null;
  retrievedAtUnixMs: number;
  contentType?: string | null;
};

export type ConversationUserMessage = {
  id: string;
  role: "user";
  content: string;
  meta?: string;
  sequence?: number;
};

export type ConversationAssistantMessage = {
  id: string;
  role: "assistant";
  blocks: ConversationAnswerBlock[];
  /** 真实 Quick AI 回答的原始 Markdown 文本；存在时页面优先渲染它（白名单子集），否则回退 blocks。 */
  markdown?: string;
  citation?: ConversationMemoryCitation;
  /** 回答引用的外部来源（任务 3/4）：来自 Agent 结构化来源事件，随回答落库后历史回看同样可用。 */
  sources?: AgentSource[];
  /** finish_reason=length 的诚实截断标志（任务 4）：回答已持久化，只显示轻微提示。 */
  truncated?: boolean;
  sequence?: number;
};

export type ConversationMessage =
  | ConversationUserMessage
  | ConversationAssistantMessage;

export type ConversationThread = {
  id: string;
  title: string;
  messages: ConversationMessage[];
  pendingTurn?: {
    userMessageId: string;
    prompt: string;
    expectedUserSequence: number;
  };
};

export type ConversationSummary = {
  id: string;
  title: string;
  origin: ConversationOrigin;
  updatedAtUnixMs: number;
};

export type ConversationOperationIdentity = {
  requestKey: number;
  conversationId: string;
};

export function isConversationOperationCurrent(
  mounted: boolean,
  operation: ConversationOperationIdentity,
  currentRequestKey: number,
  currentConversationId?: string,
) {
  return (
    mounted &&
    operation.requestKey === currentRequestKey &&
    operation.conversationId === currentConversationId
  );
}

export function shouldResetDeletedConversation(
  activePageId: string,
  activeConversationId: string | undefined,
  currentRequestKey: number,
  operation: ConversationOperationIdentity,
) {
  return isActiveConversationOperation(
    activePageId,
    activeConversationId,
    currentRequestKey,
    operation,
  );
}

export function isActiveConversationOperation(
  activePageId: string,
  activeConversationId: string | undefined,
  currentRequestKey: number,
  operation: ConversationOperationIdentity,
) {
  return (
    activePageId === "conversation" &&
    isConversationOperationCurrent(
      true,
      operation,
      currentRequestKey,
      activeConversationId,
    )
  );
}

export function conversationTitleEditAction(
  key: string,
  isComposing: boolean,
): "save" | "cancel" | undefined {
  if (isComposing) {
    return undefined;
  }
  if (key === "Enter") {
    return "save";
  }
  if (key === "Escape") {
    return "cancel";
  }
  return undefined;
}

export function conversationExportUnavailableReason(
  thread: ConversationThread | null,
  generating: boolean,
  canExport: boolean,
) {
  if (generating) {
    return "回答生成期间不能导出";
  }
  if (!thread) {
    return "当前没有可导出的对话";
  }
  if (thread.messages.length === 0) {
    return "空白会话没有可导出的消息";
  }
  if (!canExport) {
    return "当前对话服务不支持导出";
  }
  return undefined;
}

export type ConversationRequest =
  | {
      key: number;
      kind: "new";
    }
  | {
      key: number;
      kind: "existing";
      conversationId: string;
      title: string;
    }
  | {
      key: number;
      kind: "prompt";
      prompt: string;
    };

export type ConversationGeneratedReply =
  | {
      status: "complete";
      assistantMessageId: string;
      chunks: string[];
      persistedThread?: ConversationThread;
    }
  | {
      status: "truncated";
      assistantMessageId: string;
      chunks: string[];
      persistedThread?: ConversationThread;
    }
  | {
      status: "pending";
      persistedThread: ConversationThread;
      errorMessage: string;
    };

export type ConversationGenerationRequest = {
  conversationId: string;
  messages: ConversationMessage[];
  prompt: string;
  mode: "append" | "regenerate";
  /** 重新生成目标（任务 4）：被替代的旧 assistant 前端消息 ID（quick-ai-message-{id}）。 */
  replaceAssistantMessageId?: string;
  onStreamDelta?: (delta: string) => void;
  /** 来源更新回调（任务 3）：每次工具来源发布时增量更新。 */
  onSourcesUpdated?: (sources: AgentSource[]) => void;
  /** 工具状态文案回调（任务 3）："正在搜索/正在读取/正在整理"。 */
  onToolState?: (label: string) => void;
};

export type ConversationExportResult =
  | {
      exported: false;
      reason?: "cancelled" | "unavailable";
    }
  | {
      exported: true;
      fileName: string;
      messageCount: number;
      browserFile?: {
        fileName: string;
        mimeType: string;
        content: string;
      };
      nativeFilePath?: string;
    };

export type ConversationServiceCapabilities = {
  delivery: "complete" | "chunked-preview" | "streaming";
  canStop: boolean;
  canExport: boolean;
};

export interface ConversationService {
  readonly capabilities: ConversationServiceCapabilities;
  createConversation(): Promise<ConversationThread>;
  loadConversation(
    conversationId: string,
    title: string,
  ): Promise<ConversationThread>;
  listConversations(): Promise<ConversationSummary[]>;
  renameConversation(
    conversationId: string,
    title: string,
  ): Promise<ConversationThread>;
  deleteConversation(conversationId: string): Promise<void>;
  generateReply(
    request: ConversationGenerationRequest,
  ): Promise<ConversationGeneratedReply>;
  exportConversation(
    thread: ConversationThread,
  ): Promise<ConversationExportResult>;
  stopGeneration?(conversationId: string): Promise<void>;
  /** 受控打开来源 URL（任务 3）：Rust 端校验 HTTP(S)/凭据/保留网段后交给 opener。 */
  openSource(url: string): Promise<void>;
}
