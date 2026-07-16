export type MainAppNavigationId =
  | "today"
  | "review"
  | "writing"
  | "memory"
  | "settings";

export type TodayActionId = "review" | "writing" | "instruction";

export type MainAppNavigationItem = {
  id: Exclude<MainAppNavigationId, "settings">;
  label: string;
  icon: "today" | "review" | "write" | "memory";
};

export type RecentConversationItem = {
  id: string;
  title: string;
};

export type TodayActionItem = {
  id: TodayActionId;
  title: string;
  description: string;
  icon: "clock" | "book" | "chat";
};

export type TodayPageViewModel = {
  heading: string;
  dateLabel: string;
  dateTime: string;
  summary: string;
  localContext: string;
  actions: TodayActionItem[];
  composerPlaceholder: string;
};

export type MainAppViewModel = {
  navigation: MainAppNavigationItem[];
  recentConversations: RecentConversationItem[];
  today: TodayPageViewModel;
};

export const mainAppFixture: MainAppViewModel = {
  navigation: [
    { id: "today", label: "今天", icon: "today" },
    { id: "review", label: "复习", icon: "review" },
    { id: "writing", label: "写作", icon: "write" },
    { id: "memory", label: "记忆", icon: "memory" },
  ],
  recentConversations: [
    { id: "instruction", title: "理解 instruction" },
    { id: "context-circumstance", title: "context 与 circumstance 的区别" },
    { id: "diary", title: "改进昨天的英语日记" },
    { id: "anchor-rect", title: "技术英语中的 anchorRect" },
    { id: "carry-out", title: "carry out 的常见用法" },
    { id: "concurrency", title: "如何更自然地表达 concurrency" },
  ],
  today: {
    heading: "今天",
    dateLabel: "2026 年 7 月 15 日 · 星期三",
    dateTime: "2026-07-15",
    summary:
      "今天有 12 条待复习内容，其中 4 条来自昨天的阅读。你还留下一篇未完成的英语日记，最近多次查询了 instruction。",
    localContext: "ReadRay 已根据本地学习记录，为你整理出最值得继续的三件事。",
    actions: [
      {
        id: "review",
        title: "开始 5 分钟复习",
        description: "12 条内容 · 优先处理昨天阅读中反复出现的词",
        icon: "clock",
      },
      {
        id: "writing",
        title: "继续昨天的英语日记",
        description: "A quiet afternoon · 上次写到图书馆里的阅读",
        icon: "book",
      },
      {
        id: "instruction",
        title: "讨论最近查询的 instruction",
        description: "结合昨天 09:42 的查询与技术文档语境继续理解",
        icon: "chat",
      },
    ],
    composerPlaceholder: "和 ReadRay 讨论今天想学什么……",
  },
};
