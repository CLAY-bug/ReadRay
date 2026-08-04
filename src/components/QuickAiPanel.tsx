import {
  useEffect,
  useRef,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import type { QuickAiConversation } from "../types/quickAi";
import {
  shouldSendMultilineMessage,
  type SendShortcut,
} from "../appPreferences";

type QuickAiPanelProps = {
  open: boolean;
  conversation: QuickAiConversation | null;
  draft: string;
  pendingMessage?: string;
  loading: boolean;
  error?: string;
  onDraftChange: (value: string) => void;
  onSend: (value: string) => void;
  onNewConversation: () => void;
  onOpenChange: (open: boolean) => void;
  sendShortcut: SendShortcut;
};

function QuickAiPanel({
  open,
  conversation,
  draft,
  pendingMessage,
  loading,
  error,
  onDraftChange,
  onSend,
  onNewConversation,
  onOpenChange,
  sendShortcut,
}: QuickAiPanelProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const messageListRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      return;
    }

    window.requestAnimationFrame(() => {
      textareaRef.current?.focus({ preventScroll: true });
    });
  }, [open, conversation?.id]);

  useEffect(() => {
    const element = messageListRef.current;
    if (!element) {
      return;
    }
    element.scrollTop = element.scrollHeight;
  }, [conversation?.messages, pendingMessage]);

  useEffect(() => {
    if (!open) {
      return;
    }

    function handleWindowKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onOpenChange(false);
        return;
      }
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        onNewConversation();
      }
    }

    document.addEventListener("keydown", handleWindowKeyDown);
    return () => document.removeEventListener("keydown", handleWindowKeyDown);
  }, [onNewConversation, onOpenChange, open]);

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

  function handleWindowDrag(event: MouseEvent<HTMLElement>) {
    if (event.button !== 0) {
      return;
    }
    if (
      event.target instanceof HTMLElement &&
      event.target.closest("button, textarea, .quick-ai-panel__messages")
    ) {
      return;
    }

    event.preventDefault();
    invoke("begin_overlay_window_drag", {
      pointerX: event.screenX,
      pointerY: event.screenY,
    }).catch(() => undefined);

    function handleMouseMove(moveEvent: globalThis.MouseEvent) {
      invoke("drag_overlay_window", {
        pointerX: moveEvent.screenX,
        pointerY: moveEvent.screenY,
      }).catch(() => undefined);
    }

    function handleMouseUp() {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      invoke("finish_overlay_window_drag").catch(() => undefined);
    }

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  }

  if (!open) {
    return null;
  }

  const messages = conversation?.messages ?? [];

  return (
    <article
      className="quick-ai-panel"
      aria-label="Quick AI 对话"
      onMouseDown={handleWindowDrag}
    >
      <header className="quick-ai-panel__header">
        <div className="quick-ai-panel__identity">
          <strong>{conversation?.title || "New Chat"}</strong>
          <span>{conversation?.model || "DeepSeek Flash"}</span>
        </div>
        <button
          className="quick-ai-panel__new-chat"
          type="button"
          title="新建对话 (Ctrl+N)"
          aria-label="新建对话"
          onClick={onNewConversation}
        >
          +
        </button>
      </header>

      <div
        ref={messageListRef}
        className={`quick-ai-panel__messages${messages.length ? "" : " is-empty"}`}
        aria-live="polite"
      >
        {messages.length === 0 && !pendingMessage ? (
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
            <span>{message.role === "user" ? "你" : "AI"}</span>
            <p>{message.content}</p>
          </div>
        ))}

        {pendingMessage ? (
          <>
            <div className="quick-ai-panel__message is-user is-pending">
              <span>你</span>
              <p>{pendingMessage}</p>
            </div>
            <div className="quick-ai-panel__message is-assistant is-loading">
              <span>AI</span>
              <p>正在回复</p>
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
          ↗
        </button>
      </footer>
      {error ? <p className="quick-ai-panel__error">{error}</p> : null}
    </article>
  );
}

export default QuickAiPanel;
