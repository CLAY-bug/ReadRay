import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { TauriThemeRepository } from "../src/themeRepository.ts";
import { RepositoryThemeService } from "../src/themeService.ts";
import { ThemeMutationCoordinator } from "../src/themeMutationCoordinator.ts";
import {
  __setMainWindowBackgroundForTest,
  syncMainWindowBackground,
  toMainWindowBackgroundColor,
} from "../src/mainWindowBackground.ts";
import {
  applyThemeVariables,
  DEFAULT_THEME_SNAPSHOT,
  FLEXOKI_THEME,
  READRAY_DEFAULT_THEME,
  themeCssVariables,
  validateThemeSnapshot,
} from "../src/themeProtocol.ts";

function clone(value) {
  return structuredClone(value);
}

function customTheme(id = "custom-theme") {
  const theme = clone(READRAY_DEFAULT_THEME);
  theme.manifest = {
    ...theme.manifest,
    id,
    name: "Custom Theme",
    author: "Tester",
  };
  theme.builtin = false;
  return theme;
}

function snapshot(overrides = {}) {
  return {
    ...clone(DEFAULT_THEME_SNAPSHOT),
    themes: [...clone(DEFAULT_THEME_SNAPSHOT.themes), customTheme()],
    ...overrides,
  };
}

function customThemeIndex() {
  return DEFAULT_THEME_SNAPSHOT.themes.length;
}

function styleTarget(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    getPropertyValue: (name) => values.get(name) ?? "",
    setProperty: (name, value) => values.set(name, value),
    removeProperty: (name) => values.delete(name),
  };
}

test("主题画布色可无损转换为 Tauri 主窗口背景色", () => {
  assert.deepEqual(toMainWindowBackgroundColor("#f2f1ed"), [242, 241, 237]);
  assert.deepEqual(toMainWindowBackgroundColor("#fff"), [255, 255, 255]);
  assert.deepEqual(toMainWindowBackgroundColor("#1234"), [17, 34, 51, 68]);
  assert.deepEqual(toMainWindowBackgroundColor("rgb(16, 15, 15)"), [16, 15, 15]);
  assert.deepEqual(
    toMainWindowBackgroundColor("rgba(38, 37, 30, 0.5)"),
    [38, 37, 30, 128],
  );
  assert.throws(
    () => toMainWindowBackgroundColor("transparent"),
    /不是主题协议允许的规范颜色/,
  );
});

test("Tauri 主窗口背景同步使用当前主题画布色", async () => {
  const colors = [];
  globalThis.window = { __TAURI_INTERNALS__: {} };
  __setMainWindowBackgroundForTest(async (color) => {
    colors.push(color);
  });
  try {
    await syncMainWindowBackground(DEFAULT_THEME_SNAPSHOT);
  } finally {
    delete globalThis.window;
  }
  assert.deepEqual(colors, [[242, 241, 237]]);
});

test("主窗口配置使用透明合成并给原生窗口与 WebView 开放主题背景同步", async () => {
  const [configSource, capabilitySource, themeHookSource] = await Promise.all([
    readFile("src-tauri/tauri.conf.json", "utf8"),
    readFile("src-tauri/capabilities/default.json", "utf8"),
    readFile("src/useAppTheme.ts", "utf8"),
  ]);
  const config = JSON.parse(configSource);
  const capability = JSON.parse(capabilitySource);
  const mainWindow = config.app.windows.find((window) => window.label === "main");

  assert.equal(mainWindow.transparent, true);
  assert.equal(mainWindow.backgroundColor, "#f2f1ed");
  assert.equal(mainWindow.minWidth, 480);
  assert.equal(mainWindow.minHeight, 600);
  assert.ok(capability.permissions.includes("core:window:allow-set-background-color"));
  assert.ok(
    capability.permissions.includes(
      "core:webview:allow-set-webview-background-color",
    ),
  );
  assert.match(themeHookSource, /syncMainWindowBackground\(validated\)/);
});

test("主题 repository 只把原生目录选择结果交给 typed Rust commands，取消不读取文件", async () => {
  const calls = [];
  const dialogCalls = [];
  const repository = new TauriThemeRepository(
    async (command, args) => {
      calls.push({ command, args });
      if (command === "inspect_theme_package") return customTheme();
      return snapshot({ revision: args?.expectedRevision ?? 0 });
    },
    async (options) => {
      dialogCalls.push(options);
      return "D:\\Themes\\safe-theme";
    },
  );

  await repository.getSnapshot();
  const prepared = await repository.prepareImport();
  await repository.importPreparedPackage(prepared, 4);
  await repository.select("custom-theme", "light", 5);
  await repository.delete("custom-theme", 6);

  assert.deepEqual(calls, [
    { command: "get_theme_snapshot", args: undefined },
    {
      command: "inspect_theme_package",
      args: { directoryPath: "D:\\Themes\\safe-theme" },
    },
    {
      command: "import_theme_package",
      args: {
        directoryPath: "D:\\Themes\\safe-theme",
        expectedThemeId: "custom-theme",
        expectedRevision: 4,
      },
    },
    {
      command: "select_theme",
      args: { themeId: "custom-theme", mode: "light", expectedRevision: 5 },
    },
    {
      command: "delete_custom_theme",
      args: { themeId: "custom-theme", expectedRevision: 6 },
    },
  ]);
  assert.deepEqual(dialogCalls, [
    { title: "选择 ReadRay 主题包目录", directory: true, multiple: false },
  ]);

  const cancelledCalls = [];
  const cancelled = new TauriThemeRepository(
    async (command, args) => {
      cancelledCalls.push({ command, args });
      return snapshot();
    },
    async () => null,
  );
  assert.equal(await cancelled.prepareImport(), null);
  assert.deepEqual(cancelledCalls, []);
});

test("ThemeService 严格校验当前 themeId、模式和内置默认主题", async () => {
  const service = new RepositoryThemeService({
    getSnapshot: async () => snapshot(),
    prepareImport: async () => ({ directoryPath: "D:\\Themes\\safe-theme", theme: customTheme() }),
    importPreparedPackage: async () => snapshot({ revision: 1 }),
    select: async () => snapshot({ revision: 2 }),
    delete: async () => clone(DEFAULT_THEME_SNAPSHOT),
  });
  assert.equal((await service.load()).currentThemeId, "readray-default");
  const prepared = await service.prepareImport();
  assert.equal(prepared.theme.manifest.id, "custom-theme");
  assert.equal((await service.importPreparedPackage(prepared, 0)).revision, 1);
  assert.equal((await service.select("custom-theme", "light", 1)).revision, 2);
  assert.equal((await service.delete("custom-theme", 2)).themes.length, DEFAULT_THEME_SNAPSHOT.themes.length);
  assert.throws(() =>
    validateThemeSnapshot(snapshot({ currentThemeId: "missing-theme" })),
  );
  assert.throws(() =>
    validateThemeSnapshot(snapshot({ currentMode: "sepia" })),
  );
  const missingBuiltin = snapshot({ themes: [customTheme()] });
  assert.throws(() => validateThemeSnapshot(missingBuiltin));
  const flexokiTampered = snapshot();
  flexokiTampered.themes[1].light.accent = "#123456";
  assert.throws(() => validateThemeSnapshot(flexokiTampered));
  const extraBuiltin = snapshot();
  extraBuiltin.themes[customThemeIndex()].builtin = true;
  assert.throws(() => validateThemeSnapshot(extraBuiltin));
});

test("前端按 Unicode code point 计数字符串长度，与 Rust 一致", () => {
  const within = snapshot();
  within.themes[customThemeIndex()].manifest.name = "😀".repeat(80);
  assert.equal([...within.themes[customThemeIndex()].manifest.name].length, 80);
  assert.equal(
    [...validateThemeSnapshot(within).themes[customThemeIndex()].manifest.name].length,
    80,
  );

  const over = snapshot();
  over.themes[customThemeIndex()].manifest.name = "😀".repeat(81);
  assert.equal([...over.themes[customThemeIndex()].manifest.name].length, 81);
  assert.throws(() => validateThemeSnapshot(over), /主题名称无效/);

  const sourceWithin = snapshot();
  sourceWithin.themes[customThemeIndex()].manifest.sourceUrl = "https://" + "😀".repeat(2_040);
  assert.equal([...sourceWithin.themes[customThemeIndex()].manifest.sourceUrl].length, 2_048);
  assert.equal(
    [...validateThemeSnapshot(sourceWithin).themes[customThemeIndex()].manifest.sourceUrl].length,
    2_048,
  );

  const sourceOver = snapshot();
  sourceOver.themes[customThemeIndex()].manifest.sourceUrl = "https://" + "😀".repeat(2_041);
  assert.equal([...sourceOver.themes[customThemeIndex()].manifest.sourceUrl].length, 2_049);
  assert.throws(() => validateThemeSnapshot(sourceOver), /主题来源无效/);
});

test("前端只接受 Rust 输出的规范颜色格式", () => {
  const invalidAlpha = snapshot();
  invalidAlpha.themes[customThemeIndex()].light.border = "rgba(38, 37, 30, 00.5)";
  assert.throws(() => validateThemeSnapshot(invalidAlpha), /不是规范化颜色/);

  const invalidHex = snapshot();
  invalidHex.themes[customThemeIndex()].light.accentText = "#ffffff";
  assert.throws(() => validateThemeSnapshot(invalidHex), /不是规范化颜色/);

  const canonical = snapshot();
  canonical.themes[customThemeIndex()].light.border = "rgba(38, 37, 30, 0.5)";
  assert.equal(validateThemeSnapshot(canonical).themes[customThemeIndex()].light.border, "rgba(38, 37, 30, 0.5)");
});

test("ReadRay Default 应用前后保留原有主应用 CSS 变量值", async () => {
  const css = await readFile("src/styles/main-app.css", "utf8");
  const expectedLegacyVariables = {
    "--rr-main-bg": "#f2f1ed",
    "--rr-main-surface": "#e6e5e0",
    "--rr-main-surface-warm": "#ebeae5",
    "--rr-main-fg": "#26251e",
    "--rr-main-fg-secondary": "rgba(38, 37, 30, 0.9)",
    "--rr-main-muted": "rgba(38, 37, 30, 0.55)",
    "--rr-main-meta": "rgba(38, 37, 30, 0.4)",
    "--rr-main-border": "rgba(38, 37, 30, 0.1)",
    "--rr-main-border-soft": "rgba(38, 37, 30, 0.06)",
    "--rr-main-accent": "#f54e00",
    "--rr-main-danger": "#cf2d56",
  };
  const runtime = themeCssVariables(DEFAULT_THEME_SNAPSHOT);
  const target = styleTarget();
  applyThemeVariables(target, DEFAULT_THEME_SNAPSHOT);
  for (const [name, value] of Object.entries(expectedLegacyVariables)) {
    assert.match(css, new RegExp(`${name.replaceAll("-", "\\-")}:\\s*${value.replace(/[()#.]/g, "\\$&")};`));
    assert.equal(runtime[name], value);
    assert.equal(target.values.get(name), value);
  }
});

test("ReadRay Default 补充与浅色同语义层级的深色模式，并沿用 Graphite + Amber 品牌色", async () => {
  assert.deepEqual(READRAY_DEFAULT_THEME.manifest.modes, ["light", "dark"]);
  assert.ok(READRAY_DEFAULT_THEME.dark);
  const darkRuntime = themeCssVariables({
    ...clone(DEFAULT_THEME_SNAPSHOT),
    currentMode: "dark",
  });
  assert.equal(darkRuntime["--rr-main-bg"], "#0d0d0b");
  assert.equal(darkRuntime["--rr-main-sidebar"], "#171512");
  assert.equal(darkRuntime["--rr-main-surface"], "#1f1b18");
  assert.equal(darkRuntime["--rr-main-fg"], "#f6f0e8");
  assert.equal(darkRuntime["--rr-main-accent"], "#ff6a32");
  assert.equal(darkRuntime["--rr-main-accent-text"], "#0d0d0b");
  assert.equal(darkRuntime["--rr-main-shadow"], "rgba(0, 0, 0, 0.32)");
});

test("Flexoki 内置主题默认可见、支持双模式切换，且运行时变量随模式映射", async () => {
  const lightSnapshot = {
    ...clone(DEFAULT_THEME_SNAPSHOT),
    currentThemeId: "flexoki",
    currentMode: "light",
  };
  const lightRuntime = themeCssVariables(lightSnapshot);
  assert.equal(lightRuntime["--rr-main-bg"], "#fffcf0");
  assert.equal(lightRuntime["--rr-main-accent"], "#24837b");

  const darkSnapshot = {
    ...clone(DEFAULT_THEME_SNAPSHOT),
    currentThemeId: "flexoki",
    currentMode: "dark",
  };
  const darkRuntime = themeCssVariables(darkSnapshot);
  assert.equal(darkRuntime["--rr-main-bg"], "#100f0f");
  assert.equal(darkRuntime["--rr-main-accent"], "#3aa99f");
  assert.equal(darkRuntime["--rr-main-fg"], "#cecdc3");

  // 主题列表默认包含 ReadRay Default、Flexoki 与 15 个双模式 Codex 主题，且都不可删除。
  const list = validateThemeSnapshot(DEFAULT_THEME_SNAPSHOT).themes;
  assert.equal(list.length, 17);
  assert.ok(list.every((theme) => theme.builtin));
  const coordinator = new ThemeMutationCoordinator({
    service: {
      load: async () => DEFAULT_THEME_SNAPSHOT,
      prepareImport: async () => null,
      importPreparedPackage: async () => DEFAULT_THEME_SNAPSHOT,
      select: async () => DEFAULT_THEME_SNAPSHOT,
      delete: async () => DEFAULT_THEME_SNAPSHOT,
    },
    apply: () => undefined,
  });
  const outcome = await coordinator.delete(DEFAULT_THEME_SNAPSHOT, "flexoki");
  assert.equal(outcome.status, "failed");
  assert.match(outcome.message, /内置主题不能删除/);
});

test("Codex 内置主题清单、模式支持、唯一 ID、删除限制与运行时变量映射", async () => {
  const themes = validateThemeSnapshot(DEFAULT_THEME_SNAPSHOT).themes;
  const codexThemes = themes.filter((theme) => theme.manifest.id !== "readray-default" && theme.manifest.id !== "flexoki");
  assert.equal(codexThemes.length, 15);

  // 唯一 ID
  const ids = new Set(codexThemes.map((theme) => theme.manifest.id));
  assert.equal(ids.size, 15);

  // 内置 Codex 主题只保留同时支持两种模式的主题。
  assert.ok(codexThemes.every((theme) => {
    assert.deepEqual(theme.manifest.modes, ["dark", "light"]);
    assert.ok(theme.dark);
    assert.ok(theme.light);
    return true;
  }));
  const catppuccin = codexThemes.find((t) => t.manifest.id === "catppuccin");
  assert.deepEqual(catppuccin.manifest.modes, ["dark", "light"]);
  assert.ok(catppuccin.dark);
  assert.ok(catppuccin.light);

  // 每个主题 token 完整（28 个颜色字段），且颜色规范
  const colorFieldCount = Object.keys(catppuccin.dark).length;
  assert.equal(colorFieldCount, 28);

  // 内置主题删除限制
  const coordinator = new ThemeMutationCoordinator({
    service: {
      load: async () => DEFAULT_THEME_SNAPSHOT,
      prepareImport: async () => null,
      importPreparedPackage: async () => DEFAULT_THEME_SNAPSHOT,
      select: async () => DEFAULT_THEME_SNAPSHOT,
      delete: async () => DEFAULT_THEME_SNAPSHOT,
    },
    apply: () => undefined,
  });
  const outcome = await coordinator.delete(DEFAULT_THEME_SNAPSHOT, "catppuccin");
  assert.equal(outcome.status, "failed");
  assert.match(outcome.message, /内置主题不能删除/);

  // 运行时变量映射：Catppuccin dark 应用后背景/前景/强调
  const catppuccinDark = themeCssVariables({
    ...clone(DEFAULT_THEME_SNAPSHOT),
    currentThemeId: "catppuccin",
    currentMode: "dark",
  });
  assert.equal(catppuccinDark["--rr-main-bg"], "#1e1e2e");
  assert.equal(catppuccinDark["--rr-main-fg"], "#cdd6f4");
  assert.equal(catppuccinDark["--rr-main-accent"], "#cba6f7");
});

test("主题选择失败会恢复数据库权威主题并保留可重试请求", async () => {
  const authority = snapshot({ revision: 7 });
  const applied = [];
  let reloads = 0;
  const coordinator = new ThemeMutationCoordinator({
    service: {
      load: async () => {
        reloads += 1;
        return authority;
      },
      prepareImport: async () => null,
      importPreparedPackage: async () => authority,
      select: async () => { throw new Error("revision conflict"); },
      delete: async () => authority,
    },
    apply: (next) => applied.push(next.currentThemeId),
  });

  const outcome = await coordinator.select(authority, "custom-theme", "light");
  assert.equal(outcome.status, "failed");
  assert.deepEqual(outcome.retry, {
    kind: "select",
    themeId: "custom-theme",
    mode: "light",
  });
  assert.equal(reloads, 1);
  assert.deepEqual(applied, ["custom-theme", "readray-default"]);
  assert.match(outcome.message, /数据库权威主题/);
});

test("mutation 已提交但响应丢失时按精确后置条件确认成功", async () => {
  const beforeSelect = snapshot({ revision: 3 });
  const afterSelect = snapshot({
    revision: 4,
    currentThemeId: "custom-theme",
    currentMode: "light",
  });
  const selected = new ThemeMutationCoordinator({
    service: {
      load: async () => afterSelect,
      prepareImport: async () => null,
      importPreparedPackage: async () => afterSelect,
      select: async () => { throw new Error("IPC response lost"); },
      delete: async () => afterSelect,
    },
    apply: () => undefined,
  });
  const selectOutcome = await selected.select(beforeSelect, "custom-theme", "light");
  assert.equal(selectOutcome.status, "saved");
  assert.deepEqual(selectOutcome.mutation, {
    kind: "select",
    themeId: "custom-theme",
    mode: "light",
  });

  const beforeDelete = snapshot({
    revision: 8,
    currentThemeId: "custom-theme",
    currentMode: "light",
  });
  const afterDelete = clone(DEFAULT_THEME_SNAPSHOT);
  afterDelete.revision = 9;
  const deleted = new ThemeMutationCoordinator({
    service: {
      load: async () => afterDelete,
      prepareImport: async () => null,
      importPreparedPackage: async () => afterDelete,
      select: async () => afterDelete,
      delete: async () => { throw new Error("IPC response lost"); },
    },
    apply: () => undefined,
  });
  const deleteOutcome = await deleted.delete(beforeDelete, "custom-theme");
  assert.equal(deleteOutcome.status, "saved");
  assert.deepEqual(deleteOutcome.mutation, { kind: "delete", themeId: "custom-theme" });
});

test("导入响应丢失只确认精确目标，任意并发新主题判为冲突", async () => {
  const before = clone(DEFAULT_THEME_SNAPSHOT);
  before.revision = 12;
  const intended = customTheme("intended-theme");
  const importedAuthority = {
    ...clone(DEFAULT_THEME_SNAPSHOT),
    revision: 13,
    themes: [...clone(DEFAULT_THEME_SNAPSHOT.themes), intended],
  };
  const committed = new ThemeMutationCoordinator({
    service: {
      load: async () => importedAuthority,
      prepareImport: async () => ({ directoryPath: "D:\\Themes\\intended", theme: intended }),
      importPreparedPackage: async () => { throw new Error("IPC response lost"); },
      select: async () => importedAuthority,
      delete: async () => importedAuthority,
    },
    apply: () => undefined,
  });
  const committedOutcome = await committed.importPackage(before);
  assert.equal(committedOutcome.status, "saved");
  assert.deepEqual(committedOutcome.mutation, { kind: "import", themeId: "intended-theme" });

  const concurrentAuthority = {
    ...clone(DEFAULT_THEME_SNAPSHOT),
    revision: 13,
    themes: [...clone(DEFAULT_THEME_SNAPSHOT.themes), customTheme("unrelated-theme")],
  };
  const conflicted = new ThemeMutationCoordinator({
    service: {
      load: async () => concurrentAuthority,
      prepareImport: async () => ({ directoryPath: "D:\\Themes\\intended", theme: intended }),
      importPreparedPackage: async () => { throw new Error("revision conflict"); },
      select: async () => concurrentAuthority,
      delete: async () => concurrentAuthority,
    },
    apply: () => undefined,
  });
  const conflictOutcome = await conflicted.importPackage(before);
  assert.equal(conflictOutcome.status, "conflict");
  assert.equal("retry" in conflictOutcome, false);
  assert.match(conflictOutcome.message, /不会自动重试/);
});

test("CSS 变量应用中途失败会回滚，原始主题保持不变", () => {
  const initial = themeCssVariables(DEFAULT_THEME_SNAPSHOT);
  const target = styleTarget(initial);
  const originalSet = target.setProperty;
  target.setProperty = (name, value) => {
    if (name === "--rr-main-accent" && value === "#123456") {
      throw new Error("style rejected");
    }
    originalSet(name, value);
  };
  const candidate = snapshot({ currentThemeId: "custom-theme" });
  candidate.themes[customThemeIndex()].light.accent = "#123456";
  assert.throws(() => applyThemeVariables(target, candidate), /style rejected/);
  assert.deepEqual(Object.fromEntries(target.values), initial);
});

test("主应用主题敏感背景、焦点、状态色和阴影不再绕过语义 token", async () => {
  const [mainCss, settingsCss, writingCss] = await Promise.all([
    readFile("src/styles/main-app.css", "utf8"),
    readFile("src/styles/settings-page.css", "utf8"),
    readFile("src/styles/writing-page.css", "utf8"),
  ]);
  assert.doesNotMatch(mainCss, /rgba\(242, 241, 237, 0\.88\)/);
  assert.doesNotMatch(mainCss, /rgba\(245, 78, 0, 0\.32\)/);
  assert.doesNotMatch(mainCss, /#9f3a2f/i);
  assert.doesNotMatch(settingsCss, /#23745a/i);
  assert.doesNotMatch(writingCss, /var\(--rr-writing-bg\),\s*#fff/);
  // 默认石墨色阴影不得绕过主题语义 token 重新进入正式主题区域的 box-shadow 声明。
  const graphiteShadow = /box-shadow:[^;]*rgba\(38,\s*37,\s*30|box-shadow:[^;]*rgb\(\s*38\s+37\s+30/;
  assert.doesNotMatch(mainCss, graphiteShadow);
  assert.doesNotMatch(settingsCss, graphiteShadow);
  assert.doesNotMatch(writingCss, graphiteShadow);
  assert.match(mainCss, /--rr-main-shadow:/);
  assert.match(
    settingsCss,
    /box-shadow:\s*0 0 0 2px color-mix\(in oklab, var\(--rr-main-accent\), transparent 78%\)/,
  );
});

test("正式主题路径不使用前端存储、不执行原始 CSS，也不联网下载主题", async () => {
  const files = await Promise.all([
    readFile("src/themeProtocol.ts", "utf8"),
    readFile("src/themeRepository.ts", "utf8"),
    readFile("src/themeService.ts", "utf8"),
    readFile("src/useAppTheme.ts", "utf8"),
    readFile("src/components/SettingsPage.tsx", "utf8"),
    readFile("src-tauri/src/themes.rs", "utf8"),
  ]);
  const combined = files.join("\n");
  assert.doesNotMatch(combined, /localStorage|sessionStorage|insertAdjacentHTML|dangerouslySetInnerHTML|eval\s*\(/);
  assert.doesNotMatch(files[3], /theme\.css|manifest\.json|fetch\s*\(/);
  assert.doesNotMatch(files[4], /\binvoke\s*\(|readTextFile|writeTextFile|fetch\s*\(/);
  assert.match(files[5], /read_direct_package_file/);
  assert.match(files[5], /禁止 URL、脚本、远程字体、图片或可执行表达式/);
  assert.match(files[5], /manifest_json, light_colors_json, dark_colors_json, warnings_json/);
  assert.doesNotMatch(files[5], /reqwest|ureq|webview|set_inner_html/);
});

import {
  __resetPrefetchedThemeForTest,
  __setThemeInvokeForTest,
  __setThemePrefetchTimeoutForTest,
  getPrefetchedThemeSnapshot,
  prefetchThemeSnapshot,
} from "../src/themePrefetch.ts";

test("主题预取：非 Tauri 环境跳过，不调用 invoke", async () => {
  __resetPrefetchedThemeForTest();
  __setThemeInvokeForTest(() => {
    throw new Error("非 Tauri 环境不应调用 invoke");
  });
  // Node 无 window：isTauriRuntime() 返回 false，直接跳过。
  await prefetchThemeSnapshot();
  assert.equal(getPrefetchedThemeSnapshot(), null);
});

test("主题预取：Tauri 环境成功预取并校验快照", async () => {
  __resetPrefetchedThemeForTest();
  globalThis.window = globalThis.window ?? {};
  globalThis.window.__TAURI_INTERNALS__ = {};
  const valid = clone(DEFAULT_THEME_SNAPSHOT);
  __setThemeInvokeForTest(async () => valid);
  await prefetchThemeSnapshot();
  assert.deepEqual(getPrefetchedThemeSnapshot(), validateThemeSnapshot(valid));
  delete globalThis.window;
});

test("主题预取：invoke 失败时静默回退，不抛出", async () => {
  __resetPrefetchedThemeForTest();
  globalThis.window = { __TAURI_INTERNALS__: {} };
  __setThemeInvokeForTest(async () => {
    throw new Error("IPC 失败");
  });
  await prefetchThemeSnapshot();
  assert.equal(getPrefetchedThemeSnapshot(), null);
  delete globalThis.window;
});

test("主题预取：非法快照被校验拒绝并静默回退", async () => {
  __resetPrefetchedThemeForTest();
  globalThis.window = { __TAURI_INTERNALS__: {} };
  __setThemeInvokeForTest(async () => ({ currentThemeId: "bad", themes: [] }));
  await prefetchThemeSnapshot();
  assert.equal(getPrefetchedThemeSnapshot(), null);
  delete globalThis.window;
});

test("主题预取：invoke 挂起超过超时后静默回退，不阻塞挂载", async () => {
  __resetPrefetchedThemeForTest();
  __setThemePrefetchTimeoutForTest(50);
  globalThis.window = { __TAURI_INTERNALS__: {} };
  __setThemeInvokeForTest(
    () => new Promise(() => {}), // 永不 resolve：模拟 IPC 挂起
  );
  const startedAt = Date.now();
  await prefetchThemeSnapshot();
  const elapsed = Date.now() - startedAt;
  // 超时兜底生效：不阻塞挂载，快速回退，且不抛异常。
  assert.equal(getPrefetchedThemeSnapshot(), null);
  assert.ok(elapsed < 500, `预取应在超时后返回而非挂起，实际耗时 ${elapsed}ms`);
  delete globalThis.window;
  __setThemePrefetchTimeoutForTest(2000);
});
