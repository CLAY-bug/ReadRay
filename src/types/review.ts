import type { LearningRecord } from "./learningRecord";

export type ReviewReasonCode =
  | "scheduledToday"
  | "newRecord"
  | "continuedPractice";

export type ReviewOutcome = "remembered" | "forgotten";

export type ReviewQualityPolarity = "up" | "down";

export type ReviewTarget = {
  learningTargetId: number;
  revision: number;
  nextReviewAtUnixMs: number;
  attemptCount: number;
  rememberedCount: number;
  forgottenCount: number;
  successStreak: number;
  lastReviewedAtUnixMs?: number | null;
  lastOutcome?: ReviewOutcome | null;
  lastUsedHint?: boolean | null;
  lastAttemptId?: number | null;
};

export type ReviewAttempt = {
  id: number;
  feedItemId: number;
  learningRecordId: number;
  learningTargetId: number;
  requestKey: string;
  expectedRevision: number;
  targetRevision: number;
  outcome: ReviewOutcome;
  usedHint: boolean;
  nextReviewAtUnixMs: number;
  createdAtUnixMs: number;
  undoneAtUnixMs?: number | null;
  undoRequestKey?: string | null;
  undoTargetRevision?: number | null;
};

export type ReviewQualityFeedback = {
  id: number;
  feedItemId: number;
  learningRecordId: number;
  generatedCardId?: number | null;
  revision: number;
  active: boolean;
  polarity: ReviewQualityPolarity;
  reasonCodes: string[];
  detail?: string | null;
  createdAtUnixMs: number;
  updatedAtUnixMs: number;
};

export type GeneratedReviewCard = {
  id: number;
  learningRecordId: number;
  learningTargetId: number;
  variantIndex: number;
  englishContext: string;
  englishContextZh: string;
  hint: string;
  model: string;
  createdAtUnixMs: number;
  expiresAtUnixMs: number;
  lastUsedAtUnixMs: number;
  useCount: number;
};

export type ReviewCardGenerationFailure = {
  requestKey: string;
  feedItemId: number;
  learningRecordId: number;
  failureCount: number;
  retryAfterUnixMs: number;
  lastError: string;
  createdAtUnixMs: number;
  updatedAtUnixMs: number;
};

export type ReviewFeedItem = {
  id: number;
  ordinal: number;
  cycleIndex: number;
  reasonCode: ReviewReasonCode;
  learningRecord: LearningRecord;
  target: ReviewTarget;
  attempt?: ReviewAttempt | null;
  qualityFeedback?: ReviewQualityFeedback | null;
  generatedCard?: GeneratedReviewCard | null;
  generationFailure?: ReviewCardGenerationFailure | null;
};

export type ReviewFeedPage = {
  dayStartUnixMs: number;
  dayEndUnixMs: number;
  pageSize: number;
  items: ReviewFeedItem[];
  nextCursor?: number | null;
  canContinue: boolean;
  completedCount: number;
  rememberedCount: number;
  forgottenCount: number;
};

export type PrepareReviewFeedCardInput = {
  feedItemId: number;
  learningRecordId: number;
  learningTargetId: number;
  requestKey: string;
  explicitRetry?: boolean;
};

export type SubmitReviewOutcomeInput = {
  feedItemId: number;
  learningRecordId: number;
  learningTargetId: number;
  expectedRevision: number;
  outcome: ReviewOutcome;
  usedHint: boolean;
  requestKey: string;
};

export type UndoReviewOutcomeInput = {
  attemptId: number;
  feedItemId: number;
  learningRecordId: number;
  learningTargetId: number;
  expectedRevision: number;
  requestKey: string;
};

export type ReviewOutcomeWriteResult = {
  target: ReviewTarget;
  attempt: ReviewAttempt;
  canContinue: boolean;
};

export type ReviewFeedItemState = {
  dayStartUnixMs: number;
  dayEndUnixMs: number;
  item: ReviewFeedItem;
  completedCount: number;
  rememberedCount: number;
  forgottenCount: number;
  canContinue: boolean;
};

export type SaveReviewQualityFeedbackInput = {
  feedItemId: number;
  learningRecordId: number;
  cardContextKey: string;
  expectedRevision?: number | null;
  polarity: ReviewQualityPolarity;
  reasonCodes: string[];
  detail?: string | null;
  requestKey: string;
};

export type UndoReviewQualityFeedbackInput = {
  feedbackId: number;
  feedItemId: number;
  learningRecordId: number;
  expectedRevision: number;
  requestKey: string;
};
