import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent,
} from "react";
import type {
  ConversationService,
  ConversationSummary,
} from "../conversationViewModel";
import MainAppIcon from "./MainAppIcon";

type ConversationHistoryPageProps = {
  service: ConversationService;
  refreshToken: number;
  onOpenConversation: (conversation: ConversationSummary) => void;
  onConversationContextMenu: (
    conversation: ConversationSummary,
    event: MouseEvent<HTMLElement>,
  ) => void;
};

type ConversationSortOrder = "recent" | "oldest" | "title";

const RECENT_CONVERSATION_LIMIT = 6;

const conversationSortOptions: ReadonlyArray<{
  value: ConversationSortOrder;
  label: string;
}> = [
  { value: "recent", label: "最近更新" },
  { value: "oldest", label: "最早更新" },
  { value: "title", label: "标题" },
];

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
  if (date.getFullYear() === now.getFullYear()) {
    return `${date.getMonth() + 1} 月 ${date.getDate()} 日`;
  }
  return `${date.getFullYear()} 年 ${date.getMonth() + 1} 月 ${date.getDate()} 日`;
}

function sortConversations(
  conversations: ConversationSummary[],
  sortOrder: ConversationSortOrder,
) {
  return [...conversations].sort((first, second) => {
    if (sortOrder === "title") {
      const titleOrder = first.title.localeCompare(
        second.title,
        "zh-CN",
        { sensitivity: "base" },
      );
      if (titleOrder !== 0) {
        return titleOrder;
      }
    }

    const updatedOrder = second.updatedAtUnixMs - first.updatedAtUnixMs;
    return sortOrder === "oldest" ? -updatedOrder : updatedOrder;
  });
}

type ConversationHistoryGroupProps = {
  groupKey: "recent" | "other";
  label: string;
  conversations: ConversationSummary[];
  onOpenConversation: (conversation: ConversationSummary) => void;
  onConversationContextMenu: (
    conversation: ConversationSummary,
    event: MouseEvent<HTMLElement>,
  ) => void;
};

function ConversationHistoryGroup({
  groupKey,
  label,
  conversations,
  onOpenConversation,
  onConversationContextMenu,
}: ConversationHistoryGroupProps) {
  if (conversations.length === 0) {
    return null;
  }

  const headingId = `rr-conversation-history-group-${groupKey}`;

  return (
    <section
      className="rr-conversation-history-group"
      aria-labelledby={headingId}
    >
      <header className="rr-conversation-history-group-heading">
        <h2 id={headingId}>{label}</h2>
        <span>{conversations.length}</span>
      </header>
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
              title={conversation.title}
              onClick={() => onOpenConversation(conversation)}
            >
              <strong>{conversation.title}</strong>
              <span className="rr-conversation-history-date">
                {formatUpdatedAt(conversation.updatedAtUnixMs)}
              </span>
              <span className="rr-conversation-history-arrow" aria-hidden="true">
                <MainAppIcon name="arrow" />
              </span>
            </button>
          </article>
        ))}
      </div>
    </section>
  );
}

function ConversationHistorySortSelect({
  value,
  onChange,
}: {
  value: ConversationSortOrder;
  onChange: (value: ConversationSortOrder) => void;
}) {
  const selectId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const selectedIndex = Math.max(
    0,
    conversationSortOptions.findIndex((option) => option.value === value),
  );
  const [activeIndex, setActiveIndex] = useState(selectedIndex);
  const selectedOption = conversationSortOptions[selectedIndex];

  useEffect(() => {
    if (!open) return;

    const handlePointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const handleDocumentKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        buttonRef.current?.focus();
      }
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleDocumentKeyDown);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleDocumentKeyDown);
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      setActiveIndex(selectedIndex);
    }
  }, [open, selectedIndex]);

  const chooseOption = (index: number) => {
    const option = conversationSortOptions[index];
    if (!option) return;
    onChange(option.value);
    setOpen(false);
    buttonRef.current?.focus();
  };

  const openMenu = () => {
    setActiveIndex(selectedIndex);
    setOpen(true);
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) {
        openMenu();
        return;
      }
      const delta = event.key === "ArrowDown" ? 1 : -1;
      setActiveIndex((index) =>
        Math.min(
          Math.max(index + delta, 0),
          conversationSortOptions.length - 1,
        ),
      );
      return;
    }

    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      if (!open) openMenu();
      setActiveIndex(event.key === "Home" ? 0 : conversationSortOptions.length - 1);
      return;
    }

    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (open) {
        chooseOption(activeIndex);
      } else {
        openMenu();
      }
    }
  };

  return (
    <div
      ref={rootRef}
      className={`rr-conversation-history-sort${open ? " is-open" : ""}`}
    >
      <button
        ref={buttonRef}
        className="rr-conversation-history-sort-trigger"
        type="button"
        role="combobox"
        aria-label="对话排序"
        aria-controls={`${selectId}-listbox`}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-activedescendant={
          open ? `${selectId}-option-${activeIndex}` : undefined
        }
        onClick={() => (open ? setOpen(false) : openMenu())}
        onKeyDown={handleKeyDown}
      >
        <span className="rr-conversation-history-sort-label">排序</span>
        <span className="rr-conversation-history-sort-value">
          {selectedOption?.label ?? value}
        </span>
        <MainAppIcon name="chevron" />
      </button>
      {open ? (
        <div
          id={`${selectId}-listbox`}
          className="rr-conversation-history-sort-menu"
          role="listbox"
          aria-label="对话排序"
        >
          {conversationSortOptions.map((option, index) => (
            <button
              id={`${selectId}-option-${index}`}
              className={`rr-conversation-history-sort-option${
                index === activeIndex ? " is-active" : ""
              }${option.value === value ? " is-selected" : ""}`}
              key={option.value}
              type="button"
              role="option"
              aria-selected={option.value === value}
              onMouseEnter={() => setActiveIndex(index)}
              onClick={() => chooseOption(index)}
            >
              {option.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
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
  const [query, setQuery] = useState("");
  const [sortOrder, setSortOrder] = useState<ConversationSortOrder>("recent");

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

  const recentConversationIds = useMemo(
    () =>
      new Set(
        [...conversations]
          .sort((first, second) => second.updatedAtUnixMs - first.updatedAtUnixMs)
          .slice(0, RECENT_CONVERSATION_LIMIT)
          .map((conversation) => conversation.id),
      ),
    [conversations],
  );

  const visibleConversations = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    const matched = normalizedQuery
      ? conversations.filter((conversation) =>
          conversation.title.toLocaleLowerCase().includes(normalizedQuery),
        )
      : conversations;
    return sortConversations(matched, sortOrder);
  }, [conversations, query, sortOrder]);

  const recentConversations = visibleConversations.filter((conversation) =>
    recentConversationIds.has(conversation.id),
  );
  const otherConversations = visibleConversations.filter(
    (conversation) => !recentConversationIds.has(conversation.id),
  );

  return (
    <main className="rr-main-panel rr-conversation-history" aria-label="全部对话">
      <div className="rr-conversation-history-inner">
        <header className="rr-conversation-history-heading">
          <div>
            <h1>全部对话</h1>
          </div>
          {status === "ready" ? (
            <span>
              {query.trim()
                ? `${visibleConversations.length} / ${conversations.length} 个会话`
                : `${conversations.length} 个会话`}
            </span>
          ) : null}
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
          <>
            <div className="rr-conversation-history-toolbar">
              <label className="rr-conversation-history-search">
                <MainAppIcon name="search" />
                <input
                  type="search"
                  value={query}
                  placeholder="搜索对话"
                  aria-label="搜索对话"
                  onChange={(event) => setQuery(event.target.value)}
                />
                {query ? (
                  <button
                    className="rr-conversation-history-clear"
                    type="button"
                    aria-label="清除搜索"
                    onClick={() => setQuery("")}
                  >
                    <MainAppIcon name="close" />
                  </button>
                ) : null}
              </label>
              <ConversationHistorySortSelect
                value={sortOrder}
                onChange={(value) => setSortOrder(value)}
              />
            </div>

            {visibleConversations.length === 0 ? (
              <div className="rr-conversation-history-state is-search-empty">
                <strong>没有找到匹配的对话</strong>
                <p>换一个关键词试试，或清除搜索条件。</p>
                <button type="button" onClick={() => setQuery("")}>
                  清除搜索
                </button>
              </div>
            ) : (
              <>
                <ConversationHistoryGroup
                  groupKey="recent"
                  label="最近对话"
                  conversations={recentConversations}
                  onOpenConversation={onOpenConversation}
                  onConversationContextMenu={onConversationContextMenu}
                />
                <ConversationHistoryGroup
                  groupKey="other"
                  label="其他对话"
                  conversations={otherConversations}
                  onOpenConversation={onOpenConversation}
                  onConversationContextMenu={onConversationContextMenu}
                />
              </>
            )}
          </>
        )}
      </div>
    </main>
  );
}

export default ConversationHistoryPage;
