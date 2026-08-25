export type DesktopSaveFlusher = {
  label: string;
  flush: () => Promise<void>;
};

export class DesktopSaveCoordinator {
  private nextId = 0;
  private readonly flushers = new Map<number, DesktopSaveFlusher>();
  private readonly activeOperations = new Set<Promise<void>>();
  private readonly operationFailures = new Map<string, string>();
  private readonly operationGenerations = new Map<string, number>();
  private mutationGeneration = 0;
  private exitRequestId: number | undefined;

  register(flusher: DesktopSaveFlusher) {
    const id = ++this.nextId;
    this.flushers.set(id, flusher);
    return () => {
      this.flushers.delete(id);
    };
  }

  beginExit(requestId: number) {
    if (
      this.exitRequestId !== undefined &&
      this.exitRequestId !== requestId
    ) {
      throw new Error("另一个 ReadRay 退出请求仍在处理中。");
    }
    this.exitRequestId = requestId;
  }

  endExit(requestId: number) {
    if (this.exitRequestId === requestId) {
      this.exitRequestId = undefined;
    }
  }

  recordMutation() {
    if (this.exitRequestId !== undefined) return false;
    this.mutationGeneration += 1;
    return true;
  }

  runMutation<T>(label: string, start: () => Promise<T>) {
    if (!this.recordMutation()) {
      return Promise.reject(new Error("ReadRay 正在保存并退出，暂不接受新的修改。"));
    }
    let operation: Promise<T>;
    try {
      operation = start();
    } catch (error) {
      operation = Promise.reject(error);
    }
    const generation = (this.operationGenerations.get(label) ?? 0) + 1;
    this.operationGenerations.set(label, generation);
    this.operationFailures.delete(label);
    const tracked = operation.then(
      () => {
        if (this.operationGenerations.get(label) === generation) {
          this.operationFailures.delete(label);
        }
      },
      (error) => {
        if (this.operationGenerations.get(label) === generation) {
          this.operationFailures.set(
            label,
            error instanceof Error ? error.message : String(error),
          );
        }
      },
    );
    this.activeOperations.add(tracked);
    void tracked.finally(() => this.activeOperations.delete(tracked));
    return operation;
  }

  private async waitForTrackedOperations() {
    while (this.activeOperations.size) {
      await Promise.all([...this.activeOperations]);
    }
  }

  async flushAll() {
    const failures: string[] = [];
    let observedGeneration: number;
    do {
      observedGeneration = this.mutationGeneration;
      await this.waitForTrackedOperations();
      for (const flusher of [...this.flushers.values()]) {
        try {
          await flusher.flush();
        } catch (error) {
          failures.push(
            `${flusher.label}：${error instanceof Error ? error.message : String(error)}`,
          );
        }
      }
      await this.waitForTrackedOperations();
    } while (observedGeneration !== this.mutationGeneration);
    for (const [label, message] of this.operationFailures) {
      failures.push(`${label}：${message}`);
    }
    if (failures.length) {
      throw new Error([...new Set(failures)].join("；"));
    }
  }
}

export const desktopSaveCoordinator = new DesktopSaveCoordinator();

export type SafeExitOutcome =
  | { status: "exited" }
  | { status: "failed"; message: string }
  | { status: "stale" };

export async function runSafeExit(
  requestId: number,
  operations: {
    flush: () => Promise<void>;
    complete: (requestId: number) => Promise<void>;
    isCurrent: () => boolean;
  },
): Promise<SafeExitOutcome> {
  try {
    await operations.flush();
    if (!operations.isCurrent()) return { status: "stale" };
    await operations.complete(requestId);
    if (!operations.isCurrent()) return { status: "stale" };
    return { status: "exited" };
  } catch (error) {
    if (!operations.isCurrent()) return { status: "stale" };
    return {
      status: "failed",
      message: error instanceof Error ? error.message : String(error),
    };
  }
}

export async function runForcedExit(
  requestId: number,
  confirmed: boolean,
  force: (requestId: number) => Promise<void>,
) {
  if (!confirmed) return "cancelled" as const;
  await force(requestId);
  return "forced" as const;
}

export type ShortcutRecordingAction =
  | "quickQuery"
  | "selectionExplanation";

export type ShortcutRecordingResult = {
  action: ShortcutRecordingAction;
  binding?: import("./appPreferences").ShortcutBinding;
  cancelled: boolean;
  error?: string;
};
