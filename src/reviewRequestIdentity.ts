export type ReviewMutationIdentity = {
  pageKey: number;
  queueItemId: number;
  requestKey: string;
};

export function isReviewMutationCurrent(
  currentPageKey: number,
  currentQueueItemId: number | undefined,
  pendingRequestKey: string | undefined,
  identity: ReviewMutationIdentity,
) {
  return (
    currentPageKey === identity.pageKey &&
    currentQueueItemId === identity.queueItemId &&
    pendingRequestKey === identity.requestKey
  );
}

let fallbackRequestSequence = 0;

export function createReviewRequestKey(prefix: string) {
  const random = globalThis.crypto?.randomUUID?.();
  if (random) return `${prefix}:${random}`;
  fallbackRequestSequence += 1;
  return `${prefix}:${Date.now()}:${fallbackRequestSequence}`;
}
