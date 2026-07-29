export type MemoryRecordType = "word" | "phrase" | "sentence" | "paragraph";

export type MemoryFilterId = "all" | MemoryRecordType;

export type MemoryRecordGroup = "今天" | "昨天" | "更早";

export type MemoryFilterItem = {
  id: MemoryFilterId;
  label: string;
};

export type MemoryHistoryOccurrence = {
  time: string;
  app: string;
  context: string;
};

export type MemoryRecordItem = {
  id: string;
  group: MemoryRecordGroup;
  query: string;
  summary: string;
  app: string;
  time: string;
  type: MemoryRecordType;
  typeLabel: string;
  phonetic: string;
  part: string;
  definition: string;
  meaning: string;
  sentence: string;
  translation: string;
  sourceTime: string;
  history: MemoryHistoryOccurrence[];
};

export type MemoryPageViewModel = {
  heading: string;
  searchPlaceholder: string;
  groups: MemoryRecordGroup[];
  filters: MemoryFilterItem[];
};

export type MemoryPageFixture = MemoryPageViewModel & {
  totalCount: number;
  records: MemoryRecordItem[];
};

export const memoryPageViewModel: MemoryPageViewModel = {
  heading: "记忆",
  searchPlaceholder: "搜索查过的单词、短语或句子",
  groups: ["今天", "昨天", "更早"],
  filters: [
    { id: "all", label: "全部" },
    { id: "word", label: "单词" },
    { id: "phrase", label: "短语" },
    { id: "sentence", label: "句子" },
    { id: "paragraph", label: "段落" },
  ],
};
