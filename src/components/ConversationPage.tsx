import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type ReactNode,
} from "react";
import type {
  ConversationAssistantMessage,
  ConversationInline,
  ConversationMessage,
  ConversationMemoryCitation,
  ConversationRequest,
  ConversationService,
  ConversationThread,
  ConversationUserMessage,
} from "../conversationViewModel";
import MainAppIcon from "./MainAppIcon";

type ConversationPageProps = {
  request: ConversationRequest;
  service: ConversationService;
};

type GenerationState = {
  phase: "generating" | "complete" | "stopped" | "failed";
  prompt: string;
  text: string;
  chunks: string[];
  nextChunkIndex: number;
  assistantMessageId: string;
  userMessageId: string;
  mode: "append" | "regenerate";
  replaceAssistantMessageId?: string;
  retryKind: "request" | "generation";
};

const USER_CLAMP_LINES = 5;

function downloadConversationFile(file: {
  fileName: string;
  mimeType: string;
  content: string;
}) {
  const blob = new Blob([file.content], { type: file.mimeType });
  if (blob.size === 0) {
    throw new Error("conversation export is empty");
  }

  const downloadUrl = URL.createObjectURL(blob);
  const downloadLink = document.createElement("a");
  downloadLink.href = downloadUrl;
  downloadLink.download = file.fileName;
  downloadLink.style.display = "none";
  document.body.appendChild(downloadLink);
  downloadLink.click();
  downloadLink.remove();
  window.setTimeout(() => URL.revokeObjectURL(downloadUrl), 0);
}

function resizeComposer(input: HTMLTextAreaElement) {
  input.style.height = "auto";
  const maxHeight = Number.parseFloat(getComputedStyle(input).maxHeight);
  const contentHeight = input.scrollHeight;
  input.style.height = `${Math.min(contentHeight, maxHeight)}px`;
  input.style.overflowY = contentHeight > maxHeight ? "auto" : "hidden";
}

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
    const observer = new ResizeObserver(measure);
    observer.observe(copy);
    return () => observer.disconnect();
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

function AssistantMessage({
  message,
  onOpenMemory,
}: {
  message: ConversationAssistantMessage;
  onOpenMemory: (
    citation: ConversationMemoryCitation,
    trigger: HTMLButtonElement,
  ) => void;
}) {
  return (
    <article className="rr-conversation-message is-assistant">
      <div className="rr-conversation-assistant-copy">
        {message.blocks.map((block, index) => {
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
        })}
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
      </div>
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

function GenerationMessage({
  state,
  onStop,
  onRetry,
}: {
  state: GenerationState;
  onStop: () => void;
  onRetry: () => void;
}) {
  if (state.phase === "failed") {
    return (
      <article className="rr-conversation-message is-assistant">
        <div className="rr-conversation-assistant-copy">
          {state.text ? <p>{state.text}</p> : null}
          <div className="rr-conversation-answer-kicker">生成中断</div>
          <p className="rr-conversation-error-copy">
            暂时无法完成回答。你的输入仍然保留，可以直接重试。
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
        {state.phase === "generating" ? (
          <div className="rr-conversation-answer-kicker">正在生成</div>
        ) : null}
        <p>{state.text}</p>
        {state.phase === "generating" ? (
          <div className="rr-conversation-generation-row">
            <span className="rr-conversation-stream-line" />
            <button
              className="rr-conversation-stop-button"
              type="button"
              onClick={onStop}
            >
              停止生成
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

function ConversationPage({ request, service }: ConversationPageProps) {
  const [thread, setThread] = useState<ConversationThread | null>(null);
  const [draft, setDraft] = useState("");
  const [generation, setGeneration] = useState<GenerationState | null>(null);
  const [requestRetryToken, setRequestRetryToken] = useState(0);
  const [lastExportedMessageCount, setLastExportedMessageCount] = useState(0);
  const [menuOpen, setMenuOpen] = useState(false);
  const [drawerCitation, setDrawerCitation] =
    useState<ConversationMemoryCitation | null>(null);
  const [toast, setToast] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const drawerRef = useRef<HTMLElement>(null);
  const drawerCloseRef = useRef<HTMLButtonElement>(null);
  const citationTriggerRef = useRef<HTMLButtonElement | null>(null);
  const threadRef = useRef<ConversationThread | null>(null);
  const timerRef = useRef<number | undefined>(undefined);
  const generationTokenRef = useRef(0);
  const toastTimerRef = useRef<number | undefined>(undefined);
  const messageIdRef = useRef(0);

  const stopTimer = useCallback(() => {
    if (timerRef.current !== undefined) {
      window.clearInterval(timerRef.current);
      timerRef.current = undefined;
    }
  }, []);

  const updateThread = useCallback((nextThread: ConversationThread) => {
    threadRef.current = nextThread;
    setThread(nextThread);
  }, []);

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
      const runningState = { ...state, phase: "generating" as const };
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
      };
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
        });
        if (generationToken !== generationTokenRef.current) {
          return;
        }
        if (!reply.chunks.length || !reply.chunks.some((chunk) => chunk.trim())) {
          throw new Error("fixture returned an empty reply");
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
        });
      }
    },
    [closeMemoryDrawer, service, stopTimer, streamRemainingChunks],
  );

  useEffect(() => {
    let ignore = false;
    const requestToken = ++generationTokenRef.current;
    stopTimer();
    setGeneration(null);
    setLastExportedMessageCount(0);
    setMenuOpen(false);
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
          }
          return;
        }

        const nextThread = await service.createConversation();
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
        setGeneration({
          phase: "failed",
          prompt: "",
          text: "",
          chunks: [],
          nextChunkIndex: 0,
          assistantMessageId: "",
          userMessageId: "",
          mode: "append",
          retryKind: "request",
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
    () => () => {
      stopTimer();
      if (toastTimerRef.current !== undefined) {
        window.clearTimeout(toastTimerRef.current);
      }
    },
    [stopTimer],
  );

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

  useLayoutEffect(() => {
    if (inputRef.current) {
      resizeComposer(inputRef.current);
    }
  }, [draft]);

  useEffect(() => {
    const handleResize = () => {
      if (inputRef.current) {
        resizeComposer(inputRef.current);
      }
    };
    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, []);

  useLayoutEffect(() => {
    if (generation && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [generation?.text, generation?.phase]);

  useLayoutEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = 0;
    }
  }, [thread?.id]);

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
    if (generation) {
      return;
    }
    if (!currentThread) {
      notify("当前没有可导出的对话");
      return;
    }

    try {
      const result = await service.exportConversation(
        structuredClone(currentThread),
      );
      if (!result.exported) {
        throw new Error("fixture did not produce an export");
      }
      if (
        !result.file.fileName.trim() ||
        !result.file.mimeType.trim() ||
        !result.file.content.trim()
      ) {
        throw new Error("fixture returned an invalid export file");
      }
      downloadConversationFile(result.file);
      setLastExportedMessageCount(currentThread.messages.length);
      notify("当前对话已导出");
    } catch (error) {
      console.error("ReadRay 对话导出失败：", error);
      setLastExportedMessageCount(0);
      notify("导出失败，请稍后重试");
    }
  };

  const stopGeneration = () => {
    stopTimer();
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
      generation.chunks.length > 0
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
            />
          ),
        )
      ) : (
        <EmptyConversation />
      )}
      {generation ? (
        <GenerationMessage
          state={generation}
          onStop={stopGeneration}
          onRetry={retryOrContinueGeneration}
        />
      ) : null}
    </>
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
        <div className="rr-conversation-title">{thread?.title ?? "对话"}</div>
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
                  onClick={regenerate}
                >
                  重新生成回答
                </button>
                <button
                  className="rr-conversation-menu-item"
                  type="button"
                  role="menuitem"
                  disabled={generation !== null}
                  onClick={() => void exportConversation()}
                >
                  导出当前对话
                </button>
              </div>
            ) : null}
          </div>
        </div>
      </header>

      <div className="rr-conversation-scroll" ref={scrollRef}>
        <div className="rr-conversation-message-column" aria-live="polite">
          {messageContent}
        </div>
      </div>

      <form className="rr-conversation-composer-area" onSubmit={submit}>
        <div className="rr-conversation-composer-inner">
          <div className="rr-conversation-composer">
            <textarea
              ref={inputRef}
              rows={1}
              value={draft}
              placeholder="继续提问…"
              aria-label="继续提问"
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (
                  event.key === "Enter" &&
                  !event.shiftKey &&
                  !event.nativeEvent.isComposing
                ) {
                  event.preventDefault();
                  event.currentTarget.form?.requestSubmit();
                }
              }}
            />
            <button
              className="rr-conversation-send"
              type="submit"
              aria-label="发送"
              disabled={!draft.trim() || generation !== null}
            >
              <MainAppIcon name="send" />
            </button>
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
    </main>
  );
}

export default ConversationPage;
