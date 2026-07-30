import type { ConversationRepository } from "./conversationRepository.ts";
import type {
  ConversationAssistantMessage,
  ConversationExportResult,
  ConversationGenerationRequest,
  ConversationMessage,
  ConversationService,
  ConversationThread,
  ConversationUserMessage,
} from "./conversationViewModel.ts";
import type {
  QuickAiConversation,
  QuickAiMessage,
} from "./types/quickAi.ts";

const COMPLETE_DELIVERY_CAPABILITIES = {
  delivery: "complete",
  canStop: false,
  canRegenerate: false,
  canExport: false,
} as const;

function requireConversationId(value: string) {
  const conversationId = Number(value);
  if (!Number.isSafeInteger(conversationId) || conversationId <= 0) {
    throw new Error("Quick AI 会话 ID 无效。");
  }
  return conversationId;
}

function formatMessageMeta(createdAtUnixMs: number, now: Date) {
  const createdAt = new Date(createdAtUnixMs);
  if (Number.isNaN(createdAt.getTime())) {
    return undefined;
  }

  const sameDay =
    createdAt.getFullYear() === now.getFullYear() &&
    createdAt.getMonth() === now.getMonth() &&
    createdAt.getDate() === now.getDate();
  const yesterday = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() - 1,
  );
  const wasYesterday =
    createdAt.getFullYear() === yesterday.getFullYear() &&
    createdAt.getMonth() === yesterday.getMonth() &&
    createdAt.getDate() === yesterday.getDate();
  const time = `${createdAt.getHours().toString().padStart(2, "0")}:${createdAt
    .getMinutes()
    .toString()
    .padStart(2, "0")}`;

  if (sameDay) {
    return `今天 · ${time}`;
  }
  if (wasYesterday) {
    return `昨天 · ${time}`;
  }
  return `${createdAt.getFullYear()}-${(createdAt.getMonth() + 1)
    .toString()
    .padStart(2, "0")}-${createdAt
    .getDate()
    .toString()
    .padStart(2, "0")} · ${time}`;
}

function validateSnapshot(snapshot: QuickAiConversation) {
  if (!Number.isSafeInteger(snapshot.id) || snapshot.id <= 0) {
    throw new Error("Quick AI 返回了无效的会话 ID。");
  }

  let previousSequence = 0;
  for (const message of snapshot.messages) {
    if (
      !Number.isSafeInteger(message.id) ||
      message.id <= 0 ||
      message.conversationId !== snapshot.id ||
      !Number.isSafeInteger(message.sequence) ||
      message.sequence <= previousSequence ||
      !message.content.trim()
    ) {
      throw new Error("Quick AI 返回了无效的会话消息。");
    }
    previousSequence = message.sequence;
  }
}

function mapMessage(
  message: QuickAiMessage,
  now: Date,
): ConversationMessage {
  if (message.role === "user") {
    return {
      id: `quick-ai-message-${message.id}`,
      role: "user",
      content: message.content,
      meta: formatMessageMeta(message.createdAtUnixMs, now),
      sequence: message.sequence,
    } satisfies ConversationUserMessage;
  }

  return {
    id: `quick-ai-message-${message.id}`,
    role: "assistant",
    blocks: [
      {
        kind: "paragraph",
        content: [{ kind: "text", text: message.content }],
      },
    ],
    sequence: message.sequence,
  } satisfies ConversationAssistantMessage;
}

export function mapQuickAiConversation(
  snapshot: QuickAiConversation,
  now = new Date(),
): ConversationThread {
  validateSnapshot(snapshot);
  const messages = snapshot.messages.map((message) => mapMessage(message, now));
  const lastMessage = snapshot.messages[snapshot.messages.length - 1];
  return {
    id: String(snapshot.id),
    title: snapshot.title?.trim() || "新对话",
    messages,
    pendingTurn:
      lastMessage?.role === "user"
        ? {
            userMessageId: `quick-ai-message-${lastMessage.id}`,
            prompt: lastMessage.content,
            expectedUserSequence: lastMessage.sequence,
          }
        : undefined,
  };
}

export type RepositoryConversationServiceOptions = {
  onConversationUpdated?: () => void;
};

function expectedUserSequence(request: ConversationGenerationRequest) {
  const pendingUser = request.messages[request.messages.length - 1];
  if (
    pendingUser?.role !== "user" ||
    pendingUser.content.trim() !== request.prompt.trim()
  ) {
    throw new Error("当前对话缺少待发送的用户消息。");
  }
  if (pendingUser.sequence !== undefined) {
    if (
      !Number.isSafeInteger(pendingUser.sequence) ||
      pendingUser.sequence <= 0 ||
      pendingUser.sequence % 2 === 0
    ) {
      throw new Error("待回答用户消息的 sequence 无效。");
    }
    return pendingUser.sequence;
  }

  const previousSequence = request.messages
    .slice(0, -1)
    .reduce(
      (maximum, message) => Math.max(maximum, message.sequence ?? 0),
      0,
    );
  const sequence = previousSequence + 1;
  if (sequence % 2 === 0) {
    throw new Error("当前对话历史不是可追加用户消息的完整版本。");
  }
  return sequence;
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function turnAt(
  snapshot: QuickAiConversation,
  userSequence: number,
  prompt: string,
) {
  const userMessage = snapshot.messages.find(
    (message) => message.sequence === userSequence,
  );
  if (
    userMessage?.role !== "user" ||
    userMessage.content.trim() !== prompt.trim()
  ) {
    return null;
  }
  const assistantMessage = snapshot.messages.find(
    (message) => message.sequence === userSequence + 1,
  );
  return { userMessage, assistantMessage };
}

export class RepositoryConversationService implements ConversationService {
  readonly capabilities = COMPLETE_DELIVERY_CAPABILITIES;
  private readonly repository: ConversationRepository;
  private readonly onConversationUpdated?: () => void;

  constructor(
    repository: ConversationRepository,
    options: RepositoryConversationServiceOptions = {},
  ) {
    this.repository = repository;
    this.onConversationUpdated = options.onConversationUpdated;
  }

  async createConversation() {
    return mapQuickAiConversation(await this.repository.create());
  }

  async loadConversation(conversationId: string) {
    const snapshot = await this.repository.get(
      requireConversationId(conversationId),
    );
    if (!snapshot) {
      throw new Error("这个 Quick AI 会话不存在或已无法读取。");
    }
    return mapQuickAiConversation(snapshot);
  }

  async generateReply(request: ConversationGenerationRequest) {
    if (request.mode !== "append") {
      throw new Error("真实 Quick AI 暂不支持重新生成回答。");
    }

    const conversationId = requireConversationId(request.conversationId);
    const userSequence = expectedUserSequence(request);
    let snapshot: QuickAiConversation;
    let updateNotified = false;
    try {
      snapshot = await this.repository.send(
        conversationId,
        userSequence,
        request.prompt,
      );
    } catch (sendError) {
      this.onConversationUpdated?.();
      updateNotified = true;
      const recovered = await this.repository.get(conversationId).catch(() => null);
      if (!recovered) {
        throw sendError;
      }
      const recoveredTurn = turnAt(recovered, userSequence, request.prompt);
      if (!recoveredTurn) {
        throw sendError;
      }
      if (!recoveredTurn.assistantMessage) {
        const persistedThread = mapQuickAiConversation(recovered);
        return {
          status: "pending" as const,
          persistedThread,
          errorMessage: errorMessage(sendError),
        };
      }
      snapshot = recovered;
    }
    const persistedThread = mapQuickAiConversation(snapshot);
    const completedTurn = turnAt(snapshot, userSequence, request.prompt);
    const assistantMessage = completedTurn?.assistantMessage;
    if (assistantMessage?.role !== "assistant") {
      throw new Error("Quick AI 已返回会话，但目标轮次仍没有助手回答。");
    }

    if (!updateNotified) {
      this.onConversationUpdated?.();
    }
    return {
      status: "complete" as const,
      assistantMessageId: `quick-ai-message-${assistantMessage.id}`,
      chunks: [assistantMessage.content],
      persistedThread,
    };
  }

  async exportConversation(): Promise<ConversationExportResult> {
    return { exported: false };
  }
}
