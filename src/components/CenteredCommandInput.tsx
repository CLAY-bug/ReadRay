import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  beginOverlayWindowDrag,
  overlayWindowDragCommands,
} from "../overlayWindowDrag";

type VocabularySuggestion = {
  term: string;
  fromHistory: boolean;
};

const SUGGESTION_DEBOUNCE_MS = 200;
const SUGGESTION_MAX_ITEMS = 5;
const INPUT_STAGE_HEIGHT = 58;
const SUGGESTION_ROW_HEIGHT = 34;
const SUGGESTION_PANEL_VERTICAL_SPACE = 16;

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
  const suggestGenerationRef = useRef(0);
  const [suggestions, setSuggestions] = useState<VocabularySuggestion[]>([]);
  const [highlightIndex, setHighlightIndex] = useState(-1);
  const trimmedValue = value.trim();
  const suggestionsVisible = open && !loading && suggestions.length > 0;

  useEffect(() => {
    if (!open) {
      return;
    }

    window.requestAnimationFrame(() => {
      inputRef.current?.focus({ preventScroll: true });
    });
  }, [open]);

  useEffect(() => {
    return () => {
      suggestGenerationRef.current += 1;
      invoke("set_overlay_input_window_height", {
        height: INPUT_STAGE_HEIGHT,
      }).catch(() => undefined);
    };
  }, []);

  useEffect(() => {
    if (!open) {
      setSuggestions([]);
      setHighlightIndex(-1);
      return;
    }

    // 词表匹配与历史前缀均由 Rust 侧权威校验；这里只保留防抖前的廉价门槛。
    const suggestable = trimmedValue.length >= 2 && !/\s/.test(trimmedValue);
    if (!suggestable) {
      clearSuggestions();
      return;
    }

    const generation = suggestGenerationRef.current + 1;
    suggestGenerationRef.current = generation;
    const timer = window.setTimeout(() => {
      invoke<VocabularySuggestion[]>("suggest_vocabulary_terms_command", {
        query: trimmedValue,
        limit: SUGGESTION_MAX_ITEMS,
      })
        .then((result) => {
          if (suggestGenerationRef.current !== generation) {
            return;
          }
          setSuggestions(
            Array.isArray(result)
              ? result.slice(0, SUGGESTION_MAX_ITEMS)
              : [],
          );
          setHighlightIndex(-1);
        })
        .catch(() => undefined);
    }, SUGGESTION_DEBOUNCE_MS);

    return () => window.clearTimeout(timer);
  }, [open, trimmedValue]);

  useEffect(() => {
    if (!open) {
      return;
    }

    const height =
      suggestionsVisible && !loading
        ? INPUT_STAGE_HEIGHT +
          SUGGESTION_PANEL_VERTICAL_SPACE +
          Math.min(suggestions.length, SUGGESTION_MAX_ITEMS) *
            SUGGESTION_ROW_HEIGHT
        : INPUT_STAGE_HEIGHT;
    invoke("set_overlay_input_window_height", { height }).catch(
      () => undefined,
    );
  }, [loading, open, suggestions.length, suggestionsVisible]);

  function clearSuggestions() {
    suggestGenerationRef.current += 1;
    setSuggestions([]);
    setHighlightIndex(-1);
  }

  function submitValue(candidate: string) {
    if (!candidate || loading) {
      return;
    }

    clearSuggestions();
    onSubmit(candidate);
  }

  function submitCurrentValue() {
    if (!trimmedValue || loading) {
      return;
    }

    if (highlightIndex >= 0 && suggestions[highlightIndex]) {
      submitValue(suggestions[highlightIndex].term);
      return;
    }

    clearSuggestions();
    onSubmit(trimmedValue);
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    submitCurrentValue();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      if (suggestionsVisible) {
        event.preventDefault();
        clearSuggestions();
        return;
      }
      event.preventDefault();
      if (value || loading || error) {
        onValueChange("");
      } else {
        onOpenChange(false);
      }
      return;
    }

    if (event.key === "ArrowDown" && suggestionsVisible) {
      event.preventDefault();
      setHighlightIndex((index) =>
        index + 1 >= suggestions.length ? 0 : index + 1,
      );
      return;
    }

    if (event.key === "ArrowUp" && suggestionsVisible) {
      event.preventDefault();
      setHighlightIndex((index) =>
        index <= 0 ? suggestions.length - 1 : index - 1,
      );
      return;
    }

    if (event.key === "Tab") {
      event.preventDefault();
      if (!loading) {
        clearSuggestions();
        onQuickAi(trimmedValue);
      }
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
      className="centered-command-input"
      aria-label="无选区时的居中查询输入"
    >
      <form
        className={`centered-command-input__box${
          loading ? " is-loading" : ""
        }${error ? " is-error-lite" : ""}${
          suggestionsVisible ? " has-suggestions" : ""
        }`}
        autoComplete="off"
        onMouseDown={(event) =>
          beginOverlayWindowDrag(event, overlayWindowDragCommands, "input")
        }
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
      {suggestionsVisible ? (
        <ul
          className="centered-command-input__suggestions"
          role="listbox"
          aria-label="输入建议"
        >
          {suggestions.map((suggestion, index) => (
            <li key={suggestion.term} role="presentation">
              <button
                type="button"
                role="option"
                aria-selected={index === highlightIndex}
                className={
                  index === highlightIndex ? "is-active" : undefined
                }
                title={suggestion.fromHistory ? "你查过的词" : undefined}
                onMouseDown={(event) => {
                  event.preventDefault();
                  submitValue(suggestion.term);
                }}
                onMouseEnter={() => setHighlightIndex(index)}
              >
                {suggestion.term}
              </button>
            </li>
          ))}
        </ul>
      ) : null}
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
