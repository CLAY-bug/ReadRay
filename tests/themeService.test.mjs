import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { TauriThemeRepository } from "../src/themeRepository.ts";
import { RepositoryThemeService } from "../src/themeService.ts";
import { ThemeMutationCoordinator } from "../src/themeMutationCoordinator.ts";
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
    validateThemeSnapshot(snapshot({ currentMode: "dark" })),
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

  // 主题列表默认包含全部随包内置主题（ReadRay Default、Flexoki 与 28 个 Codex 主题），且都不可删除。
  const list = validateThemeSnapshot(DEFAULT_THEME_SNAPSHOT).themes;
  assert.equal(list.length, 30);
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
  assert.equal(codexThemes.length, 28);

  // 唯一 ID
  const ids = new Set(codexThemes.map((theme) => theme.manifest.id));
  assert.equal(ids.size, 28);

  // 模式支持：单模式主题不虚构另一模式
  const ayu = codexThemes.find((t) => t.manifest.id === "ayu");
  assert.deepEqual(ayu.manifest.modes, ["dark"]);
  assert.equal(ayu.light, null);
  assert.ok(ayu.dark);

  const proof = codexThemes.find((t) => t.manifest.id === "proof");
  assert.deepEqual(proof.manifest.modes, ["light"]);
  assert.equal(proof.dark, null);
  assert.ok(proof.light);

  const catppuccin = codexThemes.find((t) => t.manifest.id === "catppuccin");
  assert.deepEqual(catppuccin.manifest.modes, ["dark", "light"]);
  assert.ok(catppuccin.dark);
  assert.ok(catppuccin.light);

  // 每个主题 token 完整（28 个颜色字段），且颜色规范
  const colorFieldCount = Object.keys(ayu.dark).length;
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
  const outcome = await coordinator.delete(DEFAULT_THEME_SNAPSHOT, "ayu");
  assert.equal(outcome.status, "failed");
  assert.match(outcome.message, /内置主题不能删除/);

  // 运行时变量映射：ayu dark 应用后背景/前景/强调
  const ayuDark = themeCssVariables({
    ...clone(DEFAULT_THEME_SNAPSHOT),
    currentThemeId: "ayu",
    currentMode: "dark",
  });
  assert.equal(ayuDark["--rr-main-bg"], "#10141c");
  assert.equal(ayuDark["--rr-main-fg"], "#bfbdb6");
  assert.equal(ayuDark["--rr-main-accent"], "#e6b450");
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
  assert.match(settingsCss, /box-shadow:\s*0 4px 12px var\(--rr-main-shadow\)/);
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
