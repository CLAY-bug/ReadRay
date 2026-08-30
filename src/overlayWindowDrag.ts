import { invoke } from "@tauri-apps/api/core";

// overlay 输入框、结果面板、Quick AI 面板与划词锚定浮层共用同一套
// mousedown → mousemove → mouseup 原生窗口拖动接线；三段命令名按窗口角色传入。
export type OverlayDragCommands = {
  begin: string;
  drag: string;
  finish: string;
};

export const overlayWindowDragCommands: OverlayDragCommands = {
  begin: "begin_overlay_window_drag",
  drag: "drag_overlay_window",
  finish: "finish_overlay_window_drag",
};

// 锚定拖动不写 overlay 位置缓存：只对本次显示生效，下次划词重新锚定。
export const anchoredWindowDragCommands: OverlayDragCommands = {
  begin: "begin_anchored_window_drag",
  drag: "drag_anchored_window",
  finish: "finish_anchored_window_drag",
};

// 固定时仍是同一个 overlay 页面，只把拖动命令切换到固定卡生命周期。
export const pinnedCardDragCommands: OverlayDragCommands = {
  begin: "begin_pinned_card_drag",
  drag: "drag_pinned_card",
  finish: "finish_pinned_card_drag",
};

type DragStartEvent = {
  button: number;
  screenX: number;
  screenY: number;
  target: EventTarget | null;
  preventDefault: () => void;
};

export function beginOverlayWindowDrag(
  event: DragStartEvent,
  commands: OverlayDragCommands,
  exclusionSelector?: string,
): boolean {
  if (event.button !== 0) {
    return false;
  }
  if (
    exclusionSelector &&
    event.target instanceof HTMLElement &&
    event.target.closest(exclusionSelector)
  ) {
    return false;
  }

  event.preventDefault();
  invoke(commands.begin, {
    pointerX: event.screenX,
    pointerY: event.screenY,
  }).catch(() => undefined);

  function handleMouseMove(moveEvent: globalThis.MouseEvent) {
    invoke(commands.drag, {
      pointerX: moveEvent.screenX,
      pointerY: moveEvent.screenY,
    }).catch(() => undefined);
  }

  function handleMouseUp() {
    window.removeEventListener("mousemove", handleMouseMove);
    window.removeEventListener("mouseup", handleMouseUp);
    invoke(commands.finish).catch(() => undefined);
  }

  window.addEventListener("mousemove", handleMouseMove);
  window.addEventListener("mouseup", handleMouseUp);
  return true;
}
