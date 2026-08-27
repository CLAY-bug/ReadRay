import { useEffect, type KeyboardEvent } from "react";
import type { ExplanationResult } from "../explanationViewModel";
import {
  beginOverlayWindowDrag,
  overlayWindowDragCommands,
} from "../overlayWindowDrag";
import ExplanationResultContent from "./ExplanationResultContent";

export type CenteredResult = ExplanationResult;

type CenteredResultPanelProps = {
  query: string;
  result: CenteredResult;
  open: boolean;
  onQueryChange: (value: string) => void;
  onSubmit: (value: string) => void;
};

function CenteredResultPanel({
  query,
  result,
  open,
  onQueryChange,
  onSubmit,
}: CenteredResultPanelProps) {
  useEffect(() => {
    if (!open) {
      return;
    }

    function handleKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onQueryChange("");
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onQueryChange, open]);

  function handleInputKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.preventDefault();
      const nextQuery = event.currentTarget.value.trim();
      if (nextQuery) {
        onSubmit(nextQuery);
      }
    }
  }

  if (!open) {
    return null;
  }

  const resultLabel =
    result.kind === "word" ? result.headword : result.sourceText;

  return (
    <article
      className="centered-result-panel"
      aria-label={`${resultLabel} 的居中解释结果`}
      onMouseDown={(event) =>
        beginOverlayWindowDrag(
          event,
          overlayWindowDragCommands,
          "input, .centered-result-panel__body",
        )
      }
    >
      <span
        className="centered-result-panel__drag-region"
        data-tauri-drag-region
        aria-hidden="true"
      />
      <div className="centered-result-panel__query-row">
        <input
          className="centered-result-panel__query"
          type="text"
          value={query}
          aria-label="查询内容"
          spellCheck={false}
          onChange={(event) => onQueryChange(event.target.value)}
          onKeyDown={handleInputKeyDown}
        />
        <span className="centered-result-panel__state" aria-hidden="true" />
      </div>

      <div className="centered-result-panel__divider" />

      <div className="centered-result-panel__body">
        <ExplanationResultContent result={result} highlightText={query} />
      </div>
    </article>
  );
}

export default CenteredResultPanel;
