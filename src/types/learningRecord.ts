import type {
  ExplanationCard,
  QueryType,
  SourceType,
} from "./explanation";

export type LearningRecord = {
  id: number;
  learningTargetId: number;
  queryText: string;
  learningTargetText: string;
  queryDirection: "enToZh" | "zhToEn";
  normalizedText: string;
  queryType: QueryType;
  sourceType: SourceType;
  sourceApp?: string | null;
  contextText?: string | null;
  explanationCard: ExplanationCard;
  schemaVersion: number;
  createdAtUnixMs: number;
  difficulty?: string | null;
};

export type LearningTargetSummary = {
  id: number;
  stableKey: string;
  canonicalizationVersion: number;
  queryType: QueryType;
  learningTargetText: string;
  normalizedTargetText: string;
  queryCount: number;
  firstSeenAtUnixMs: number;
  lastSeenAtUnixMs: number;
  representativeRecord: LearningRecord;
};

export type LearningTargetPage = {
  targets: LearningTargetSummary[];
  page: number;
  pageSize: number;
  total: number;
};

export type LearningTargetDetail = {
  target: LearningTargetSummary;
  occurrences: LearningRecord[];
};

export type LearningRecordPage = {
  records: LearningRecord[];
  page: number;
  pageSize: number;
  total: number;
};
