import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  RepositoryReviewService,
  appendReviewFeedPage,
  applyPreparedReviewCard,
  applyReviewOutcomeResult,
  applyReviewQualityFeedback,
  applyReviewFeedItemState,
  createImmediateReviewQualityFeedbackInput,
  isUsableEnglishContext,
  mapReviewFeedItemState,
  mapReviewFeedPage,
  reviewCardPresentation,
  visibleReviewCards,
} from "../src/reviewService.ts";
import { ReviewAuthorityRefreshGate } from "../src/reviewAuthorityRefresh.ts";
import { isReviewMutationCurrent } from "../src/reviewRequestIdentity.ts";

const NOW = new Date(2026, 7, 9, 12, 0, 0);
const DAY_START = new Date(2026, 7, 9).getTime();
const DAY_END = new Date(2026, 7, 10).getTime();

function learningRecord(id = 11, overrides = {}) {
  return {
    id,
    learningTargetId: id,
    queryText: "robust",
    learningTargetText: "robust",
    queryDirection: "enToZh",
    normalizedText: "robust",
    queryType: "word",
    sourceType: "windows_uia",
    sourceApp: "Code.exe",
    contextText:
      "The retry path must remain robust when an older response arrives late.",
    explanationCard: {
      queryType: "word",
      sourceText: "robust",
      learningTargetText: "robust",
      headword: "robust",
      partOfSpeech: "adjective",
      phonetic: "/rəʊˈbʌst/",
      basicMeanings: ["稳健的", "可靠的"],
      contextMeaning: "在异常情况下仍然可靠",
      sourceSentence:
        "The retry path must remain robust when an older response arrives late.",
      sourceSentenceZh: "旧响应迟到时，重试路径仍须可靠。",
      phrases: [],
      nearMeanings: [{ term: "resilient", meaning: "强调恢复能力" }],
      examples: [{ en: "The parser is robust.", zh: "这个解析器很稳健。" }],
      reviewHint: "强调异常情况下的可靠性。",
    },
    schemaVersion: 1,
    createdAtUnixMs: DAY_START - 1_000,
    difficulty: null,
    ...overrides,
  };
}

function target(learningTargetId = 11, overrides = {}) {
  return {
    learningTargetId,
    revision: 0,
    nextReviewAtUnixMs: DAY_START,
    attemptCount: 0,
    rememberedCount: 0,
    forgottenCount: 0,
    successStreak: 0,
    lastReviewedAtUnixMs: null,
    lastOutcome: null,
    lastUsedHint: null,
    lastAttemptId: null,
    ...overrides,
  };
}

function feedItem(overrides = {}) {
  return {
    id: 31,
    ordinal: 0,
    cycleIndex: 0,
    reasonCode: "newRecord",
    learningRecord: learningRecord(),
    target: target(),
    attempt: null,
    qualityFeedback: null,
    generatedCard: null,
    ...overrides,
  };
}

function page(overrides = {}) {
  return {
    dayStartUnixMs: DAY_START,
    dayEndUnixMs: DAY_END,
    pageSize: 12,
    items: [feedItem()],
    nextCursor: 0,
    canContinue: true,
    completedCount: 0,
    rememberedCount: 0,
    forgottenCount: 0,
    ...overrides,
  };
}

function attempt(overrides = {}) {
  return {
    id: 51,
    feedItemId: 31,
    learningRecordId: 11,
    learningTargetId: 11,
    requestKey: "review-outcome:stable",
    expectedRevision: 0,
    targetRevision: 1,
    outcome: "remembered",
    usedHint: false,
    nextReviewAtUnixMs: DAY_END + 2 * 24 * 60 * 60 * 1_000,
    createdAtUnixMs: DAY_START + 500,
    undoneAtUnixMs: null,
    undoRequestKey: null,
    undoTargetRevision: null,
    ...overrides,
  };
}

function generatedCard(overrides = {}) {
  return {
    id: 81,
    learningRecordId: 11,
    learningTargetId: 11,
    variantIndex: 1,
    englishContext: "A robust plan can survive an unexpected change.",
    englishContextZh: "稳健的计划可以应对意外变化。",
    hint: "想想强调可靠性的形容词。",
    model: "deepseek-v4-flash",
    createdAtUnixMs: DAY_START + 600,
    expiresAtUnixMs: DAY_END + 30 * 24 * 60 * 60 * 1_000,
    lastUsedAtUnixMs: DAY_START + 600,
    useCount: 1,
    ...overrides,
  };
}

function repository(overrides = {}) {
  return {
    loadFeedPage: async () => page(),
    loadFeedItemState: async () => {
      throw new Error("unused");
    },
    prepareFeedCard: async () => generatedCard(),
    submitOutcome: async () => {
      throw new Error("unused");
    },
    undoOutcome: async () => {
      throw new Error("unused");
    },
    saveQualityFeedback: async () => {
      throw new Error("unused");
    },
    undoQualityFeedback: async () => {
      throw new Error("unused");
    },
    ...overrides,
  };
}

test("正式 ReviewService 用本机日期边界和 cursor 加载持久化 Feed", async () => {
  let received;
  const service = new RepositoryReviewService(
    repository({
      loadFeedPage: async (...args) => {
        received = args;
        return page();
      },
    }),
  );
  const model = await service.loadFeedPage({ cursor: 9, pageSize: 12, now: NOW });
  assert.deepEqual(received, [DAY_START, DAY_END, 9, 12]);
  assert.equal(model.cards.length, 1);
  assert.equal(model.nextCursor, 0);
  assert.equal(model.canContinue, true);
  assert.equal(model.sourceCounts[0].label, "Code");
});

test("复习来源类型与应用同名时只展示一次", () => {
  const model = mapReviewFeedPage(
    page({
      items: [
        feedItem({
          learningRecord: learningRecord(11, {
            sourceType: "manual",
            sourceApp: "主动查询",
          }),
        }),
      ],
    }),
    NOW,
  );

  assert.equal(model.cards[0].sourceLabel, "主动查询");
  assert.equal(model.cards[0].sourceApp, "主动查询");
});

test("学习记录中的完整英文语境直接使用", () => {
  const model = mapReviewFeedPage(page(), NOW);
  const card = model.cards[0];
  assert.equal(card.promptKind, "cloze");
  assert.equal(card.promptAnswer, "robust");
  assert.equal(card.promptBefore, "The retry path must remain ");
  assert.equal(card.contextOrigin, "recorded");
  assert.equal(card.needsPreparation, false);
  assert.equal(card.sourceExcerpt, learningRecord().contextText);
  assert.equal(isUsableEnglishContext(card.promptText, "robust"), true);
  assert.equal(
    isUsableEnglishContext("A robustly designed parser can still fail.", "robust"),
    false,
  );
  assert.equal(isUsableEnglishContext("persistent", "persistent"), false);
});

test("过长或带终端装饰的原始上下文不冒充单词语境", () => {
  const noisyContext = `PowerShell 7.6.3 \uE0B6 19150 D:\\project\\ReadRay features ${
    "Running BeforeDevCommand pnpm dev. ".repeat(40)
  }`;
  assert.equal(isUsableEnglishContext(noisyContext, "features"), false);
  assert.equal(
    isUsableEnglishContext("The \uE0B6 features are listed in the terminal output.", "features"),
    false,
  );
  assert.equal(
    isUsableEnglishContext("The features are listed in the release notes. ".repeat(20), "features"),
    false,
  );

  const record = learningRecord(11, {
    queryText: "features",
    learningTargetText: "features",
    contextText: noisyContext,
  });
  const model = mapReviewFeedPage(
    page({ items: [feedItem({ learningRecord: record })] }),
    NOW,
  );
  assert.equal(model.cards[0].promptKind, "meaning");
  assert.equal(model.cards[0].promptText, "features");
  assert.equal(model.cards[0].needsPreparation, true);
});

test("ExplanationCard 的 AI 例句和 sourceSentence 不冒充学习时语境", () => {
  const record = learningRecord(11, {
    contextText: "这是一段中文上下文。",
  });
  const model = mapReviewFeedPage(
    page({ items: [feedItem({ learningRecord: record })] }),
    NOW,
  );
  assert.equal(model.cards[0].promptKind, "meaning");
  assert.equal(model.cards[0].needsPreparation, true);
  assert.notEqual(
    model.cards[0].promptText,
    record.explanationCard.examples[0].en,
  );
  assert.notEqual(
    model.cards[0].promptText,
    record.explanationCard.sourceSentence,
  );
});

test("缺少英文语境时保留可读查询并在生成成功后换成持久化英文卡", () => {
  const record = learningRecord(11, {
    contextText: "只有中文上下文",
    explanationCard: {
      ...learningRecord().explanationCard,
      sourceSentence: null,
      examples: [],
      sourceText: "robust",
    },
  });
  const initial = mapReviewFeedPage(
    page({ items: [feedItem({ learningRecord: record })] }),
    NOW,
  );
  assert.equal(initial.cards[0].promptKind, "meaning");
  assert.equal(initial.cards[0].promptText, "robust");
  assert.equal(initial.cards[0].needsPreparation, true);

  const prepared = applyPreparedReviewCard(
    initial,
    31,
    generatedCard({ variantIndex: 0 }),
  );
  assert.equal(prepared.cards[0].needsPreparation, false);
  assert.equal(prepared.cards[0].contextOrigin, "generated");
  assert.equal(prepared.cards[0].promptAnswer, "robust");
  assert.equal(prepared.cards[0].generatedCard.englishContextZh, "稳健的计划可以应对意外变化。");
});

test("卡片密度和 editorial variant 只由最终内容稳定决定", () => {
  const compact = reviewCardPresentation({
    promptText: "The handoff went smoothly.",
    queryType: "word",
  });
  const regular = reviewCardPresentation({
    promptText: "A clear handoff helps the next person understand the decision without reopening every earlier discussion.",
    queryType: "word",
  });
  const extendedText = `${"This paragraph keeps enough English context to explain the original decision clearly. ".repeat(3)}End.`;
  const extended = reviewCardPresentation({
    promptText: extendedText,
    queryType: "paragraph",
  });

  assert.deepEqual(compact, {
    density: "compact",
    visualVariant: "lexical",
    paperTone: "paper",
  });
  assert.deepEqual(regular, {
    density: "regular",
    visualVariant: "editorial",
    paperTone: "tint",
  });
  assert.deepEqual(extended, {
    density: "extended",
    visualVariant: "quote",
    paperTone: "mist",
  });
  assert.deepEqual(
    reviewCardPresentation({ promptText: extendedText, queryType: "paragraph" }),
    extended,
  );
});

test("同一目标下一轮允许新的英文语境，不复用第一轮原句", () => {
  const model = mapReviewFeedPage(
    page({
      items: [
        feedItem({
          id: 32,
          ordinal: 1,
          cycleIndex: 1,
          reasonCode: "continuedPractice",
        }),
      ],
      nextCursor: 1,
    }),
    NOW,
  );
  assert.equal(model.cards[0].learningRecordId, 11);
  assert.equal(model.cards[0].needsPreparation, true);
  assert.equal(model.cards[0].reasonCode, "continuedPractice");
});

test("等待后台生成的条目不会发布到可浏览 Feed", () => {
  const model = mapReviewFeedPage(
    page({
      items: [
        feedItem(),
        feedItem({
          id: 32,
          ordinal: 1,
          cycleIndex: 1,
          reasonCode: "continuedPractice",
        }),
      ],
      nextCursor: 1,
    }),
    NOW,
  );
  assert.deepEqual(
    visibleReviewCards(model).map((card) => card.feedItemId),
    [31],
  );
  assert.equal(visibleReviewCards(model).some((card) => card.needsPreparation), false);
});

test("持久化制卡失败退避随 Feed 映射并拒绝错配条目身份", () => {
  const requestKey = `review-card:${DAY_START}:31:11:0`;
  const generationFailure = {
    requestKey,
    feedItemId: 31,
    learningRecordId: 11,
    failureCount: 2,
    retryAfterUnixMs: DAY_START + 60_000,
    lastError: "model unavailable",
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START + 1,
  };
  const mapped = mapReviewFeedPage(
    page({
      items: [
        feedItem({
          learningRecord: learningRecord(11, { contextText: "只有中文语境" }),
          generationFailure,
        }),
      ],
    }),
    NOW,
  );
  assert.equal(mapped.cards[0].generationFailure.requestKey, requestKey);
  assert.equal(mapped.cards[0].generationFailure.failureCount, 2);
  assert.throws(
    () =>
      mapReviewFeedPage(
        page({
          items: [
            feedItem({
              learningRecord: learningRecord(11, { contextText: "只有中文语境" }),
              generationFailure: { ...generationFailure, feedItemId: 99 },
            }),
          ],
        }),
        NOW,
      ),
    /失败状态与 Feed 条目身份不一致/,
  );
});

test("分页统计来自全局真实 attempt，不要求等于当前一页的已完成数量", () => {
  const model = mapReviewFeedPage(
    page({ completedCount: 7, rememberedCount: 5, forgottenCount: 2 }),
    NOW,
  );
  assert.equal(model.completedCount, 7);
  assert.throws(
    () => mapReviewFeedPage(page({ completedCount: 7, rememberedCount: 5, forgottenCount: 1 }), NOW),
    /完成统计与结果分类不一致/,
  );
});

test("Feed 页面追加去重并持续推进 cursor", () => {
  const first = mapReviewFeedPage(page(), NOW);
  const second = mapReviewFeedPage(
    page({
      items: [
        feedItem({ id: 32, ordinal: 1, learningRecord: learningRecord(12), target: target(12) }),
      ],
      nextCursor: 1,
    }),
    NOW,
  );
  const merged = appendReviewFeedPage(first, second);
  assert.deepEqual(merged.cards.map((card) => card.feedItemId), [31, 32]);
  assert.equal(merged.nextCursor, 1);
});

test("迟到分页不会用较旧 target revision 覆盖同目标已加载卡片", () => {
  const first = mapReviewFeedPage(
    page({ items: [feedItem({ target: target(11, { revision: 3 }) })] }),
    NOW,
  );
  const staleNext = mapReviewFeedPage(
    page({
      items: [
        feedItem({ id: 32, ordinal: 1, cycleIndex: 1, target: target(11, { revision: 2 }) }),
      ],
      nextCursor: 1,
    }),
    NOW,
  );
  const merged = appendReviewFeedPage(first, staleNext);
  assert.deepEqual(merged.cards.map((card) => card.target.revision), [3, 3]);
});

test("结果写回更新同目标所有卡的 revision，只完成对应 Feed 条目，且可撤销", () => {
  const first = feedItem();
  const repeated = feedItem({ id: 32, ordinal: 1, cycleIndex: 1, reasonCode: "continuedPractice" });
  const initial = mapReviewFeedPage(page({ items: [first, repeated], nextCursor: 1 }), NOW);
  const written = applyReviewOutcomeResult(initial, 31, {
    target: target(11, { revision: 1, attemptCount: 1, rememberedCount: 1 }),
    attempt: attempt(),
    canContinue: false,
  });
  assert.equal(written.completedCount, 1);
  assert.equal(written.cards[0].attempt.id, 51);
  assert.equal(written.cards[1].attempt, undefined);
  assert.equal(written.cards[1].target.revision, 1);

  const undone = applyReviewOutcomeResult(written, 31, {
    target: target(11, { revision: 2 }),
    attempt: attempt({
      undoneAtUnixMs: DAY_START + 1_000,
      undoRequestKey: "review-undo:stable",
      undoTargetRevision: 2,
    }),
    canContinue: true,
  });
  assert.equal(undone.completedCount, 0);
  assert.equal(undone.cards[0].attempt, undefined);
  assert.equal(undone.cards[1].target.revision, 2);
});

test("卡片质量反馈与学习结果分离，并只更新具体 Feed 卡片", () => {
  const initial = mapReviewFeedPage(
    page({ items: [feedItem(), feedItem({ id: 32, ordinal: 1, cycleIndex: 1 })] }),
    NOW,
  );
  const feedback = {
    id: 71,
    feedItemId: 31,
    learningRecordId: 11,
    generatedCardId: null,
    revision: 0,
    active: true,
    polarity: "down",
    reasonCodes: ["unclear_prompt"],
    detail: "上下文不足",
    createdAtUnixMs: DAY_START + 200,
    updatedAtUnixMs: DAY_START + 200,
  };
  const next = applyReviewQualityFeedback(initial, feedback);
  assert.equal(next.cards[0].qualityFeedback.id, 71);
  assert.equal(next.cards[1].qualityFeedback, undefined);
  assert.equal(next.completedCount, 0);
  assert.deepEqual(next.cards[0].target, initial.cards[0].target);
});

test("赞踩动作立即形成卡片级空详情写入，详情保持可选", () => {
  const card = mapReviewFeedPage(page(), NOW).cards[0];
  const input = createImmediateReviewQualityFeedbackInput(
    card,
    "up",
    "review-quality:item-31",
  );
  assert.deepEqual(input, {
    feedItemId: 31,
    learningRecordId: 11,
    cardContextKey: "recorded",
    expectedRevision: undefined,
    polarity: "up",
    reasonCodes: [],
    requestKey: "review-quality:item-31",
  });

  const alreadySaved = {
    ...card,
    qualityFeedback: {
      id: 71,
      feedItemId: 31,
      learningRecordId: 11,
      generatedCardId: undefined,
      revision: 0,
      active: true,
      polarity: "up",
      reasonCodes: [],
      createdAtUnixMs: DAY_START,
      updatedAtUnixMs: DAY_START,
    },
  };
  assert.equal(
    createImmediateReviewQualityFeedbackInput(
      alreadySaved,
      "up",
      "unused-key",
    ),
    undefined,
  );
});

test("外部刷新后的条目最终由 SQLite 权威快照恢复 mutation 结果", () => {
  const staleExternalFeed = mapReviewFeedPage(page(), NOW);
  const authority = mapReviewFeedItemState({
    dayStartUnixMs: DAY_START,
    dayEndUnixMs: DAY_END,
    item: feedItem({
      target: target(11, {
        revision: 1,
        attemptCount: 1,
        rememberedCount: 1,
      }),
      attempt: attempt(),
    }),
    completedCount: 1,
    rememberedCount: 1,
    forgottenCount: 0,
    canContinue: false,
  });
  const reconciled = applyReviewFeedItemState(staleExternalFeed, authority);
  assert.equal(reconciled.cards[0].attempt.requestKey, "review-outcome:stable");
  assert.equal(reconciled.cards[0].target.revision, 1);
  assert.equal(reconciled.completedCount, 1);
  assert.equal(reconciled.canContinue, false);
});

test("外部 learning-record 刷新等待全部写回结束后再读取 SQLite 权威状态", () => {
  const gate = new ReviewAuthorityRefreshGate();
  const before = mapReviewFeedPage(
    page({ items: [feedItem({ attempt: undefined })] }),
    NOW,
  );
  const staleExternalPage = mapReviewFeedPage(
    page({ items: [feedItem({ attempt: undefined })] }),
    NOW,
  );
  const authoritative = mapReviewFeedItemState({
    dayStartUnixMs: DAY_START,
    dayEndUnixMs: DAY_END,
    item: feedItem({
      attempt: attempt({ requestKey: "outcome-in-flight" }),
      qualityFeedback: {
        id: 72,
        feedItemId: 31,
        learningRecordId: 11,
        generatedCardId: undefined,
        revision: 0,
        active: true,
        polarity: "down",
        reasonCodes: [],
        createdAtUnixMs: DAY_START,
        updatedAtUnixMs: DAY_START,
      },
      target: target(11, { revision: 1, attemptCount: 1 }),
    }),
    completedCount: 1,
    rememberedCount: 1,
    forgottenCount: 0,
    canContinue: true,
  });

  let visible = before;
  const outcomeWorking = true;
  assert.equal(gate.requestRefresh(outcomeWorking), false);
  if (gate.requestRefresh(outcomeWorking)) visible = staleExternalPage;
  assert.equal(visible.cards[0].attempt, undefined);
  assert.equal(gate.hasDeferredRefresh, true);

  const qualityStillWorking = true;
  assert.equal(gate.releaseDeferredRefresh(qualityStillWorking), false);
  assert.equal(gate.hasDeferredRefresh, true);
  assert.equal(gate.releaseDeferredRefresh(false), true);
  visible = applyReviewFeedItemState(visible, authoritative);

  assert.equal(visible.cards[0].attempt?.requestKey, "outcome-in-flight");
  assert.equal(visible.cards[0].qualityFeedback?.polarity, "down");
  assert.equal(visible.completedCount, 1);
  assert.equal(visible.canContinue, true);
  assert.equal(gate.hasDeferredRefresh, false);
});

test("页面、条目和稳定 request key 共同拒绝迟到结果", () => {
  const identity = { pageKey: 4, queueItemId: 31, requestKey: "stable-key" };
  assert.equal(isReviewMutationCurrent(4, 31, "stable-key", identity), true);
  assert.equal(isReviewMutationCurrent(5, 31, "stable-key", identity), false);
  assert.equal(isReviewMutationCurrent(4, 32, "stable-key", identity), false);
  assert.equal(isReviewMutationCurrent(4, 31, "new-key", identity), false);
});

test("service 原样提交 expectedRevision、提示使用和稳定 request key", async () => {
  let received;
  const result = {
    target: target(11, { revision: 1 }),
    attempt: attempt({ usedHint: true }),
    canContinue: false,
  };
  const service = new RepositoryReviewService(
    repository({
      submitOutcome: async (input) => {
        received = input;
        return result;
      },
    }),
  );
  const input = {
    feedItemId: 31,
    learningRecordId: 11,
    learningTargetId: 11,
    expectedRevision: 0,
    outcome: "remembered",
    usedHint: true,
    requestKey: "review-outcome:stable",
  };
  await service.submitOutcome(input);
  assert.deepEqual(received, input);
});

test("正式复习路径与浏览器 fixture 动态隔离且不使用 localStorage", async () => {
  for (const file of [
    "src/components/ReviewPage.tsx",
    "src/reviewBackgroundPreparation.ts",
    "src/reviewPreparationCoordinator.ts",
    "src/reviewQualitySaveQueue.ts",
    "src/reviewRepository.ts",
    "src/reviewService.ts",
    "src/reviewRequestIdentity.ts",
  ]) {
    const content = await readFile(file, "utf8");
    assert.doesNotMatch(content, /reviewFixtureService|localStorage/);
  }
  const fixture = await readFile("src/reviewFixtureService.ts", "utf8");
  assert.doesNotMatch(fixture, /localStorage|\binvoke\s*\(|TauriReviewRepository|\bfetch\s*\(/);

  const app = await readFile("src/App.tsx", "utf8");
  const repositorySource = await readFile("src/reviewRepository.ts", "utf8");
  const pageSource = await readFile("src/components/ReviewPage.tsx", "utf8");
  assert.match(app, /new RepositoryReviewService\(new TauriReviewRepository\(\)\)/);
  assert.match(app, /new ReviewBackgroundPreparationController\(/);
  assert.match(app, /!isTauriRuntime \|\| !reviewBackgroundPreparation/);
  assert.match(app, /reviewBackgroundPreparation\.warmFirstPage\(\)/);
  assert.match(app, /if \(isTauriRuntime\) \{\s*return;/);
  assert.match(app, /import\("\.\/reviewFixtureService"\)/);
  assert.match(repositorySource, /get_review_feed_page/);
  assert.match(repositorySource, /prepare_review_feed_card/);
  assert.doesNotMatch(pageSource, /\binvoke\s*\(/);
});

test("复习页通过应用级协调器保存/撤销质量反馈，不自行调用反馈 service", async () => {
  const app = await readFile("src/App.tsx", "utf8");
  const shell = await readFile("src/components/MainAppShell.tsx", "utf8");
  const page = await readFile("src/components/ReviewPage.tsx", "utf8");
  const coordinator = await readFile("src/reviewQualitySaveQueue.ts", "utf8");
  assert.match(app, /new ReviewQualityCoordinator\(reviewService,[\s\S]*?recordMutation/);
  assert.match(app, /label: "复习卡片质量反馈"[\s\S]*?reviewQualityCoordinator\.flush\(\)/);
  assert.match(app, /reviewQualityCoordinator=\{reviewQualityCoordinator\}/);
  assert.match(shell, /qualityCoordinator=\{reviewQualityCoordinator\}/);
  assert.match(page, /qualityCoordinator\?\.enqueueSave\(/);
  assert.match(page, /qualityCoordinator\.enqueueUndo\(/);
  assert.match(page, /qualityCoordinator\?\.retryFailure\(/);
  assert.match(page, /event\.type === "failed"[\s\S]*?reconcileFeedItemFromAuthority/);
  assert.doesNotMatch(page, /service\.saveQualityFeedback|service\.undoQualityFeedback/);
  assert.doesNotMatch(page, /new ReviewQualitySaveQueue|qualitySaveQueueRef/);
  assert.match(coordinator, /export class ReviewQualityCoordinator/);
});

test("页面采用轻量多列 Feed、浅遮罩和直接展开的完整内容，不保留旧翻面或阶段九伪承诺", async () => {
  const pageSource = await readFile("src/components/ReviewPage.tsx", "utf8");
  const styles = await readFile("src/styles/review-page.css", "utf8");
  assert.match(styles, /\.rr-review-feed\s*\{[\s\S]*?column-count:\s*3/);
  assert.match(styles, /var\(--rr-main-scrim\), transparent 83%/);
  assert.match(styles, /\.rr-review-focus-body\s*\{[\s\S]*?max-height:[\s\S]*?overflow-y:\s*auto/);
  assert.doesNotMatch(styles, /rotateY\(180deg\)|rr-review-flip|rr-review-answer-blank/);
  assert.match(pageSource, /className="rr-review-focus-body"/);
  assert.match(pageSource, /className="rr-review-answer-word"/);
  assert.match(pageSource, /usedHint:\s*true/);
  assert.match(pageSource, /"复习详情"/);
  assert.doesNotMatch(pageSource, /翻到背面|先在脑中补全或理解|给一点提示|主动回忆|hintVisible|setFlipped/);
  assert.doesNotMatch(styles, /min-height:\s*168px|min-height:\s*150px|height:\s*386px/);
  assert.doesNotMatch(pageSource, /今日上限|有限列表|今日已经看过|薄弱项/);
  assert.doesNotMatch(pageSource, /等待生成英文语境|打开后生成|正在生成英文练习语境|\bprepareCard\s*\(/);
  assert.match(pageSource, /长期记忆与个性化排序留到阶段九/);
  assert.match(pageSource, /在记忆页查看这个学习目标/);
  assert.match(pageSource, /用同一请求重试/);
  assert.match(pageSource, /loadingCursorRef\.current !== cursor/);
  assert.match(pageSource, /preparationCoordinator\?\.getSnapshot\(\)\.needsMoreCandidates/);
  assert.match(pageSource, /data-review-ordinal/);
  assert.match(pageSource, /preparationCoordinator\.syncFeed/);
  assert.doesNotMatch(pageSource, /rootMargin:\s*"1200px/);
  assert.doesNotMatch(pageSource, /nth-child|Math\.random/);
});
