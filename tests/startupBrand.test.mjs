import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { markMainStartupReady } from "../src/startupBrand.ts";

test("静态品牌层只为主窗口启用，并在 React 入口之前存在", async () => {
  const html = await readFile("index.html", "utf8");
  const marker = html.indexOf('view === "main"');
  const brand = html.indexOf('id="rr-startup-brand"');
  const root = html.indexOf('id="root"');
  const entry = html.indexOf('src="/src/main.tsx"');

  assert.ok(marker >= 0);
  assert.ok(brand >= 0);
  assert.ok(root > brand);
  assert.ok(entry > root);
  assert.match(html, /\.rr-main-view #rr-startup-brand\s*{\s*display: grid;/);
  assert.match(html, /\.rr-main-ready #rr-startup-brand/);
  assert.match(html, /branding\/readray-startup-icon\.png/);
  assert.match(html, /background: #141412;/);
  assert.match(html, /width: clamp\(132px, 11vw, 176px\);/);
  assert.doesNotMatch(html, /vite\.svg/);
  assert.doesNotMatch(html, /setTimeout\s*\(/);
});

test("主界面就绪信号只添加稳定的 ready class", () => {
  const classes = [];
  markMainStartupReady({
    classList: {
      add(value) {
        classes.push(value);
      },
    },
  });
  assert.deepEqual(classes, ["rr-main-ready"]);
});

test("主窗口在首次绘制帧发布就绪信号，overlay 不装配该路径", async () => {
  const app = await readFile("src/App.tsx", "utf8");
  const mainStart = app.indexOf("function MainAppWindow()");
  const readyCall = app.indexOf("markMainStartupReady()", mainStart);
  const nextFunction = app.indexOf("\nfunction ", mainStart + 1);

  assert.ok(mainStart >= 0);
  assert.ok(readyCall > mainStart);
  assert.ok(nextFunction < 0 || readyCall < nextFunction);
  assert.match(
    app.slice(mainStart, readyCall + "markMainStartupReady()".length),
    /requestAnimationFrame\(\(\) =>\s*{\s*markMainStartupReady\(\)/,
  );
});
