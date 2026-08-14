import type {
  ReviewCardModel,
  ReviewFeedModel,
  ReviewService,
} from "./reviewService";
import type { GeneratedReviewCard } from "./types/review";

export const REVIEW_PREPARATION_BUFFER_LOW_WATER = 6;
export const REVIEW_PREPARATION_BUFFER_HIGH_WATER = 12;
export const REVIEW_PREPARATION_CONCURRENCY = 2;

export type ReviewPreparationIdentity = {
  dayStartUnixMs: number;
  feedItemId: number;
  learningRecordId: number;
  learningTargetId: number;
  cycleIndex: number;
};

export function createReviewPreparationRequestKey(
  input: ReviewPreparationIdentity,
) {
  const values = [
    input.dayStartUnixMs,
    input.feedItemId,
    input.learningTargetId,
    input.learningRecordId,
    input.cycleIndex,
  ];
  if (!values.every((value) => Number.isSafeInteger(value) && value >= 0)) {
    throw new Error("后台制卡请求包含无效身份。");
  }
  return `review-card:${values.join(":")}`;
}

type ReviewPreparationTask = ReviewPreparationIdentity & {
  ordinal: number;
  requestKey: string;
  explicitRetry: boolean;
};

export type ReviewPreparationSnapshot = {
  queuedCount: number;
  workingCount: number;
  readyAheadCount: number;
  bufferedCount: number;
  failedFeedItemIds: number[];
  needsMoreCandidates: boolean;
};

export type ReviewPreparationEvent =
  | {
      type: "prepared";
      task: ReviewPreparationTask;
      generatedCard: GeneratedReviewCard;
    }
  | {
      type: "failed";
      task: ReviewPreparationTask;
      error: string;
    }
  | {
      type: "status";
      snapshot: ReviewPreparationSnapshot;
    };

type ReviewPreparationListener = (event: ReviewPreparationEvent) => void;

type BufferMetrics = ReviewPreparationSnapshot & {
  schedulableCards: ReviewCardModel[];
  failedAheadCount: number;
};

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function taskFromCard(dayStartUnixMs: number, card: ReviewCardModel) {
  const identity = {
    dayStartUnixMs,
    feedItemId: card.feedItemId,
    learningRecordId: card.learningRecordId,
    learningTargetId: card.learningTargetId,
    cycleIndex: card.cycleIndex,
  };
  return {
    ...identity,
    ordinal: card.ordinal,
    requestKey: createReviewPreparationRequestKey(identity),
    explicitRetry: false,
  } satisfies ReviewPreparationTask;
}

function identityForCard(dayStartUnixMs: number, card: ReviewCardModel) {
  return createReviewPreparationRequestKey({
    dayStartUnixMs,
    feedItemId: card.feedItemId,
    learningRecordId: card.learningRecordId,
    learningTargetId: card.learningTargetId,
    cycleIndex: card.cycleIndex,
  });
}

function emptySnapshot(): ReviewPreparationSnapshot {
  return {
    queuedCount: 0,
    workingCount: 0,
    readyAheadCount: 0,
    bufferedCount: 0,
    failedFeedItemIds: [],
    needsMoreCandidates: false,
  };
}

export class ReviewPreparationCoordinator {
  private readonly service: ReviewService;
  private readonly listeners = new Set<ReviewPreparationListener>();
  private readonly queuedKeys = new Set<string>();
  private readonly working = new Map<string, ReviewPreparationTask>();
  private readonly failed = new Map<
    string,
    { task: ReviewPreparationTask; error: string; persisted: boolean }
  >();
  private readonly preparedCards = new Map<string, GeneratedReviewCard>();
  private readonly queue: ReviewPreparationTask[] = [];
  private activeDayStartUnixMs: number | undefined;
  private activeFeed: ReviewFeedModel | undefined;
  private readonly consumedFeedItemIds = new Set<number>();
  private pageConsumerCount = 0;
  private refillActive = false;
  private disposed = false;
  private readonly now: () => number;

  constructor(service: ReviewService, now: () => number = Date.now) {
    this.service = service;
    this.now = now;
  }

  subscribe(listener: ReviewPreparationListener) {
    this.listeners.add(listener);
    listener({ type: "status", snapshot: this.getSnapshot() });
    return () => {
      this.listeners.delete(listener);
    };
  }

  attachPageConsumer() {
    if (this.disposed) return () => undefined;
    this.pageConsumerCount += 1;
    let attached = true;
    return () => {
      if (!attached) return;
      attached = false;
      this.pageConsumerCount = Math.max(0, this.pageConsumerCount - 1);
    };
  }

  hasPageConsumer() {
    return this.pageConsumerCount > 0;
  }

  getSnapshot(): ReviewPreparationSnapshot {
    if (!this.activeFeed) return emptySnapshot();
    const metrics = this.bufferMetrics();
    return {
      queuedCount: metrics.queuedCount,
      workingCount: metrics.workingCount,
      readyAheadCount: metrics.readyAheadCount,
      bufferedCount: metrics.bufferedCount,
      failedFeedItemIds: metrics.failedFeedItemIds,
      needsMoreCandidates: metrics.needsMoreCandidates,
    };
  }

  getPreparedCard(identity: ReviewPreparationIdentity) {
    return this.preparedCards.get(
      createReviewPreparationRequestKey(identity),
    );
  }

  syncFeed(feed: ReviewFeedModel) {
    if (this.disposed) return;
    if (this.activeDayStartUnixMs !== feed.dayStartUnixMs) {
      this.switchDay(feed.dayStartUnixMs);
    }
    this.activeFeed = feed;
    this.syncPersistentFailures(feed);
    this.reconcileBuffer();
  }

  markConsumed(feed: ReviewFeedModel, feedItemId: number) {
    if (this.disposed) return;
    this.syncFeed(feed);
    const card = feed.cards.find((candidate) => candidate.feedItemId === feedItemId);
    if (!card || card.needsPreparation) return;
    this.consumedFeedItemIds.add(feedItemId);
    this.reconcileBuffer();
  }

  retryFailed(feed: ReviewFeedModel) {
    if (this.disposed) return;
    this.syncFeed(feed);
    const metrics = this.bufferMetrics();
    let capacity = Math.max(
      0,
      REVIEW_PREPARATION_BUFFER_HIGH_WATER - metrics.bufferedCount,
    );
    const failures = [...this.failed.values()]
      .filter(
        ({ task }) =>
          task.dayStartUnixMs === this.activeDayStartUnixMs &&
          feed.cards.some((card) => card.feedItemId === task.feedItemId),
      )
      .sort((left, right) => left.task.ordinal - right.task.ordinal);
    for (const failure of failures) {
      if (capacity <= 0) break;
      this.failed.delete(failure.task.requestKey);
      this.queue.push({ ...failure.task, explicitRetry: true });
      this.queuedKeys.add(failure.task.requestKey);
      capacity -= 1;
    }
    this.refillActive = true;
    this.reconcileBuffer();
  }

  dispose() {
    this.disposed = true;
    this.queue.length = 0;
    this.queuedKeys.clear();
    this.listeners.clear();
    this.pageConsumerCount = 0;
  }

  private switchDay(dayStartUnixMs: number) {
    this.activeDayStartUnixMs = dayStartUnixMs;
    this.activeFeed = undefined;
    this.consumedFeedItemIds.clear();
    this.refillActive = false;
    this.queue.length = 0;
    this.queuedKeys.clear();
    this.failed.clear();
    this.preparedCards.clear();
  }

  private reconcileBuffer() {
    if (this.disposed || !this.activeFeed) return;
    this.pruneQueuedTasks();
    let metrics = this.bufferMetrics();
    if (metrics.bufferedCount < REVIEW_PREPARATION_BUFFER_LOW_WATER) {
      this.refillActive = true;
    } else if (
      metrics.bufferedCount >= REVIEW_PREPARATION_BUFFER_HIGH_WATER
    ) {
      this.refillActive = false;
    }

    if (this.refillActive) {
      let capacity = Math.max(
        0,
        REVIEW_PREPARATION_BUFFER_HIGH_WATER - metrics.bufferedCount,
      );
      for (const card of metrics.schedulableCards) {
        if (capacity <= 0) break;
        const task = taskFromCard(this.activeFeed.dayStartUnixMs, card);
        this.queue.push(task);
        this.queuedKeys.add(task.requestKey);
        capacity -= 1;
      }
      metrics = this.bufferMetrics();
      if (metrics.bufferedCount >= REVIEW_PREPARATION_BUFFER_HIGH_WATER) {
        this.refillActive = false;
      }
    }

    this.emitStatus();
    this.pump();
  }

  private syncPersistentFailures(feed: ReviewFeedModel) {
    for (const card of feed.cards) {
      if (!card.needsPreparation) continue;
      const task = taskFromCard(feed.dayStartUnixMs, card);
      const persistedFailure = card.generationFailure;
      const current = this.failed.get(task.requestKey);
      const isActivePersistedFailure = Boolean(
        persistedFailure &&
          persistedFailure.requestKey === task.requestKey &&
          persistedFailure.retryAfterUnixMs > this.now(),
      );
      if (
        isActivePersistedFailure &&
        !this.queuedKeys.has(task.requestKey) &&
        !this.working.has(task.requestKey) &&
        !this.preparedCards.has(task.requestKey)
      ) {
        this.failed.set(task.requestKey, {
          task,
          error: persistedFailure!.lastError,
          persisted: true,
        });
      } else if (!isActivePersistedFailure && current?.persisted) {
        this.failed.delete(task.requestKey);
      }
    }
  }

  private pruneQueuedTasks() {
    const feed = this.activeFeed;
    if (!feed) return;
    const pendingAheadKeys = new Set(
      feed.cards
        .filter(
          (card) => card.needsPreparation,
        )
        .map((card) => identityForCard(feed.dayStartUnixMs, card)),
    );
    for (let index = this.queue.length - 1; index >= 0; index -= 1) {
      const task = this.queue[index];
      if (
        task.dayStartUnixMs === feed.dayStartUnixMs &&
        pendingAheadKeys.has(task.requestKey)
      ) {
        continue;
      }
      this.queue.splice(index, 1);
      this.queuedKeys.delete(task.requestKey);
    }
  }

  private bufferMetrics(): BufferMetrics {
    const feed = this.activeFeed;
    if (!feed) return { ...emptySnapshot(), schedulableCards: [], failedAheadCount: 0 };
    const dayStartUnixMs = feed.dayStartUnixMs;
    const pendingAheadKeys = new Set<string>();
    let readyAheadCount = 0;
    for (const card of feed.cards) {
      const key = identityForCard(dayStartUnixMs, card);
      if (!card.needsPreparation || this.preparedCards.has(key)) {
        if (!this.consumedFeedItemIds.has(card.feedItemId)) {
          readyAheadCount += 1;
        }
      } else {
        pendingAheadKeys.add(key);
      }
    }

    const queuedCount = this.queue.filter(
      (task) =>
        task.dayStartUnixMs === dayStartUnixMs &&
        pendingAheadKeys.has(task.requestKey),
    ).length;
    const workingCount = [...this.working.values()].filter(
      (task) =>
        task.dayStartUnixMs === dayStartUnixMs &&
        pendingAheadKeys.has(task.requestKey),
    ).length;
    const failedAhead = [...this.failed.values()]
      .filter(
        ({ task }) =>
          task.dayStartUnixMs === dayStartUnixMs &&
          pendingAheadKeys.has(task.requestKey),
      )
      .sort((left, right) => left.task.ordinal - right.task.ordinal);
    const bufferedCount = readyAheadCount + queuedCount + workingCount;
    const schedulableCards = feed.cards
      .filter((card) => {
        if (!card.needsPreparation) {
          return false;
        }
        const key = identityForCard(dayStartUnixMs, card);
        return (
          !this.queuedKeys.has(key) &&
          !this.working.has(key) &&
          !this.failed.has(key) &&
          !this.preparedCards.has(key)
        );
      })
      .sort((left, right) => left.ordinal - right.ordinal);
    const blockedByTotalFailure =
      failedAhead.length > 0 &&
      readyAheadCount === 0 &&
      queuedCount + workingCount === 0;
    const needsMoreCandidates =
      this.refillActive &&
      bufferedCount < REVIEW_PREPARATION_BUFFER_HIGH_WATER &&
      schedulableCards.length === 0 &&
      feed.canContinue &&
      !blockedByTotalFailure;

    return {
      queuedCount,
      workingCount,
      readyAheadCount,
      bufferedCount,
      failedFeedItemIds: failedAhead.map(
        ({ task }) => task.feedItemId,
      ),
      needsMoreCandidates,
      schedulableCards,
      failedAheadCount: failedAhead.length,
    };
  }

  private pump() {
    if (this.disposed) return;
    while (
      this.working.size < REVIEW_PREPARATION_CONCURRENCY &&
      this.queue.length > 0
    ) {
      const task = this.queue.shift();
      if (!task) break;
      this.queuedKeys.delete(task.requestKey);
      this.working.set(task.requestKey, task);
      this.emitStatus();
      let request: Promise<GeneratedReviewCard>;
      try {
        request = this.service.prepareFeedCard({
          feedItemId: task.feedItemId,
          learningRecordId: task.learningRecordId,
          learningTargetId: task.learningTargetId,
          requestKey: task.requestKey,
          explicitRetry: task.explicitRetry,
        });
      } catch (error) {
        this.finishFailed(task, errorMessage(error));
        continue;
      }
      void request.then(
        (generatedCard) => this.finishPrepared(task, generatedCard),
        (error) => this.finishFailed(task, errorMessage(error)),
      );
    }
  }

  private finishPrepared(
    task: ReviewPreparationTask,
    generatedCard: GeneratedReviewCard,
  ) {
    this.working.delete(task.requestKey);
    if (
      generatedCard.learningRecordId !== task.learningRecordId ||
      generatedCard.learningTargetId !== task.learningTargetId
    ) {
      this.finishFailed(task, "后台生成卡片与当前 Feed 条目身份不一致。");
      return;
    }
    if (!this.disposed && task.dayStartUnixMs === this.activeDayStartUnixMs) {
      this.preparedCards.set(task.requestKey, generatedCard);
      this.failed.delete(task.requestKey);
      this.emit({ type: "prepared", task, generatedCard });
    }
    this.reconcileBuffer();
  }

  private finishFailed(task: ReviewPreparationTask, error: string) {
    this.working.delete(task.requestKey);
    if (!this.disposed && task.dayStartUnixMs === this.activeDayStartUnixMs) {
      this.failed.set(task.requestKey, { task, error, persisted: false });
      this.emit({ type: "failed", task, error });
    }
    this.reconcileBuffer();
  }

  private emitStatus() {
    this.emit({ type: "status", snapshot: this.getSnapshot() });
  }

  private emit(event: ReviewPreparationEvent) {
    for (const listener of this.listeners) listener(event);
  }
}
