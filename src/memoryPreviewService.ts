import { memoryPageFixture } from "./memoryPageFixture";
import type { MemoryRecordItem } from "./memoryViewModel";
import type {
  MemoryRecordPageModel,
  MemoryService,
} from "./memoryService";
import type { LearningRecordQuery } from "./memoryRepository";

function searchableText(record: MemoryRecordItem) {
  return [
    record.query,
    record.summary,
    record.meaning,
    record.sentence,
    record.translation,
    record.app,
  ]
    .join(" ")
    .toLocaleLowerCase("zh-CN");
}

class BrowserPreviewMemoryService implements MemoryService {
  async listRecords(query: LearningRecordQuery): Promise<MemoryRecordPageModel> {
    const keyword = query.keyword?.trim().toLocaleLowerCase("zh-CN");
    const matchingRecords = memoryPageFixture.records.filter((record) => {
      const matchesType = !query.queryType || record.type === query.queryType;
      const matchesKeyword = !keyword || searchableText(record).includes(keyword);
      return matchesType && matchesKeyword;
    });
    const start = (query.page - 1) * query.pageSize;

    return {
      records: matchingRecords.slice(start, start + query.pageSize),
      page: query.page,
      pageSize: query.pageSize,
      total: matchingRecords.length,
    };
  }

  async getRecord(id: string) {
    return memoryPageFixture.records.find((record) => record.id === id) ?? null;
  }
}

export function createBrowserPreviewMemoryService(): MemoryService {
  return new BrowserPreviewMemoryService();
}
