// 与 conversationViewModel 的类型互引仅为 type-only（编译期擦除），无运行时循环。
import type { AgentSource } from "../conversationViewModel";

export type QuickAiRole = "user" | "assistant";
export type ConversationOrigin = "overlay" | "main";

export type QuickAiMessage = {
  id: number;
  conversationId: number;
  role: QuickAiRole;
  content: string;
  sequence: number;
  createdAtUnixMs: number;
  /** 回答引用的外部来源（任务 4）：随 assistant 消息落库，重启与历史回看可用。 */
  sources?: AgentSource[] | null;
  /** finish_reason=length 的诚实截断标志（任务 4）：回答已持久化，只给轻微提示。 */
  truncated?: boolean;
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
