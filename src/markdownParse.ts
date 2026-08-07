/**
 * ReadRay 对话页的轻量 Markdown 白名单解析器。
 *
 * 只支持白名单子集：段落与软换行、`#`/`##`/`###` 标题、`**粗体**`、
 * `*斜体*`、`~~删除线~~`、行内代码、多行代码块、无序/有序列表、引用、
 * 分隔线，以及链接 `[text](url)`（仅作为文本 + 可见 URL，不生成可点击元素）。
 * 其余语法（表格、HTML 标签等）一律按纯文本处理；输入永不作为 HTML 注入。
 *
 * 流式兼容：`streaming` 模式下，未闭合代码块按代码块渲染（不显示围栏标记）、
 * 未闭合行内标记隐藏起始符号，避免生成过程中闪现原始标记；完成态未闭合
 * 语法一律保留原文、降级为纯文本。
 *
 * 本模块只做解析与类型化 token 输出，不依赖 React，可由 Node 测试直接运行。
 */

export type MarkdownInline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong"; text: string }
  | { kind: "em"; text: string }
  | { kind: "del"; text: string }
  | { kind: "link"; text: string; url: string };

export type MarkdownBlock =
  | { kind: "paragraph"; inlines: MarkdownInline[] }
  | { kind: "heading"; level: 1 | 2 | 3; inlines: MarkdownInline[] }
  | { kind: "codeBlock"; text: string }
  | { kind: "list"; ordered: boolean; items: MarkdownInline[][] }
  | { kind: "quote"; inlines: MarkdownInline[] }
  | { kind: "hr" };

export type MarkdownParseOptions = {
  /** 流式生成中：未闭合语法按"正在生成"的宽容方式渲染。 */
  streaming?: boolean;
};

const plainText = (value: string): MarkdownInline => ({ kind: "text", text: value });

function isFenceLine(line: string) {
  return line.trimStart().startsWith("```");
}

function isHrLine(line: string) {
  return /^(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line);
}

function parseHeadingLine(
  line: string,
): { level: 1 | 2 | 3; content: string } | null {
  const match = /^(#{1,3})\s+(.+)$/.exec(line);
  if (!match) {
    return null;
  }
  return { level: match[1].length as 1 | 2 | 3, content: match[2] };
}

function parseListItemLine(
  line: string,
): { ordered: boolean; content: string } | null {
  let match = /^[-*+]\s+(.*)$/.exec(line);
  if (match) {
    return { ordered: false, content: match[1] };
  }
  match = /^\d+[.)]\s+(.*)$/.exec(line);
  if (match) {
    return { ordered: true, content: match[1] };
  }
  return null;
}

const INLINE_MARKS = ["**", "~~", "`", "[", "*"] as const;

function nextInlineMark(text: string) {
  let best: { index: number; mark: string } | null = null;
  for (const mark of INLINE_MARKS) {
    const index = text.indexOf(mark);
    if (index >= 0 && (best === null || index < best.index)) {
      best = { index, mark };
    }
  }
  return best;
}

function tryParseInlineStart(
  rest: string,
  streaming: boolean,
): { inline: MarkdownInline; rest: string } | null {
  if (rest.startsWith("**")) {
    const end = rest.indexOf("**", 2);
    if (end > 2) {
      return {
        inline: { kind: "strong", text: rest.slice(2, end) },
        rest: rest.slice(end + 2),
      };
    }
    return {
      inline: plainText(streaming ? rest.slice(2) : rest),
      rest: "",
    };
  }
  if (rest.startsWith("~~")) {
    const end = rest.indexOf("~~", 2);
    if (end > 2) {
      return {
        inline: { kind: "del", text: rest.slice(2, end) },
        rest: rest.slice(end + 2),
      };
    }
    return {
      inline: plainText(streaming ? rest.slice(2) : rest),
      rest: "",
    };
  }
  if (rest.startsWith("`")) {
    const end = rest.indexOf("`", 1);
    if (end > 1) {
      return {
        inline: { kind: "code", text: rest.slice(1, end) },
        rest: rest.slice(end + 1),
      };
    }
    return {
      inline: plainText(streaming ? rest.slice(1) : rest),
      rest: "",
    };
  }
  if (rest.startsWith("*")) {
    const end = rest.indexOf("*", 1);
    if (end > 1) {
      return {
        inline: { kind: "em", text: rest.slice(1, end) },
        rest: rest.slice(end + 1),
      };
    }
    return {
      inline: plainText(streaming ? rest.slice(1) : rest),
      rest: "",
    };
  }
  if (rest.startsWith("[")) {
    const match = /^\[([^\]]*)\]\(([^)\s]+)\)/.exec(rest);
    if (match) {
      const url = match[2];
      // 只接受 http/https，其余协议（javascript:、data: 等）按纯文本降级
      if (/^https?:\/\//i.test(url)) {
        return {
          inline: { kind: "link", text: match[1], url },
          rest: rest.slice(match[0].length),
        };
      }
    }
    return { inline: plainText("["), rest: rest.slice(1) };
  }
  return null;
}

function parseInline(text: string, streaming: boolean): MarkdownInline[] {
  const inlines: MarkdownInline[] = [];
  let rest = text;
  let textBuffer = "";
  const flushText = () => {
    if (textBuffer) {
      inlines.push(plainText(textBuffer));
      textBuffer = "";
    }
  };

  while (rest.length > 0) {
    const next = nextInlineMark(rest);
    if (!next) {
      textBuffer += rest;
      break;
    }
    if (next.index > 0) {
      textBuffer += rest.slice(0, next.index);
      rest = rest.slice(next.index);
      continue;
    }
    const parsed = tryParseInlineStart(rest, streaming);
    if (parsed) {
      flushText();
      if (parsed.inline.kind !== "text" || parsed.inline.text) {
        inlines.push(parsed.inline);
      }
      rest = parsed.rest;
    } else {
      textBuffer += rest[0];
      rest = rest.slice(1);
    }
  }
  flushText();
  return inlines;
}

export function parseMarkdown(
  text: string,
  options: MarkdownParseOptions = {},
): MarkdownBlock[] {
  const streaming = options.streaming ?? false;
  const lines = text.split("\n");
  const blocks: MarkdownBlock[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];

    if (isFenceLine(line)) {
      const fenceText = [line];
      index += 1;
      let closed = false;
      while (index < lines.length) {
        const current = lines[index];
        if (isFenceLine(current)) {
          closed = true;
          index += 1;
          break;
        }
        fenceText.push(current);
        index += 1;
      }
      if (closed || streaming) {
        blocks.push({ kind: "codeBlock", text: fenceText.slice(1).join("\n") });
      } else {
        // 完成态未闭合代码块：整体降级为原文纯文本
        blocks.push({
          kind: "paragraph",
          inlines: [plainText(fenceText.join("\n"))],
        });
      }
      continue;
    }

    if (line.trim() === "") {
      index += 1;
      continue;
    }

    if (isHrLine(line)) {
      blocks.push({ kind: "hr" });
      index += 1;
      continue;
    }

    const heading = parseHeadingLine(line);
    if (heading) {
      blocks.push({
        kind: "heading",
        level: heading.level,
        inlines: parseInline(heading.content, streaming),
      });
      index += 1;
      continue;
    }

    if (line.startsWith(">")) {
      const quoteLines: string[] = [];
      while (index < lines.length && lines[index].startsWith(">")) {
        quoteLines.push(lines[index].replace(/^>\s?/, ""));
        index += 1;
      }
      blocks.push({
        kind: "quote",
        inlines: parseInline(quoteLines.join("\n"), streaming),
      });
      continue;
    }

    const listItem = parseListItemLine(line);
    if (listItem) {
      const ordered = listItem.ordered;
      const items: MarkdownInline[][] = [
        parseInline(listItem.content, streaming),
      ];
      index += 1;
      while (index < lines.length) {
        const nextItem = parseListItemLine(lines[index]);
        if (!nextItem || nextItem.ordered !== ordered) {
          break;
        }
        items.push(parseInline(nextItem.content, streaming));
        index += 1;
      }
      blocks.push({ kind: "list", ordered, items });
      continue;
    }

    const paragraphLines: string[] = [];
    while (index < lines.length) {
      const current = lines[index];
      if (
        current.trim() === "" ||
        isFenceLine(current) ||
        isHrLine(current) ||
        parseHeadingLine(current) ||
        current.startsWith(">") ||
        parseListItemLine(current)
      ) {
        break;
      }
      paragraphLines.push(current);
      index += 1;
    }
    blocks.push({
      kind: "paragraph",
      inlines: parseInline(paragraphLines.join("\n"), streaming),
    });
  }

  return blocks;
}
