import { useEffect, useMemo, useState } from "react";
import {
  countWritingWords,
  getRecordSnapshot,
  type WritingDocumentStatus,
  type WritingDocumentSummary,
} from "../writingViewModel";

type LibraryEntry = {
  record: WritingDocumentSummary;
  status: WritingDocumentStatus;
};

type WritingLibraryProps = {
  records: WritingDocumentSummary[];
  status: "loading" | "ready" | "error";
  error?: string;
  deletingDocumentId?: number;
  onNew: () => void;
  onOpen: (
    record: WritingDocumentSummary,
    status: WritingDocumentStatus,
  ) => void;
  onDelete: (record: WritingDocumentSummary) => void;
  onSearch: (query: string) => void;
  onRetry: () => void;
};

function entryDate(entry: LibraryEntry) {
  return entry.status === "draft"
    ? entry.record.draftUpdatedAtUnixMs ?? entry.record.updatedAtUnixMs
    : entry.record.completedAtUnixMs ?? entry.record.updatedAtUnixMs;
}

function entrySnapshot(entry: LibraryEntry) {
  if (entry.status === "draft" && entry.record.draftSnapshot) {
    return entry.record.draftSnapshot;
  }
  if (entry.status === "completed" && entry.record.completedSnapshot) {
    return entry.record.completedSnapshot;
  }
  return getRecordSnapshot(entry.record);
}

function formatEntryTime(value: number) {
  const date = new Date(value);
  const parts = new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).formatToParts(date);
  const part = (type: Intl.DateTimeFormatPartTypes) =>
    parts.find((item) => item.type === type)?.value ?? "";
  return `${part("month")} 月 ${part("day")} 日 ${part("hour")}:${part(
    "minute",
  )}`;
}

function LibraryRow({
  entry,
  deleting,
  onOpen,
  onDelete,
}: {
  entry: LibraryEntry;
  deleting: boolean;
  onOpen: () => void;
  onDelete: () => void;
}) {
  const snapshot = entrySnapshot(entry);
  return (
    <div className="rr-writing-article-row">
      <button
        className="rr-writing-article-open"
        type="button"
        onClick={onOpen}
      >
        <span className="rr-writing-article-main">
          <span className="rr-writing-article-title">
            {snapshot.title || "未命名文章"}
          </span>
          <span className="rr-writing-article-meta">
            <span className={entry.status === "draft" ? "is-draft" : ""}>
              {entry.status === "draft" ? "修改中" : "已完成"}
            </span>
            <span>{formatEntryTime(entryDate(entry))}</span>
            <span>{countWritingWords(snapshot)} 词</span>
          </span>
          <span className="rr-writing-article-excerpt">
            {snapshot.paragraphs.find(Boolean) || "尚未写下正文"}
          </span>
        </span>
        <span className="rr-writing-article-enter" aria-hidden="true">
          →
        </span>
      </button>
      <button
        className="rr-writing-article-delete"
        type="button"
        disabled={deleting}
        onClick={onDelete}
        aria-label={`删除文章 ${snapshot.title || "未命名文章"}`}
      >
        {deleting ? "删除中…" : "删除"}
      </button>
    </div>
  );
}

function WritingLibrary({
  records,
  status,
  error,
  deletingDocumentId,
  onNew,
  onOpen,
  onDelete,
  onSearch,
  onRetry,
}: WritingLibraryProps) {
  const [filter, setFilter] = useState<"all" | WritingDocumentStatus>("all");
  const [year, setYear] = useState("all");
  const [query, setQuery] = useState("");
  const [sortOrder, setSortOrder] = useState<
    "recent" | "oldest" | "title"
  >("recent");

  useEffect(() => {
    const timer = window.setTimeout(() => onSearch(query.trim()), 260);
    return () => window.clearTimeout(timer);
  }, [onSearch, query]);

  const allEntries = useMemo(() => {
    const entries: LibraryEntry[] = [];
    records.forEach((record) => {
      if (record.draftSnapshot) {
        entries.push({ record, status: "draft" });
      }
      if (record.completedSnapshot) {
        entries.push({ record, status: "completed" });
      }
    });
    return entries;
  }, [records]);

  const years = useMemo(
    () =>
      Array.from(
        new Set(
          allEntries.map((entry) =>
            String(new Date(entryDate(entry)).getFullYear()),
          ),
        ),
      ).sort((first, second) => Number(second) - Number(first)),
    [allEntries],
  );

  const filteredEntries = useMemo(() => {
    const matches = allEntries.filter((entry) => {
      if (filter !== "all" && entry.status !== filter) {
        return false;
      }
      if (
        filter === "all" &&
        entry.status === "completed" &&
        entry.record.draftSnapshot
      ) {
        return false;
      }
      return (
        year === "all" ||
        String(new Date(entryDate(entry)).getFullYear()) === year
      );
    });
    return matches.sort((first, second) => {
      if (sortOrder === "title") {
        return entrySnapshot(first).title.localeCompare(
          entrySnapshot(second).title,
          "zh-CN",
          { sensitivity: "base" },
        );
      }
      const delta = entryDate(first) - entryDate(second);
      return sortOrder === "oldest" ? delta : -delta;
    });
  }, [allEntries, filter, sortOrder, year]);

  const draftCount = records.filter((record) => record.draftSnapshot).length;
  const completedCount = records.filter(
    (record) => record.completedSnapshot,
  ).length;
  const allCount = new Set(
    records
      .filter((record) => record.draftSnapshot || record.completedSnapshot)
      .map((record) => record.id),
  ).size;
  const contextLabels = {
    all: "最近写作",
    draft: "继续写",
    completed: "已完成",
  } as const;

  return (
    <section
      className="rr-writing-library"
      aria-labelledby="rr-writing-library-heading"
      data-testid="writing-library-view"
    >
      <div className="rr-writing-library-shell">
        <header className="rr-writing-library-head">
          <div>
            <p>本地写作归档</p>
            <h1 id="rr-writing-library-heading">写作</h1>
            <span>
              未完成的文章从这里继续，完成稿会安静保存在本机；重新打开时仍保留当时的版本和修改记录。
            </span>
          </div>
          <div className="rr-writing-library-head-actions">
            <input
              type="search"
              value={query}
              aria-label="搜索文章"
              placeholder="搜索标题或正文"
              onChange={(event) => setQuery(event.target.value)}
            />
            <button
              className="rr-writing-btn is-secondary"
              type="button"
              onClick={onNew}
            >
              新建文章
            </button>
          </div>
        </header>

        <div className="rr-writing-library-layout">
          <aside className="rr-writing-library-index" aria-label="文章分类">
            <p>状态</p>
            <div role="tablist" aria-label="文章状态">
              {(
                [
                  ["all", "全部", allCount],
                  ["draft", "继续写", draftCount],
                  ["completed", "已完成", completedCount],
                ] as const
              ).map(([value, label, count]) => (
                <button
                  type="button"
                  role="tab"
                  key={value}
                  className={filter === value ? "is-active" : ""}
                  aria-selected={filter === value}
                  onClick={() => setFilter(value)}
                >
                  <span>{label}</span>
                  <span>{count}</span>
                </button>
              ))}
            </div>
            <p className="is-year">年份</p>
            <div>
              {["all", ...years].map((value) => {
                const count =
                  value === "all"
                    ? allCount
                    : new Set(
                        allEntries
                          .filter(
                            (entry) =>
                              String(
                                new Date(entryDate(entry)).getFullYear(),
                              ) === value,
                          )
                          .map((entry) => entry.record.id),
                      ).size;
                return (
                  <button
                    type="button"
                    key={value}
                    className={year === value ? "is-active" : ""}
                    aria-pressed={year === value}
                    onClick={() => setYear(value)}
                  >
                    <span>
                      {value === "all" ? "全部年份" : value}
                    </span>
                    <span>{count}</span>
                  </button>
                );
              })}
            </div>
          </aside>

          <main className="rr-writing-library-catalog">
            <div className="rr-writing-library-catalog-head">
              <div>
                <h2>
                  {year === "all"
                    ? contextLabels[filter]
                    : `${year} 年 · ${contextLabels[filter]}`}
                </h2>
                {query || filter !== "all" || year !== "all" ? (
                  <span>
                    {query ? `“${query}” · ` : ""}
                    {filteredEntries.length} 篇文章
                  </span>
                ) : null}
              </div>
              <select
                value={sortOrder}
                aria-label="文章排序"
                onChange={(event) =>
                  setSortOrder(event.target.value as typeof sortOrder)
                }
              >
                <option value="recent">最近更新</option>
                <option value="oldest">最早更新</option>
                <option value="title">标题 A–Z</option>
              </select>
            </div>

            {status === "error" ? (
              <div className="rr-writing-library-error" role="alert">
                <p>{error || "写作文章加载失败。"}</p>
                <button type="button" onClick={onRetry}>
                  重试
                </button>
              </div>
            ) : (
              <div className="rr-writing-article-list">
                {filteredEntries.map((entry) => (
                  <LibraryRow
                    key={`${entry.record.id}-${entry.status}`}
                    entry={entry}
                    deleting={deletingDocumentId === entry.record.id}
                    onOpen={() => onOpen(entry.record, entry.status)}
                    onDelete={() => onDelete(entry.record)}
                  />
                ))}
              </div>
            )}
            {status === "loading" ? (
              <p className="rr-writing-library-empty">正在读取本机文章…</p>
            ) : null}
            {status === "ready" && !filteredEntries.length ? (
              <p className="rr-writing-library-empty">
                {query
                  ? "没有找到匹配的文章。"
                  : filter === "draft"
                    ? "目前没有需要继续写的文章。"
                    : filter === "completed"
                      ? "完成的文章会安静保存在这里。"
                      : "文章会在这里形成你的个人写作目录。"}
              </p>
            ) : null}
          </main>
        </div>
      </div>
    </section>
  );
}

export default WritingLibrary;
