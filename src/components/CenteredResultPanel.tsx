import { useEffect, type KeyboardEvent } from "react";

export type CenteredResultPhrase = {
  phrase: string;
  meaning: string;
};

export type CenteredResult = {
  word: string;
  phonetic: string;
  partOfSpeech?: string;
  definition: string;
  phrases: CenteredResultPhrase[];
  nearMeaningTitle: string;
  nearMeanings: CenteredResultPhrase[];
  example: string;
  exampleZh: string;
};

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

  if (!open) {
    return null;
  }

  return (
    <article
      className="centered-result-panel"
      aria-label={`${result.word} 的居中解释结果`}
    >
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
        <header className="centered-result-panel__head">
          <div className="centered-result-panel__word-main">
            <span className="centered-result-panel__word">{result.word}</span>
            <span className="centered-result-panel__phonetic">
              {result.phonetic}
            </span>
          </div>
          {result.partOfSpeech ? (
            <p className="centered-result-panel__part-of-speech">
              {result.partOfSpeech}
            </p>
          ) : null}
        </header>

        <p className="centered-result-panel__definition">
          {result.definition}
        </p>

        <section
          className="centered-result-panel__section"
          aria-label="常见语块"
        >
          <div className="centered-result-panel__section-label">常见语块</div>
          <div className="centered-result-panel__phrase-list">
            {result.phrases.map((item) => (
              <div className="centered-result-panel__phrase-row" key={item.phrase}>
                <span className="centered-result-panel__phrase">
                  {item.phrase}
                </span>
                <span className="centered-result-panel__phrase-meaning">
                  {item.meaning}
                </span>
              </div>
            ))}
          </div>
        </section>

        <section
          className="centered-result-panel__section"
          aria-label={result.nearMeaningTitle}
        >
          <div className="centered-result-panel__section-label">
            {result.nearMeaningTitle}
          </div>
          <div className="centered-result-panel__phrase-list">
            {result.nearMeanings.map((item) => (
              <div className="centered-result-panel__phrase-row" key={item.phrase}>
                <span className="centered-result-panel__phrase">
                  {item.phrase}
                </span>
                <span className="centered-result-panel__phrase-meaning">
                  {item.meaning}
                </span>
              </div>
            ))}
          </div>
        </section>

        <section className="centered-result-panel__section" aria-label="例句">
          <div className="centered-result-panel__section-label">例句</div>
          <div className="centered-result-panel__example">
            <div className="centered-result-panel__example-en">
              {result.example}
            </div>
            <div className="centered-result-panel__example-zh">
              {result.exampleZh}
            </div>
          </div>
        </section>
      </div>
    </article>
  );
}

export default CenteredResultPanel;
