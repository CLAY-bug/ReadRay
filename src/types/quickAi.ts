export type QuickAiRole = "user" | "assistant";

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
  createdAtUnixMs: number;
  updatedAtUnixMs: number;
  messages: QuickAiMessage[];
};

export type RecentQuickAiConversation = {
  id: number;
  title: string;
  updatedAtUnixMs: number;
};
