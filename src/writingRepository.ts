import {
  cloneWritingSnapshot,
  writingDocumentFixtures,
  type WritingDocumentRecord,
} from "./writingViewModel";

export type WritingRepository = {
  list(): WritingDocumentRecord[];
  save(record: WritingDocumentRecord): WritingDocumentRecord[];
};

const DEMO_STORAGE_KEY = "readray:writing-demo:v1";

function cloneRecord(record: WritingDocumentRecord): WritingDocumentRecord {
  return {
    ...record,
    draftSnapshot: record.draftSnapshot ? cloneWritingSnapshot(record.draftSnapshot) : undefined,
    completedSnapshot: record.completedSnapshot ? cloneWritingSnapshot(record.completedSnapshot) : undefined,
    comparisonBaseline: cloneWritingSnapshot(record.comparisonBaseline),
    versions: record.versions.map((version) => ({
      ...version,
      snapshot: cloneWritingSnapshot(version.snapshot),
      comparisonBaseline: cloneWritingSnapshot(version.comparisonBaseline),
    })),
  };
}

function cloneRecords(records: WritingDocumentRecord[]) {
  return records.map(cloneRecord);
}

/**
 * 仅用于本轮前端演示的可替换 repository。它不是 ReadRay 的正式 SQLite 方案；
 * 页面组件只依赖 WritingRepository，后续可在装配层替换为 Tauri commands。
 */
class BrowserWritingDemoRepository implements WritingRepository {
  private fallback = cloneRecords(writingDocumentFixtures);

  list() {
    try {
      const raw = window.localStorage.getItem(DEMO_STORAGE_KEY);
      if (!raw) {
        window.localStorage.setItem(DEMO_STORAGE_KEY, JSON.stringify(this.fallback));
        return cloneRecords(this.fallback);
      }
      const stored = JSON.parse(raw) as WritingDocumentRecord[];
      if (!Array.isArray(stored)) {
        return cloneRecords(this.fallback);
      }
      this.fallback = cloneRecords(stored);
      return cloneRecords(stored);
    } catch {
      return cloneRecords(this.fallback);
    }
  }

  save(record: WritingDocumentRecord) {
    const records = this.list();
    const index = records.findIndex((candidate) => candidate.id === record.id);
    if (index >= 0) {
      records[index] = cloneRecord(record);
    } else {
      records.unshift(cloneRecord(record));
    }
    this.fallback = cloneRecords(records);
    try {
      window.localStorage.setItem(DEMO_STORAGE_KEY, JSON.stringify(records));
    } catch {
      // localStorage 不可用时保留当前会话内存副本。
    }
    return cloneRecords(records);
  }
}

export const writingDemoRepository: WritingRepository = new BrowserWritingDemoRepository();
