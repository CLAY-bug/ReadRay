import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type FormEvent, type ReactNode } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type {
  AgentSource,
  ConversationAssistantMessage,
  ConversationOperationIdentity,
  ConversationInline,
  ConversationMessage,
  ConversationMemoryCitation,
  ConversationRequest,
  ConversationService,
  ConversationThread,
  ConversationUserMessage,
} from "../conversationViewModel";
import {
  conversationExportUnavailableReason,
  conversationTitleEditAction,
  isConversationOperationCurrent,
} from "../conversationViewModel";
import { deliverConversationExport } from "../conversationExportDelivery";
import MainAppIcon from "./MainAppIcon";
import { MarkdownContent } from "./MarkdownContent";
import { AgentSourceList } from "./AgentSourceList";
import {
  shouldSendMultilineMessage,
  type SendShortcut,
} from "../appPreferences";
import { useAutoResizeTextarea } from "./useAutoResizeTextarea";

type ConversationPageProps = {
  request: ConversationRequest;
  service: ConversationService;
  onThreadIdentityChange?: (conversationId: string) => void;
  onConversationDeleted?: (
    operation: ConversationOperationIdentity,
  ) => void;
  externalTitleUpdate?: {
    key: number;
    conversationId: string;
    title: string;
  };
  sendShortcut: SendShortcut;
};

type GenerationState = {
  phase: "generating" | "complete" | "stopped" | "truncated" | "failed";
  prompt: string;
  text: string;
  errorMessage?: string;
  chunks: string[];
  nextChunkIndex: number;
  assistantMessageId: string;
  userMessageId: string;
  mode: "append" | "regenerate";
  replaceAssistantMessageId?: string;
  retryKind: "request" | "generation";
  /** 本轮 Agent 工具来源（任务 3）：按 sourceId 去重累积。 */
  sources: AgentSource[];
  /** 当前工具状态文案（任务 3）："正在搜索相关资料…"等。 */
  toolLabel?: string;
};

const USER_CLAMP_LINES = 5;
const CONVERSATION_PIXEL_DELAYS = [
  90, 180, 270,
  0, 90, 180,
  90, 180, 270,
];

function InlineContent({ content }: { content: ConversationInline[] }) {
  return content.map((item, index) => {
    const key = `${item.kind}-${index}`;
    if (item.kind === "code") {
      return <code key={key}>{item.text}</code>;
    }
    if (item.kind === "strong") {
      return <strong key={key}>{item.text}</strong>;
    }
    return <span key={key}>{item.text}</span>;
  });
}

function UserMessage({ message }: { message: ConversationUserMessage }) {
  const copyRef = useRef<HTMLDivElement>(null);
  const [isOverflowing, setIsOverflowing] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const copyId = useId();

  useLayoutEffect(() => {
    const copy = copyRef.current;
    if (!copy) {
      return;
    }

    const measure = () => {
      const lineHeight = Number.parseFloat(getComputedStyle(copy).lineHeight) || 26;
      const maxHeight = Math.round(lineHeight * USER_CLAMP_LINES);
      const nextOverflowing = copy.scrollHeight > maxHeight + 1;
      setIsOverflowing(nextOverflowing);
      if (!nextOverflowing) {
        setExpanded(false);
      }
    };

    measure();
    void document.fonts.ready.then(measure);
    // 窗口缩放期间每条消息都会触发观察回调；settle 后统一测量，避免逐帧强制布局。
    let measureTimer: number | undefined;
    const measureAfterSettles = () => {
      window.clearTimeout(measureTimer);
      measureTimer = window.setTimeout(measure, 120);
    };
    const observer = new ResizeObserver(measureAfterSettles);
    observer.observe(copy);
    return () => {
      observer.disconnect();
      window.clearTimeout(measureTimer);
    };
  }, [message.content]);

  const lineHeight = copyRef.current
    ? Number.parseFloat(getComputedStyle(copyRef.current).lineHeight) || 26
    : 26;
  const collapsed = isOverflowing && !expanded;

  return (
    <article className="rr-conversation-message is-user">
      <div
        className={`rr-conversation-user-bubble${collapsed ? " is-collapsed" : ""}`}
      >
        <div
          ref={copyRef}
          id={copyId}
          className="rr-conversation-user-copy"
          style={
            collapsed
              ? ({ maxHeight: Math.round(lineHeight * USER_CLAMP_LINES) } as CSSProperties)
              : undefined
          }
        >
          {message.content}
        </div>
        {isOverflowing ? (
          <button
            className="rr-conversation-user-expand"
            type="button"
            aria-controls={copyId}
            aria-expanded={expanded}
            onClick={() => setExpanded((value) => !value)}
          >
            {expanded ? "收起" : "展开"}
          </button>
        ) : null}
      </div>
      {message.meta ? (
        <div className="rr-conversation-user-meta">{message.meta}</div>
      ) : null}
    </article>
  );
}

function renderAnswerText(
  message: ConversationAssistantMessage,
): string {
  if (message.markdown !== undefined) {
    return message.markdown;
  }
  return message.blocks
    .map((block) => {
      if (block.kind === "paragraph") {
        return block.content.map((inline) => inline.text).join("");
      }
      if (block.kind === "list") {
        return block.items
          .map((item) => `- ${item.map((inline) => inline.text).join("")}`)
          .join("\n");
      }
      return `${block.english}\n${block.translation}`;
    })
    .join("\n\n");
}

function AssistantMessage({
  message,
  onOpenMemory,
  onOpenSource,
}: {
  message: ConversationAssistantMessage;
  onOpenMemory: (
    citation: ConversationMemoryCitation,
    trigger: HTMLButtonElement,
  ) => void;
  onOpenSource: (source: AgentSource) => void;
}) {
  const [copied, setCopied] = useState(false);
  const copyTimerRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    return () => {
      if (copyTimerRef.current !== undefined) {
        window.clearTimeout(copyTimerRef.current);
      }
    };
  }, []);

  const handleCopy = async () => {
    const text = renderAnswerText(message).trim();
    if (!text) {
      return;
    }
    try {
      await writeText(text);
      setCopied(true);
      if (copyTimerRef.current !== undefined) {
        window.clearTimeout(copyTimerRef.current);
      }
      copyTimerRef.current = window.setTimeout(() => setCopied(false), 1600);
    } catch (error) {
      console.error("ReadRay 复制回答失败：", error);
    }
  };

  return (
    <article className="rr-conversation-message is-assistant">
      <div className="rr-conversation-assistant-copy">
        {message.markdown !== undefined ? (
          <MarkdownContent text={message.markdown} />
        ) : (
          message.blocks.map((block, index) => {
            const key = `${block.kind}-${index}`;
            if (block.kind === "list") {
              return (
                <ul className="rr-conversation-answer-list" key={key}>
                  {block.items.map((item, itemIndex) => (
                    <li key={`${key}-${itemIndex}`}>
                      <InlineContent content={item} />
                    </li>
                  ))}
                </ul>
              );
            }
            if (block.kind === "example") {
              return (
                <p className="rr-conversation-example" lang="en" key={key}>
                  {block.english}
                  <span className="rr-conversation-translation">
                    {block.translation}
                  </span>
                </p>
              );
            }
            return (
              <p
                className={block.tone === "lead" ? "rr-conversation-answer-lead" : undefined}
                key={key}
              >
                <InlineContent content={block.content} />
              </p>
            );
          })
        )}
        {message.citation ? (
          <div className="rr-conversation-answer-footnote">
            <button
              className="rr-conversation-citation"
              type="button"
              onClick={(event) =>
                onOpenMemory(message.citation!, event.currentTarget)
              }
            >
              <span className="rr-conversation-memory-dot" />
              来自记忆：{message.citation.title}
            </button>
          </div>
        ) : null}
        {message.sources && message.sources.length > 0 ? (
          <div className="rr-conversation-answer-footnote">
            <AgentSourceList
              sources={message.sources}
              onOpen={onOpenSource}
            />
          </div>
        ) : null}
      </div>
      <button
        className="rr-conversation-assistant-copy-button"
        type="button"
        aria-label={copied ? "已复制" : "复制回答"}
        title={copied ? "已复制" : "复制回答"}
        onClick={() => void handleCopy()}
      >
        {copied ? (
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.1"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d="m5 12 4 4L19 6" />
          </svg>
        ) : (
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <rect x="9" y="9" width="12" height="12" rx="2.5" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </svg>
        )}
      </button>
    </article>
  );
}

function EmptyConversation() {
  return (
    <div className="rr-conversation-empty">
      <div className="rr-conversation-empty-copy">
        <h2>开始一段新对话</h2>
        <p>可以问一个单词、一处用法，或把刚才没说清的问题继续写下来。</p>
      </div>
    </div>
  );
}

function formatGenerationElapsed(elapsedMs: number) {
  const totalSeconds = Math.max(0, elapsedMs) / 1000;
  if (totalSeconds < 60) {
    return `${totalSeconds.toFixed(1)}s`;
  }
  return `${Math.floor(totalSeconds / 60)}m ${(totalSeconds % 60).toFixed(1)}s`;
}

function ConversationGenerationIndicator({
  onStop,
  label,
}: {
  onStop?: () => void;
  label?: string;
}) {
  const [elapsedMs, setElapsedMs] = useState(0);

  useEffect(() => {
    const startedAt = Date.now();
    const updateElapsed = () => setElapsedMs(Date.now() - startedAt);
    updateElapsed();
    const timer = window.setInterval(updateElapsed, 100);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <div className="rr-conversation-generation-indicator">
      <span className="rr-conversation-pixel-grid" aria-hidden="true">
        {CONVERSATION_PIXEL_DELAYS.map((delay, index) => (
          <span
            className="rr-conversation-pixel"
            key={index}
            style={{ animationDelay: `${delay}ms` }}
          />
        ))}
      </span>
      <span className="rr-conversation-generation-label" aria-live="polite">
        {label ?? "正在生成"}
      </span>
      <span
        className="rr-conversation-generation-elapsed"
        aria-hidden="true"
      >
        {formatGenerationElapsed(elapsedMs)}
      </span>
      {onStop ? (
        <button
          className="rr-conversation-stop-button"
          type="button"
          onClick={onStop}
        >
          停止生成
        </button>
      ) : null}
    </div>
  );
}

function GenerationMessage({
  state,
  canStop,
  onStop,
  onRetry,
  onOpenSource,
}: {
  state: GenerationState;
  canStop: boolean;
  onStop: () => void;
  onRetry: () => void;
  onOpenSource: (source: AgentSource) => void;
}) {
  const sourcesBlock =
    state.sources.length > 0 ? (
      <div className="rr-conversation-answer-footnote">
        <AgentSourceList sources={state.sources} onOpen={onOpenSource} />
      </div>
    ) : null;

  if (state.phase === "failed") {
    return (
      <article className="rr-conversation-message is-assistant">
        <div className="rr-conversation-assistant-copy">
          {state.text ? <MarkdownContent text={state.text} /> : null}
          {sourcesBlock}
          <div className="rr-conversation-answer-kicker">生成中断</div>
          <p className="rr-conversation-error-copy">
            {state.errorMessage
              ? `${state.errorMessage} 你的输入仍然保留，可以直接重试。`
              : "暂时无法完成回答。你的输入仍然保留，可以直接重试。"}
          </p>
          <button
            className="rr-conversation-quiet-button"
            type="button"
            onClick={onRetry}
          >
            重试
          </button>
        </div>
      </article>
    );
  }

  return (
    <article className="rr-conversation-message is-assistant">
      <div className="rr-conversation-assistant-copy">
        {state.text ? <MarkdownContent text={state.text} streaming /> : null}
        {sourcesBlock}
        {state.phase === "generating" ? (
          <ConversationGenerationIndicator
            onStop={canStop ? onStop : undefined}
            label={state.toolLabel}
          />
        ) : state.phase === "truncated" ? (
          <div className="rr-conversation-generation-row">
            <span className="rr-conversation-message-meta">
              回答达到长度上限被截断
            </span>
            <button
              className="rr-conversation-quiet-button"
              type="button"
              onClick={onRetry}
            >
              继续生成
            </button>
          </div>
        ) : state.phase === "stopped" ? (
          <div className="rr-conversation-generation-row">
            <span className="rr-conversation-message-meta">已停止</span>
            <button
              className="rr-conversation-quiet-button"
              type="button"
              onClick={onRetry}
            >
              继续生成
            </button>
          </div>
        ) : null}
      </div>
    </article>
  );
}

function ConversationPage({
  request,
  service,
  onThreadIdentityChange,
  onConversationDeleted,
  externalTitleUpdate,
  sendShortcut,
}: ConversationPageProps) {
  const [thread, setThread] = useState<ConversationThread | null>(null);
  const [draft, setDraft] = useState("");
  const [generation, setGeneration] = useState<GenerationState | null>(null);
  const [requestRetryToken, setRequestRetryToken] = useState(0);
  const [lastExportedMessageCount, setLastExportedMessageCount] = useState(0);
  const [menuOpen, setMenuOpen] = useState(false);
  const [renameDraft, setRenameDraft] = useState<string | null>(null);
  const [deleteConfirmationOpen, setDeleteConfirmationOpen] = useState(false);
  const [managementBusy, setManagementBusy] = useState<
    "rename" | "delete" | null
  >(null);
  const [managementError, setManagementError] = useState("");
  const [drawerCitation, setDrawerCitation] =
    useState<ConversationMemoryCitation | null>(null);
  const [toast, setToast] = useState("");
  const [showScrollToBottom, setShowScrollToBottom] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const drawerRef = useRef<HTMLElement>(null);
  const drawerCloseRef = useRef<HTMLButtonElement>(null);
  const citationTriggerRef = useRef<HTMLButtonElement | null>(null);
  const threadRef = useRef<ConversationThread | null>(null);
  const mountedRef = useRef(true);
  const requestKeyRef = useRef(request.key);
  const timerRef = useRef<number | undefined>(undefined);
  const generationTokenRef = useRef(0);
  const generationSourcesRef = useRef<AgentSource[]>([]);
  const generationToolLabelRef = useRef<string | undefined>(undefined);
  const toastTimerRef = useRef<number | undefined>(undefined);
  const messageIdRef = useRef(0);
  const scrollAnimationRef = useRef<number | undefined>(undefined);
  const scrollPositionsRef = useRef<Map<string, number>>(new Map());
  const conversationCreationRef = useRef<{
    requestKey: number;
    service: ConversationService;
    promise: Promise<ConversationThread>;
  } | null>(null);
  requestKeyRef.current = request.key;

  const updateScrollToBottomVisibility = useCallback(() => {
    const scroll = scrollRef.current;
    if (!scroll) {
      setShowScrollToBottom(false);
      return;
    }

    const distanceFromBottom =
      scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight;
    // 滞回：向上滚动超过 96px 才显示，回到距底部 24px 以内才隐藏；
    // 中间区间保持当前状态，避免在临界值附近反复闪现。
    // 隐藏阈值较小，保证点击按钮后的平滑滚动到达末尾时按钮才消失。
    if (distanceFromBottom > 96) {
      setShowScrollToBottom(true);
      return;
    }
    if (distanceFromBottom < 24) {
      setShowScrollToBottom(false);
    }
  }, []);

  const scrollToBottom = useCallback(() => {
    const scroll = scrollRef.current;
    if (!scroll) {
      return;
    }

    // 手动缓动滚动到底部：不依赖系统/浏览器对 smooth 的支持
    // （Windows"减少动态效果"会让 behavior:"smooth" 退化为直接跳转），
    // 用 rAF 逐帧逼近目标，达到类似真实鼠标滚动的丝滑感。
    if (scrollAnimationRef.current !== undefined) {
      cancelAnimationFrame(scrollAnimationRef.current);
      scrollAnimationRef.current = undefined;
    }
    const startTop = scroll.scrollTop;
    const targetTop = scroll.scrollHeight;
    const distance = targetTop - startTop;
    if (distance <= 0) {
      return;
    }
    const duration = 320;
    const startedAt = performance.now();
    let lastAnimatedTop = startTop;
    const step = (now: number) => {
      // 用户手动滚动会打断动画：当前 scrollTop 与动画预期值出现偏差。
      // 不能依赖 scroll 事件判断——程序化设置 scrollTop 也会触发 scroll 事件。
      if (Math.abs(scroll.scrollTop - lastAnimatedTop) > 2) {
        scrollAnimationRef.current = undefined;
        return;
      }
      const progress = Math.min((now - startedAt) / duration, 1);
      const eased = 1 - Math.pow(1 - progress, 3);
      const nextTop = startTop + distance * eased;
      lastAnimatedTop = nextTop;
      scroll.scrollTop = nextTop;
      if (progress < 1) {
        scrollAnimationRef.current = requestAnimationFrame(step);
      } else {
        scrollAnimationRef.current = undefined;
      }
    };
    scrollAnimationRef.current = requestAnimationFrame(step);
  }, []);

  const operationIsCurrent = useCallback(
    (operation: ConversationOperationIdentity) =>
      isConversationOperationCurrent(
        mountedRef.current,
        operation,
        requestKeyRef.current,
        threadRef.current?.id,
      ),
    [],
  );

  const stopTimer = useCallback(() => {
    if (timerRef.current !== undefined) {
      window.clearInterval(timerRef.current);
      timerRef.current = undefined;
    }
  }, []);

  const updateThread = useCallback(
    (nextThread: ConversationThread) => {
      threadRef.current = nextThread;
      setThread(nextThread);
      onThreadIdentityChange?.(nextThread.id);
    },
    [onThreadIdentityChange],
  );

  const closeMemoryDrawer = useCallback((restoreFocus = true) => {
    const trigger = citationTriggerRef.current;
    citationTriggerRef.current = null;
    const activeElement = document.activeElement;
    if (
      activeElement instanceof HTMLElement &&
      drawerRef.current?.contains(activeElement)
    ) {
      activeElement.blur();
    }
    setDrawerCitation(null);
    if (restoreFocus && trigger) {
      requestAnimationFrame(() => trigger.focus());
    }
  }, []);

  const openMemoryDrawer = useCallback(
    (
      citation: ConversationMemoryCitation,
      trigger: HTMLButtonElement,
    ) => {
      citationTriggerRef.current = trigger;
      setDrawerCitation(citation);
    },
    [],
  );

  const commitAssistantReply = useCallback(
    (state: GenerationState, replyText: string) => {
      const assistantMessage: ConversationAssistantMessage = {
        id: state.assistantMessageId,
        role: "assistant",
        blocks: [
          {
            kind: "paragraph",
            content: [{ kind: "text", text: replyText }],
          },
        ],
        markdown: replyText,
        sources:
          generationSourcesRef.current.length > 0
            ? [...generationSourcesRef.current]
            : undefined,
      };

      setThread((current) => {
        if (!current) {
          return current;
        }

        const messages = state.replaceAssistantMessageId
          ? current.messages.map((message) =>
              message.id === state.replaceAssistantMessageId
                ? assistantMessage
                : message,
            )
          : [...current.messages, assistantMessage];
        const nextThread = { ...current, messages };
        threadRef.current = nextThread;
        return nextThread;
      });
    },
    [],
  );

  const streamRemainingChunks = useCallback(
    (state: GenerationState, generationToken: number) => {
      stopTimer();
      let chunkIndex = state.nextChunkIndex;
      let replyText = state.text;
      const runningState = {
        ...state,
        phase: "generating" as const,
        sources: [...generationSourcesRef.current],
        toolLabel: generationToolLabelRef.current,
      };
      setGeneration(runningState);

      if (chunkIndex >= state.chunks.length) {
        commitAssistantReply(state, replyText);
        setGeneration(null);
        return;
      }

      timerRef.current = window.setInterval(() => {
        if (generationToken !== generationTokenRef.current) {
          stopTimer();
          return;
        }

        const chunk = state.chunks[chunkIndex];
        chunkIndex += 1;
        replyText = `${replyText}${replyText ? " " : ""}${chunk}`;

        if (chunkIndex === state.chunks.length) {
          stopTimer();
          commitAssistantReply(state, replyText);
          setGeneration(null);
          return;
        }

        setGeneration({
          ...state,
          phase: "generating",
          text: replyText,
          nextChunkIndex: chunkIndex,
          sources: [...generationSourcesRef.current],
          toolLabel: generationToolLabelRef.current,
        });
      }, 520);
    },
    [commitAssistantReply, stopTimer],
  );

  const requestReply = useCallback(
    async (
      baseThread: ConversationThread,
      prompt: string,
      userMessageId: string,
      mode: "append" | "regenerate",
      replaceAssistantMessageId?: string,
    ) => {
      stopTimer();
      const generationToken = ++generationTokenRef.current;
      setMenuOpen(false);
      closeMemoryDrawer(false);

      const pendingState: GenerationState = {
        phase: "generating",
        prompt,
        text: "",
        chunks: [],
        nextChunkIndex: 0,
        assistantMessageId: "",
        userMessageId,
        mode,
        replaceAssistantMessageId,
        retryKind: "generation",
        sources: [],
      };
      // 来源与工具状态以 ref 为权威累积（React state 投影可能在后续
      // setGeneration 中被旧 pendingState 覆盖），每轮生成开始时重置。
      generationSourcesRef.current = [];
      generationToolLabelRef.current = undefined;
      setGeneration(pendingState);

      try {
        const messages = replaceAssistantMessageId
          ? baseThread.messages.filter(
              (message) => message.id !== replaceAssistantMessageId,
            )
          : baseThread.messages;
        const reply = await service.generateReply({
          conversationId: baseThread.id,
          messages: structuredClone(messages),
          prompt,
          mode,
          onStreamDelta: (delta) => {
            if (generationToken !== generationTokenRef.current) {
              return;
            }
            setGeneration((current) =>
              current
                ? { ...current, phase: "generating", text: current.text + delta }
                : current,
            );
          },
          onSourcesUpdated: (sources) => {
            if (generationToken !== generationTokenRef.current) {
              return;
            }
            for (const source of sources) {
              if (
                !generationSourcesRef.current.some(
                  (known) => known.sourceId === source.sourceId,
                )
              ) {
                generationSourcesRef.current.push(source);
              }
            }
            setGeneration((current) =>
              current
                ? { ...current, sources: [...generationSourcesRef.current] }
                : current,
            );
          },
          onToolState: (label) => {
            if (generationToken !== generationTokenRef.current) {
              return;
            }
            generationToolLabelRef.current = label;
            setGeneration((current) =>
              current ? { ...current, toolLabel: label } : current,
            );
          },
        });
        if (generationToken !== generationTokenRef.current) {
          return;
        }
        if (reply.status === "pending") {
          const pendingTurn = reply.persistedThread.pendingTurn;
          updateThread(reply.persistedThread);
          setGeneration({
            ...pendingState,
            phase: "failed",
            userMessageId:
              pendingTurn?.userMessageId ?? pendingState.userMessageId,
            errorMessage: reply.errorMessage,
          });
          return;
        }
        if (!reply.chunks.length || !reply.chunks.some((chunk) => chunk.trim())) {
          throw new Error("对话服务返回了空回答。");
        }
        if (service.capabilities.delivery === "streaming") {
          updateThread(reply.persistedThread!);
          if (reply.status === "truncated") {
            setGeneration({
              ...pendingState,
              phase: "truncated",
              text: reply.chunks[0] ?? "",
              sources: [...generationSourcesRef.current],
            });
            return;
          }
          // 合并本轮来源到最终 assistant 消息（展示增强；SQLite 权威不受影响）。
          if (generationSourcesRef.current.length > 0) {
            const mergedThread = {
              ...reply.persistedThread!,
              messages: [...reply.persistedThread!.messages],
            };
            const lastIndex = mergedThread.messages.length - 1;
            const last = mergedThread.messages[lastIndex];
            if (lastIndex >= 0 && last.role === "assistant") {
              mergedThread.messages[lastIndex] = {
                ...last,
                sources: [...generationSourcesRef.current],
              };
              updateThread(mergedThread);
            }
          }
          setGeneration(null);
          return;
        }
        if (service.capabilities.delivery === "complete") {
          if (!reply.persistedThread) {
            throw new Error("正式对话服务没有返回已保存的会话。");
          }
          updateThread(reply.persistedThread);
          setGeneration(null);
          return;
        }

        streamRemainingChunks(
          {
            ...pendingState,
            chunks: reply.chunks,
            assistantMessageId: reply.assistantMessageId,
          },
          generationToken,
        );
      } catch (error) {
        if (generationToken !== generationTokenRef.current) {
          return;
        }
        console.error("ReadRay 对话生成失败：", error);
        stopTimer();
        setGeneration({
          ...pendingState,
          phase: "failed",
          errorMessage:
            error instanceof Error ? error.message : String(error),
        });
      }
    },
    [
      closeMemoryDrawer,
      service,
      stopTimer,
      streamRemainingChunks,
      updateThread,
    ],
  );

  useEffect(() => {
    let ignore = false;
    const requestToken = ++generationTokenRef.current;
    stopTimer();
    setGeneration(null);
    threadRef.current = null;
    setThread(null);
    setDraft("");
    setLastExportedMessageCount(0);
    setMenuOpen(false);
    setRenameDraft(null);
    setDeleteConfirmationOpen(false);
    setManagementBusy(null);
    setManagementError("");
    closeMemoryDrawer(false);

    const load = async () => {
      try {
        if (request.kind === "existing") {
          const nextThread = await service.loadConversation(
            request.conversationId,
            request.title,
          );
          if (!ignore && requestToken === generationTokenRef.current) {
            updateThread(nextThread);
            if (nextThread.pendingTurn) {
              setGeneration({
                phase: "failed",
                prompt: nextThread.pendingTurn.prompt,
                text: "",
                errorMessage:
                  "上次回答未完成。问题已经保存在本机，可以直接重试。",
                chunks: [],
                nextChunkIndex: 0,
                assistantMessageId: "",
                userMessageId: nextThread.pendingTurn.userMessageId,
                mode: "append",
                retryKind: "generation",
                sources: [],
              });
            }
          }
          return;
        }

        const cachedCreation = conversationCreationRef.current;
        let creationPromise =
          cachedCreation?.requestKey === request.key &&
          cachedCreation.service === service
            ? cachedCreation.promise
            : undefined;
        if (!creationPromise) {
          creationPromise = service.createConversation();
          conversationCreationRef.current = {
            requestKey: request.key,
            service,
            promise: creationPromise,
          };
          void creationPromise.catch(() => {
            if (conversationCreationRef.current?.promise === creationPromise) {
              conversationCreationRef.current = null;
            }
          });
        }
        const nextThread = await creationPromise;
        if (ignore || requestToken !== generationTokenRef.current) {
          return;
        }
        if (request.kind === "prompt") {
          const userMessage: ConversationUserMessage = {
            id: `${nextThread.id}-user-${++messageIdRef.current}`,
            role: "user",
            content: request.prompt,
            meta: "刚刚",
          };
          const promptThread = {
            ...nextThread,
            title: request.prompt.slice(0, 34),
            messages: [...nextThread.messages, userMessage],
          };
          updateThread(promptThread);
          void requestReply(
            promptThread,
            request.prompt,
            userMessage.id,
            "append",
          );
        } else {
          updateThread(nextThread);
          requestAnimationFrame(() => inputRef.current?.focus());
        }
      } catch (error) {
        if (ignore || requestToken !== generationTokenRef.current) {
          return;
        }
        console.error("ReadRay 对话打开失败：", error);
        if (request.kind === "prompt") {
          setDraft(request.prompt);
        }
        setGeneration({
          phase: "failed",
          prompt: request.kind === "prompt" ? request.prompt : "",
          text: "",
          errorMessage:
            error instanceof Error ? error.message : String(error),
          chunks: [],
          nextChunkIndex: 0,
          assistantMessageId: "",
          userMessageId: "",
          mode: "append",
          retryKind: "request",
          sources: [],
        });
      }
    };

    void load();
    return () => {
      ignore = true;
    };
  }, [
    closeMemoryDrawer,
    request,
    requestReply,
    requestRetryToken,
    service,
    stopTimer,
    updateThread,
  ]);

  useEffect(
    () => {
      mountedRef.current = true;
      return () => {
        mountedRef.current = false;
        generationTokenRef.current += 1;
        stopTimer();
        if (scrollAnimationRef.current !== undefined) {
          cancelAnimationFrame(scrollAnimationRef.current);
          scrollAnimationRef.current = undefined;
        }
        if (toastTimerRef.current !== undefined) {
          window.clearTimeout(toastTimerRef.current);
        }
      };
    },
    [stopTimer],
  );

  useEffect(() => {
    const currentThread = threadRef.current;
    if (
      currentThread &&
      externalTitleUpdate?.conversationId === currentThread.id &&
      externalTitleUpdate.title !== currentThread.title
    ) {
      updateThread({ ...currentThread, title: externalTitleUpdate.title });
    }
  }, [externalTitleUpdate, updateThread]);

  useEffect(() => {
    const handlePointerDown = (event: PointerEvent) => {
      if (
        menuOpen &&
        menuRef.current &&
        !menuRef.current.contains(event.target as Node)
      ) {
        setMenuOpen(false);
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMenuOpen(false);
        closeMemoryDrawer();
      }
    };
    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [closeMemoryDrawer, menuOpen]);

  useLayoutEffect(() => {
    if (!drawerCitation) {
      return;
    }
    requestAnimationFrame(() => drawerCloseRef.current?.focus());
  }, [drawerCitation]);

  useAutoResizeTextarea(inputRef, draft);

  useEffect(() => {
    const scroll = scrollRef.current;
    if (!scroll) {
      return;
    }

    const update = () => {
      // 记录当前会话的滚动位置（供切回时恢复），并更新按钮显隐
      const currentId = threadRef.current?.id;
      if (currentId) {
        scrollPositionsRef.current.set(currentId, scroll.scrollTop);
      }
      updateScrollToBottomVisibility();
    };
    update();
    scroll.addEventListener("scroll", update, { passive: true });

    // 滚动监听保持即时；窗口缩放触发的观察回调按 settle 更新，避免逐帧读取滚动几何。
    let updateTimer: number | undefined;
    const updateAfterSettles = () => {
      window.clearTimeout(updateTimer);
      updateTimer = window.setTimeout(update, 120);
    };
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(updateAfterSettles);
    observer?.observe(scroll);
    const messageColumn = scroll.querySelector<HTMLElement>(
      ".rr-conversation-message-column",
    );
    if (messageColumn) {
      observer?.observe(messageColumn);
    }

    return () => {
      scroll.removeEventListener("scroll", update);
      observer?.disconnect();
      window.clearTimeout(updateTimer);
    };
  }, [updateScrollToBottomVisibility]);

  useLayoutEffect(() => {
    if (generation && scrollRef.current) {
      const scroll = scrollRef.current;
      // 用户主动上翻阅读时暂停自动跟随，接近底部时恢复
      const distanceFromBottom =
        scroll.scrollHeight - scroll.scrollTop - scroll.clientHeight;
      if (distanceFromBottom < 80) {
        scroll.scrollTop = scroll.scrollHeight;
      }
      updateScrollToBottomVisibility();
    }
  }, [generation?.text, generation?.phase, updateScrollToBottomVisibility]);

  useLayoutEffect(() => {
    const scroll = scrollRef.current;
    if (!scroll) {
      setShowScrollToBottom(false);
      return;
    }

    // 切换会话后恢复滚动位置：上次在底部（或新会话）→ 回到底部；
    // 上次在上翻阅读 → 恢复到记录的位置。等一帧让异步加载的消息
    // 完成布局后再设置，避免 scrollHeight 还是旧值导致恢复错误。
    let cancelled = false;
    const frame = requestAnimationFrame(() => {
      if (cancelled || !thread) {
        return;
      }
      const savedTop = scrollPositionsRef.current.get(thread.id) ?? null;
      const atBottom =
        savedTop === null ||
        scroll.scrollHeight - savedTop - scroll.clientHeight < 80;
      scroll.scrollTop = atBottom ? scroll.scrollHeight : savedTop!;
      if (atBottom) {
        scrollPositionsRef.current.delete(thread.id);
      }
      updateScrollToBottomVisibility();
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [thread?.id, updateScrollToBottomVisibility]);

  useLayoutEffect(() => {
    // 消息数量变化（发送/收到消息）后重算按钮显隐，不恢复滚动位置
    const scroll = scrollRef.current;
    if (!scroll) {
      return;
    }
    let cancelled = false;
    const frame = requestAnimationFrame(() => {
      if (cancelled) {
        return;
      }
      updateScrollToBottomVisibility();
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [thread?.messages.length, updateScrollToBottomVisibility]);

  const notify = (message: string) => {
    setToast(message);
    if (toastTimerRef.current !== undefined) {
      window.clearTimeout(toastTimerRef.current);
    }
    toastTimerRef.current = window.setTimeout(() => setToast(""), 1600);
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const prompt = draft.trim();
    const currentThread = threadRef.current;
    if (!prompt || !currentThread || generation) {
      inputRef.current?.focus();
      return;
    }

    const userMessage: ConversationUserMessage = {
      id: `${currentThread.id}-user-${++messageIdRef.current}`,
      role: "user",
      content: prompt,
      meta: "刚刚",
    };
    const nextThread: ConversationThread = {
      ...currentThread,
      title:
        currentThread.messages.length === 0
          ? prompt.slice(0, 34)
          : currentThread.title,
      messages: [...currentThread.messages, userMessage],
    };
    setDraft("");
    updateThread(nextThread);
    void requestReply(nextThread, prompt, userMessage.id, "append");
  };

  const regenerate = () => {
    if (!service.capabilities.canRegenerate) {
      setMenuOpen(false);
      notify("真实 Quick AI 暂不支持重新生成回答");
      return;
    }
    const currentThread = threadRef.current;
    if (!currentThread) {
      setMenuOpen(false);
      return;
    }

    const assistantIndex = currentThread.messages.length - 1;
    if (
      assistantIndex < 0 ||
      currentThread.messages[assistantIndex].role !== "assistant"
    ) {
      setMenuOpen(false);
      notify("当前对话还没有可重新生成的回答");
      return;
    }

    let userMessage: ConversationUserMessage | null = null;
    for (let index = assistantIndex - 1; index >= 0; index -= 1) {
      const message: ConversationMessage = currentThread.messages[index];
      if (message.role === "user") {
        userMessage = message;
        break;
      }
    }
    if (!userMessage) {
      setMenuOpen(false);
      notify("未找到这一轮回答对应的问题");
      return;
    }

    void requestReply(
      currentThread,
      userMessage.content,
      userMessage.id,
      "regenerate",
      currentThread.messages[assistantIndex].id,
    );
  };

  const exportConversation = async () => {
    const currentThread = threadRef.current;
    setMenuOpen(false);
    const unavailableReason = conversationExportUnavailableReason(
      currentThread,
      generation !== null,
      service.capabilities.canExport,
    );
    if (unavailableReason) {
      notify(unavailableReason);
      return;
    }
    if (!currentThread) {
      return;
    }
    const operation = {
      requestKey: requestKeyRef.current,
      conversationId: currentThread.id,
    };

    try {
      const result = await service.exportConversation(
        structuredClone(currentThread),
      );
      if (!result.exported) {
        if (result.reason === "cancelled") {
          return;
        }
        throw new Error("当前对话无法导出。");
      }
      if (!operationIsCurrent(operation)) {
        return;
      }
      deliverConversationExport(result);
      setLastExportedMessageCount(result.messageCount);
      notify(`已导出 ${result.fileName}`);
    } catch (error) {
      if (!operationIsCurrent(operation)) {
        return;
      }
      console.error("ReadRay 对话导出失败：", error);
      setLastExportedMessageCount(0);
      notify("导出失败，请稍后重试");
    }
  };

  const openInlineRename = () => {
    const currentThread = threadRef.current;
    setMenuOpen(false);
    if (!currentThread?.messages.length || generation) {
      return;
    }
    setManagementError("");
    setRenameDraft(currentThread.title);
  };

  const cancelInlineRename = () => {
    if (managementBusy === "rename") {
      return;
    }
    setRenameDraft(null);
    setManagementError("");
  };

  const renameConversation = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const currentThread = threadRef.current;
    const title = renameDraft?.trim() ?? "";
    if (!currentThread || !title || managementBusy) {
      return;
    }
    const targetId = currentThread.id;
    const operation = {
      requestKey: requestKeyRef.current,
      conversationId: targetId,
    };
    setManagementBusy("rename");
    setManagementError("");
    try {
      const renamed = await service.renameConversation(targetId, title);
      if (!operationIsCurrent(operation)) {
        return;
      }
      updateThread(renamed);
      setRenameDraft(null);
      notify("会话名称已更新");
    } catch (error) {
      if (operationIsCurrent(operation)) {
        setManagementError(error instanceof Error ? error.message : String(error));
        notify("重命名失败，请重试");
      }
    } finally {
      if (operationIsCurrent(operation)) {
        setManagementBusy(null);
      }
    }
  };

  const deleteConversation = async () => {
    const currentThread = threadRef.current;
    if (!currentThread || managementBusy) {
      return;
    }
    const targetId = currentThread.id;
    const operation = {
      requestKey: requestKeyRef.current,
      conversationId: targetId,
    };
    setManagementBusy("delete");
    setManagementError("");
    try {
      await service.deleteConversation(targetId);
      if (!operationIsCurrent(operation)) {
        return;
      }
      setDeleteConfirmationOpen(false);
      onConversationDeleted?.(operation);
    } catch (error) {
      if (operationIsCurrent(operation)) {
        setManagementError(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (operationIsCurrent(operation)) {
        setManagementBusy(null);
      }
    }
  };

  const stopGeneration = () => {
    if (!service.capabilities.canStop) {
      return;
    }
    stopTimer();
    const currentThread = threadRef.current;
    if (currentThread && service.capabilities.delivery === "streaming") {
      void service.stopGeneration?.(currentThread.id).catch(() => undefined);
    }
    setGeneration((current) => {
      if (!current) {
        return current;
      }
      if (current.chunks.length === 0) {
        generationTokenRef.current += 1;
      }
      return { ...current, phase: "stopped" };
    });
  };

  const retryOrContinueGeneration = () => {
    if (!generation) {
      return;
    }
    if (generation.retryKind === "request") {
      setRequestRetryToken((token) => token + 1);
      return;
    }

    const currentThread = threadRef.current;
    if (!currentThread) {
      return;
    }
    if (
      generation.phase === "stopped" &&
      generation.chunks.length > 0 &&
      service.capabilities.delivery !== "streaming"
    ) {
      streamRemainingChunks(generation, generationTokenRef.current);
      return;
    }
    void requestReply(
      currentThread,
      generation.prompt,
      generation.userMessageId,
      generation.mode,
      generation.replaceAssistantMessageId,
    );
  };

  const openSource = useCallback(
    (source: AgentSource) => {
      void service.openSource(source.url).catch((error) => {
        console.error("ReadRay 来源打开失败：", error);
        notify("来源打开失败");
      });
    },
    [service],
  );

  const messageContent: ReactNode = (
    <>
      {thread?.messages.length ? (
        thread.messages.map((message) =>
          message.role === "user" ? (
            <UserMessage message={message} key={message.id} />
          ) : (
            <AssistantMessage
              message={message}
              key={message.id}
              onOpenMemory={openMemoryDrawer}
              onOpenSource={openSource}
            />
          ),
        )
      ) : (
        <EmptyConversation />
      )}
      {generation ? (
        <GenerationMessage
          state={generation}
          canStop={service.capabilities.canStop}
          onStop={stopGeneration}
          onRetry={retryOrContinueGeneration}
          onOpenSource={openSource}
        />
      ) : null}
    </>
  );
  const exportUnavailableReason = conversationExportUnavailableReason(
    thread,
    generation !== null,
    service.capabilities.canExport,
  );

  return (
    <main
      className={`rr-main-panel rr-conversation-page${drawerCitation ? " is-drawer-open" : ""}`}
      aria-label="ReadRay 对话"
      data-thread-id={thread?.id ?? ""}
      data-thread-message-count={thread?.messages.length ?? 0}
      data-generation-phase={generation?.phase ?? "idle"}
      data-exported-message-count={lastExportedMessageCount}
    >
      <header className="rr-conversation-bar">
        {renameDraft !== null ? (
          <form
            className="rr-conversation-title-edit"
            onSubmit={renameConversation}
          >
            <input
              autoFocus
              maxLength={80}
              value={renameDraft}
              aria-label="会话名称"
              aria-invalid={managementError ? true : undefined}
              title={managementError || undefined}
              disabled={managementBusy === "rename"}
              onChange={(event) => setRenameDraft(event.target.value)}
              onBlur={cancelInlineRename}
              onKeyDown={(event) => {
                const action = conversationTitleEditAction(
                  event.key,
                  event.nativeEvent.isComposing,
                );
                if (action === "save") {
                  event.preventDefault();
                  event.currentTarget.form?.requestSubmit();
                } else if (action === "cancel") {
                  event.preventDefault();
                  cancelInlineRename();
                }
              }}
            />
          </form>
        ) : (
          <button
            className="rr-conversation-title"
            type="button"
            disabled={generation !== null || !thread?.messages.length}
            title={
              thread?.messages.length
                ? "单击重命名会话"
                : "空白会话无需重命名"
            }
            onClick={openInlineRename}
          >
            {thread?.title ?? "对话"}
          </button>
        )}
        <div className="rr-conversation-controls">
          <div className="rr-conversation-more-wrap" ref={menuRef}>
            <button
              className="rr-conversation-icon-button"
              type="button"
              aria-label="更多操作"
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              onClick={() => setMenuOpen((open) => !open)}
            >
              <MainAppIcon name="more" />
            </button>
            {menuOpen ? (
              <div className="rr-conversation-more-menu" role="menu">
                <button
                  className="rr-conversation-menu-item"
                  type="button"
                  role="menuitem"
                  disabled={
                    generation !== null ||
                    !service.capabilities.canRegenerate
                  }
                  title={
                    service.capabilities.canRegenerate
                      ? undefined
                      : "真实 Quick AI 暂不支持重新生成"
                  }
                  onClick={regenerate}
                >
                  重新生成回答
                </button>
                <button
                  className="rr-conversation-menu-item"
                  type="button"
                  role="menuitem"
                  disabled={
                    exportUnavailableReason !== undefined
                  }
                  title={exportUnavailableReason}
                  onClick={() => void exportConversation()}
                >
                  导出当前对话
                </button>
                <button
                  className="rr-conversation-menu-item is-danger"
                  type="button"
                  role="menuitem"
                  disabled={generation !== null || !thread}
                  onClick={() => {
                    setMenuOpen(false);
                    setManagementError("");
                    setDeleteConfirmationOpen(true);
                  }}
                >
                  删除会话
                </button>
              </div>
            ) : null}
          </div>
        </div>
      </header>

      <div className="rr-conversation-scroll-wrap">
        <div
          className="rr-conversation-scroll"
          ref={scrollRef}
        >
          <div className="rr-conversation-message-column" aria-live="polite">
            {messageContent}
          </div>
        </div>
        <button
          className={`rr-conversation-scroll-to-bottom${showScrollToBottom ? " is-visible" : ""}`}
          type="button"
          aria-label="滚动到底部"
          title="滚动到底部"
          onClick={scrollToBottom}
        >
          <MainAppIcon name="send-up" />
        </button>
      </div>

      <form className="rr-main-composer-area" onSubmit={submit}>
        <div className="rr-main-composer-inner">
          <div
            className="rr-main-composer"
            onClick={(event) => {
              const target = event.target;
              if (target instanceof Element && target.closest("button")) {
                return;
              }
              inputRef.current?.focus();
            }}
          >
            <textarea
              ref={inputRef}
              rows={1}
              value={draft}
              placeholder="继续提问…"
              aria-label="继续提问"
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
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
                  event.currentTarget.form?.requestSubmit();
                }
              }}
            />
            <div className="rr-main-composer-actions">
              <button
                className="rr-main-send"
                type="submit"
                aria-label="发送"
                disabled={!draft.trim() || generation !== null}
              >
                <MainAppIcon name="send-up" />
              </button>
            </div>
          </div>
        </div>
      </form>

      <button
        className="rr-conversation-drawer-backdrop"
        type="button"
        aria-label="关闭记忆引用"
        tabIndex={drawerCitation ? 0 : -1}
        onClick={() => closeMemoryDrawer()}
      />
      <aside
        ref={drawerRef}
        className="rr-conversation-memory-drawer"
        aria-hidden={!drawerCitation}
        aria-label="记忆引用"
      >
        <header className="rr-conversation-drawer-head">
          <h2>记忆引用</h2>
          <button
            ref={drawerCloseRef}
            className="rr-conversation-icon-button"
            type="button"
            aria-label="关闭"
            tabIndex={drawerCitation ? 0 : -1}
            onClick={() => closeMemoryDrawer()}
          >
            <MainAppIcon name="close" />
          </button>
        </header>
        {drawerCitation ? (
          <>
            <p className="rr-conversation-drawer-context">
              本回答使用了 1 条本地记录
            </p>
            <h3 className="rr-conversation-memory-record-title">
              {drawerCitation.title}
            </h3>
            <dl className="rr-conversation-memory-fields">
              <div className="rr-conversation-memory-field">
                <dt>类型</dt>
                <dd>{drawerCitation.typeLabel}</dd>
              </div>
              <div className="rr-conversation-memory-field">
                <dt>来源应用</dt>
                <dd>{drawerCitation.sourceApp}</dd>
              </div>
              <div className="rr-conversation-memory-field">
                <dt>记录时间</dt>
                <dd>{drawerCitation.recordedAt}</dd>
              </div>
            </dl>
            <p className="rr-conversation-memory-excerpt">
              {drawerCitation.excerpt}
            </p>
          </>
        ) : null}
      </aside>
      <div
        className={`rr-conversation-toast${toast ? " is-visible" : ""}`}
        role="status"
      >
        {toast}
      </div>
      {deleteConfirmationOpen ? (
        <div className="rr-conversation-dialog-backdrop">
          <section
            className="rr-conversation-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="rr-conversation-delete-title"
          >
            <h2 id="rr-conversation-delete-title">删除这个会话？</h2>
            <p>完整消息将从本机 SQLite 中删除，此操作无法撤销。</p>
            {managementError ? (
              <p className="rr-conversation-dialog-error">{managementError}</p>
            ) : null}
            <div className="rr-conversation-dialog-actions">
              <button
                type="button"
                disabled={managementBusy !== null}
                onClick={() => setDeleteConfirmationOpen(false)}
              >
                取消
              </button>
              <button
                className="is-danger"
                type="button"
                disabled={managementBusy !== null}
                onClick={() => void deleteConversation()}
              >
                {managementBusy === "delete" ? "正在删除…" : "删除"}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </main>
  );
}

export default ConversationPage;
