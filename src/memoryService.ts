import type {
  MemoryRecordGroup,
  MemoryRecordItem,
} from "./memoryViewModel";
import type {
  LearningRecordQuery,
  MemoryRepository,
} from "./memoryRepository";
import type { QueryType, SourceType } from "./types/explanation";
import type {
  LearningRecord,
  LearningTargetDetail,
  LearningTargetSummary,
} from "./types/learningRecord";
import { sourceSentenceForDisplay } from "./sourceSentenceDisplay.js";

export type MemoryRecordPageModel = {
  records: MemoryRecordItem[];
  page: number;
  pageSize: number;
  total: number;
};

export interface MemoryService {
  listRecords(query: LearningRecordQuery): Promise<MemoryRecordPageModel>;
  getRecord(id: string): Promise<MemoryRecordItem | null>;
}

const typeLabels: Record<QueryType, string> = {
  word: "单词",
  phrase: "短语",
  sentence: "句子",
  paragraph: "段落",
};

const sourceTypeLabels: Record<SourceType, string> = {
  manual: "ReadRay",
  clipboard: "剪贴板",
  windows_uia: "Windows UIA",
  app_adapter: "应用适配器",
  ocr: "OCR",
};

function sameLocalDate(left: Date, right: Date) {
  return (
    left.getFullYear() === right.getFullYear() &&
    left.getMonth() === right.getMonth() &&
    left.getDate() === right.getDate()
  );
}

function groupForDate(date: Date, now: Date): MemoryRecordGroup {
  if (sameLocalDate(date, now)) {
    return "今天";
  }

  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  if (sameLocalDate(date, yesterday)) {
    return "昨天";
  }

  return "更早";
}

function twoDigits(value: number) {
  return value.toString().padStart(2, "0");
}

function timeParts(createdAtUnixMs: number, now: Date) {
  const date = new Date(createdAtUnixMs);
  const group = groupForDate(date, now);
  const clock = `${twoDigits(date.getHours())}:${twoDigits(date.getMinutes())}`;
  const day = `${date.getMonth() + 1} 月 ${date.getDate()} 日`;
  const time = group === "更早" ? day : clock;
  const sourceTime =
    group === "更早"
      ? `${date.getFullYear()} 年 ${day} ${clock}`
      : `${group} ${clock}`;

  return { group, time, sourceTime };
}

function cleanText(value?: string | null) {
  return value?.trim() ?? "";
}

function cleanDisplayedSource(value?: string | null) {
  return cleanText(value)
    .replace(/^\s{0,3}#{1,6}[ \t]+/gm, "")
    .trim();
}

function comparableDisplayText(value?: string | null) {
  return cleanText(value).replace(/\s+/g, " ").toLocaleLowerCase();
}

function distinctDisplayText(value: string, comparedWith: string) {
  return comparableDisplayText(value) === comparableDisplayText(comparedWith) ? "" : value;
}

function sourceApp(record: LearningRecord) {
  const app = cleanText(record.sourceApp).replace(/\.exe$/i, "");
  return app || sourceTypeLabels[record.sourceType];
}

function contextualSource(record: LearningRecord, sourceText: string) {
  return (
    cleanDisplayedSource(record.contextText) ||
    cleanDisplayedSource(sourceText) ||
    cleanDisplayedSource(record.queryText)
  );
}

function queriedSource(record: LearningRecord, sourceText: string) {
  return cleanDisplayedSource(sourceText) || cleanDisplayedSource(record.queryText);
}

export function mapLearningRecordToMemoryItem(
  record: LearningRecord,
  now = new Date(),
): MemoryRecordItem {
  const { explanationCard: card } = record;
  const shared = {
    id: String(record.learningTargetId),
    representativeLearningRecordId: String(record.id),
    queryCount: 1,
    ...timeParts(record.createdAtUnixMs, now),
    query: cleanDisplayedSource(record.learningTargetText),
    app: sourceApp(record),
    type: record.queryType,
    typeLabel: typeLabels[record.queryType],
    history: [],
  } satisfies Pick<
    MemoryRecordItem,
    | "id"
    | "representativeLearningRecordId"
    | "queryCount"
    | "group"
    | "query"
    | "app"
    | "time"
    | "sourceTime"
    | "type"
    | "typeLabel"
    | "history"
  >;

  switch (card.queryType) {
    case "word": {
      const definition = card.basicMeanings.join("；");
      const sourceSentence = sourceSentenceForDisplay(
        card.sourceSentence,
        card.sourceSentenceZh,
      );
      return {
        ...shared,
        summary: cleanText(card.contextMeaning) || definition,
        phonetic: cleanText(card.phonetic),
        part: cleanText(card.partOfSpeech) || "单词",
        definition,
        meaning: cleanText(card.contextMeaning),
        sentence:
          cleanDisplayedSource(sourceSentence.sourceSentence) ||
          contextualSource(record, card.sourceText),
        translation: cleanText(sourceSentence.sourceSentenceZh),
      };
    }
    case "phrase": {
      const sourceSentence = sourceSentenceForDisplay(
        card.sourceSentence,
        card.sourceSentenceZh,
      );
      return {
        ...shared,
        summary: cleanText(card.contextMeaning) || card.basicMeaning,
        phonetic: "",
        part: "短语",
        definition: card.basicMeaning,
        meaning: cleanText(card.contextMeaning),
        sentence:
          cleanDisplayedSource(sourceSentence.sourceSentence) ||
          contextualSource(record, card.sourceText),
        translation: cleanText(sourceSentence.sourceSentenceZh),
      };
    }
    case "sentence":
      {
        const translation = cleanText(card.translation);
        const meaning = distinctDisplayText(cleanText(card.explanation), translation);
        return {
          ...shared,
          summary: translation,
          phonetic: "",
          part: "完整句",
          definition: translation,
          meaning,
          sentence: queriedSource(record, card.sourceText),
          translation,
        };
      }
    case "paragraph": {
      const translation = cleanText(card.translation);
      const meaning = distinctDisplayText(cleanText(card.summary), translation);
      return {
        ...shared,
        summary: meaning || translation,
        phonetic: "",
        part: "段落",
        definition: translation,
        meaning,
        sentence: queriedSource(record, card.sourceText),
        translation,
      };
    }
  }
}

export function mapLearningTargetToMemoryItem(
  target: LearningTargetSummary,
  now = new Date(),
): MemoryRecordItem {
  return {
    ...mapLearningRecordToMemoryItem(target.representativeRecord, now),
    id: String(target.id),
    representativeLearningRecordId: String(target.representativeRecord.id),
    query: cleanDisplayedSource(target.learningTargetText),
    queryCount: target.queryCount,
  };
}

function mapOccurrenceToHistory(record: LearningRecord, now: Date) {
  const card = record.explanationCard;
  return {
    learningRecordId: String(record.id),
    time: timeParts(record.createdAtUnixMs, now).sourceTime,
    app: sourceApp(record),
    query: cleanDisplayedSource(record.queryText),
    context: contextualSource(record, card.sourceText),
  };
}

function mapLearningTargetDetailToMemoryItem(
  detail: LearningTargetDetail,
  now = new Date(),
) {
  return {
    ...mapLearningTargetToMemoryItem(detail.target, now),
    history: detail.occurrences.map((record) => mapOccurrenceToHistory(record, now)),
  };
}

export class RepositoryMemoryService implements MemoryService {
  private readonly repository: MemoryRepository;

  constructor(repository: MemoryRepository) {
    this.repository = repository;
  }

  async listRecords(query: LearningRecordQuery) {
    const result = await this.repository.list(query);
    const now = new Date();
    return {
      records: result.targets.map((target) =>
        mapLearningTargetToMemoryItem(target, now),
      ),
      page: result.page,
      pageSize: result.pageSize,
      total: result.total,
    };
  }

  async getRecord(id: string) {
    const numericId = Number(id);
    if (!Number.isSafeInteger(numericId)) {
      throw new Error("学习目标 ID 无效。");
    }

    const target = await this.repository.get(numericId);
    return target ? mapLearningTargetDetailToMemoryItem(target) : null;
  }
}
