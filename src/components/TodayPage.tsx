import { useLayoutEffect, useRef, useState, type FormEvent } from "react";
import type { TodayActionId, TodayPageViewModel } from "../mainAppViewModel";
import MainAppIcon from "./MainAppIcon";

type TodayPageProps = {
  viewModel: TodayPageViewModel;
  onActionSelect: (id: TodayActionId) => void;
  onSubmitPrompt: (value: string) => void;
};

function TodayPage({
  viewModel,
  onActionSelect,
  onSubmitPrompt,
}: TodayPageProps) {
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useLayoutEffect(() => {
    const input = inputRef.current;
    if (!input) {
      return;
    }

    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, 96)}px`;
  }, [draft]);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = draft.trim();
    if (!value) {
      inputRef.current?.focus();
      return;
    }
    onSubmitPrompt(value);
  }

  return (
    <main className="rr-main-panel">
      <section className="rr-main-home-view" aria-labelledby="rr-main-today-heading">
        <div className="rr-main-home-content">
          <header className="rr-main-page-heading">
            <h1 id="rr-main-today-heading">{viewModel.heading}</h1>
            <time className="rr-main-date" dateTime={viewModel.dateTime}>
              {viewModel.dateLabel}
            </time>
          </header>

          <section className="rr-main-agent-summary" aria-label="今天的学习摘要">
            <p>{viewModel.summary}</p>
            <p className="rr-main-local-context">{viewModel.localContext}</p>
          </section>

          <section className="rr-main-action-list" aria-label="今天可以开始的学习任务">
            {viewModel.actions.map((action) => (
              <button
                className="rr-main-action-row"
                type="button"
                key={action.id}
                onClick={() => onActionSelect(action.id)}
              >
                <span className="rr-main-action-icon"><MainAppIcon name={action.icon} /></span>
                <span className="rr-main-action-copy">
                  <span className="rr-main-action-title">{action.title}</span>
                  <span className="rr-main-action-meta">{action.description}</span>
                </span>
                <span className="rr-main-action-arrow"><MainAppIcon name="arrow" /></span>
              </button>
            ))}
          </section>
        </div>
      </section>

      <section className="rr-main-composer-area" aria-label="与 ReadRay 对话">
        <div className="rr-main-composer-inner">
          <form className="rr-main-composer" onSubmit={submit}>
            <textarea
              ref={inputRef}
              rows={1}
              value={draft}
              aria-label="对话内容"
              placeholder={viewModel.composerPlaceholder}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  event.currentTarget.form?.requestSubmit();
                }
              }}
            />
            <button className="rr-main-send" type="submit" aria-label="发送并开始持续对话">
              <MainAppIcon name="send" />
            </button>
          </form>
          <div className="rr-main-composer-meta">
            <span>Enter 发送 · Shift + Enter 换行</span>
            <span>对话与学习记录保存在本地</span>
          </div>
        </div>
      </section>
    </main>
  );
}

export default TodayPage;
