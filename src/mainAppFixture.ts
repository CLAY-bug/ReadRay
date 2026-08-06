import type {
  RecentConversationItem,
  TodayPageViewModel,
} from "./mainAppViewModel";

export const previewRecentConversations: RecentConversationItem[] = [
  { id: "instruction", title: "理解 instruction" },
  { id: "context-circumstance", title: "context 与 circumstance 的区别" },
  { id: "diary", title: "改进昨天的英语日记" },
  { id: "anchor-rect", title: "技术英语中的 anchorRect" },
  { id: "carry-out", title: "carry out 的常见用法" },
  { id: "concurrency", title: "如何更自然地表达 concurrency" },
];

export const previewTodayPage: TodayPageViewModel = {
  heading: "今天",
  dateLabel: "2026 年 7 月 19 日 · 星期日",
  dateTime: "2026-07-19",
  summary: "今天已经保存了 3 条学习记录。最近一次查询是“instruction”。",
  localContext: "浏览器预览使用演示数据；正式 Tauri 路径读取本机 SQLite。",
  actions: [
    {
      id: "todayRecords",
      title: "查看今天的学习记录",
      description: "3 条记录 · 最近更新于 09:42",
      icon: "clock",
      recordId: "instruction",
    },
    {
      id: "writing",
      title: "打开写作文章库",
      description: "继续已有文章或开始新草稿",
      icon: "book",
    },
    {
      id: "recentRecord",
      title: "查看最近查询的 instruction",
      description: "单词 · Obsidian · 今天 09:42",
      icon: "chat",
      recordId: "instruction",
    },
  ],
  composerPlaceholder: "今天想和 ReadRay 讨论什么？",
};
