import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  type RefObject,
} from "react";

function resizeTextarea(input: HTMLTextAreaElement) {
  input.style.height = "auto";

  const contentHeight = input.scrollHeight;
  const parsedMaxHeight = Number.parseFloat(getComputedStyle(input).maxHeight);
  const maxHeight = Number.isFinite(parsedMaxHeight)
    ? parsedMaxHeight
    : contentHeight;

  input.style.height = `${Math.min(contentHeight, maxHeight)}px`;
  input.style.overflowY = contentHeight > maxHeight ? "auto" : "hidden";
}

export function useAutoResizeTextarea(
  inputRef: RefObject<HTMLTextAreaElement | null>,
  value: string,
) {
  const frameRef = useRef<number | null>(null);

  const scheduleResize = useCallback(() => {
    if (frameRef.current !== null) {
      return;
    }

    frameRef.current = window.requestAnimationFrame(() => {
      frameRef.current = null;
      if (inputRef.current) {
        resizeTextarea(inputRef.current);
      }
    });
  }, [inputRef]);

  useLayoutEffect(() => {
    scheduleResize();
  }, [scheduleResize, value]);

  useEffect(() => {
    let windowResizeTimer: number | undefined;

    const resizeAfterWindowSettles = () => {
      window.clearTimeout(windowResizeTimer);
      windowResizeTimer = window.setTimeout(scheduleResize, 120);
    };

    window.addEventListener("resize", resizeAfterWindowSettles);
    return () => {
      window.removeEventListener("resize", resizeAfterWindowSettles);
      window.clearTimeout(windowResizeTimer);
      if (frameRef.current !== null) {
        window.cancelAnimationFrame(frameRef.current);
        frameRef.current = null;
      }
    };
  }, [scheduleResize]);
}
