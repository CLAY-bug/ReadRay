import type { ConversationRepository } from "./conversationRepository.ts";
import type {
  ConversationAssistantMessage,
  ConversationExportResult,
  ConversationGenerationRequest,
  ConversationMessage,
  ConversationService,
  ConversationSummary,
  ConversationThread,
  ConversationUserMessage,
} from "./conversationViewModel.ts";
import type {
  QuickAiConversation,
  QuickAiMessage,
} from "./types/quickAi.ts";

const STREAMING_DELIVERY_CAPABILITIES = {
  delivery: "streaming",
  canStop: true,
  canRegenerate: true,
  canExport: true,
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
  if (!["overlay", "main"].includes(snapshot.origin)) {
    throw new Error("Quick AI 返回了无效的会话来源。");
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
    // 真实回答保留原始 Markdown 文本，页面渲染层优先走白名单渲染；
    // blocks 字段继续存在以兼容既有协议与 fixture 路径。
    markdown: message.content,
    // 任务 4：来源随 assistant 消息落库，重启与历史回看直接可用；
    // 截断标志驱动"回答可能不完整"的轻微提示。
    sources: message.sources && message.sources.length > 0
      ? message.sources
      : undefined,
    truncated: message.truncated === true,
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
  // 上一条历史消息仍是 user（上一轮 pending 未完成，失败/中断）时，新消息
  // 开启新轮次并跳过待完成的 assistant 位置（previousSequence + 2）；
  // 否则正常连续追加（previousSequence + 1）。两种结果都必须保持奇数。
  const previousIsPendingUser =
    request.messages[request.messages.length - 2]?.role === "user";
  const sequence = previousSequence + (previousIsPendingUser ? 2 : 1);
  if (sequence % 2 === 0) {
    throw new Error("当前对话历史不是可追加用户消息的完整版本。");
  }
  return sequence;
}

function exportFileName(title: string) {
  const safeTitle = title
    .trim()
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "-")
    .replace(/[. ]+$/g, "")
    .slice(0, 64);
  return `${safeTitle || "ReadRay-对话"}.md`;
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
  // 当前回答 = 该 user 之后最后一条 assistant。重新生成后 sequence 存在间隙
  // （旧回答 seq+1 被替代、新回答在更后的偶数位），不能假设恰好位于
  // userSequence + 1；被替代的旧回答已被 Rust 快照过滤。
  const assistantMessage = [...snapshot.messages]
    .reverse()
    .find(
      (message) => message.role === "assistant" && message.sequence > userSequence,
    );
  return { userMessage, assistantMessage };
}

function parseReplaceMessageId(messageId: string | undefined) {
  if (!messageId) {
    return null;
  }
  const match = /^quick-ai-message-(\d+)$/.exec(messageId);
  if (!match) {
    throw new Error("重新生成目标回答 ID 无效。");
  }
  const id = Number(match[1]);
  if (!Number.isSafeInteger(id) || id <= 0) {
    throw new Error("重新生成目标回答 ID 无效。");
  }
  return id;
}

export class RepositoryConversationService implements ConversationService {
  readonly capabilities = STREAMING_DELIVERY_CAPABILITIES;
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

  async listConversations(): Promise<ConversationSummary[]> {
    const snapshots = await this.repository.list();
    return snapshots.map((snapshot) => {
      if (
        !Number.isSafeInteger(snapshot.id) ||
        snapshot.id <= 0 ||
        !snapshot.title.trim() ||
        !["overlay", "main"].includes(snapshot.origin) ||
        !Number.isFinite(snapshot.updatedAtUnixMs)
      ) {
        throw new Error("Quick AI 返回了无效的会话摘要。");
      }
      return {
        id: String(snapshot.id),
        title: snapshot.title.trim(),
        origin: snapshot.origin,
        updatedAtUnixMs: snapshot.updatedAtUnixMs,
      };
    });
  }

  async renameConversation(conversationId: string, title: string) {
    const normalizedTitle = title.trim();
    if (!normalizedTitle) {
      throw new Error("会话名称不能为空。");
    }
    const renamed = mapQuickAiConversation(
      await this.repository.rename(
        requireConversationId(conversationId),
        normalizedTitle,
      ),
    );
    this.onConversationUpdated?.();
    return renamed;
  }

  async deleteConversation(conversationId: string) {
    const deleted = await this.repository.delete(
      requireConversationId(conversationId),
    );
    if (!deleted) {
      throw new Error("这个 Quick AI 会话不存在或已经被删除。");
    }
    this.onConversationUpdated?.();
  }

  async generateReply(request: ConversationGenerationRequest) {
    const conversationId = requireConversationId(request.conversationId);
    const replaceMessageId = parseReplaceMessageId(
      request.replaceAssistantMessageId,
    );
    if (request.mode === "regenerate" && replaceMessageId === null) {
      throw new Error("重新生成必须指定目标回答。");
    }
    if (request.mode === "append" && replaceMessageId !== null) {
      throw new Error("追加回答不能携带重新生成目标。");
    }
    const userSequence = expectedUserSequence(request);

    let snapshot: QuickAiConversation;
    let updateNotified = false;
    let streamed = false;
    let streamedText = "";
    let stoppedByUser = false;
    let eventTruncated = false;
    try {
      snapshot = await this.repository.sendStreaming(
        conversationId,
        userSequence,
        request.prompt,
        (event) => {
          if (event.type === "delta") {
            streamed = true;
            streamedText += event.text;
            request.onStreamDelta?.(event.text);
          } else if (event.type === "stopped") {
            stoppedByUser = true;
          } else if (event.type === "truncated") {
            eventTruncated = true;
          } else if (event.type === "sources_updated") {
            request.onSourcesUpdated?.(event.sources);
          } else if (event.type === "tool_state") {
            request.onToolState?.(event.label);
          }
        },
        replaceMessageId,
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
          errorMessage: stoppedByUser
            ? "回答已停止，已保留你的问题，可以直接重试。"
            : "暂时无法回答，请重试。",
        };
      }
      if (
        replaceMessageId !== null &&
        `quick-ai-message-${recoveredTurn.assistantMessage.id}` ===
          request.replaceAssistantMessageId
      ) {
        // 重新生成未提交：目标回答仍是当前回答（旧回答未被替代），保持失败可重试。
        const persistedThread = mapQuickAiConversation(recovered);
        return {
          status: "pending" as const,
          persistedThread,
          errorMessage: "暂时无法回答，请重试。",
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
    // 任务 4：finish_reason=length 的截断以落库标志为准（历史回看一致）；
    // 事件标志保留兼容旧路径。
    const truncated = eventTruncated || assistantMessage.truncated === true;
    return {
      status: truncated ? ("truncated" as const) : ("complete" as const),
      assistantMessageId: `quick-ai-message-${assistantMessage.id}`,
      chunks: streamed ? [streamedText] : [assistantMessage.content],
      persistedThread,
    };
  }

  async stopGeneration(conversationId: string) {
    await this.repository.abortStreaming(
      requireConversationId(conversationId),
    );
  }

  async openSource(url: string): Promise<void> {
    await this.repository.openSource(url);
  }

  async exportConversation(
    thread: ConversationThread,
  ): Promise<ConversationExportResult> {
    if (thread.messages.length === 0) {
      return { exported: false, reason: "unavailable" };
    }
    const conversationId = requireConversationId(thread.id);
    const result = await this.repository.export(
      conversationId,
      exportFileName(thread.title),
    );
    if (!result) {
      return { exported: false, reason: "cancelled" };
    }
    if (
      result.conversationId !== conversationId ||
      !result.fileName.trim() ||
      !result.filePath.trim() ||
      !Number.isSafeInteger(result.messageCount) ||
      result.messageCount <= 0
    ) {
      throw new Error("Quick AI 返回了无效的导出结果。");
    }
    return {
      exported: true,
      fileName: result.fileName,
      messageCount: result.messageCount,
      nativeFilePath: result.filePath,
    };
  }
}
