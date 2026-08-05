// 一次性生成脚本：把 Codex 提取的核心调色板派生为 ReadRayThemeV1 27-token，
// 输出用于 Rust/TS 内置主题注册表的 JSON。同时做对比度预检。
import { readFileSync, writeFileSync } from "node:fs";

const palette = JSON.parse(
  readFileSync("scripts/codex-theme-extract/palette.json", "utf8"),
);

// 主题注册表：chunk 名 -> (dark, light)。None 表示无该模式。
const REGISTRY = {
  ayu: ["ayu-dark", null],
  catppuccin: ["catppuccin-mocha", "catppuccin-latte"],
  absolutely: ["absolutely-dark", "absolutely-light"],
  codex: ["codex-dark", "codex-light"],
  dracula: ["dracula", null],
  everforest: ["everforest-dark", "everforest-light"],
  github: ["github-dark-default", "github-light-default"],
  gruvbox: ["gruvbox-dark-medium", "gruvbox-light-medium"],
  linear: ["linear-dark", "linear-light"],
  lobster: ["lobster-dark", null],
  material: ["material-theme-darker", null],
  matrix: ["matrix-dark", null],
  monokai: ["monokai", null],
  "night-owl": ["night-owl", null],
  nord: ["nord", null],
  notion: ["notion-dark", "notion-light"],
  oscurange: ["oscurange", null],
  one: ["one-dark-pro", "one-light"],
  proof: [null, "proof-light"],
  raycast: ["raycast-dark", "raycast-light"],
  "rose-pine": ["rose-pine-moon", "rose-pine-dawn"],
  sentry: ["sentry-dark", null],
  solarized: ["solarized-dark", "solarized-light"],
  "tokyo-night": ["tokyo-night", null],
  temple: ["temple-dark", null],
  vercel: ["vercel-dark", "vercel-light"],
  "vscode-plus": ["dark-plus", "light-plus"],
  xcode: ["xcode-dark", "xcode-light"],
};

const LABELS = {
  ayu: "Ayu",
  catppuccin: "Catppuccin",
  absolutely: "Absolutely",
  codex: "Codex",
  dracula: "Dracula",
  everforest: "Everforest",
  github: "GitHub",
  gruvbox: "Gruvbox",
  linear: "Linear",
  lobster: "Lobster",
  material: "Material",
  matrix: "Matrix",
  monokai: "Monokai",
  "night-owl": "Night Owl",
  nord: "Nord",
  notion: "Notion",
  oscurange: "Oscurange",
  one: "One",
  proof: "Proof",
  raycast: "Raycast",
  "rose-pine": "Rose Pine",
  sentry: "Sentry",
  solarized: "Solarized",
  "tokyo-night": "Tokyo Night",
  temple: "Temple",
  vercel: "Vercel",
  "vscode-plus": "VS Code Plus",
  xcode: "Xcode",
};

// 个别主题需修正来源值（因对比度不足或提取值不完整）。
// key: 主题 id + "_" + 模式 -> { field: 覆盖值 }；从原始主题语义档位取更合适的颜色。
const INK_OVERRIDES = {
  // Solarized light 的 base00 #657b83 与 canvas 对比度仅 4.13，
  // 取原主题更深一档 base01 #586e75（Solarized 官方 foreground 档位），对比度 4.99。
  // sideBar 背景 #eee8d5（base2）比 canvas 更暗，4.99 的 ink 在其上仍只有 4.39，
  // 因此把 sidebar 提升到与 canvas 相同的 base3，保证对比度 ≥ 4.5。
  "solarized_light": { textPrimary: "#586e75", sidebar: "#fdf6e3" },
};

// ---------- 颜色工具 ----------
function hexToRgba(hex) {
  let v = hex.trim();
  if (v.startsWith("rgba(")) {
    const m = v.match(/rgba\((\d+), (\d+), (\d+), ([\d.]+)\)/);
    return [Number(m[1]), Number(m[2]), Number(m[3]), Number(m[4])];
  }
  if (v.startsWith("rgb(")) {
    const m = v.match(/rgb\((\d+), (\d+), (\d+)\)/);
    return [Number(m[1]), Number(m[2]), Number(m[3]), 1];
  }
  let h = v.replace("#", "");
  if (h.length === 3) h = h.split("").map((c) => c + c).join("");
  if (h.length === 4) h = h.split("").map((c) => c + c).join("");
  const r = parseInt(h.slice(0, 2), 16);
  const g = parseInt(h.slice(2, 4), 16);
  const b = parseInt(h.slice(4, 6), 16);
  const a = h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1;
  return [r, g, b, a];
}

function clampByte(v) {
  return Math.max(0, Math.min(255, Math.round(v)));
}

// 规范 hex：小写，可缩短，alpha 尾随
function toHex(r, g, b, a = 1) {
  const comps = [clampByte(r), clampByte(g), clampByte(b)];
  const alphaByte = Math.round(a * 255);
  if (alphaByte !== 255) comps.push(alphaByte);
  const canShorten = comps.every((c) => (c >> 4) === (c & 0x0f));
  const parts = canShorten
    ? comps.map((c) => (c >> 4).toString(16))
    : comps.map((c) => c.toString(16).padStart(2, "0"));
  return "#" + parts.join("");
}

// 规范 rgba 字符串
function toRgba([r, g, b, a]) {
  if (a === 0) return `rgba(${clampByte(r)}, ${clampByte(g)}, ${clampByte(b)}, 0)`;
  if (a === 1) return `rgb(${clampByte(r)}, ${clampByte(g)}, ${clampByte(b)})`;
  let alpha = a.toFixed(4).replace(/0+$/, "").replace(/\.$/, "");
  if (!alpha.startsWith("0.")) alpha = "0." + alpha;
  return `rgba(${clampByte(r)}, ${clampByte(g)}, ${clampByte(b)}, ${alpha})`;
}

// 混合 color-mix(in oklab, base, overlay%) 近似：简化为线性插值 on base
function mix(baseHex, overlayRgba, percent) {
  const b = hexToRgba(baseHex);
  const o = overlayRgba;
  const m = (x, y) => (1 - percent / 100) * x + (percent / 100) * y;
  const r = m(b[0], o[0]);
  const g = m(b[1], o[1]);
  const bl = m(b[2], o[2]);
  const a = m(b[3], o[3]);
  return [r, g, bl, a];
}

// 把带 alpha 的前景合成到不透明背景上，得到不透明的有效颜色
function compositeOnBg(fgRgba, bgHex) {
  const f = fgRgba;
  const b = hexToRgba(bgHex);
  const r = f[0] * f[3] + b[0] * (1 - f[3]);
  const g = f[1] * f[3] + b[1] * (1 - f[3]);
  const bl = f[2] * f[3] + b[2] * (1 - f[3]);
  return [r, g, bl, 1];
}

function mixWithInk(surfaceHex, inkHex, percent) {
  return mix(surfaceHex, hexToRgba(inkHex), percent);
}

// 亮度
function luminance([r, g, b]) {
  const lin = (c) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function contrast(fgHex, bgHex) {
  const f = hexToRgba(fgHex);
  const b = hexToRgba(bgHex);
  const L1 = luminance([f[0], f[1], f[2]]);
  const L2 = luminance([b[0], b[1], b[2]]);
  const [hi, lo] = L1 >= L2 ? [L1, L2] : [L2, L1];
  return (hi + 0.05) / (lo + 0.05);
}

// accent 上的前景（黑或白）
function accentTextColor(accentHex) {
  const [r, g, b] = hexToRgba(accentHex);
  return luminance([r, g, b]) > 0.4 ? "#1a1a18" : "#fff";
}

// 规范化任意来源颜色：hex（可能 3/4/6/8 位，大小写）→ 规范 hex；rgba hex → rgba 字符串
function normalizeColor(value) {
  if (!value) return null;
  let v = value.trim();
  if (v.startsWith("#")) {
    const [r, g, b, a] = hexToRgba(v);
    if (a === 1) return toHex(r, g, b);
    return toRgba([r, g, b, a]);
  }
  // 非 hex（理论上不会出现，兜底）
  return v;
}

function buildTheme(id, chunkName, mode) {
  const core = palette[chunkName];
  if (!core) throw new Error(`missing chunk ${chunkName}`);
  const surface = normalizeColor(core.surface);
  const ink = normalizeColor(core.ink);
  const accent = normalizeColor(core.accent);
  const sidebar = normalizeColor(core.sidebar) || surface;
  const selection = normalizeColor(core.selection) || null;
  const diffAdded = normalizeColor(core.diffAdded) || accent;
  const diffRemoved = normalizeColor(core.diffRemoved) || accent;
  const skill = normalizeColor(core.skill) || accent;
  // 对比度修正覆盖
  const override = INK_OVERRIDES[`${id}_${mode}`];
  const effectiveSidebar = override?.sidebar || sidebar;
  if (override?.textPrimary) {
    // 覆盖 textPrimary 后，派生色基于新的 ink
    return buildThemeFromCore(id, surface, override.textPrimary, accent, effectiveSidebar, selection, diffAdded, diffRemoved, skill);
  }
  return buildThemeFromCore(id, surface, ink, accent, effectiveSidebar, selection, diffAdded, diffRemoved, skill);
}

function buildThemeFromCore(id, surface, ink, accent, sidebar, selection, diffAdded, diffRemoved, skill) {

  // surface 与 sidebar 必须不透明
  const surfaceRgba = hexToRgba(surface);
  const sidebarRgba = hexToRgba(sidebar);
  if (surfaceRgba[3] !== 1) throw new Error(`${id} surface 必须不透明`);
  if (sidebarRgba[3] !== 1) throw new Error(`${id} sidebar 必须不透明`);

  // 把带 alpha 的前景合成到表面，得到不透明有效色，再用于派生
  const solidInk = compositeOnBg(hexToRgba(ink), surface);
  const solidAccent = compositeOnBg(hexToRgba(accent), surface);
  const solidSidebar = sidebar;
  const solidDiffAdded = compositeOnBg(hexToRgba(diffAdded), surface);
  const solidDiffRemoved = compositeOnBg(hexToRgba(diffRemoved), surface);
  const solidSkill = compositeOnBg(hexToRgba(skill), surface);
  const solidSelection = selection ? compositeOnBg(hexToRgba(selection), surface) : null;

  // 派生 28 token（全部基于不透明有效色）
  const textSecondary = toRgba(mixWithInk(surface, toHex(...solidInk), 90));
  const textMuted = toRgba(mixWithInk(surface, toHex(...solidInk), 55));
  const textSubtle = toRgba(mixWithInk(surface, toHex(...solidInk), 40));
  const border = toRgba(mixWithInk(surface, toHex(...solidInk), 10));
  const borderSoft = toRgba(mixWithInk(surface, toHex(...solidInk), 6));
  const surfaceSubtle = toRgba(mixWithInk(surface, toHex(...solidInk), 3));
  const surfaceElevated = toRgba(mixWithInk(surface, "#ffffff", 4));
  const surfaceContrast = toRgba(mixWithInk(surface, toHex(...solidInk), 8));
  const accentText = accentTextColor(toHex(...solidAccent));
  const successSoft = toRgba(mix(surface, [solidDiffAdded[0], solidDiffAdded[1], solidDiffAdded[2], 1], 11));
  const warningSoft = toRgba(mix(surface, [solidSkill[0], solidSkill[1], solidSkill[2], 1], 11));
  const dangerSoft = toRgba(mix(surface, [solidDiffRemoved[0], solidDiffRemoved[1], solidDiffRemoved[2], 1], 9));
  const selectionColor = solidSelection ? toRgba(solidSelection) : toRgba(mix(surface, [solidAccent[0], solidAccent[1], solidAccent[2], 1], 12));
  const scrim = toRgba([0, 0, 0, 0.32]);
  const shadow = toRgba(mixWithInk(surface, toHex(...solidInk), 10));

  return {
    canvas: surface,
    sidebar: solidSidebar,
    surface,
    surfaceElevated,
    surfaceSubtle,
    surfaceContrast,
    textPrimary: toHex(...solidInk),
    textSecondary,
    textMuted,
    textSubtle,
    border,
    borderSoft,
    accent: toHex(...solidAccent),
    accentHover: toHex(...solidAccent),
    accentText,
    success: toHex(...solidDiffAdded),
    successSoft,
    warning: toHex(...solidSkill),
    warningSoft,
    warningStrong: toHex(...solidSkill),
    danger: toHex(...solidDiffRemoved),
    dangerSoft,
    dangerStrong: toHex(...solidDiffRemoved),
    selection: selectionColor,
    diffAdded: toHex(...solidDiffAdded),
    diffRemoved: toHex(...solidDiffRemoved),
    scrim,
    shadow,
  };
}

// 主输出：所有主题的展开配色 + compact core palette（供 Rust/TS 数据驱动注册表使用）
const out = {};
const coreOut = {};
const contrastIssues = [];
for (const [id, [dark, light]] of Object.entries(REGISTRY)) {
  const entry = { id, name: LABELS[id] };
  const coreEntry = { id, name: LABELS[id] };
  if (dark) entry.dark = buildTheme(id, dark, "dark");
  if (light) entry.light = buildTheme(id, light, "light");
  out[id] = entry;
  // compact core：从派生结果提取核心值，供两端共享派生
  const coreTheme = (chunkName, mode) => {
    const c = palette[chunkName];
    const override = INK_OVERRIDES[`${id}_${mode}`];
    return {
      surface: normalizeColor(c.surface),
      ink: override?.textPrimary || normalizeColor(c.ink),
      accent: normalizeColor(c.accent),
      sidebar: override?.sidebar || normalizeColor(c.sidebar) || normalizeColor(c.surface),
      selection: normalizeColor(c.selection),
      diffAdded: normalizeColor(c.diffAdded) || normalizeColor(c.accent),
      diffRemoved: normalizeColor(c.diffRemoved) || normalizeColor(c.accent),
      skill: normalizeColor(c.skill) || normalizeColor(c.accent),
    };
  };
  if (dark) coreEntry.dark = coreTheme(dark, "dark");
  if (light) coreEntry.light = coreTheme(light, "light");
  coreOut[id] = coreEntry;
  // 对比度预检：textPrimary vs canvas/sidebar/surface
  for (const mode of ["dark", "light"]) {
    const colors = entry[mode];
    if (!colors) continue;
    for (const bg of ["canvas", "sidebar", "surface"]) {
      const ratio = contrast(colors.textPrimary, colors[bg]);
      if (ratio < 4.5) contrastIssues.push(`${id} ${mode} textPrimary vs ${bg}: ${ratio.toFixed(2)}`);
    }
  }
}

console.log(JSON.stringify(out, null, 2));
if (contrastIssues.length) {
  console.error("=== 对比度预检问题 ===");
  console.error(contrastIssues.join("\n"));
}
// 输出 compact core palette 到单独文件供 Rust/TS 生成器读取
writeFileSync(
  "scripts/codex-theme-extract/core-palette.json",
  JSON.stringify(coreOut, null, 1),
);
console.error("wrote core-palette.json with", Object.keys(coreOut).length, "themes");
