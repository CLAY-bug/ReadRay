import { useEffect, useMemo, useState, type FormEvent } from "react";
import {
  answerWritingQuestion,
  writingIssues,
  type AgentAnswer,
  type WritingIssueId,
  type WritingMode,
} from "../writingViewModel";

export type CoachIssueState = {
  status: "open" | "editing" | "modified" | "ignored";
  showDeeperHint: boolean;
  showReference: boolean;
};

export type WritingSelectionAction = "解释这处" | "给我提示" | "比较表达" | "问这处";

export type WritingAgentRequest = {
  token: number;
  intent: "ask" | "start" | "stuck";
  selectionText?: string;
  action?: WritingSelectionAction;
};

type WritingCoachProps = {
  mode: Exclude<WritingMode, "compare" | "library">;
  assistOpen: boolean;
  request: WritingAgentRequest;
  round: number;
  activeIssueId: WritingIssueId | null;
  issueStates: Record<WritingIssueId, CoachIssueState>;
  visibleIssueIds: WritingIssueId[];
  onCloseAssist: () => void;
  onActivateIssue: (issueId: WritingIssueId) => void;
  onReviseIssue: (issueId: WritingIssueId) => void;
  onToggleHint: (issueId: WritingIssueId) => void;
  onToggleReference: (issueId: WritingIssueId) => void;
  onToggleIgnore: (issueId: WritingIssueId) => void;
};

const quickPrompts = [
  ["“逐渐意识到”可以怎么表达？", "“逐渐意识到”可以用哪些英文词组？"],
  ["我忘了虚拟语气怎么写", "我忘了虚拟语气的基本结构。"],
  ["这一段不知道怎么继续", "帮我梳理当前这一段的下一步。"],
] as const;

function WritingAgentPanel({
  request,
  mode,
  onClose,
}: {
  request: WritingAgentRequest;
  mode: Exclude<WritingMode, "compare" | "library">;
  onClose: () => void;
}) {
  const [question, setQuestion] = useState("");
  const [selectionText, setSelectionText] = useState("");
  const [currentAnswer, setCurrentAnswer] = useState<AgentAnswer | null>(null);
  const [history, setHistory] = useState<AgentAnswer[]>([]);

  const scopeLabel = selectionText
    ? "所选内容"
    : request.intent === "start" ? "整篇文章" : "当前段落";

  function showAnswer(nextQuestion: string) {
    const answer = answerWritingQuestion(nextQuestion, scopeLabel, selectionText);
    setCurrentAnswer((previous) => {
      if (previous) {
        setHistory((entries) => [previous, ...entries].slice(0, 5));
      }
      return answer;
    });
    setQuestion("");
  }

  useEffect(() => {
    setSelectionText(request.selectionText ?? "");
    if (request.action && request.action !== "问这处") {
      const nextScope = request.selectionText ? "所选内容" : "当前段落";
      setCurrentAnswer(answerWritingQuestion(request.action, nextScope, request.selectionText));
    } else if (request.action === "问这处") {
      setCurrentAnswer({
        question: "等待你的具体问题",
        scopeLabel: "所选内容",
        title: "你想具体确认什么？",
        copy: "可以问语气、用词、语法或表达重点。回答会完整显示在这里，并且可以继续追问。",
      });
    }
  }, [request.token]);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = question.trim();
    if (value) {
      showAnswer(value);
    }
  }

  function refine(kind: "angle" | "simple" | "phrases" | "restart") {
    if (!currentAnswer) {
      return;
    }
    if (kind === "restart") {
      setQuestion(currentAnswer.question);
      return;
    }
    setHistory((entries) => [currentAnswer, ...entries].slice(0, 5));
    const replacements = {
      angle: {
        title: "从变化发生的时刻切入",
        copy: "不要先解释背景，先找到让你改变判断的那一刻。读者会更早看见这篇文章真正属于你的部分。",
      },
      simple: {
        title: "用短句保留自己的声音",
        copy: "先用一个主语和一个核心动词写清判断；例子与原因分别放到后面的句子。",
      },
      phrases: {
        title: "只补能立即用上的表达",
        copy: "I once believed、over time、what made me reconsider、I came to realize。先选一两个，不必全部使用。",
      },
    } as const;
    setCurrentAnswer({
      ...currentAnswer,
      question: kind === "angle" ? "换个角度" : kind === "simple" ? "表达再简单些" : "补充相关词组",
      ...replacements[kind],
    });
  }

  return (
    <section className="rr-writing-assist-panel" aria-labelledby="rr-writing-agent-title" data-testid="writing-agent-panel">
      <header className="rr-writing-assist-head">
        <div><p>Writing agent</p><h2 id="rr-writing-agent-title">ReadRay 写作辅助</h2></div>
        <button type="button" onClick={onClose}>{mode === "review" ? "返回检查结果" : "收起"}</button>
      </header>
      <div className="rr-writing-agent-shell">
        <div className="rr-writing-agent-scroll">
          {!currentAnswer ? (
            <section className="rr-writing-agent-empty">
              <p>想不起词组、忘了语法，或不知道下一句怎么接时，都可以直接问。</p>
              <span>ReadRay 会参考你的正文给出下一步，但不会替你写完整句子或接管文章。</span>
              <div>
                {quickPrompts.map(([label, prompt]) => (
                  <button type="button" key={label} onClick={() => showAnswer(prompt)}>{label}</button>
                ))}
              </div>
            </section>
          ) : (
            <section className="rr-writing-agent-current">
              <p className="rr-writing-agent-question">{currentAnswer.scopeLabel} · {currentAnswer.question}</p>
              {selectionText ? <blockquote>“{selectionText}”</blockquote> : null}
              <h3>{currentAnswer.title}</h3>
              <p className="rr-writing-agent-copy">{currentAnswer.copy}</p>
              {currentAnswer.map ? (
                <div className="rr-writing-map">
                  <section><span>你想表达的核心</span><p>{currentAnswer.map.core}</p></section>
                  <section><span>先想清这三个问题</span><ol>{currentAnswer.map.questions.map((item) => <li key={item}>{item}</li>)}</ol></section>
                  <section><span>可以先用到的表达</span><ul>{currentAnswer.map.phrases.map((item) => <li key={item}>{item}</li>)}</ul></section>
                  <section><span>两个起笔句式</span>{currentAnswer.map.starters.map((item) => <p className="rr-writing-map-starter" key={item}>{item}</p>)}</section>
                  <div className="rr-writing-agent-refine">
                    <button type="button" onClick={() => refine("angle")}>换个角度</button>
                    <button type="button" onClick={() => refine("simple")}>表达再简单些</button>
                    <button type="button" onClick={() => refine("phrases")}>补充相关词组</button>
                    <button type="button" onClick={() => refine("restart")}>重新梳理</button>
                  </div>
                </div>
              ) : null}
            </section>
          )}

          {history.length ? (
            <details className="rr-writing-agent-history">
              <summary>之前的问题 <span>{history.length}</span></summary>
              <ol>{history.map((entry, index) => <li key={`${entry.question}-${index}`}><strong>{entry.question}</strong><span>{entry.title}：{entry.copy}</span></li>)}</ol>
            </details>
          ) : null}
        </div>

        <form className="rr-writing-agent-composer" onSubmit={submit}>
          <div className={`rr-writing-agent-input${question.trim() ? " has-content" : ""}`}>
            {selectionText ? (
              <span className="rr-writing-agent-context">
                <span>所选内容 · {selectionText.slice(0, 16)}{selectionText.length > 16 ? "…" : ""}</span>
                <button type="button" aria-label="移除所选内容" onClick={() => setSelectionText("")}>×</button>
              </span>
            ) : null}
            <div>
              <textarea
                rows={1}
                value={question}
                aria-label="向 ReadRay 提问"
                placeholder={request.intent === "start"
                  ? "说说这篇文章大概想写什么，中文或英文都可以……"
                  : selectionText ? "例如：这里的语气是不是太强？" : "问用词、语法，或告诉我你卡在哪里……"}
                onChange={(event) => setQuestion(event.target.value)}
                onKeyDown={(event) => {
                  if (event.ctrlKey && event.key === "Enter") {
                    event.preventDefault();
                    event.currentTarget.form?.requestSubmit();
                  }
                }}
              />
              <button type="submit" disabled={!question.trim()}>提问</button>
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
  activeIssueId,
  issueStates,
  visibleIssueIds,
  onCloseAssist,
  onActivateIssue,
  onReviseIssue,
  onToggleHint,
  onToggleReference,
  onToggleIgnore,
}: WritingCoachProps) {
  const visibleIssues = useMemo(
    () => writingIssues.filter((issue) => visibleIssueIds.includes(issue.id)),
    [visibleIssueIds],
  );

  if (assistOpen) {
    return <WritingAgentPanel request={request} mode={mode} onClose={onCloseAssist} />;
  }

  return (
    <section className="rr-writing-coach" data-testid="writing-coach-panel">
      <header className="rr-writing-coach-head">
        <div><h2>写作教练</h2><span>{round > 1 ? `第 ${round} 轮 · ` : ""}{visibleIssues.length} 个关键问题</span></div>
        <p>{visibleIssues.length
          ? "按对理解和表达的影响排序。先自己判断，再决定怎么改。"
          : "这一轮没有发现需要优先处理的问题。你仍可随时询问 ReadRay，或返回正文继续写。"}</p>
      </header>
      <div className="rr-writing-feedback-list">
        {visibleIssues.map((issue, index) => {
          const state = issueStates[issue.id];
          return (
            <article
              className={`rr-writing-feedback${activeIssueId === issue.id ? " is-active" : ""}${state.status === "modified" ? " is-settled" : ""}${state.status === "ignored" ? " is-ignored" : ""}`}
              key={issue.id}
              role="region"
              tabIndex={0}
              aria-label={`问题 ${String(index + 1).padStart(2, "0")}：${issue.category}`}
              onClick={(event) => {
                if (!(event.target as HTMLElement).closest("button")) {
                  onActivateIssue(issue.id);
                }
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onActivateIssue(issue.id);
                }
              }}
            >
              <div className="rr-writing-feedback-type"><span>{issue.category}</span><span>{String(index + 1).padStart(2, "0")}</span></div>
              <p className="rr-writing-feedback-source">“{issue.source}”</p>
              <p className="rr-writing-feedback-copy">{issue.explanation}</p>
              <p className="rr-writing-feedback-hint">{issue.hint}</p>
              {state.showDeeperHint ? <p className="rr-writing-deeper-hint">{issue.deeperHint}</p> : null}
              {state.showReference ? <p className="rr-writing-reference-copy">{issue.reference}</p> : null}
              <div className="rr-writing-feedback-actions">
                {state.status === "modified" ? <span>已有改动</span> : null}
                <button type="button" onClick={() => onReviseIssue(issue.id)}>{state.status === "modified" ? "返回正文" : "我来修改"}</button>
                <button type="button" onClick={() => onToggleHint(issue.id)}>{state.showDeeperHint ? "收起提示" : "进一步提示"}</button>
                <button type="button" onClick={() => onToggleReference(issue.id)}>{state.showReference ? "收起参考" : "查看参考"}</button>
                <button type="button" onClick={() => onToggleIgnore(issue.id)}>{state.status === "ignored" ? "撤销忽略" : "忽略"}</button>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}

export default WritingCoach;
