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
  citation?: ConversationMemoryCitation;
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
      status: "pending";
      persistedThread: ConversationThread;
      errorMessage: string;
    };

export type ConversationGenerationRequest = {
  conversationId: string;
  messages: ConversationMessage[];
  prompt: string;
  mode: "append" | "regenerate";
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
  delivery: "complete" | "chunked-preview";
  canStop: boolean;
  canRegenerate: boolean;
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
}
