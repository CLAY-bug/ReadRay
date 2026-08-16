import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from "react";
import type {
  WritingAgentAnswer,
  WritingIssue,
  WritingMode,
  WritingQuestionScope,
} from "../writingViewModel";
import {
  LatestWritingRequestSequence,
  mergeWritingConversationAnswers,
} from "../writingReviewState";
import {
  shouldSendMultilineMessage,
  type SendShortcut,
} from "../appPreferences";

export type CoachIssueState = {
  status: "open" | "editing" | "modified" | "ignored" | "baseline";
  showDeeperHint: boolean;
  showReference: boolean;
};

export type WritingSelectionAction =
  | "解释这处"
  | "给我提示"
  | "比较表达"
  | "问这处";

export type WritingAgentRequest = {
  token: number;
  intent: "ask" | "start" | "stuck";
  selectionText?: string;
  action?: WritingSelectionAction;
};

export type WritingAgentQuestion = {
  question: string;
  scope: WritingQuestionScope;
  selectionText?: string;
  parentAnswerId?: number;
};

type WritingCoachProps = {
  mode: Exclude<WritingMode, "compare" | "library">;
  assistOpen: boolean;
  request: WritingAgentRequest;
  round: number;
  issues: WritingIssue[];
  answers: WritingAgentAnswer[];
  activeIssueId: string | null;
  issueStates: Record<string, CoachIssueState>;
  onAsk: (request: WritingAgentQuestion) => Promise<WritingAgentAnswer>;
  onCloseAssist: () => void;
  onActivateIssue: (issueId: string) => void;
  onReviseIssue: (issueId: string) => void;
  onToggleHint: (issueId: string) => void;
  onToggleReference: (issueId: string) => void;
  onToggleIgnore: (issueId: string) => void;
  sendShortcut: SendShortcut;
};

const quickPrompts = [
  ["“逐渐意识到”可以怎么表达？", "“逐渐意识到”可以用哪些英文词组？"],
  ["我忘了虚拟语气怎么写", "我忘了虚拟语气的基本结构。"],
  ["这一段不知道怎么继续", "帮我梳理当前这一段的下一步。"],
] as const;

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function resizeQuestionInput(input: HTMLTextAreaElement) {
  input.style.height = "auto";
  const maximum = Number.parseFloat(getComputedStyle(input).maxHeight);
  const contentHeight = input.scrollHeight;
  const height = Number.isFinite(maximum)
    ? Math.min(contentHeight, maximum)
    : contentHeight;
  input.style.height = `${height}px`;
  input.style.overflowY = contentHeight > height + 1 ? "auto" : "hidden";
}

function WritingUserPrompt({
  question,
  selectionText,
}: {
  question: string;
  selectionText?: string;
}) {
  return (
    <div className="rr-writing-user-message" aria-label="你的提问">
      <div className="rr-writing-user-prompt">
        {selectionText ? (
          <div className="rr-writing-user-context">
            <span>针对选中内容</span>
            <p>“{selectionText}”</p>
          </div>
        ) : null}
        <p className="rr-writing-user-question">{question}</p>
      </div>
    </div>
  );
}

function WritingAnswerTurn({
  answer,
  latest,
  loading,
  onRefine,
  onReframe,
}: {
  answer: WritingAgentAnswer;
  latest: boolean;
  loading: boolean;
  onRefine: (question: string) => void;
  onReframe: () => void;
}) {
  return (
    <li className="rr-writing-agent-turn">
      <WritingUserPrompt
        question={answer.question}
        selectionText={answer.selectionText}
      />
      <section className="rr-writing-agent-answer" aria-label="ReadRay 的回答">
        <p className="rr-writing-agent-copy">{answer.copy}</p>
        {answer.map ? (
          <div className="rr-writing-map">
            <section>
              <span>你想表达的核心</span>
              <p>{answer.map.core}</p>
            </section>
            <section>
              <span>先想清这些问题</span>
              <ol>
                {answer.map.questions.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ol>
            </section>
            <section>
              <span>可以先用到的表达</span>
              <ul>
                {answer.map.phrases.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
            </section>
            <section>
              <span>起笔句式</span>
              {answer.map.starters.map((item) => (
                <p className="rr-writing-map-starter" key={item}>
                  {item}
                </p>
              ))}
            </section>
          </div>
        ) : null}
        {latest ? (
          <div className="rr-writing-agent-refine">
            <button
              type="button"
              disabled={loading}
              onClick={() => onRefine("换个角度说明。")}
            >
              换个角度
            </button>
            <button
              type="button"
              disabled={loading}
              onClick={() => onRefine("表达再简单一些。")}
            >
              表达再简单些
            </button>
            <button
              type="button"
              disabled={loading}
              onClick={() => onRefine("补充几个相关词组。")}
            >
              补充相关词组
            </button>
            <button type="button" disabled={loading} onClick={onReframe}>
              重新梳理
            </button>
          </div>
        ) : null}
      </section>
    </li>
  );
}

function WritingAgentPanel({
  request,
  mode,
  answers,
  onAsk,
  onClose,
  sendShortcut,
}: {
  request: WritingAgentRequest;
  mode: Exclude<WritingMode, "compare" | "library">;
  answers: WritingAgentAnswer[];
  onAsk: (request: WritingAgentQuestion) => Promise<WritingAgentAnswer>;
  onClose: () => void;
  sendShortcut: SendShortcut;
}) {
  const [question, setQuestion] = useState("");
  const [selectionText, setSelectionText] = useState("");
  const [conversationAnswers, setConversationAnswers] = useState<
    WritingAgentAnswer[]
  >(() => mergeWritingConversationAnswers([], answers));
  const [pendingRequest, setPendingRequest] =
    useState<WritingAgentQuestion>();
  const scrollRef = useRef<HTMLDivElement>(null);
  const questionInputRef = useRef<HTMLTextAreaElement>(null);
  const stickToBottomRef = useRef(true);
  const latestAnswerRef = useRef<WritingAgentAnswer | undefined>(
    conversationAnswers[conversationAnswers.length - 1],
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();
  const lastQuestionRef = useRef<WritingAgentQuestion | undefined>(
    undefined,
  );
  const requestSequenceRef = useRef(
    new LatestWritingRequestSequence(),
  );

  const scope: WritingQuestionScope = selectionText
    ? "selection"
    : request.intent === "start"
      ? "document"
      : "paragraph";

  useEffect(() => {
    setConversationAnswers((current) =>
      mergeWritingConversationAnswers(current, answers),
    );
  }, [answers]);

  useEffect(() => {
    latestAnswerRef.current =
      conversationAnswers[conversationAnswers.length - 1];
  }, [conversationAnswers]);

  useLayoutEffect(() => {
    const input = questionInputRef.current;
    if (input) {
      resizeQuestionInput(input);
    }
  }, [question]);

  // 窗口缩放期间按 settle 重新测量，避免拖动每一帧强制同步布局。
  useEffect(() => {
    let resizeTimer: number | undefined;
    const resizeAfterWindowSettles = () => {
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(() => {
        if (questionInputRef.current) {
          resizeQuestionInput(questionInputRef.current);
        }
      }, 120);
    };
    window.addEventListener("resize", resizeAfterWindowSettles);
    return () => {
      window.removeEventListener("resize", resizeAfterWindowSettles);
      window.clearTimeout(resizeTimer);
    };
  }, []);

  useLayoutEffect(() => {
    const scroll = scrollRef.current;
    if (scroll && stickToBottomRef.current) {
      scroll.scrollTop = scroll.scrollHeight;
    }
  }, [conversationAnswers.length, error, pendingRequest]);

  async function showAnswer(nextRequest: WritingAgentQuestion) {
    const requestSequence = requestSequenceRef.current.begin();
    const resolvedRequest = {
      ...nextRequest,
      parentAnswerId:
        nextRequest.parentAnswerId ?? latestAnswerRef.current?.id,
    };
    stickToBottomRef.current = true;
    setLoading(true);
    setError(undefined);
    setPendingRequest(resolvedRequest);
    setQuestion("");
    lastQuestionRef.current = resolvedRequest;
    try {
      const answer = await onAsk(resolvedRequest);
      requestSequenceRef.current.requireCurrent(requestSequence);
      setConversationAnswers((current) =>
        mergeWritingConversationAnswers(current, [answer]),
      );
      setPendingRequest(undefined);
    } catch (nextError) {
      if (requestSequenceRef.current.isCurrent(requestSequence)) {
        setError(errorMessage(nextError));
        setPendingRequest(undefined);
        setQuestion((current) => current || resolvedRequest.question);
      }
    } finally {
      if (requestSequenceRef.current.isCurrent(requestSequence)) {
        setLoading(false);
      }
    }
  }

  useEffect(() => {
    requestSequenceRef.current.invalidate();
    const nextSelection = request.selectionText ?? "";
    setSelectionText(nextSelection);
    setPendingRequest(undefined);
    setLoading(false);
    setError(undefined);
    if (request.action && request.action !== "问这处") {
      void showAnswer({
        question: request.action,
        scope: nextSelection ? "selection" : "paragraph",
        selectionText: nextSelection || undefined,
        parentAnswerId: latestAnswerRef.current?.id,
      });
    }
  }, [request.token]);

  useEffect(
    () => () => requestSequenceRef.current.invalidate(),
    [],
  );

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = question.trim();
    if (value && !loading) {
      void showAnswer({
        question: value,
        scope,
        selectionText: selectionText || undefined,
        parentAnswerId: latestAnswerRef.current?.id,
      });
    }
  }

  function refine(question: string) {
    const latestAnswer = latestAnswerRef.current;
    if (!latestAnswer || loading) {
      return;
    }
    void showAnswer({
      question,
      scope,
      selectionText: selectionText || undefined,
      parentAnswerId: latestAnswer.id,
    });
  }

  return (
    <section
      className="rr-writing-assist-panel"
      aria-labelledby="rr-writing-agent-title"
      data-testid="writing-agent-panel"
    >
      <header className="rr-writing-assist-head">
        <h2 id="rr-writing-agent-title">写作辅助</h2>
        <button type="button" onClick={onClose}>
          {mode === "review" ? "返回检查结果" : "收起"}
        </button>
      </header>
      <div className="rr-writing-agent-shell">
        <div
          ref={scrollRef}
          className="rr-writing-agent-scroll"
          onScroll={(event) => {
            const element = event.currentTarget;
            const remaining =
              element.scrollHeight - element.scrollTop - element.clientHeight;
            stickToBottomRef.current = remaining < 48;
          }}
        >
          {!conversationAnswers.length && !pendingRequest ? (
            <section className="rr-writing-agent-empty">
              <p>
                {request.action === "问这处"
                  ? "你想具体确认什么？可以问语气、用词、语法或表达重点。"
                  : "想不起词组、忘了语法，或不知道下一句怎么接时，都可以直接问。"}
              </p>
              <span>
                ReadRay 会参考当前已保存的正文给出下一步，但不会替你写完整文章。
              </span>
              {!request.action ? (
                <div>
                  {quickPrompts.map(([label, prompt]) => (
                    <button
                      type="button"
                      key={label}
                      disabled={loading}
                      onClick={() =>
                        void showAnswer({
                          question: prompt,
                          scope,
                          selectionText: selectionText || undefined,
                        })
                      }
                    >
                      {label}
                    </button>
                  ))}
                </div>
              ) : null}
            </section>
          ) : null}

          {conversationAnswers.length ? (
            <ol className="rr-writing-agent-transcript">
              {conversationAnswers.map((answer, index) => (
                <WritingAnswerTurn
                  key={answer.id}
                  answer={answer}
                  latest={index === conversationAnswers.length - 1}
                  loading={loading}
                  onRefine={refine}
                  onReframe={() => {
                    setQuestion(answer.question);
                    window.requestAnimationFrame(() => {
                      questionInputRef.current?.focus();
                    });
                  }}
                />
              ))}
            </ol>
          ) : null}

          {pendingRequest ? (
            <article className="rr-writing-agent-pending" aria-live="polite">
              <WritingUserPrompt
                question={pendingRequest.question}
                selectionText={pendingRequest.selectionText}
              />
              <div className="rr-writing-agent-thinking">
                <p>正在结合已保存的正文思考…</p>
              </div>
            </article>
          ) : null}
          {error ? (
            <div className="rr-writing-agent-error" role="alert">
              <span>{error}</span>
              <button
                type="button"
                onClick={() => {
                  const lastQuestion = lastQuestionRef.current;
                  if (lastQuestion) {
                    void showAnswer(lastQuestion);
                  }
                }}
              >
                重试
              </button>
            </div>
          ) : null}

        </div>

        <form className="rr-writing-agent-composer" onSubmit={submit}>
          <div
            className={`rr-writing-agent-input${
              question.trim() ? " has-content" : ""
            }`}
          >
            {selectionText ? (
              <span className="rr-writing-agent-context">
                <span>
                  所选内容 · {selectionText.slice(0, 16)}
                  {selectionText.length > 16 ? "…" : ""}
                </span>
                <button
                  type="button"
                  aria-label="移除所选内容"
                  onClick={() => setSelectionText("")}
                >
                  ×
                </button>
              </span>
            ) : null}
            <div>
              <textarea
                ref={questionInputRef}
                rows={1}
                value={question}
                aria-label="向 ReadRay 提问"
                placeholder={
                  request.intent === "start"
                    ? "说说这篇文章大概想写什么，中文或英文都可以……"
                    : selectionText
                      ? "例如：这里的语气是不是太强？"
                      : "问用词、语法，或告诉我你卡在哪里……"
                }
                onChange={(event) => setQuestion(event.target.value)}
                onKeyDown={(event) => {
                  if (
                    shouldSendMultilineMessage(
                      {
                        key: event.key,
                        shiftKey: event.shiftKey,
                        ctrlKey: event.ctrlKey,
                        isComposing: event.nativeEvent.isComposing,
                      },
                      sendShortcut,
                    )
                  ) {
                    event.preventDefault();
                    event.currentTarget.form?.requestSubmit();
                  }
                }}
              />
              <button type="submit" disabled={!question.trim() || loading}>
                提问
              </button>
            </div>
          </div>
        </form>
      </div>
    </section>
  );
}

function WritingCoach({
  mode,
  assistOpen,
  request,
  round,
  issues,
  answers,
  activeIssueId,
  issueStates,
  onAsk,
  onCloseAssist,
  onActivateIssue,
  onReviseIssue,
  onToggleHint,
  onToggleReference,
  onToggleIgnore,
  sendShortcut,
}: WritingCoachProps) {
  const readOnlyBaseline = mode === "completed";
  const visibleIssues = useMemo(
    () => issues.filter((issue) => issueStates[issue.id]),
    [issueStates, issues],
  );

  if (assistOpen) {
    return (
      <WritingAgentPanel
        request={request}
        mode={mode}
        answers={answers}
        onAsk={onAsk}
        onClose={onCloseAssist}
        sendShortcut={sendShortcut}
      />
    );
  }

  return (
    <section
      className="rr-writing-coach"
      data-testid="writing-coach-panel"
    >
      <header className="rr-writing-coach-head">
        <div>
          <h2>
            {readOnlyBaseline ? "基线问题 / 处理回顾" : "写作教练"}
          </h2>
          <span>
            {!readOnlyBaseline && round > 1 ? `第 ${round} 轮 · ` : ""}
            {visibleIssues.length} 个
            {readOnlyBaseline ? "基线问题" : "关键问题"}
          </span>
        </div>
        <p>
          {readOnlyBaseline
            ? "这些问题属于该完成版本的对比基线，仅供回顾，不会在完成稿中重新定位或标记为待处理。"
            : visibleIssues.length
              ? "按对理解和表达的影响排序。先自己判断，再决定怎么改。"
              : "这一轮没有发现需要优先处理的问题。你仍可随时询问 ReadRay，或返回正文继续写。"}
        </p>
      </header>
      <div className="rr-writing-feedback-list">
        {visibleIssues.map((issue, index) => {
          const state = issueStates[issue.id];
          return (
            <article
              className={`rr-writing-feedback${
                activeIssueId === issue.id ? " is-active" : ""
              }${state.status === "modified" ? " is-settled" : ""}${
                state.status === "ignored" ? " is-ignored" : ""
              }`}
              key={issue.id}
              role={readOnlyBaseline ? undefined : "region"}
              tabIndex={readOnlyBaseline ? undefined : 0}
              aria-label={`${readOnlyBaseline ? "基线问题" : "问题"} ${String(
                index + 1,
              ).padStart(2, "0")}：${
                issue.category
              }`}
              onClick={
                readOnlyBaseline
                  ? undefined
                  : (event) => {
                      if (!(event.target as HTMLElement).closest("button")) {
                        onActivateIssue(issue.id);
                      }
                    }
              }
              onKeyDown={
                readOnlyBaseline
                  ? undefined
                  : (event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        onActivateIssue(issue.id);
                      }
                    }
              }
            >
              <div className="rr-writing-feedback-type">
                <span>{issue.category}</span>
                <span>{String(index + 1).padStart(2, "0")}</span>
              </div>
              <p className="rr-writing-feedback-source">“{issue.source}”</p>
              <p className="rr-writing-feedback-copy">{issue.explanation}</p>
              <p className="rr-writing-feedback-hint">{issue.hint}</p>
              {state.showDeeperHint ? (
                <p className="rr-writing-deeper-hint">{issue.deeperHint}</p>
              ) : null}
              {state.showReference ? (
                <p className="rr-writing-reference-copy">{issue.reference}</p>
              ) : null}
              {readOnlyBaseline ? null : (
                <div className="rr-writing-feedback-actions">
                  {state.status === "modified" ? <span>已有改动</span> : null}
                  <button type="button" onClick={() => onReviseIssue(issue.id)}>
                    {state.status === "modified" ? "返回正文" : "我来修改"}
                  </button>
                  <button type="button" onClick={() => onToggleHint(issue.id)}>
                    {state.showDeeperHint ? "收起提示" : "进一步提示"}
                  </button>
                  <button
                    type="button"
                    onClick={() => onToggleReference(issue.id)}
                  >
                    {state.showReference ? "收起参考" : "查看参考"}
                  </button>
                  <button type="button" onClick={() => onToggleIgnore(issue.id)}>
                    {state.status === "ignored" ? "撤销忽略" : "忽略"}
                  </button>
                </div>
              )}
            </article>
          );
        })}
      </div>
    </section>
  );
}

export default WritingCoach;
