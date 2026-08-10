import type { ReviewService } from "./reviewService";
import type {
  ReviewQualityFeedback,
  ReviewQualityPolarity,
  SaveReviewQualityFeedbackInput,
  UndoReviewQualityFeedbackInput,
} from "./types/review";

export type ReviewQualitySaveIntent = {
  feedItemId: number;
  learningRecordId: number;
  cardContextKey: string;
  polarity: ReviewQualityPolarity;
  reasonCodes: string[];
  detail?: string;
  requestKey: string;
};

export type ReviewQualityMutationInput =
  | SaveReviewQualityFeedbackInput
  | UndoReviewQualityFeedbackInput;

export type ReviewQualityMutationKind = "save" | "undo";

export type ReviewQualityMutationStart = {
  kind: ReviewQualityMutationKind;
  input: ReviewQualityMutationInput;
};

export type ReviewQualityFailure = {
  feedItemId: number;
  learningRecordId: number;
  kind: ReviewQualityMutationKind;
  intent: ReviewQualitySaveIntent | UndoReviewQualityFeedbackInput;
  start: ReviewQualityMutationStart;
  error: string;
};

export type ReviewQualityState = {
  working:
    | {
        kind: ReviewQualityMutationKind;
        input: ReviewQualityMutationInput;
        requestKey: string;
      }
    | undefined;
  failures: ReviewQualityFailure[];
};

export type ReviewQualityCoordinatorEvent =
  | { type: "state"; state: ReviewQualityState }
  | {
      type: "finished";
      start: ReviewQualityMutationStart;
      result: ReviewQualityFeedback;
    }
  | {
      type: "failed";
      start: ReviewQualityMutationStart;
      error: string;
    };

type ReviewQualityUserIntent = {
  kind: ReviewQualityMutationKind;
  intent: ReviewQualitySaveIntent | UndoReviewQualityFeedbackInput;
};

type ReviewQualityPendingWork =
  | { type: "intent"; value: ReviewQualityUserIntent }
  | { type: "retry"; start: ReviewQualityMutationStart };

type ReviewQualityActiveMutation = {
  phase: "sending" | "reconciling";
  start: ReviewQualityMutationStart;
  intent: ReviewQualitySaveIntent | UndoReviewQualityFeedbackInput;
};

type ReviewQualityCoordinatorOptions = {
  recordMutation?: () => boolean;
};

function identityOf(input: { feedItemId: number; learningRecordId: number }) {
  return `${input.feedItemId}:${input.learningRecordId}`;
}

function cloneSaveIntent(intent: ReviewQualitySaveIntent): ReviewQualitySaveIntent {
  return { ...intent, reasonCodes: [...intent.reasonCodes] };
}

function cloneMutationInput(
  input: ReviewQualityMutationInput,
): ReviewQualityMutationInput {
  return "cardContextKey" in input
    ? { ...input, reasonCodes: [...input.reasonCodes] }
    : { ...input };
}

function cloneStart(start: ReviewQualityMutationStart): ReviewQualityMutationStart {
  return { kind: start.kind, input: cloneMutationInput(start.input) };
}

export function createReviewFeedbackDraft(
  feedback: ReviewQualityFeedback | undefined,
  polarity: ReviewQualityPolarity,
) {
  const restoresExisting = Boolean(
    feedback?.active && feedback.polarity === polarity,
  );
  return {
    reasonCodes: restoresExisting ? [...feedback!.reasonCodes] : [],
    detail: restoresExisting ? feedback!.detail ?? "" : "",
  };
}

/**
 * 应用级卡片质量反馈异步状态机。用户尚未发出的同卡意图可以合并；一旦发送，
 * requestKey、expectedRevision 与 payload 作为一个完整请求冻结。直接成功、
 * SQLite 对账确认的模糊成功和确定失败都从同一 active mutation 进入唯一终态。
 */
export class ReviewQualityCoordinator {
  private readonly service: ReviewService;
  private readonly recordMutation: () => boolean;
  private readonly listeners = new Set<(event: ReviewQualityCoordinatorEvent) => void>();
  private readonly pendingByIdentity = new Map<string, ReviewQualityPendingWork>();
  private readonly pendingOrder: string[] = [];
  private readonly latestFeedback = new Map<number, ReviewQualityFeedback>();
  private readonly failures = new Map<string, ReviewQualityFailure>();
  private readonly idleWaiters = new Set<() => void>();
  private preparingIdentity: string | undefined;
  private active: ReviewQualityActiveMutation | undefined;
  private accepting = true;
  private disposed = false;

  constructor(
    service: ReviewService,
    options: ReviewQualityCoordinatorOptions = {},
  ) {
    this.service = service;
    this.recordMutation = options.recordMutation ?? (() => true);
  }

  subscribe(listener: (event: ReviewQualityCoordinatorEvent) => void) {
    this.listeners.add(listener);
    listener({ type: "state", state: this.getState() });
    return () => {
      this.listeners.delete(listener);
    };
  }

  getState(): ReviewQualityState {
    return {
      working: this.active
        ? {
            kind: this.active.start.kind,
            input: cloneMutationInput(this.active.start.input),
            requestKey: this.active.start.input.requestKey,
          }
        : undefined,
      failures: [...this.failures.values()],
    };
  }

  hasWorkingMutation() {
    return this.hasPendingWork();
  }

  hasQueuedIntent() {
    return this.pendingByIdentity.size > 0;
  }

  hasQueuedIntentFor(input: { feedItemId: number; learningRecordId: number }) {
    return this.pendingByIdentity.has(identityOf(input));
  }

  enqueueSave(intent: ReviewQualitySaveIntent) {
    if (!this.acceptNewMutation()) return false;
    const cloned = cloneSaveIntent(intent);
    this.enqueueUserIntent({ kind: "save", intent: cloned });
    return true;
  }

  enqueueUndo(input: UndoReviewQualityFeedbackInput) {
    if (!this.acceptNewMutation()) return false;
    this.enqueueUserIntent({ kind: "undo", intent: { ...input } });
    return true;
  }

  updateFeedItemFeedback(
    feedItemId: number,
    feedback: ReviewQualityFeedback | undefined,
  ) {
    if (this.disposed) return;
    if (!feedback) {
      this.latestFeedback.delete(feedItemId);
      return;
    }
    const current = this.latestFeedback.get(feedItemId);
    if (!current || feedback.revision >= current.revision) {
      this.latestFeedback.set(feedItemId, feedback);
    }
  }

  retryFailure(feedItemId: number, learningRecordId: number) {
    const identity = `${feedItemId}:${learningRecordId}`;
    const failure = this.failures.get(identity);
    if (!failure || !this.acceptNewMutation()) return false;
    this.failures.delete(identity);
    this.enqueuePending(identity, {
      type: "retry",
      start: cloneStart(failure.start),
    });
    this.emitState();
    void this.pump();
    return true;
  }

  async flush() {
    while (this.hasPendingWork()) {
      await new Promise<void>((resolve) => this.idleWaiters.add(resolve));
    }
    if (this.failures.size > 0) {
      const details = [...this.failures.values()].map((failure) =>
        `卡片 ${failure.feedItemId}（学习记录 ${failure.learningRecordId}）${
          failure.kind === "save" ? "保存" : "撤销"
        }失败：${failure.error}`,
      );
      throw new Error(
        `仍有 ${this.failures.size} 条复习卡片质量反馈未解决。${details.join(
          "；",
        )}。请返回复习页重试后再退出。`,
      );
    }
  }

  async close() {
    this.accepting = false;
    await this.flush();
    this.disposed = true;
    this.latestFeedback.clear();
    this.listeners.clear();
  }

  private acceptNewMutation() {
    return this.accepting && !this.disposed && this.recordMutation();
  }

  private enqueueUserIntent(value: ReviewQualityUserIntent) {
    const identity = identityOf(value.intent);
    this.failures.delete(identity);
    this.enqueuePending(identity, { type: "intent", value });
    this.emitState();
    void this.pump();
  }

  private enqueuePending(identity: string, pending: ReviewQualityPendingWork) {
    if (!this.pendingByIdentity.has(identity)) {
      this.pendingOrder.push(identity);
    }
    this.pendingByIdentity.set(identity, pending);
  }

  private emitState() {
    for (const listener of this.listeners) {
      listener({ type: "state", state: this.getState() });
    }
  }

  private emitTerminal(
    event:
      | {
          type: "finished";
          start: ReviewQualityMutationStart;
          result: ReviewQualityFeedback;
        }
      | { type: "failed"; start: ReviewQualityMutationStart; error: string },
  ) {
    for (const listener of this.listeners) listener(event);
  }

  private hasPendingWork() {
    return Boolean(
      this.preparingIdentity || this.active || this.pendingByIdentity.size > 0,
    );
  }

  private resolveIdleWaiters() {
    if (this.hasPendingWork()) return;
    for (const resolve of this.idleWaiters) resolve();
    this.idleWaiters.clear();
  }

  private async pump() {
    if (
      this.disposed ||
      this.active ||
      this.preparingIdentity ||
      this.pendingOrder.length === 0
    ) {
      this.resolveIdleWaiters();
      return;
    }
    const identity = this.pendingOrder[0];
    const pending = this.pendingByIdentity.get(identity);
    if (!pending) {
      this.pendingOrder.shift();
      void this.pump();
      return;
    }
    this.preparingIdentity = identity;
    this.emitState();

    let authorityFeedback: ReviewQualityFeedback | undefined;
    try {
      const feedItemId =
        pending.type === "retry"
          ? pending.start.input.feedItemId
          : pending.value.intent.feedItemId;
      const state = await this.service.loadFeedItemState(feedItemId);
      authorityFeedback = state.card.qualityFeedback;
      this.updateFeedItemFeedback(feedItemId, authorityFeedback);
    } catch {
      // 预读失败不改写输入；后端 revision 与稳定 request key 继续兜底。
    }

    if (this.disposed) return;
    const latestPending = this.pendingByIdentity.get(identity);
    if (
      this.preparingIdentity !== identity ||
      this.pendingOrder[0] !== identity ||
      latestPending !== pending
    ) {
      this.preparingIdentity = undefined;
      this.emitState();
      void this.pump();
      return;
    }

    this.pendingByIdentity.delete(identity);
    this.pendingOrder.shift();
    const start =
      pending.type === "retry"
        ? cloneStart(pending.start)
        : this.freezeUserIntent(pending.value);
    const intent =
      pending.type === "retry"
        ? this.normalizeIntent(start.kind, start.input)
        : pending.value.intent;
    const operation: ReviewQualityActiveMutation = {
      phase: "sending",
      start,
      intent,
    };
    this.preparingIdentity = undefined;
    this.active = operation;

    if (
      authorityFeedback &&
      this.matchesInput(authorityFeedback, start.input)
    ) {
      this.settleSuccess(operation, authorityFeedback);
      return;
    }

    this.emitState();
    const request =
      start.kind === "save"
        ? this.service.saveQualityFeedback(
            start.input as SaveReviewQualityFeedbackInput,
          )
        : this.service.undoQualityFeedback(
            start.input as UndoReviewQualityFeedbackInput,
          );
    void request.then(
      (result) => this.settleSuccess(operation, result),
      (error) =>
        void this.reconcileFailure(
          operation,
          error instanceof Error ? error.message : String(error),
        ),
    );
  }

  private freezeUserIntent(
    pending: ReviewQualityUserIntent,
  ): ReviewQualityMutationStart {
    const input =
      pending.kind === "save"
        ? this.buildSaveInput(pending.intent as ReviewQualitySaveIntent)
        : this.buildUndoInput(
            pending.intent as UndoReviewQualityFeedbackInput,
          );
    return { kind: pending.kind, input: cloneMutationInput(input) };
  }

  private buildSaveInput(
    intent: ReviewQualitySaveIntent,
  ): SaveReviewQualityFeedbackInput {
    return {
      feedItemId: intent.feedItemId,
      learningRecordId: intent.learningRecordId,
      cardContextKey: intent.cardContextKey,
      expectedRevision: this.latestFeedback.get(intent.feedItemId)?.revision,
      polarity: intent.polarity,
      reasonCodes: [...intent.reasonCodes],
      detail: intent.detail,
      requestKey: intent.requestKey,
    };
  }

  private buildUndoInput(
    input: UndoReviewQualityFeedbackInput,
  ): UndoReviewQualityFeedbackInput {
    const feedback = this.latestFeedback.get(input.feedItemId);
    if (!feedback || !feedback.active) return { ...input };
    return {
      feedbackId: feedback.id,
      feedItemId: input.feedItemId,
      learningRecordId: input.learningRecordId,
      expectedRevision: feedback.revision,
      requestKey: input.requestKey,
    };
  }

  private settleSuccess(
    operation: ReviewQualityActiveMutation,
    result: ReviewQualityFeedback,
  ) {
    if (this.disposed || this.active !== operation) return;
    this.active = undefined;
    this.latestFeedback.set(result.feedItemId, result);
    this.failures.delete(identityOf(operation.start.input));
    this.emitState();
    this.emitTerminal({
      type: "finished",
      start: cloneStart(operation.start),
      result,
    });
    this.resolveIdleWaiters();
    void this.pump();
  }

  private async reconcileFailure(
    operation: ReviewQualityActiveMutation,
    error: string,
  ) {
    if (this.disposed || this.active !== operation) return;
    operation.phase = "reconciling";
    this.emitState();
    let committedResult: ReviewQualityFeedback | undefined;
    try {
      const state = await this.service.loadFeedItemState(
        operation.start.input.feedItemId,
      );
      const feedback = state.card.qualityFeedback;
      this.updateFeedItemFeedback(operation.start.input.feedItemId, feedback);
      if (this.matchesInput(feedback, operation.start.input)) {
        committedResult = feedback;
      }
    } catch {
      // 对账未知时保留完整冻结请求，显式重试只能复用同一输入。
    }
    if (this.disposed || this.active !== operation) return;
    if (committedResult) {
      this.settleSuccess(operation, committedResult);
      return;
    }
    this.settleFailure(operation, error);
  }

  private settleFailure(
    operation: ReviewQualityActiveMutation,
    error: string,
  ) {
    if (this.disposed || this.active !== operation) return;
    this.active = undefined;
    const identity = identityOf(operation.start.input);
    const superseded = this.pendingByIdentity.has(identity);
    if (!superseded) {
      this.failures.set(identity, {
        feedItemId: operation.start.input.feedItemId,
        learningRecordId: operation.start.input.learningRecordId,
        kind: operation.start.kind,
        intent:
          operation.start.kind === "save"
            ? cloneSaveIntent(operation.intent as ReviewQualitySaveIntent)
            : { ...(operation.intent as UndoReviewQualityFeedbackInput) },
        start: cloneStart(operation.start),
        error,
      });
    }
    this.emitState();
    this.emitTerminal({
      type: "failed",
      start: cloneStart(operation.start),
      error,
    });
    this.resolveIdleWaiters();
    void this.pump();
  }

  private matchesInput(
    feedback: ReviewQualityFeedback | undefined,
    input: ReviewQualityMutationInput,
  ) {
    if (
      !feedback ||
      feedback.feedItemId !== input.feedItemId ||
      feedback.learningRecordId !== input.learningRecordId
    ) {
      return false;
    }
    if ("cardContextKey" in input) {
      const cardContextMatches =
        input.cardContextKey === "recorded"
          ? feedback.generatedCardId == null
          : input.cardContextKey === `generated:${feedback.generatedCardId}`;
      return (
        cardContextMatches &&
        feedback.active &&
        feedback.polarity === input.polarity &&
        JSON.stringify(feedback.reasonCodes) ===
          JSON.stringify(input.reasonCodes) &&
        (feedback.detail ?? undefined) === (input.detail ?? undefined)
      );
    }
    return !feedback.active && feedback.revision >= input.expectedRevision + 1;
  }

  private normalizeIntent(
    kind: ReviewQualityMutationKind,
    input: ReviewQualityMutationInput,
  ): ReviewQualitySaveIntent | UndoReviewQualityFeedbackInput {
    if (kind === "undo") return { ...(input as UndoReviewQualityFeedbackInput) };
    const save = input as SaveReviewQualityFeedbackInput;
    return {
      feedItemId: save.feedItemId,
      learningRecordId: save.learningRecordId,
      cardContextKey: save.cardContextKey,
      polarity: save.polarity,
      reasonCodes: [...save.reasonCodes],
      detail: save.detail ?? undefined,
      requestKey: save.requestKey,
    };
  }
}
