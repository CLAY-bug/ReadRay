import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { installDesktopContextMenuGuard } from "../src/desktopContextMenu.ts";

test("桌面右键守卫只阻止 WebView 默认菜单，并保留事件传播", () => {
  let listener;
  let addCapture;
  let removedListener;
  let removeCapture;
  let prevented = false;
  const target = {
    addEventListener(type, nextListener, capture) {
      assert.equal(type, "contextmenu");
      listener = nextListener;
      addCapture = capture;
    },
    removeEventListener(type, nextListener, capture) {
      assert.equal(type, "contextmenu");
      removedListener = nextListener;
      removeCapture = capture;
    },
  };

  const cleanup = installDesktopContextMenuGuard(target);
  listener({
    preventDefault() {
      prevented = true;
    },
  });

  assert.equal(prevented, true);
  assert.equal(addCapture, true);

  cleanup();
  assert.equal(removedListener, listener);
  assert.equal(removeCapture, true);
});

test("右键守卫只在 Tauri 入口中安装，浏览器预览保持默认行为", async () => {
  const main = await readFile("src/main.tsx", "utf8");

  assert.match(
    main,
    /if \("__TAURI_INTERNALS__" in window\) \{\s*installDesktopContextMenuGuard\(\);\s*\}/,
  );
});
