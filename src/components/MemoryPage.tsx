import { useEffect, useRef, useState } from "react";
import type {
  MemoryFilterId,
  MemoryPageViewModel,
  MemoryRecordItem,
} from "../memoryViewModel";
import type { MemoryService } from "../memoryService";
import MainAppIcon from "./MainAppIcon";

type MemoryPageProps = {
  viewModel: MemoryPageViewModel;
  service: MemoryService | null;
  refreshToken: number;
  requestedRecordId?: string;
};

const PAGE_SIZE = 20;

function errorMessage(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function MemoryPage({
  viewModel,
  service,
  refreshToken,
  requestedRecordId,
}: MemoryPageProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [activeFilter, setActiveFilter] = useState<MemoryFilterId>("all");
  const [records, setRecords] = useState<MemoryRecordItem[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [page, setPage] = useState(1);
  const [selectedId, setSelectedId] = useState("");
  const [selectedRecord, setSelectedRecord] =
    useState<MemoryRecordItem | null>(null);
  const [listStatus, setListStatus] = useState<
    "loading" | "ready" | "error"
  >("loading");
  const [listError, setListError] = useState<string>();
  const [detailStatus, setDetailStatus] = useState<
    "idle" | "loading" | "ready" | "error"
  >("idle");
  const [detailError, setDetailError] = useState<string>();
  const [reloadToken, setReloadToken] = useState(0);
  const [detailReloadToken, setDetailReloadToken] = useState(0);
  const [historyExpanded, setHistoryExpanded] = useState(false);
  const [detailOpen, setDetailOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const recordsScrollRef = useRef<HTMLDivElement>(null);
  const detailScrollRef = useRef<HTMLDivElement>(null);
  const detailTitleRef = useRef<HTMLHeadingElement>(null);
  const handledRequestedRecordIdRef = useRef<string | undefined>(undefined);

  useEffect(() => {
    const timeout = window.setTimeout(() => {
      setDebouncedQuery(searchQuery.trim());
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [searchQuery]);

  useEffect(() => {
    let ignore = false;

    if (!service) {
      setListStatus("loading");
      return;
    }

    setListStatus("loading");
    setListError(undefined);
    setRecords([]);
    setTotalCount(0);
    setSelectedId("");
    setSelectedRecord(null);
    setDetailStatus("idle");

    service
      .listRecords({
        page,
        pageSize: PAGE_SIZE,
        keyword: debouncedQuery || undefined,
        queryType: activeFilter === "all" ? undefined : activeFilter,
      })
      .then((result) => {
        if (ignore) {
          return;
        }

        const lastPage = Math.max(1, Math.ceil(result.total / result.pageSize));
        if (!result.records.length && result.total > 0 && page > lastPage) {
          setPage(lastPage);
          return;
        }

        setRecords(result.records);
        setTotalCount(result.total);
        setListStatus("ready");
        const pendingRequestedId =
          requestedRecordId &&
          handledRequestedRecordIdRef.current !== requestedRecordId
            ? requestedRecordId
            : undefined;
        if (pendingRequestedId) {
          handledRequestedRecordIdRef.current = requestedRecordId;
        }
        setSelectedId(pendingRequestedId ?? result.records[0]?.id ?? "");
        setSelectedRecord(null);
      })
      .catch((error) => {
        if (ignore) {
          return;
        }
        setTotalCount(0);
        setListError(errorMessage(error));
        setListStatus("error");
      });

    return () => {
      ignore = true;
    };
  }, [
    activeFilter,
    debouncedQuery,
    page,
    refreshToken,
    reloadToken,
    requestedRecordId,
    service,
  ]);

  useEffect(() => {
    let ignore = false;

    if (!service || !selectedId) {
      setDetailStatus("idle");
      return;
    }

    setDetailStatus("loading");
    setDetailError(undefined);
    service
      .getRecord(selectedId)
      .then((record) => {
        if (ignore) {
          return;
        }
        if (!record) {
          setSelectedRecord(null);
          setDetailError("这个学习目标已不存在。");
          setDetailStatus("error");
          return;
        }
        setSelectedRecord(record);
        setDetailStatus("ready");
      })
      .catch((error) => {
        if (ignore) {
          return;
        }
        setDetailError(errorMessage(error));
        setDetailStatus("error");
      });

    return () => {
      ignore = true;
    };
  }, [detailReloadToken, selectedId, service]);

  useEffect(() => {
    setHistoryExpanded(false);
    if (detailScrollRef.current) {
      detailScrollRef.current.scrollTop = 0;
    }
  }, [selectedRecord?.id]);

  useEffect(() => {
    if (recordsScrollRef.current) {
      recordsScrollRef.current.scrollTop = 0;
    }
  }, [activeFilter, debouncedQuery, page]);

  useEffect(() => {
    function focusSearch(event: KeyboardEvent) {
      if (event.key === "/" && document.activeElement !== searchInputRef.current) {
        event.preventDefault();
        searchInputRef.current?.focus();
      }

      if (event.key === "Escape" && detailOpen) {
        event.preventDefault();
        setDetailOpen(false);
        window.requestAnimationFrame(() => {
          recordsScrollRef.current
            ?.querySelector<HTMLElement>(`[data-record-id="${selectedId}"]`)
            ?.focus({ preventScroll: true });
        });
      } else if (
        event.key === "Escape" &&
        document.activeElement === searchInputRef.current &&
        searchQuery
      ) {
        event.preventDefault();
        setSearchQuery("");
        setActiveFilter("all");
      }
    }

    window.addEventListener("keydown", focusSearch);
    return () => window.removeEventListener("keydown", focusSearch);
  }, [detailOpen, searchQuery, selectedId]);

  useEffect(() => {
    const compactLayout = window.matchMedia("(max-width: 980px)");
    function leaveCompactLayout(event: MediaQueryListEvent) {
      if (!event.matches) {
        setDetailOpen(false);
      }
    }

    compactLayout.addEventListener("change", leaveCompactLayout);
    return () => compactLayout.removeEventListener("change", leaveCompactLayout);
  }, []);

  function resetSearch() {
    setSearchQuery("");
    setDebouncedQuery("");
    setActiveFilter("all");
    setPage(1);
    searchInputRef.current?.focus();
  }

  function selectRecord(recordId: string) {
    const compactLayout = window.matchMedia("(max-width: 980px)").matches;
    setSelectedId(recordId);
    setSelectedRecord(null);
    setDetailOpen(compactLayout);

    if (compactLayout) {
      window.requestAnimationFrame(() => {
        detailTitleRef.current?.focus({ preventScroll: true });
      });
    }
  }

  function handleRecordListKeyDown(event: React.KeyboardEvent<HTMLElement>) {
    const target = event.target;
    if (!(target instanceof HTMLElement) || !target.matches(".rr-memory-record")) {
      return;
    }

    const items = [
      ...(recordsScrollRef.current?.querySelectorAll<HTMLElement>(
        ".rr-memory-record",
      ) ?? []),
    ];
    const currentIndex = items.indexOf(target);
    const targetIndex = {
      ArrowDown: Math.min(currentIndex + 1, items.length - 1),
      ArrowUp: Math.max(currentIndex - 1, 0),
      Home: 0,
      End: items.length - 1,
    }[event.key];

    if (targetIndex === undefined) {
      return;
    }

    event.preventDefault();
    items[targetIndex]?.focus();
  }

  const isFiltering = activeFilter !== "all" || Boolean(debouncedQuery);
  const pageCount = Math.max(1, Math.ceil(totalCount / PAGE_SIZE));
  const recordCountLabel =
    listStatus === "loading"
      ? "正在读取"
      : listStatus === "error"
        ? "读取失败"
        : isFiltering
          ? `${totalCount} 个结果`
          : `${totalCount} 个学习目标`;
  const selectedRecordIsPassage =
    selectedRecord?.type === "sentence" || selectedRecord?.type === "paragraph";

  return (
    <main className="rr-main-panel rr-memory-page" data-testid="memory-page">
      <section
        className={`rr-memory-shell${detailOpen ? " is-detail-open" : ""}`}
        aria-labelledby="rr-memory-heading"
      >
        <header className="rr-memory-header">
          <div className="rr-memory-heading-row">
            <h1 id="rr-memory-heading">{viewModel.heading}</h1>
            <span className="rr-memory-record-count">
              {recordCountLabel}
            </span>
          </div>

          <div className="rr-memory-search-wrap">
            <MainAppIcon name="search" />
            <input
              ref={searchInputRef}
              className="rr-memory-search-input"
              data-testid="memory-search"
              type="search"
              autoComplete="off"
              value={searchQuery}
              placeholder={viewModel.searchPlaceholder}
              aria-label="搜索记忆记录"
              onChange={(event) => {
                setSearchQuery(event.target.value);
                setPage(1);
              }}
            />
            {searchQuery ? (
              <button
                className="rr-memory-clear-search"
                type="button"
                aria-label="清空搜索"
                onClick={() => {
                  resetSearch();
                }}
              >
                <MainAppIcon name="close" />
              </button>
            ) : null}
          </div>

          <div
            className="rr-memory-filters"
            role="group"
            aria-label="按查询类型筛选"
          >
            {viewModel.filters.map((filter) => {
              const active = filter.id === activeFilter;
              return (
                <button
                  className={`rr-memory-filter${active ? " is-active" : ""}`}
                  data-testid={`memory-filter-${filter.id}`}
                  type="button"
                  aria-pressed={active}
                  key={filter.id}
                  onClick={() => {
                    setActiveFilter(filter.id);
                    setPage(1);
                  }}
                >
                  {filter.label}
                </button>
              );
            })}
          </div>
        </header>

        <div className="rr-memory-workspace">
          <section className="rr-memory-records-pane" aria-label="查询记录列表">
            <div
              className="rr-memory-records-scroll"
              ref={recordsScrollRef}
              onKeyDown={handleRecordListKeyDown}
              aria-busy={listStatus === "loading"}
            >
              {listStatus === "ready"
                ? viewModel.groups.map((group) => {
                    const groupRecords = records.filter(
                      (record) => record.group === group,
                    );
                    if (!groupRecords.length) {
                      return null;
                    }

                    const groupId = `rr-memory-group-${group}`;
                    return (
                      <section
                        className="rr-memory-record-group"
                        aria-labelledby={groupId}
                        key={group}
                      >
                        <h2 className="rr-memory-group-label" id={groupId}>
                          {group}
                        </h2>
                        {groupRecords.map((record) => {
                          const active = record.id === selectedId;
                          return (
                            <button
                              className={`rr-memory-record${active ? " is-active" : ""}`}
                              data-testid={`memory-record-${record.id}`}
                              data-record-id={record.id}
                              type="button"
                              aria-current={active ? "true" : undefined}
                              aria-controls="rr-memory-detail-content"
                              key={record.id}
                              onClick={() => selectRecord(record.id)}
                            >
                              <span className="rr-memory-record-query">
                                {record.query}
                              </span>
                              <span className="rr-memory-record-summary">
                                {record.summary}
                              </span>
                              <span className="rr-memory-record-meta">
                                <span>{record.app}</span>
                                <span aria-hidden="true">·</span>
                                <span>{record.time}</span>
                                <span aria-hidden="true">·</span>
                                <span>{record.typeLabel}</span>
                                <span aria-hidden="true">·</span>
                                <span>查询 {record.queryCount ?? 1} 次</span>
                              </span>
                            </button>
                          );
                        })}
                      </section>
                    );
                  })
                : null}

              {listStatus === "loading" ? (
                <div className="rr-memory-pane-state" role="status">
                  <strong>正在读取记忆</strong>
                  <p>正在从本地学习记录中加载。</p>
                </div>
              ) : listStatus === "error" ? (
                <div className="rr-memory-pane-state" role="alert">
                  <strong>暂时无法读取记忆</strong>
                  <p>{listError}</p>
                  <button
                    className="rr-memory-state-action"
                    type="button"
                    onClick={() => setReloadToken((token) => token + 1)}
                  >
                    重新读取
                  </button>
                </div>
              ) : !records.length ? (
                <div className="rr-memory-pane-state" role="status">
                  <strong>没有找到相关语境</strong>
                  <p>
                    {isFiltering
                      ? "试试更短的关键词，或切换查询类型。"
                      : "完成一次查询后，学习记录会出现在这里。"}
                  </p>
                  {isFiltering ? (
                    <button
                      className="rr-memory-state-action"
                      type="button"
                      onClick={resetSearch}
                    >
                      清除搜索条件
                    </button>
                  ) : null}
                </div>
              ) : null}
            </div>

            {totalCount > 0 ? (
              <nav className="rr-memory-pagination" aria-label="记忆记录分页">
                <button
                  type="button"
                  disabled={page <= 1 || listStatus === "loading"}
                  onClick={() => setPage((current) => Math.max(1, current - 1))}
                >
                  上一页
                </button>
                <span>
                  {page} / {pageCount}
                </span>
                <button
                  type="button"
                  disabled={page >= pageCount || listStatus === "loading"}
                  onClick={() =>
                    setPage((current) => Math.min(pageCount, current + 1))
                  }
                >
                  下一页
                </button>
              </nav>
            ) : null}
          </section>

          <section className="rr-memory-detail-pane" aria-label="选中记录详情">
            <div className="rr-memory-detail-scroll" ref={detailScrollRef}>
              {detailStatus === "loading" && !selectedRecord ? (
                <div className="rr-memory-pane-state" role="status">
                  <strong>正在读取详情</strong>
                  <p>正在打开这条学习记录。</p>
                </div>
              ) : detailStatus === "error" ? (
                <div className="rr-memory-pane-state" role="alert">
                  <strong>暂时无法读取详情</strong>
                  <p>{detailError}</p>
                  {selectedId ? (
                    <button
                      className="rr-memory-state-action"
                      type="button"
                      onClick={() => setDetailReloadToken((token) => token + 1)}
                    >
                      重新读取
                    </button>
                  ) : null}
                </div>
              ) : selectedRecord ? (
                <article
                  className="rr-memory-detail-content"
                  id="rr-memory-detail-content"
                  data-testid="memory-detail"
                  data-target-id={selectedRecord.id}
                >
                  <header
                    className={`rr-memory-detail-head${selectedRecordIsPassage ? " is-passage" : ""}`}
                  >
                    <button
                      className="rr-memory-detail-back"
                      type="button"
                      aria-label="返回记忆记录列表"
                      onClick={() => {
                        setDetailOpen(false);
                        window.requestAnimationFrame(() => {
                          recordsScrollRef.current
                            ?.querySelector<HTMLElement>(
                              `[data-record-id="${selectedId}"]`,
                            )
                            ?.focus({ preventScroll: true });
                        });
                      }}
                    >
                      <MainAppIcon name="back" />
                    </button>
                    <div>
                      {selectedRecordIsPassage ? (
                        <h2
                          className="rr-memory-detail-passage-title"
                          ref={detailTitleRef}
                          tabIndex={-1}
                        >
                          {selectedRecord.typeLabel}
                        </h2>
                      ) : (
                        <h2
                          className="rr-memory-detail-title"
                          ref={detailTitleRef}
                          tabIndex={-1}
                        >
                          {selectedRecord.query}
                        </h2>
                      )}
                      {!selectedRecordIsPassage ? (
                        <div className="rr-memory-lexical-line">
                          {selectedRecord.phonetic ? (
                            <span className="rr-memory-phonetic">
                              {selectedRecord.phonetic}
                            </span>
                          ) : null}
                          <span className="rr-memory-part-of-speech">
                            {selectedRecord.part}
                          </span>
                        </div>
                      ) : null}
                    </div>
                    {!selectedRecordIsPassage ? (
                      <span className="rr-memory-detail-type">
                        {selectedRecord.typeLabel}
                      </span>
                    ) : null}
                  </header>

                  <p className="rr-memory-query-count">
                    共查询 {selectedRecord.queryCount ?? 1} 次
                  </p>

                  {selectedRecordIsPassage ? (
                    <div className="rr-memory-passage-detail">
                      <section className="rr-memory-passage-section">
                        <h3>原文</h3>
                        <blockquote className="rr-memory-source-sentence" lang="en">
                          {selectedRecord.sentence}
                        </blockquote>
                      </section>
                      {selectedRecord.translation ? (
                        <section className="rr-memory-passage-section">
                          <h3>中文翻译</h3>
                          <p className="rr-memory-passage-translation">
                            {selectedRecord.translation}
                          </p>
                        </section>
                      ) : null}
                      {selectedRecord.meaning ? (
                        <section className="rr-memory-passage-section">
                          <h3>{selectedRecord.type === "sentence" ? "理解提示" : "核心理解"}</h3>
                          <p className="rr-memory-context-meaning">
                            {selectedRecord.meaning}
                          </p>
                        </section>
                      ) : null}
                    </div>
                  ) : (
                    <>
                      <section className="rr-memory-detail-meaning">
                        <h3>核心理解</h3>
                        <p className="rr-memory-definition">
                          {selectedRecord.definition}
                        </p>
                        {selectedRecord.meaning ? (
                          <div className="rr-memory-context-meaning-block">
                            <span className="rr-memory-context-meaning-label">
                              本次语境
                            </span>
                            <p className="rr-memory-context-meaning">
                              {selectedRecord.meaning}
                            </p>
                          </div>
                        ) : null}
                      </section>

                      <section className="rr-memory-detail-source">
                        <h3>原文语境</h3>
                        <blockquote className="rr-memory-source-sentence" lang="en">
                          {selectedRecord.sentence}
                        </blockquote>
                        {selectedRecord.translation ? (
                          <p className="rr-memory-translation">
                            {selectedRecord.translation}
                          </p>
                        ) : null}
                      </section>
                    </>
                  )}

                  <div className="rr-memory-source-row">
                    <span className="rr-memory-source-app">
                      {selectedRecord.app}
                    </span>
                    <span aria-hidden="true">·</span>
                    <time>{selectedRecord.sourceTime}</time>
                  </div>

                  {selectedRecord.history.length ? (
                    <section className="rr-memory-history" aria-label="过去的出现">
                      <button
                        className="rr-memory-history-toggle"
                        data-testid="memory-history-toggle"
                        type="button"
                        aria-expanded={historyExpanded}
                        aria-controls={`rr-memory-history-${selectedRecord.id}`}
                        onClick={() => setHistoryExpanded((expanded) => !expanded)}
                      >
                        <span className="rr-memory-history-label">过去的出现</span>
                        <span className="rr-memory-history-count">
                          {selectedRecord.history.length} 次
                        </span>
                        <MainAppIcon name="chevron" />
                      </button>

                      <div
                        className={`rr-memory-history-list${historyExpanded ? " is-open" : ""}`}
                        id={`rr-memory-history-${selectedRecord.id}`}
                      >
                        {selectedRecord.history.map((occurrence) => (
                          <article
                            className="rr-memory-history-event"
                            key={
                              occurrence.learningRecordId ??
                              `${occurrence.time}-${occurrence.app}`
                            }
                            data-learning-record-id={occurrence.learningRecordId}
                          >
                            <time className="rr-memory-history-time">
                              {occurrence.time}
                            </time>
                            <p className="rr-memory-history-context" lang="en">
                              {occurrence.context}
                            </p>
                            <div className="rr-memory-history-source">
                              {occurrence.app}
                              {occurrence.query
                                ? ` · 原始查询：${occurrence.query}`
                                : ""}
                            </div>
                          </article>
                        ))}
                      </div>
                    </section>
                  ) : null}
                </article>
              ) : (
                <div className="rr-memory-pane-state" role="status">
                  <strong>没有匹配的记录</strong>
                  <p>调整关键词或查询类型后，相关语境会重新出现在这里。</p>
                </div>
              )}
            </div>
          </section>
        </div>
      </section>
    </main>
  );
}

export default MemoryPage;
