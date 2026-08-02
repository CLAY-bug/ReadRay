import {
  forwardRef,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  type KeyboardEvent,
} from "react";
import {
  normalizeWritingText,
  type WritingIssue,
  type WritingPattern,
  type WritingMode,
  type WritingSnapshot,
} from "../writingViewModel";

export type WritingSelection = {
  text: string;
  left: number;
  top: number;
};

export type WritingEditorHandle = {
  focusBody(atEnd?: boolean): void;
  focusIssue(issueId: string): void;
  getSnapshot(): WritingSnapshot;
};

type WritingEditorProps = {
  mode: Exclude<WritingMode, "compare" | "library">;
  snapshot: WritingSnapshot;
  issues: WritingIssue[];
  patterns: WritingPattern[];
  resetKey: number;
  kicker: string;
  activeIssueId: string | null;
  editingIssueId: string | null;
  onChange: (snapshot: WritingSnapshot) => void;
  onSelection: (selection: WritingSelection | null) => void;
  onIssueSelect: (issueId: string) => void;
  onStartAssist: () => void;
};

function snapshotFromElements(
  title: HTMLElement | null,
  body: HTMLElement | null,
): WritingSnapshot {
  const paragraphs = body
    ? Array.from(body.children)
      .filter((element) => element.tagName === "P" || element.tagName === "DIV")
      .map((element) => normalizeWritingText((element as HTMLElement).innerText))
    : [];

  return {
    title: normalizeWritingText(title?.innerText ?? ""),
    paragraphs: paragraphs.length ? paragraphs : [normalizeWritingText(body?.innerText ?? "")],
  };
}

function appendParagraphWithIssueTargets(
  container: HTMLElement,
  paragraphText: string,
  issues: WritingIssue[],
) {
  const paragraph = document.createElement("p");
  let cursor = 0;

  while (cursor < paragraphText.length) {
    const nextIssue = issues
      .map((issue) => ({ issue, index: paragraphText.indexOf(issue.targetText, cursor) }))
      .filter((candidate) => candidate.index >= 0)
      .sort((first, second) => first.index - second.index)[0];

    if (!nextIssue) {
      paragraph.append(document.createTextNode(paragraphText.slice(cursor)));
      break;
    }

    if (nextIssue.index > cursor) {
      paragraph.append(document.createTextNode(paragraphText.slice(cursor, nextIssue.index)));
    }

    const target = document.createElement("span");
    target.className = "rr-writing-issue-target";
    target.dataset.issue = nextIssue.issue.id;
    target.textContent = nextIssue.issue.targetText;
    paragraph.append(target);
    cursor = nextIssue.index + nextIssue.issue.targetText.length;
  }

  if (!paragraphText) {
    paragraph.append(document.createElement("br"));
  }
  container.append(paragraph);
}

const WritingEditor = forwardRef<WritingEditorHandle, WritingEditorProps>(function WritingEditor(
  {
    mode,
    snapshot,
    issues,
    patterns,
    resetKey,
    kicker,
    activeIssueId,
    editingIssueId,
    onChange,
    onSelection,
    onIssueSelect,
    onStartAssist,
  },
  forwardedRef,
) {
  const titleRef = useRef<HTMLHeadingElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const columnRef = useRef<HTMLDivElement>(null);

  function readSnapshot() {
    return snapshotFromElements(titleRef.current, bodyRef.current);
  }

  function emitChange() {
    onChange(readSnapshot());
  }

  useLayoutEffect(() => {
    if (!titleRef.current || !bodyRef.current) {
      return;
    }
    titleRef.current.textContent = snapshot.title;
    bodyRef.current.replaceChildren();
    const paragraphs = snapshot.paragraphs.length ? snapshot.paragraphs : [""];
    paragraphs.forEach((paragraph) =>
      appendParagraphWithIssueTargets(bodyRef.current!, paragraph, issues),
    );
    columnRef.current?.scrollTo({ top: 0 });
  }, [resetKey]);

  useLayoutEffect(() => {
    bodyRef.current?.querySelectorAll<HTMLElement>(".rr-writing-issue-target").forEach((target) => {
      const issueId = target.dataset.issue!;
      target.classList.toggle("is-active", issueId === activeIssueId);
      target.classList.toggle("has-focus-anchor", issueId === editingIssueId && target.innerText.length <= 56);
    });
  }, [activeIssueId, editingIssueId]);

  useImperativeHandle(forwardedRef, () => ({
    focusBody(atEnd = false) {
      const body = bodyRef.current;
      if (!body) {
        return;
      }
      body.focus({ preventScroll: true });
      if (!atEnd) {
        return;
      }
      const range = document.createRange();
      range.selectNodeContents(body);
      range.collapse(false);
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
    },
    focusIssue(issueId) {
      const target = bodyRef.current?.querySelector<HTMLElement>(`[data-issue="${issueId}"]`);
      if (!target) {
        return;
      }
      target.classList.add("is-locating");
      target.scrollIntoView({ block: "center", behavior: "smooth" });
      bodyRef.current?.focus({ preventScroll: true });
      const range = document.createRange();
      range.selectNodeContents(target);
      range.collapse(false);
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
      window.setTimeout(() => target.classList.remove("is-locating"), 880);
    },
    getSnapshot: readSnapshot,
  }));

  function updateSelection() {
    window.requestAnimationFrame(() => {
      const selection = window.getSelection();
      if (!selection || selection.isCollapsed || !selection.rangeCount || !bodyRef.current) {
        onSelection(null);
        return;
      }
      const range = selection.getRangeAt(0);
      const common = range.commonAncestorContainer.nodeType === Node.TEXT_NODE
        ? range.commonAncestorContainer.parentElement
        : range.commonAncestorContainer;
      if (!(common instanceof Node) || !bodyRef.current.contains(common)) {
        onSelection(null);
        return;
      }
      const text = normalizeWritingText(selection.toString()).slice(0, 120);
      if (!text) {
        onSelection(null);
        return;
      }
      const rect = range.getBoundingClientRect();
      onSelection({
        text,
        left: rect.left + rect.width / 2,
        top: rect.top,
      });
    });
  }

  const currentIsEmpty = !snapshot.paragraphs.some((paragraph) => normalizeWritingText(paragraph));
  const readOnly = mode === "completed";

  return (
    <div className="rr-writing-editor-column" ref={columnRef} data-testid="writing-editor-column">
      <article className="rr-writing-editor-page" aria-label="英文文档编辑器">
        <div className="rr-writing-editor-kicker">{kicker}</div>
        <h1
          ref={titleRef}
          className="rr-writing-editor-title"
          contentEditable={!readOnly}
          suppressContentEditableWarning
          spellCheck={false}
          data-placeholder="文章标题"
          lang="en"
          onInput={emitChange}
          onKeyDown={(event: KeyboardEvent<HTMLHeadingElement>) => {
            if (event.key === "Enter") {
              event.preventDefault();
              bodyRef.current?.focus();
            }
          }}
        />
        <div className={`rr-writing-editor-body${currentIsEmpty ? " is-empty" : ""}`}>
          <div
            ref={bodyRef}
            className="rr-writing-article-editor"
            contentEditable={!readOnly}
            suppressContentEditableWarning
            spellCheck={!readOnly}
            lang="en"
            aria-readonly={readOnly}
            onInput={emitChange}
            onMouseUp={updateSelection}
            onKeyUp={updateSelection}
            onClick={(event) => {
              const target = (event.target as HTMLElement).closest<HTMLElement>("[data-issue]");
              if (target?.dataset.issue) {
                onIssueSelect(target.dataset.issue);
              }
            }}
          />
          {currentIsEmpty && mode === "draft" ? (
            <div className="rr-writing-starter" aria-hidden="true">
              <p>写下你想表达的内容，中文也可以。</p>
              <button type="button" onClick={onStartAssist}>帮我梳理思路</button>
            </div>
          ) : null}
        </div>

        {mode === "completed" ? (
          <section className="rr-writing-completion-takeaways" aria-labelledby="rr-writing-takeaways-title">
            <header>
              <h2 id="rr-writing-takeaways-title">本次写作要点</h2>
              <p>随本文版本保存，未加入复习</p>
            </header>
            <div>
              {patterns.map((pattern) => (
                <article className="rr-writing-completion-pattern" key={pattern.id}>
                  <span>{pattern.id}</span>
                  <div><h3>{pattern.title}</h3><p>{pattern.description}</p></div>
                </article>
              ))}
            </div>
          </section>
        ) : null}
      </article>
    </div>
  );
});

export default WritingEditor;
