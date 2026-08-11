import type { ReactNode } from "react";
import type { ExplanationResult } from "../explanationViewModel";

type ExplanationResultContentProps = {
  result: ExplanationResult;
  highlightText?: string;
  variant?: "default" | "anchored";
};

function renderHighlightedText(text: string, highlightText?: string): ReactNode {
  if (!highlightText) {
    return text;
  }

  const start = text.toLocaleLowerCase().indexOf(highlightText.toLocaleLowerCase());
  if (start === -1) {
    return text;
  }

  return (
    <>
      {text.slice(0, start)}
      <strong>{text.slice(start, start + highlightText.length)}</strong>
      {text.slice(start + highlightText.length)}
    </>
  );
}

function BilingualText({
  en,
  zh,
  highlightText,
}: {
  en: string;
  zh?: string;
  highlightText?: string;
}) {
  return (
    <div className="explanation-content__bilingual">
      <p className="explanation-content__english">
        {renderHighlightedText(en, highlightText)}
      </p>
      {zh ? <p className="explanation-content__chinese">{zh}</p> : null}
    </div>
  );
}

function KeyPoints({
  items,
}: {
  items: Array<{ expression: string; meaning: string }>;
}) {
  if (items.length === 0) {
    return null;
  }

  return (
    <section className="explanation-content__section">
      <h2>关键表达</h2>
      <div className="explanation-content__item-list">
        {items.map((item) => (
          <div className="explanation-content__item" key={item.expression}>
            <strong>{item.expression}</strong>
            <span>{item.meaning}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

function Examples({
  items,
  highlightText,
  compact = false,
}: {
  items: Array<{ en: string; zh: string }>;
  highlightText?: string;
  compact?: boolean;
}) {
  if (items.length === 0) {
    return null;
  }

  return (
    <section
      className={`explanation-content__section explanation-content__examples-section${
        compact ? " is-compact" : ""
      }`}
    >
      <h2>例句</h2>
      <div className="explanation-content__examples">
        {items.map((item) => (
          <BilingualText
            key={`${item.en}:${item.zh}`}
            en={item.en}
            zh={item.zh}
            highlightText={highlightText}
          />
        ))}
      </div>
    </section>
  );
}

function ExplanationResultContent({
  result,
  highlightText,
  variant = "default",
}: ExplanationResultContentProps) {
  if (result.kind === "word") {
    const isAnchored = variant === "anchored";

    return (
      <div className="explanation-content is-word">
        <header className="explanation-content__header">
          <div className="explanation-content__title-line">
            <h1>{result.headword}</h1>
            {isAnchored && result.phonetic ? (
              <span className="explanation-content__phonetic">
                {result.phonetic}
              </span>
            ) : null}
            {result.partOfSpeech ? <span>{result.partOfSpeech}</span> : null}
          </div>
          {!isAnchored && result.phonetic ? (
            <p className="explanation-content__phonetic">{result.phonetic}</p>
          ) : null}
        </header>

        {result.contextMeaning ? (
          <section className="explanation-content__primary explanation-content__context">
            {!isAnchored ? <h2>当前语境</h2> : null}
            <p>{result.contextMeaning}</p>
          </section>
        ) : null}

        {result.sourceSentence ? (
          <section className="explanation-content__section explanation-content__source-sentence">
            {!isAnchored ? <h2>原句</h2> : null}
            <BilingualText
              en={result.sourceSentence}
              zh={result.sourceSentenceZh}
              highlightText={highlightText}
            />
          </section>
        ) : null}

        <section className="explanation-content__section explanation-content__basic-meanings">
          {!isAnchored ? <h2>基础释义</h2> : null}
          <p className="explanation-content__meanings">
            {result.basicMeanings.join("；")}
          </p>
        </section>

        {result.phrases.length > 0 ? (
          <section className="explanation-content__section explanation-content__supporting">
            <h2>常见搭配</h2>
            <div className="explanation-content__item-list">
              {result.phrases.map((item) => (
                <div className="explanation-content__item" key={item.phrase}>
                  <strong>{item.phrase}</strong>
                  <span>{item.meaning}</span>
                </div>
              ))}
            </div>
          </section>
        ) : null}

        {result.nearMeanings.length > 0 ? (
          <section className="explanation-content__section explanation-content__supporting">
            <h2>近义辨析</h2>
            <div className="explanation-content__item-list">
              {result.nearMeanings.map((item) => (
                <div className="explanation-content__item" key={item.term}>
                  <strong>{item.term}</strong>
                  <span>{item.meaning}</span>
                </div>
              ))}
            </div>
          </section>
        ) : null}

        <Examples
          items={result.examples}
          highlightText={highlightText}
          compact={isAnchored}
        />
        {!isAnchored && result.reviewHint ? (
          <p className="explanation-content__note">{result.reviewHint}</p>
        ) : null}
      </div>
    );
  }

  if (result.kind === "phrase") {
    return (
      <div className="explanation-content is-phrase">
        <header className="explanation-content__header">
          <h1>{result.sourceText}</h1>
        </header>
        <section className="explanation-content__primary">
          <h2>{result.contextMeaning ? "当前语境" : "整体含义"}</h2>
          <p>{result.contextMeaning ?? result.basicMeaning}</p>
        </section>
        {result.contextMeaning ? (
          <section className="explanation-content__section">
            <h2>基础含义</h2>
            <p>{result.basicMeaning}</p>
          </section>
        ) : null}
        {result.sourceSentence ? (
          <section className="explanation-content__section">
            <h2>原句</h2>
            <BilingualText
              en={result.sourceSentence}
              zh={result.sourceSentenceZh}
              highlightText={highlightText}
            />
          </section>
        ) : null}
        {result.composition ? (
          <section className="explanation-content__section">
            <h2>结构</h2>
            <p>{result.composition}</p>
          </section>
        ) : null}
        <Examples items={result.examples} highlightText={highlightText} />
        {result.reviewHint ? (
          <p className="explanation-content__note">{result.reviewHint}</p>
        ) : null}
      </div>
    );
  }

  if (result.kind === "sentence") {
    return (
      <div className="explanation-content is-sentence">
        <section className="explanation-content__source">
          <h2>原文</h2>
          <p>{result.sourceText}</p>
        </section>
        <section className="explanation-content__translation">
          <h1>翻译</h1>
          <p>{result.translation}</p>
        </section>
        <KeyPoints items={result.keyPoints} />
        {result.explanation ? (
          <section className="explanation-content__section">
            <h2>理解</h2>
            <p>{result.explanation}</p>
          </section>
        ) : null}
        {result.reviewHint ? (
          <p className="explanation-content__note">{result.reviewHint}</p>
        ) : null}
      </div>
    );
  }

  return (
    <div className="explanation-content is-paragraph">
      <section className="explanation-content__source">
        <h2>原文</h2>
        <p>{result.sourceText}</p>
      </section>
      <section className="explanation-content__translation">
        <h1>翻译</h1>
        <p>{result.translation}</p>
      </section>
      <KeyPoints items={result.keyPoints} />
      {result.summary ? (
        <section className="explanation-content__section">
          <h2>概括</h2>
          <p>{result.summary}</p>
        </section>
      ) : null}
    </div>
  );
}

export default ExplanationResultContent;
