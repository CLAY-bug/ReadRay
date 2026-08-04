import type { AppPreferences } from "./appPreferences";

export type AppPreferenceSaveOutcome =
  | {
      status: "saved";
      preferences: AppPreferences;
    }
  | {
      status: "failed";
      preferences: AppPreferences;
      retryPreferences: AppPreferences;
      message: string;
    }
  | {
      status: "superseded";
    };

type AppPreferenceSaveOperations = {
  save: (preferences: AppPreferences) => Promise<AppPreferences>;
  load: () => Promise<AppPreferences>;
  apply: (preferences: AppPreferences) => void;
};

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export class AppPreferenceSaveCoordinator {
  private generation = 0;
  private pendingRequests = 0;
  private readonly activeSaves = new Set<Promise<AppPreferenceSaveOutcome>>();
  private readonly operations: AppPreferenceSaveOperations;

  constructor(operations: AppPreferenceSaveOperations) {
    this.operations = operations;
  }

  get isSaving() {
    return this.pendingRequests > 0;
  }

  dispose() {
    this.generation += 1;
  }

  save(
    candidate: AppPreferences,
    previousAuthority: AppPreferences,
  ): Promise<AppPreferenceSaveOutcome> {
    const active = this.runSave(candidate, previousAuthority);
    this.activeSaves.add(active);
    void active.finally(() => this.activeSaves.delete(active));
    return active;
  }

  async flush() {
    const failures: string[] = [];
    while (this.activeSaves.size) {
      const outcomes = await Promise.all([...this.activeSaves]);
      for (const outcome of outcomes) {
        if (outcome.status === "failed") failures.push(outcome.message);
      }
    }
    if (failures.length) {
      throw new Error([...new Set(failures)].join("；"));
    }
  }

  private async runSave(
    candidate: AppPreferences,
    previousAuthority: AppPreferences,
  ): Promise<AppPreferenceSaveOutcome> {
    const generation = this.generation + 1;
    this.generation = generation;
    this.pendingRequests += 1;

    try {
      try {
        this.operations.apply(candidate);
        const saved = await this.operations.save(candidate);
        if (generation !== this.generation) {
          return { status: "superseded" };
        }
        this.operations.apply(saved);
        return { status: "saved", preferences: saved };
      } catch (error) {
        return await this.rollback(
          generation,
          candidate,
          previousAuthority,
          error,
        );
      }
    } finally {
      this.pendingRequests -= 1;
    }
  }

  private async rollback(
    generation: number,
    candidate: AppPreferences,
    previousAuthority: AppPreferences,
    error: unknown,
  ): Promise<AppPreferenceSaveOutcome> {
    if (generation !== this.generation) {
      return { status: "superseded" };
    }

    let authority = previousAuthority;
    let reloadError: string | undefined;
    try {
      authority = await this.operations.load();
    } catch (readError) {
      reloadError = errorMessage(readError);
    }
    if (generation !== this.generation) {
      return { status: "superseded" };
    }

    let restoreError: string | undefined;
    try {
      this.operations.apply(authority);
    } catch (applyError) {
      restoreError = errorMessage(applyError);
    }

    return {
      status: "failed",
      preferences: authority,
      retryPreferences: { ...candidate, revision: authority.revision },
      message: `保存失败，已恢复数据库设置：${errorMessage(error)}${
        reloadError ? `；重新读取失败：${reloadError}` : ""
      }${restoreError ? `；重新应用失败：${restoreError}` : ""}`,
    };
  }
}
