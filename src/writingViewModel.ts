export type WritingMode = "draft" | "review" | "compare" | "completed" | "library";

export type WritingDocumentStatus = "draft" | "completed";

export type WritingSnapshot = {
  title: string;
  paragraphs: string[];
};

export type WritingVersion = {
  id: string;
  completedAt: string;
  snapshot: WritingSnapshot;
  comparisonBaseline: WritingSnapshot;
};

export type WritingDocumentRecord = {
  id: string;
  createdAt: string;
  updatedAt: string;
  lastOpenedAt?: string;
  draftUpdatedAt?: string;
  completedAt?: string;
  draftSnapshot?: WritingSnapshot;
  completedSnapshot?: WritingSnapshot;
  comparisonBaseline: WritingSnapshot;
  versions: WritingVersion[];
};

export type WritingIssueId = "reference" | "verb" | "rhythm" | "wording";

export type WritingIssue = {
  id: WritingIssueId;
  category: string;
  source: string;
  targetText: string;
  explanation: string;
  hint: string;
  deeperHint: string;
  reference: string;
};

export type WritingPattern = {
  id: string;
  title: string;
  description: string;
};

export type AgentAnswer = {
  question: string;
  scopeLabel: string;
  title: string;
  copy: string;
  map?: {
    core: string;
    questions: string[];
    phrases: string[];
    starters: string[];
  };
};

export const quietTechnologySnapshot: WritingSnapshot = {
  title: "When Technology Learns to Stay Quiet",
  paragraphs: [
    "Digital tools often promise to make us more productive, but many of them ask for our attention before they earn it. A notification arrives, a panel opens, and the work we were doing becomes secondary.",
    "Last semester, I tried several writing applications because I wanted to express my ideas more clearly in English. Most of them corrected every sentence immediately. This was useful at first, but it also made me to wait for the machine's opinion before trusting my own.",
    "I now prefer tools that remain quiet while I am drafting. They can notice patterns in my writing, but they should explain a problem only when I ask. For example, I often use very long sentences when a shorter one would make the main point easier to follow.",
    "A helpful writing assistant should not replace my voice. It should help me see where my reasoning becomes unclear, why an expression sounds unnatural, and what I can try next. In this way, revision becomes a form of learning instead of a final cleaning step.",
    "Good assistance does not remove effort. It helps the writer spend effort on the decisions that matter.",
  ],
};

const completedReadingSnapshot: WritingSnapshot = {
  title: "Learning to Read More Slowly",
  paragraphs: [
    "I used to measure reading by the number of pages I finished. That habit made difficult passages feel like delays.",
    "Now I stop when a sentence changes how I understand the argument. Reading less can sometimes mean noticing more.",
  ],
};

export const writingDocumentFixtures: WritingDocumentRecord[] = [
  {
    id: "quiet-technology",
    createdAt: "2026-07-18T14:12:00+08:00",
    updatedAt: "2026-07-18T23:04:00+08:00",
    draftUpdatedAt: "2026-07-18T23:04:00+08:00",
    draftSnapshot: quietTechnologySnapshot,
    comparisonBaseline: quietTechnologySnapshot,
    versions: [],
  },
  {
    id: "slow-reading",
    createdAt: "2026-06-28T19:20:00+08:00",
    updatedAt: "2026-07-02T21:18:00+08:00",
    completedAt: "2026-07-02T21:18:00+08:00",
    completedSnapshot: completedReadingSnapshot,
    comparisonBaseline: completedReadingSnapshot,
    versions: [
      {
        id: "slow-reading-v1",
        completedAt: "2026-07-02T21:18:00+08:00",
        snapshot: completedReadingSnapshot,
        comparisonBaseline: completedReadingSnapshot,
      },
    ],
  },
];

export const writingIssues: WritingIssue[] = [
  {
    id: "reference",
    category: "指代清晰度",
    source: "This was useful at first",
    targetText: "This was useful at first",
    explanation: "读者需要回看上一句，才能确定 This 指的是“每句即时纠正”，还是“尝试这些应用”。",
    hint: "提示：把真正有用的事物直接说出来。",
    deeperHint: "再想一步：先圈出上一句中你真正想指代的名词，再让这一句直接从它开始。",
    reference: "At first, this immediate feedback felt useful.",
  },
  {
    id: "verb",
    category: "动词结构",
    source: "made me to wait",
    targetText: "made me to wait",
    explanation: "make + 人 后面的动作通常直接使用动词原形。",
    hint: "提示：去掉动作前多余的连接词。",
    deeperHint: "再想一步：保留 made me，然后直接接你真正执行的动作。",
    reference: "It made me wait for the machine's opinion.",
  },
  {
    id: "rhythm",
    category: "句子重心",
    source: "It should help me see where … why … and what …",
    targetText: "A helpful writing assistant should not replace my voice. It should help me see where my reasoning becomes unclear, why an expression sounds unnatural, and what I can try next.",
    explanation: "三个并列从句都在争夺重点，读者不容易抓住你最想强调的学习价值。",
    hint: "提示：先保留一个核心发现，再把行动放到下一句。",
    deeperHint: "再想一步：第一句只回答“它帮我看见什么”，第二句再回答“我接下来做什么”。",
    reference: "It should help me see where my reasoning becomes unclear. Then I can decide what to try next.",
  },
  {
    id: "wording",
    category: "自然表达",
    source: "final cleaning step",
    targetText: "final cleaning step",
    explanation: "这里的“清理”像是从中文直译，英语读者不一定会自然联想到写作修订。",
    hint: "提示：先判断你要表达的是“收尾整理”还是“语言润色”。",
    deeperHint: "再想一步：选一个英语写作者会用来描述“最后修订动作”的名词，而不是逐字翻译“清理”。",
    reference: "Revision becomes learning rather than a final cleanup step.",
  },
];

export const writingPatterns: WritingPattern[] = [
  {
    id: "01",
    title: "make + 人 + 动词原形",
    description: "使役动词后直接接动作；以后遇到 make me to…，先检查这个结构。",
  },
  {
    id: "02",
    title: "让指代和句子重心可见",
    description: "用具体名词替代模糊的 this；一句承担一个主要发现，行动可以另起一句。",
  },
];

export function normalizeWritingText(value: string) {
  return value.replace(/\s+/g, " ").trim();
}

export function cloneWritingSnapshot(snapshot: WritingSnapshot): WritingSnapshot {
  return {
    title: snapshot.title,
    paragraphs: [...snapshot.paragraphs],
  };
}

export function writingSnapshotsEqual(first: WritingSnapshot, second: WritingSnapshot) {
  return first.title === second.title
    && first.paragraphs.length === second.paragraphs.length
    && first.paragraphs.every((paragraph, index) => paragraph === second.paragraphs[index]);
}

export function countWritingWords(snapshot: WritingSnapshot) {
  return (snapshot.paragraphs.join(" ").match(/[A-Za-z]+(?:['’-][A-Za-z]+)*/g) ?? []).length;
}

export function getRecordStatus(record: WritingDocumentRecord): WritingDocumentStatus {
  return record.draftSnapshot ? "draft" : "completed";
}

export function getRecordSnapshot(record: WritingDocumentRecord): WritingSnapshot {
  return cloneWritingSnapshot(record.draftSnapshot ?? record.completedSnapshot ?? { title: "", paragraphs: [""] });
}

export function answerWritingQuestion(
  question: string,
  scopeLabel: string,
  selectionText = "",
): AgentAnswer {
  const normalized = normalizeWritingText(question);
  const wantsMap = /梳理|思路|开头|下一步|继续|卡住/.test(normalized);
  let title = "先处理最小的障碍";
  let copy = "先说清你此刻最难确定的是意思、结构还是语气。一次只解决一个问题，原来的观点和句子骨架先保留。";

  if (selectionText && /解释/.test(normalized)) {
    title = "先看它在句子里承担什么";
    copy = `“${selectionText}”需要先确认指向和句子作用，再判断是否要改。不要先追求更高级的词。`;
  } else if (/提示/.test(normalized)) {
    title = "只给你一个下一步";
    copy = "保留原意，先处理一个障碍：把指代说具体，或把过长的判断拆成两步。";
  } else if (/比较/.test(normalized)) {
    title = "比较语气与重心";
    copy = "先判断你想强调的是事实、感受还是变化。更合适的表达会让这个重点更早出现，而不是显得更复杂。";
  } else if (/逐渐意识到|词组|怎么说|表达/.test(normalized)) {
    title = "按变化速度选择表达";
    copy = "gradually realize 强调逐步意识到；come to realize 更自然地表示后来才明白；it dawned on me 强调某一刻突然领悟。先判断变化是渐进还是瞬间，再选词组。";
  } else if (/虚拟语气/.test(normalized)) {
    title = "先确认你在谈哪个时间";
    copy = "与现在相反：If + 过去式，would + 动词原形；与过去相反：If + had + 过去分词，would have + 过去分词。先写清时间，再补主句。";
  } else if (/语法|grammar|时态|介词/i.test(normalized)) {
    title = "先确认句子骨架";
    copy = "先找主语、核心动词和时间，再检查介词或时态。只修正影响理解的结构点，不必同时改写整句。";
  } else if (wantsMap) {
    title = scopeLabel === "整篇文章" ? "把文章变成几个可写的决定" : "先确定这一段只完成什么";
    copy = "这不是替你写的提纲，而是一张把下一步变清楚的写作地图。你可以继续调整角度、难度和表达素材。";
  }

  return {
    question: normalized,
    scopeLabel,
    title,
    copy,
    map: wantsMap ? {
      core: scopeLabel === "整篇文章"
        ? `你想写的是“${normalized.slice(0, 86)}”。先抓住最具体的经历，再说明它怎样改变了你的想法。`
        : `这一段想说的是“${normalized.slice(0, 86)}”。先写清一个中心判断，再只补一个最能支撑它的事实。`,
      questions: scopeLabel === "整篇文章"
        ? ["最具体的一件事是什么？", "它为什么改变了你的想法？", "你希望读者最后理解什么？"]
        : ["这一段最重要的一句话是什么？", "需要哪一个事实或例子支撑？", "它怎样连接上一段或下一段？"],
      phrases: ["I used to think", "what changed was", "at first", "because of this", "I began to realize", "from my perspective"],
      starters: ["I used to think ___, but later I realized ___.", "One experience that changed my view was ___."],
    } : undefined,
  };
}
