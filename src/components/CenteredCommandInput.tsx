import {
  useEffect,
  useRef,
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";

type CenteredCommandInputProps = {
  value: string;
  onValueChange: (value: string) => void;
  open: boolean;
  loading: boolean;
  error?: string;
  onSubmit: (value: string) => void;
  onQuickAi: (value: string) => void;
  onOpenChange: (open: boolean) => void;
};

function CenteredCommandInput({
  value,
  onValueChange,
  open,
  loading,
  error,
  onSubmit,
  onQuickAi,
  onOpenChange,
}: CenteredCommandInputProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const trimmedValue = value.trim();

  useEffect(() => {
    if (!open) {
      return;
    }

    window.requestAnimationFrame(() => {
      inputRef.current?.focus({ preventScroll: true });
    });
  }, [open]);

  function submitCurrentValue() {
    if (!trimmedValue || loading) {
      return;
    }

    onSubmit(trimmedValue);
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    submitCurrentValue();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      if (value || loading || error) {
        onValueChange("");
      } else {
        onOpenChange(false);
      }
      return;
    }

    if (event.key === "Tab") {
      event.preventDefault();
      if (!loading) {
        onQuickAi(trimmedValue);
      }
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      submitCurrentValue();
    }
  }

  function handleWindowDrag(event: MouseEvent<HTMLElement>) {
    if (event.button !== 0) {
      return;
    }

    if (event.target instanceof HTMLElement && event.target.closest("input")) {
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

  return (
    <section
      className={`centered-command-input${error ? " is-error-lite" : ""}`}
      aria-label="无选区时的居中查询输入"
    >
      <form
        className={`centered-command-input__box${
          loading ? " is-loading" : ""
        }${error ? " is-error-lite" : ""}`}
        autoComplete="off"
        onMouseDown={handleWindowDrag}
        onSubmit={handleSubmit}
      >
        <span
          className="centered-command-input__drag-region"
          data-tauri-drag-region
          aria-hidden="true"
        />
        <input
          ref={inputRef}
          className="centered-command-input__field"
          type="text"
          inputMode="text"
          value={value}
          placeholder="解释单词、短语或句子..."
          aria-label="输入英文单词、短语或句子"
          aria-describedby={error ? "centered-command-input-error" : undefined}
          aria-busy={loading}
          spellCheck={false}
          onChange={(event) => onValueChange(event.target.value)}
          onKeyDown={handleKeyDown}
        />
        <span className="centered-command-input__quick-ai" aria-hidden="true">
          <span>Quick AI</span>
          <kbd>Tab</kbd>
        </span>
        <span className="centered-command-input__state" aria-hidden="true">
          <span className="centered-command-input__loading-dot" />
        </span>
      </form>
      {error ? (
        <p
          className="centered-command-input__error"
          id="centered-command-input-error"
        >
          {error}
        </p>
      ) : null}
    </section>
  );
}

export default CenteredCommandInput;
