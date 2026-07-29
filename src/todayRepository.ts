import { invoke } from "@tauri-apps/api/core";
import type { LearningRecord } from "./types/learningRecord";
import type { RecentQuickAiConversation } from "./types/quickAi";

export type TodayLearningSummary = {
  recordCount: number;
  latestRecord?: LearningRecord | null;
};

export interface TodayRepository {
  getLearningSummary(
    startUnixMs: number,
    endUnixMs: number,
  ): Promise<TodayLearningSummary>;
  listRecentConversations(limit: number): Promise<RecentQuickAiConversation[]>;
}

export class TauriTodayRepository implements TodayRepository {
  getLearningSummary(startUnixMs: number, endUnixMs: number) {
    return invoke<TodayLearningSummary>("get_today_learning_summary", {
      startUnixMs,
      endUnixMs,
    });
  }

  listRecentConversations(limit: number) {
    return invoke<RecentQuickAiConversation[]>(
      "list_recent_quick_ai_conversations",
      { limit },
    );
  }
}
