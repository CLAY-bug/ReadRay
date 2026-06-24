import {
  type CSSProperties,
  type ReactNode,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

export type AnchoredResult = {
  word: string;
  phonetic: string;
  definition: string;
  contextMeaning: string;
  usage: string;
  example: string;
  exampleZh: string;
  highlightText?: string;
};

export type AnchorRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type AnchoredResultPopoverProps = {
  result: AnchoredResult;
  anchorRect: AnchorRect | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

type PopoverSide = "top" | "bottom";

type PopoverPlacement = {
  side: PopoverSide;
  style: CSSProperties;
};

const viewportMargin = 12;
const anchorGap = 12;

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(value, max));
}

function placePopover(
  anchorRect: AnchorRect,
  popoverRect: { width: number; height: number },
): PopoverPlacement {
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;
  const maxLeft = Math.max(viewportMargin, viewportWidth - popoverRect.width - viewportMargin);
  const preferredBottomTop = anchorRect.y + anchorRect.height + anchorGap;
  const hasBottomSpace =
    preferredBottomTop + popoverRect.height + viewportMargin <= viewportHeight;
  const hasTopSpace = anchorRect.y - popoverRect.height - anchorGap >= viewportMargin;
  const side: PopoverSide = hasBottomSpace || !hasTopSpace ? "bottom" : "top";
  const rawTop =
    side === "bottom"
      ? preferredBottomTop
      : anchorRect.y - popoverRect.height - anchorGap;
  const top = clamp(
    rawTop,
    viewportMargin,
    Math.max(viewportMargin, viewportHeight - popoverRect.height - viewportMargin),
  );
  const left = clamp(
    anchorRect.x + anchorRect.width / 2 - popoverRect.width / 2,
    viewportMargin,
    maxLeft,
  );
  const arrowX = clamp(
    anchorRect.x + anchorRect.width / 2 - left - 6,
    20,
    popoverRect.width - 28,
  );

  return {
    side,
    style: {
      left,
      top,
      "--popover-arrow-x": `${arrowX}px`,
    } as CSSProperties,
  };
}

function renderHighlightedText(text: string, highlightText?: string): ReactNode {
  if (!highlightText) {
    return text;
  }

  const start = text.indexOf(highlightText);
  if (start === -1) {
    return text;
  }

  return (
    <>
      {text.slice(0, start)}
      <strong>{highlightText}</strong>
      {text.slice(start + highlightText.length)}
    </>
  );
}

function AnchoredResultPopover({
  result,
  anchorRect,
  open,
  onOpenChange,
}: AnchoredResultPopoverProps) {
  const popoverRef = useRef<HTMLElement>(null);
  const [placement, setPlacement] = useState<PopoverPlacement | null>(null);
  const [primaryContextMeaning, ...secondaryContextMeanings] =
    result.contextMeaning.split("；");
  const usageParts = result.usage.split(" = ");

  useLayoutEffect(() => {
    if (!open || !anchorRect || !popoverRef.current) {
      return;
    }

    const measuredRect = popoverRef.current.getBoundingClientRect();
    setPlacement(
      placePopover(anchorRect, {
        width: measuredRect.width,
        height: measuredRect.height,
      }),
    );
  }, [anchorRect, open, result]);

  useEffect(() => {
    if (!open) {
      return;
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onOpenChange(false);
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onOpenChange, open]);

  if (!open || !anchorRect) {
    return null;
  }

  return (
    <section
      ref={popoverRef}
      className="anchored-result-popover"
      data-side={placement?.side ?? "bottom"}
      style={{
        ...placement?.style,
        visibility: placement ? "visible" : "hidden",
      }}
      aria-label={`${result.word} 的紧凑语境解释`}
    >
      <header className="anchored-result-popover__header">
        <h1 className="anchored-result-popover__word">{result.word}</h1>
        <span className="anchored-result-popover__phonetic">
          {result.phonetic}
        </span>
      </header>

      <dl className="anchored-result-popover__meanings">
        <div className="anchored-result-popover__row">
          <dt>基础释义</dt>
          <dd>{result.definition}</dd>
        </div>
        <div className="anchored-result-popover__row">
          <dt>在这句里</dt>
          <dd>
            <strong>{primaryContextMeaning}</strong>
            {secondaryContextMeanings.length > 0
              ? `；${secondaryContextMeanings.join("；")}`
              : ""}
          </dd>
        </div>
        <div className="anchored-result-popover__row">
          <dt>用法</dt>
          <dd>
            <strong>{usageParts[0]}</strong>
            {usageParts[1] ? ` = ${usageParts[1]}` : ""}
          </dd>
        </div>
      </dl>

      <div className="anchored-result-popover__example" aria-label="例句和翻译">
        <p className="anchored-result-popover__example-en">
          {renderHighlightedText(result.example, result.highlightText)}
        </p>
        <p className="anchored-result-popover__example-zh">
          {result.exampleZh}
        </p>
      </div>
    </section>
  );
}

export default AnchoredResultPopover;
