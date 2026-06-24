import { useEffect, useRef, type FormEvent, type KeyboardEvent } from "react";

type CenteredCommandInputProps = {
  value: string;
  onValueChange: (value: string) => void;
  open: boolean;
  loading: boolean;
  error?: string;
  onSubmit: (value: string) => void;
  onOpenChange: (open: boolean) => void;
};

function CenteredCommandInput({
  value,
  onValueChange,
  open,
  loading,
  error,
  onSubmit,
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
      onOpenChange(false);
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      submitCurrentValue();
    }
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
        onSubmit={handleSubmit}
      >
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
