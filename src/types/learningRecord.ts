import type {
  ExplanationCard,
  QueryType,
  SourceType,
} from "./explanation";

export type LearningRecord = {
  id: number;
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

export type LearningRecordPage = {
  records: LearningRecord[];
  page: number;
  pageSize: number;
  total: number;
};
