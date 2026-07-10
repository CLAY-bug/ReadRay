import { useEffect, type KeyboardEvent, type MouseEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ExplanationResult } from "../explanationViewModel";
import ExplanationResultContent from "./ExplanationResultContent";

export type CenteredResult = ExplanationResult;

type CenteredResultPanelProps = {
  query: string;
  result: CenteredResult;
  open: boolean;
  onQueryChange: (value: string) => void;
  onSubmit: (value: string) => void;
  onOpenChange: (open: boolean) => void;
};

function CenteredResultPanel({
  query,
  result,
  open,
  onQueryChange,
  onSubmit,
  onOpenChange,
}: CenteredResultPanelProps) {
  useEffect(() => {
    if (!open) {
      return;
    }

    function handleKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        onOpenChange(false);
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onOpenChange, open]);

  function handleInputKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onOpenChange(false);
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      const nextQuery = event.currentTarget.value.trim();
      if (nextQuery) {
        onSubmit(nextQuery);
      }
    }
  }

  function handleWindowDrag(event: MouseEvent<HTMLElement>) {
    if (event.button !== 0) {
      return;
    }

    if (
      event.target instanceof HTMLElement &&
      event.target.closest("input, .centered-result-panel__body")
    ) {
      return;
    }

    event.preventDefault();
    invoke("begin_overlay_window_drag", {
      pointerX: event.screenX,
      pointerY: event.screenY,
    }).catch(() => undefined);

    function handleMouseMove(moveEvent: globalThis.MouseEvent) {
      invoke("drag_overlay_window", {
        pointerX: moveEvent.screenX,
        pointerY: moveEvent.screenY,
      }).catch(() => undefined);
    }

    function handleMouseUp() {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      invoke("finish_overlay_window_drag").catch(() => undefined);
    }

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
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
      onMouseDown={handleWindowDrag}
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
