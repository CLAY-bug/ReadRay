import assert from "node:assert/strict";
import test from "node:test";
import {
  REVIEW_PREPARATION_CONCURRENCY,
  REVIEW_PREPARATION_BUFFER_HIGH_WATER,
  ReviewPreparationCoordinator,
  createReviewPreparationRequestKey,
} from "../src/reviewPreparationCoordinator.ts";
import { ReviewBackgroundPreparationController } from "../src/reviewBackgroundPreparation.ts";
import {
  createReviewFeedbackDraft,
  ReviewQualityCoordinator,
} from "../src/reviewQualitySaveQueue.ts";
import { ReviewAuthorityRefreshGate } from "../src/reviewAuthorityRefresh.ts";
import { DesktopSaveCoordinator } from "../src/desktopLifecycle.ts";
import { applyPreparedReviewCard, visibleReviewCards } from "../src/reviewService.ts";

const DAY_START = new Date(2026, 7, 10).getTime();

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}

async function flushPromises() {
  await Promise.resolve();
  await new Promise((resolve) => setImmediate(resolve));
}

function savedQualityFeedback(input, overrides = {}) {
  return {
    id: overrides.id ?? 71,
    feedItemId: input.feedItemId,
    learningRecordId: input.learningRecordId,
    generatedCardId: overrides.generatedCardId ?? input.feedItemId,
    revision: overrides.revision ?? (input.expectedRevision ?? -1) + 1,
    active: overrides.active ?? true,
    polarity: overrides.polarity ?? input.polarity ?? "up",
    reasonCodes: overrides.reasonCodes ?? [...(input.reasonCodes ?? [])],
    detail: overrides.detail ?? input.detail ?? undefined,
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  };
}

function card(index, overrides = {}) {
  const learningRecordId = 100 + index;
  const learningTargetId = 500 + index;
  const feedItemId = 1_000 + index;
  const cycleIndex = overrides.cycleIndex ?? 0;
  return {
    feedItemId,
    ordinal: index,
    cycleIndex,
    learningRecordId,
    learningTargetId,
    reasonCode: "newRecord",
    reason: "测试记录",
    typeLabel: "单词语境",
    sourceLabel: "划词查询 · Code",
    sourceApp: "Code",
    sourceTypeLabel: "划词查询",
    sourceTime: "2026年8月10日 10:00",
    query: `word-${index}`,
    sourceExcerpt: `word-${index}`,
    promptKind: "meaning",
    promptBefore: "",
    promptAnswer: `word-${index}`,
    promptAfter: "",
    promptText: `word-${index}`,
    hint: "测试提示",
    answerTitle: "测试词义",
    answerDetail: "测试词义",
    answerNote: "",
    contextOrigin: "query",
    contextOriginLabel: "后台准备中",
    needsPreparation: true,
    density: "compact",
    visualVariant: "lexical",
    paperTone: "paper",
    learningRecord: {
      id: learningRecordId,
      learningTargetId,
      queryText: `word-${index}`,
      learningTargetText: `word-${index}`,
      queryDirection: "enToZh",
      normalizedText: `word-${index}`,
      queryType: "word",
      sourceType: "windows_uia",
      sourceApp: "Code.exe",
      contextText: "只有中文语境",
      explanationCard: {
        queryType: "word",
        sourceText: `word-${index}`,
        learningTargetText: `word-${index}`,
        headword: `word-${index}`,
        basicMeanings: ["测试词义"],
        phrases: [],
        nearMeanings: [],
        examples: [],
      },
      schemaVersion: 1,
      createdAtUnixMs: DAY_START - index - 1,
    },
    target: {
      learningTargetId,
      revision: 0,
      nextReviewAtUnixMs: DAY_START,
      attemptCount: 0,
      rememberedCount: 0,
      forgottenCount: 0,
      successStreak: 0,
    },
    ...overrides,
  };
}

function feed(cards, overrides = {}) {
  return {
    dayStartUnixMs: DAY_START,
    dayEndUnixMs: DAY_START + 24 * 60 * 60 * 1_000,
    dayLabel: "2026年8月10日星期一",
    cards,
    nextCursor: cards.at(-1)?.ordinal,
    canContinue: true,
    completedCount: 0,
    rememberedCount: 0,
    forgottenCount: 0,
    sourceCounts: [{ label: "Code", count: cards.length }],
    ...overrides,
  };
}

function identityFor(sourceCard, dayStartUnixMs = DAY_START) {
  return {
    dayStartUnixMs,
    feedItemId: sourceCard.feedItemId,
    learningRecordId: sourceCard.learningRecordId,
    learningTargetId: sourceCard.learningTargetId,
    cycleIndex: sourceCard.cycleIndex,
  };
}

function generatedFor(sourceCard) {
  return {
    id: 2_000 + sourceCard.feedItemId,
    learningRecordId: sourceCard.learningRecordId,
    learningTargetId: sourceCard.learningTargetId,
    variantIndex: sourceCard.cycleIndex,
    englishContext: `The team used ${sourceCard.query} in a complete English context.`,
    englishContextZh: `团队在完整英文语境中使用了 ${sourceCard.query}。`,
    hint: "测试提示",
    model: "test-model",
    createdAtUnixMs: DAY_START + sourceCard.ordinal,
  };
}

test("后台制卡 request key 由日期与条目身份稳定派生", () => {
  const identity = {
    dayStartUnixMs: DAY_START,
    feedItemId: 1_002,
    learningRecordId: 102,
    learningTargetId: 502,
    cycleIndex: 0,
  };
  const first = createReviewPreparationRequestKey(identity);
  const cloned = createReviewPreparationRequestKey(structuredClone(identity));
  assert.equal(first, `review-card:${DAY_START}:1002:502:102:0`);
  assert.equal(cloned, first);
  assert.notEqual(
    createReviewPreparationRequestKey({ ...identity, cycleIndex: 1 }),
    first,
  );
  assert.notEqual(
    createReviewPreparationRequestKey({ ...identity, feedItemId: 1_003 }),
    first,
  );
  assert.doesNotMatch(createReviewPreparationRequestKey.toString(), /randomUUID|Date\.now/);
});

test("重复调度只填充有限后台队列，并严格限制并发", async () => {
  const cards = Array.from({ length: 14 }, (_, index) => card(index));
  const pending = [];
  let active = 0;
  let maxActive = 0;
  const service = {
    prepareFeedCard(input) {
      const request = deferred();
      active += 1;
      maxActive = Math.max(maxActive, active);
      pending.push({ input, request });
      return request.promise.finally(() => {
        active -= 1;
      });
    },
  };
  const coordinator = new ReviewPreparationCoordinator(service);
  let currentFeed = feed(cards);
  coordinator.subscribe((event) => {
    if (event.type !== "prepared") return;
    currentFeed = applyPreparedReviewCard(
      currentFeed,
      event.task.feedItemId,
      event.generatedCard,
    );
    coordinator.syncFeed(currentFeed);
  });

  coordinator.syncFeed(currentFeed);
  coordinator.syncFeed(structuredClone(currentFeed));

  assert.equal(pending.length, REVIEW_PREPARATION_CONCURRENCY);
  assert.deepEqual(coordinator.getSnapshot(), {
    queuedCount: REVIEW_PREPARATION_BUFFER_HIGH_WATER - REVIEW_PREPARATION_CONCURRENCY,
    workingCount: REVIEW_PREPARATION_CONCURRENCY,
    readyAheadCount: 0,
    bufferedCount: REVIEW_PREPARATION_BUFFER_HIGH_WATER,
    failedFeedItemIds: [],
    needsMoreCandidates: false,
  });

  let completed = 0;
  while (completed < REVIEW_PREPARATION_BUFFER_HIGH_WATER) {
    const running = pending[completed];
    const sourceCard = cards.find(
      (candidate) => candidate.feedItemId === running.input.feedItemId,
    );
    running.request.resolve(generatedFor(sourceCard));
    completed += 1;
    await flushPromises();
  }

  assert.equal(pending.length, REVIEW_PREPARATION_BUFFER_HIGH_WATER);
  assert.equal(maxActive, REVIEW_PREPARATION_CONCURRENCY);
  assert.equal(coordinator.getSnapshot().workingCount, 0);
  assert.equal(coordinator.getSnapshot().queuedCount, 0);
  assert.equal(coordinator.getSnapshot().readyAheadCount, REVIEW_PREPARATION_BUFFER_HIGH_WATER);
  assert.equal(coordinator.getSnapshot().bufferedCount, REVIEW_PREPARATION_BUFFER_HIGH_WATER);
});

test("Ready 恰好 6 张时不补充，消费到 5 张后一次补到 12", async () => {
  const readyCards = Array.from({ length: 6 }, (_, index) =>
    card(index, { needsPreparation: false }),
  );
  const pendingCards = Array.from({ length: 7 }, (_, index) => card(index + 6));
  let currentFeed = feed([...readyCards, ...pendingCards]);
  const calls = [];
  const coordinator = new ReviewPreparationCoordinator({
    prepareFeedCard(input) {
      const request = deferred();
      calls.push({ input, request });
      return request.promise;
    },
  });
  coordinator.subscribe((event) => {
    if (event.type !== "prepared") return;
    currentFeed = applyPreparedReviewCard(
      currentFeed,
      event.task.feedItemId,
      event.generatedCard,
    );
    coordinator.syncFeed(currentFeed);
  });

  coordinator.syncFeed(currentFeed);
  assert.equal(calls.length, 0);
  assert.equal(coordinator.getSnapshot().bufferedCount, 6);

  coordinator.markConsumed(currentFeed, readyCards[0].feedItemId);
  assert.equal(calls.length, REVIEW_PREPARATION_CONCURRENCY);
  assert.equal(coordinator.getSnapshot().bufferedCount, 12);
  for (let index = 0; index < pendingCards.length; index += 1) {
    const running = calls[index];
    const sourceCard = pendingCards.find(
      (candidate) => candidate.feedItemId === running.input.feedItemId,
    );
    running.request.resolve(generatedFor(sourceCard));
    await flushPromises();
  }
  assert.equal(calls.length, 7);
  assert.equal(coordinator.getSnapshot().bufferedCount, 12);
});

test("后面的可见卡不会越过并丢弃更早的 pending 或失败卡", async () => {
  const earlierPending = card(0);
  const laterReady = card(1, { needsPreparation: false });
  const calls = [];
  const coordinator = new ReviewPreparationCoordinator({
    prepareFeedCard(input) {
      const request = deferred();
      calls.push({ input, request });
      return request.promise;
    },
  });
  const currentFeed = feed([earlierPending, laterReady], { canContinue: false });

  coordinator.syncFeed(currentFeed);
  assert.equal(calls.length, 1);
  coordinator.markConsumed(currentFeed, laterReady.feedItemId);
  calls[0].request.reject(new Error("temporary failure"));
  await flushPromises();

  assert.deepEqual(
    coordinator.getSnapshot().failedFeedItemIds,
    [earlierPending.feedItemId],
  );
  coordinator.retryFailed(currentFeed);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].input.requestKey, calls[0].input.requestKey);
});

test("候选不足时只请求补页，Feed 结束或全部失败时停止自动扩张", async () => {
  const readyCards = Array.from({ length: 5 }, (_, index) =>
    card(index, { needsPreparation: false }),
  );
  const coordinator = new ReviewPreparationCoordinator({
    async prepareFeedCard() {
      throw new Error("不应调用制卡");
    },
  });

  coordinator.syncFeed(feed(readyCards));
  assert.equal(coordinator.getSnapshot().needsMoreCandidates, true);

  coordinator.syncFeed(feed(readyCards, { canContinue: false }));
  assert.equal(coordinator.getSnapshot().needsMoreCandidates, false);

  const failedCard = card(6);
  const failingCoordinator = new ReviewPreparationCoordinator({
    async prepareFeedCard() {
      throw new Error("model unavailable");
    },
  });
  failingCoordinator.syncFeed(feed([failedCard]));
  await flushPromises();
  assert.deepEqual(
    failingCoordinator.getSnapshot().failedFeedItemIds,
    [failedCard.feedItemId],
  );
  assert.equal(failingCoordinator.getSnapshot().needsMoreCandidates, false);
});

test("并发制卡乱序完成时两张结果都能合并到最新 Feed", async () => {
  const cards = [card(0), card(1)];
  let currentFeed = feed(cards);
  const requests = new Map();
  const service = {
    prepareFeedCard(input) {
      const request = deferred();
      requests.set(input.feedItemId, request);
      return request.promise;
    },
  };
  const coordinator = new ReviewPreparationCoordinator(service);
  const preparedOrder = [];
  coordinator.subscribe((event) => {
    if (event.type !== "prepared") return;
    preparedOrder.push(event.task.feedItemId);
    currentFeed = applyPreparedReviewCard(
      currentFeed,
      event.task.feedItemId,
      event.generatedCard,
    );
    coordinator.syncFeed(currentFeed);
  });

  coordinator.syncFeed(currentFeed);
  requests.get(cards[1].feedItemId).resolve(generatedFor(cards[1]));
  await flushPromises();
  requests.get(cards[0].feedItemId).resolve(generatedFor(cards[0]));
  await flushPromises();

  assert.deepEqual(preparedOrder, [cards[1].feedItemId, cards[0].feedItemId]);
  assert.deepEqual(
    visibleReviewCards(currentFeed).map((candidate) => candidate.feedItemId),
    cards.map((candidate) => candidate.feedItemId),
  );
  assert.equal(currentFeed.cards.some((candidate) => candidate.needsPreparation), false);
});

test("生成结果身份不匹配时隔离为失败而不发布", async () => {
  const sourceCard = card(0);
  const coordinator = new ReviewPreparationCoordinator({
    async prepareFeedCard() {
      return generatedFor({ ...sourceCard, learningRecordId: 999 });
    },
  });
  let preparedEventCount = 0;
  coordinator.subscribe((event) => {
    if (event.type === "prepared") preparedEventCount += 1;
  });

  coordinator.syncFeed(feed([sourceCard]));
  await flushPromises();

  assert.equal(preparedEventCount, 0);
  assert.deepEqual(coordinator.getSnapshot().failedFeedItemIds, [sourceCard.feedItemId]);
  assert.equal(coordinator.getPreparedCard(identityFor(sourceCard)), undefined);
});

test("跨日切换使用完整身份，旧任务完成不会进入新页面缓存", async () => {
  const oldCard = card(0);
  const newCard = card(1, { feedItemId: oldCard.feedItemId, ordinal: 0 });
  const nextDayStart = DAY_START + 24 * 60 * 60 * 1_000;
  const calls = [];
  const preparedEvents = [];
  const coordinator = new ReviewPreparationCoordinator({
    prepareFeedCard(input) {
      const request = deferred();
      calls.push({ input, request });
      return request.promise;
    },
  });
  coordinator.subscribe((event) => {
    if (event.type === "prepared") preparedEvents.push(event.task.dayStartUnixMs);
  });

  coordinator.syncFeed(feed([oldCard]));
  coordinator.syncFeed(
    feed([newCard], {
      dayStartUnixMs: nextDayStart,
      dayEndUnixMs: nextDayStart + 24 * 60 * 60 * 1_000,
    }),
  );
  assert.equal(calls.length, 2);

  calls[0].request.resolve(generatedFor(oldCard));
  await flushPromises();
  assert.deepEqual(preparedEvents, []);
  assert.equal(coordinator.getPreparedCard(identityFor(oldCard)), undefined);

  const newGenerated = generatedFor(newCard);
  calls[1].request.resolve(newGenerated);
  await flushPromises();
  assert.deepEqual(preparedEvents, [nextDayStart]);
  assert.equal(
    coordinator.getPreparedCard(identityFor(newCard, nextDayStart))?.learningRecordId,
    newCard.learningRecordId,
  );
});

test("单项失败释放并发槽，显式重试复用原 request key", async () => {
  const cards = [card(0), card(1), card(2)];
  const calls = [];
  const service = {
    prepareFeedCard(input) {
      const request = deferred();
      calls.push({ input, request });
      return request.promise;
    },
  };
  const coordinator = new ReviewPreparationCoordinator(service);
  const currentFeed = feed(cards);
  coordinator.syncFeed(currentFeed);

  const failedKey = calls[0].input.requestKey;
  calls[0].request.reject(new Error("temporary failure"));
  await flushPromises();
  assert.equal(calls.length, 3);
  assert.deepEqual(coordinator.getSnapshot().failedFeedItemIds, [cards[0].feedItemId]);

  calls[1].request.resolve(generatedFor(cards[1]));
  calls[2].request.resolve(generatedFor(cards[2]));
  await flushPromises();
  assert.equal(calls.length, 3, "失败项不会在同一轮形成热重试");

  coordinator.retryFailed(currentFeed);
  assert.equal(calls.length, 4);
  assert.equal(calls[3].input.feedItemId, cards[0].feedItemId);
  assert.equal(calls[3].input.requestKey, failedKey);
  calls[3].request.resolve(generatedFor(cards[0]));
  await flushPromises();
  assert.deepEqual(coordinator.getSnapshot().failedFeedItemIds, []);
});

test("已经持久化完成的卡片不会再次进入后台生成，dispose 后不再派发 UI 事件", async () => {
  const ready = card(0, { needsPreparation: false });
  const pendingCard = card(1);
  const request = deferred();
  let callCount = 0;
  let preparedEventCount = 0;
  const coordinator = new ReviewPreparationCoordinator({
    prepareFeedCard() {
      callCount += 1;
      return request.promise;
    },
  });
  coordinator.subscribe((event) => {
    if (event.type === "prepared") preparedEventCount += 1;
  });

  coordinator.syncFeed(feed([ready]));
  assert.equal(callCount, 0);
  coordinator.syncFeed(feed([ready, pendingCard]));
  assert.equal(callCount, 1);
  coordinator.dispose();
  request.resolve(generatedFor(pendingCard));
  await flushPromises();
  assert.equal(preparedEventCount, 0);
  assert.equal(coordinator.getPreparedCard(identityFor(pendingCard)), undefined);
});

test("主应用未打开复习页时会预热首屏并填充同一个持久化协调器", async () => {
  const cards = Array.from({ length: 12 }, (_, index) => card(index));
  const preparedInputs = [];
  const service = {
    async loadFeedPage() {
      return feed(cards);
    },
    async prepareFeedCard(input) {
      preparedInputs.push(input);
      const sourceCard = cards.find(
        (candidate) => candidate.feedItemId === input.feedItemId,
      );
      return generatedFor(sourceCard);
    },
  };
  const coordinator = new ReviewPreparationCoordinator(service);
  const background = new ReviewBackgroundPreparationController(
    service,
    coordinator,
  );

  assert.equal(coordinator.hasPageConsumer(), false);
  assert.equal(await background.warmFirstPage(), true);
  await flushPromises();

  assert.equal(preparedInputs.length, REVIEW_PREPARATION_BUFFER_HIGH_WATER);
  assert.ok(coordinator.getPreparedCard(identityFor(cards[0])));
  assert.equal(
    coordinator.getSnapshot().readyAheadCount,
    REVIEW_PREPARATION_BUFFER_HIGH_WATER,
  );
});

test("复习页在预热读取完成前接管时拒绝迟到首屏，避免覆盖页面 Feed", async () => {
  const firstPage = deferred();
  let prepareCount = 0;
  const service = {
    loadFeedPage() {
      return firstPage.promise;
    },
    async prepareFeedCard() {
      prepareCount += 1;
      throw new Error("页面接管后不应由迟到预热触发制卡");
    },
  };
  const coordinator = new ReviewPreparationCoordinator(service);
  const background = new ReviewBackgroundPreparationController(
    service,
    coordinator,
  );
  const warming = background.warmFirstPage();
  const detachPageConsumer = coordinator.attachPageConsumer();

  firstPage.resolve(feed([card(0)]));
  assert.equal(await warming, false);
  await flushPromises();
  assert.equal(prepareCount, 0);
  assert.equal(coordinator.getSnapshot().bufferedCount, 0);

  detachPageConsumer();
});

test("较新的学习记录刷新会使较旧预热结果失效", async () => {
  const firstPage = deferred();
  let prepareCount = 0;
  const service = {
    loadFeedPage() {
      return firstPage.promise;
    },
    async prepareFeedCard() {
      prepareCount += 1;
      throw new Error("失效预热不应继续制卡");
    },
  };
  const coordinator = new ReviewPreparationCoordinator(service);
  const background = new ReviewBackgroundPreparationController(
    service,
    coordinator,
  );
  const warming = background.warmFirstPage();
  background.invalidate();
  firstPage.resolve(feed([card(0)]));

  assert.equal(await warming, false);
  await flushPromises();
  assert.equal(prepareCount, 0);
});

test("重启后持久化退避阻止自动制卡，显式重试仍复用稳定 key", () => {
  const sourceCard = card(0);
  const requestKey = createReviewPreparationRequestKey(identityFor(sourceCard));
  const backedOffCard = {
    ...sourceCard,
    generationFailure: {
      requestKey,
      feedItemId: sourceCard.feedItemId,
      learningRecordId: sourceCard.learningRecordId,
      failureCount: 3,
      retryAfterUnixMs: DAY_START + 60_000,
      lastError: "model unavailable",
      createdAtUnixMs: DAY_START - 1_000,
      updatedAtUnixMs: DAY_START,
    },
  };
  const calls = [];
  const coordinator = new ReviewPreparationCoordinator(
    {
      prepareFeedCard(input) {
        calls.push(input);
        return new Promise(() => {});
      },
    },
    () => DAY_START + 1,
  );
  const restoredFeed = feed([backedOffCard], { canContinue: false });

  coordinator.syncFeed(restoredFeed);
  assert.equal(calls.length, 0);
  assert.deepEqual(coordinator.getSnapshot().failedFeedItemIds, [sourceCard.feedItemId]);

  coordinator.retryFailed(restoredFeed);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].requestKey, requestKey);
  assert.equal(calls[0].explicitRetry, true);
});

test("快速连续切换赞踩会串行写入并以最后一次选择及最新 revision 为准", async () => {
  const calls = [];
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState() {
      return { card: { qualityFeedback: undefined } };
    },
    async saveQualityFeedback(input) {
      calls.push(input);
      return {
        id: 71,
        feedItemId: input.feedItemId,
        learningRecordId: input.learningRecordId,
        generatedCardId: 501,
        revision: 0,
        active: true,
        polarity: input.polarity,
        reasonCodes: [...input.reasonCodes],
        detail: input.detail ?? undefined,
        createdAtUnixMs: DAY_START,
        updatedAtUnixMs: DAY_START,
      };
    },
    async undoQualityFeedback() {
      throw new Error("unused");
    },
  });
  const base = {
    feedItemId: 1_001,
    learningRecordId: 101,
    cardContextKey: "generated:501",
    reasonCodes: [],
  };
  coordinator.enqueueSave({ ...base, polarity: "up", requestKey: "quality-up" });
  coordinator.enqueueSave({ ...base, polarity: "down", requestKey: "quality-down" });
  await flushPromises();
  await flushPromises();

  assert.equal(calls.length, 1, "同一卡片语境的连续选择只保留最后一次");
  assert.equal(calls[0].polarity, "down");
  assert.equal(calls[0].requestKey, "quality-down");
  assert.equal(calls[0].expectedRevision, undefined);
});

test("反馈队列在 A 写回期间保留 B 与 C 两张具体卡片的独立意图", async () => {
  const calls = [];
  const request = deferred();
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState() {
      return { card: { qualityFeedback: undefined } };
    },
    saveQualityFeedback(input) {
      calls.push(input);
      return request.promise;
    },
    async undoQualityFeedback() {
      throw new Error("unused");
    },
  });
  const intent = (feedItemId, cardContextKey, polarity, requestKey) => ({
    feedItemId,
    learningRecordId: feedItemId - 900,
    cardContextKey,
    polarity,
    reasonCodes: [],
    requestKey,
  });
  coordinator.enqueueSave(intent(1_001, "generated:501", "up", "quality-a"));
  coordinator.enqueueSave(intent(1_002, "generated:502", "down", "quality-b"));
  coordinator.enqueueSave(intent(1_003, "recorded", "up", "quality-c"));
  await flushPromises();
  await flushPromises();

  assert.equal(calls.length, 1, "串行写回，A 未完成时 B/C 不得并发");
  assert.equal(calls[0].requestKey, "quality-a");
  request.resolve({
    id: 71,
    feedItemId: 1_001,
    learningRecordId: 101,
    generatedCardId: 501,
    revision: 0,
    active: true,
    polarity: "up",
    reasonCodes: [],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  await flushPromises();
  await flushPromises();

  assert.deepEqual(
    calls.map((input) => input.requestKey),
    ["quality-a", "quality-b", "quality-c"],
    "不同卡片的意图全部按进入顺序保留并持久化",
  );
});

test("反馈协调器只合并同一卡片语境的连续选择且保留其队列位置", async () => {
  const calls = [];
  const requests = new Map();
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState(feedItemId) {
      if (feedItemId === 1_002) {
        return {
          card: {
            qualityFeedback: {
              id: 72,
              feedItemId: 1_002,
              learningRecordId: 102,
              generatedCardId: 502,
              revision: 4,
              active: true,
              polarity: "up",
              reasonCodes: [],
              createdAtUnixMs: DAY_START,
              updatedAtUnixMs: DAY_START,
            },
          },
        };
      }
      return { card: { qualityFeedback: undefined } };
    },
    saveQualityFeedback(input) {
      const request = deferred();
      requests.set(input.requestKey, request);
      calls.push(input);
      return request.promise;
    },
    async undoQualityFeedback() {
      throw new Error("unused");
    },
  });
  const baseA = {
    feedItemId: 1_001,
    learningRecordId: 101,
    cardContextKey: "generated:501",
    reasonCodes: [],
  };
  const baseB = {
    feedItemId: 1_002,
    learningRecordId: 102,
    cardContextKey: "generated:502",
    reasonCodes: [],
  };
  coordinator.enqueueSave({ ...baseA, polarity: "up", requestKey: "quality-a" });
  coordinator.enqueueSave({ ...baseB, polarity: "up", requestKey: "quality-b-up" });
  coordinator.enqueueSave({ ...baseB, polarity: "down", requestKey: "quality-b-down" });
  coordinator.enqueueSave({
    feedItemId: 1_003,
    learningRecordId: 103,
    cardContextKey: "recorded",
    polarity: "up",
    reasonCodes: [],
    requestKey: "quality-c",
  });
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 1);
  assert.equal(calls[0].requestKey, "quality-a");
  requests.get("quality-a").resolve({
    id: 71,
    feedItemId: 1_001,
    learningRecordId: 101,
    generatedCardId: 501,
    revision: 0,
    active: true,
    polarity: "up",
    reasonCodes: [],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  await flushPromises();
  await flushPromises();

  assert.equal(calls.length, 2);
  assert.equal(calls[1].requestKey, "quality-b-down", "同卡连续选择只保留最后一次");
  assert.equal(calls[1].expectedRevision, 4, "写回使用该卡最新 SQLite revision");
  requests.get("quality-b-down").resolve({
    id: 72,
    feedItemId: 1_002,
    learningRecordId: 102,
    generatedCardId: 502,
    revision: 5,
    active: true,
    polarity: "down",
    reasonCodes: [],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 3);
  assert.equal(calls[2].requestKey, "quality-c");
});

test("反馈失败只阻塞失败卡片的重试，不同卡片继续持久化", async () => {
  const calls = [];
  const requests = new Map();
  const terminalEvents = [];
  const gate = new ReviewAuthorityRefreshGate();
  let deferredRefreshReleased = 0;
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState(feedItemId) {
      return { card: { qualityFeedback: undefined } };
    },
    saveQualityFeedback(input) {
      const request = deferred();
      requests.set(input.requestKey, request);
      calls.push(input);
      return request.promise;
    },
    async undoQualityFeedback() {
      throw new Error("unused");
    },
  });
  coordinator.subscribe((event) => {
    if (event.type === "state") return;
    terminalEvents.push(event.type);
    if (gate.releaseDeferredRefresh(coordinator.hasWorkingMutation())) {
      deferredRefreshReleased += 1;
    }
  });
  const intent = (feedItemId, polarity, requestKey) => ({
    feedItemId,
    learningRecordId: feedItemId - 900,
    cardContextKey: "generated:" + feedItemId,
    polarity,
    reasonCodes: [],
    requestKey,
  });
  coordinator.enqueueSave(intent(1_001, "up", "quality-a"));
  coordinator.enqueueSave(intent(1_002, "down", "quality-b"));
  coordinator.enqueueSave(intent(1_003, "up", "quality-c"));
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 1);
  assert.equal(gate.requestRefresh(coordinator.hasWorkingMutation()), false);

  requests.get("quality-a").reject(new Error("network down"));
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 2, "A 失败不阻塞 B 继续保存");
  requests.get("quality-b").resolve({
    id: 81,
    feedItemId: 1_002,
    learningRecordId: 102,
    generatedCardId: 1_002,
    revision: 0,
    active: true,
    polarity: "down",
    reasonCodes: [],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 3, "B 完成后 C 继续保存");
  assert.equal(calls[2].requestKey, "quality-c");

  const failure = coordinator.getState().failures.find(
    (candidate) => candidate.feedItemId === 1_001,
  );
  assert.ok(failure, "A 的失败独立记录，不丢进任何卡片详情框");
  assert.equal(failure.intent.requestKey, "quality-a", "失败意图保留稳定 request key");
  requests.get("quality-c").resolve({
    id: 82,
    feedItemId: 1_003,
    learningRecordId: 103,
    generatedCardId: 1_003,
    revision: 0,
    active: true,
    polarity: "up",
    reasonCodes: [],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  await flushPromises();
  await flushPromises();
  coordinator.retryFailure(1_001, 101);
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 4);
  assert.equal(calls[3].requestKey, "quality-a", "显式重试复用原 request key");
  requests.get("quality-a").resolve({
    id: 71,
    feedItemId: 1_001,
    learningRecordId: 101,
    generatedCardId: 1_001,
    revision: 0,
    active: true,
    polarity: "up",
    reasonCodes: [],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  await flushPromises();
  await flushPromises();
  assert.equal(coordinator.getState().failures.length, 0);
  assert.equal(terminalEvents.filter((type) => type === "failed").length, 1);
  assert.equal(deferredRefreshReleased, 1, "最后一个终态释放页面外部刷新门禁");
  assert.equal(gate.hasDeferredRefresh, false);
});

test("写回失败对账期间仍被视为在途 mutation，外部刷新继续延后", async () => {
  const authorityRequest = deferred();
  let stateCalls = 0;
  const coordinator = new ReviewQualityCoordinator({
    loadFeedItemState() {
      stateCalls += 1;
      if (stateCalls === 1) {
        return Promise.resolve({ card: { qualityFeedback: undefined } });
      }
      return authorityRequest.promise;
    },
    async saveQualityFeedback() {
      throw new Error("IPC 未确认");
    },
    async undoQualityFeedback() {
      throw new Error("unused");
    },
  });
  coordinator.enqueueSave({
    feedItemId: 1_001,
    learningRecordId: 101,
    cardContextKey: "generated:501",
    polarity: "up",
    reasonCodes: [],
    requestKey: "quality-reconciling",
  });
  await flushPromises();
  await flushPromises();
  assert.equal(coordinator.hasWorkingMutation(), true, "权威对账完成前仍视为在途 mutation");
  authorityRequest.resolve({ card: { qualityFeedback: undefined } });
  await flushPromises();
  await flushPromises();
  assert.equal(coordinator.hasWorkingMutation(), false);
  assert.equal(
    coordinator.getState().failures.some(
      (candidate) => candidate.feedItemId === 1_001,
    ),
    true,
    "对账确认未提交后按真正失败记录",
  );
});

test("模糊成功先按 SQLite 权威反馈对账，不把已提交结果当失败", async () => {
  const calls = [];
  const reconcileRequest = deferred();
  const gate = new ReviewAuthorityRefreshGate();
  const finished = [];
  let cachedFeedback;
  let released = 0;
  let stateCalls = 0;
  const committed = {
    id: 71,
    feedItemId: 1_001,
    learningRecordId: 101,
    generatedCardId: 1_001,
    revision: 1,
    active: true,
    polarity: "down",
    reasonCodes: ["unclear_prompt"],
    detail: "语境不足",
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  };
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState() {
      stateCalls += 1;
      if (stateCalls === 1) return { card: { qualityFeedback: undefined } };
      return reconcileRequest.promise;
    },
    async saveQualityFeedback(input) {
      calls.push(input);
      throw new Error("IPC 未确认");
    },
    async undoQualityFeedback() {
      throw new Error("unused");
    },
  });
  coordinator.subscribe((event) => {
    if (event.type !== "finished") return;
    finished.push(event);
    cachedFeedback = event.result;
    if (gate.releaseDeferredRefresh(coordinator.hasWorkingMutation())) {
      released += 1;
    }
  });
  coordinator.enqueueSave({
    feedItemId: 1_001,
    learningRecordId: 101,
    cardContextKey: "generated:1001",
    polarity: "down",
    reasonCodes: ["unclear_prompt"],
    detail: "语境不足",
    requestKey: "quality-ambiguous",
  });
  await flushPromises();
  assert.equal(calls.length, 1, "请求已经进入后端写回");
  assert.equal(gate.requestRefresh(coordinator.hasWorkingMutation()), false);
  reconcileRequest.resolve({ card: { qualityFeedback: committed } });
  await flushPromises();

  assert.equal(coordinator.getState().failures.length, 0, "模糊成功必须对账为成功");
  assert.equal(coordinator.getState().working, undefined);
  assert.equal(calls.length, 1);
  assert.equal(finished.length, 1, "直接成功与模糊成功共用恰好一次 finished 收口");
  assert.deepEqual(cachedFeedback, committed, "页面缓存收到 SQLite 权威反馈");
  assert.equal(released, 1);
  assert.equal(gate.hasDeferredRefresh, false);
});

test("模糊 undo 首次对账失败后显式重试完整复用冻结输入", async () => {
  const calls = [];
  let stateCalls = 0;
  const activeFeedback = (revision) => ({
    id: 81,
    feedItemId: 1_002,
    learningRecordId: 102,
    generatedCardId: 502,
    revision,
    active: true,
    polarity: "down",
    reasonCodes: ["unclear_prompt"],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState() {
      stateCalls += 1;
      if (stateCalls === 1) {
        return { card: { qualityFeedback: activeFeedback(3) } };
      }
      if (stateCalls === 2) throw new Error("首次 SQLite 对账暂时失败");
      return { card: { qualityFeedback: activeFeedback(9) } };
    },
    async saveQualityFeedback() {
      throw new Error("unused");
    },
    async undoQualityFeedback(input) {
      calls.push(structuredClone(input));
      if (calls.length === 1) throw new Error("IPC 未确认");
      return savedQualityFeedback(input, {
        id: 81,
        generatedCardId: 502,
        revision: 4,
        active: false,
        polarity: "down",
        reasonCodes: ["unclear_prompt"],
      });
    },
  });

  coordinator.enqueueUndo({
    feedbackId: 81,
    feedItemId: 1_002,
    learningRecordId: 102,
    expectedRevision: 3,
    requestKey: "quality-undo-ambiguous",
  });
  await flushPromises();
  await flushPromises();
  assert.equal(coordinator.getState().failures.length, 1);

  assert.equal(coordinator.retryFailure(1_002, 102), true);
  await flushPromises();
  await flushPromises();

  assert.equal(calls.length, 2);
  assert.deepEqual(
    calls[1],
    calls[0],
    "requestKey、expectedRevision 与 payload 必须整体冻结",
  );
  assert.equal(coordinator.getState().failures.length, 0);
  assert.equal(coordinator.hasWorkingMutation(), false);
});

test("ReviewPage 卸载只退订页面监听，应用级协调器继续保存已点击意图", async () => {
  const calls = [];
  const finished = [];
  const request = deferred();
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState() {
      return { card: { qualityFeedback: undefined } };
    },
    saveQualityFeedback(input) {
      calls.push(input);
      return request.promise;
    },
    async undoQualityFeedback() {
      throw new Error("unused");
    },
  });
  const unsubscribePage = coordinator.subscribe((event) => {
    if (event.type === "finished") finished.push(event);
  });
  coordinator.enqueueSave({
    feedItemId: 1_001,
    learningRecordId: 101,
    cardContextKey: "generated:501",
    polarity: "down",
    reasonCodes: ["unclear_prompt"],
    detail: "页面关闭前点击",
    requestKey: "quality-a",
  });
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 1, "写回已在页面卸载前启动");

  unsubscribePage();
  request.resolve({
    id: 71,
    feedItemId: 1_001,
    learningRecordId: 101,
    generatedCardId: 501,
    revision: 0,
    active: true,
    polarity: "down",
    reasonCodes: ["unclear_prompt"],
    detail: "页面关闭前点击",
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  await flushPromises();
  await coordinator.flush();
  assert.equal(coordinator.getState().working, undefined, "页面卸载不终止应用级写回");
  assert.equal(coordinator.getState().failures.length, 0);
  assert.equal(finished.length, 0, "卸载页面不会接收迟到 UI 事件");
});

test("安全退出等待 A/B/C 全队列，失败阻止退出且退出后的新意图被拒绝", async () => {
  const saves = new DesktopSaveCoordinator();
  const calls = [];
  const requests = new Map();
  const coordinator = new ReviewQualityCoordinator(
    {
      async loadFeedItemState() {
        return { card: { qualityFeedback: undefined } };
      },
      saveQualityFeedback(input) {
        calls.push(structuredClone(input));
        const request = deferred();
        requests.set(input.requestKey, request);
        return request.promise;
      },
      async undoQualityFeedback() {
        throw new Error("unused");
      },
    },
    { recordMutation: () => saves.recordMutation() },
  );
  saves.register({ label: "复习卡片质量反馈", flush: () => coordinator.flush() });
  const intent = (feedItemId, requestKey) => ({
    feedItemId,
    learningRecordId: feedItemId - 900,
    cardContextKey: `generated:${feedItemId}`,
    polarity: "up",
    reasonCodes: [],
    requestKey,
  });
  coordinator.enqueueSave(intent(1_001, "quality-exit-a"));
  coordinator.enqueueSave(intent(1_002, "quality-exit-b"));
  coordinator.enqueueSave(intent(1_003, "quality-exit-c"));
  await flushPromises();

  saves.beginExit(41);
  let flushSettled = false;
  const flushing = saves.flushAll().finally(() => {
    flushSettled = true;
  });
  assert.equal(
    coordinator.enqueueSave(intent(1_004, "quality-exit-d")),
    false,
    "退出开始后拒绝新反馈意图",
  );
  assert.equal(flushSettled, false);

  for (const requestKey of ["quality-exit-a", "quality-exit-b", "quality-exit-c"]) {
    requests.get(requestKey).resolve(savedQualityFeedback(
      calls.find((input) => input.requestKey === requestKey),
    ));
    await flushPromises();
    if (requestKey !== "quality-exit-c") assert.equal(flushSettled, false);
  }
  await flushing;
  assert.deepEqual(
    calls.map((input) => input.requestKey),
    ["quality-exit-a", "quality-exit-b", "quality-exit-c"],
  );
  saves.endExit(41);

  const failingSaves = new DesktopSaveCoordinator();
  const failingCoordinator = new ReviewQualityCoordinator(
    {
      async loadFeedItemState() {
        return { card: { qualityFeedback: undefined } };
      },
      async saveQualityFeedback() {
        throw new Error("database is locked");
      },
      async undoQualityFeedback() {
        throw new Error("unused");
      },
    },
    { recordMutation: () => failingSaves.recordMutation() },
  );
  failingSaves.register({
    label: "复习卡片质量反馈",
    flush: () => failingCoordinator.flush(),
  });
  failingCoordinator.enqueueSave(intent(1_005, "quality-exit-failed"));
  failingSaves.beginExit(42);
  await assert.rejects(
    () => failingSaves.flushAll(),
    /复习卡片质量反馈.*卡片 1005.*database is locked.*重试后再退出/,
  );
  failingSaves.endExit(42);
});

test("协调器 close 拒绝新意图并等待已接受的写回结束", async () => {
  const request = deferred();
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState() {
      return { card: { qualityFeedback: undefined } };
    },
    saveQualityFeedback(input) {
      return request.promise.then(() => savedQualityFeedback(input));
    },
    async undoQualityFeedback() {
      throw new Error("unused");
    },
  });
  const intent = {
    feedItemId: 1_001,
    learningRecordId: 101,
    cardContextKey: "generated:1001",
    polarity: "up",
    reasonCodes: [],
    requestKey: "quality-close",
  };
  coordinator.enqueueSave(intent);
  await flushPromises();
  let closed = false;
  const closing = coordinator.close().then(() => {
    closed = true;
  });
  assert.equal(coordinator.enqueueSave({ ...intent, requestKey: "too-late" }), false);
  assert.equal(closed, false);
  request.resolve();
  await closing;
  assert.equal(closed, true);
});

test("其他卡写回期间点击撤销反馈会排队并在同一协调器串行执行", async () => {
  const calls = [];
  const requests = new Map();
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState() {
      return { card: { qualityFeedback: undefined } };
    },
    saveQualityFeedback(input) {
      const request = deferred();
      requests.set(input.requestKey, request);
      calls.push(input);
      return request.promise;
    },
    undoQualityFeedback(input) {
      const request = deferred();
      requests.set(input.requestKey, request);
      calls.push(input);
      return request.promise;
    },
  });
  coordinator.enqueueSave({
    feedItemId: 1_001,
    learningRecordId: 101,
    cardContextKey: "generated:501",
    polarity: "up",
    reasonCodes: [],
    requestKey: "quality-save-a",
  });
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 1);
  assert.equal(coordinator.getState().working.requestKey, "quality-save-a");

  coordinator.enqueueUndo({
    feedbackId: 81,
    feedItemId: 1_002,
    learningRecordId: 102,
    expectedRevision: 3,
    requestKey: "quality-undo-b",
  });
  await flushPromises();
  assert.equal(calls.length, 1, "撤销在 A 写回期间排队，不静默忽略也不并发");

  requests.get("quality-save-a").resolve({
    id: 71,
    feedItemId: 1_001,
    learningRecordId: 101,
    generatedCardId: 501,
    revision: 1,
    active: true,
    polarity: "up",
    reasonCodes: [],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 2);
  assert.equal(calls[1].requestKey, "quality-undo-b", "A 写回结束后撤销排队执行");
  requests.get("quality-undo-b").resolve({
    id: 81,
    feedItemId: 1_002,
    learningRecordId: 102,
    generatedCardId: 502,
    revision: 4,
    active: false,
    polarity: "up",
    reasonCodes: [],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  });
  await flushPromises();
  await flushPromises();
  assert.equal(coordinator.getState().working, undefined);
});

test("同卡写回期间再次点击同一张卡会合并到最后选择并保持稳定 request key 语义", async () => {
  const calls = [];
  const requests = new Map();
  let committed;
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState() {
      return { card: { qualityFeedback: committed } };
    },
    saveQualityFeedback(input) {
      const request = deferred();
      requests.set(input.requestKey, request);
      calls.push(input);
      return request.promise;
    },
    async undoQualityFeedback() {
      throw new Error("unused");
    },
  });
  const base = {
    feedItemId: 1_001,
    learningRecordId: 101,
    cardContextKey: "generated:501",
    reasonCodes: [],
  };
  coordinator.enqueueSave({ ...base, polarity: "up", requestKey: "quality-up" });
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 1);
  coordinator.enqueueSave({ ...base, polarity: "down", requestKey: "quality-down" });
  assert.equal(coordinator.hasQueuedIntentFor({ feedItemId: 1_001, learningRecordId: 101 }), true);

  committed = {
    id: 71,
    feedItemId: 1_001,
    learningRecordId: 101,
    generatedCardId: 501,
    revision: 0,
    active: true,
    polarity: "up",
    reasonCodes: [],
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  };
  requests.get("quality-up").resolve(committed);
  await flushPromises();
  await flushPromises();
  assert.equal(calls.length, 2);
  assert.equal(calls[1].requestKey, "quality-down", "同卡意图按最后选择执行");
  assert.equal(calls[1].expectedRevision, 0, "串行写回使用最新 revision");
});

test("同卡 save 在途时的 undo 意图保留最后操作并使用 save 后权威 revision", async () => {
  const calls = [];
  const requests = new Map();
  let committed;
  const coordinator = new ReviewQualityCoordinator({
    async loadFeedItemState() {
      return { card: { qualityFeedback: committed } };
    },
    saveQualityFeedback(input) {
      calls.push(structuredClone(input));
      const request = deferred();
      requests.set(input.requestKey, request);
      return request.promise;
    },
    undoQualityFeedback(input) {
      calls.push(structuredClone(input));
      return Promise.resolve(savedQualityFeedback(input, {
        id: 91,
        generatedCardId: 501,
        revision: 6,
        active: false,
      }));
    },
  });
  coordinator.enqueueSave({
    feedItemId: 1_001,
    learningRecordId: 101,
    cardContextKey: "generated:501",
    polarity: "down",
    reasonCodes: [],
    requestKey: "quality-save-before-undo",
  });
  await flushPromises();
  coordinator.enqueueUndo({
    feedbackId: 80,
    feedItemId: 1_001,
    learningRecordId: 101,
    expectedRevision: 4,
    requestKey: "quality-undo-after-save",
  });
  committed = savedQualityFeedback(calls[0], {
    id: 91,
    generatedCardId: 501,
    revision: 5,
    active: true,
    polarity: "down",
  });
  requests.get("quality-save-before-undo").resolve(committed);
  await flushPromises();
  await flushPromises();

  assert.deepEqual(
    calls.map((input) => input.requestKey),
    ["quality-save-before-undo", "quality-undo-after-save"],
  );
  assert.equal(calls[1].feedbackId, 91);
  assert.equal(calls[1].expectedRevision, 5);
  assert.equal(coordinator.hasWorkingMutation(), false);
});

test("切换反馈 polarity 时不会继承相反评价的原因或详情", () => {
  const existing = {
    id: 71,
    feedItemId: 1_001,
    learningRecordId: 101,
    generatedCardId: 501,
    revision: 2,
    active: true,
    polarity: "up",
    reasonCodes: ["helpful_context"],
    detail: "这个语境很自然",
    createdAtUnixMs: DAY_START,
    updatedAtUnixMs: DAY_START,
  };

  assert.deepEqual(createReviewFeedbackDraft(existing, "up"), {
    reasonCodes: ["helpful_context"],
    detail: "这个语境很自然",
  });
  assert.deepEqual(createReviewFeedbackDraft(existing, "down"), {
    reasonCodes: [],
    detail: "",
  });
});
