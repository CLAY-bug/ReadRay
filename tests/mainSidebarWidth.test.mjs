import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  MAIN_SIDEBAR_WIDTH_STORAGE_KEY,
  loadMainSidebarWidth,
  parseStoredMainSidebarWidth,
  saveMainSidebarWidth,
} from "../src/mainSidebarWidth.ts";

test("主边栏宽度只恢复当前版本的有限数值并重新应用范围约束", () => {
  assert.equal(parseStoredMainSidebarWidth(null), null);
  assert.equal(parseStoredMainSidebarWidth("not-json"), null);
  assert.equal(parseStoredMainSidebarWidth('{"version":0,"width":280}'), null);
  assert.equal(parseStoredMainSidebarWidth('{"version":1,"width":"280"}'), null);
  assert.equal(parseStoredMainSidebarWidth('{"version":1,"width":280.6}'), 281);
  assert.equal(parseStoredMainSidebarWidth('{"version":1,"width":20}'), 180);
  assert.equal(parseStoredMainSidebarWidth('{"version":1,"width":999}'), 360);
});

test("主边栏存储失败时回退默认布局且保存值保持版本化", () => {
  assert.equal(
    loadMainSidebarWidth({
      getItem() {
        throw new Error("storage unavailable");
      },
      setItem() {},
    }),
    null,
  );

  const writes = [];
  const storage = {
    getItem() {
      return null;
    },
    setItem(key, value) {
      writes.push([key, value]);
    },
  };
  saveMainSidebarWidth(288, storage);
  saveMainSidebarWidth(Number.NaN, storage);
  assert.deepEqual(writes, [
    [MAIN_SIDEBAR_WIDTH_STORAGE_KEY, '{"version":1,"width":288}'],
  ]);
});

test("主边栏拖动期间只更新内存并在拖动结束后提交存储", async () => {
  const sidebar = await readFile("src/components/MainSidebar.tsx", "utf8");
  const shell = await readFile("src/components/MainAppShell.tsx", "utf8");
  const styles = await readFile("src/styles/main-app.css", "utf8");
  assert.match(sidebar, /if \(collapsed\) return;/);
  assert.match(sidebar, /handlePointerMove[\s\S]*?onWidthChange\(next\)/);
  assert.match(sidebar, /handlePointerUp[\s\S]*?onWidthChangeEnd\(state\.currentWidth\)/);
  assert.doesNotMatch(sidebar, /localStorage|saveMainSidebarWidth/);
  assert.match(shell, /loadMainSidebarWidth\(\)/);
  assert.match(
    shell,
    /commitSidebarWidth[\s\S]*?saveMainSidebarWidth\(width\)[\s\S]*?onWidthChangeEnd=\{commitSidebarWidth\}/,
  );
  assert.match(styles, /--rr-main-sidebar-width: calc\(252px \* var\(--rr-main-design-scale\)\)/);
  assert.match(styles, /\.is-sidebar-collapsed \{\s*--rr-main-sidebar-width: 0px;/);
});
