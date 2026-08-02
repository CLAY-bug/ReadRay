import {
  cloneWritingSnapshot,
  writingSnapshotsEqual,
  type WritingMode,
  type WritingSnapshot,
} from "./writingViewModel.ts";

export type WritingRequestIdentity = {
  documentId: number;
  revision: number;
  generation: number;
  snapshot: WritingSnapshot;
  versionId?: number;
};

export function captureWritingRequestIdentity(
  identity: WritingRequestIdentity,
): WritingRequestIdentity {
  return {
    ...identity,
    snapshot: cloneWritingSnapshot(identity.snapshot),
  };
}

export function writingRequestIdentityMatches(
  captured: WritingRequestIdentity,
  current: WritingRequestIdentity,
) {
  return (
    captured.documentId === current.documentId &&
    captured.revision === current.revision &&
    captured.generation === current.generation &&
    captured.versionId === current.versionId &&
    writingSnapshotsEqual(captured.snapshot, current.snapshot)
  );
}

export async function runGuardedWritingRequest<T>(
  captured: WritingRequestIdentity,
  getCurrent: () => WritingRequestIdentity,
  request: Promise<T>,
  operation: string,
  onResolved?: (result: T) => void | Promise<void>,
) {
  const result = await request;
  let matches = false;
  try {
    matches = writingRequestIdentityMatches(
      captured,
      getCurrent(),
    );
  } catch {
    matches = false;
  }
  await onResolved?.(result);
  if (!matches) {
    throw new Error(`${operation}结果已过期，不再属于当前可见文章版本。`);
  }
  return result;
}

export function shouldHandleWritingShortcut(
  hidden: boolean,
  mode: WritingMode,
  event: Pick<KeyboardEvent, "key" | "ctrlKey">,
) {
  if (hidden) {
    return false;
  }
  if (event.key === "Escape") {
    return true;
  }
  return (
    event.ctrlKey &&
    event.key.toLowerCase() === "j" &&
    ["draft", "review", "completed"].includes(mode)
  );
}
