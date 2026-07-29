import { invoke } from "@tauri-apps/api/core";
import type { QueryType } from "./types/explanation";
import type {
  LearningRecord,
  LearningRecordPage,
} from "./types/learningRecord";

export type LearningRecordQuery = {
  page: number;
  pageSize: number;
  keyword?: string;
  queryType?: QueryType;
};

export interface MemoryRepository {
  list(query: LearningRecordQuery): Promise<LearningRecordPage>;
  get(id: number): Promise<LearningRecord | null>;
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
      return invoke<LearningRecordPage>("search_learning_records", {
        ...payload,
        keyword,
      });
    }

    return invoke<LearningRecordPage>("list_learning_records", payload);
  }

  get(id: number) {
    return invoke<LearningRecord | null>("get_learning_record", { id });
  }
}
