import { useEffect, useMemo, useRef, useState } from "react";
import type {
  MemoryFilterId,
  MemoryPageViewModel,
  MemoryRecordItem,
} from "../memoryViewModel";
import MainAppIcon from "./MainAppIcon";

type MemoryPageProps = {
  viewModel: MemoryPageViewModel;
};

function searchableText(record: MemoryRecordItem) {
  return [
    record.query,
    record.summary,
    record.meaning,
    record.sentence,
    record.translation,
    record.app,
  ]
    .join(" ")
    .toLocaleLowerCase("zh-CN");
}

function MemoryPage({ viewModel }: MemoryPageProps) {
  const [searchQuery, setSearchQuery] = useState("");
  const [activeFilter, setActiveFilter] = useState<MemoryFilterId>("all");
  const [selectedId, setSelectedId] = useState(viewModel.records[0]?.id ?? "");
  const [historyExpanded, setHistoryExpanded] = useState(false);
  const [detailOpen, setDetailOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const recordsScrollRef = useRef<HTMLDivElement>(null);
  const detailScrollRef = useRef<HTMLDivElement>(null);
  const detailTitleRef = useRef<HTMLHeadingElement>(null);

  const visibleRecords = useMemo(() => {
    const normalizedQuery = searchQuery.trim().toLocaleLowerCase("zh-CN");

    return viewModel.records.filter((record) => {
      const matchesType = activeFilter === "all" || record.type === activeFilter;
      const matchesQuery =
        !normalizedQuery || searchableText(record).includes(normalizedQuery);
      return matchesType && matchesQuery;
    });
  }, [activeFilter, searchQuery, viewModel.records]);

  const effectiveSelectedId = visibleRecords.some(
    (record) => record.id === selectedId,
  )
    ? selectedId
    : (visibleRecords[0]?.id ?? "");
  const selectedRecord = viewModel.records.find(
    (record) => record.id === effectiveSelectedId,
  );

  useEffect(() => {
    if (effectiveSelectedId !== selectedId) {
      setSelectedId(effectiveSelectedId);
    }
  }, [effectiveSelectedId, selectedId]);

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
  }, [activeFilter, searchQuery]);

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
            ?.querySelector<HTMLElement>(`[data-record-id="${effectiveSelectedId}"]`)
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
  }, [detailOpen, effectiveSelectedId, searchQuery]);

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
    setActiveFilter("all");
    searchInputRef.current?.focus();
  }

  function selectRecord(recordId: string) {
    const compactLayout = window.matchMedia("(max-width: 980px)").matches;
    setSelectedId(recordId);
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

  const isFiltering = activeFilter !== "all" || Boolean(searchQuery.trim());
  const recordCountLabel = isFiltering
    ? `${visibleRecords.length} 条结果`
    : `${viewModel.totalCount} 条记录`;

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
              onChange={(event) => setSearchQuery(event.target.value)}
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
                  onClick={() => setActiveFilter(filter.id)}
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
            >
              {viewModel.groups.map((group) => {
                const groupRecords = visibleRecords.filter(
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
                      const active = record.id === effectiveSelectedId;
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
                          </span>
                        </button>
                      );
                    })}
                  </section>
                );
              })}

              {!visibleRecords.length ? (
                <div className="rr-memory-pane-state" role="status">
                  <strong>没有找到相关语境</strong>
                  <p>试试更短的关键词，或切换查询类型。</p>
                  <button
                    className="rr-memory-state-action"
                    type="button"
                    onClick={resetSearch}
                  >
                    清除搜索条件
                  </button>
                </div>
              ) : null}
            </div>
          </section>

          <section className="rr-memory-detail-pane" aria-label="选中记录详情">
            <div className="rr-memory-detail-scroll" ref={detailScrollRef}>
              {selectedRecord ? (
                <article
                  className="rr-memory-detail-content"
                  id="rr-memory-detail-content"
                  data-testid="memory-detail"
                  data-record-id={selectedRecord.id}
                >
                  <header className="rr-memory-detail-head">
                    <button
                      className="rr-memory-detail-back"
                      type="button"
                      aria-label="返回记忆记录列表"
                      onClick={() => {
                        setDetailOpen(false);
                        window.requestAnimationFrame(() => {
                          recordsScrollRef.current
                            ?.querySelector<HTMLElement>(
                              `[data-record-id="${effectiveSelectedId}"]`,
                            )
                            ?.focus({ preventScroll: true });
                        });
                      }}
                    >
                      <MainAppIcon name="back" />
                    </button>
                    <div>
                      <h2
                        className="rr-memory-detail-title"
                        ref={detailTitleRef}
                        tabIndex={-1}
                      >
                        {selectedRecord.query}
                      </h2>
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
                    </div>
                    <span className="rr-memory-detail-type">
                      {selectedRecord.typeLabel}
                    </span>
                  </header>

                  <section className="rr-memory-detail-section">
                    <h3>基础释义</h3>
                    <p className="rr-memory-definition">
                      {selectedRecord.definition}
                    </p>
                  </section>

                  <section className="rr-memory-detail-section">
                    <h3>在当时语境中的意思</h3>
                    <p className="rr-memory-context-meaning">
                      {selectedRecord.meaning}
                    </p>
                  </section>

                  <section className="rr-memory-detail-section">
                    <h3>当时看到的内容</h3>
                    <blockquote className="rr-memory-source-sentence" lang="en">
                      {selectedRecord.sentence}
                    </blockquote>
                    <p className="rr-memory-translation">
                      {selectedRecord.translation}
                    </p>
                  </section>

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
                            key={`${occurrence.time}-${occurrence.app}`}
                          >
                            <time className="rr-memory-history-time">
                              {occurrence.time}
                            </time>
                            <p className="rr-memory-history-context" lang="en">
                              {occurrence.context}
                            </p>
                            <div className="rr-memory-history-source">
                              {occurrence.app}
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
