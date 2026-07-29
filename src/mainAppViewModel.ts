export type MainAppNavigationId =
  | "today"
  | "review"
  | "writing"
  | "memory"
  | "settings";

export type TodayActionId = "todayRecords" | "writing" | "recentRecord";

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
  recordId?: string;
  disabled?: boolean;
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
};

export const mainAppViewModel: MainAppViewModel = {
  navigation: [
    { id: "today", label: "今天", icon: "today" },
    { id: "review", label: "复习", icon: "review" },
    { id: "writing", label: "写作", icon: "write" },
    { id: "memory", label: "记忆", icon: "memory" },
  ],
};
