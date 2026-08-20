import {
  type CSSProperties,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  preferredAnchoredWidth,
  type ExplanationResult,
} from "../explanationViewModel";
import ExplanationResultContent from "./ExplanationResultContent";

export type AnchorRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type AnchoredResultPopoverProps = {
  result: ExplanationResult;
  anchorRect: AnchorRect | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  embedded?: boolean;
  highlightText?: string;
  onContentSizeChange?: (size: { width: number; height: number }) => void;
};

type PopoverSide = "top" | "bottom";

type PopoverPlacement = {
  side: PopoverSide;
  style: CSSProperties;
};

const viewportMargin = 12;
const anchorGap = 12;
const windowInset = 8;

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(value, max));
}

function placePopover(
  anchorRect: AnchorRect,
  popoverRect: { width: number; height: number },
): PopoverPlacement {
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;
  const maxLeft = Math.max(
    viewportMargin,
    viewportWidth - popoverRect.width - viewportMargin,
  );
  const preferredBottomTop = anchorRect.y + anchorRect.height + anchorGap;
  const hasBottomSpace =
    preferredBottomTop + popoverRect.height + viewportMargin <= viewportHeight;
  const hasTopSpace =
    anchorRect.y - popoverRect.height - anchorGap >= viewportMargin;
  const side: PopoverSide = hasBottomSpace || !hasTopSpace ? "bottom" : "top";
  const rawTop =
    side === "bottom"
      ? preferredBottomTop
      : anchorRect.y - popoverRect.height - anchorGap;
  const top = clamp(
    rawTop,
    viewportMargin,
    Math.max(
      viewportMargin,
      viewportHeight - popoverRect.height - viewportMargin,
    ),
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

function resultLabel(result: ExplanationResult) {
  switch (result.kind) {
    case "word":
      return `${result.headword} 的单词解释`;
    case "phrase":
      return `${result.sourceText} 的短语解释`;
    case "sentence":
      return "所选句子的翻译与解释";
    case "paragraph":
      return "所选段落的翻译与解释";
  }
}

function AnchoredResultPopover({
  result,
  anchorRect,
  open,
  onOpenChange,
  embedded = false,
  highlightText,
  onContentSizeChange,
}: AnchoredResultPopoverProps) {
  const popoverRef = useRef<HTMLElement>(null);
  const lastReportedSize = useRef("");
  const [placement, setPlacement] = useState<PopoverPlacement | null>(null);
  const preferredWidth = preferredAnchoredWidth(result);

  useLayoutEffect(() => {
    if (embedded || !open || !anchorRect || !popoverRef.current) {
      return;
    }

    const measuredRect = popoverRef.current.getBoundingClientRect();
    setPlacement(
      placePopover(anchorRect, {
        width: measuredRect.width,
        height: measuredRect.height,
      }),
    );
  }, [anchorRect, embedded, open, result]);

  useLayoutEffect(() => {
    const element = popoverRef.current;
    if (!embedded || !open || !element || !onContentSizeChange) {
      return;
    }

    let animationFrame = 0;
    let settleTimer = 0;
    const reportSize = () => {
      window.cancelAnimationFrame(animationFrame);
      window.clearTimeout(settleTimer);
      settleTimer = window.setTimeout(() => {
        animationFrame = window.requestAnimationFrame(() => {
          const width = Math.ceil(preferredWidth + windowInset * 2);
          // The native window is still using the compact loading width here.
          // Measure at the intended card width so narrow-viewport wrapping does
          // not produce a second height correction after the native resize.
          const previousMaxWidth = element.style.maxWidth;
          element.style.maxWidth = "none";
          const height = Math.ceil(element.scrollHeight + windowInset * 2 + 6);
          element.style.maxWidth = previousMaxWidth;
          const sizeKey = `${width}:${height}`;
          if (lastReportedSize.current !== sizeKey) {
            lastReportedSize.current = sizeKey;
            onContentSizeChange({ width, height });
          }
        });
      }, 32);
    };

    const observer = new ResizeObserver(reportSize);
    observer.observe(element);
    reportSize();

    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(animationFrame);
      window.clearTimeout(settleTimer);
    };
  }, [embedded, onContentSizeChange, open, preferredWidth, result]);

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

  if (!open || (!embedded && !anchorRect)) {
    return null;
  }

  return (
    <section
      ref={popoverRef}
      className={`anchored-result-popover${
        embedded ? " is-window-overlay" : ""
      }`}
      data-side={embedded ? "window" : placement?.side ?? "bottom"}
      data-result-kind={result.kind}
      style={
        embedded
          ? ({
              visibility: "visible",
              "--anchored-preferred-width": `${preferredWidth}px`,
            } as CSSProperties)
          : {
              ...placement?.style,
              visibility: placement ? "visible" : "hidden",
            }
      }
      aria-label={resultLabel(result)}
    >
      <ExplanationResultContent
        result={result}
        highlightText={highlightText}
        variant="anchored"
      />
    </section>
  );
}

export default AnchoredResultPopover;
