import { useMemo, useState } from "react";
import {
  countWritingWords,
  getRecordSnapshot,
  normalizeWritingText,
  type WritingDocumentRecord,
  type WritingDocumentStatus,
} from "../writingViewModel";

type LibraryEntry = {
  record: WritingDocumentRecord;
  status: WritingDocumentStatus;
};

type WritingLibraryProps = {
  records: WritingDocumentRecord[];
  onNew: () => void;
  onOpen: (record: WritingDocumentRecord, status: WritingDocumentStatus) => void;
};

function entryDate(entry: LibraryEntry) {
  return entry.status === "draft"
    ? entry.record.draftUpdatedAt ?? entry.record.updatedAt
    : entry.record.completedAt ?? entry.record.updatedAt;
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

function formatEntryTime(value: string) {
  const date = new Date(value);
  const parts = new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).formatToParts(date);
  const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((item) => item.type === type)?.value ?? "";
  return `${part("month")} 月 ${part("day")} 日 ${part("hour")}:${part("minute")}`;
}

function LibraryRow({ entry, onOpen }: { entry: LibraryEntry; onOpen: () => void }) {
  const snapshot = entrySnapshot(entry);
  return (
    <button className="rr-writing-article-row" type="button" onClick={onOpen}>
      <span className="rr-writing-article-main">
        <span className="rr-writing-article-title">{snapshot.title || "未命名文章"}</span>
        <span className="rr-writing-article-meta">
          <span className={entry.status === "draft" ? "is-draft" : ""}>{entry.status === "draft" ? "修改中" : "已完成"}</span>
          <span>{formatEntryTime(entryDate(entry))}</span>
          <span>{countWritingWords(snapshot)} 词</span>
        </span>
        <span className="rr-writing-article-excerpt">{snapshot.paragraphs.find(Boolean) || "尚未写下正文"}</span>
      </span>
      <span className="rr-writing-article-enter" aria-hidden="true">→</span>
    </button>
  );
}

function WritingLibrary({ records, onNew, onOpen }: WritingLibraryProps) {
  const [filter, setFilter] = useState<"all" | WritingDocumentStatus>("all");
  const [year, setYear] = useState("all");
  const [query, setQuery] = useState("");
  const [sortOrder, setSortOrder] = useState<"recent" | "oldest" | "title">("recent");

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

  const years = useMemo(() => (
    Array.from(new Set(allEntries.map((entry) => String(new Date(entryDate(entry)).getFullYear()))))
      .sort((first, second) => Number(second) - Number(first))
  ), [allEntries]);

  const filteredEntries = useMemo(() => {
    const normalizedQuery = normalizeWritingText(query).toLocaleLowerCase("zh-CN");
    const matches = allEntries.filter((entry) => {
      if (filter !== "all" && entry.status !== filter) {
        return false;
      }
      if (filter === "all" && entry.status === "completed" && entry.record.draftSnapshot) {
        return false;
      }
      if (year !== "all" && String(new Date(entryDate(entry)).getFullYear()) !== year) {
        return false;
      }
      if (!normalizedQuery) {
        return true;
      }
      const snapshot = entrySnapshot(entry);
      return normalizeWritingText([snapshot.title, ...snapshot.paragraphs].join(" "))
        .toLocaleLowerCase("zh-CN")
        .includes(normalizedQuery);
    });
    return matches.sort((first, second) => {
      if (sortOrder === "title") {
        return entrySnapshot(first).title.localeCompare(entrySnapshot(second).title, "zh-CN", { sensitivity: "base" });
      }
      const delta = new Date(entryDate(first)).getTime() - new Date(entryDate(second)).getTime();
      return sortOrder === "oldest" ? delta : -delta;
    });
  }, [allEntries, filter, query, sortOrder, year]);

  const draftCount = records.filter((record) => record.draftSnapshot).length;
  const completedCount = records.filter((record) => record.completedSnapshot).length;
  const allCount = new Set(records.filter((record) => record.draftSnapshot || record.completedSnapshot).map((record) => record.id)).size;
  const contextLabels = { all: "最近写作", draft: "继续写", completed: "已完成" } as const;

  return (
    <section className="rr-writing-library" aria-labelledby="rr-writing-library-heading" data-testid="writing-library-view">
      <div className="rr-writing-library-shell">
        <header className="rr-writing-library-head">
          <div>
            <p>本地写作归档</p>
            <h1 id="rr-writing-library-heading">写作</h1>
            <span>未完成的文章从这里继续，完成稿会安静保存在本机；重新打开时仍保留当时的版本和修改记录。</span>
          </div>
          <div className="rr-writing-library-head-actions">
            <input
              type="search"
              value={query}
              aria-label="搜索文章"
              placeholder="搜索标题或正文"
              onChange={(event) => setQuery(event.target.value)}
            />
            <button className="rr-writing-btn is-secondary" type="button" onClick={onNew}>新建文章</button>
          </div>
        </header>

        <div className="rr-writing-library-layout">
          <aside className="rr-writing-library-index" aria-label="文章分类">
            <p>状态</p>
            <div role="tablist" aria-label="文章状态">
              {([
                ["all", "全部", allCount],
                ["draft", "继续写", draftCount],
                ["completed", "已完成", completedCount],
              ] as const).map(([value, label, count]) => (
                <button
                  type="button"
                  role="tab"
                  key={value}
                  className={filter === value ? "is-active" : ""}
                  aria-selected={filter === value}
                  onClick={() => setFilter(value)}
                ><span>{label}</span><span>{count}</span></button>
              ))}
            </div>
            <p className="is-year">年份</p>
            <div>
              {["all", ...years].map((value) => {
                const count = value === "all"
                  ? allCount
                  : new Set(allEntries.filter((entry) => String(new Date(entryDate(entry)).getFullYear()) === value).map((entry) => entry.record.id)).size;
                return (
                  <button
                    type="button"
                    key={value}
                    className={year === value ? "is-active" : ""}
                    aria-pressed={year === value}
                    onClick={() => setYear(value)}
                  ><span>{value === "all" ? "全部年份" : value}</span><span>{count}</span></button>
                );
              })}
            </div>
          </aside>

          <main className="rr-writing-library-catalog">
            <div className="rr-writing-library-catalog-head">
              <div>
                <h2>{year === "all" ? contextLabels[filter] : `${year} 年 · ${contextLabels[filter]}`}</h2>
                {query || filter !== "all" || year !== "all" ? <span>{query ? `“${query}” · ` : ""}{filteredEntries.length} 篇文章</span> : null}
              </div>
              <select
                value={sortOrder}
                aria-label="文章排序"
                onChange={(event) => setSortOrder(event.target.value as typeof sortOrder)}
              >
                <option value="recent">最近更新</option>
                <option value="oldest">最早更新</option>
                <option value="title">标题 A–Z</option>
              </select>
            </div>

            <div className="rr-writing-article-list">
              {filteredEntries.map((entry) => (
                <LibraryRow
                  key={`${entry.record.id}-${entry.status}`}
                  entry={entry}
                  onOpen={() => onOpen(entry.record, entry.status)}
                />
              ))}
            </div>
            {!filteredEntries.length ? (
              <p className="rr-writing-library-empty">{query
                ? "没有找到匹配的文章。"
                : filter === "draft" ? "目前没有需要继续写的文章。"
                  : filter === "completed" ? "完成的文章会安静保存在这里。"
                    : "文章会在这里形成你的个人写作目录。"}</p>
            ) : null}
          </main>
        </div>
      </div>
    </section>
  );
}

export default WritingLibrary;
