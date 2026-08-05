// 一次性生成脚本：把 core-palette.json 展开为完整 28-token ReadRayThemeV1，
// 生成 Rust 与 TypeScript 两侧的完整内置主题数据（不运行时派生，两端字节级一致）。
// 数据来自同一份 core-palette.json + 单一权威派生实现。
import { readFileSync, writeFileSync } from "node:fs";

const core = JSON.parse(
  readFileSync("scripts/codex-theme-extract/core-palette.json", "utf8"),
);

// 主题归属与许可证（核对自 Codex app.asar 内置主题来源；OpenAI 自有主题再分发许可未确认，
// 仅供 ReadRay 本地内置使用，标注来源）。MIT 主题为社区知名开源 VS Code 主题。
const THEME_META = {
  ayu: { license: "MIT", sourceUrl: "https://github.com/ayu-theme/ayu-colors", author: "tebyi / ayu-theme" },
  catppuccin: { license: "MIT", sourceUrl: "https://github.com/catppuccin/catppuccin", author: "Catppuccin" },
  absolutely: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  codex: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  dracula: { license: "MIT", sourceUrl: "https://github.com/dracula/dracula-theme", author: "Dracula" },
  everforest: { license: "MIT", sourceUrl: "https://github.com/sainnhe/everforest", author: "Sainnhe Park" },
  github: { license: "MIT", sourceUrl: "https://github.com/primer/github-vscode-theme", author: "GitHub / Primer" },
  gruvbox: { license: "MIT", sourceUrl: "https://github.com/morhetz/gruvbox", author: "morhetz / Gruvbox" },
  linear: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  lobster: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  material: { license: "MIT", sourceUrl: "https://github.com/equinusocio/material-theme", author: "Mattia Astorino" },
  matrix: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  monokai: { license: "MIT", sourceUrl: "https://github.com/microsoft/vscode", author: "Microsoft (VS Code 内置)" },
  "night-owl": { license: "MIT", sourceUrl: "https://github.com/sdras/night-owl-vscode-theme", author: "Sarah Drasner" },
  nord: { license: "MIT", sourceUrl: "https://github.com/arcticicestudio/nord-vscode", author: "arcticicestudio" },
  notion: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  oscurange: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  one: { license: "MIT", sourceUrl: "https://github.com/akamud/vscode-theme-onedark", author: "akamud / One" },
  proof: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  raycast: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  "rose-pine": { license: "MIT", sourceUrl: "https://github.com/rose-pine/rose-pine-theme", author: "Rose Pine" },
  sentry: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  solarized: { license: "MIT", sourceUrl: "https://github.com/microsoft/vscode", author: "Ethan Schoonover" },
  "tokyo-night": { license: "MIT", sourceUrl: "https://github.com/enkia/tokyo-night-vscode-theme", author: "enkia" },
  temple: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  vercel: { license: "OpenAI Codex 内置", sourceUrl: "https://openai.com/codex", author: "OpenAI" },
  "vscode-plus": { license: "MIT", sourceUrl: "https://github.com/microsoft/vscode", author: "Microsoft (VS Code 内置)" },
  xcode: { license: "MIT", sourceUrl: "https://github.com/matt-deboer/xcode", author: "Matt DeBoer" },
};

const themes = Object.values(core).map((theme) => ({
  ...theme,
  ...(THEME_META[theme.id] || { license: "详见 README 主题归属", sourceUrl: "https://openai.com/codex", author: "Codex" }),
}));

// ---------- 权威派生实现（与 Rust/TS 不再重复实现）----------
function clampByte(v) {
  return Math.max(0, Math.min(255, Math.round(v)));
}

function parseChannels(value) {
  const v = value.trim();
  if (v.startsWith("rgba(")) {
    const m = v.match(/rgba\((\d+), (\d+), (\d+), ([\d.]+)\)/);
    return [Number(m[1]), Number(m[2]), Number(m[3]), Number(m[4])];
  }
  if (v.startsWith("rgb(")) {
    const m = v.match(/rgb\((\d+), (\d+), (\d+)\)/);
    return [Number(m[1]), Number(m[2]), Number(m[3]), 1];
  }
  let hex = v.replace("#", "");
  if (hex.length === 3) hex = [...hex].map((c) => c + c).join("");
  if (hex.length === 4) hex = [...hex].map((c) => c + c).join("");
  const r = parseInt(hex.slice(0, 2), 16);
  const g = parseInt(hex.slice(2, 4), 16);
  const b = parseInt(hex.slice(4, 6), 16);
  const a = hex.length === 8 ? parseInt(hex.slice(6, 8), 16) / 255 : 1;
  return [r, g, b, a];
}

function formatHex(channels) {
  const bytes = [clampByte(channels[0]), clampByte(channels[1]), clampByte(channels[2])];
  const canShorten = bytes.every((byte) => (byte >> 4) === (byte & 0x0f));
  if (canShorten) {
    return `#${bytes.map((byte) => (byte >> 4).toString(16)).join("")}`;
  }
  return `#${bytes.map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

function compositeOnBg(fg, surface) {
  return [
    fg[0] * fg[3] + surface[0] * (1 - fg[3]),
    fg[1] * fg[3] + surface[1] * (1 - fg[3]),
    fg[2] * fg[3] + surface[2] * (1 - fg[3]),
    1,
  ];
}

function derive(entry) {
  const surface = parseChannels(entry.surface);
  const sidebar = parseChannels(entry.sidebar);
  const ink = compositeOnBg(parseChannels(entry.ink), surface);
  const accent = compositeOnBg(parseChannels(entry.accent), surface);
  const diffAdded = compositeOnBg(parseChannels(entry.diffAdded), surface);
  const diffRemoved = compositeOnBg(parseChannels(entry.diffRemoved), surface);
  const skill = compositeOnBg(parseChannels(entry.skill), surface);
  const selection = entry.selection ? compositeOnBg(parseChannels(entry.selection), surface) : null;
  const inkHex = formatHex(ink);
  const mixWithInk = (percent) => [
    (1 - percent / 100) * surface[0] + (percent / 100) * ink[0],
    (1 - percent / 100) * surface[1] + (percent / 100) * ink[1],
    (1 - percent / 100) * surface[2] + (percent / 100) * ink[2],
    1,
  ];
  const mixPlain = (fg, percent) => [
    (1 - percent / 100) * surface[0] + (percent / 100) * fg[0],
    (1 - percent / 100) * surface[1] + (percent / 100) * fg[1],
    (1 - percent / 100) * surface[2] + (percent / 100) * fg[2],
    1,
  ];
  const luminance = ([r, g, b]) => {
    const linear = (c) => {
      const s = c / 255;
      return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
  };
  const accentText = luminance([accent[0], accent[1], accent[2]]) > 0.4 ? "#1a1a18" : "#fff";
  return {
    canvas: entry.surface,
    sidebar: entry.sidebar,
    surface: entry.surface,
    surfaceElevated: formatHex(mixPlain([255, 255, 255, 1], 4)),
    surfaceSubtle: formatHex(mixWithInk(3)),
    surfaceContrast: formatHex(mixWithInk(8)),
    textPrimary: inkHex,
    textSecondary: formatHex(mixWithInk(90)),
    textMuted: formatHex(mixWithInk(55)),
    textSubtle: formatHex(mixWithInk(40)),
    border: formatHex(mixWithInk(10)),
    borderSoft: formatHex(mixWithInk(6)),
    accent: formatHex(accent),
    accentHover: formatHex(accent),
    accentText,
    success: formatHex(diffAdded),
    successSoft: formatHex(mixPlain(diffAdded, 11)),
    warning: formatHex(skill),
    warningSoft: formatHex(mixPlain(skill, 11)),
    warningStrong: formatHex(skill),
    danger: formatHex(diffRemoved),
    dangerSoft: formatHex(mixPlain(diffRemoved, 9)),
    dangerStrong: formatHex(diffRemoved),
    selection: formatHex(selection ?? mixPlain(accent, 12)),
    diffAdded: formatHex(diffAdded),
    diffRemoved: formatHex(diffRemoved),
    scrim: "rgba(0, 0, 0, 0.32)",
    shadow: formatHex(mixWithInk(10)),
  };
}

// 每个主题完整展开
const expanded = themes.map((theme) => ({
  id: theme.id,
  name: theme.name,
  author: theme.author,
  license: theme.license,
  sourceUrl: theme.sourceUrl,
  dark: theme.dark ? derive(theme.dark) : null,
  light: theme.light ? derive(theme.light) : null,
}));

// ---------- Rust 生成 ----------
const COLOR_ORDER = [
  "canvas", "sidebar", "surface", "surfaceElevated", "surfaceSubtle", "surfaceContrast",
  "textPrimary", "textSecondary", "textMuted", "textSubtle", "border", "borderSoft",
  "accent", "accentHover", "accentText", "success", "successSoft", "warning",
  "warningSoft", "warningStrong", "danger", "dangerSoft", "dangerStrong", "selection",
  "diffAdded", "diffRemoved", "scrim", "shadow",
];

function rustColors(colors) {
  const lines = ["        Some(CodexThemeFullColors {"];
  for (const key of COLOR_ORDER) {
    const field = key.replace(/([A-Z])/g, (m) => `_${m.toLowerCase()}`);
    lines.push(`            ${field}: "${colors[key]}",`);
  }
  lines.push("        }),");
  return lines.join("\n");
}

function genRust() {
  const out = [];
  out.push("// 由 scripts/gen-themes.mjs 从 scripts/codex-theme-extract/core-palette.json 生成；不要手工编辑。");
  out.push("use super::*;");
  out.push("#[rustfmt::skip]");
  out.push("/// 随包 Codex 内置主题：完整展开配色（&'static str，满足 const 静态表）。");
  out.push("/// 运行时由 codex_builtin_themes 转换为 ReadRayThemeV1。");
  out.push("pub(crate) const CODEX_BUILTIN_FULL_THEMES: &[(CodexThemeManifest, Option<CodexThemeFullColors>, Option<CodexThemeFullColors>)] = &[");
  for (const t of expanded) {
    const modes = [t.dark && "ThemeMode::Dark", t.light && "ThemeMode::Light"].filter(Boolean).join(", ");
    out.push("    (");
    out.push("        CodexThemeManifest {");
    out.push(`            id: "${t.id}",`);
    out.push(`            name: "${t.name}",`);
    out.push(`            version: "1.0.0",`);
    out.push(`            author: "${t.author}",`);
    out.push(`            modes: &[${modes}],`);
    out.push(`            license: Some("${t.license}"),`);
    out.push(`            source_url: Some("${t.sourceUrl}"),`);
    out.push("        },");
    out.push(t.dark ? rustColors(t.dark) : "        None,");
    out.push(t.light ? rustColors(t.light) : "        None,");
    out.push("    ),");
  }
  out.push("];");
  out.push("");
  return out.join("\n");
}

// ---------- TS 生成 ----------
function tsColors(colors) {
  const lines = ["    {"];
  for (const key of COLOR_ORDER) {
    lines.push(`      ${key}: ${JSON.stringify(colors[key])},`);
  }
  lines.push("    },");
  return lines.join("\n");
}

function genTs() {
  const out = [];
  out.push("// 由 scripts/gen-themes.mjs 从 scripts/codex-theme-extract/core-palette.json 生成；不要手工编辑。");
  out.push("import type { ReadRayThemeColors, ThemeMode } from \"./themeProtocol.ts\";");
  out.push("");
  out.push("export type CodexThemeManifest = {");
  out.push("  formatVersion: 1;");
  out.push("  id: string;");
  out.push("  name: string;");
  out.push("  version: string;");
  out.push("  author: string;");
  out.push("  modes: ThemeMode[];");
  out.push("  license: string;");
  out.push("  sourceUrl: string;");
  out.push("};");
  out.push("");
  out.push("export type CodexThemeFull = {");
  out.push("  manifest: CodexThemeManifest;");
  out.push("  dark: ReadRayThemeColors | null;");
  out.push("  light: ReadRayThemeColors | null;");
  out.push("};");
  out.push("");
  out.push("export const CODEX_BUILTIN_FULL_THEMES: CodexThemeFull[] = [");
  for (const t of expanded) {
    out.push("  {");
    out.push("    manifest: {");
    out.push("      formatVersion: 1,");
    out.push(`      id: ${JSON.stringify(t.id)},`);
    out.push(`      name: ${JSON.stringify(t.name)},`);
    out.push(`      version: "1.0.0",`);
    out.push(`      author: ${JSON.stringify(t.author)},`);
    out.push(`      modes: [${[t.dark && '"dark"', t.light && '"light"'].filter(Boolean).join(", ")}],`);
    out.push(`      license: ${JSON.stringify(t.license)},`);
    out.push(`      sourceUrl: ${JSON.stringify(t.sourceUrl)},`);
    out.push("    },");
    out.push(`    dark: ${t.dark ? tsColors(t.dark) : "null,"}`);
    out.push(`    light: ${t.light ? tsColors(t.light) : "null,"}`);
    out.push("  },");
  }
  out.push("];");
  out.push("");
  return out.join("\n");
}

const rustCode = genRust();
const tsCode = genTs();
writeFileSync("scripts/codex-theme-extract/generated-full-themes.rs", rustCode);
writeFileSync("scripts/codex-theme-extract/generated-full-themes.ts", tsCode);
console.log(`generated ${expanded.length} full themes -> Rust + TS`);
