import {
  previewRecentConversations,
  previewTodayPage,
} from "./mainAppFixture";
import type { TodayService } from "./todayService";

export function createBrowserPreviewTodayService(): TodayService {
  return {
    async loadToday(now = new Date()) {
      const dateLabel = new Intl.DateTimeFormat("zh-CN", {
        year: "numeric",
        month: "long",
        day: "numeric",
        weekday: "long",
      }).format(now);
      return {
        ...previewTodayPage,
        dateLabel,
        dateTime: `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`,
      };
    },
    async listRecentConversations(limit = 6) {
      return previewRecentConversations.slice(0, limit);
    },
  };
}
