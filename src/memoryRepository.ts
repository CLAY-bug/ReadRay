import { invoke } from "@tauri-apps/api/core";
import type { QueryType } from "./types/explanation";
import type {
  LearningTargetDetail,
  LearningTargetPage,
} from "./types/learningRecord";

export type LearningRecordQuery = {
  page: number;
  pageSize: number;
  keyword?: string;
  queryType?: QueryType;
};

export interface MemoryRepository {
  list(query: LearningRecordQuery): Promise<LearningTargetPage>;
  get(id: number): Promise<LearningTargetDetail | null>;
}

export class TauriMemoryRepository implements MemoryRepository {
  async list(query: LearningRecordQuery) {
    const keyword = query.keyword?.trim();
    const payload = {
      page: query.page,
      pageSize: query.pageSize,
      queryType: query.queryType,
    };

    if (keyword) {
      return invoke<LearningTargetPage>("search_learning_targets", {
        ...payload,
        keyword,
      });
    }

    return invoke<LearningTargetPage>("list_learning_targets", payload);
  }

  get(id: number) {
    return invoke<LearningTargetDetail | null>("get_learning_target", { id });
  }
}
