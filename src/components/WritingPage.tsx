import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent,
  type PointerEvent,
} from "react";
import {
  WritingDraftSaveCoordinator,
  type WritingSaveState,
} from "../writingDraftSaveCoordinator";
import { desktopSaveCoordinator } from "../desktopLifecycle";
import { loadWritingLibrary } from "../writingLibraryLoader";
import {
  captureWritingRequestIdentity,
  runGuardedWritingRequest,
  shouldHandleWritingShortcut,
} from "../writingRequestIdentity";
import {
  createWritingReviewTargetState,
  LatestWritingRequestSequence,
} from "../writingReviewState";
import type { WritingService } from "../writingService";
import {
  cloneWritingSnapshot,
  countWritingWords,
  emptyWritingSnapshot,
  getRecordSnapshot,
  getRecordStatus,
  normalizeWritingText,
  writingSnapshotsEqual,
  type WritingDocumentRecord,
  type WritingDocumentStatus,
  type WritingDocumentSummary,
  type WritingMode,
  type WritingSnapshot,
} from "../writingViewModel";
import WritingCoach, {
  type CoachIssueState,
  type WritingAgentQuestion,
  type WritingAgentRequest,
  type WritingSelectionAction,
} from "./WritingCoach";
import WritingCompareView from "./WritingCompareView";
import WritingEditor, {
  type WritingEditorHandle,
  type WritingSelection,
} from "./WritingEditor";
import WritingLibrary from "./WritingLibrary";
import type { SendShortcut } from "../appPreferences";

type WritingPageProps = {
  hidden?: boolean;
  libraryRequest: number;
  service: WritingService | null;
  onWindowTitleChange: (title: string) => void;
  sendShortcut: SendShortcut;
};

type OperationError = {
  message: string;
  actionLabel: string;
};

const WRITING_ASSIST_AUTO_COLLAPSE_WIDTH = 1120;
const WRITING_EDITOR_DEFAULT_WIDTH = 736;
const WRITING_EDITOR_MIN_WIDTH = 520;
const WRITING_EDITOR_MAX_WIDTH = 960;
const WRITING_COACH_MIN_WIDTH = 320;
const WRITING_SPLITTER_COLUMN_WIDTH = 24;

function getWritingEditorWidthBounds(grid: HTMLElement) {
  const availableMax = Math.floor(
    grid.getBoundingClientRect().width -
      WRITING_SPLITTER_COLUMN_WIDTH -
      WRITING_COACH_MIN_WIDTH,
  );
  const max = Math.max(
    WRITING_EDITOR_MIN_WIDTH,
    Math.min(WRITING_EDITOR_MAX_WIDTH, availableMax),
  );
  return {
    min: Math.min(WRITING_EDITOR_MIN_WIDTH, max),
    max,
  };
}

function getWritingEditorColumnWidth(grid: HTMLElement) {
  return (
    grid
      .querySelector<HTMLElement>(".rr-writing-editor-column")
      ?.getBoundingClientRect().width ?? WRITING_EDITOR_DEFAULT_WIDTH
  );
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function formatDocumentTime(value: number | undefined, prefix: string) {
  const date = new Date(value ?? Date.now());
  const parts = new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).formatToParts(date);
  const part = (type: Intl.DateTimeFormatPartTypes) =>
    parts.find((item) => item.type === type)?.value ?? "";
  return `${prefix}${part("month")} 月 ${part("day")} 日 ${part(
    "hour",
  )}:${part("minute")}`;
}

function toSummary(document: WritingDocumentRecord): WritingDocumentSummary {
  const {
    comparisonBaseline: _comparisonBaseline,
    comparisonBaselineRevision: _comparisonBaselineRevision,
    versions: _versions,
    activeAnalysis: _activeAnalysis,
    baselineAnalysis: _baselineAnalysis,
    answers: _answers,
    ...summary
  } = document;
  return summary;
}

function replaceSummary(
  records: WritingDocumentSummary[],
  next: WritingDocumentSummary,
) {
  return [
    next,
    ...records.filter((record) => record.id !== next.id),
  ].sort(
    (first, second) =>
      second.updatedAtUnixMs - first.updatedAtUnixMs ||
      second.id - first.id,
  );
}

function WritingPage({
  hidden = false,
  libraryRequest,
  service,
  onWindowTitleChange,
  sendShortcut,
}: WritingPageProps) {
  const [records, setRecords] = useState<WritingDocumentSummary[]>([]);
  const [libraryStatus, setLibraryStatus] = useState<
    "loading" | "ready" | "error"
  >("loading");
  const [libraryError, setLibraryError] = useState<string>();
  const [libraryQuery, setLibraryQuery] = useState("");
  const [libraryRetryToken, setLibraryRetryToken] = useState(0);
  const [mode, setMode] = useState<WritingMode>("library");
  const [activeDocument, setActiveDocument] =
    useState<WritingDocumentRecord | null>(null);
  const activeDocumentRef = useRef<WritingDocumentRecord | null>(null);
  const [snapshot, setSnapshot] = useState<WritingSnapshot>(
    emptyWritingSnapshot,
  );
  const snapshotRef = useRef(snapshot);
  const [comparisonBaseline, setComparisonBaseline] =
    useState<WritingSnapshot>(emptyWritingSnapshot);
  const [selectedVersionId, setSelectedVersionId] = useState<number>();
  const selectedVersionIdRef = useRef<number | undefined>(
    undefined,
  );
  const localEditGenerationRef = useRef(0);
  const hiddenRef = useRef(hidden);
  const [resetKey, setResetKey] = useState(0);
  const [saveState, setSaveState] =
    useState<WritingSaveState>("saved");
  const [saveError, setSaveError] = useState<string>();
  const [checking, setChecking] = useState(false);
  const [checkingLabel, setCheckingLabel] = useState("检查中…");
  const [checkingStopped, setCheckingStopped] = useState(false);
  const checkingStoppedRef = useRef(false);
  const [busyAction, setBusyAction] = useState<
    "create" | "open" | "complete" | "continue" | undefined
  >();
  const [deletingDocumentId, setDeletingDocumentId] = useState<number>();
  const [issueStates, setIssueStates] = useState<
    Record<string, CoachIssueState>
  >({});
  const [activeIssueId, setActiveIssueId] = useState<string | null>(null);
  const [editingIssueId, setEditingIssueId] = useState<string | null>(null);
  const [compareOrigin, setCompareOrigin] =
    useState<"review" | "completed">("review");
  const [documentSwitcherOpen, setDocumentSwitcherOpen] = useState(false);
  const [selection, setSelection] = useState<WritingSelection | null>(null);
  const [assistOpen, setAssistOpen] = useState(false);
  const [responsiveCoachCollapsed, setResponsiveCoachCollapsed] =
    useState(false);
  const [editorColumnWidth, setEditorColumnWidth] = useState<number | null>(
    null,
  );
  const [writingResizing, setWritingResizing] = useState(false);
  const [agentRequest, setAgentRequest] = useState<WritingAgentRequest>({
    token: 0,
    intent: "ask",
  });
  const [operationError, setOperationError] =
    useState<OperationError>();
  const operationRetryRef = useRef<(() => void) | undefined>(undefined);
  const editorRef = useRef<WritingEditorHandle>(null);
  const documentSwitcherRef = useRef<HTMLDivElement | null>(null);
  const writingPageRef = useRef<HTMLElement | null>(null);
  const writingGridRef = useRef<HTMLDivElement | null>(null);
  const writingResizeRef = useRef<{
    startX: number;
    startWidth: number;
  } | null>(null);
  const responsiveCoachNarrowRef = useRef(false);
  const saveCoordinatorRef =
    useRef<WritingDraftSaveCoordinator | null>(null);
  const operationTokenRef = useRef(0);
  const questionSequenceRef = useRef(
    new LatestWritingRequestSequence(),
  );

  const selectedVersion = activeDocument?.versions.find(
    (version) => version.id === selectedVersionId,
  );
  const reviewTarget = useMemo(
    () =>
      activeDocument
        ? createWritingReviewTargetState(
            activeDocument,
            selectedVersionId,
          )
        : undefined,
    [activeDocument, selectedVersionId],
  );
  const visibleAnalysis =
    selectedVersionId === undefined
      ? activeDocument?.activeAnalysis ??
        activeDocument?.baselineAnalysis
      : undefined;
  const issues = reviewTarget?.issues ?? [];
  const patterns = reviewTarget?.patterns ?? [];
  const visibleAnswers = reviewTarget?.answers ?? [];
  const revisionFromCompleted = Boolean(
    activeDocument?.draftSnapshot && activeDocument.completedSnapshot,
  );
  const hasChanges = !writingSnapshotsEqual(snapshot, comparisonBaseline);
  const isBlank =
    !snapshot.title &&
    !snapshot.paragraphs.some((paragraph) =>
      normalizeWritingText(paragraph),
    );

  useEffect(() => {
    snapshotRef.current = snapshot;
  }, [snapshot]);

  useEffect(() => {
    if (hidden && !hiddenRef.current) {
      localEditGenerationRef.current += 1;
    }
    hiddenRef.current = hidden;
  }, [hidden]);

  useEffect(() => {
    if (!service) {
      saveCoordinatorRef.current = null;
      return;
    }
    const coordinator = new WritingDraftSaveCoordinator(service, {
      onSaved: (document) => {
        setRecords((current) =>
          replaceSummary(current, toSummary(document)),
        );
        if (activeDocumentRef.current?.id === document.id) {
          activeDocumentRef.current = {
            ...document,
            draftSnapshot: cloneWritingSnapshot(snapshotRef.current),
          };
          setActiveDocument(activeDocumentRef.current);
        }
      },
      onStateChange: (documentId, state, nextError) => {
        if (activeDocumentRef.current?.id === documentId) {
          setSaveState(state);
          setSaveError(nextError);
        }
      },
    });
    saveCoordinatorRef.current = coordinator;
    const unregisterExitFlusher = desktopSaveCoordinator.register({
      label: "写作草稿",
      flush: () => coordinator.flushAll(),
    });
    if (activeDocumentRef.current) {
      coordinator.register(activeDocumentRef.current);
    }
    return () => {
      unregisterExitFlusher();
      if (saveCoordinatorRef.current === coordinator) {
        saveCoordinatorRef.current = null;
      }
      void coordinator.dispose();
    };
  }, [service]);

  useEffect(() => {
    let ignore = false;
    if (!service) {
      setLibraryStatus("loading");
      return;
    }
    void loadWritingLibrary(service, libraryQuery, (state) => {
      if (ignore) {
        return;
      }
      setLibraryStatus(state.status);
      setRecords(state.records);
      setLibraryError(
        state.status === "error" ? state.error : undefined,
      );
    }).catch(() => undefined);
    return () => {
      ignore = true;
    };
  }, [libraryQuery, libraryRetryToken, service]);

  useEffect(() => {
    const labels: Record<WritingMode, string> = {
      draft: revisionFromCompleted ? "写作 · 修改中" : "写作 · 草稿",
      review: "写作 · 检查",
      compare: "写作 · 对比",
      completed: "写作 · 已完成",
      library: "写作",
    };
    onWindowTitleChange(labels[mode]);
  }, [mode, onWindowTitleChange, revisionFromCompleted]);

  const handleLibrarySearch = useCallback((query: string) => {
    setLibraryQuery(query);
  }, []);

  async function flushActiveDraft() {
    const current = activeDocumentRef.current;
    if (!current?.draftSnapshot) {
      return true;
    }
    return (
      (await saveCoordinatorRef.current?.flush(current.id)) ?? true
    );
  }

  function currentVisibleIdentity(documentId: number) {
    const document = activeDocumentRef.current;
    const revision =
      saveCoordinatorRef.current?.currentRevision(documentId);
    if (!document || document.id !== documentId || revision === undefined) {
      throw new Error("当前文章已切换，模型结果已取消。");
    }
    return captureWritingRequestIdentity({
      documentId,
      revision,
      generation: localEditGenerationRef.current,
      snapshot: snapshotRef.current,
      versionId: selectedVersionIdRef.current,
    });
  }

  function showOperationError(
    error: unknown,
    retry: () => void,
    actionLabel = "重试",
  ) {
    setOperationError({
      message: errorMessage(error),
      actionLabel,
    });
    operationRetryRef.current = retry;
  }

  function clearOperationError() {
    setOperationError(undefined);
    operationRetryRef.current = undefined;
  }

  function resetReviewState(
    document: WritingDocumentRecord,
    versionId?: number,
  ) {
    const target = createWritingReviewTargetState(
      document,
      versionId,
    );
    setIssueStates(target.issueStates);
    setActiveIssueId(target.activeIssueId);
    setEditingIssueId(null);
  }

  function applyDocument(
    document: WritingDocumentRecord,
    status: WritingDocumentStatus,
  ) {
    localEditGenerationRef.current += 1;
    questionSequenceRef.current.invalidate();
    setChecking(false);
    setCheckingStopped(false);
    checkingStoppedRef.current = false;
    saveCoordinatorRef.current?.register(document);
    activeDocumentRef.current = document;
    setActiveDocument(document);
    setRecords((current) =>
      replaceSummary(current, toSummary(document)),
    );
    const nextSnapshot =
      status === "draft"
        ? cloneWritingSnapshot(
            document.draftSnapshot ?? emptyWritingSnapshot,
          )
        : cloneWritingSnapshot(
            document.completedSnapshot ?? emptyWritingSnapshot,
          );
    const latestVersion =
      document.versions[document.versions.length - 1];
    const nextBaseline =
      status === "completed"
        ? cloneWritingSnapshot(
            latestVersion?.comparisonBaseline ??
              document.comparisonBaseline,
          )
        : cloneWritingSnapshot(document.comparisonBaseline);
    setSnapshot(nextSnapshot);
    snapshotRef.current = nextSnapshot;
    setComparisonBaseline(nextBaseline);
    const nextVersionId =
      status === "completed" ? latestVersion?.id : undefined;
    selectedVersionIdRef.current = nextVersionId;
    setSelectedVersionId(nextVersionId);
    setResetKey((key) => key + 1);
    setSaveState("saved");
    setSaveError(undefined);
    setSelection(null);
    setAssistOpen(false);
    setDocumentSwitcherOpen(false);
    resetReviewState(document, nextVersionId);
    setMode(status === "completed" ? "completed" : "draft");
    clearOperationError();
  }

  async function openRecord(
    record: WritingDocumentSummary,
    status: WritingDocumentStatus,
  ) {
    if (!service || busyAction) {
      return;
    }
    if (!(await flushActiveDraft())) {
      showOperationError(
        new Error(
          "当前文章尚未保存，已停止切换。请先重试保存，避免正文丢失。",
        ),
        () => void openRecord(record, status),
      );
      return;
    }
    const token = ++operationTokenRef.current;
    setBusyAction("open");
    clearOperationError();
    try {
      const document = await service.loadDocument(record.id);
      if (token === operationTokenRef.current) {
        applyDocument(document, status);
      }
    } catch (error) {
      if (token === operationTokenRef.current) {
        showOperationError(error, () => void openRecord(record, status));
      }
    } finally {
      if (token === operationTokenRef.current) {
        setBusyAction(undefined);
      }
    }
  }

  async function createNewDraft() {
    if (!service || busyAction) {
      return;
    }
    if (!(await flushActiveDraft())) {
      showOperationError(
        new Error(
          "当前文章尚未保存，已停止新建。请先重试保存，避免正文丢失。",
        ),
        () => void createNewDraft(),
      );
      return;
    }
    const token = ++operationTokenRef.current;
    setBusyAction("create");
    clearOperationError();
    try {
      const document = await service.createDocument();
      if (token === operationTokenRef.current) {
        applyDocument(document, "draft");
        window.requestAnimationFrame(() =>
          documentQuery(".rr-writing-editor-title")?.focus(),
        );
      }
    } catch (error) {
      if (token === operationTokenRef.current) {
        showOperationError(error, () => void createNewDraft());
      }
    } finally {
      if (token === operationTokenRef.current) {
        setBusyAction(undefined);
      }
    }
  }

  async function showLibrary() {
    if (!(await flushActiveDraft())) {
      showOperationError(
        new Error(
          "当前文章尚未保存，已停止离开。请先重试保存，避免正文丢失。",
        ),
        () => void showLibrary(),
      );
      return;
    }
    localEditGenerationRef.current += 1;
    questionSequenceRef.current.invalidate();
    selectedVersionIdRef.current = undefined;
    setDocumentSwitcherOpen(false);
    setAssistOpen(false);
    setMode("library");
    setLibraryRetryToken((token) => token + 1);
  }

  useEffect(() => {
    if (libraryRequest > 0) {
      void showLibrary();
    }
  }, [libraryRequest]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (!shouldHandleWritingShortcut(hidden, mode, event)) {
        return;
      }
      if (
        event.ctrlKey &&
        event.key.toLowerCase() === "j" &&
        ["draft", "review", "completed"].includes(mode)
      ) {
        event.preventDefault();
        if (assistOpen) {
          setAssistOpen(false);
        } else {
          setAgentRequest((request) => ({
            ...request,
            token: request.token + 1,
            intent: "ask",
            selectionText: undefined,
            action: undefined,
          }));
          setAssistOpen(true);
        }
      }
      if (event.key === "Escape") {
        setSelection(null);
        setDocumentSwitcherOpen(false);
        setAssistOpen(false);
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [assistOpen, hidden, mode]);

  useEffect(() => {
    if (!documentSwitcherOpen) {
      return;
    }

    function handlePointerDown(event: Event) {
      const target = event.target;
      if (
        !(target instanceof Node) ||
        !documentSwitcherRef.current?.contains(target)
      ) {
        setDocumentSwitcherOpen(false);
      }
    }

    document.addEventListener("pointerdown", handlePointerDown);
    return () =>
      document.removeEventListener("pointerdown", handlePointerDown);
  }, [documentSwitcherOpen]);

  useEffect(() => {
    if (hidden || mode === "library" || mode === "compare") {
      responsiveCoachNarrowRef.current = false;
      return;
    }
    const page = writingPageRef.current;
    if (!page || typeof ResizeObserver === "undefined") {
      return;
    }

    const applyResponsiveWidth = (width: number) => {
      const isNarrow = width <= WRITING_ASSIST_AUTO_COLLAPSE_WIDTH;
      if (!isNarrow) {
        responsiveCoachNarrowRef.current = false;
        const grid = writingGridRef.current;
        if (grid) {
          setEditorColumnWidth((currentWidth) => {
            if (currentWidth === null) {
              return currentWidth;
            }
            const bounds = getWritingEditorWidthBounds(grid);
            const nextWidth = Math.round(
              Math.min(bounds.max, Math.max(bounds.min, currentWidth)),
            );
            return nextWidth === currentWidth ? currentWidth : nextWidth;
          });
        }
        setResponsiveCoachCollapsed(false);
        return;
      }
      if (responsiveCoachNarrowRef.current) {
        return;
      }
      responsiveCoachNarrowRef.current = true;
      setResponsiveCoachCollapsed(true);
      setAssistOpen(false);
    };

    const observer = new ResizeObserver(([entry]) => {
      if (entry) {
        applyResponsiveWidth(entry.contentRect.width);
      }
    });
    observer.observe(page);
    applyResponsiveWidth(page.getBoundingClientRect().width);
    return () => {
      observer.disconnect();
      responsiveCoachNarrowRef.current = false;
    };
  }, [hidden, mode]);

  const handleWritingResizePointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) {
        return;
      }
      const page = writingPageRef.current;
      const grid = writingGridRef.current;
      if (
        !page ||
        !grid ||
        page.getBoundingClientRect().width <=
          WRITING_ASSIST_AUTO_COLLAPSE_WIDTH
      ) {
        return;
      }
      const bounds = getWritingEditorWidthBounds(grid);
      const currentWidth = Math.min(
        bounds.max,
        Math.max(bounds.min, getWritingEditorColumnWidth(grid)),
      );
      writingResizeRef.current = {
        startX: event.clientX,
        startWidth: currentWidth,
      };
      setWritingResizing(true);
      event.currentTarget.setPointerCapture(event.pointerId);
    },
    [],
  );

  const handleWritingResizePointerMove = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      const state = writingResizeRef.current;
      const grid = writingGridRef.current;
      if (!state || !grid) {
        return;
      }
      const bounds = getWritingEditorWidthBounds(grid);
      const nextWidth = Math.round(
        Math.min(
          bounds.max,
          Math.max(
            bounds.min,
            state.startWidth + (event.clientX - state.startX),
          ),
        ),
      );
      setEditorColumnWidth(nextWidth);
    },
    [],
  );

  const handleWritingResizePointerUp = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (!writingResizeRef.current) {
        return;
      }
      writingResizeRef.current = null;
      setWritingResizing(false);
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
    },
    [],
  );

  const handleWritingResizeKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      if (
        event.key !== "ArrowLeft" &&
        event.key !== "ArrowRight" &&
        event.key !== "Home" &&
        event.key !== "End"
      ) {
        return;
      }
      const page = writingPageRef.current;
      const grid = writingGridRef.current;
      if (
        !page ||
        !grid ||
        page.getBoundingClientRect().width <=
          WRITING_ASSIST_AUTO_COLLAPSE_WIDTH
      ) {
        return;
      }
      const bounds = getWritingEditorWidthBounds(grid);
      const currentWidth = Math.min(
        bounds.max,
        Math.max(
          bounds.min,
          editorColumnWidth ?? getWritingEditorColumnWidth(grid),
        ),
      );
      const step = event.shiftKey ? 64 : 24;
      const nextWidth =
        event.key === "Home"
          ? bounds.min
          : event.key === "End"
            ? bounds.max
            : currentWidth +
              (event.key === "ArrowLeft" ? -step : step);
      event.preventDefault();
      setEditorColumnWidth(
        Math.round(Math.min(bounds.max, Math.max(bounds.min, nextWidth))),
      );
    },
    [editorColumnWidth],
  );

  function handleEditorChange(nextSnapshot: WritingSnapshot) {
    const document = activeDocumentRef.current;
    if (!document?.draftSnapshot) {
      return;
    }
    if (!desktopSaveCoordinator.recordMutation()) {
      setResetKey((key) => key + 1);
      return;
    }
    localEditGenerationRef.current += 1;
    setSnapshot(nextSnapshot);
    snapshotRef.current = nextSnapshot;
    activeDocumentRef.current = {
      ...document,
      draftSnapshot: cloneWritingSnapshot(nextSnapshot),
    };
    setActiveDocument(activeDocumentRef.current);
    setSelection(null);
    saveCoordinatorRef.current?.schedule(document.id, nextSnapshot);

    if (editingIssueId) {
      const issue = issues.find(
        (candidate) => candidate.id === editingIssueId,
      );
      const text = normalizeWritingText(
        [nextSnapshot.title, ...nextSnapshot.paragraphs].join(" "),
      );
      if (
        issue &&
        !text.includes(normalizeWritingText(issue.targetText))
      ) {
        setIssueStates((states) => ({
          ...states,
          [editingIssueId]: {
            ...states[editingIssueId],
            status: "modified",
          },
        }));
        const nextIssue = issues.find(
          (candidate) =>
            candidate.id !== editingIssueId &&
            text.includes(normalizeWritingText(candidate.targetText)) &&
            issueStates[candidate.id]?.status !== "ignored",
        );
        setActiveIssueId(nextIssue?.id ?? null);
        setEditingIssueId(null);
      }
    }
  }

  async function beginReview() {
    const document = activeDocumentRef.current;
    if (!service || !document || checking) {
      return;
    }
    if (isBlank) {
      openAgent({ intent: "start" });
      return;
    }
    setChecking(true);
    setCheckingLabel("检查中…");
    setCheckingStopped(false);
    checkingStoppedRef.current = false;
    clearOperationError();
    try {
      if (!(await flushActiveDraft())) {
        throw new Error(
          "正文保存失败，尚未调用模型。请重试保存后再检查。",
        );
      }
      const currentDocument = activeDocumentRef.current;
      const revision = saveCoordinatorRef.current?.currentRevision(
        document.id,
      );
      if (
        !currentDocument ||
        currentDocument.id !== document.id ||
        revision === undefined
      ) {
        throw new Error("当前文章已切换，检查请求已取消。");
      }
      const captured = currentVisibleIdentity(document.id);
      const analyzed = await runGuardedWritingRequest(
        captured,
        () => currentVisibleIdentity(document.id),
        service.analyzeDocument(document.id, revision, (event) => {
          if (event.type === "status") {
            setCheckingLabel(event.label);
          } else if (event.type === "stopped") {
            checkingStoppedRef.current = true;
          }
        }),
        "写作检查",
        async (result) => {
          if (activeDocumentRef.current?.id === result.id) {
            await saveCoordinatorRef.current?.acceptAuthoritative(
              result,
            );
          }
        },
      );
      activeDocumentRef.current = {
        ...analyzed,
        draftSnapshot: cloneWritingSnapshot(captured.snapshot),
      };
      setActiveDocument(activeDocumentRef.current);
      setRecords((current) =>
        replaceSummary(current, toSummary(analyzed)),
      );
      setComparisonBaseline(
        cloneWritingSnapshot(analyzed.comparisonBaseline),
      );
      resetReviewState(analyzed);
      setResetKey((key) => key + 1);
      setAssistOpen(false);
      setMode("review");
    } catch (error) {
      if (checkingStoppedRef.current) {
        // 用户主动放弃：诚实提示可重新检查，不弹阻塞式重试对话框。
        setOperationError(undefined);
        operationRetryRef.current = undefined;
        setCheckingStopped(true);
      } else {
        showOperationError(error, () => void beginReview());
      }
    } finally {
      setChecking(false);
    }
  }

  function abandonCheck() {
    const document = activeDocumentRef.current;
    if (!service || !document || !checking) {
      return;
    }
    // 触发 Rust 侧中止标志；等待 invoke 以 Stopped 收尾。
    void service.abortAnalysis(document.id).catch(() => {
      /* 放弃不等待确认 */
    });
  }

  function openAgent({
    intent = "ask",
    selectionText,
    action,
  }: {
    intent?: WritingAgentRequest["intent"];
    selectionText?: string;
    action?: WritingSelectionAction;
  } = {}) {
    setSelection(null);
    setResponsiveCoachCollapsed(false);
    setAgentRequest((request) => ({
      token: request.token + 1,
      intent,
      selectionText,
      action,
    }));
    setAssistOpen(true);
  }

  async function askAgent(request: WritingAgentQuestion) {
    const document = activeDocumentRef.current;
    if (!service || !document) {
      throw new Error("写作服务尚未准备好。");
    }
    const requestSequence = questionSequenceRef.current.begin();
    if (!(await flushActiveDraft())) {
      throw new Error(
        "正文保存失败，尚未调用模型。请重试保存后再提问。",
      );
    }
    const revision =
      saveCoordinatorRef.current?.currentRevision(document.id);
    if (
      revision === undefined ||
      activeDocumentRef.current?.id !== document.id
    ) {
      throw new Error("当前文章已切换，问题未发送。");
    }
    const captured = currentVisibleIdentity(document.id);
    const answer = await runGuardedWritingRequest(
      captured,
      () => currentVisibleIdentity(document.id),
      service.askQuestion(
        {
          documentId: document.id,
          expectedRevision: revision,
          versionId: captured.versionId,
          ...request,
        },
        // 问答经共享 Runtime 执行并提供取消/状态能力；助手面板默认保持
        // 简洁等待，这里暂不消费中间状态。
        () => {},
      ),
      "写作问答",
    );
    questionSequenceRef.current.requireCurrent(requestSequence);
    const visibleVersion = captured.versionId
      ? document.versions.find(
          (version) => version.id === captured.versionId,
        )
      : undefined;
    const targetRevision =
      visibleVersion?.sourceRevision ?? captured.revision;
    if (
      activeDocumentRef.current?.id !== answer.documentId ||
      answer.documentRevision !== targetRevision ||
      answer.versionId !== captured.versionId
    ) {
      throw new Error("回答属于其他正文版本，已拒绝显示。");
    }
    const current = activeDocumentRef.current;
    const nextDocument = {
      ...current,
      answers: [
        ...current.answers.filter(
          (candidate) => candidate.id !== answer.id,
        ),
        answer,
      ],
    };
    activeDocumentRef.current = nextDocument;
    setActiveDocument(nextDocument);
    return answer;
  }

  async function completeWriting() {
    const document = activeDocumentRef.current;
    if (!service || !document || busyAction) {
      return;
    }
    setBusyAction("complete");
    clearOperationError();
    try {
      if (!(await flushActiveDraft())) {
        throw new Error(
          "正文保存失败，尚未创建完成版本。请重试保存后再完成。",
        );
      }
      const revision =
        saveCoordinatorRef.current?.currentRevision(document.id);
      if (revision === undefined) {
        throw new Error("无法确认当前文章 revision。");
      }
      const completed = await service.completeDocument(
        document.id,
        revision,
      );
      applyDocument(completed, "completed");
    } catch (error) {
      showOperationError(error, () => void completeWriting());
    } finally {
      setBusyAction(undefined);
    }
  }

  async function continueEditing() {
    const document = activeDocumentRef.current;
    if (!service || !document || busyAction) {
      return;
    }
    if (document.draftSnapshot) {
      showOperationError(
        new Error(
          "这篇文章已有修改中草稿，完成版本不会覆盖它。",
        ),
        () => void returnToExistingDraft(),
        "回到草稿",
      );
      return;
    }
    setBusyAction("continue");
    clearOperationError();
    try {
      const continued = await service.continueEditing(
        document.id,
        document.revision,
        selectedVersionId,
      );
      applyDocument(continued, "draft");
      window.requestAnimationFrame(() =>
        editorRef.current?.focusBody(true),
      );
    } catch (error) {
      showOperationError(error, () => void continueEditing());
    } finally {
      setBusyAction(undefined);
    }
  }

  async function returnToExistingDraft() {
    const document = activeDocumentRef.current;
    if (!service || !document) {
      return;
    }
    try {
      const current = await service.loadDocument(document.id);
      if (!current.draftSnapshot) {
        throw new Error("修改中草稿已不存在，请重新加载文章列表。");
      }
      applyDocument(current, "draft");
    } catch (error) {
      showOperationError(
        error,
        () => void returnToExistingDraft(),
        "重新读取",
      );
    }
  }

  function viewVersion(versionId: number) {
    const document = activeDocumentRef.current;
    if (!document) {
      return;
    }
    const version = document.versions.find(
      (candidate) => candidate.id === versionId,
    );
    if (!version) {
      return;
    }
    localEditGenerationRef.current += 1;
    questionSequenceRef.current.invalidate();
    selectedVersionIdRef.current = version.id;
    setSelectedVersionId(version.id);
    setSnapshot(cloneWritingSnapshot(version.snapshot));
    snapshotRef.current = cloneWritingSnapshot(version.snapshot);
    setComparisonBaseline(
      cloneWritingSnapshot(version.comparisonBaseline),
    );
    const target = createWritingReviewTargetState(
      document,
      version.id,
    );
    setIssueStates(target.issueStates);
    setActiveIssueId(target.activeIssueId);
    setEditingIssueId(null);
    setSelection(null);
    setAssistOpen(false);
    setResetKey((key) => key + 1);
    setMode("completed");
  }

  async function deleteRecord(record: WritingDocumentSummary) {
    if (!service || deletingDocumentId !== undefined) {
      return;
    }
    const title = getRecordSnapshot(record).title || "未命名文章";
    if (!window.confirm(`删除“${title}”及其全部写作版本？此操作无法撤销。`)) {
      return;
    }
    setDeletingDocumentId(record.id);
    clearOperationError();
    try {
      const deleted = await service.deleteDocument(
        record.id,
        record.revision,
      );
      if (!deleted) {
        throw new Error("文章已不存在，列表将重新加载。");
      }
      saveCoordinatorRef.current?.forget(record.id);
      setRecords((current) =>
        current.filter((candidate) => candidate.id !== record.id),
      );
      if (activeDocumentRef.current?.id === record.id) {
        activeDocumentRef.current = null;
        setActiveDocument(null);
      }
    } catch (error) {
      showOperationError(error, () => void deleteRecord(record));
      setLibraryRetryToken((token) => token + 1);
    } finally {
      setDeletingDocumentId(undefined);
    }
  }

  function updateIssueState(
    issueId: string,
    update: (state: CoachIssueState) => CoachIssueState,
  ) {
    setIssueStates((states) => {
      const state = states[issueId];
      return state
        ? { ...states, [issueId]: update(state) }
        : states;
    });
  }

  const recentRecords = useMemo(
    () =>
      records
        .filter((record) => record.id !== activeDocument?.id)
        .slice(0, 4),
    [activeDocument?.id, records],
  );
  const documentTime =
    saveState === "saving"
      ? "正在自动保存…"
      : saveState === "error"
        ? "自动保存失败"
        : formatDocumentTime(
            mode === "completed"
              ? selectedVersion?.completedAtUnixMs ??
                  activeDocument?.completedAtUnixMs
              : activeDocument?.draftUpdatedAtUnixMs,
            "更新于 ",
          );

  if (mode === "library") {
    return (
      <main className="rr-writing-page is-library" hidden={hidden}>
        {operationError ? (
          <div className="rr-writing-operation-error" role="alert">
            <span>{operationError.message}</span>
            <button
              type="button"
              onClick={() => operationRetryRef.current?.()}
            >
              {operationError.actionLabel}
            </button>
          </div>
        ) : null}
        <WritingLibrary
          records={records}
          status={libraryStatus}
          error={libraryError}
          deletingDocumentId={deletingDocumentId}
          onNew={() => void createNewDraft()}
          onOpen={(record, status) =>
            void openRecord(record, status)
          }
          onDelete={(record) => void deleteRecord(record)}
          onSearch={handleLibrarySearch}
          onRetry={() =>
            setLibraryRetryToken((token) => token + 1)
          }
        />
      </main>
    );
  }

  if (mode === "compare") {
    return (
      <main className="rr-writing-page is-compare" hidden={hidden}>
        <WritingCompareView
          original={comparisonBaseline}
          current={snapshot}
          patterns={patterns}
          origin={compareOrigin}
          checking={checking}
          onBack={() =>
            setMode(
              compareOrigin === "completed" ? "completed" : "review",
            )
          }
          onCheckAgain={() => void beginReview()}
          onFinish={() => void completeWriting()}
        />
      </main>
    );
  }

  if (!activeDocument) {
    return (
      <main className="rr-writing-page" hidden={hidden}>
        <div className="rr-writing-library-empty">
          正在读取写作文章…
        </div>
      </main>
    );
  }

  return (
    <main
      ref={writingPageRef}
      hidden={hidden}
      className={`rr-writing-page is-${mode}${
        assistOpen ? " has-assist" : ""
      }${responsiveCoachCollapsed ? " is-responsive-coach-collapsed" : ""}`}
      data-testid="writing-page"
    >
      <header className="rr-writing-document-bar">
        <div className="rr-writing-document-meta">
          <div
            ref={documentSwitcherRef}
            className="rr-writing-document-switcher"
          >
            <button
              className="rr-writing-document-name"
              type="button"
              aria-expanded={documentSwitcherOpen}
              title="切换文章"
              onClick={() =>
                setDocumentSwitcherOpen((open) => !open)
              }
            >
              {snapshot.title || "未命名文章"}
            </button>
            {documentSwitcherOpen ? (
              <div className="rr-writing-document-menu">
                <button type="button" onClick={() => void showLibrary()}>
                  查看全部文章
                </button>
                <div aria-hidden="true" />
                <p>最近文章</p>
                {recentRecords.length ? (
                  recentRecords.map((record) => {
                    const status = getRecordStatus(record);
                    const entrySnapshot = getRecordSnapshot(record);
                    return (
                      <button
                        type="button"
                        key={record.id}
                        onClick={() =>
                          void openRecord(record, status)
                        }
                      >
                        <strong>
                          {entrySnapshot.title || "未命名文章"}
                        </strong>
                        <span>
                          {status === "draft" ? "修改中" : "已完成"} ·{" "}
                          {formatDocumentTime(
                            record.updatedAtUnixMs,
                            "",
                          )}
                        </span>
                      </button>
                    );
                  })
                ) : (
                  <span className="rr-writing-switcher-empty">
                    还没有其他文章。
                  </span>
                )}
              </div>
            ) : null}
          </div>
          <span>·</span>
          <span>{documentTime}</span>
          {mode === "completed" && activeDocument.versions.length > 1 ? (
            <>
              <span>·</span>
              <select
                className="rr-writing-version-select"
                value={selectedVersionId}
                aria-label="查看完成版本"
                onChange={(event) =>
                  viewVersion(Number(event.target.value))
                }
              >
                {activeDocument.versions.map((version) => (
                  <option key={version.id} value={version.id}>
                    版本 {version.ordinal}
                  </option>
                ))}
              </select>
            </>
          ) : null}
        </div>
        <div className="rr-writing-document-actions">
          <span className="rr-writing-word-count">
            {countWritingWords(snapshot)} 词
          </span>
          {mode === "review" ? (
            <button
              className="rr-writing-btn is-ghost"
              type="button"
              onClick={() => setMode("draft")}
            >
              返回草稿
            </button>
          ) : null}
          <span className="rr-writing-agent-slot">
            <button
              className={`rr-writing-btn is-ghost rr-writing-ask${
                assistOpen ? " is-active" : ""
              }`}
              type="button"
              aria-expanded={assistOpen}
              onClick={() =>
                assistOpen ? setAssistOpen(false) : openAgent()
              }
            >
              {mode === "review" && assistOpen
                ? "检查结果"
                : "问 ReadRay"}
            </button>
          </span>
          {mode === "draft" ? (
            <>
              <button
                className={`rr-writing-btn is-secondary${
                  checking ? " is-checking" : ""
                }`}
                type="button"
                title={checking ? "停止检查" : undefined}
                onClick={() => (checking ? abandonCheck() : void beginReview())}
              >
                <span
                  className="rr-writing-check-dot"
                  aria-hidden="true"
                />
                {checking ? checkingLabel : "检查文章"}
              </button>
              {checkingStopped && !checking ? (
                <span className="rr-writing-check-stopped">
                  检查已停止，可重新检查
                </span>
              ) : null}
            </>
          ) : null}
          {mode === "review" ? (
            <button
              className="rr-writing-btn is-secondary"
              type="button"
              disabled={!hasChanges}
              title={
                hasChanges
                  ? "查看初稿与当前稿"
                  : "完成至少一处修改后可查看对比"
              }
              onClick={() => {
                setCompareOrigin("review");
                setMode("compare");
              }}
            >
              查看对比
            </button>
          ) : null}
          {mode === "completed" ? (
            <>
              <button
                className="rr-writing-btn is-ghost"
                type="button"
                onClick={() => {
                  setCompareOrigin("completed");
                  setMode("compare");
                }}
              >
                查看修改
              </button>
              <button
                className="rr-writing-btn is-secondary"
                type="button"
                disabled={busyAction === "continue"}
                onClick={() => void continueEditing()}
              >
                {busyAction === "continue" ? "准备中…" : "继续修改"}
              </button>
            </>
          ) : null}
        </div>
      </header>

      {saveState === "error" || operationError ? (
        <div className="rr-writing-operation-error" role="alert">
          <span>{saveError ?? operationError?.message}</span>
          <button
            type="button"
            onClick={() => {
              if (saveState === "error") {
                void saveCoordinatorRef.current?.retry(
                  activeDocument.id,
                );
              } else {
                operationRetryRef.current?.();
              }
            }}
          >
            {saveState === "error"
              ? "重试"
              : operationError?.actionLabel ?? "重试"}
          </button>
        </div>
      ) : null}

      <section
        className="rr-writing-workspace"
        aria-label="英文写作工作区"
      >
        <div
          ref={writingGridRef}
          className={`rr-writing-grid${writingResizing ? " is-resizing" : ""}`}
          style={
            editorColumnWidth === null
              ? undefined
              : ({
                  "--rr-writing-editor-column-width": `${editorColumnWidth}px`,
                } as CSSProperties)
          }
        >
          <WritingEditor
            ref={editorRef}
            mode={mode}
            snapshot={snapshot}
            issues={mode === "completed" ? [] : issues}
            patterns={patterns}
            resetKey={resetKey}
            activeIssueId={activeIssueId}
            editingIssueId={editingIssueId}
            onChange={handleEditorChange}
            onSelection={setSelection}
            onIssueSelect={(issueId) => {
              setActiveIssueId(issueId);
              if (mode === "review") {
                setAssistOpen(false);
              }
            }}
            onStartAssist={() => openAgent({ intent: "start" })}
          />
          <div
            className="rr-writing-layout-resizer"
            role="separator"
            aria-orientation="vertical"
            aria-label="调整写作编辑区宽度"
            aria-valuemin={WRITING_EDITOR_MIN_WIDTH}
            aria-valuemax={WRITING_EDITOR_MAX_WIDTH}
            aria-valuenow={Math.round(
              editorColumnWidth ?? WRITING_EDITOR_DEFAULT_WIDTH,
            )}
            tabIndex={0}
            data-testid="writing-layout-resizer"
            onPointerDown={handleWritingResizePointerDown}
            onPointerMove={handleWritingResizePointerMove}
            onPointerUp={handleWritingResizePointerUp}
            onPointerCancel={handleWritingResizePointerUp}
            onKeyDown={handleWritingResizeKeyDown}
          />
          <aside
            className="rr-writing-coach-column"
            aria-label="写作教练"
          >
            <WritingCoach
              key={reviewTarget?.targetKey}
              mode={mode}
              assistOpen={assistOpen}
              request={agentRequest}
              round={visibleAnalysis?.round ?? 0}
              issues={issues}
              answers={visibleAnswers}
              activeIssueId={activeIssueId}
              issueStates={issueStates}
              onAsk={askAgent}
              onCloseAssist={() => setAssistOpen(false)}
              onActivateIssue={setActiveIssueId}
              onReviseIssue={(issueId) => {
                setAssistOpen(false);
                setActiveIssueId(issueId);
                setEditingIssueId(issueId);
                updateIssueState(issueId, (state) => ({
                  ...state,
                  status:
                    state.status === "modified"
                      ? "editing"
                      : state.status,
                }));
                window.requestAnimationFrame(() =>
                  editorRef.current?.focusIssue(issueId),
                );
              }}
              onToggleHint={(issueId) =>
                updateIssueState(issueId, (state) => ({
                  ...state,
                  showDeeperHint: !state.showDeeperHint,
                }))
              }
              onToggleReference={(issueId) =>
                updateIssueState(issueId, (state) => ({
                  ...state,
                  showReference: !state.showReference,
                }))
              }
              onToggleIgnore={(issueId) =>
                updateIssueState(issueId, (state) => ({
                  ...state,
                  status:
                    state.status === "ignored" ? "open" : "ignored",
                }))
              }
              sendShortcut={sendShortcut}
            />
          </aside>
        </div>
      </section>

      {selection ? (
        <div
          className="rr-writing-selection-menu"
          role="toolbar"
          aria-label="选中文字操作"
          style={{ left: selection.left, top: selection.top }}
          onMouseDown={(event: MouseEvent<HTMLDivElement>) =>
            event.preventDefault()
          }
        >
          {(
            [
              "解释这处",
              "给我提示",
              "比较表达",
              "问这处",
            ] as WritingSelectionAction[]
          ).map((action) => (
            <button
              type="button"
              key={action}
              onClick={() =>
                openAgent({
                  selectionText: selection.text,
                  action,
                })
              }
            >
              {action === "问这处" ? "问这处…" : action}
            </button>
          ))}
        </div>
      ) : null}
    </main>
  );
}

function documentQuery(selector: string) {
  return document.querySelector<HTMLElement>(selector);
}

export default WritingPage;
