import type { ReviewRepository } from "./reviewRepository";
import type { LearningRecord } from "./types/learningRecord";
import type {
  GeneratedReviewCard,
  PrepareReviewFeedCardInput,
  ReviewCardGenerationFailure,
  ReviewAttempt,
  ReviewFeedItem,
  ReviewFeedItemState,
  ReviewFeedPage,
  ReviewOutcomeWriteResult,
  ReviewQualityFeedback,
  ReviewReasonCode,
  ReviewTarget,
  SaveReviewQualityFeedbackInput,
  SubmitReviewOutcomeInput,
  UndoReviewOutcomeInput,
  UndoReviewQualityFeedbackInput,
} from "./types/review";

export type ReviewPromptKind = "cloze" | "translation" | "meaning";
export type ReviewCardDensity = "compact" | "regular" | "extended";
export type ReviewCardVisualVariant = "lexical" | "editorial" | "quote";
export type ReviewCardPaperTone = "paper" | "mist" | "tint";

export type ReviewSourceCount = { label: string; count: number };

export type ReviewCardModel = {
  feedItemId: number;
  ordinal: number;
  cycleIndex: number;
  learningRecordId: number;
  reasonCode: ReviewReasonCode;
  reason: string;
  typeLabel: string;
  sourceLabel: string;
  sourceApp: string;
  sourceTypeLabel: string;
  sourceTime: string;
  query: string;
  sourceExcerpt: string;
  promptKind: ReviewPromptKind;
  promptBefore: string;
  promptAnswer: string;
  promptAfter: string;
  promptText: string;
  hint: string;
  answerTitle: string;
  answerDetail: string;
  answerNote: string;
  example?: { en: string; zh: string };
  contextOrigin: "recorded" | "generated" | "query";
  contextOriginLabel: string;
  needsPreparation: boolean;
  density: ReviewCardDensity;
  visualVariant: ReviewCardVisualVariant;
  paperTone: ReviewCardPaperTone;
  learningRecord: LearningRecord;
  generatedCard?: GeneratedReviewCard;
  generationFailure?: ReviewCardGenerationFailure;
  target: ReviewTarget;
  attempt?: ReviewAttempt;
  qualityFeedback?: ReviewQualityFeedback;
};

export type ReviewFeedModel = {
  dayStartUnixMs: number;
  dayEndUnixMs: number;
  dayLabel: string;
  cards: ReviewCardModel[];
  nextCursor?: number;
  canContinue: boolean;
  completedCount: number;
  rememberedCount: number;
  forgottenCount: number;
  sourceCounts: ReviewSourceCount[];
};

export type ReviewFeedItemStateModel = {
  dayStartUnixMs: number;
  dayEndUnixMs: number;
  card: ReviewCardModel;
  completedCount: number;
  rememberedCount: number;
  forgottenCount: number;
  canContinue: boolean;
};

export type LoadReviewFeedInput = {
  cursor?: number;
  pageSize?: number;
  now?: Date;
};

export interface ReviewService {
  loadFeedPage(input?: LoadReviewFeedInput): Promise<ReviewFeedModel>;
  loadFeedItemState(feedItemId: number): Promise<ReviewFeedItemStateModel>;
  prepareFeedCard(input: PrepareReviewFeedCardInput): Promise<GeneratedReviewCard>;
  submitOutcome(input: SubmitReviewOutcomeInput): Promise<ReviewOutcomeWriteResult>;
  undoOutcome(input: UndoReviewOutcomeInput): Promise<ReviewOutcomeWriteResult>;
  saveQualityFeedback(
    input: SaveReviewQualityFeedbackInput,
  ): Promise<ReviewQualityFeedback>;
  undoQualityFeedback(
    input: UndoReviewQualityFeedbackInput,
  ): Promise<ReviewQualityFeedback>;
}

const queryTypeLabels = {
  word: "单词语境",
  phrase: "短语语境",
  sentence: "句子理解",
  paragraph: "段落理解",
} as const;

const sourceTypeLabels = {
  manual: "主动查询",
  clipboard: "剪贴板查询",
  windows_uia: "划词查询",
  app_adapter: "应用适配器",
  ocr: "OCR",
} as const;

const reasonLabels: Record<ReviewReasonCode, string> = {
  scheduledToday: "这条学习目标已经到期，或计划在今天结束前复习。",
  newRecord: "这条真实学习记录还没有生效的复习结果。",
  continuedPractice: "已完成一轮浏览，继续用新的语境练习同一目标。",
};

function assertObject(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} 不是有效对象。`);
  }
  return value as Record<string, unknown>;
}

function assertInteger(value: unknown, label: string, minimum = 0) {
  if (!Number.isSafeInteger(value) || Number(value) < minimum) {
    throw new Error(`${label} 不是有效整数。`);
  }
  return Number(value);
}

function assertString(value: unknown, label: string) {
  if (typeof value !== "string" || !value.trim()) {
    throw new Error(`${label} 不是有效字符串。`);
  }
  return value.trim();
}

function assertOptionalInteger(value: unknown, label: string) {
  return value === null || value === undefined
    ? undefined
    : assertInteger(value, label);
}

function cleanText(value?: string | null) {
  return value?.trim() ?? "";
}

function hasCjk(value: string) {
  return [...value].some((character) => {
    const code = character.codePointAt(0) ?? 0;
    return (
      (code >= 0x3400 && code <= 0x9fff) ||
      (code >= 0xf900 && code <= 0xfaff) ||
      (code >= 0x20000 && code <= 0x2ebef) ||
      (code >= 0x2f800 && code <= 0x2fa1f) ||
      (code >= 0x30000 && code <= 0x3134f)
    );
  });
}

export function isUsableEnglishText(value?: string | null) {
  const text = cleanText(value);
  const latinCount = [...text].filter((character) =>
    /[A-Za-z]/.test(character),
  ).length;
  return latinCount >= 8 && !hasCjk(text);
}

function splitAroundNeedle(source: string, needle: string) {
  if (!source || !needle) return undefined;
  const sourceLower = source.toLocaleLowerCase("en-US");
  const needleLower = needle.toLocaleLowerCase("en-US");
  let index = sourceLower.indexOf(needleLower);
  while (index >= 0) {
    const before = sourceLower[index - 1];
    const after = sourceLower[index + needleLower.length];
    const needsLeftBoundary = /[a-z0-9]/i.test(needleLower[0] ?? "");
    const needsRightBoundary = /[a-z0-9]/i.test(
      needleLower[needleLower.length - 1] ?? "",
    );
    if (
      (!needsLeftBoundary || !before || !/[a-z0-9]/i.test(before)) &&
      (!needsRightBoundary || !after || !/[a-z0-9]/i.test(after))
    ) {
      break;
    }
    index = sourceLower.indexOf(needleLower, index + 1);
  }
  if (index < 0) return undefined;
  return {
    before: source.slice(0, index),
    answer: source.slice(index, index + needle.length),
    after: source.slice(index + needle.length),
  };
}

export function isUsableEnglishContext(value: string | null | undefined, query: string) {
  const text = cleanText(value);
  const match = splitAroundNeedle(text, query);
  if (!isUsableEnglishText(text) || !match) return false;
  const surroundingLatinCount = [...`${match.before}${match.after}`].filter(
    (character) => /[A-Za-z]/.test(character),
  ).length;
  return surroundingLatinCount >= 4;
}

function isGeneratableEnglishQuery(value: string) {
  return /[A-Za-z]/.test(value) && !hasCjk(value);
}

function formatSourceApp(record: LearningRecord) {
  return (
    cleanText(record.sourceApp).replace(/\.exe$/i, "") ||
    sourceTypeLabels[record.sourceType]
  );
}

function formatSourceTime(createdAtUnixMs: number) {
  const date = new Date(createdAtUnixMs);
  const clock = `${date.getHours().toString().padStart(2, "0")}:${date
    .getMinutes()
    .toString()
    .padStart(2, "0")}`;
  const now = new Date();
  if (
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  ) {
    return `今天 ${clock}`;
  }
  return `${date.getFullYear()} 年 ${date.getMonth() + 1} 月 ${date.getDate()} 日 ${clock}`;
}

function localDayRange(now: Date) {
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return { dayStartUnixMs: start.getTime(), dayEndUnixMs: end.getTime() };
}

function formatDayLabel(now: Date) {
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
    weekday: "long",
  }).format(now);
}

function recordSourceExcerpt(record: LearningRecord) {
  return (
    cleanText(record.contextText) ||
    cleanText(record.explanationCard.sourceText) ||
    cleanText(record.queryText)
  );
}

function recordedEnglishContext(record: LearningRecord) {
  const query = cleanText(record.learningTargetText);
  const candidates = [record.contextText];
  return candidates.map(cleanText).find((value) => isUsableEnglishContext(value, query));
}

function savedReviewHint(record: LearningRecord) {
  const card = record.explanationCard;
  return card.queryType === "paragraph" ? "" : cleanText(card.reviewHint);
}

function answerContent(record: LearningRecord) {
  const card = record.explanationCard;
  switch (card.queryType) {
    case "word": {
      const meanings = card.basicMeanings.join("；");
      return {
        answerTitle: cleanText(card.contextMeaning) || meanings,
        answerDetail: meanings,
        answerNote: card.nearMeanings
          .map((item) => `${item.term}：${item.meaning}`)
          .join("；"),
        example: card.examples[0],
      };
    }
    case "phrase":
      return {
        answerTitle: cleanText(card.contextMeaning) || card.basicMeaning,
        answerDetail: card.basicMeaning,
        answerNote: cleanText(card.composition),
        example: card.examples[0],
      };
    case "sentence":
      return {
        answerTitle: card.translation,
        answerDetail: cleanText(card.explanation),
        answerNote: card.keyPoints
          .map((item) => `${item.expression}：${item.meaning}`)
          .join("；"),
      };
    case "paragraph":
      return {
        answerTitle: cleanText(card.summary) || card.translation,
        answerDetail: card.translation,
        answerNote: card.keyPoints
          .map((item) => `${item.expression}：${item.meaning}`)
          .join("；"),
      };
  }
}

function cardContent(item: ReviewFeedItem) {
  const record = item.learningRecord;
  const card = record.explanationCard;
  const query = cleanText(record.learningTargetText);
  const sourceExcerpt = recordSourceExcerpt(record);
  const answer = answerContent(record);
  const generated = item.generatedCard ?? undefined;
  const recordedContext = item.cycleIndex === 0 ? recordedEnglishContext(record) : undefined;
  const context = generated?.englishContext || recordedContext;

  if (context) {
    const cloze = splitAroundNeedle(context, query);
    if (!cloze) throw new Error("复习英文语境没有包含目标表达。");
    return {
      sourceExcerpt,
      promptKind: "cloze" as const,
      promptBefore: cloze.before,
      promptAnswer: cloze.answer,
      promptAfter: cloze.after,
      promptText: context,
      hint:
        generated?.hint ||
        savedReviewHint(record) ||
        "回忆这条记录中保存的当前语境义。",
      ...answer,
      contextOrigin: generated ? ("generated" as const) : ("recorded" as const),
      contextOriginLabel: generated ? "AI 生成语境" : "学习时英文语境",
      needsPreparation: false,
      generatedCard: generated,
    };
  }

  if (card.queryType === "sentence" || card.queryType === "paragraph") {
    const englishSource = [record.contextText, query]
      .map(cleanText)
      .find(isUsableEnglishText);
    if (englishSource && item.cycleIndex === 0) {
      return {
        sourceExcerpt,
        promptKind: "translation" as const,
        promptBefore: "",
        promptAnswer: answer.answerTitle,
        promptAfter: "",
        promptText: englishSource,
        hint:
          savedReviewHint(record) ||
          (card.queryType === "paragraph" ? cleanText(card.summary) : "") ||
          "先回忆这段英文的完整中文含义。",
        ...answer,
        contextOrigin: "recorded" as const,
        contextOriginLabel: "学习时英文内容",
        needsPreparation: false,
      };
    }
  }

  return {
    sourceExcerpt,
    promptKind: "meaning" as const,
    promptBefore: "",
    promptAnswer: query,
    promptAfter: "",
    promptText: query,
    hint: savedReviewHint(record) || "根据保存的语境义回忆这个表达。",
    ...answer,
    contextOrigin: "query" as const,
    contextOriginLabel: "后台准备中",
    needsPreparation: isGeneratableEnglishQuery(query),
  };
}

export function reviewCardPresentation(input: {
  promptText: string;
  queryType: LearningRecord["queryType"];
}) {
  const length = [...cleanText(input.promptText)].length;
  const density: ReviewCardDensity =
    length <= 72 ? "compact" : length >= 170 ? "extended" : "regular";
  const visualVariant: ReviewCardVisualVariant =
    input.queryType === "sentence" ||
    input.queryType === "paragraph" ||
    density === "extended"
      ? "quote"
      : density === "compact"
        ? "lexical"
        : "editorial";
  const paperTone: ReviewCardPaperTone =
    visualVariant === "quote"
      ? "mist"
      : visualVariant === "editorial"
        ? "tint"
        : "paper";
  return {
    density,
    visualVariant,
    paperTone,
  };
}

function validateTarget(value: unknown): ReviewTarget {
  const target = assertObject(value, "复习目标");
  if (
    target.lastOutcome !== null &&
    target.lastOutcome !== undefined &&
    target.lastOutcome !== "remembered" &&
    target.lastOutcome !== "forgotten"
  ) {
    throw new Error("复习目标 lastOutcome 无效。");
  }
  if (
    target.lastUsedHint !== null &&
    target.lastUsedHint !== undefined &&
    typeof target.lastUsedHint !== "boolean"
  ) {
    throw new Error("复习目标 lastUsedHint 无效。");
  }
  return {
    learningRecordId: assertInteger(target.learningRecordId, "复习目标 learningRecordId", 1),
    revision: assertInteger(target.revision, "复习目标 revision"),
    nextReviewAtUnixMs: assertInteger(target.nextReviewAtUnixMs, "复习目标 nextReviewAtUnixMs"),
    attemptCount: assertInteger(target.attemptCount, "复习目标 attemptCount"),
    rememberedCount: assertInteger(target.rememberedCount, "复习目标 rememberedCount"),
    forgottenCount: assertInteger(target.forgottenCount, "复习目标 forgottenCount"),
    successStreak: assertInteger(target.successStreak, "复习目标 successStreak"),
    lastReviewedAtUnixMs: assertOptionalInteger(target.lastReviewedAtUnixMs, "复习目标 lastReviewedAtUnixMs"),
    lastOutcome: target.lastOutcome as ReviewTarget["lastOutcome"],
    lastUsedHint: target.lastUsedHint as ReviewTarget["lastUsedHint"],
    lastAttemptId: assertOptionalInteger(target.lastAttemptId, "复习目标 lastAttemptId"),
  };
}

function validateAttempt(value: unknown): ReviewAttempt {
  const attempt = assertObject(value, "复习 attempt");
  if (attempt.outcome !== "remembered" && attempt.outcome !== "forgotten") {
    throw new Error("复习 attempt outcome 无效。");
  }
  if (typeof attempt.usedHint !== "boolean") {
    throw new Error("复习 attempt usedHint 无效。");
  }
  return {
    id: assertInteger(attempt.id, "复习 attempt id", 1),
    feedItemId: assertInteger(attempt.feedItemId, "复习 attempt feedItemId", 1),
    learningRecordId: assertInteger(attempt.learningRecordId, "复习 attempt learningRecordId", 1),
    requestKey: assertString(attempt.requestKey, "复习 attempt requestKey"),
    expectedRevision: assertInteger(attempt.expectedRevision, "复习 attempt expectedRevision"),
    targetRevision: assertInteger(attempt.targetRevision, "复习 attempt targetRevision", 1),
    outcome: attempt.outcome,
    usedHint: attempt.usedHint,
    nextReviewAtUnixMs: assertInteger(attempt.nextReviewAtUnixMs, "复习 attempt nextReviewAtUnixMs"),
    createdAtUnixMs: assertInteger(attempt.createdAtUnixMs, "复习 attempt createdAtUnixMs"),
    undoneAtUnixMs: assertOptionalInteger(attempt.undoneAtUnixMs, "复习 attempt undoneAtUnixMs"),
    undoRequestKey:
      attempt.undoRequestKey === null || attempt.undoRequestKey === undefined
        ? undefined
        : assertString(attempt.undoRequestKey, "复习 attempt undoRequestKey"),
    undoTargetRevision: assertOptionalInteger(attempt.undoTargetRevision, "复习 attempt undoTargetRevision"),
  };
}

function validateQualityFeedback(value: unknown): ReviewQualityFeedback {
  const feedback = assertObject(value, "卡片反馈");
  if (feedback.polarity !== "up" && feedback.polarity !== "down") {
    throw new Error("卡片反馈 polarity 无效。");
  }
  if (typeof feedback.active !== "boolean" || !Array.isArray(feedback.reasonCodes)) {
    throw new Error("卡片反馈状态无效。");
  }
  return {
    id: assertInteger(feedback.id, "卡片反馈 id", 1),
    feedItemId: assertInteger(feedback.feedItemId, "卡片反馈 feedItemId", 1),
    learningRecordId: assertInteger(feedback.learningRecordId, "卡片反馈 learningRecordId", 1),
    generatedCardId: assertOptionalInteger(
      feedback.generatedCardId,
      "卡片反馈 generatedCardId",
    ),
    revision: assertInteger(feedback.revision, "卡片反馈 revision"),
    active: feedback.active,
    polarity: feedback.polarity,
    reasonCodes: feedback.reasonCodes.map((reason, index) =>
      assertString(reason, `卡片反馈 reasonCodes[${index}]`),
    ),
    detail:
      feedback.detail === null || feedback.detail === undefined
        ? undefined
        : assertString(feedback.detail, "卡片反馈 detail"),
    createdAtUnixMs: assertInteger(feedback.createdAtUnixMs, "卡片反馈 createdAtUnixMs"),
    updatedAtUnixMs: assertInteger(feedback.updatedAtUnixMs, "卡片反馈 updatedAtUnixMs"),
  };
}

function validateLearningRecord(value: unknown): LearningRecord {
  const record = assertObject(value, "复习学习记录");
  if (!( ["word", "phrase", "sentence", "paragraph"] as unknown[]).includes(record.queryType)) {
    throw new Error("复习学习记录 queryType 无效。");
  }
  if (!( ["manual", "clipboard", "windows_uia", "app_adapter", "ocr"] as unknown[]).includes(record.sourceType)) {
    throw new Error("复习学习记录 sourceType 无效。");
  }
  const explanationCard = assertObject(record.explanationCard, "复习学习记录 explanationCard");
  if (explanationCard.queryType !== record.queryType) {
    throw new Error("复习学习记录与 ExplanationCard 类型不一致。");
  }
  assertInteger(record.id, "复习学习记录 id", 1);
  assertString(record.queryText, "复习学习记录 queryText");
  if (record.queryDirection !== "enToZh" && record.queryDirection !== "zhToEn") {
    throw new Error("复习学习记录 queryDirection 无效。");
  }
  assertString(record.learningTargetText, "复习学习记录 learningTargetText");
  if (hasCjk(String(record.learningTargetText)) || !/[A-Za-z]/.test(String(record.learningTargetText))) {
    throw new Error("复习学习记录 learningTargetText 不是规范英文目标。");
  }
  assertInteger(record.createdAtUnixMs, "复习学习记录 createdAtUnixMs");
  return value as LearningRecord;
}

function validateGeneratedCard(value: unknown): GeneratedReviewCard {
  const card = assertObject(value, "AI 复习卡");
  return {
    id: assertInteger(card.id, "AI 复习卡 id", 1),
    learningRecordId: assertInteger(card.learningRecordId, "AI 复习卡 learningRecordId", 1),
    variantIndex: assertInteger(card.variantIndex, "AI 复习卡 variantIndex"),
    englishContext: assertString(card.englishContext, "AI 复习卡 englishContext"),
    englishContextZh: assertString(card.englishContextZh, "AI 复习卡 englishContextZh"),
    hint: assertString(card.hint, "AI 复习卡 hint"),
    model: assertString(card.model, "AI 复习卡 model"),
    createdAtUnixMs: assertInteger(card.createdAtUnixMs, "AI 复习卡 createdAtUnixMs"),
    expiresAtUnixMs: assertInteger(card.expiresAtUnixMs, "AI 复习卡 expiresAtUnixMs"),
    lastUsedAtUnixMs: assertInteger(card.lastUsedAtUnixMs, "AI 复习卡 lastUsedAtUnixMs"),
    useCount: assertInteger(card.useCount, "AI 复习卡 useCount", 1),
  };
}

function validateGenerationFailure(value: unknown): ReviewCardGenerationFailure {
  const failure = assertObject(value, "AI 复习卡失败状态");
  return {
    requestKey: assertString(failure.requestKey, "AI 复习卡失败状态 requestKey"),
    feedItemId: assertInteger(failure.feedItemId, "AI 复习卡失败状态 feedItemId", 1),
    learningRecordId: assertInteger(
      failure.learningRecordId,
      "AI 复习卡失败状态 learningRecordId",
      1,
    ),
    failureCount: assertInteger(failure.failureCount, "AI 复习卡失败状态 failureCount", 1),
    retryAfterUnixMs: assertInteger(
      failure.retryAfterUnixMs,
      "AI 复习卡失败状态 retryAfterUnixMs",
    ),
    lastError: assertString(failure.lastError, "AI 复习卡失败状态 lastError"),
    createdAtUnixMs: assertInteger(
      failure.createdAtUnixMs,
      "AI 复习卡失败状态 createdAtUnixMs",
    ),
    updatedAtUnixMs: assertInteger(
      failure.updatedAtUnixMs,
      "AI 复习卡失败状态 updatedAtUnixMs",
    ),
  };
}

function validateFeedItem(value: unknown): ReviewFeedItem {
  const item = assertObject(value, "复习 Feed 条目");
  if (
    item.reasonCode !== "scheduledToday" &&
    item.reasonCode !== "newRecord" &&
    item.reasonCode !== "continuedPractice"
  ) {
    throw new Error("复习 Feed 条目 reasonCode 无效。");
  }
  const learningRecord = validateLearningRecord(item.learningRecord);
  const target = validateTarget(item.target);
  const id = assertInteger(item.id, "复习 Feed 条目 id", 1);
  if (learningRecord.id !== target.learningRecordId) {
    throw new Error("复习 Feed 条目的记录与目标身份不一致。");
  }
  const attempt = item.attempt == null ? undefined : validateAttempt(item.attempt);
  if (
    attempt &&
    (attempt.learningRecordId !== learningRecord.id ||
      attempt.feedItemId !== id ||
      attempt.undoneAtUnixMs !== undefined)
  ) {
    throw new Error("复习 Feed 条目的生效 attempt 身份无效。");
  }
  const qualityFeedback =
    item.qualityFeedback == null
      ? undefined
      : validateQualityFeedback(item.qualityFeedback);
  const generatedCard =
    item.generatedCard == null ? undefined : validateGeneratedCard(item.generatedCard);
  const generationFailure =
    item.generationFailure == null
      ? undefined
      : validateGenerationFailure(item.generationFailure);
  const cycleIndex = assertInteger(item.cycleIndex, "复习 Feed 条目 cycleIndex");
  if (
    generatedCard &&
    generatedCard.learningRecordId !== learningRecord.id
  ) {
    throw new Error("AI 复习卡与 Feed 条目身份不一致。");
  }
  if (
    generationFailure &&
    (generationFailure.feedItemId !== id ||
      generationFailure.learningRecordId !== learningRecord.id)
  ) {
    throw new Error("AI 复习卡失败状态与 Feed 条目身份不一致。");
  }
  if (
    qualityFeedback &&
    (qualityFeedback.feedItemId !== id ||
      qualityFeedback.learningRecordId !== learningRecord.id ||
      qualityFeedback.generatedCardId !== generatedCard?.id)
  ) {
    throw new Error("卡片反馈与 Feed 条目或生成语境身份不一致。");
  }
  return {
    id,
    ordinal: assertInteger(item.ordinal, "复习 Feed 条目 ordinal"),
    cycleIndex,
    reasonCode: item.reasonCode,
    learningRecord,
    target,
    attempt,
    qualityFeedback,
    generatedCard,
    generationFailure,
  };
}

function validateFeedPage(value: unknown): ReviewFeedPage {
  const page = assertObject(value, "复习 Feed 页面");
  if (!Array.isArray(page.items) || typeof page.canContinue !== "boolean") {
    throw new Error("复习 Feed 页面状态无效。");
  }
  const items = page.items.map(validateFeedItem);
  const completedCount = assertInteger(page.completedCount, "复习 Feed completedCount");
  const rememberedCount = assertInteger(page.rememberedCount, "复习 Feed rememberedCount");
  const forgottenCount = assertInteger(page.forgottenCount, "复习 Feed forgottenCount");
  if (completedCount !== rememberedCount + forgottenCount) {
    throw new Error("复习 Feed 完成统计与结果分类不一致。");
  }
  return {
    dayStartUnixMs: assertInteger(page.dayStartUnixMs, "复习 Feed dayStartUnixMs"),
    dayEndUnixMs: assertInteger(page.dayEndUnixMs, "复习 Feed dayEndUnixMs"),
    pageSize: assertInteger(page.pageSize, "复习 Feed pageSize", 1),
    items,
    nextCursor: assertOptionalInteger(page.nextCursor, "复习 Feed nextCursor"),
    canContinue: page.canContinue,
    completedCount,
    rememberedCount,
    forgottenCount,
  };
}

function validateFeedItemState(value: unknown): ReviewFeedItemState {
  const state = assertObject(value, "复习 Feed 条目权威状态");
  if (typeof state.canContinue !== "boolean") {
    throw new Error("复习 Feed 条目权威状态 canContinue 无效。");
  }
  const completedCount = assertInteger(
    state.completedCount,
    "复习 Feed 条目权威状态 completedCount",
  );
  const rememberedCount = assertInteger(
    state.rememberedCount,
    "复习 Feed 条目权威状态 rememberedCount",
  );
  const forgottenCount = assertInteger(
    state.forgottenCount,
    "复习 Feed 条目权威状态 forgottenCount",
  );
  if (completedCount !== rememberedCount + forgottenCount) {
    throw new Error("复习 Feed 条目权威统计与结果分类不一致。");
  }
  return {
    dayStartUnixMs: assertInteger(
      state.dayStartUnixMs,
      "复习 Feed 条目权威状态 dayStartUnixMs",
    ),
    dayEndUnixMs: assertInteger(
      state.dayEndUnixMs,
      "复习 Feed 条目权威状态 dayEndUnixMs",
    ),
    item: validateFeedItem(state.item),
    completedCount,
    rememberedCount,
    forgottenCount,
    canContinue: state.canContinue,
  };
}

function mapFeedItem(item: ReviewFeedItem): ReviewCardModel {
  const record = item.learningRecord;
  const sourceApp = formatSourceApp(record);
  const content = cardContent(item);
  return {
    feedItemId: item.id,
    ordinal: item.ordinal,
    cycleIndex: item.cycleIndex,
    learningRecordId: record.id,
    reasonCode: item.reasonCode,
    reason: reasonLabels[item.reasonCode],
    typeLabel: queryTypeLabels[record.queryType],
    sourceLabel: `${sourceTypeLabels[record.sourceType]} · ${sourceApp}`,
    sourceApp,
    sourceTypeLabel: sourceTypeLabels[record.sourceType],
    sourceTime: formatSourceTime(record.createdAtUnixMs),
    query: cleanText(record.learningTargetText),
    ...content,
    ...reviewCardPresentation({
      promptText: content.promptText,
      queryType: record.queryType,
    }),
    learningRecord: record,
    target: item.target,
    attempt: item.attempt ?? undefined,
    qualityFeedback: item.qualityFeedback ?? undefined,
    generationFailure: item.generationFailure ?? undefined,
  };
}

function sourceCounts(cards: ReviewCardModel[]) {
  const counts = new Map<string, number>();
  for (const card of cards) counts.set(card.sourceApp, (counts.get(card.sourceApp) ?? 0) + 1);
  return [...counts.entries()].map(([label, count]) => ({ label, count }));
}

export function mapReviewFeedPage(value: unknown, now = new Date()): ReviewFeedModel {
  const page = validateFeedPage(value);
  const cards = page.items.map(mapFeedItem);
  return {
    dayStartUnixMs: page.dayStartUnixMs,
    dayEndUnixMs: page.dayEndUnixMs,
    dayLabel: formatDayLabel(now),
    cards,
    nextCursor: page.nextCursor ?? undefined,
    canContinue: page.canContinue,
    completedCount: page.completedCount,
    rememberedCount: page.rememberedCount,
    forgottenCount: page.forgottenCount,
    sourceCounts: sourceCounts(cards),
  };
}

export function mapReviewFeedItemState(value: unknown): ReviewFeedItemStateModel {
  const state = validateFeedItemState(value);
  return {
    dayStartUnixMs: state.dayStartUnixMs,
    dayEndUnixMs: state.dayEndUnixMs,
    card: mapFeedItem(state.item),
    completedCount: state.completedCount,
    rememberedCount: state.rememberedCount,
    forgottenCount: state.forgottenCount,
    canContinue: state.canContinue,
  };
}

export function appendReviewFeedPage(current: ReviewFeedModel, next: ReviewFeedModel) {
  if (
    current.dayStartUnixMs !== next.dayStartUnixMs ||
    current.dayEndUnixMs !== next.dayEndUnixMs
  ) {
    throw new Error("不能把不同本地日期的复习 Feed 合并。");
  }
  const byId = new Map(current.cards.map((card) => [card.feedItemId, card]));
  for (const card of next.cards) byId.set(card.feedItemId, card);
  const targetByRecord = new Map<number, ReviewTarget>();
  for (const card of byId.values()) {
    const knownTarget = targetByRecord.get(card.learningRecordId);
    if (!knownTarget || card.target.revision > knownTarget.revision) {
      targetByRecord.set(card.learningRecordId, card.target);
    }
  }
  const cards = [...byId.values()]
    .map((card) => ({
      ...card,
      target: targetByRecord.get(card.learningRecordId) ?? card.target,
    }))
    .sort((a, b) => a.ordinal - b.ordinal);
  return {
    ...current,
    cards,
    nextCursor: next.nextCursor,
    canContinue: next.canContinue,
    completedCount: next.completedCount,
    rememberedCount: next.rememberedCount,
    forgottenCount: next.forgottenCount,
    sourceCounts: sourceCounts(cards),
  };
}

export function applyReviewFeedItemState(
  feed: ReviewFeedModel,
  state: ReviewFeedItemStateModel,
) {
  if (
    feed.dayStartUnixMs !== state.dayStartUnixMs ||
    feed.dayEndUnixMs !== state.dayEndUnixMs
  ) {
    throw new Error("复习 Feed 条目权威状态属于不同本地日期。");
  }
  const current = feed.cards.find(
    (card) => card.feedItemId === state.card.feedItemId,
  );
  if (current && current.learningRecordId !== state.card.learningRecordId) {
    throw new Error("复习 Feed 条目权威状态与当前页面身份不一致。");
  }
  const byId = new Map(feed.cards.map((card) => [card.feedItemId, card]));
  byId.set(state.card.feedItemId, state.card);
  const cards = [...byId.values()]
    .map((card) => ({
      ...card,
      target:
        card.learningRecordId === state.card.learningRecordId
          ? state.card.target
          : card.target,
    }))
    .sort((left, right) => left.ordinal - right.ordinal);
  return {
    ...feed,
    cards,
    completedCount: state.completedCount,
    rememberedCount: state.rememberedCount,
    forgottenCount: state.forgottenCount,
    canContinue: state.canContinue,
    sourceCounts: sourceCounts(cards),
  };
}

export function visibleReviewCards(feed: ReviewFeedModel | undefined) {
  return feed?.cards.filter((card) => !card.needsPreparation) ?? [];
}

export function reviewQualityCardContextKey(card: ReviewCardModel) {
  return card.generatedCard ? `generated:${card.generatedCard.id}` : "recorded";
}

export function createImmediateReviewQualityFeedbackInput(
  card: ReviewCardModel,
  polarity: ReviewQualityFeedback["polarity"],
  requestKey: string,
): SaveReviewQualityFeedbackInput | undefined {
  if (card.qualityFeedback?.active && card.qualityFeedback.polarity === polarity) {
    return undefined;
  }
  return {
    feedItemId: card.feedItemId,
    learningRecordId: card.learningRecordId,
    cardContextKey: reviewQualityCardContextKey(card),
    expectedRevision: card.qualityFeedback?.revision,
    polarity,
    reasonCodes: [],
    requestKey,
  };
}

export function applyPreparedReviewCard(
  feed: ReviewFeedModel,
  feedItemId: number,
  generatedCard: GeneratedReviewCard,
) {
  let matched = false;
  const cards = feed.cards.map((card) => {
    if (card.feedItemId !== feedItemId) return card;
    if (card.learningRecordId !== generatedCard.learningRecordId) {
      throw new Error("AI 复习卡与当前 Feed 条目身份不一致。");
    }
    matched = true;
    return mapFeedItem({
      id: card.feedItemId,
      ordinal: card.ordinal,
      cycleIndex: card.cycleIndex,
      reasonCode: card.reasonCode,
      learningRecord: card.learningRecord,
      target: card.target,
      attempt: card.attempt,
      qualityFeedback: card.qualityFeedback,
      generatedCard,
      generationFailure: undefined,
    });
  });
  if (!matched) throw new Error("AI 复习卡对应的页面条目不存在。");
  return { ...feed, cards };
}

export function applyReviewOutcomeResult(
  feed: ReviewFeedModel,
  feedItemId: number,
  result: ReviewOutcomeWriteResult,
) {
  const targetItem = feed.cards.find((card) => card.feedItemId === feedItemId);
  if (
    !targetItem ||
    targetItem.learningRecordId !== result.target.learningRecordId ||
    result.attempt.feedItemId !== feedItemId ||
    result.attempt.learningRecordId !== targetItem.learningRecordId
  ) {
    throw new Error("复习结果与当前页面条目身份不一致。");
  }
  const wasCompleted = Boolean(targetItem.attempt);
  const isCompleted = result.attempt.undoneAtUnixMs == null;
  const previousOutcome = targetItem.attempt?.outcome;
  const nextOutcome = isCompleted ? result.attempt.outcome : undefined;
  const cards = feed.cards.map((card) => ({
    ...card,
    target:
      card.learningRecordId === result.target.learningRecordId
        ? result.target
        : card.target,
    attempt:
      card.feedItemId === feedItemId
        ? isCompleted
          ? result.attempt
          : undefined
        : card.attempt,
  }));
  return {
    ...feed,
    cards,
    completedCount: feed.completedCount + Number(isCompleted) - Number(wasCompleted),
    rememberedCount:
      feed.rememberedCount +
      Number(nextOutcome === "remembered") -
      Number(previousOutcome === "remembered"),
    forgottenCount:
      feed.forgottenCount +
      Number(nextOutcome === "forgotten") -
      Number(previousOutcome === "forgotten"),
    canContinue: result.canContinue,
  };
}

export function applyReviewQualityFeedback(
  feed: ReviewFeedModel,
  feedback: ReviewQualityFeedback,
) {
  let matched = false;
  const cards = feed.cards.map((card) => {
    if (card.feedItemId !== feedback.feedItemId) return card;
    if (
      card.learningRecordId !== feedback.learningRecordId ||
      card.generatedCard?.id !== (feedback.generatedCardId ?? undefined)
    ) {
      throw new Error("卡片反馈与页面条目或生成语境身份不一致。");
    }
    matched = true;
    return { ...card, qualityFeedback: feedback };
  });
  if (!matched) throw new Error("卡片反馈对应的页面条目不存在。");
  return { ...feed, cards };
}

function validateOutcomeResult(value: unknown): ReviewOutcomeWriteResult {
  const result = assertObject(value, "复习结果写回");
  const target = validateTarget(result.target);
  const attempt = validateAttempt(result.attempt);
  if (
    target.learningRecordId !== attempt.learningRecordId ||
    target.revision < (attempt.undoTargetRevision ?? attempt.targetRevision)
  ) {
    throw new Error("复习结果写回的目标与 attempt 身份不一致。");
  }
  if (typeof result.canContinue !== "boolean") {
    throw new Error("复习结果写回的 canContinue 无效。");
  }
  return { target, attempt, canContinue: result.canContinue };
}

export class RepositoryReviewService implements ReviewService {
  private readonly repository: ReviewRepository;

  constructor(repository: ReviewRepository) {
    this.repository = repository;
  }

  async loadFeedPage(input: LoadReviewFeedInput = {}) {
    const now = input.now ?? new Date();
    const { dayStartUnixMs, dayEndUnixMs } = localDayRange(now);
    const page = await this.repository.loadFeedPage(
      dayStartUnixMs,
      dayEndUnixMs,
      input.cursor,
      input.pageSize,
    );
    return mapReviewFeedPage(page, now);
  }

  async loadFeedItemState(feedItemId: number) {
    return mapReviewFeedItemState(
      await this.repository.loadFeedItemState(feedItemId),
    );
  }

  async prepareFeedCard(input: PrepareReviewFeedCardInput) {
    return validateGeneratedCard(await this.repository.prepareFeedCard(input));
  }

  async submitOutcome(input: SubmitReviewOutcomeInput) {
    return validateOutcomeResult(await this.repository.submitOutcome(input));
  }

  async undoOutcome(input: UndoReviewOutcomeInput) {
    return validateOutcomeResult(await this.repository.undoOutcome(input));
  }

  async saveQualityFeedback(input: SaveReviewQualityFeedbackInput) {
    return validateQualityFeedback(await this.repository.saveQualityFeedback(input));
  }

  async undoQualityFeedback(input: UndoReviewQualityFeedbackInput) {
    return validateQualityFeedback(await this.repository.undoQualityFeedback(input));
  }
}
