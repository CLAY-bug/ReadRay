// 一次性只读脚本：从提取的主题 JS chunk 中解析核心调色板。
// 依据 app-initial 注册表中的 FSi 映射规则，从 VS Code theme colors 推导
// surface/ink/accent/diffAdded/diffRemoved/skill，并保留原始 colors 关键字段。
// 输出 JSON 到 stdout，供设计 ReadRayThemeV1 映射使用。不修改任何源文件。
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";

const DIR = process.argv[2] || "scripts/codex-theme-extract/webview/assets";

// FSi 推导规则（来自 Codex app-initial 注册表）
const SURFACE_KEYS = [
  "editor.background",
  "sideBar.background",
  "editorGroupHeader.tabsBackground",
  "panel.background",
  "activityBar.background",
];
const INK_KEYS = [
  "editor.foreground",
  "sideBarTitle.foreground",
  "sideBar.foreground",
  "foreground",
];
const ACCENT_KEYS = [
  "activityBarBadge.background",
  "textLink.foreground",
  "editorCursor.foreground",
  "focusBorder",
  "button.background",
  "activityBar.activeBorder",
];
const DIFF_ADDED_KEYS = [
  "gitDecoration.addedResourceForeground",
  "gitDecoration.untrackedResourceForeground",
  "terminal.ansiGreen",
  "terminal.ansiBrightGreen",
];
const DIFF_REMOVED_KEYS = [
  "gitDecoration.deletedResourceForeground",
  "terminal.ansiRed",
  "terminal.ansiBrightRed",
];
const SKILL_KEYS = ["charts.purple", "terminal.ansiMagenta", "terminal.ansiBrightMagenta"];
// 额外提取用于 ReadRay 语义 token 的原始 VS Code 背景/强调 key
const SIDEBAR_KEYS = ["sideBar.background", "activityBar.background", "panel.background"];
const SELECTION_KEYS = ["editor.selectionBackground"];

function firstColor(colors, keys) {
  for (const key of keys) {
    const value = colors[key];
    if (typeof value === "string") return { key, value };
  }
  return null;
}

// 从文件内容提取 colors 对象与 type/name。
// shiki 主题: Object.freeze(JSON.parse(`{...}`))，colors/type/name 是 JSON 内字段
// Codex 主题: colors=Object 变量或导出对象，name/type 是模板字符串
function extractColors(source) {
  // shiki: JSON.parse(`...`) 冻结对象
  const parseMatch = source.match(/JSON\.parse\(`([\s\S]*?)`\)/);
  if (parseMatch) {
    try {
      const obj = JSON.parse(parseMatch[1]);
      if (obj && obj.colors) return obj;
      return obj;
    } catch (e) {
      return null;
    }
  }
  // Codex 主题: 形如 t=`bg`,n={colors...},r=`fg`,i=`name`,a=[...],o=`type`
  const colorsMatch = source.match(/colors:\s*(\{[^;]*?\})\s*,/);
  if (colorsMatch) {
    try {
      const colors = JSON.parse(
        colorsMatch[1]
          .replace(/`([^`]*)`/g, '"$1"')
          .replace(/([{,]\s*)([A-Za-z0-9_.]+)\s*:/g, '$1"$2":'),
      );
      return { colors, type: extractField(source, "type") || extractField(source, "o"), name: extractField(source, "name") || extractField(source, "i") };
    } catch (e) {
      return null;
    }
  }
  // Codex 主题: colors 以变量赋值形式存在，形如 `a={"editor.background":`#0B0B0F`,...}`，
  // 之后 export{... a as colors, ...}。键带引号、值用反引号。按 export 名定位 colors 变量。
  const codexExport = source.match(/export\{(?:[^}]*?)(\w+) as colors,([^}]*?)\}/);
  if (codexExport) {
    const varName = codexExport[1];
    const marker = `${varName}={`;
    const start = source.indexOf(marker);
    if (start >= 0) {
      const objStart = start + marker.length;
      // 用平衡花括号定位对象真正结束（colors 对象自身的 { 已计入 depth=1）
      let depth = 1;
      let objEnd = -1;
      for (let i = objStart; i < source.length; i += 1) {
        if (source[i] === "{") depth += 1;
        else if (source[i] === "}") {
          depth -= 1;
          if (depth === 0) { objEnd = i; break; }
        }
      }
      if (objEnd > objStart) {
        try {
          const colors = JSON.parse(
            "{"
              + source
                .slice(objStart, objEnd)
                .replace(/`([^`]*)`/g, '"$1"')
                .replace(/,\s*([A-Za-z0-9_.]+)\s*:/g, ',"$1":')
                .replace(/^\s*([A-Za-z0-9_.]+)\s*:/, '"$1":')
              + "}",
          );
          return {
            colors,
            type: extractField(source, "type") || extractField(source, "i"),
            name: extractField(source, "name") || extractField(source, "n") || extractField(source, "i"),
          };
        } catch (e) {
          process.stderr.write(`[codex colors parse fail] ${e.message}\n`);
          return null;
        }
      }
    } else {
      process.stderr.write(`[codex no marker ${marker}]\n`);
    }
  } else {
    process.stderr.write(`[codex no export match]\n`);
  }
  return null;
}

function extractField(source, field) {
  // 查找 `field`:`value` 或 field:`value` 或 field=...
  const m = source.match(new RegExp(field + "[:=]`([^`]*)`"));
  if (m) return m[1];
  return null;
}

const results = [];
if (!existsSync(DIR)) {
  console.error("no dir", DIR);
  process.exit(1);
}
for (const file of readdirSync(DIR).sort()) {
  if (!file.endsWith(".js")) continue;
  const source = readFileSync(join(DIR, file), "utf8");
  const parsed = extractColors(source);
  if (!parsed) continue;
  const colors = parsed.colors;
  const type = parsed.type;
  const name = parsed.name;
  const surface = firstColor(colors, SURFACE_KEYS);
  const ink = firstColor(colors, INK_KEYS);
  const accent = firstColor(colors, ACCENT_KEYS);
  const diffAdded = firstColor(colors, DIFF_ADDED_KEYS);
  const diffRemoved = firstColor(colors, DIFF_REMOVED_KEYS);
  const skill = firstColor(colors, SKILL_KEYS);
  const sidebar = firstColor(colors, SIDEBAR_KEYS);
  const selection = firstColor(colors, SELECTION_KEYS);
  results.push({
    file,
    name,
    type,
    palette: {
      surface: surface ? { from: surface.key, value: surface.value } : null,
      ink: ink ? { from: ink.key, value: ink.value } : null,
      accent: accent ? { from: accent.key, value: accent.value } : null,
      diffAdded: diffAdded ? { from: diffAdded.key, value: diffAdded.value } : null,
      diffRemoved: diffRemoved ? { from: diffRemoved.key, value: diffRemoved.value } : null,
      skill: skill ? { from: skill.key, value: skill.value } : null,
      sidebar: sidebar ? { from: sidebar.key, value: sidebar.value } : null,
      selection: selection ? { from: selection.key, value: selection.value } : null,
    },
    colorCount: Object.keys(colors).length,
  });
}
console.log(JSON.stringify(results, null, 2));
