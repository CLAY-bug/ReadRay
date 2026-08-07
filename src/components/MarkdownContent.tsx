import type { ReactNode } from "react";
import { parseMarkdown, type MarkdownBlock, type MarkdownInline } from "../markdownParse";

/**
 * 对话页 assistant 回答的 Markdown 渲染组件。
 *
 * 只渲染 `parseMarkdown` 产出的白名单 token，输入文本永不作为 HTML 注入；
 * 链接只展示可见文本 + 完整 URL，不生成可点击元素（避免引入 opener 与安全边界）。
 * 样式全部由 `rr-conversation-*` 作用域承担，本组件不输出固定色值或布局。
 */

function renderInlines(inlines: MarkdownInline[]): ReactNode[] {
  return inlines.map((inline, index) => {
    switch (inline.kind) {
      case "code":
        return <code key={index}>{inline.text}</code>;
      case "strong":
        return <strong key={index}>{inline.text}</strong>;
      case "em":
        return <em key={index}>{inline.text}</em>;
      case "del":
        return <del key={index}>{inline.text}</del>;
      case "link":
        return (
          <span className="rr-conversation-markdown-link" key={index}>
            {inline.text || inline.url}
            <span className="rr-conversation-markdown-url">（{inline.url}）</span>
          </span>
        );
      default:
        return <span key={index}>{inline.text}</span>;
    }
  });
}

function renderBlock(block: MarkdownBlock, key: number): ReactNode {
  switch (block.kind) {
    case "heading":
      if (block.level === 1) {
        return <h3 className="rr-conversation-markdown-h1" key={key}>{renderInlines(block.inlines)}</h3>;
      }
      if (block.level === 2) {
        return <h4 className="rr-conversation-markdown-h2" key={key}>{renderInlines(block.inlines)}</h4>;
      }
      return <h5 className="rr-conversation-markdown-h3" key={key}>{renderInlines(block.inlines)}</h5>;
    case "codeBlock":
      return (
        <pre className="rr-conversation-code-block" key={key}>
          <code>{block.text}</code>
        </pre>
      );
    case "list":
      if (block.ordered) {
        return (
          <ol className="rr-conversation-markdown-list" key={key}>
            {block.items.map((item, itemIndex) => (
              <li key={itemIndex}>{renderInlines(item)}</li>
            ))}
          </ol>
        );
      }
      return (
        <ul className="rr-conversation-markdown-list" key={key}>
          {block.items.map((item, itemIndex) => (
            <li key={itemIndex}>{renderInlines(item)}</li>
          ))}
        </ul>
      );
    case "quote":
      return (
        <blockquote className="rr-conversation-markdown-quote" key={key}>
          {renderInlines(block.inlines)}
        </blockquote>
      );
    case "hr":
      return <hr className="rr-conversation-markdown-hr" key={key} />;
    default:
      return <p key={key}>{renderInlines(block.inlines)}</p>;
  }
}

export function MarkdownContent({
  text,
  streaming = false,
}: {
  text: string;
  streaming?: boolean;
}) {
  const blocks = parseMarkdown(text, { streaming });
  return <>{blocks.map((block, index) => renderBlock(block, index))}</>;
}
