import type { AgentSource } from "../conversationViewModel";

function sourceHostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}

/** 来源卡片列表（任务 3）：展示标题、站点与安全 URL；点击经受控 opener 打开。 */
export function AgentSourceList({
  sources,
  onOpen,
}: {
  sources: AgentSource[];
  onOpen: (source: AgentSource) => void;
}) {
  return (
    <section className="rr-agent-sources" aria-label="回答来源">
      <h4 className="rr-agent-sources-title">来源</h4>
      <ol className="rr-agent-source-list">
        {sources.map((source, index) => (
          <li key={source.sourceId}>
            <button
              className="rr-agent-source-card"
              type="button"
              title={source.url}
              onClick={() => onOpen(source)}
            >
              <span className="rr-agent-source-index">{index + 1}</span>
              <span className="rr-agent-source-copy">
                <span className="rr-agent-source-title">
                  {source.title || "Web source"}
                </span>
                <span className="rr-agent-source-site">
                  {source.siteName ?? sourceHostname(source.url)}
                </span>
              </span>
            </button>
          </li>
        ))}
      </ol>
    </section>
  );
}
