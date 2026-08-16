import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  MAIN_SIDEBAR_AUTO_COLLAPSE_WIDTH,
  MAIN_SIDEBAR_AUTO_EXPAND_WIDTH,
  createSidebarAutoCollapseState,
  reduceSidebarAutoCollapse,
  releaseSidebarAutoCollapse,
} from "../src/sidebarAutoCollapse.ts";

const WIDE = MAIN_SIDEBAR_AUTO_EXPAND_WIDTH + 40;
const HYSTERESIS = MAIN_SIDEBAR_AUTO_COLLAPSE_WIDTH + 40;
const NARROW = MAIN_SIDEBAR_AUTO_COLLAPSE_WIDTH - 20;

test("宽窗口首次测量保持展开，窄窗口首次测量自动折叠", () => {
  const wide = reduceSidebarAutoCollapse(
    createSidebarAutoCollapseState(),
    WIDE,
    false,
  );
  assert.equal(wide.autoCollapsed, false);

  const narrow = reduceSidebarAutoCollapse(
    createSidebarAutoCollapseState(),
    NARROW,
    false,
  );
  assert.equal(narrow.autoCollapsed, true);

  const narrowManual = reduceSidebarAutoCollapse(
    createSidebarAutoCollapseState(),
    NARROW,
    true,
  );
  assert.equal(narrowManual.autoCollapsed, false);
});

test("只有跨越阈值才切换自动折叠，迟滞带内保持原状态", () => {
  let state = createSidebarAutoCollapseState();
  state = reduceSidebarAutoCollapse(state, WIDE, false);
  assert.equal(state.autoCollapsed, false);

  // 进入迟滞带（未低于折叠阈值）：保持展开。
  state = reduceSidebarAutoCollapse(state, HYSTERESIS, false);
  assert.equal(state.autoCollapsed, false);

  // 跨越折叠阈值：自动折叠；带内继续缩小不重复触发。
  state = reduceSidebarAutoCollapse(state, NARROW, false);
  assert.equal(state.autoCollapsed, true);
  const collapsed = reduceSidebarAutoCollapse(state, NARROW - 60, false);
  assert.equal(collapsed.autoCollapsed, true);

  // 回到迟滞带：保持折叠，避免边界附近来回抖动。
  state = reduceSidebarAutoCollapse(collapsed, HYSTERESIS, false);
  assert.equal(state.autoCollapsed, true);

  // 跨越展开阈值：自动展开。
  state = reduceSidebarAutoCollapse(state, WIDE, false);
  assert.equal(state.autoCollapsed, false);
});

test("手动折叠不产生自动状态，放大后也不被自动展开语义影响", () => {
  let state = createSidebarAutoCollapseState();
  state = reduceSidebarAutoCollapse(state, WIDE, true);
  state = reduceSidebarAutoCollapse(state, NARROW, true);
  assert.equal(state.autoCollapsed, false);
  state = reduceSidebarAutoCollapse(state, WIDE, true);
  assert.equal(state.autoCollapsed, false);
});

test("用户在窄窗自动折叠后手动展开，同带内保持展开，再次跨越后恢复自动", () => {
  let state = createSidebarAutoCollapseState();
  state = reduceSidebarAutoCollapse(state, WIDE, false);
  state = reduceSidebarAutoCollapse(state, NARROW, false);
  assert.equal(state.autoCollapsed, true);

  // 用户在窄窗手动展开：清除自动状态，同带内继续缩小不再自动折叠。
  state = releaseSidebarAutoCollapse(state);
  assert.equal(state.autoCollapsed, false);
  state = reduceSidebarAutoCollapse(state, NARROW - 60, false);
  assert.equal(state.autoCollapsed, false);

  // 先变宽再变窄：重新跨越折叠阈值后恢复自动折叠。
  state = reduceSidebarAutoCollapse(state, WIDE, false);
  state = reduceSidebarAutoCollapse(state, NARROW, false);
  assert.equal(state.autoCollapsed, true);
});

test("非有限或非正宽度输入被忽略", () => {
  const state = createSidebarAutoCollapseState();
  assert.equal(reduceSidebarAutoCollapse(state, Number.NaN, false), state);
  assert.equal(reduceSidebarAutoCollapse(state, 0, false), state);
  assert.equal(reduceSidebarAutoCollapse(state, -10, false), state);
});

test("主应用外壳按有效折叠接线自动收放，并在缩放会话中抑制宽度过渡", async () => {
  const [shellSource, sidebarSource, stylesheet] = await Promise.all([
    readFile(new URL("../src/components/MainAppShell.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/components/MainSidebar.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/styles/main-app.css", import.meta.url), "utf8"),
  ]);

  // 外壳通过 ResizeObserver 驱动纯状态机，并区分手动与自动折叠。
  assert.match(shellSource, /new ResizeObserver/);
  assert.match(shellSource, /reduceSidebarAutoCollapse/);
  assert.match(shellSource, /releaseSidebarAutoCollapse/);
  assert.match(shellSource, /sidebarCollapsed \|\| sidebarAutoCollapsed/);
  // 自动折叠期间仍保留 hover 预览：peek 触发条件使用有效折叠而非仅手动折叠。
  assert.match(shellSource, /sidebarEffectiveCollapsed && sidebarPeekOpen/);
  // 固定收放由布局槽推动正文；hover 只打开绝对定位侧栏，不恢复布局槽宽度。
  assert.match(shellSource, /className="rr-main-sidebar-slot"[\s\S]*?<MainSidebar/);
  assert.match(
    stylesheet,
    /\.rr-main-sidebar-slot\s*\{[\s\S]*?flex:\s*0 0 var\(--rr-main-sidebar-layout-width\)/,
  );
  assert.match(
    stylesheet,
    /\.rr-main-app\.is-sidebar-collapsed\s*\{[\s\S]*?--rr-main-sidebar-layout-width:\s*0px/,
  );
  assert.match(
    stylesheet,
    /\.rr-main-app\.is-sidebar-collapsed\.is-sidebar-peeking \.rr-main-sidebar\s*\{[\s\S]*?transform:\s*translateX\(0\)/,
  );

  // 拖拽调宽来自侧栏 resizer 的逐帧回调，外壳在拖拽期间挂出独立类名。
  assert.match(sidebarSource, /onWidthChange/);
  assert.match(shellSource, /is-sidebar-resizing/);
  assert.match(shellSource, /is-window-resizing/);
  assert.match(shellSource, /windowResizeSettleTimerRef/);
  assert.match(shellSource, /}, 120\);/);
  assert.match(
    stylesheet,
    /\.rr-main-app\.is-sidebar-resizing \.rr-main-sidebar/,
  );
  assert.match(
    stylesheet,
    /\.rr-main-app\.is-window-resizing \.rr-main-sidebar/,
  );
  assert.match(
    stylesheet,
    /\.rr-main-app\.is-sidebar-resizing \.rr-main-sidebar-slot/,
  );
  assert.match(
    stylesheet,
    /\.rr-main-app\.is-window-resizing \.rr-main-sidebar-slot/,
  );
});
