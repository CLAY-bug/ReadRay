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
};

export type ConversationAssistantMessage = {
  id: string;
  role: "assistant";
  blocks: ConversationAnswerBlock[];
  citation?: ConversationMemoryCitation;
};

export type ConversationMessage =
  | ConversationUserMessage
  | ConversationAssistantMessage;

export type ConversationThread = {
  id: string;
  title: string;
  messages: ConversationMessage[];
};

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

export type ConversationGeneratedReply = {
  assistantMessageId: string;
  chunks: string[];
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
    }
  | {
      exported: true;
      file: {
        fileName: string;
        mimeType: string;
        content: string;
      };
    };

export interface ConversationService {
  createConversation(): Promise<ConversationThread>;
  loadConversation(
    conversationId: string,
    title: string,
  ): Promise<ConversationThread>;
  generateReply(
    request: ConversationGenerationRequest,
  ): Promise<ConversationGeneratedReply>;
  exportConversation(
    thread: ConversationThread,
  ): Promise<ConversationExportResult>;
}
