import assert from "node:assert/strict";
import test from "node:test";
import { parseMarkdown } from "../src/markdownParse.ts";

function inlineText(inlines) {
  return inlines.map((inline) => inline.text).join("");
}

function renderBlockText(block) {
  if (block.kind === "codeBlock") {
    return block.text;
  }
  if (block.kind === "list") {
    return block.items.map((item) => inlineText(item)).join("\n");
  }
  if (block.inlines) {
    return inlineText(block.inlines);
  }
  return "";
}

function renderInlineText(text) {
  const blocks = typeof text === "string" ? parseMarkdown(text) : [text];
  return blocks.map(renderBlockText).join("\n");
}

test("基础子集：标题、粗体、斜体、删除线、行内代码与分隔线", () => {
  const blocks = parseMarkdown(
    [
      "## 使用说明",
      "这是 **粗体**、*斜体*、~~删除线~~ 和 `code` 的示例。",
      "---",
      "结尾段落。",
    ].join("\n"),
  );

  assert.equal(blocks.length, 4);
  assert.deepEqual(blocks[0], {
    kind: "heading",
    level: 2,
    inlines: [{ kind: "text", text: "使用说明" }],
  });
  assert.deepEqual(blocks[1].inlines, [
    { kind: "text", text: "这是 " },
    { kind: "strong", text: "粗体" },
    { kind: "text", text: "、" },
    { kind: "em", text: "斜体" },
    { kind: "text", text: "、" },
    { kind: "del", text: "删除线" },
    { kind: "text", text: " 和 " },
    { kind: "code", text: "code" },
    { kind: "text", text: " 的示例。" },
  ]);
  assert.deepEqual(blocks[2], { kind: "hr" });
});

test("标题只识别 #/##/### 且必须带空格，#### 按纯文本", () => {
  const blocks = parseMarkdown("# 一级\n#### 四级\n#没有空格");
  assert.equal(blocks.length, 2);
  assert.equal(blocks[0].kind, "heading");
  assert.equal(blocks[0].level, 1);
  assert.equal(blocks[1].kind, "paragraph");
  assert.equal(inlineText(blocks[1].inlines), "#### 四级\n#没有空格");
});

test("无序与有序列表", () => {
  const blocks = parseMarkdown("- 第一项\n- 第二项\n1. 一\n2. 二");
  assert.equal(blocks.length, 2);
  assert.deepEqual(blocks[0], {
    kind: "list",
    ordered: false,
    items: [
      [{ kind: "text", text: "第一项" }],
      [{ kind: "text", text: "第二项" }],
    ],
  });
  assert.deepEqual(blocks[1], {
    kind: "list",
    ordered: true,
    items: [
      [{ kind: "text", text: "一" }],
      [{ kind: "text", text: "二" }],
    ],
  });
});

test("列表项支持行内标记，列表前后行不属于列表", () => {
  const blocks = parseMarkdown("- 带 **强调** 的项\n\n列表后的正文");
  assert.equal(blocks.length, 2);
  assert.equal(blocks[0].kind, "list");
  assert.deepEqual(blocks[0].items[0], [
    { kind: "text", text: "带 " },
    { kind: "strong", text: "强调" },
    { kind: "text", text: " 的项" },
  ]);
  assert.equal(blocks[1].kind, "paragraph");
  assert.equal(renderBlockText(blocks[1]), "列表后的正文");
});

test("代码块完整渲染且不解析内部标记", () => {
  const blocks = parseMarkdown("```\nconst x = **not strong**;\n```\n\n后面段落");
  assert.equal(blocks.length, 2);
  assert.deepEqual(blocks[0], {
    kind: "codeBlock",
    text: "const x = **not strong**;",
  });
  assert.equal(blocks[1].kind, "paragraph");
  assert.equal(renderBlockText(blocks[1]), "后面段落");
});

test("引用与多行引用", () => {
  const blocks = parseMarkdown("> 引用第一行\n> 引用第二行\n\n普通段落");
  assert.equal(blocks.length, 2);
  assert.equal(blocks[0].kind, "quote");
  assert.equal(renderBlockText(blocks[0]), "引用第一行\n引用第二行");
  assert.equal(blocks[1].kind, "paragraph");
});

test("链接只渲染可见文本和完整 URL，不生成可点击元素", () => {
  const blocks = parseMarkdown("看这个 [官方文档](https://example.com/docs)");
  assert.deepEqual(blocks[0].inlines[1], {
    kind: "link",
    text: "官方文档",
    url: "https://example.com/docs",
  });
  const blocks2 = parseMarkdown("[没有链接](http://x.invalid)");
  assert.equal(blocks2[0].inlines[0].kind, "link");
  assert.equal(blocks2[0].inlines[0].url, "http://x.invalid");
});

test("畸形链接按纯文本降级", () => {
  assert.equal(renderInlineText("[未闭合链接"), "[未闭合链接");
  assert.equal(renderInlineText("[文本](缺右括号"), "[文本](缺右括号");
  assert.equal(renderInlineText("纯文本中的 [a] [b]"), "纯文本中的 [a] [b]");
});

test("未闭合行内标记在完成态按纯文本显示", () => {
  const text = "有 **粗体未闭合 和 `代码未闭合";
  const blocks = parseMarkdown(text);
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, "paragraph");
  assert.equal(renderInlineText(text), "有 **粗体未闭合 和 `代码未闭合");
});

test("流式未闭合代码块按代码块渲染且不闪现围栏", () => {
  const blocks = parseMarkdown("```\nconst a = 1;", { streaming: true });
  assert.equal(blocks.length, 1);
  assert.deepEqual(blocks[0], { kind: "codeBlock", text: "const a = 1;" });
});

test("完成态未闭合代码块整体降级为纯文本段落", () => {
  const blocks = parseMarkdown("```\nconst a = 1;");
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, "paragraph");
  assert.equal(renderInlineText("```\nconst a = 1;"), "```\nconst a = 1;");
});

test("流式未闭合行内标记隐藏起始符号，内容先以纯文本展示", () => {
  const blocks = parseMarkdown("正在写 **重点内容", { streaming: true });
  assert.equal(blocks.length, 1);
  assert.deepEqual(blocks[0].inlines, [
    { kind: "text", text: "正在写 " },
    { kind: "text", text: "重点内容" },
  ]);
  assert.equal(
    blocks[0].inlines.map((inline) => inline.text).join(""),
    "正在写 重点内容",
  );
});

test("流式中途的列表与标题稳定渲染，序号从第一个字符开始", () => {
  const blocks = parseMarkdown("1. 第一项\n2. 第", { streaming: true });
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, "list");
  assert.equal(blocks[0].ordered, true);
  assert.equal(blocks[0].items.length, 2);
  assert.equal(blocks[0].items[1][0].text, "第");
});

test("表格语法不进入白名单，按纯文本保留", () => {
  const text = "| 词 | 含义 |\n| --- | --- |\n| context | 语境 |";
  assert.equal(renderInlineText(text), text);
});

test("HTML 标签透传为纯文本", () => {
  assert.equal(renderInlineText("<b>bold</b>"), "<b>bold</b>");
  assert.equal(renderInlineText("<br>"), "<br>");
});

test("恶意输入全部降级为文本，无任何可执行元素", () => {
  const samples = [
    "<script>alert('xss')</script>",
    "<img src=x onerror=alert(1)>",
    "[点我](javascript:alert(1))",
    "[点我](data:text/html,<script>alert(1)</script>)",
    "<iframe src=\"https://evil.example\"></iframe>",
    "`onerror=alert(1)`",
  ];
  for (const sample of samples) {
    const blocks = parseMarkdown(sample);
    const kinds = blocks.flatMap((block) => {
      const blockKinds = [block.kind];
      if (block.kind === "paragraph" || block.kind === "heading" || block.kind === "quote") {
        blockKinds.push(...block.inlines.map((inline) => inline.kind));
      }
      return blockKinds;
    });
    for (const kind of kinds) {
      assert.equal(
        kind === "code" || kind === "text" || kind === "paragraph",
        true,
        `样本必须只产生 text/code/paragraph：${sample} -> ${kind}`,
      );
    }
    if (sample.startsWith("[")) {
      assert.equal(
        blocks[0].inlines.some((inline) => inline.kind === "link" && inline.url.startsWith("javascript:")),
        false,
        "javascript: 链接不得解析成 link token",
      );
    }
  }
});

test("恶意链接 URL 在渲染层只是文本字段", () => {
  const blocks = parseMarkdown("[x](javascript:alert(1))");
  assert.equal(
    blocks[0].inlines.every((inline) => inline.kind === "text"),
    true,
  );
  assert.equal(
    blocks[0].inlines.map((inline) => inline.text).join(""),
    "[x](javascript:alert(1))",
  );
});

test("连续等宽的 * 只可能形成单一标记", () => {
  assert.equal(renderInlineText("**bold**"), "bold");
  assert.equal(renderInlineText("*italic*"), "italic");
});

test("空文本与纯文本不产生额外结构", () => {
  assert.deepEqual(parseMarkdown(""), []);
  const blocks = parseMarkdown("只有一行普通文本");
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, "paragraph");
  assert.equal(blocks[0].inlines.map((inline) => inline.text).join(""), "只有一行普通文本");
});

test("千行文本解析不崩溃且输出稳定", () => {
  const line = "- 一个列表项\n";
  const text = line.repeat(2000);
  const blocks = parseMarkdown(text);
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, "list");
  assert.equal(blocks[0].items.length, 2000);
});
