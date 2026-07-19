import { writingPatterns, type WritingSnapshot } from "../writingViewModel";

type DiffToken = {
  value: string;
  changed: boolean;
};

type WritingCompareViewProps = {
  original: WritingSnapshot;
  current: WritingSnapshot;
  origin: "review" | "completed";
  checking: boolean;
  onBack: () => void;
  onCheckAgain: () => void;
  onFinish: () => void;
};

function tokenize(value: string) {
  return value.match(/\s+|[A-Za-z]+(?:['’][A-Za-z]+)?|[^A-Za-z\s]/g) ?? [];
}

function diffText(before: string, after: string) {
  const originalTokens = tokenize(before);
  const currentTokens = tokenize(after);
  const lcs = Array.from(
    { length: originalTokens.length + 1 },
    () => new Uint16Array(currentTokens.length + 1),
  );

  for (let row = originalTokens.length - 1; row >= 0; row -= 1) {
    for (let column = currentTokens.length - 1; column >= 0; column -= 1) {
      lcs[row][column] = originalTokens[row] === currentTokens[column]
        ? lcs[row + 1][column + 1] + 1
        : Math.max(lcs[row + 1][column], lcs[row][column + 1]);
    }
  }

  const original: DiffToken[] = [];
  const current: DiffToken[] = [];
  let row = 0;
  let column = 0;

  while (row < originalTokens.length || column < currentTokens.length) {
    if (
      row < originalTokens.length
      && column < currentTokens.length
      && originalTokens[row] === currentTokens[column]
    ) {
      original.push({ value: originalTokens[row], changed: false });
      current.push({ value: currentTokens[column], changed: false });
      row += 1;
      column += 1;
    } else if (
      column < currentTokens.length
      && (row === originalTokens.length || lcs[row][column + 1] >= lcs[row + 1][column])
    ) {
      current.push({ value: currentTokens[column], changed: true });
      column += 1;
    } else {
      original.push({ value: originalTokens[row], changed: true });
      row += 1;
    }
  }

  return { original, current };
}

function changedSectionCount(original: WritingSnapshot, current: WritingSnapshot) {
  let count = original.title === current.title ? 0 : 1;
  const length = Math.max(original.paragraphs.length, current.paragraphs.length);
  for (let index = 0; index < length; index += 1) {
    if ((original.paragraphs[index] ?? "") !== (current.paragraphs[index] ?? "")) {
      count += 1;
    }
  }
  return count;
}

function DiffContent({
  tokens,
  side,
}: {
  tokens: DiffToken[];
  side: "original" | "current";
}) {
  return tokens.map((token, index) => {
    if (!token.changed) {
      return <span key={`${token.value}-${index}`}>{token.value}</span>;
    }
    return side === "original"
      ? <del key={`${token.value}-${index}`}>{token.value}</del>
      : <ins key={`${token.value}-${index}`}>{token.value}</ins>;
  });
}

function WritingCompareView({
  original,
  current,
  origin,
  checking,
  onBack,
  onCheckAgain,
  onFinish,
}: WritingCompareViewProps) {
  const titleDiff = diffText(original.title, current.title);
  const paragraphCount = Math.max(original.paragraphs.length, current.paragraphs.length);
  const paragraphDiffs = Array.from({ length: paragraphCount }, (_, index) => (
    diffText(original.paragraphs[index] ?? "", current.paragraphs[index] ?? "")
  ));
  const editCount = changedSectionCount(original, current);

  return (
    <section className="rr-writing-compare" aria-labelledby="rr-writing-compare-heading" data-testid="writing-compare-view">
      <header className="rr-writing-compare-head">
        <div>
          <h1 id="rr-writing-compare-heading">这次修改保留了你的原意</h1>
          <p>只对照你亲自完成的修改；ReadRay 不会生成替代全文。</p>
        </div>
        <div className="rr-writing-compare-actions">
          <button className="rr-writing-btn is-ghost" type="button" onClick={onBack}>{origin === "completed" ? "返回完成稿" : "返回编辑"}</button>
          {origin === "review" ? (
            <>
              <button className="rr-writing-btn is-secondary" type="button" disabled={checking} onClick={onCheckAgain}>{checking ? "检查中…" : "再次检查"}</button>
              <button className="rr-writing-btn is-primary" type="button" onClick={onFinish}>完成写作</button>
            </>
          ) : null}
        </div>
      </header>

      <div className="rr-writing-diff-grid" aria-label="初稿与当前稿对比">
        <article className="rr-writing-diff-pane">
          <div className="rr-writing-diff-label"><span>初稿</span><span>你的原始表达</span></div>
          <div className="rr-writing-diff-scroll">
            <h2><DiffContent tokens={titleDiff.original} side="original" /></h2>
            <div className="rr-writing-diff-copy">
              {paragraphDiffs.map((diff, index) => <p key={index}><DiffContent tokens={diff.original} side="original" /></p>)}
            </div>
          </div>
        </article>
        <article className="rr-writing-diff-pane">
          <div className="rr-writing-diff-label"><span>当前稿</span><span>{editCount} 处内容有修改</span></div>
          <div className="rr-writing-diff-scroll">
            <h2><DiffContent tokens={titleDiff.current} side="current" /></h2>
            <div className="rr-writing-diff-copy">
              {paragraphDiffs.map((diff, index) => <p key={index}><DiffContent tokens={diff.current} side="current" /></p>)}
            </div>
          </div>
        </article>
      </div>

      <section className="rr-writing-patterns" aria-label="本次语言模式总结">
        <div className="rr-writing-patterns-title">两个值得带走的模式</div>
        {writingPatterns.map((pattern) => (
          <article key={pattern.id}>
            <span>{pattern.id}</span>
            <div><h2>{pattern.title}</h2><p>{pattern.description}</p></div>
          </article>
        ))}
      </section>
    </section>
  );
}

export default WritingCompareView;
