import type { WritingService } from "./writingService";
import {
  cloneWritingSnapshot,
  writingSnapshotsEqual,
  type WritingDocumentRecord,
  type WritingDocumentSummary,
  type WritingSnapshot,
} from "./writingViewModel.ts";

export type WritingSaveState = "saved" | "saving" | "error";

export type WritingDraftSaveCoordinatorOptions = {
  delayMs?: number;
  onSaved?: (document: WritingDocumentRecord) => void;
  onStateChange?: (
    documentId: number,
    state: WritingSaveState,
    error?: string,
  ) => void;
};

type DocumentSaveState = {
  revision: number;
  persistedSnapshot?: WritingSnapshot;
  pending?: WritingSnapshot;
  timer?: ReturnType<typeof globalThis.setTimeout>;
  inFlight?: Promise<boolean>;
  lastError?: string;
  acceptedAnalysisAdvance?: {
    fromRevision: number;
    toRevision: number;
    baseSnapshot: WritingSnapshot;
  };
  reconciliation?: {
    expectedRevision: number;
    baseSnapshot?: WritingSnapshot;
    snapshot: WritingSnapshot;
    saveError: string;
  };
};

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export class WritingDraftSaveCoordinator {
  private readonly service: Pick<
    WritingService,
    "saveDraft" | "loadDocument"
  >;
  private readonly delayMs: number;
  private readonly onSaved?: (document: WritingDocumentRecord) => void;
  private readonly onStateChange?: WritingDraftSaveCoordinatorOptions["onStateChange"];
  private readonly states = new Map<number, DocumentSaveState>();
  private disposed = false;

  constructor(
    service: Pick<WritingService, "saveDraft" | "loadDocument">,
    options: WritingDraftSaveCoordinatorOptions = {},
  ) {
    this.service = service;
    this.delayMs = options.delayMs ?? 420;
    this.onSaved = options.onSaved;
    this.onStateChange = options.onStateChange;
  }

  register(document: WritingDocumentSummary) {
    const state = this.states.get(document.id);
    if (state?.inFlight || state?.pending) {
      return;
    }
    this.states.set(document.id, {
      revision: document.revision,
      persistedSnapshot: document.draftSnapshot
        ? cloneWritingSnapshot(document.draftSnapshot)
        : undefined,
    });
  }

  currentRevision(documentId: number) {
    return this.states.get(documentId)?.revision;
  }

  schedule(documentId: number, snapshot: WritingSnapshot) {
    const state = this.requireState(documentId);
    state.pending = cloneWritingSnapshot(snapshot);
    state.lastError = undefined;
    if (state.timer !== undefined) {
      globalThis.clearTimeout(state.timer);
    }
    this.emitState(documentId, "saving");
    state.timer = globalThis.setTimeout(() => {
      state.timer = undefined;
      void this.drain(documentId);
    }, this.delayMs);
  }

  async flush(documentId: number) {
    const state = this.requireState(documentId);
    if (state.timer !== undefined) {
      globalThis.clearTimeout(state.timer);
      state.timer = undefined;
    }
    return this.drain(documentId);
  }

  async flushAll() {
    const failures: string[] = [];
    for (const [documentId, state] of this.states) {
      if (!state.pending && !state.inFlight) continue;
      try {
        const saved = await this.flush(documentId);
        if (!saved) {
          failures.push(
            `文章 ${documentId}：${state.lastError ?? "草稿仍未保存。"}`,
          );
        }
      } catch (error) {
        failures.push(
          `文章 ${documentId}：${error instanceof Error ? error.message : String(error)}`,
        );
      }
    }
    if (failures.length) {
      throw new Error(failures.join("；"));
    }
  }

  async retry(documentId: number) {
    const state = this.requireState(documentId);
    state.lastError = undefined;
    if (state.reconciliation) {
      const reconciled = await this.reconcile(
        documentId,
        state,
        state.reconciliation,
      );
      if (
        reconciled === "committed" ||
        reconciled === "rebased"
      ) {
        return this.drain(documentId);
      }
      if (reconciled === "safe-to-retry") {
        return this.drain(documentId);
      }
      return false;
    }
    return this.drain(documentId);
  }

  async acceptAuthoritative(document: WritingDocumentSummary) {
    const state = this.requireState(document.id);
    if (document.revision < state.revision) {
      return true;
    }
    const nextSnapshot = document.draftSnapshot
      ? cloneWritingSnapshot(document.draftSnapshot)
      : undefined;
    if (
      document.revision === state.revision &&
      state.persistedSnapshot &&
      (!nextSnapshot ||
        !writingSnapshotsEqual(
          state.persistedSnapshot,
          nextSnapshot,
        ))
    ) {
      throw new Error(
        "同一 revision 返回了不同正文，权威结果已拒绝。",
      );
    }
    if (document.revision > state.revision) {
      if (
        document.revision !== state.revision + 1 ||
        !state.persistedSnapshot ||
        !nextSnapshot ||
        !writingSnapshotsEqual(state.persistedSnapshot, nextSnapshot)
      ) {
        throw new Error(
          "分析结果未证明正文保持不变，不能据此推进自动保存 revision。",
        );
      }
      state.acceptedAnalysisAdvance = {
        fromRevision: state.revision,
        toRevision: document.revision,
        baseSnapshot: cloneWritingSnapshot(state.persistedSnapshot),
      };
    }
    state.revision = document.revision;
    state.persistedSnapshot = nextSnapshot;
    if (
      state.reconciliation &&
      this.canRebaseAfterAcceptedAnalysis(
        state.reconciliation,
        document,
        state.acceptedAnalysisAdvance,
      )
    ) {
      state.pending ??= cloneWritingSnapshot(
        state.reconciliation.snapshot,
      );
      state.reconciliation = undefined;
      state.lastError = undefined;
      this.emitState(document.id, "saving");
    }
    const inFlight = state.inFlight;
    if (inFlight) {
      await inFlight;
    }
    if (state.pending && !state.reconciliation) {
      return this.drain(document.id);
    }
    return true;
  }

  forget(documentId: number) {
    const state = this.states.get(documentId);
    if (state?.timer !== undefined) {
      globalThis.clearTimeout(state.timer);
    }
    this.states.delete(documentId);
  }

  dispose() {
    this.disposed = true;
    const pendingSaves: Promise<boolean>[] = [];
    for (const [documentId, state] of this.states) {
      if (state.timer !== undefined) {
        globalThis.clearTimeout(state.timer);
        state.timer = undefined;
      }
      if (state.pending || state.inFlight) {
        pendingSaves.push(this.drain(documentId));
      }
    }
    return Promise.allSettled(pendingSaves);
  }

  private requireState(documentId: number) {
    const state = this.states.get(documentId);
    if (!state) {
      throw new Error(`写作文章 ${documentId} 尚未注册自动保存版本。`);
    }
    return state;
  }

  private drain(documentId: number): Promise<boolean> {
    const state = this.requireState(documentId);
    if (state.inFlight) {
      return state.inFlight;
    }
    state.inFlight = (async () => {
      if (state.reconciliation) {
        const reconciled = await this.reconcile(
          documentId,
          state,
          state.reconciliation,
        );
        if (reconciled === "blocked") {
          return false;
        }
      }
      while (state.pending) {
        const snapshot = state.pending;
        state.pending = undefined;
        const expectedRevision = state.revision;
        const baseSnapshot = state.persistedSnapshot
          ? cloneWritingSnapshot(state.persistedSnapshot)
          : undefined;
        try {
          const document = await this.service.saveDraft(
            documentId,
            expectedRevision,
            snapshot,
          );
          if (document.id !== documentId) {
            throw new Error("自动保存返回了其他文章，结果已拒绝。");
          }
          if (document.revision <= expectedRevision) {
            throw new Error("自动保存返回的 revision 未前进，结果已拒绝。");
          }
          state.revision = document.revision;
          state.persistedSnapshot = document.draftSnapshot
            ? cloneWritingSnapshot(document.draftSnapshot)
            : undefined;
          if (
            state.acceptedAnalysisAdvance &&
            document.revision >
              state.acceptedAnalysisAdvance.toRevision
          ) {
            state.acceptedAnalysisAdvance = undefined;
          }
          state.reconciliation = undefined;
          state.lastError = undefined;
          if (!this.disposed) {
            this.onSaved?.(document);
          }
        } catch (error) {
          const reconciliation = {
            expectedRevision,
            baseSnapshot,
            snapshot: cloneWritingSnapshot(snapshot),
            saveError: errorMessage(error),
          };
          state.reconciliation = reconciliation;
          const reconciled = await this.reconcile(
            documentId,
            state,
            reconciliation,
          );
          if (
            reconciled === "committed" ||
            reconciled === "rebased"
          ) {
            continue;
          }
          state.pending ??= snapshot;
          return false;
        }
      }
      this.emitState(documentId, "saved");
      return true;
    })().finally(() => {
      state.inFlight = undefined;
    });
    return state.inFlight;
  }

  private async reconcile(
    documentId: number,
    state: DocumentSaveState,
    attempt: NonNullable<DocumentSaveState["reconciliation"]>,
  ): Promise<
    "committed" | "rebased" | "safe-to-retry" | "blocked"
  > {
    let remote: WritingDocumentRecord;
    try {
      remote = await this.service.loadDocument(documentId);
    } catch (error) {
      state.reconciliation = attempt;
      state.lastError =
        `自动保存结果无法确认：${attempt.saveError}；` +
        `读回数据库也失败：${errorMessage(error)}。请重试对账。`;
      this.emitState(documentId, "error", state.lastError);
      return "blocked";
    }
    if (remote.id !== documentId) {
      state.reconciliation = attempt;
      state.lastError = "自动保存对账返回了其他文章，已停止重试。";
      this.emitState(documentId, "error", state.lastError);
      return "blocked";
    }
    if (
      remote.revision > attempt.expectedRevision &&
      remote.draftSnapshot &&
      writingSnapshotsEqual(remote.draftSnapshot, attempt.snapshot)
    ) {
      state.revision = remote.revision;
      state.persistedSnapshot = cloneWritingSnapshot(
        remote.draftSnapshot,
      );
      state.acceptedAnalysisAdvance = undefined;
      state.reconciliation = undefined;
      state.lastError = undefined;
      if (!this.disposed) {
        this.onSaved?.(remote);
      }
      return "committed";
    }
    if (
      this.canRebaseAfterAcceptedAnalysis(
        attempt,
        remote,
        state.acceptedAnalysisAdvance,
      )
    ) {
      state.revision = remote.revision;
      state.persistedSnapshot = cloneWritingSnapshot(
        remote.draftSnapshot!,
      );
      state.reconciliation = undefined;
      state.pending ??= cloneWritingSnapshot(attempt.snapshot);
      state.lastError = undefined;
      return "rebased";
    }
    if (
      remote.revision === attempt.expectedRevision &&
      remote.draftSnapshot &&
      attempt.baseSnapshot &&
      writingSnapshotsEqual(
        remote.draftSnapshot,
        attempt.baseSnapshot,
      )
    ) {
      state.revision = remote.revision;
      state.persistedSnapshot = cloneWritingSnapshot(
        remote.draftSnapshot,
      );
      state.reconciliation = undefined;
      state.pending ??= cloneWritingSnapshot(attempt.snapshot);
      state.lastError = attempt.saveError;
      this.emitState(documentId, "error", state.lastError);
      return "safe-to-retry";
    }

    state.reconciliation = attempt;
    state.lastError =
      "自动保存发生正文冲突：数据库 revision 或内容已由其他结果推进，" +
      "已保留当前正文且不会用旧 revision 覆盖。";
    this.emitState(documentId, "error", state.lastError);
    return "blocked";
  }

  private canRebaseAfterAcceptedAnalysis(
    attempt: NonNullable<DocumentSaveState["reconciliation"]>,
    remote: WritingDocumentSummary,
    advance: DocumentSaveState["acceptedAnalysisAdvance"],
  ) {
    return Boolean(
      advance &&
        attempt.baseSnapshot &&
        remote.draftSnapshot &&
        advance.fromRevision === attempt.expectedRevision &&
        advance.toRevision === remote.revision &&
        writingSnapshotsEqual(
          attempt.baseSnapshot,
          advance.baseSnapshot,
        ) &&
        writingSnapshotsEqual(
          remote.draftSnapshot,
          advance.baseSnapshot,
        ),
    );
  }

  private emitState(
    documentId: number,
    state: WritingSaveState,
    error?: string,
  ) {
    if (!this.disposed) {
      this.onStateChange?.(documentId, state, error);
    }
  }
}
