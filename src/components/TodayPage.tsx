import { useRef, useState, type FormEvent } from "react";
import type { TodayActionId, TodayPageViewModel } from "../mainAppViewModel";
import MainAppIcon from "./MainAppIcon";
import {
  shouldSendMultilineMessage,
  type SendShortcut,
} from "../appPreferences";
import { useAutoResizeTextarea } from "./useAutoResizeTextarea";

type TodayPageProps = {
  viewModel: TodayPageViewModel;
  status: "loading" | "ready" | "error";
  error?: string;
  onRetry: () => void;
  onActionSelect: (id: TodayActionId) => void;
  onSubmitPrompt: (value: string) => void;
  sendShortcut: SendShortcut;
};

function TodayPage({
  viewModel,
  status,
  error,
  onRetry,
  onActionSelect,
  onSubmitPrompt,
  sendShortcut,
}: TodayPageProps) {
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useAutoResizeTextarea(inputRef, draft);

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

          <section
            className="rr-main-agent-summary"
            aria-label="今天的学习摘要"
            aria-busy={status === "loading"}
          >
            {status === "error" ? (
              <>
                <p>暂时无法读取今天的本地学习记录。</p>
                <p className="rr-main-local-context">{error}</p>
                <button
                  className="rr-main-today-retry"
                  type="button"
                  onClick={onRetry}
                >
                  重新读取
                </button>
              </>
            ) : (
              <>
                <p>{viewModel.summary}</p>
                <p className="rr-main-local-context">{viewModel.localContext}</p>
              </>
            )}
          </section>

          <section className="rr-main-action-list" aria-label="今天可以开始的学习任务">
            {status === "ready" ? viewModel.actions.map((action) => (
              <button
                className={`rr-main-action-row${action.disabled ? " is-disabled" : ""}`}
                type="button"
                key={action.id}
                disabled={action.disabled}
                onClick={() => onActionSelect(action.id)}
              >
                <span className="rr-main-action-icon"><MainAppIcon name={action.icon} /></span>
                <span className="rr-main-action-copy">
                  <span className="rr-main-action-title">{action.title}</span>
                  <span className="rr-main-action-meta">{action.description}</span>
                </span>
                <span className="rr-main-action-arrow"><MainAppIcon name="arrow" /></span>
              </button>
            )) : null}
          </section>
        </div>
      </section>

      <section className="rr-main-composer-area" aria-label="与 ReadRay 对话">
        <div className="rr-main-composer-inner">
          <form
            className="rr-main-composer"
            onSubmit={submit}
            onClick={(event) => {
              const target = event.target;
              if (target instanceof Element && target.closest("button")) {
                return;
              }
              inputRef.current?.focus();
            }}
          >
            <textarea
              ref={inputRef}
              rows={1}
              value={draft}
              aria-label="对话内容"
              placeholder={viewModel.composerPlaceholder}
              onChange={(event) => setDraft(event.target.value)}
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
            <div className="rr-main-composer-actions">
              <button
                className="rr-main-send"
                type="submit"
                aria-label="发送并开始持续对话"
                disabled={!draft.trim()}
              >
                <MainAppIcon name="send-up" />
              </button>
            </div>
          </form>
        </div>
      </section>
    </main>
  );
}

export default TodayPage;
