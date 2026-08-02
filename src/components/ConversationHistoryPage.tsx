import { useEffect, useState, type MouseEvent } from "react";
import type {
  ConversationService,
  ConversationSummary,
} from "../conversationViewModel";

type ConversationHistoryPageProps = {
  service: ConversationService;
  refreshToken: number;
  onOpenConversation: (conversation: ConversationSummary) => void;
  onConversationContextMenu: (
    conversation: ConversationSummary,
    event: MouseEvent<HTMLElement>,
  ) => void;
};

function formatUpdatedAt(value: number, now = new Date()) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "时间未知";
  }
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  if (sameDay) {
    return `今天 ${date.getHours().toString().padStart(2, "0")}:${date
      .getMinutes()
      .toString()
      .padStart(2, "0")}`;
  }
  return `${date.getFullYear()}-${(date.getMonth() + 1)
    .toString()
    .padStart(2, "0")}-${date.getDate().toString().padStart(2, "0")}`;
}

function ConversationHistoryPage({
  service,
  refreshToken,
  onOpenConversation,
  onConversationContextMenu,
}: ConversationHistoryPageProps) {
  const [status, setStatus] = useState<"loading" | "ready" | "error">(
    "loading",
  );
  const [error, setError] = useState("");
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [retryToken, setRetryToken] = useState(0);

  useEffect(() => {
    let ignore = false;
    setStatus("loading");
    setError("");
    service.listConversations().then(
      (items) => {
        if (!ignore) {
          setConversations(items);
          setStatus("ready");
        }
      },
      (loadError) => {
        if (!ignore) {
          setError(
            loadError instanceof Error ? loadError.message : String(loadError),
          );
          setStatus("error");
        }
      },
    );
    return () => {
      ignore = true;
    };
  }, [refreshToken, retryToken, service]);

  return (
    <main className="rr-main-panel rr-conversation-history" aria-label="全部对话">
      <div className="rr-conversation-history-inner">
        <header className="rr-conversation-history-heading">
          <div>
            <h1>全部对话</h1>
            <p>这些会话来自本机 ReadRay SQLite 数据库。</p>
          </div>
          {status === "ready" ? <span>{conversations.length} 个会话</span> : null}
        </header>

        {status === "loading" ? (
          <div className="rr-conversation-history-state" role="status">
            正在读取对话历史…
          </div>
        ) : status === "error" ? (
          <div className="rr-conversation-history-state is-error" role="alert">
            <strong>暂时无法读取对话历史</strong>
            <p>{error || "请稍后重试。"}</p>
            <button type="button" onClick={() => setRetryToken((value) => value + 1)}>
              重试
            </button>
          </div>
        ) : conversations.length === 0 ? (
          <div className="rr-conversation-history-state">
            <strong>还没有可管理的对话</strong>
            <p>完成一次 Quick AI 提问后，会话会出现在这里。</p>
          </div>
        ) : (
          <div className="rr-conversation-history-list">
            {conversations.map((conversation) => (
              <article
                className="rr-conversation-history-row"
                key={conversation.id}
                onContextMenu={(event) =>
                  onConversationContextMenu(conversation, event)
                }
              >
                <button
                  className="rr-conversation-history-open"
                  type="button"
                  onClick={() => onOpenConversation(conversation)}
                >
                  <strong>{conversation.title}</strong>
                  <span>{formatUpdatedAt(conversation.updatedAtUnixMs)}</span>
                </button>
              </article>
            ))}
          </div>
        )}
      </div>

    </main>
  );
}

export default ConversationHistoryPage;
