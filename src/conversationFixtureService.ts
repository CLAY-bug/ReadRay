import type {
  ConversationAnswerBlock,
  ConversationExportResult,
  ConversationGenerationRequest,
  ConversationMemoryCitation,
  ConversationService,
  ConversationThread,
} from "./conversationViewModel";

const text = (value: string) => ({ kind: "text" as const, text: value });
const code = (value: string) => ({ kind: "code" as const, text: value });

const memoryCitation: ConversationMemoryCitation = {
  title: "context 与 circumstance 的区别",
  typeLabel: "短语",
  sourceApp: "Obsidian",
  recordedAt: "今天 07:54",
  excerpt:
    "阅读文本时优先用 context 表示“上下文”；描述限制决策的现实条件时更适合 circumstance。",
};

const memoryBlocks: ConversationAnswerBlock[] = [
  {
    kind: "paragraph",
    tone: "lead",
    content: [
      code("under this context"),
      text(" 可以理解，但英语里通常更自然地说 "),
      code("in this context"),
      text("。"),
    ],
  },
  {
    kind: "list",
    items: [
      [
        code("context"),
        text(" 指帮助人理解一句话、一个词或一件事的语境与背景信息。"),
      ],
      [
        code("circumstances"),
        text(" 指事情发生时的现实条件、处境或限制。"),
      ],
    ],
  },
  {
    kind: "example",
    english: "The word changes meaning in this context.",
    translation: "这个词在这一语境下含义会发生变化。",
  },
  {
    kind: "example",
    english: "The decision was reasonable under the circumstances.",
    translation: "在当时的情况下，这个决定是合理的。",
  },
  {
    kind: "paragraph",
    content: [
      text("强调语境时，还可以说 "),
      code("given this context"),
      text(" 或 "),
      code("in the context of ..."),
      text("；描述现实条件时则常用 "),
      code("under the circumstances"),
      text("。"),
    ],
  },
];

const threads: Record<string, ConversationThread> = {
  memory: {
    id: "memory",
    title: "context 与 circumstance 的区别",
    messages: [
      {
        id: "memory-user",
        role: "user",
        content:
          "context 和 circumstance 到底怎么区分？我写 under this context 感觉怪怪的。",
        meta: "来自今天 · 10:12",
      },
      {
        id: "memory-assistant",
        role: "assistant",
        blocks: memoryBlocks,
        citation: memoryCitation,
      },
    ],
  },
  direct: {
    id: "direct",
    title: "carry out 和 conduct 的区别",
    messages: [
      {
        id: "direct-user",
        role: "user",
        content: "carry out 和 conduct 都能表示“进行”，实际写作时怎么选？",
        meta: "今天 · 08:48",
      },
      {
        id: "direct-assistant",
        role: "assistant",
        blocks: [
          {
            kind: "paragraph",
            tone: "lead",
            content: [
              code("carry out"),
              text(" 强调把安排好的事情执行完；"),
              code("conduct"),
              text(" 更正式，强调有方法地组织并开展一项活动。"),
            ],
          },
          {
            kind: "list",
            items: [
              [
                code("carry out"),
                text(" 常搭配 plan、task、test、duty，重点是执行落地。"),
              ],
              [
                code("conduct"),
                text(
                  " 常搭配 research、interview、survey、experiment，重点是主持并按流程进行。",
                ),
              ],
            ],
          },
          {
            kind: "example",
            english:
              "We carried out the plan and conducted three interviews.",
            translation: "我们执行了这项计划，并开展了三次访谈。",
          },
        ],
      },
    ],
  },
  long: {
    id: "long",
    title: "practical 与 pragmatic 的语气",
    messages: [
      {
        id: "long-user-1",
        role: "user",
        content:
          "我在写项目总结时想说“我们最后选择了一个更务实的方案”，但 practical solution 和 pragmatic solution 看起来都可以。我的重点不是说方案容易操作，而是团队考虑了时间、现有技术和比赛截止日期之后，放弃了理论上更漂亮但风险更高的做法。这里应该用哪个？",
        meta: "昨天 · 21:36",
      },
      {
        id: "long-assistant-1",
        role: "assistant",
        blocks: [
          {
            kind: "paragraph",
            tone: "lead",
            content: [
              text("这里更适合 "),
              code("pragmatic solution"),
              text("，因为你强调的是在真实约束下权衡之后作出的选择。"),
            ],
          },
          {
            kind: "paragraph",
            content: [
              code("practical"),
              text(" 关注方案能不能用、是否方便实施；"),
              code("pragmatic"),
              text(" 关注面对时间、资源和风险时怎样作出现实判断。"),
            ],
          },
          {
            kind: "example",
            english:
              "Given the deadline and the limits of our current stack, we chose a more pragmatic solution.",
            translation:
              "考虑到截止时间和现有技术栈的限制，我们选择了更务实的方案。",
          },
        ],
      },
      {
        id: "long-user-2",
        role: "user",
        content: "如果我想让语气再积极一点，不像是被迫妥协呢？",
        meta: "昨天 · 21:39",
      },
      {
        id: "long-assistant-2",
        role: "assistant",
        blocks: [
          {
            kind: "paragraph",
            content: [text("可以把“限制迫使我们放弃”改写成“判断帮助我们聚焦”：")],
          },
          {
            kind: "example",
            english:
              "We made a pragmatic choice that let us focus on the strongest part of the product while keeping delivery risk under control.",
            translation:
              "我们作出了务实选择，把精力集中在产品最有优势的部分，同时控制交付风险。",
          },
        ],
      },
    ],
  },
  instruction: {
    id: "instruction",
    title: "理解 instruction",
    messages: [
      {
        id: "instruction-user",
        role: "user",
        content: "理解 instruction",
        meta: "最近对话",
      },
      {
        id: "instruction-assistant",
        role: "assistant",
        blocks: [
          {
            kind: "paragraph",
            tone: "lead",
            content: [
              text(
                "instruction 在技术文档里通常指明确的操作要求，而不是一般性的指导。",
              ),
            ],
          },
        ],
      },
    ],
  },
  anchor: {
    id: "anchor",
    title: "技术英语中的 anchorRect",
    messages: [
      {
        id: "anchor-user",
        role: "user",
        content: "技术英语中的 anchorRect",
        meta: "最近对话",
      },
      {
        id: "anchor-assistant",
        role: "assistant",
        blocks: [
          {
            kind: "paragraph",
            tone: "lead",
            content: [
              text(
                "anchorRect 是用于确定浮层位置的锚点矩形；它描述位置关系，不是可见控件。",
              ),
            ],
          },
        ],
      },
    ],
  },
  concurrency: {
    id: "concurrency",
    title: "如何更自然地表达 concurrency",
    messages: [
      {
        id: "concurrency-user",
        role: "user",
        content: "如何更自然地表达 concurrency",
        meta: "最近对话",
      },
      {
        id: "concurrency-assistant",
        role: "assistant",
        blocks: [
          {
            kind: "paragraph",
            tone: "lead",
            content: [
              text(
                "谈系统能力时可以说 support concurrent tasks；谈同时发生的程度时，再使用 concurrency。",
              ),
            ],
          },
        ],
      },
    ],
  },
};

const aliases: Record<string, string> = {
  "context-circumstance": "memory",
  "carry-out": "direct",
  "anchor-rect": "anchor",
  diary: "long",
};

function cloneThread(thread: ConversationThread): ConversationThread {
  return structuredClone(thread);
}

function renderInlineMarkdown(
  content: Extract<ConversationAnswerBlock, { kind: "paragraph" }>["content"],
) {
  return content
    .map((item) => (item.kind === "code" ? `\`${item.text}\`` : item.text))
    .join("");
}

function renderAnswerBlock(block: ConversationAnswerBlock) {
  if (block.kind === "paragraph") {
    return renderInlineMarkdown(block.content);
  }
  if (block.kind === "list") {
    return block.items
      .map((item) => `- ${renderInlineMarkdown(item)}`)
      .join("\n");
  }
  return `> ${block.english}\n>\n> ${block.translation}`;
}

function renderThreadMarkdown(thread: ConversationThread) {
  const messages = thread.messages.map((message) => {
    if (message.role === "user") {
      return `## 你\n\n${message.content}`;
    }

    const answer = message.blocks.map(renderAnswerBlock).join("\n\n");
    const citation = message.citation
      ? `\n\n> 来自记忆：${message.citation.title}`
      : "";
    return `## ReadRay\n\n${answer}${citation}`;
  });

  return `# ${thread.title}\n\n${messages.join("\n\n---\n\n")}\n`;
}

function exportFileName(title: string) {
  const safeTitle = title
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "-")
    .replace(/[.\s]+$/g, "")
    .trim();
  return `${safeTitle || "ReadRay 对话"}.md`;
}

function resolveFixtureId(conversationId: string, title: string) {
  if (threads[conversationId]) {
    return conversationId;
  }
  if (aliases[conversationId]) {
    return aliases[conversationId];
  }

  const normalizedTitle = title.toLowerCase();
  if (normalizedTitle.includes("context") || normalizedTitle.includes("circumstance")) {
    return "memory";
  }
  if (normalizedTitle.includes("carry out") || normalizedTitle.includes("conduct")) {
    return "direct";
  }
  if (normalizedTitle.includes("practical") || normalizedTitle.includes("pragmatic")) {
    return "long";
  }
  if (normalizedTitle.includes("instruction")) {
    return "instruction";
  }
  if (normalizedTitle.includes("anchor")) {
    return "anchor";
  }
  if (normalizedTitle.includes("concurr")) {
    return "concurrency";
  }
  return null;
}

export type FixtureConversationFailureOperation =
  | "create"
  | "load"
  | "generate"
  | "export";

export type FixtureConversationServiceOptions = {
  failOnce?: FixtureConversationFailureOperation;
  failureCount?: number;
};

export class FixtureConversationService implements ConversationService {
  private nextConversationId = 1;
  private pendingFailure: FixtureConversationFailureOperation | null;
  private remainingFailures: number;
  private lastExportedThread: ConversationThread | null = null;

  constructor(options: FixtureConversationServiceOptions = {}) {
    this.pendingFailure = options.failOnce ?? null;
    this.remainingFailures = options.failOnce
      ? Math.max(1, options.failureCount ?? 1)
      : 0;
  }

  private failIfRequested(operation: FixtureConversationFailureOperation) {
    if (this.pendingFailure !== operation) {
      return;
    }

    this.remainingFailures -= 1;
    if (this.remainingFailures === 0) {
      this.pendingFailure = null;
    }
    throw new Error(`fixture ${operation} failure`);
  }

  async createConversation(): Promise<ConversationThread> {
    this.failIfRequested("create");
    const id = `new-${this.nextConversationId}`;
    this.nextConversationId += 1;

    return {
      id,
      title: "新对话",
      messages: [],
    };
  }

  async loadConversation(
    conversationId: string,
    title: string,
  ): Promise<ConversationThread> {
    this.failIfRequested("load");
    const fixtureId = resolveFixtureId(conversationId, title);
    if (fixtureId) {
      return {
        ...cloneThread(threads[fixtureId]),
        id: conversationId,
        title,
      };
    }

    return {
      id: conversationId,
      title,
      messages: [
        {
          id: `${conversationId}-user`,
          role: "user",
          content: title,
          meta: "最近对话",
        },
        {
          id: `${conversationId}-assistant`,
          role: "assistant",
          blocks: [
            {
              kind: "paragraph",
              tone: "lead",
              content: [
                text(
                  "这是前端对话页的演示记录。正式历史服务接入后，将在同一页面模型边界下替换。",
                ),
              ],
            },
          ],
        },
      ],
    };
  }

  async generateReply(
    request: ConversationGenerationRequest,
  ): Promise<{ assistantMessageId: string; chunks: string[] }> {
    this.failIfRequested("generate");

    const chunks =
      request.prompt === "[fixture:slow]"
        ? Array.from(
            { length: 40 },
            (_, index) => `这是用于停止与继续验证的第 ${index + 1} 个片段。`,
          )
        : [
            "先看你真正想表达的关系。",
            "如果重点是词句如何被理解，通常用 context；",
            "如果重点是时间、资源或环境怎样影响决定，通常用 circumstances。",
            "这也解释了为什么 in this context 和 under these circumstances 最自然。",
          ];

    return {
      assistantMessageId: `${request.conversationId}-assistant-${Date.now()}`,
      chunks,
    };
  }

  async exportConversation(
    thread: ConversationThread,
  ): Promise<ConversationExportResult> {
    this.failIfRequested("export");

    if (thread.messages.length === 0) {
      return { exported: false };
    }

    const exportedThread = cloneThread(thread);
    const content = renderThreadMarkdown(exportedThread);
    if (!content.trim()) {
      return { exported: false };
    }

    this.lastExportedThread = exportedThread;
    return {
      exported: true,
      file: {
        fileName: exportFileName(exportedThread.title),
        mimeType: "text/markdown;charset=utf-8",
        content,
      },
    };
  }

  getLastExportedThread() {
    return this.lastExportedThread
      ? cloneThread(this.lastExportedThread)
      : null;
  }
}
