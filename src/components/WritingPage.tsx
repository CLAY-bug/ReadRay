import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent,
} from "react";
import { writingDemoRepository, type WritingRepository } from "../writingRepository";
import {
  cloneWritingSnapshot,
  countWritingWords,
  getRecordSnapshot,
  normalizeWritingText,
  writingIssues,
  writingSnapshotsEqual,
  type WritingDocumentRecord,
  type WritingDocumentStatus,
  type WritingIssueId,
  type WritingMode,
  type WritingSnapshot,
} from "../writingViewModel";
import WritingCoach, {
  type CoachIssueState,
  type WritingAgentRequest,
  type WritingSelectionAction,
} from "./WritingCoach";
import WritingCompareView from "./WritingCompareView";
import WritingEditor, {
  type WritingEditorHandle,
  type WritingSelection,
} from "./WritingEditor";
import WritingLibrary from "./WritingLibrary";

type WritingPageProps = {
  libraryRequest: number;
  repository?: WritingRepository;
  onWindowTitleChange: (title: string) => void;
};

const emptySnapshot: WritingSnapshot = { title: "", paragraphs: [""] };

function createIssueStates(): Record<WritingIssueId, CoachIssueState> {
  return Object.fromEntries(writingIssues.map((issue) => [issue.id, {
    status: "open",
    showDeeperHint: false,
    showReference: false,
  }])) as Record<WritingIssueId, CoachIssueState>;
}

function createDocumentId() {
  return globalThis.crypto?.randomUUID?.() ?? `writing-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function formatDocumentTime(value: string | undefined, prefix: string) {
  const date = value ? new Date(value) : new Date();
  const parts = new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).formatToParts(date);
  const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((item) => item.type === type)?.value ?? "";
  return `${prefix}${part("month")} 月 ${part("day")} 日 ${part("hour")}:${part("minute")}`;
}

function currentRecordStatus(record: WritingDocumentRecord): WritingDocumentStatus {
  return record.draftSnapshot ? "draft" : "completed";
}

function WritingPage({
  libraryRequest,
  repository = writingDemoRepository,
  onWindowTitleChange,
}: WritingPageProps) {
  const [records, setRecords] = useState(() => repository.list());
  const [mode, setMode] = useState<WritingMode>("library");
  const [activeDocumentId, setActiveDocumentId] = useState<string | null>(null);
  const activeDocumentIdRef = useRef<string | null>(null);
  const [snapshot, setSnapshot] = useState<WritingSnapshot>(emptySnapshot);
  const snapshotRef = useRef(snapshot);
  const [comparisonBaseline, setComparisonBaseline] = useState<WritingSnapshot>(emptySnapshot);
  const comparisonBaselineRef = useRef(comparisonBaseline);
  const [completedComparisonBaseline, setCompletedComparisonBaseline] = useState<WritingSnapshot>(emptySnapshot);
  const [resetKey, setResetKey] = useState(0);
  const [revisionFromCompleted, setRevisionFromCompleted] = useState(false);
  const [saveState, setSaveState] = useState<"saved" | "saving">("saved");
  const [checking, setChecking] = useState(false);
  const [reviewRound, setReviewRound] = useState(0);
  const [reviewIssueIds, setReviewIssueIds] = useState<WritingIssueId[]>([]);
  const [issueStates, setIssueStates] = useState(createIssueStates);
  const [activeIssueId, setActiveIssueId] = useState<WritingIssueId | null>(null);
  const [editingIssueId, setEditingIssueId] = useState<WritingIssueId | null>(null);
  const [compareOrigin, setCompareOrigin] = useState<"review" | "completed">("review");
  const [documentSwitcherOpen, setDocumentSwitcherOpen] = useState(false);
  const [selection, setSelection] = useState<WritingSelection | null>(null);
  const [assistOpen, setAssistOpen] = useState(false);
  const [agentRequest, setAgentRequest] = useState<WritingAgentRequest>({ token: 0, intent: "ask" });
  const editorRef = useRef<WritingEditorHandle>(null);
  const saveTimerRef = useRef<number | null>(null);

  const activeRecord = useMemo(
    () => records.find((record) => record.id === activeDocumentId) ?? null,
    [activeDocumentId, records],
  );

  const hasChanges = !writingSnapshotsEqual(snapshot, comparisonBaseline);
  const isBlank = !snapshot.title && !snapshot.paragraphs.some((paragraph) => normalizeWritingText(paragraph));

  useEffect(() => {
    snapshotRef.current = snapshot;
  }, [snapshot]);

  useEffect(() => {
    comparisonBaselineRef.current = comparisonBaseline;
  }, [comparisonBaseline]);

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

  useEffect(() => {
    if (libraryRequest > 0) {
      setDocumentSwitcherOpen(false);
      setAssistOpen(false);
      setMode("library");
    }
  }, [libraryRequest]);

  useEffect(() => () => {
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
    }
  }, []);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.ctrlKey && event.key.toLowerCase() === "j" && ["draft", "review", "completed"].includes(mode)) {
        event.preventDefault();
        if (assistOpen) {
          setAssistOpen(false);
        } else {
          setAgentRequest((request) => ({ ...request, token: request.token + 1, intent: "ask", selectionText: undefined, action: undefined }));
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
  }, [assistOpen, mode]);

  function saveRecord(record: WritingDocumentRecord) {
    setRecords(repository.save(record));
  }

  function scheduleDraftSave(nextSnapshot: WritingSnapshot, documentId: string) {
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
    }
    setSaveState("saving");
    saveTimerRef.current = window.setTimeout(() => {
      const now = new Date().toISOString();
      const currentRecords = repository.list();
      const existing = currentRecords.find((record) => record.id === documentId);
      const record: WritingDocumentRecord = existing ? {
        ...existing,
        draftSnapshot: cloneWritingSnapshot(nextSnapshot),
        draftUpdatedAt: now,
        updatedAt: now,
      } : {
        id: documentId,
        createdAt: now,
        updatedAt: now,
        draftUpdatedAt: now,
        draftSnapshot: cloneWritingSnapshot(nextSnapshot),
        comparisonBaseline: cloneWritingSnapshot(comparisonBaselineRef.current),
        versions: [],
      };
      saveRecord(record);
      setSaveState("saved");
      saveTimerRef.current = null;
    }, 420);
  }

  function handleEditorChange(nextSnapshot: WritingSnapshot) {
    setSnapshot(nextSnapshot);
    snapshotRef.current = nextSnapshot;
    setSelection(null);

    let documentId = activeDocumentIdRef.current;
    const hasContent = Boolean(nextSnapshot.title || nextSnapshot.paragraphs.some((paragraph) => normalizeWritingText(paragraph)));
    if (!documentId && hasContent) {
      documentId = createDocumentId();
      activeDocumentIdRef.current = documentId;
      setActiveDocumentId(documentId);
    }
    if (documentId && mode !== "completed") {
      scheduleDraftSave(nextSnapshot, documentId);
    }

    if (editingIssueId) {
      const issue = writingIssues.find((candidate) => candidate.id === editingIssueId);
      const text = normalizeWritingText(nextSnapshot.paragraphs.join(" "));
      if (issue && !text.includes(issue.targetText)) {
        setIssueStates((states) => ({
          ...states,
          [editingIssueId]: { ...states[editingIssueId], status: "modified" },
        }));
        const nextIssue = writingIssues.find((candidate) => (
          candidate.id !== editingIssueId
          && text.includes(candidate.targetText)
          && issueStates[candidate.id].status !== "ignored"
        ));
        setActiveIssueId(nextIssue?.id ?? null);
        setEditingIssueId(null);
      }
    }
  }

  function resetReviewState(nextSnapshot: WritingSnapshot, round: number) {
    const ids = writingIssues
      .filter((issue) => normalizeWritingText(nextSnapshot.paragraphs.join(" ")).includes(issue.targetText))
      .map((issue) => issue.id);
    setIssueStates(createIssueStates());
    setReviewIssueIds(ids);
    setActiveIssueId(ids[0] ?? null);
    setEditingIssueId(null);
    setReviewRound(round);
  }

  function openRecord(record: WritingDocumentRecord, status: WritingDocumentStatus) {
    const nextSnapshot = status === "draft"
      ? cloneWritingSnapshot(record.draftSnapshot ?? emptySnapshot)
      : cloneWritingSnapshot(record.completedSnapshot ?? emptySnapshot);
    const lastVersion = record.versions[record.versions.length - 1];
    const nextBaseline = status === "completed"
      ? cloneWritingSnapshot(nextSnapshot)
      : cloneWritingSnapshot(record.comparisonBaseline);
    activeDocumentIdRef.current = record.id;
    setActiveDocumentId(record.id);
    setSnapshot(nextSnapshot);
    snapshotRef.current = nextSnapshot;
    setComparisonBaseline(nextBaseline);
    comparisonBaselineRef.current = nextBaseline;
    setCompletedComparisonBaseline(cloneWritingSnapshot(lastVersion?.comparisonBaseline ?? record.comparisonBaseline));
    setRevisionFromCompleted(status === "draft" && Boolean(record.completedSnapshot));
    setDocumentSwitcherOpen(false);
    setAssistOpen(false);
    setSelection(null);
    setSaveState("saved");
    setResetKey((key) => key + 1);
    resetReviewState(nextSnapshot, 0);
    setMode(status === "completed" ? "completed" : "draft");
  }

  function createNewDraft() {
    activeDocumentIdRef.current = null;
    setActiveDocumentId(null);
    setSnapshot(emptySnapshot);
    snapshotRef.current = emptySnapshot;
    setComparisonBaseline(emptySnapshot);
    comparisonBaselineRef.current = emptySnapshot;
    setCompletedComparisonBaseline(emptySnapshot);
    setRevisionFromCompleted(false);
    setResetKey((key) => key + 1);
    resetReviewState(emptySnapshot, 0);
    setMode("draft");
    window.requestAnimationFrame(() => document.querySelector<HTMLElement>(".rr-writing-editor-title")?.focus());
  }

  function beginReview(nextBaseline = comparisonBaseline, round = reviewRound + 1) {
    if (isBlank) {
      openAgent({ intent: "start" });
      return;
    }
    setChecking(true);
    window.setTimeout(() => {
      if (!writingSnapshotsEqual(nextBaseline, comparisonBaseline)) {
        setComparisonBaseline(cloneWritingSnapshot(nextBaseline));
        comparisonBaselineRef.current = cloneWritingSnapshot(nextBaseline);
      }
      resetReviewState(snapshotRef.current, round);
      setChecking(false);
      setAssistOpen(false);
      setMode("review");
    }, 720);
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
    setAgentRequest((request) => ({
      token: request.token + 1,
      intent,
      selectionText,
      action,
    }));
    setAssistOpen(true);
  }

  function completeWriting() {
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }
    const now = new Date().toISOString();
    const currentId = activeDocumentIdRef.current ?? createDocumentId();
    const previous = repository.list().find((record) => record.id === currentId);
    const version = {
      id: createDocumentId(),
      completedAt: now,
      snapshot: cloneWritingSnapshot(snapshot),
      comparisonBaseline: cloneWritingSnapshot(comparisonBaseline),
    };
    const record: WritingDocumentRecord = previous ? {
      ...previous,
      updatedAt: now,
      completedAt: now,
      draftUpdatedAt: undefined,
      draftSnapshot: undefined,
      completedSnapshot: cloneWritingSnapshot(snapshot),
      versions: [...previous.versions, version],
    } : {
      id: currentId,
      createdAt: now,
      updatedAt: now,
      completedAt: now,
      completedSnapshot: cloneWritingSnapshot(snapshot),
      comparisonBaseline: cloneWritingSnapshot(comparisonBaseline),
      versions: [version],
    };
    activeDocumentIdRef.current = currentId;
    setActiveDocumentId(currentId);
    saveRecord(record);
    setCompletedComparisonBaseline(cloneWritingSnapshot(comparisonBaseline));
    setComparisonBaseline(cloneWritingSnapshot(snapshot));
    comparisonBaselineRef.current = cloneWritingSnapshot(snapshot);
    setRevisionFromCompleted(false);
    resetReviewState(snapshot, 0);
    setMode("completed");
  }

  function continueEditing() {
    if (!activeRecord) {
      return;
    }
    const now = new Date().toISOString();
    const record: WritingDocumentRecord = {
      ...activeRecord,
      draftSnapshot: cloneWritingSnapshot(snapshot),
      draftUpdatedAt: now,
      updatedAt: now,
      comparisonBaseline: cloneWritingSnapshot(snapshot),
    };
    saveRecord(record);
    setComparisonBaseline(cloneWritingSnapshot(snapshot));
    comparisonBaselineRef.current = cloneWritingSnapshot(snapshot);
    setRevisionFromCompleted(true);
    setMode("draft");
    window.requestAnimationFrame(() => editorRef.current?.focusBody(true));
  }

  function updateIssueState(issueId: WritingIssueId, update: (state: CoachIssueState) => CoachIssueState) {
    setIssueStates((states) => ({ ...states, [issueId]: update(states[issueId]) }));
  }

  const recentRecords = records
    .filter((record) => record.id !== activeDocumentId)
    .sort((first, second) => new Date(second.updatedAt).getTime() - new Date(first.updatedAt).getTime())
    .slice(0, 4);
  const documentStatus = mode === "completed" ? "已完成" : revisionFromCompleted ? "基于完成稿修改" : "本地草稿";
  const documentTime = saveState === "saving"
    ? "正在自动保存…"
    : formatDocumentTime(
      mode === "completed" ? activeRecord?.completedAt : activeRecord?.draftUpdatedAt,
      mode === "completed" ? "完成于 " : "更新于 ",
    );
  const kicker = mode === "completed"
    ? "Personal reflection · 已完成"
    : revisionFromCompleted ? "Personal reflection · 修改中" : "Personal reflection · Draft";

  if (mode === "library") {
    return (
      <main className="rr-writing-page is-library">
        <WritingLibrary records={records} onNew={createNewDraft} onOpen={openRecord} />
      </main>
    );
  }

  if (mode === "compare") {
    return (
      <main className="rr-writing-page is-compare">
        <WritingCompareView
          original={compareOrigin === "completed" ? completedComparisonBaseline : comparisonBaseline}
          current={snapshot}
          origin={compareOrigin}
          checking={checking}
          onBack={() => setMode(compareOrigin === "completed" ? "completed" : "review")}
          onCheckAgain={() => beginReview(cloneWritingSnapshot(snapshot), reviewRound + 1)}
          onFinish={completeWriting}
        />
      </main>
    );
  }

  return (
    <main className={`rr-writing-page is-${mode}${assistOpen ? " has-assist" : ""}`} data-testid="writing-page">
      <header className="rr-writing-document-bar">
        <div className="rr-writing-document-meta">
          <div className="rr-writing-document-switcher">
            <button
              className="rr-writing-document-name"
              type="button"
              aria-expanded={documentSwitcherOpen}
              title="切换文章"
              onClick={() => setDocumentSwitcherOpen((open) => !open)}
            >{snapshot.title || "未命名文章"}</button>
            {documentSwitcherOpen ? (
              <div className="rr-writing-document-menu">
                <button type="button" onClick={() => setMode("library")}>查看全部文章</button>
                <div aria-hidden="true" />
                <p>最近文章</p>
                {recentRecords.length ? recentRecords.map((record) => {
                  const status = currentRecordStatus(record);
                  const entrySnapshot = getRecordSnapshot(record);
                  return (
                    <button type="button" key={record.id} onClick={() => openRecord(record, status)}>
                      <strong>{entrySnapshot.title || "未命名文章"}</strong>
                      <span>{status === "draft" ? "修改中" : "已完成"} · {formatDocumentTime(record.updatedAt, "")}</span>
                    </button>
                  );
                }) : <span className="rr-writing-switcher-empty">还没有其他文章。</span>}
              </div>
            ) : null}
          </div>
          <span>·</span><span>{documentStatus}</span><span>·</span><span>{documentTime}</span>
        </div>
        <div className="rr-writing-document-actions">
          <span className="rr-writing-word-count">{countWritingWords(snapshot)} 词</span>
          {mode === "review" ? <button className="rr-writing-btn is-ghost" type="button" onClick={() => setMode("draft")}>返回草稿</button> : null}
          <span className="rr-writing-agent-slot">
            <button
              className={`rr-writing-btn is-ghost rr-writing-ask${assistOpen ? " is-active" : ""}`}
              type="button"
              aria-expanded={assistOpen}
              onClick={() => assistOpen ? setAssistOpen(false) : openAgent()}
            >{mode === "review" && assistOpen ? "检查结果" : "问 ReadRay"}</button>
          </span>
          {mode === "draft" ? (
            <button className={`rr-writing-btn is-secondary${checking ? " is-checking" : ""}`} type="button" disabled={checking} onClick={() => beginReview()}>
              <span className="rr-writing-check-dot" aria-hidden="true" />{checking ? "检查中…" : "检查文章"}
            </button>
          ) : null}
          {mode === "review" ? (
            <button className="rr-writing-btn is-secondary" type="button" disabled={!hasChanges} title={hasChanges ? "查看初稿与当前稿" : "完成至少一处修改后可查看对比"} onClick={() => { setCompareOrigin("review"); setMode("compare"); }}>查看对比</button>
          ) : null}
          {mode === "completed" ? (
            <>
              <button className="rr-writing-btn is-ghost" type="button" onClick={() => { setCompareOrigin("completed"); setMode("compare"); }}>查看修改</button>
              <button className="rr-writing-btn is-secondary" type="button" onClick={continueEditing}>继续修改</button>
            </>
          ) : null}
        </div>
      </header>

      <section className="rr-writing-workspace" aria-label="英文写作工作区">
        <div className="rr-writing-grid">
          <WritingEditor
            ref={editorRef}
            mode={mode}
            snapshot={snapshot}
            resetKey={resetKey}
            kicker={kicker}
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
          <aside className="rr-writing-coach-column" aria-label="写作教练">
            <WritingCoach
              mode={mode}
              assistOpen={assistOpen}
              request={agentRequest}
              round={reviewRound}
              activeIssueId={activeIssueId}
              issueStates={issueStates}
              visibleIssueIds={reviewIssueIds}
              onCloseAssist={() => setAssistOpen(false)}
              onActivateIssue={(issueId) => setActiveIssueId(issueId)}
              onReviseIssue={(issueId) => {
                setAssistOpen(false);
                setActiveIssueId(issueId);
                setEditingIssueId(issueId);
                updateIssueState(issueId, (state) => ({ ...state, status: state.status === "modified" ? "editing" : state.status }));
                window.requestAnimationFrame(() => editorRef.current?.focusIssue(issueId));
              }}
              onToggleHint={(issueId) => updateIssueState(issueId, (state) => ({ ...state, showDeeperHint: !state.showDeeperHint }))}
              onToggleReference={(issueId) => updateIssueState(issueId, (state) => ({ ...state, showReference: !state.showReference }))}
              onToggleIgnore={(issueId) => updateIssueState(issueId, (state) => ({ ...state, status: state.status === "ignored" ? "open" : "ignored" }))}
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
          onMouseDown={(event: MouseEvent<HTMLDivElement>) => event.preventDefault()}
        >
          {(["解释这处", "给我提示", "比较表达", "问这处"] as WritingSelectionAction[]).map((action) => (
            <button type="button" key={action} onClick={() => openAgent({ selectionText: selection.text, action })}>{action === "问这处" ? "问这处…" : action}</button>
          ))}
        </div>
      ) : null}
    </main>
  );
}

export default WritingPage;
