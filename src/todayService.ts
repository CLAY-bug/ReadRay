import type {
  RecentConversationItem,
  TodayActionItem,
  TodayPageViewModel,
} from "./mainAppViewModel";
import { mapLearningRecordToMemoryItem } from "./memoryService.ts";
import type { TodayRepository } from "./todayRepository";

export interface TodayService {
  loadToday(now?: Date): Promise<TodayPageViewModel>;
  listRecentConversations(limit?: number): Promise<RecentConversationItem[]>;
}

const weekdays = [
  "星期日",
  "星期一",
  "星期二",
  "星期三",
  "星期四",
  "星期五",
  "星期六",
];

function twoDigits(value: number) {
  return value.toString().padStart(2, "0");
}

function dateFields(now: Date) {
  const year = now.getFullYear();
  const month = now.getMonth() + 1;
  const day = now.getDate();
  return {
    dateLabel: `${year} 年 ${month} 月 ${day} 日 · ${weekdays[now.getDay()]}`,
    dateTime: `${year}-${twoDigits(month)}-${twoDigits(day)}`,
  };
}

function truncate(value: string, maxLength: number) {
  const normalized = value.trim();
  return normalized.length > maxLength
    ? `${normalized.slice(0, maxLength)}…`
    : normalized;
}

export function createTodayLoadingViewModel(now = new Date()): TodayPageViewModel {
  return {
    heading: "今天",
    ...dateFields(now),
    summary: "正在读取今天的本地学习记录。",
    localContext: "数据只从本机 ReadRay 存储中读取。",
    actions: [],
    composerPlaceholder: "今天想和 ReadRay 讨论什么？",
  };
}

export class RepositoryTodayService implements TodayService {
  private readonly repository: TodayRepository;

  constructor(repository: TodayRepository) {
    this.repository = repository;
  }

  async loadToday(now = new Date()): Promise<TodayPageViewModel> {
    const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const end = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
    const summary = await this.repository.getLearningSummary(
      start.getTime(),
      end.getTime(),
    );
    const latest = summary.latestRecord
      ? mapLearningRecordToMemoryItem(summary.latestRecord, now)
      : null;
    const actions: TodayActionItem[] = [
      {
        id: "todayRecords",
        title: "查看今天的学习记录",
        description: latest
          ? `${summary.recordCount} 条记录 · 最近更新于 ${latest.time}`
          : "今天还没有学习记录",
        icon: "clock",
        recordId: latest?.id,
        disabled: !latest,
      },
      {
        id: "writing",
        title: "打开写作文章库",
        description: "继续已有文章或开始新草稿",
        icon: "book",
      },
      {
        id: "recentRecord",
        title: latest
          ? `查看最近查询的 ${truncate(latest.query, 32)}`
          : "最近查询",
        description: latest
          ? `${latest.typeLabel} · ${latest.app} · ${latest.sourceTime}`
          : "今天还没有可查看的查询记录",
        icon: "chat",
        recordId: latest?.id,
        disabled: !latest,
      },
    ];

    return {
      heading: "今天",
      ...dateFields(now),
      summary: latest
        ? `今天已经保存了 ${summary.recordCount} 条学习记录。最近一次查询是“${truncate(latest.query, 48)}”，类型为${latest.typeLabel}，来源是 ${latest.app}，时间为 ${latest.time}。`
        : "今天还没有保存学习记录。",
      localContext: latest
        ? "以上内容来自本机今天的学习记录，不包含复习数量、趋势或高频推断。"
        : "通过 overlay 完成一次解释查询后，记录会自动出现在这里。",
      actions,
      composerPlaceholder: "今天想和 ReadRay 讨论什么？",
    };
  }

  async listRecentConversations(limit = 6) {
    const conversations = await this.repository.listRecentConversations(limit);
    return conversations.map((conversation) => ({
      id: String(conversation.id),
      title: conversation.title.trim(),
    }));
  }
}
