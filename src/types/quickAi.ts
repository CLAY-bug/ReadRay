export type QuickAiRole = "user" | "assistant";
export type ConversationOrigin = "overlay" | "main";

export type QuickAiMessage = {
  id: number;
  conversationId: number;
  role: QuickAiRole;
  content: string;
  sequence: number;
  createdAtUnixMs: number;
};

export type QuickAiConversation = {
  id: number;
  title?: string | null;
  model: string;
  origin: ConversationOrigin;
  createdAtUnixMs: number;
  updatedAtUnixMs: number;
  messages: QuickAiMessage[];
};

export type RecentQuickAiConversation = {
  id: number;
  title: string;
  origin: ConversationOrigin;
  updatedAtUnixMs: number;
};

export type QuickAiConversationExport = {
  conversationId: number;
  fileName: string;
  filePath: string;
  messageCount: number;
};
