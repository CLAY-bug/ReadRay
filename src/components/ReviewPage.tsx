import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type MouseEvent,
} from "react";
import {
  appendReviewFeedPage,
  applyPreparedReviewCard,
  applyReviewFeedItemState,
  applyReviewOutcomeResult,
  applyReviewQualityFeedback,
  reviewQualityCardContextKey,
  visibleReviewCards,
  type ReviewCardModel,
  type ReviewFeedModel,
  type ReviewService,
} from "../reviewService";
import { ReviewAuthorityRefreshGate } from "../reviewAuthorityRefresh";
import {
  createReviewFeedbackDraft,
  type ReviewQualityCoordinator,
  type ReviewQualityFailure,
} from "../reviewQualitySaveQueue";
import type {
  ReviewPreparationCoordinator,
  ReviewPreparationSnapshot,
} from "../reviewPreparationCoordinator";
import {
  createReviewRequestKey,
  isReviewMutationCurrent,
  type ReviewMutationIdentity,
} from "../reviewRequestIdentity";
import type {
  ReviewOutcome,
  ReviewQualityPolarity,
  SubmitReviewOutcomeInput,
  UndoReviewOutcomeInput,
  UndoReviewQualityFeedbackInput,
} from "../types/review";

type ReviewPageProps = {
  service: ReviewService | null;
  preparationCoordinator: ReviewPreparationCoordinator | null;
  qualityCoordinator: ReviewQualityCoordinator | null;
  refreshToken: number;
  onOpenMemoryRecord: (learningRecordId: string) => void;
  onReturnToday: () => void;
};

type AsyncMutation<T> = {
  identity: ReviewMutationIdentity;
  input: T;
  status: "working" | "error";
  error?: string;
};

type UndoMutation = AsyncMutation<UndoReviewOutcomeInput> & { attemptId: number };

const positiveReasons = [
  ["needed", "正好需要"],
  ["helpful_context", "语境有帮助"],
  ["suitable_difficulty", "难度合适"],
  ["clear_prompt", "题目清楚"],
  ["want_similar", "希望多看类似卡片"],
  ["other", "其他"],
] as const;

const negativeReasons = [
  ["already_known", "早就会了"],
  ["not_worth_reviewing", "不值得复习"],
  ["incorrect_meaning", "词义或语境有问题"],
  ["unclear_prompt", "题目不清楚"],
  ["answer_problem", "答案或解释有问题"],
  ["too_frequent", "出现太频繁"],
  ["unwanted_source", "不想使用此来源"],
  ["other", "其他"],
] as const;

const EMPTY_PREPARATION: ReviewPreparationSnapshot = {
  queuedCount: 0,
  workingCount: 0,
  readyAheadCount: 0,
  bufferedCount: 0,
  failedFeedItemIds: [],
  needsMoreCandidates: false,
};

const REVIEW_SHELF_MAX_COLUMNS = 3;
const REVIEW_SHELF_CAPACITY = REVIEW_SHELF_MAX_COLUMNS * 2;
const LIST_MARKER_LINE = /^[\t ]*[•●◦▪▫‣⁃][\t ]+/gmu;

function reviewShelfCapacity(columnCount: number) {
  const columns = Math.min(
    REVIEW_SHELF_MAX_COLUMNS,
    Math.max(1, Math.trunc(columnCount) || 1),
  );
  return columns * 2;
}

function compactReviewSourceTime(sourceTime: string) {
  const match = sourceTime.match(
    /^(\d{4})\s*年\s*(\d{1,2})\s*月\s*(\d{1,2})\s*日(?:\s+(\d{1,2}:\d{2}))?/u,
  );
  if (!match) return sourceTime;
  const [, year, month, day, time] = match;
  const date = `${month.padStart(2, "0")}.${day.padStart(2, "0")}`;
  const dated = Number(year) === new Date().getFullYear() ? date : `${year}.${date}`;
  return time ? `${dated} · ${time}` : dated;
}

function reviewDisplayText(text: string, fullPrompt: string) {
  const markers = fullPrompt.match(LIST_MARKER_LINE) ?? [];
  if (markers.length !== 1) return text;
  return text.replace(/^[\t ]*[•●◦▪▫‣⁃][\t ]+/u, "");
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function Icon({ name }: { name: "close" | "source" | "up" | "down" | "check" }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      {name === "close" ? (
        <path d="m6 6 12 12M18 6 6 18" />
      ) : name === "source" ? (
        <>
          <path d="M7 4h10a3 3 0 0 1 3 3v10a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3V7a3 3 0 0 1 3-3z" />
          <path d="M8 9h8M8 13h8M8 17h5" />
        </>
      ) : name === "up" ? (
        <path d="M7 10v10H4V10h3Zm0 8h10.2a2 2 0 0 0 1.94-1.52l1.1-4.5A2 2 0 0 0 18.3 9.5H14l.7-3.1A2 2 0 0 0 12.75 4L7 10v8Z" />
      ) : name === "down" ? (
        <path d="M7 14V4H4v10h3Zm0-8h10.2a2 2 0 0 1 1.94 1.52l1.1 4.5a2 2 0 0 1-1.94 2.48H14l.7 3.1a2 2 0 0 1-1.95 2.4L7 14V6Z" />
      ) : (
        <path d="m5 12 4 4L19 6" />
      )}
    </svg>
  );
}

function FocusPrompt({ card }: { card: ReviewCardModel }) {
  if (card.promptKind !== "cloze") {
    return <>{reviewDisplayText(card.promptText, card.promptText)}</>;
  }
  return (
    <>
      {reviewDisplayText(card.promptBefore, card.promptText)}
      <strong className="rr-review-answer-word">{card.promptAnswer}</strong>
      {card.promptAfter}
    </>
  );
}

function FeedPrompt({ card }: { card: ReviewCardModel }) {
  if (card.promptKind !== "cloze") {
    return <>{reviewDisplayText(card.promptText, card.promptText)}</>;
  }
  return (
    <>
      {reviewDisplayText(card.promptBefore, card.promptText)}
      <strong>{card.promptAnswer}</strong>
      {card.promptAfter}
    </>
  );
}

const compactReviewTypeLabels: Record<string, string> = {
  单词语境: "单词",
  短语语境: "短语",
  句子理解: "句子",
  段落理解: "段落",
};

function compactReviewTypeLabel(label: string) {
  return compactReviewTypeLabels[label] ?? label;
}

function ReviewPage({
  service,
  preparationCoordinator,
  qualityCoordinator,
  refreshToken,
  onOpenMemoryRecord,
  onReturnToday,
}: ReviewPageProps) {
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [error, setError] = useState<string>();
  const [retryToken, setRetryToken] = useState(0);
  const [feed, setFeedState] = useState<ReviewFeedModel>();
  const [loadMoreError, setLoadMoreError] = useState<string>();
  const [shelfCapacity, setShelfCapacity] = useState(REVIEW_SHELF_CAPACITY);
  const [shelfFeedItemIds, setShelfFeedItemIds] = useState<
    Array<number | undefined>
  >([]);
  const [activeFeedItemId, setActiveFeedItemId] = useState<number>();
  const [sourceOpen, setSourceOpen] = useState(false);
  const [feedbackFeedItemId, setFeedbackFeedItemId] = useState<number>();
  const [feedbackPolarity, setFeedbackPolarity] = useState<ReviewQualityPolarity>("up");
  const [feedbackReasons, setFeedbackReasons] = useState<string[]>([]);
  const [feedbackDetail, setFeedbackDetail] = useState("");
  const [preparation, setPreparation] =
    useState<ReviewPreparationSnapshot>(EMPTY_PREPARATION);
  const [outcomeMutation, setOutcomeMutation] = useState<AsyncMutation<SubmitReviewOutcomeInput>>();
  const [undoMutation, setUndoMutation] = useState<UndoMutation>();
  const [qualityFailures, setQualityFailures] = useState<ReviewQualityFailure[]>([]);
  const [qualityWorking, setQualityWorking] = useState<{
    kind: "save" | "undo";
    input: { feedItemId: number; learningRecordId: number };
    requestKey: string;
  }>();
  const [toast, setToast] = useState<string>();
  const [authorityRefreshSequence, setAuthorityRefreshSequence] = useState(0);

  const mountedRef = useRef(true);
  const pageKeyRef = useRef(0);
  const feedRef = useRef(feed);
  const activeFeedItemIdRef = useRef(activeFeedItemId);
  const outcomeMutationRef = useRef(outcomeMutation);
  const undoMutationRef = useRef(undoMutation);
  const undoRequestKeysRef = useRef(new Map<number, string>());
  const pageRef = useRef<HTMLElement>(null);
  const feedElementRef = useRef<HTMLElement>(null);
  const loadingCursorRef = useRef<number | undefined>(undefined);
  const toastTimerRef = useRef<number | undefined>(undefined);
  const authorityRefreshGateRef = useRef(new ReviewAuthorityRefreshGate());
  const authorityRequestRef = useRef(0);
  const lastExternalRefreshTokenRef = useRef(refreshToken);
  const lastAuthorityRefreshSequenceRef = useRef(authorityRefreshSequence);
  const toastShownForRequestRef = useRef(new Set<string>());

  feedRef.current = feed;
  activeFeedItemIdRef.current = activeFeedItemId;
  outcomeMutationRef.current = outcomeMutation;
  undoMutationRef.current = undoMutation;

  const setFeed = useCallback((next: ReviewFeedModel) => {
    feedRef.current = next;
    setFeedState(next);
  }, []);

  const showToast = useCallback((message: string) => {
    if (toastTimerRef.current !== undefined) window.clearTimeout(toastTimerRef.current);
    setToast(message);
    toastTimerRef.current = window.setTimeout(() => setToast(undefined), 3200);
  }, []);

  const applyPreparedCache = useCallback((next: ReviewFeedModel) => {
    let reconciled = next;
    if (!preparationCoordinator) return reconciled;
    for (const card of next.cards) {
      const prepared = preparationCoordinator.getPreparedCard({
        dayStartUnixMs: next.dayStartUnixMs,
        feedItemId: card.feedItemId,
        learningRecordId: card.learningRecordId,
        learningTargetId: card.learningTargetId,
        cycleIndex: card.cycleIndex,
      });
      if (card.needsPreparation && prepared) {
        reconciled = applyPreparedReviewCard(
          reconciled,
          card.feedItemId,
          prepared,
        );
      }
    }
    return reconciled;
  }, [preparationCoordinator]);

  const hasWorkingMutation = useCallback(() =>
    outcomeMutationRef.current?.status === "working" ||
    undoMutationRef.current?.status === "working" ||
    Boolean(qualityCoordinator?.hasWorkingMutation()), [qualityCoordinator]);

  const flushDeferredAuthorityRefresh = useCallback(() => {
    if (
      !authorityRefreshGateRef.current.releaseDeferredRefresh(hasWorkingMutation())
    ) return;
    setAuthorityRefreshSequence((value) => value + 1);
  }, [hasWorkingMutation]);

  const reconcileFeedItemFromAuthority = useCallback(async (feedItemId: number) => {
    if (!service) return undefined;
    const state = await service.loadFeedItemState(feedItemId);
    if (state.card.qualityFeedback) {
      qualityCoordinator?.updateFeedItemFeedback(
        feedItemId,
        state.card.qualityFeedback,
      );
    }
    if (!mountedRef.current) return state;
    const current = feedRef.current;
    if (
      current &&
      current.dayStartUnixMs === state.dayStartUnixMs &&
      current.dayEndUnixMs === state.dayEndUnixMs
    ) {
      setFeed(applyReviewFeedItemState(current, state));
    }
    return state;
  }, [qualityCoordinator, service, setFeed]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      pageKeyRef.current += 1;
      if (toastTimerRef.current !== undefined) window.clearTimeout(toastTimerRef.current);
    };
  }, []);

  useEffect(() => {
    if (!qualityCoordinator) return undefined;
    const unsubscribe = qualityCoordinator.subscribe((event) => {
      if (!mountedRef.current) return;
      if (event.type === "state") {
        setQualityFailures(event.state.failures);
        setQualityWorking(event.state.working);
        return;
      }
      if (event.type === "failed") {
        void reconcileFeedItemFromAuthority(event.start.input.feedItemId)
          .catch(() => undefined)
          .finally(flushDeferredAuthorityRefresh);
        return;
      }
      const feedItemId = event.result.feedItemId;
      if (!toastShownForRequestRef.current.has(event.start.input.requestKey)) {
        toastShownForRequestRef.current.add(event.start.input.requestKey);
        showToast(
          event.start.kind === "save" ? "卡片质量反馈已保存" : "卡片质量反馈已撤销",
        );
      }
      const current = feedRef.current;
      const card = current?.cards.find((candidate) => candidate.feedItemId === feedItemId);
      if (current && card) {
        setFeed(applyReviewQualityFeedback(current, event.result));
      }
      void reconcileFeedItemFromAuthority(feedItemId)
        .catch(() => undefined)
        .finally(flushDeferredAuthorityRefresh);
    });
    return unsubscribe;
  }, [
    flushDeferredAuthorityRefresh,
    qualityCoordinator,
    reconcileFeedItemFromAuthority,
    setFeed,
    showToast,
  ]);

  useEffect(() => {
    const pageKey = ++pageKeyRef.current;
    setStatus("loading");
    setError(undefined);
    setFeedState(undefined);
    feedRef.current = undefined;
    setActiveFeedItemId(undefined);
    setPreparation(EMPTY_PREPARATION);
    setOutcomeMutation(undefined);
    setUndoMutation(undefined);
    authorityRefreshGateRef.current.reset();
    toastShownForRequestRef.current.clear();
    loadingCursorRef.current = undefined;
    setLoadMoreError(undefined);
    setShelfFeedItemIds([]);
    if (!service) {
      setStatus("error");
      setError("复习服务尚未准备好。");
      return;
    }
    service.loadFeedPage().then(
      (next) => {
        if (!mountedRef.current || pageKeyRef.current !== pageKey) return;
        setFeed(applyPreparedCache(next));
        setStatus("ready");
      },
      (reason) => {
        if (!mountedRef.current || pageKeyRef.current !== pageKey) return;
        setStatus("error");
        setError(errorMessage(reason));
      },
    );
  }, [applyPreparedCache, service, retryToken, setFeed]);

  useEffect(() => {
    const externalChanged = lastExternalRefreshTokenRef.current !== refreshToken;
    const authorityChanged =
      lastAuthorityRefreshSequenceRef.current !== authorityRefreshSequence;
    if (!externalChanged && !authorityChanged) return;
    lastExternalRefreshTokenRef.current = refreshToken;
    lastAuthorityRefreshSequenceRef.current = authorityRefreshSequence;
    if (!service) return;
    if (!authorityRefreshGateRef.current.requestRefresh(hasWorkingMutation())) {
      return;
    }
    const requestId = ++authorityRequestRef.current;
    const pageKey = pageKeyRef.current;
    service.loadFeedPage().then(
      (next) => {
        if (
          !mountedRef.current ||
          pageKeyRef.current !== pageKey ||
          authorityRequestRef.current !== requestId
        ) return;
        if (!authorityRefreshGateRef.current.requestRefresh(hasWorkingMutation())) {
          return;
        }
        setFeed(applyPreparedCache(next));
      },
      (reason) => {
        if (
          !mountedRef.current ||
          pageKeyRef.current !== pageKey ||
          authorityRequestRef.current !== requestId
        ) return;
        setLoadMoreError(errorMessage(reason));
      },
    );
  }, [
    applyPreparedCache,
    authorityRefreshSequence,
    hasWorkingMutation,
    refreshToken,
    service,
    setFeed,
  ]);

  useEffect(() => {
    if (!preparationCoordinator) {
      setPreparation(EMPTY_PREPARATION);
      return;
    }
    const detachPageConsumer = preparationCoordinator.attachPageConsumer();
    const unsubscribe = preparationCoordinator.subscribe((event) => {
      if (!mountedRef.current) return;
      if (event.type === "status") {
        setPreparation(event.snapshot);
        return;
      }
      if (event.type !== "prepared") return;
      const current = feedRef.current;
      const target = current?.cards.find(
        (card) => card.feedItemId === event.task.feedItemId,
      );
      if (
        !current ||
        current.dayStartUnixMs !== event.task.dayStartUnixMs ||
        !target?.needsPreparation ||
        target.learningRecordId !== event.task.learningRecordId ||
        target.learningTargetId !== event.task.learningTargetId ||
        target.cycleIndex !== event.task.cycleIndex
      ) {
        return;
      }
      setFeed(
        applyPreparedReviewCard(
          current,
          event.task.feedItemId,
          event.generatedCard,
        ),
      );
    });
    return () => {
      unsubscribe();
      detachPageConsumer();
    };
  }, [preparationCoordinator, setFeed]);

  useEffect(() => {
    if (feed && preparationCoordinator) {
      preparationCoordinator.syncFeed(feed);
    }
  }, [feed, preparationCoordinator]);

  const loadMore = useCallback(() => {
    const current = feedRef.current;
    if (!service || !current?.canContinue || status !== "ready") return;
    const pageKey = pageKeyRef.current;
    const cursor = current.nextCursor;
    if (cursor === undefined || loadingCursorRef.current !== undefined) return;
    loadingCursorRef.current = cursor;
    setLoadMoreError(undefined);
    service.loadFeedPage({ cursor }).then(
      (next) => {
        if (
          !mountedRef.current ||
          pageKeyRef.current !== pageKey ||
          loadingCursorRef.current !== cursor
        ) return;
        const latest = feedRef.current;
        loadingCursorRef.current = undefined;
        if (!latest) return;
        setFeed(appendReviewFeedPage(latest, next));
      },
      (reason) => {
        if (
          !mountedRef.current ||
          pageKeyRef.current !== pageKey ||
          loadingCursorRef.current !== cursor
        ) return;
        loadingCursorRef.current = undefined;
        setLoadMoreError(errorMessage(reason));
      },
    );
  }, [service, setFeed, status]);

  useEffect(() => {
    if (
      preparation.needsMoreCandidates &&
      preparationCoordinator?.getSnapshot().needsMoreCandidates
    ) {
      loadMore();
    }
  }, [
    feed?.nextCursor,
    loadMore,
    preparation.needsMoreCandidates,
    preparationCoordinator,
  ]);

  const activeCard = useMemo(
    () => feed?.cards.find((card) => card.feedItemId === activeFeedItemId),
    [activeFeedItemId, feed],
  );
  const readyCards = useMemo(() => visibleReviewCards(feed), [feed]);
  const shelfCandidates = useMemo(
    () =>
      readyCards.filter(
        (card) => !card.attempt || card.feedItemId === activeFeedItemId,
      ),
    [activeFeedItemId, readyCards],
  );
  const shelfCardsById = useMemo(
    () => new Map(shelfCandidates.map((card) => [card.feedItemId, card])),
    [shelfCandidates],
  );
  const shelfCards = useMemo(
    () =>
      shelfFeedItemIds.flatMap((feedItemId, slotIndex) => {
        if (feedItemId === undefined) return [];
        const card = shelfCardsById.get(feedItemId);
        return card ? [{ card, slotIndex }] : [];
      }),
    [shelfCardsById, shelfFeedItemIds],
  );
  const readyCardObservationKey = shelfCards
    .map(({ card, slotIndex }) => `${card.feedItemId}:${card.ordinal}:${slotIndex}`)
    .join(",");

  useLayoutEffect(() => {
    const feedElement = feedElementRef.current;
    if (!feedElement) return;
    const updateCapacity = () => {
      const columnCount = Number.parseInt(
        window
          .getComputedStyle(feedElement)
          .getPropertyValue("--rr-review-column-count"),
        10,
      );
      setShelfCapacity(reviewShelfCapacity(columnCount));
    };
    updateCapacity();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      updateCapacity();
    });
    observer.observe(feedElement);
    return () => observer.disconnect();
  }, [feed?.dayStartUnixMs, status]);

  useEffect(() => {
    setShelfFeedItemIds((current) => {
      const next: Array<number | undefined> = Array.from(
        { length: shelfCapacity },
        (_, index) => current[index],
      );
      const used = new Set<number>();
      for (let index = 0; index < next.length; index += 1) {
        const feedItemId = next[index];
        if (
          feedItemId !== undefined &&
          !used.has(feedItemId) &&
          shelfCardsById.has(feedItemId)
        ) {
          used.add(feedItemId);
        } else {
          next[index] = undefined;
        }
      }
      let candidateIndex = 0;
      for (let slotIndex = 0; slotIndex < next.length; slotIndex += 1) {
        if (next[slotIndex] !== undefined) continue;
        while (
          candidateIndex < shelfCandidates.length &&
          used.has(shelfCandidates[candidateIndex].feedItemId)
        ) {
          candidateIndex += 1;
        }
        const candidate = shelfCandidates[candidateIndex];
        if (candidate) {
          next[slotIndex] = candidate.feedItemId;
          used.add(candidate.feedItemId);
          candidateIndex += 1;
        }
      }
      return next.length === current.length &&
        next.every((feedItemId, index) => feedItemId === current[index])
        ? current
        : next;
    });
  }, [shelfCandidates, shelfCardsById, shelfCapacity]);

  useEffect(() => {
    const root = pageRef.current;
    const feedElement = feedElementRef.current;
    if (
      !root ||
      !feedElement ||
      !preparationCoordinator ||
      typeof IntersectionObserver === "undefined"
    ) {
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting || entry.intersectionRatio < 0.01) continue;
          const feedItemId = Number(
            (entry.target as HTMLElement).dataset.reviewFeedItemId,
          );
          const current = feedRef.current;
          if (current && Number.isSafeInteger(feedItemId)) {
            preparationCoordinator.markConsumed(current, feedItemId);
          }
        }
      },
      { root, threshold: [0.01] },
    );
    for (const element of feedElement.querySelectorAll<HTMLElement>(
      "[data-review-ordinal]",
    )) {
      observer.observe(element);
    }
    return () => observer.disconnect();
  }, [preparationCoordinator, readyCardObservationKey]);

  function closeCard() {
    if (outcomeMutationRef.current?.status === "working") return;
    setActiveFeedItemId(undefined);
    activeFeedItemIdRef.current = undefined;
    setSourceOpen(false);
    setFeedbackFeedItemId(undefined);
  }

  function openCard(card: ReviewCardModel) {
    if (card.needsPreparation) return;
    const current = feedRef.current;
    if (current) preparationCoordinator?.markConsumed(current, card.feedItemId);
    setActiveFeedItemId(card.feedItemId);
    activeFeedItemIdRef.current = card.feedItemId;
    setSourceOpen(false);
    setFeedbackFeedItemId(undefined);
  }

  function submitOutcome(outcome: ReviewOutcome, retry = false) {
    const card = feedRef.current?.cards.find(
      (candidate) => candidate.feedItemId === activeFeedItemIdRef.current,
    );
    if (!service || !card || card.needsPreparation || card.attempt) return;
    const existing = outcomeMutationRef.current;
    if (existing?.status === "working") return;
    const requestKey =
      retry && existing?.identity.queueItemId === card.feedItemId
        ? existing.input.requestKey
        : createReviewRequestKey(`review-outcome:${card.feedItemId}`);
    const input: SubmitReviewOutcomeInput = {
      feedItemId: card.feedItemId,
      learningRecordId: card.learningRecordId,
      learningTargetId: card.learningTargetId,
      expectedRevision: card.target.revision,
      outcome,
      // 完整答案已直接可见，旧调度应保守地按“有辅助”处理本次结果。
      usedHint: true,
      requestKey,
    };
    const identity = {
      pageKey: pageKeyRef.current,
      queueItemId: card.feedItemId,
      requestKey,
    };
    const mutation = { identity, input, status: "working" as const };
    outcomeMutationRef.current = mutation;
    setOutcomeMutation(mutation);
    service.submitOutcome(input).then(
      (result) => {
        const pending = outcomeMutationRef.current;
        if (
          !mountedRef.current ||
          !pending ||
          !isReviewMutationCurrent(
            pageKeyRef.current,
            activeFeedItemIdRef.current,
            pending.input.requestKey,
            identity,
          )
        ) return;
        const current = feedRef.current;
        if (!current) return;
        setFeed(applyReviewOutcomeResult(current, card.feedItemId, result));
        outcomeMutationRef.current = undefined;
        setOutcomeMutation(undefined);
        showToast(outcome === "remembered" ? "已记录：想起来了" : "已记录：没想起来");
        void reconcileFeedItemFromAuthority(card.feedItemId).finally(
          flushDeferredAuthorityRefresh,
        );
      },
      (reason) => {
        const pending = outcomeMutationRef.current;
        if (!mountedRef.current || !pending || pending.identity.requestKey !== requestKey) return;
        void (async () => {
          try {
            const state = await reconcileFeedItemFromAuthority(card.feedItemId);
            if (state?.card.attempt?.requestKey === requestKey) {
              outcomeMutationRef.current = undefined;
              setOutcomeMutation(undefined);
              showToast(outcome === "remembered" ? "已记录：想起来了" : "已记录：没想起来");
              flushDeferredAuthorityRefresh();
              return;
            }
          } catch {
            // 保留原请求与当前卡片，允许用同一 request key 重试。
          }
          const latestPending = outcomeMutationRef.current;
          if (!latestPending || latestPending.identity.requestKey !== requestKey) return;
          const failed = { ...latestPending, status: "error" as const, error: errorMessage(reason) };
          outcomeMutationRef.current = failed;
          setOutcomeMutation(failed);
          flushDeferredAuthorityRefresh();
        })();
      },
    );
  }

  function undoOutcome(card: ReviewCardModel, retry = false) {
    if (!service || !card.attempt || undoMutationRef.current?.status === "working") return;
    const existing = undoMutationRef.current;
    const requestKey =
      retry && existing?.attemptId === card.attempt.id
        ? existing.input.requestKey
        : undoRequestKeysRef.current.get(card.attempt.id) ??
          createReviewRequestKey(`review-undo:${card.attempt.id}`);
    undoRequestKeysRef.current.set(card.attempt.id, requestKey);
    const input: UndoReviewOutcomeInput = {
      attemptId: card.attempt.id,
      feedItemId: card.feedItemId,
      learningRecordId: card.learningRecordId,
      learningTargetId: card.learningTargetId,
      expectedRevision: card.target.revision,
      requestKey,
    };
    const identity = {
      pageKey: pageKeyRef.current,
      queueItemId: card.feedItemId,
      requestKey,
    };
    const mutation: UndoMutation = {
      identity,
      input,
      attemptId: card.attempt.id,
      status: "working",
    };
    undoMutationRef.current = mutation;
    setUndoMutation(mutation);
    service.undoOutcome(input).then(
      (result) => {
        const pending = undoMutationRef.current;
        if (!mountedRef.current || !pending || pending.identity.requestKey !== requestKey) return;
        const current = feedRef.current;
        if (!current) return;
        setFeed(applyReviewOutcomeResult(current, card.feedItemId, result));
        undoRequestKeysRef.current.delete(card.attempt!.id);
        undoMutationRef.current = undefined;
        setUndoMutation(undefined);
        showToast("已撤销这次学习结果");
        void reconcileFeedItemFromAuthority(card.feedItemId).finally(
          flushDeferredAuthorityRefresh,
        );
      },
      (reason) => {
        const pending = undoMutationRef.current;
        if (!mountedRef.current || !pending || pending.identity.requestKey !== requestKey) return;
        void (async () => {
          try {
            const state = await reconcileFeedItemFromAuthority(card.feedItemId);
            if (
              state &&
              !state.card.attempt &&
              state.card.target.revision >= input.expectedRevision + 1
            ) {
              undoRequestKeysRef.current.delete(card.attempt!.id);
              undoMutationRef.current = undefined;
              setUndoMutation(undefined);
              showToast("已撤销这次学习结果");
              flushDeferredAuthorityRefresh();
              return;
            }
          } catch {
            // 保留原撤销请求，允许用同一 request key 重试。
          }
          const latestPending = undoMutationRef.current;
          if (!latestPending || latestPending.identity.requestKey !== requestKey) return;
          const failed = { ...latestPending, status: "error" as const, error: errorMessage(reason) };
          undoMutationRef.current = failed;
          setUndoMutation(failed);
          flushDeferredAuthorityRefresh();
        })();
      },
    );
  }

  function openFeedback(card: ReviewCardModel, polarity: ReviewQualityPolarity) {
    const draft = createReviewFeedbackDraft(card.qualityFeedback, polarity);
    setFeedbackFeedItemId(card.feedItemId);
    setFeedbackPolarity(polarity);
    setFeedbackReasons(draft.reasonCodes);
    setFeedbackDetail(draft.detail);
    if (
      !card.qualityFeedback?.active ||
      card.qualityFeedback.polarity !== polarity
    ) {
      qualityCoordinator?.enqueueSave({
        feedItemId: card.feedItemId,
        learningRecordId: card.learningRecordId,
        cardContextKey: reviewQualityCardContextKey(card),
        polarity,
        reasonCodes: [],
        requestKey: createReviewRequestKey(`review-quality:${card.feedItemId}`),
      });
    }
  }

  function saveFeedback(event: FormEvent, retry = false) {
    event.preventDefault();
    const current = feedRef.current;
    const card = current?.cards.find((candidate) => candidate.feedItemId === feedbackFeedItemId);
    if (!service || !card || !qualityCoordinator) return;
    const cardContextKey = reviewQualityCardContextKey(card);
    const detail = feedbackDetail.trim() || undefined;
    const failure = qualityFailures.find(
      (candidate) =>
        candidate.kind === "save" &&
        candidate.feedItemId === card.feedItemId &&
        candidate.learningRecordId === card.learningRecordId &&
        "cardContextKey" in candidate.intent &&
        candidate.intent.cardContextKey === cardContextKey,
    );
    if (
      retry &&
      failure &&
      "cardContextKey" in failure.intent &&
      failure.intent.polarity === feedbackPolarity &&
      JSON.stringify(failure.intent.reasonCodes) === JSON.stringify(feedbackReasons) &&
      (failure.intent.detail ?? undefined) === detail
    ) {
      qualityCoordinator.retryFailure(card.feedItemId, card.learningRecordId);
      return;
    }
    qualityCoordinator.enqueueSave({
      feedItemId: card.feedItemId,
      learningRecordId: card.learningRecordId,
      cardContextKey,
      polarity: feedbackPolarity,
      reasonCodes: feedbackReasons,
      detail,
      requestKey: createReviewRequestKey(`review-quality:${card.feedItemId}`),
    });
  }

  function undoFeedback(card: ReviewCardModel) {
    const feedback = card.qualityFeedback;
    if (!feedback?.active || !qualityCoordinator) return;
    const input: UndoReviewQualityFeedbackInput = {
      feedbackId: feedback.id,
      feedItemId: card.feedItemId,
      learningRecordId: card.learningRecordId,
      expectedRevision: feedback.revision,
      requestKey: createReviewRequestKey(`review-quality-undo:${feedback.id}`),
    };
    qualityCoordinator.enqueueUndo(input);
  }

  function cardQualityFailure(card: ReviewCardModel) {
    return qualityFailures.find(
      (candidate) =>
        candidate.feedItemId === card.feedItemId &&
        candidate.learningRecordId === card.learningRecordId,
    );
  }

  function backdropClick(event: MouseEvent<HTMLDivElement>) {
    if (event.target === event.currentTarget) closeCard();
  }

  const feedbackCard = feed?.cards.find((card) => card.feedItemId === feedbackFeedItemId);
  const feedbackCardContextKey = feedbackCard
    ? reviewQualityCardContextKey(feedbackCard)
    : undefined;
  const feedbackQualityWorking =
    feedbackCard && qualityWorking?.input.feedItemId === feedbackCard.feedItemId
      ? qualityWorking
      : undefined;
  const feedbackQualityFailure = feedbackCard
    ? qualityFailures.find(
        (candidate) =>
          candidate.feedItemId === feedbackCard.feedItemId &&
          candidate.learningRecordId === feedbackCard.learningRecordId &&
          candidate.kind === "save" &&
          "cardContextKey" in candidate.intent &&
          candidate.intent.cardContextKey === feedbackCardContextKey,
      )
    : undefined;
  const reasonOptions = feedbackPolarity === "up" ? positiveReasons : negativeReasons;
  const loadedPendingIds = new Set(
    feed?.cards
      .filter((card) => card.needsPreparation)
      .map((card) => card.feedItemId) ?? [],
  );
  const failedPreparationCount = preparation.failedFeedItemIds.filter((feedItemId) =>
    loadedPendingIds.has(feedItemId),
  ).length;
  const backgroundPreparationCount =
    preparation.queuedCount + preparation.workingCount;

  return (
    <main ref={pageRef} className="rr-review-page" aria-label="复习">
      <header className="rr-review-page-header">
        <div>
          <h1>复习</h1>
          <p className="rr-review-page-intro">选择一张卡片，进入专注复习</p>
        </div>
        <p className="rr-review-day-status">
          {feed ? `已复习 ${feed.completedCount}` : "正在准备"}
        </p>
      </header>

      {status === "loading" ? (
        <section className="rr-review-state-card" aria-live="polite">
          <span className="rr-review-spinner" />
          <p>正在从 SQLite 准备复习内容…</p>
        </section>
      ) : status === "error" ? (
        <section className="rr-review-state-card rr-review-state-card-error" role="alert">
          <h2>复习内容暂时无法读取</h2>
          <p>{error}</p>
          <div className="rr-review-state-actions">
            <button type="button" onClick={() => setRetryToken((value) => value + 1)}>重试</button>
            <button type="button" className="is-secondary" onClick={onReturnToday}>返回今天</button>
          </div>
        </section>
      ) : !feed || feed.cards.length === 0 ? (
        <section className="rr-review-empty">
          <p className="rr-review-eyebrow">NO LEARNING RECORDS</p>
          <h2>还没有可以复习的记录</h2>
          <p>先完成一次真实查询，学习记录会自动进入这里。</p>
          <button type="button" onClick={onReturnToday}>返回今天</button>
        </section>
      ) : (
        <>
          <section ref={feedElementRef} className="rr-review-feed" aria-label="复习卡片书架">
            {shelfCards.map(({ card, slotIndex }) => {
              const shelfColumnCount = shelfCapacity / 2;
              return (
              <article
                className={`rr-review-feed-card is-density-${card.density} is-${card.visualVariant} is-tone-${card.paperTone}${card.attempt ? " is-completed" : ""}`}
                key={card.feedItemId}
                data-review-ordinal={card.ordinal}
                data-review-feed-item-id={card.feedItemId}
                style={{
                  "--rr-review-card-delay": `${(card.ordinal % 6) * 24}ms`,
                  gridColumn: (slotIndex % shelfColumnCount) + 1,
                  gridRow: Math.floor(slotIndex / shelfColumnCount) + 1,
                } as CSSProperties}
              >
                <button type="button" className="rr-review-feed-card-open" onClick={() => openCard(card)}>
                  <span className="rr-review-card-meta">
                    <span className="rr-review-card-meta-leading">
                      <span className="rr-review-card-type">{compactReviewTypeLabel(card.typeLabel)}</span>
                      {card.contextOrigin === "generated" ? (
                        <span className="rr-review-card-origin">AI 语境</span>
                      ) : null}
                    </span>
                    <span title={card.sourceTime}>{compactReviewSourceTime(card.sourceTime)}</span>
                  </span>
                  <span className="rr-review-card-copy"><FeedPrompt card={card} /></span>
                </button>
                <footer>
                  <span>{card.sourceApp}</span>
                  <span className={card.attempt ? "is-done" : ""}>
                    {card.attempt ? <><Icon name="check" /> 已复习</> : "开始复习 →"}
                  </span>
                </footer>
                {card.attempt ? (
                  <button
                    type="button"
                    className="rr-review-card-undo"
                    disabled={undoMutation?.status === "working"}
                    onClick={() => undoOutcome(card)}
                  >
                    撤销结果
                  </button>
                ) : null}
                {cardQualityFailure(card) ? (
                  <div className="rr-review-feed-card-error" role="alert">
                    <span>卡片质量反馈未保存：{cardQualityFailure(card)!.error}</span>
                    <button
                      type="button"
                      onClick={() =>
                        qualityCoordinator?.retryFailure(
                          card.feedItemId,
                          card.learningRecordId,
                        )
                      }
                    >
                      重试
                    </button>
                  </div>
                ) : null}
              </article>
              );
            })}
          </section>

          {shelfCards.length === 0 && backgroundPreparationCount > 0 ? (
            <section className="rr-review-warmup" aria-live="polite">
              <span className="rr-review-spinner" />
              <div>
                <h2>正在准备第一批英文卡片</h2>
                <p>准备完成后会加入当前书架，已有卡片不会换位。</p>
              </div>
            </section>
          ) : null}

          {shelfCards.length === 0 && backgroundPreparationCount === 0 &&
          !feed.canContinue && failedPreparationCount === 0 ? (
            <section className="rr-review-empty rr-review-round-complete">
              <p className="rr-review-eyebrow">REVIEW COMPLETE</p>
              <h2>这一轮已经复习完成</h2>
              <p>新的到期内容会自动进入这里。</p>
            </section>
          ) : null}

          {loadMoreError || backgroundPreparationCount > 0 || failedPreparationCount > 0 ? (
            <div className="rr-review-preparation-status" aria-live="polite">
              {loadMoreError ? (
              <><span>{loadMoreError}</span><button type="button" onClick={loadMore}>重新加载</button></>
            ) : backgroundPreparationCount > 0 ? (
              <><span className="rr-review-spinner" /> 正在后台准备后续卡片</>
            ) : failedPreparationCount > 0 ? (
              <>
                <span>{failedPreparationCount} 张卡片准备失败。</span>
                <button type="button" onClick={() => preparationCoordinator?.retryFailed(feed)}>重新准备</button>
              </>
              ) : null}
            </div>
          ) : null}
        </>
      )}

      {activeCard ? (
        <div className="rr-review-focus-layer" role="presentation" onMouseDown={backdropClick}>
          <section
            className={`rr-review-focus-panel is-tone-${activeCard.paperTone}`}
            role="dialog"
            aria-modal="true"
            aria-label="复习卡片"
          >
            <header className="rr-review-focus-header">
              <div>
                <span>{activeCard.attempt ? "已完成" : "专注复习"}</span>
                <p>{activeCard.typeLabel} · {activeCard.sourceLabel}</p>
              </div>
              <div className="rr-review-focus-tools">
                <button type="button" onClick={() => setSourceOpen(true)}><Icon name="source" />来源</button>
                <button type="button" className="rr-review-return-shelf" onClick={closeCard}>返回书架</button>
              </div>
            </header>

            <section className="rr-review-focus-body">
              <p className="rr-review-focus-kicker">{activeCard.contextOriginLabel}</p>
              <div className="rr-review-revealed-context"><FocusPrompt card={activeCard} /></div>
              {activeCard.generatedCard ? (
                <p className="rr-review-context-translation">{activeCard.generatedCard.englishContextZh}</p>
              ) : null}
              <dl className="rr-review-answer-details">
                <div><dt>语境义</dt><dd>{activeCard.answerTitle}</dd></div>
                {activeCard.answerDetail && activeCard.answerDetail !== activeCard.answerTitle ? (
                  <div><dt>保存的解释</dt><dd>{activeCard.answerDetail}</dd></div>
                ) : null}
                {activeCard.answerNote ? <div><dt>补充</dt><dd>{activeCard.answerNote}</dd></div> : null}
                {activeCard.example ? <div><dt>保存的例句</dt><dd>{activeCard.example.en}<br />{activeCard.example.zh}</dd></div> : null}
              </dl>
              {activeCard.attempt ? (
                <div className="rr-review-completed-actions">
                  <span><Icon name="check" />已记录“{activeCard.attempt.outcome === "remembered" ? "想起来了" : "没想起来"}”</span>
                  <button type="button" onClick={() => undoOutcome(activeCard)}>撤销</button>
                </div>
              ) : (
                <div className="rr-review-outcome-actions">
                  <button type="button" disabled={outcomeMutation?.status === "working"} onClick={() => submitOutcome("remembered")}>想起来了</button>
                  <button type="button" className="is-secondary" disabled={outcomeMutation?.status === "working"} onClick={() => submitOutcome("forgotten")}>没想起来</button>
                </div>
              )}
            </section>

            {outcomeMutation?.status === "error" && outcomeMutation.identity.queueItemId === activeCard.feedItemId ? (
                  <div className="rr-review-inline-error" role="alert">
                    <span>{outcomeMutation.error}</span>
                    <button type="button" onClick={() => submitOutcome(outcomeMutation.input.outcome, true)}>用同一请求重试</button>
                  </div>
                ) : null}
                {undoMutation?.status === "error" && undoMutation.identity.queueItemId === activeCard.feedItemId ? (
                  <div className="rr-review-inline-error" role="alert">
                    <span>{undoMutation.error}</span>
                    <button type="button" onClick={() => undoOutcome(activeCard, true)}>重试撤销</button>
                  </div>
                ) : null}

            <footer className="rr-review-quality-row">
              <div><strong>卡片质量</strong><span>与学习结果分开保存</span></div>
              <div>
                <button type="button" className={activeCard.qualityFeedback?.active && activeCard.qualityFeedback.polarity === "up" ? "is-selected" : ""} onClick={() => openFeedback(activeCard, "up")} aria-label="这张卡片有帮助"><Icon name="up" /></button>
                <button type="button" className={activeCard.qualityFeedback?.active && activeCard.qualityFeedback.polarity === "down" ? "is-selected" : ""} onClick={() => openFeedback(activeCard, "down")} aria-label="这张卡片有问题"><Icon name="down" /></button>
                {activeCard.qualityFeedback?.active ? <button type="button" className="rr-review-quality-undo" disabled={qualityWorking?.input.feedItemId === activeCard.feedItemId} onClick={() => undoFeedback(activeCard)}>撤销反馈</button> : null}
              </div>
            </footer>
            {qualityWorking?.input.feedItemId === activeCard.feedItemId ? (
              <p className="rr-review-feedback-saving" role="status">
                {qualityWorking.kind === "save" ? "正在保存卡片质量反馈…" : "正在撤销卡片质量反馈…"}
              </p>
            ) : null}
          </section>

          {sourceOpen ? (
            <aside className="rr-review-source-drawer" aria-label="学习记录来源">
              <header><div><span>真实学习记录</span><h2>来源</h2></div><button type="button" aria-label="关闭来源" onClick={() => setSourceOpen(false)}><Icon name="close" /></button></header>
              <dl>
                <div><dt>目标表达</dt><dd>{activeCard.query}</dd></div>
                <div><dt>记录时间</dt><dd>{activeCard.sourceTime}</dd></div>
                <div><dt>来源</dt><dd>{activeCard.sourceLabel}</dd></div>
                <div><dt>原始记录</dt><dd>{activeCard.sourceExcerpt}</dd></div>
                <div><dt>进入 Feed 的原因</dt><dd>{activeCard.reason}</dd></div>
                {activeCard.generatedCard ? <div><dt>当前语境</dt><dd>由 {activeCard.generatedCard.model} 生成并保存；原始学习记录未修改。</dd></div> : null}
              </dl>
              <button type="button" className="rr-review-open-memory" onClick={() => onOpenMemoryRecord(String(activeCard.learningTargetId))}>在记忆页查看这个学习目标</button>
            </aside>
          ) : null}

          {feedbackCard ? (
            <form
              className="rr-review-feedback-popover"
              onSubmit={(event) =>
                saveFeedback(event, Boolean(feedbackQualityFailure))
              }
            >
              <header><div><span>可选详情</span><h2>{feedbackPolarity === "up" ? "这张卡哪里做得好？" : "这张卡哪里有问题？"}</h2></div><button type="button" aria-label="关闭反馈详情" onClick={() => setFeedbackFeedItemId(undefined)}><Icon name="close" /></button></header>
              <div className="rr-review-feedback-options">
                {reasonOptions.map(([value, label]) => (
                  <label key={value}><input type="checkbox" checked={feedbackReasons.includes(value)} onChange={() => setFeedbackReasons((current) => current.includes(value) ? current.filter((item) => item !== value) : [...current, value])} /><span>{label}</span></label>
                ))}
              </div>
              <textarea value={feedbackDetail} maxLength={1000} onChange={(event) => setFeedbackDetail(event.target.value)} placeholder="补充说明（可选）" />
              {feedbackQualityFailure ? <p className="rr-review-feedback-error">{feedbackQualityFailure.error}</p> : null}
              <button type="submit" disabled={Boolean(feedbackQualityWorking)}>{feedbackQualityWorking?.kind === "save" ? "保存中…" : "保存反馈"}</button>
            </form>
          ) : null}
        </div>
      ) : null}

      {toast ? <div className="rr-review-toast" role="status">{toast}</div> : null}
    </main>
  );
}

export default ReviewPage;
