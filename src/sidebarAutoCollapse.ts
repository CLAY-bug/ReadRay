// 主侧栏随窗口宽度的自动收放状态机。
// 自动折叠只在“跨越阈值”时发生，并保留 80px 迟滞带，拖动在阈值附近不会来回抖动；
// 手动折叠优先于自动状态：用户手动收起后，放大窗口不会自动展开。

export const MAIN_SIDEBAR_AUTO_COLLAPSE_WIDTH = 1000;
export const MAIN_SIDEBAR_AUTO_EXPAND_WIDTH = 1080;

export type SidebarAutoCollapseState = {
  previousWidth: number | null;
  autoCollapsed: boolean;
};

export function createSidebarAutoCollapseState(): SidebarAutoCollapseState {
  return { previousWidth: null, autoCollapsed: false };
}

export function reduceSidebarAutoCollapse(
  state: SidebarAutoCollapseState,
  width: number,
  manuallyCollapsed: boolean,
): SidebarAutoCollapseState {
  if (!Number.isFinite(width) || width <= 0) {
    return state;
  }

  const { previousWidth } = state;
  let { autoCollapsed } = state;

  // 首次测量视为从宽到窄/从窄到宽的跨越，使窄窗口启动时即可自动折叠。
  const crossedBelowCollapse =
    width < MAIN_SIDEBAR_AUTO_COLLAPSE_WIDTH &&
    (previousWidth === null ||
      previousWidth >= MAIN_SIDEBAR_AUTO_COLLAPSE_WIDTH);
  const crossedAboveExpand =
    width > MAIN_SIDEBAR_AUTO_EXPAND_WIDTH &&
    (previousWidth === null ||
      previousWidth <= MAIN_SIDEBAR_AUTO_EXPAND_WIDTH);

  if (crossedBelowCollapse) {
    if (!manuallyCollapsed) {
      autoCollapsed = true;
    }
  } else if (crossedAboveExpand) {
    autoCollapsed = false;
  }

  if (autoCollapsed === state.autoCollapsed && previousWidth === width) {
    return state;
  }
  return { previousWidth: width, autoCollapsed };
}

// 用户在自动折叠状态下手动展开：清除自动状态，之后只有再次跨越阈值才重新自动折叠。
export function releaseSidebarAutoCollapse(
  state: SidebarAutoCollapseState,
): SidebarAutoCollapseState {
  if (!state.autoCollapsed) {
    return state;
  }
  return { previousWidth: state.previousWidth, autoCollapsed: false };
}
