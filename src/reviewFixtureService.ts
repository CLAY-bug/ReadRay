import {
  reviewCardPresentation,
  type ReviewCardModel,
  type ReviewFeedModel,
  type ReviewService,
} from "./reviewService";
import type { LearningRecord } from "./types/learningRecord";
import type {
  GeneratedReviewCard,
  ReviewAttempt,
  ReviewOutcomeWriteResult,
  ReviewQualityFeedback,
  ReviewTarget,
  SaveReviewQualityFeedbackInput,
  SubmitReviewOutcomeInput,
  UndoReviewOutcomeInput,
  UndoReviewQualityFeedbackInput,
} from "./types/review";

const PAGE_SIZE = 6;
const DAY_MS = 24 * 60 * 60 * 1_000;
const now = new Date();
const dayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
const dayEnd = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime();

type FixtureSeed = {
  record: LearningRecord;
  answerTitle: string;
  hint: string;
};

const seeds: FixtureSeed[] = [
  {
    record: learningRecord(
      101,
      "robust",
      "The retry path must remain robust when an older response arrives late.",
      "稳健的；能经受错误、变化或意外情况",
      "VS Code",
    ),
    answerTitle: "稳健的；能经受错误、变化或意外情况",
    hint: "它强调系统在变化下仍然可靠。",
  },
  {
    record: learningRecord(
      102,
      "come in handy",
      "A portable charger can really come in handy during a long train journey.",
      "在需要时派上用场",
      "Microsoft Edge",
    ),
    answerTitle: "在需要时派上用场",
    hint: "意思接近“在需要时派上用场”。",
  },
  {
    record: learningRecord(
      103,
      "account for",
      "The coordinator must account for requests that finish out of order.",
      "把必须覆盖的情况考虑在内",
      "Obsidian",
    ),
    answerTitle: "把必须覆盖的情况考虑在内",
    hint: "想想如何表达“把某种情况算进去”。",
  },
];

function learningRecord(
  id: number,
  queryText: string,
  sourceSentence: string,
  meaning: string,
  sourceApp: string,
): LearningRecord {
  return {
    id,
    queryText,
    learningTargetText: queryText,
    queryDirection: "enToZh",
    normalizedText: queryText.toLowerCase(),
    queryType: queryText.includes(" ") ? "phrase" : "word",
    sourceType: "windows_uia",
    sourceApp,
    contextText: sourceSentence,
    explanationCard: queryText.includes(" ")
      ? {
          queryType: "phrase",
          sourceText: sourceSentence,
          learningTargetText: queryText,
          basicMeaning: meaning,
          contextMeaning: meaning,
          sourceSentence,
          examples: [],
          reviewHint: "根据句子回忆这个表达。",
        }
      : {
          queryType: "word",
          sourceText: sourceSentence,
          learningTargetText: queryText,
          headword: queryText,
          basicMeanings: [meaning],
          contextMeaning: meaning,
          sourceSentence,
          phrases: [],
          nearMeanings: [],
          examples: [],
          reviewHint: "根据句子回忆这个词。",
        },
    schemaVersion: 1,
    createdAtUnixMs: Date.now() - id * 1_000,
  };
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function splitContext(context: string, query: string) {
  const index = context.toLowerCase().indexOf(query.toLowerCase());
  return {
    before: context.slice(0, index),
    answer: context.slice(index, index + query.length),
    after: context.slice(index + query.length),
  };
}

class BrowserPreviewReviewService implements ReviewService {
  private nextAttemptId = 1;
  private nextFeedbackId = 1;
  private nextGeneratedId = 1;
  private readonly targets = new Map<number, ReviewTarget>();
  private readonly attempts = new Map<number, ReviewAttempt>();
  private readonly feedback = new Map<number, ReviewQualityFeedback>();
  private readonly generated = new Map<string, GeneratedReviewCard>();
  private readonly outcomeResults = new Map<string, ReviewOutcomeWriteResult>();
  private readonly qualityResults = new Map<string, ReviewQualityFeedback>();

  constructor() {
    for (const seed of seeds) {
      this.targets.set(seed.record.id, {
        learningRecordId: seed.record.id,
        revision: 0,
        nextReviewAtUnixMs: seed.record.createdAtUnixMs,
        attemptCount: 0,
        rememberedCount: 0,
        forgottenCount: 0,
        successStreak: 0,
      });
    }
  }

  async loadFeedPage(input: { cursor?: number; pageSize?: number } = {}) {
    const pageSize = input.pageSize ?? PAGE_SIZE;
    const startOrdinal = (input.cursor ?? -1) + 1;
    const availableCount = (this.currentUnlockedCycle() + 1) * seeds.length;
    const count = Math.max(0, Math.min(pageSize, availableCount - startOrdinal));
    const cards = Array.from({ length: count }, (_, index) =>
      this.cardForOrdinal(startOrdinal + index),
    );
    return clone({
      dayStartUnixMs: dayStart,
      dayEndUnixMs: dayEnd,
      dayLabel: new Intl.DateTimeFormat("zh-CN", {
        year: "numeric",
        month: "long",
        day: "numeric",
        weekday: "long",
      }).format(now),
      cards,
      nextCursor: cards[cards.length - 1]?.ordinal,
      canContinue: startOrdinal + cards.length < availableCount,
      ...this.statistics(),
      sourceCounts: sourceCounts(cards),
    } satisfies ReviewFeedModel);
  }

  async loadFeedItemState(feedItemId: number) {
    const card = this.cardForOrdinal(feedItemId - 1);
    return clone({
      dayStartUnixMs: dayStart,
      dayEndUnixMs: dayEnd,
      card,
      ...this.statistics(),
      canContinue: feedItemId < (this.currentUnlockedCycle() + 1) * seeds.length,
    });
  }

  async prepareFeedCard(input: {
    feedItemId: number;
    learningRecordId: number;
    requestKey: string;
  }) {
    const ordinal = input.feedItemId - 1;
    const seed = seeds[ordinal % seeds.length];
    const cycleIndex = Math.floor(ordinal / seeds.length);
    if (!seed || seed.record.id !== input.learningRecordId) {
      throw new Error("浏览器预览中的制卡身份已经变化。");
    }
    const key = `${input.learningRecordId}:${cycleIndex}`;
    const existing = this.generated.get(key);
    if (existing) return clone(existing);
    const context = generatedContext(seed.record.queryText, cycleIndex);
    const card: GeneratedReviewCard = {
      id: this.nextGeneratedId++,
      learningRecordId: input.learningRecordId,
      variantIndex: cycleIndex,
      englishContext: context,
      englishContextZh: `这是包含 ${seed.record.queryText} 的浏览器预览 AI 语境。`,
      hint: seed.hint,
      model: "browser-fixture",
      createdAtUnixMs: Date.now(),
      expiresAtUnixMs: Date.now() + 30 * DAY_MS,
      lastUsedAtUnixMs: Date.now(),
      useCount: 1,
    };
    this.generated.set(key, card);
    return clone(card);
  }

  async submitOutcome(input: SubmitReviewOutcomeInput) {
    const known = this.outcomeResults.get(input.requestKey);
    if (known) return clone(known);
    const ordinal = input.feedItemId - 1;
    const seed = seeds[ordinal % seeds.length];
    const target = seed && this.targets.get(seed.record.id);
    if (
      !seed ||
      seed.record.id !== input.learningRecordId ||
      !target ||
      target.revision !== input.expectedRevision ||
      this.attempts.has(input.feedItemId)
    ) {
      throw new Error("浏览器预览中的复习条目身份或 revision 已变化。");
    }
    const createdAtUnixMs = Date.now();
    const targetRevision = target.revision + 1;
    const days = input.outcome === "forgotten" ? 1 : input.usedHint ? 2 : 3;
    const nextReviewAtUnixMs = createdAtUnixMs + days * DAY_MS;
    const attempt: ReviewAttempt = {
      id: this.nextAttemptId++,
      feedItemId: input.feedItemId,
      learningRecordId: input.learningRecordId,
      requestKey: input.requestKey,
      expectedRevision: input.expectedRevision,
      targetRevision,
      outcome: input.outcome,
      usedHint: input.usedHint,
      nextReviewAtUnixMs,
      createdAtUnixMs,
    };
    const nextTarget: ReviewTarget = {
      ...target,
      revision: targetRevision,
      nextReviewAtUnixMs,
      attemptCount: target.attemptCount + 1,
      rememberedCount: target.rememberedCount + Number(input.outcome === "remembered"),
      forgottenCount: target.forgottenCount + Number(input.outcome === "forgotten"),
      successStreak:
        input.outcome === "forgotten"
          ? 0
          : target.successStreak + Number(!input.usedHint),
      lastReviewedAtUnixMs: createdAtUnixMs,
      lastOutcome: input.outcome,
      lastUsedHint: input.usedHint,
      lastAttemptId: attempt.id,
    };
    this.targets.set(input.learningRecordId, nextTarget);
    this.attempts.set(input.feedItemId, attempt);
    const result = {
      target: nextTarget,
      attempt,
      canContinue: input.feedItemId < (this.currentUnlockedCycle() + 1) * seeds.length,
    };
    this.outcomeResults.set(input.requestKey, clone(result));
    return clone(result);
  }

  async undoOutcome(input: UndoReviewOutcomeInput) {
    const known = this.outcomeResults.get(input.requestKey);
    if (known) return clone(known);
    const attempt = this.attempts.get(input.feedItemId);
    const target = this.targets.get(input.learningRecordId);
    if (
      !attempt ||
      attempt.id !== input.attemptId ||
      !target ||
      target.revision !== input.expectedRevision
    ) {
      throw new Error("浏览器预览中的撤销目标已经变化。");
    }
    this.attempts.delete(input.feedItemId);
    const active = [...this.attempts.values()]
      .filter((candidate) => candidate.learningRecordId === input.learningRecordId)
      .sort((a, b) => a.createdAtUnixMs - b.createdAtUnixMs || a.id - b.id);
    const last = active[active.length - 1];
    const nextTarget: ReviewTarget = {
      learningRecordId: input.learningRecordId,
      revision: target.revision + 1,
      nextReviewAtUnixMs: last?.nextReviewAtUnixMs ?? dayStart,
      attemptCount: active.length,
      rememberedCount: active.filter((candidate) => candidate.outcome === "remembered").length,
      forgottenCount: active.filter((candidate) => candidate.outcome === "forgotten").length,
      successStreak: last?.outcome === "remembered" && !last.usedHint ? 1 : 0,
      lastReviewedAtUnixMs: last?.createdAtUnixMs,
      lastOutcome: last?.outcome,
      lastUsedHint: last?.usedHint,
      lastAttemptId: last?.id,
    };
    const undoneAttempt: ReviewAttempt = {
      ...attempt,
      undoneAtUnixMs: Date.now(),
      undoRequestKey: input.requestKey,
      undoTargetRevision: nextTarget.revision,
    };
    this.targets.set(input.learningRecordId, nextTarget);
    const result = {
      target: nextTarget,
      attempt: undoneAttempt,
      canContinue: input.feedItemId < (this.currentUnlockedCycle() + 1) * seeds.length,
    };
    this.outcomeResults.set(input.requestKey, clone(result));
    return clone(result);
  }

  async saveQualityFeedback(input: SaveReviewQualityFeedbackInput) {
    const known = this.qualityResults.get(input.requestKey);
    if (known) return clone(known);
    const current = this.feedback.get(input.feedItemId);
    if ((input.expectedRevision ?? undefined) !== current?.revision) {
      throw new Error("浏览器预览中的卡片反馈 revision 已变化。");
    }
    const generatedCardId = this.generatedCardIdForFeedItem(input.feedItemId);
    const cardContextKey = generatedCardId === undefined
      ? "recorded"
      : `generated:${generatedCardId}`;
    if (input.cardContextKey !== cardContextKey) {
      throw new Error("浏览器预览中的具体卡片语境已变化。");
    }
    const result: ReviewQualityFeedback = {
      id: current?.id ?? this.nextFeedbackId++,
      feedItemId: input.feedItemId,
      learningRecordId: input.learningRecordId,
      generatedCardId,
      revision: current ? current.revision + 1 : 0,
      active: true,
      polarity: input.polarity,
      reasonCodes: [...input.reasonCodes],
      detail: input.detail?.trim() || undefined,
      createdAtUnixMs: current?.createdAtUnixMs ?? Date.now(),
      updatedAtUnixMs: Date.now(),
    };
    this.feedback.set(input.feedItemId, result);
    this.qualityResults.set(input.requestKey, clone(result));
    return clone(result);
  }

  async undoQualityFeedback(input: UndoReviewQualityFeedbackInput) {
    const known = this.qualityResults.get(input.requestKey);
    if (known) return clone(known);
    const current = this.feedback.get(input.feedItemId);
    if (
      !current?.active ||
      current.id !== input.feedbackId ||
      current.revision !== input.expectedRevision
    ) {
      throw new Error("浏览器预览中的卡片反馈撤销目标已变化。");
    }
    const result = {
      ...current,
      revision: current.revision + 1,
      active: false,
      updatedAtUnixMs: Date.now(),
    };
    this.feedback.set(input.feedItemId, result);
    this.qualityResults.set(input.requestKey, clone(result));
    return clone(result);
  }

  private cardForOrdinal(ordinal: number): ReviewCardModel {
    const seed = seeds[ordinal % seeds.length];
    const cycleIndex = Math.floor(ordinal / seeds.length);
    const generated = this.generated.get(`${seed.record.id}:${cycleIndex}`);
    const originalContext = cleanContext(seed.record);
    const context = generated?.englishContext || originalContext;
    const split = splitContext(context, seed.record.queryText);
    const target = this.targets.get(seed.record.id)!;
    return {
      feedItemId: ordinal + 1,
      ordinal,
      cycleIndex,
      learningRecordId: seed.record.id,
      reasonCode: cycleIndex === 0 ? "newRecord" : "continuedPractice",
      reason:
        cycleIndex === 0
          ? "这条真实学习记录还没有生效的复习结果。"
          : "已完成一轮浏览，继续用新的语境练习同一目标。",
      typeLabel: seed.record.queryType === "phrase" ? "短语语境" : "单词语境",
      sourceLabel: `划词查询 · ${seed.record.sourceApp}`,
      sourceApp: seed.record.sourceApp ?? "浏览器预览",
      sourceTypeLabel: "划词查询",
      sourceTime: "浏览器预览记录",
      query: seed.record.queryText,
      sourceExcerpt: originalContext,
      promptKind: "cloze",
      promptBefore: split.before,
      promptAnswer: split.answer,
      promptAfter: split.after,
      promptText: context,
      hint: generated?.hint ?? seed.hint,
      answerTitle: seed.answerTitle,
      answerDetail: seed.answerTitle,
      answerNote: "浏览器预览只演示交互；正式 Tauri 使用 SQLite 与真实模型调用。",
      contextOrigin: generated ? "generated" : "recorded",
      contextOriginLabel: generated ? "AI 生成语境" : "学习时英文语境",
      needsPreparation: cycleIndex > 0 && !generated,
      ...reviewCardPresentation({
        promptText: context,
        queryType: seed.record.queryType,
      }),
      learningRecord: seed.record,
      generatedCard: generated,
      target,
      attempt: this.attempts.get(ordinal + 1),
      qualityFeedback: this.feedback.get(ordinal + 1),
    };
  }

  private statistics() {
    const attempts = [...this.attempts.values()];
    return {
      completedCount: attempts.length,
      rememberedCount: attempts.filter((attempt) => attempt.outcome === "remembered").length,
      forgottenCount: attempts.filter((attempt) => attempt.outcome === "forgotten").length,
    };
  }

  private currentUnlockedCycle() {
    let cycleIndex = 0;
    while (
      seeds.every((_, index) =>
        this.attempts.has(cycleIndex * seeds.length + index + 1),
      )
    ) {
      cycleIndex += 1;
    }
    return cycleIndex;
  }

  private generatedCardIdForFeedItem(feedItemId: number) {
    const ordinal = feedItemId - 1;
    const seed = seeds[ordinal % seeds.length];
    const cycleIndex = Math.floor(ordinal / seeds.length);
    return this.generated.get(`${seed.record.id}:${cycleIndex}`)?.id;
  }
}

function cleanContext(record: LearningRecord) {
  return record.contextText?.trim() || record.queryText;
}

function generatedContext(query: string, cycleIndex: number) {
  const variants = [
    `The team used ${query} while explaining the decision to a new colleague.`,
    `A clear example made ${query} easier to remember during the discussion.`,
    `She chose ${query} because it matched the meaning needed in that situation.`,
  ];
  return variants[cycleIndex % variants.length];
}

function sourceCounts(cards: ReviewCardModel[]) {
  const counts = new Map<string, number>();
  for (const card of cards) counts.set(card.sourceApp, (counts.get(card.sourceApp) ?? 0) + 1);
  return [...counts.entries()].map(([label, count]) => ({ label, count }));
}

export function createBrowserPreviewReviewService(): ReviewService {
  return new BrowserPreviewReviewService();
}
