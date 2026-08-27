import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { beginOverlayWindowDrag, overlayWindowDragCommands } from "../overlayWindowDrag";
import type {
  QuickAiConversation,
  RecentQuickAiConversation,
} from "../types/quickAi";
import { MarkdownContent } from "./MarkdownContent";
import {
  shouldSendMultilineMessage,
  type SendShortcut,
} from "../appPreferences";

function resizeComposer(input: HTMLTextAreaElement) {
  input.style.height = "auto";
  const maxHeight = Number.parseFloat(getComputedStyle(input).maxHeight);
  const contentHeight = input.scrollHeight;
  input.style.height = `${Math.min(contentHeight, maxHeight)}px`;
  input.style.overflowY = contentHeight > maxHeight ? "auto" : "hidden";
}

function formatHistoryTime(updatedAtUnixMs: number) {
  const updatedAt = new Date(updatedAtUnixMs);
  if (Number.isNaN(updatedAt.getTime())) {
    return "";
  }

  const now = new Date();
  const time = `${updatedAt.getHours().toString().padStart(2, "0")}:${updatedAt
    .getMinutes()
    .toString()
    .padStart(2, "0")}`;
  if (updatedAt.toDateString() === now.toDateString()) {
    return time;
  }

  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  if (updatedAt.toDateString() === yesterday.toDateString()) {
    return "昨天";
  }
  if (updatedAt.getFullYear() === now.getFullYear()) {
    return `${updatedAt.getMonth() + 1}/${updatedAt.getDate()}`;
  }
  return `${updatedAt.getFullYear()}/${updatedAt.getMonth() + 1}/${updatedAt.getDate()}`;
}

function getTitleInputSize(title: string) {
  const displayUnits = Array.from(title).reduce((total, character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return total + (codePoint <= 0xff ? 1 : 2);
  }, 0);
  return Math.min(48, Math.max(6, displayUnits + 1));
}

type QuickAiPanelProps = {
  open: boolean;
  page: "conversation" | "history";
  conversation: QuickAiConversation | null;
  recentConversations: RecentQuickAiConversation[];
  historyStatus: "idle" | "loading" | "ready" | "error";
  historyError?: string;
  allConversations: RecentQuickAiConversation[];
  allHistoryStatus: "idle" | "loading" | "ready" | "error";
  allHistoryError?: string;
  conversationLoading: boolean;
  renaming: boolean;
  draft: string;
  pendingMessage?: string;
  loading: boolean;
  error?: string;
  onDraftChange: (value: string) => void;
  onSend: (value: string) => void;
  onNewConversation: () => void;
  onHistoryRequest: () => void;
  onConversationSelect: (conversationId: number) => void;
  onRename: (title: string) => Promise<boolean>;
  onViewAllConversations: () => void;
  onAllHistoryRetry: () => void;
  onHistoryBack: () => void;
  onBack: () => void;
  sendShortcut: SendShortcut;
};

type MessageScrollState = {
  conversationId: number | null;
  scrollTop: number;
  atBottom: boolean;
};

function QuickAiPanel({
  open,
  page,
  conversation,
  recentConversations,
  historyStatus,
  historyError,
  allConversations,
  allHistoryStatus,
  allHistoryError,
  conversationLoading,
  renaming,
  draft,
  pendingMessage,
  loading,
  error,
  onDraftChange,
  onSend,
  onNewConversation,
  onHistoryRequest,
  onConversationSelect,
  onRename,
  onViewAllConversations,
  onAllHistoryRetry,
  onHistoryBack,
  onBack,
  sendShortcut,
}: QuickAiPanelProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const messageListRef = useRef<HTMLDivElement>(null);
  const historyMenuRef = useRef<HTMLElement>(null);
  const titleInputRef = useRef<HTMLInputElement>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [renameDraft, setRenameDraft] = useState<string | null>(null);
  const messageScrollStateRef = useRef<MessageScrollState>({
    conversationId: null,
    scrollTop: 0,
    atBottom: true,
  });

  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) {
      resizeComposer(textarea);
    }
  }, [draft, open]);

  useEffect(() => {
    if (!open) {
      setHistoryOpen(false);
      setRenameDraft(null);
      return;
    }

    if (page === "conversation") {
      window.requestAnimationFrame(() => {
        textareaRef.current?.focus({ preventScroll: true });
      });
    }
  }, [open, page, conversation?.id]);

  useEffect(() => {
    setRenameDraft(null);
  }, [conversation?.id]);

  useLayoutEffect(() => {
    const input = titleInputRef.current;
    if (renameDraft === null || !input || document.activeElement === input) {
      return;
    }
    input.focus({ preventScroll: true });
    input.select();
  }, [renameDraft]);

  useEffect(() => {
    if (!historyOpen) {
      return;
    }

    function handlePointerDown(event: PointerEvent) {
      if (
        event.target instanceof Node &&
        !historyMenuRef.current?.contains(event.target)
      ) {
        setHistoryOpen(false);
      }
    }

    document.addEventListener("pointerdown", handlePointerDown);
    return () => document.removeEventListener("pointerdown", handlePointerDown);
  }, [historyOpen]);

  useLayoutEffect(() => {
    if (!open) {
      return;
    }
    const element = messageListRef.current;
    if (!element) {
      return;
    }

    const saved = messageScrollStateRef.current;
    const conversationId = conversation?.id ?? null;
    const shouldStickToBottom =
      saved.conversationId !== conversationId || saved.atBottom;
    const frame = window.requestAnimationFrame(() => {
      element.scrollTop = shouldStickToBottom
        ? element.scrollHeight
        : Math.min(
            saved.scrollTop,
            Math.max(0, element.scrollHeight - element.clientHeight),
          );
      updateMessageScrollState(element);
    });

    return () => window.cancelAnimationFrame(frame);
  }, [conversation?.id, conversation?.messages, open, pendingMessage]);

  useEffect(() => {
    if (!open) {
      return;
    }

    function handleWindowKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        if (page === "history") {
          onHistoryBack();
        } else {
          onBack();
        }
        return;
      }
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        startNewConversation();
      }
    }

    document.addEventListener("keydown", handleWindowKeyDown);
    return () => document.removeEventListener("keydown", handleWindowKeyDown);
  }, [onBack, onHistoryBack, onNewConversation, open, page]);

  function updateMessageScrollState(element: HTMLDivElement) {
    const distanceFromBottom =
      element.scrollHeight - element.scrollTop - element.clientHeight;
    messageScrollStateRef.current = {
      conversationId: conversation?.id ?? null,
      scrollTop: element.scrollTop,
      atBottom: distanceFromBottom < 80,
    };
  }

  function submitDraft() {
    const message = draft.trim();
    if (!message || loading) {
      return;
    }
    onSend(message);
  }

  function handleTextareaKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (
      shouldSendMultilineMessage(
        {
          key: event.key,
          shiftKey: event.shiftKey,
          ctrlKey: event.ctrlKey,
          isComposing: event.nativeEvent.isComposing,
        },
        sendShortcut,
      )
    ) {
      event.preventDefault();
      submitDraft();
    }
  }

  async function saveTitleRename() {
    if (renameDraft === null || renaming) {
      return;
    }
    if (await onRename(renameDraft)) {
      setRenameDraft(null);
    }
  }

  function handleTitleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      setRenameDraft(null);
      return;
    }
    if (event.key === "Enter" && !event.nativeEvent.isComposing) {
      event.preventDefault();
      event.stopPropagation();
      void saveTitleRename();
    }
  }

  if (!open) {
    return null;
  }

  const messages = conversation?.messages ?? [];
  const conversationIsEmpty = messages.length === 0 && !pendingMessage;
  const hasActiveHistoryConversation = Boolean(conversation?.title?.trim());

  function toggleHistory() {
    const nextOpen = !historyOpen;
    setHistoryOpen(nextOpen);
    if (nextOpen) {
      onHistoryRequest();
    }
  }

  function startNewConversation() {
    setHistoryOpen(false);
    onNewConversation();
  }

  function selectConversation(conversationId: number) {
    setHistoryOpen(false);
    onConversationSelect(conversationId);
  }

  function viewAllConversations() {
    setHistoryOpen(false);
    onViewAllConversations();
  }

  if (page === "history") {
    return (
      <article
        className="quick-ai-panel quick-ai-panel__history-page"
        aria-label="全部 Quick AI 对话"
        onMouseDown={(event) =>
          beginOverlayWindowDrag(
            event,
            overlayWindowDragCommands,
            "button, input, textarea, .quick-ai-panel__messages, .quick-ai-panel__history-menu, .quick-ai-panel__history-page-body",
          )
        }
      >
        <header className="quick-ai-panel__header quick-ai-panel__history-page-header">
          <button
            className="quick-ai-panel__back"
            type="button"
            title="返回当前对话"
            aria-label="返回当前对话"
            onClick={onHistoryBack}
          >
            ←
          </button>
          <div className="quick-ai-panel__identity">
            <strong>全部 Quick AI 对话</strong>
            <span>
              {allHistoryStatus === "ready"
                ? `${allConversations.length} 个 Overlay 会话`
                : "仅显示 Overlay 会话"}
            </span>
          </div>
          <button
            className="quick-ai-panel__history-page-new"
            type="button"
            onClick={startNewConversation}
          >
            <span aria-hidden="true">+</span>
            新对话
          </button>
        </header>

        <div className="quick-ai-panel__history-page-body">
          {allHistoryStatus === "loading" || allHistoryStatus === "idle" ? (
            <div className="quick-ai-panel__history-page-state" role="status">
              正在读取 Quick AI 历史…
            </div>
          ) : allHistoryStatus === "error" ? (
            <div
              className="quick-ai-panel__history-page-state is-error"
              role="alert"
            >
              <strong>暂时无法读取历史</strong>
              <p>{allHistoryError || "请稍后重试。"}</p>
              <button type="button" onClick={onAllHistoryRetry}>
                重试
              </button>
            </div>
          ) : allConversations.length === 0 ? (
            <div className="quick-ai-panel__history-page-state">
              <strong>还没有 Quick AI 历史</strong>
              <p>在 Overlay 中完成一次提问后，会话会出现在这里。</p>
            </div>
          ) : (
            <div className="quick-ai-panel__history-page-list">
              {allConversations.map((item) => {
                const active = conversation?.id === item.id;
                return (
                  <button
                    className={`quick-ai-panel__history-page-item${
                      active ? " is-active" : ""
                    }`}
                    type="button"
                    key={item.id}
                    aria-current={active ? "page" : undefined}
                    disabled={conversationLoading}
                    onClick={() => selectConversation(item.id)}
                  >
                    <span className="quick-ai-panel__history-page-item-copy">
                      <span className="quick-ai-panel__history-active-dot" />
                      <strong>{item.title}</strong>
                    </span>
                    <time>{formatHistoryTime(item.updatedAtUnixMs)}</time>
                    <span aria-hidden="true">→</span>
                  </button>
                );
              })}
            </div>
          )}
        </div>
        {error ? <p className="quick-ai-panel__error">{error}</p> : null}
      </article>
    );
  }

  return (
    <article
      className="quick-ai-panel"
      aria-label="Quick AI 对话"
      onMouseDown={(event) =>
        beginOverlayWindowDrag(
          event,
          overlayWindowDragCommands,
          "button, input, textarea, .quick-ai-panel__messages, .quick-ai-panel__history-menu, .quick-ai-panel__history-page-body",
        )
      }
    >
      <header className="quick-ai-panel__header">
        <button
          className="quick-ai-panel__back"
          type="button"
          title="返回搜索"
          aria-label="返回搜索"
          onClick={onBack}
        >
          ←
        </button>
        <div className="quick-ai-panel__identity">
          {renameDraft === null ? (
            <button
              className="quick-ai-panel__title-button"
              type="button"
              title={conversation?.title ? "重命名对话" : undefined}
              disabled={!conversation?.title || loading || renaming}
              onClick={() => setRenameDraft(conversation?.title ?? null)}
            >
              <strong>{conversation?.title || "New Chat"}</strong>
            </button>
          ) : (
            <input
              ref={titleInputRef}
              className="quick-ai-panel__title-input"
              value={renameDraft}
              size={getTitleInputSize(renameDraft)}
              maxLength={80}
              aria-label="会话名称"
              aria-busy={renaming}
              readOnly={renaming}
              onChange={(event) => setRenameDraft(event.target.value)}
              onKeyDown={handleTitleKeyDown}
              onBlur={() => {
                if (!renaming) {
                  setRenameDraft(null);
                }
              }}
            />
          )}
          <span>{conversation?.model || "DeepSeek Flash"}</span>
        </div>
        <button
          className="quick-ai-panel__history"
          type="button"
          title="对话历史"
          aria-label="打开对话历史"
          aria-expanded={historyOpen}
          aria-controls="quick-ai-history-menu"
          onClick={toggleHistory}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
            <path d="M3 3v5h5" />
            <path d="M12 7v5l3 2" />
          </svg>
        </button>
        {historyOpen ? (
          <section
            ref={historyMenuRef}
            className="quick-ai-panel__history-menu"
            id="quick-ai-history-menu"
            aria-label="Quick AI 对话历史"
          >
            <button
              className={`quick-ai-panel__history-new${
                hasActiveHistoryConversation ? "" : " is-active"
              }`}
              type="button"
              aria-current={hasActiveHistoryConversation ? undefined : "page"}
              onClick={startNewConversation}
            >
              <span className="quick-ai-panel__history-new-icon">+</span>
              <span>新对话</span>
              <kbd>Ctrl N</kbd>
            </button>
            <div className="quick-ai-panel__history-heading">最近对话</div>
            {historyStatus === "loading" || historyStatus === "idle" ? (
              <p className="quick-ai-panel__history-state">正在读取…</p>
            ) : historyStatus === "error" ? (
              <div className="quick-ai-panel__history-state is-error">
                <p>{historyError || "最近对话读取失败。"}</p>
                <button type="button" onClick={onHistoryRequest}>
                  重试
                </button>
              </div>
            ) : recentConversations.length === 0 ? (
              <p className="quick-ai-panel__history-state">暂无历史对话</p>
            ) : (
              <div className="quick-ai-panel__history-list">
                {recentConversations.map((item) => {
                  const active = conversation?.id === item.id;
                  return (
                    <button
                      className={`quick-ai-panel__history-item${
                        active ? " is-active" : ""
                      }`}
                      type="button"
                      key={item.id}
                      aria-current={active ? "page" : undefined}
                      disabled={conversationLoading}
                      onClick={() => selectConversation(item.id)}
                    >
                      <span className="quick-ai-panel__history-title">
                        <span className="quick-ai-panel__history-active-dot" />
                        <span>{item.title}</span>
                      </span>
                      <time>
                        {formatHistoryTime(item.updatedAtUnixMs)}
                      </time>
                    </button>
                  );
                })}
              </div>
            )}
            <button
              className="quick-ai-panel__history-view-all"
              type="button"
              onClick={viewAllConversations}
            >
              查看全部对话
              <span aria-hidden="true">→</span>
            </button>
          </section>
        ) : null}
      </header>

      <div
        ref={messageListRef}
        className={`quick-ai-panel__messages${conversationIsEmpty ? " is-empty" : ""}`}
        aria-live="polite"
        onScroll={(event) => updateMessageScrollState(event.currentTarget)}
      >
        {conversationIsEmpty ? (
          <div className="quick-ai-panel__empty">
            <strong>Quick AI</strong>
            <span>直接输入问题开始对话</span>
          </div>
        ) : null}

        {messages.map((message) => (
          <div
            className={`quick-ai-panel__message is-${message.role}`}
            key={message.id}
          >
            {message.role === "user" ? (
              <p>{message.content}</p>
            ) : (
              <div className="quick-ai-panel__markdown">
                <MarkdownContent text={message.content} />
              </div>
            )}
          </div>
        ))}

        {pendingMessage ? (
          <>
            <div className="quick-ai-panel__message is-user is-pending">
              <p>{pendingMessage}</p>
            </div>
            <div className="quick-ai-panel__message is-assistant is-loading">
              <p
                className="quick-ai-panel__thinking"
                role="status"
                aria-label="AI 正在思考"
              >
                <span
                  className="quick-ai-panel__thinking-label"
                  data-text="Thinking…"
                  aria-hidden="true"
                >
                  Thinking…
                </span>
              </p>
            </div>
          </>
        ) : null}
      </div>

      <footer className="quick-ai-panel__composer">
        <textarea
          ref={textareaRef}
          value={draft}
          rows={1}
          placeholder="Ask anything..."
          aria-label="Quick AI 消息"
          disabled={loading}
          onChange={(event) => onDraftChange(event.target.value)}
          onKeyDown={handleTextareaKeyDown}
        />
        <button
          className="quick-ai-panel__send"
          type="button"
          title="发送"
          aria-label="发送消息"
          disabled={!draft.trim() || loading}
          onClick={submitDraft}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 19V5" />
            <path d="m6.5 10.5 5.5-5.5 5.5 5.5" />
          </svg>
        </button>
      </footer>
      {error ? <p className="quick-ai-panel__error">{error}</p> : null}
    </article>
  );
}

export default QuickAiPanel;
