export const READRAY_THEME_FORMAT_VERSION = 1 as const;
export const READRAY_DEFAULT_THEME_ID = "readray-default";
export const FLEXOKI_THEME_ID = "flexoki";

import {
  CODEX_BUILTIN_FULL_THEMES,
  type CodexThemeFull,
} from "./codexThemeData.ts";

export const READRAY_BUILTIN_THEME_IDS = [
  READRAY_DEFAULT_THEME_ID,
  FLEXOKI_THEME_ID,
  ...CODEX_BUILTIN_FULL_THEMES.map((theme) => theme.manifest.id),
] as const;

export type ThemeMode = "light" | "dark";

export type ReadRayThemeManifestV1 = {
  formatVersion: typeof READRAY_THEME_FORMAT_VERSION;
  id: string;
  name: string;
  version: string;
  author: string;
  modes: ThemeMode[];
  license: string | null;
  sourceUrl: string | null;
};

export type ReadRayThemeColors = {
  canvas: string;
  sidebar: string;
  surface: string;
  surfaceElevated: string;
  surfaceSubtle: string;
  surfaceContrast: string;
  textPrimary: string;
  textSecondary: string;
  textMuted: string;
  textSubtle: string;
  border: string;
  borderSoft: string;
  accent: string;
  accentHover: string;
  accentText: string;
  success: string;
  successSoft: string;
  warning: string;
  warningSoft: string;
  warningStrong: string;
  danger: string;
  dangerSoft: string;
  dangerStrong: string;
  selection: string;
  diffAdded: string;
  diffRemoved: string;
  scrim: string;
  shadow: string;
};

export type ReadRayThemeV1 = {
  manifest: ReadRayThemeManifestV1;
  light: ReadRayThemeColors | null;
  dark: ReadRayThemeColors | null;
  builtin: boolean;
  warnings: string[];
};

export type ThemeSnapshot = {
  revision: number;
  currentThemeId: string;
  currentMode: ThemeMode;
  themes: ReadRayThemeV1[];
};

/**
 * 外部主题只能在独立适配器中转换为 ReadRayThemeV1；适配器不得把外部 CSS
 * 直接交给浏览器。Codex、Obsidian 等来源各自实现自己的 TSource 边界。
 */
export interface ReadRayThemeAdapter<TSource> {
  readonly sourceFormat: string;
  adapt(source: TSource): ReadRayThemeV1;
}

export const READRAY_DEFAULT_THEME: ReadRayThemeV1 = {
  manifest: {
    formatVersion: READRAY_THEME_FORMAT_VERSION,
    id: READRAY_DEFAULT_THEME_ID,
    name: "ReadRay Default",
    version: "1.1.0",
    author: "ReadRay",
    modes: ["light", "dark"],
    license: null,
    sourceUrl: null,
  },
  light: {
    canvas: "#f2f1ed",
    sidebar: "#ebeae5",
    surface: "#e6e5e0",
    surfaceElevated: "#ebeae5",
    surfaceSubtle: "#f0efeb",
    surfaceContrast: "#fff",
    textPrimary: "#26251e",
    textSecondary: "rgba(38, 37, 30, 0.9)",
    textMuted: "rgba(38, 37, 30, 0.55)",
    textSubtle: "rgba(38, 37, 30, 0.4)",
    border: "rgba(38, 37, 30, 0.1)",
    borderSoft: "rgba(38, 37, 30, 0.06)",
    accent: "#f54e00",
    accentHover: "#e84800",
    accentText: "#fff",
    success: "#277250",
    successSoft: "rgba(39, 114, 80, 0.11)",
    warning: "#9a6400",
    warningSoft: "rgba(154, 100, 0, 0.11)",
    warningStrong: "#eab308",
    danger: "#cf2d56",
    dangerSoft: "rgba(207, 45, 86, 0.09)",
    dangerStrong: "#a2382a",
    selection: "rgba(245, 78, 0, 0.12)",
    diffAdded: "#1f8a65",
    diffRemoved: "#cf2d56",
    scrim: "rgba(28, 27, 23, 0.32)",
    shadow: "rgba(38, 37, 30, 0.1)",
  },
  dark: {
    canvas: "#0d0d0b",
    sidebar: "#171512",
    surface: "#1f1b18",
    surfaceElevated: "#27211d",
    surfaceSubtle: "#171512",
    surfaceContrast: "#332821",
    textPrimary: "#f6f0e8",
    textSecondary: "#d5c9bb",
    textMuted: "#8f8579",
    textSubtle: "#6f665c",
    border: "rgba(246, 240, 232, 0.12)",
    borderSoft: "rgba(246, 240, 232, 0.07)",
    accent: "#ff6a32",
    accentHover: "#ff8150",
    accentText: "#0d0d0b",
    success: "#68c08d",
    successSoft: "rgba(104, 192, 141, 0.14)",
    warning: "#e3ab52",
    warningSoft: "rgba(227, 171, 82, 0.14)",
    warningStrong: "#f2c25f",
    danger: "#ef7783",
    dangerSoft: "rgba(239, 119, 131, 0.14)",
    dangerStrong: "#ff9a8b",
    selection: "rgba(255, 106, 50, 0.22)",
    diffAdded: "#72c99a",
    diffRemoved: "#ef7783",
    scrim: "rgba(0, 0, 0, 0.5)",
    shadow: "rgba(0, 0, 0, 0.32)",
  },
  builtin: true,
  warnings: [],
};

export const FLEXOKI_THEME: ReadRayThemeV1 = {
  manifest: {
    formatVersion: READRAY_THEME_FORMAT_VERSION,
    id: FLEXOKI_THEME_ID,
    name: "Flexoki",
    version: "1.1.0",
    author: "Steph Ango",
    modes: ["light", "dark"],
    license: "MIT",
    sourceUrl: "https://stephango.com/flexoki",
  },
  light: {
    canvas: "#fffcf0",
    sidebar: "#f2f0e5",
    surface: "#f2f0e5",
    surfaceElevated: "#f2f0e5",
    surfaceSubtle: "#fffcf0",
    surfaceContrast: "#e6e4d9",
    textPrimary: "#100f0f",
    textSecondary: "#575653",
    textMuted: "#878580",
    textSubtle: "#b7b5ac",
    border: "#dad8ce",
    borderSoft: "#e6e4d9",
    accent: "#24837b",
    accentHover: "#24837b",
    accentText: "#fffcf0",
    success: "#66800b",
    successSoft: "#f2f0e5",
    warning: "#ad8301",
    warningSoft: "#f2f0e5",
    warningStrong: "#ad8301",
    danger: "#af3029",
    dangerSoft: "#f2f0e5",
    dangerStrong: "#af3029",
    selection: "#f2f0e5",
    diffAdded: "#66800b",
    diffRemoved: "#af3029",
    scrim: "#100f0f",
    shadow: "#100f0f",
  },
  dark: {
    canvas: "#100f0f",
    sidebar: "#1c1b1a",
    surface: "#1c1b1a",
    surfaceElevated: "#1c1b1a",
    surfaceSubtle: "#100f0f",
    surfaceContrast: "#282726",
    textPrimary: "#cecdc3",
    textSecondary: "#878580",
    textMuted: "#6f6e69",
    textSubtle: "#575653",
    border: "#343331",
    borderSoft: "#282726",
    accent: "#3aa99f",
    accentHover: "#3aa99f",
    accentText: "#100f0f",
    success: "#879a39",
    successSoft: "#1c1b1a",
    warning: "#d0a215",
    warningSoft: "#1c1b1a",
    warningStrong: "#d0a215",
    danger: "#d14d41",
    dangerSoft: "#1c1b1a",
    dangerStrong: "#d14d41",
    selection: "#282726",
    diffAdded: "#879a39",
    diffRemoved: "#d14d41",
    scrim: "#000",
    shadow: "#000",
  },
  builtin: true,
  warnings: [],
};

export const CODEX_BUILTIN_THEMES: ReadRayThemeV1[] = CODEX_BUILTIN_FULL_THEMES.map(
  (entry: CodexThemeFull) => ({
    manifest: {
      formatVersion: READRAY_THEME_FORMAT_VERSION,
      id: entry.manifest.id,
      name: entry.manifest.name,
      version: entry.manifest.version,
      author: entry.manifest.author,
      modes: [...entry.manifest.modes],
      license: entry.manifest.license,
      sourceUrl: entry.manifest.sourceUrl,
    },
    light: entry.light ? { ...entry.light } : null,
    dark: entry.dark ? { ...entry.dark } : null,
    builtin: true,
    warnings: [],
  }),
);

export const DEFAULT_THEME_SNAPSHOT: ThemeSnapshot = {
  revision: 0,
  currentThemeId: READRAY_DEFAULT_THEME_ID,
  currentMode: "light",
  themes: [READRAY_DEFAULT_THEME, FLEXOKI_THEME, ...CODEX_BUILTIN_THEMES],
};

const colorFields = [
  "canvas",
  "sidebar",
  "surface",
  "surfaceElevated",
  "surfaceSubtle",
  "surfaceContrast",
  "textPrimary",
  "textSecondary",
  "textMuted",
  "textSubtle",
  "border",
  "borderSoft",
  "accent",
  "accentHover",
  "accentText",
  "success",
  "successSoft",
  "warning",
  "warningSoft",
  "warningStrong",
  "danger",
  "dangerSoft",
  "dangerStrong",
  "selection",
  "diffAdded",
  "diffRemoved",
  "scrim",
  "shadow",
] as const satisfies readonly (keyof ReadRayThemeColors)[];

function isNormalizedColor(value: string) {
  const hexMatch = value.match(/^#([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/);
  if (hexMatch) {
    const source = hexMatch[1];
    const expanded = source.length <= 4
      ? [...source].map((digit) => `${digit}${digit}`).join("")
      : source;
    const opaque = expanded.length === 8 && expanded.slice(6) === "ff"
      ? expanded.slice(0, 6)
      : expanded;
    const canShorten = [...opaque.matchAll(/../g)].every(
      ([pair]) => pair[0] === pair[1],
    );
    const canonical = canShorten
      ? `#${[...opaque.matchAll(/../g)].map(([pair]) => pair[0]).join("")}`
      : `#${opaque}`;
    return canonical === value;
  }
  const match = value.match(
    /^rgba?\((\d{1,3}), (\d{1,3}), (\d{1,3})(?:, (0|1|0\.\d*[1-9]))?\)$/,
  );
  if (!match) return false;
  const [, red, green, blue, alpha] = match;
  if (
    [red, green, blue].some(
      (component) => Number(component) > 255 || String(Number(component)) !== component,
    )
  ) return false;
  if (value.startsWith("rgba(") !== (alpha !== undefined)) return false;
  return true;
}

function assertString(value: unknown, label: string, maximum: number) {
  if (
    typeof value !== "string" ||
    !value.trim() ||
    Array.from(value).length > maximum
  ) {
    throw new Error(`${label}无效。`);
  }
  return value;
}

function validateColors(value: unknown, label: string): ReadRayThemeColors {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label}配色无效。`);
  }
  const colors = value as Record<string, unknown>;
  const normalized = {} as ReadRayThemeColors;
  for (const field of colorFields) {
    const color = assertString(colors[field], `${label} ${field}`, 64);
    if (!isNormalizedColor(color)) {
      throw new Error(`${label} ${field} 不是规范化颜色。`);
    }
    normalized[field] = color;
  }
  return normalized;
}

export function validateThemeSnapshot(value: ThemeSnapshot): ThemeSnapshot {
  if (!value || typeof value !== "object") {
    throw new Error("主题快照结构无效。");
  }
  if (!Number.isSafeInteger(value.revision) || value.revision < 0) {
    throw new Error("主题版本无效，请重新读取后重试。");
  }
  if (!Array.isArray(value.themes) || value.themes.length < 1 || value.themes.length > 94) {
    throw new Error("主题列表数量无效。");
  }
  if (!(["light", "dark"] as const).includes(value.currentMode)) {
    throw new Error("当前主题模式无效。");
  }

  const ids = new Set<string>();
  const themes = value.themes.map((theme): ReadRayThemeV1 => {
    if (!theme || typeof theme !== "object") throw new Error("主题结构无效。");
    const manifest = theme.manifest;
    if (!manifest || manifest.formatVersion !== READRAY_THEME_FORMAT_VERSION) {
      throw new Error("主题协议版本无效。");
    }
    const id = assertString(manifest.id, "主题 ID", 64);
    if (ids.has(id)) throw new Error("主题列表包含重复 ID。");
    ids.add(id);
    const modes = manifest.modes;
    if (
      !Array.isArray(modes) ||
      modes.length < 1 ||
      modes.length > 2 ||
      modes.some((mode) => mode !== "light" && mode !== "dark") ||
      new Set(modes).size !== modes.length
    ) {
      throw new Error(`${id} 的主题模式无效。`);
    }
    if (typeof theme.builtin !== "boolean") throw new Error(`${id} 的内置标记无效。`);
    if (
      !Array.isArray(theme.warnings) ||
      theme.warnings.length > 128 ||
      theme.warnings.some(
        (warning) =>
          typeof warning !== "string" || Array.from(warning).length > 512,
      )
    ) {
      throw new Error(`${id} 的主题警告无效。`);
    }
    const light = theme.light === null ? null : validateColors(theme.light, `${id} light`);
    const dark = theme.dark === null ? null : validateColors(theme.dark, `${id} dark`);
    if (modes.includes("light") !== (light !== null) || modes.includes("dark") !== (dark !== null)) {
      throw new Error(`${id} 的主题模式与配色不一致。`);
    }
    return {
      manifest: {
        formatVersion: READRAY_THEME_FORMAT_VERSION,
        id,
        name: assertString(manifest.name, "主题名称", 80),
        version: assertString(manifest.version, "主题版本", 32),
        author: assertString(manifest.author, "主题作者", 80),
        modes: [...modes],
        license: manifest.license === null ? null : assertString(manifest.license, "主题许可证", 80),
        sourceUrl: manifest.sourceUrl === null ? null : assertString(manifest.sourceUrl, "主题来源", 2048),
      },
      light,
      dark,
      builtin: theme.builtin,
      warnings: [...theme.warnings],
    };
  });
  const current = themes.find((theme) => theme.manifest.id === value.currentThemeId);
  if (!current || !current.manifest.modes.includes(value.currentMode)) {
    throw new Error("当前主题不存在或不支持已保存的模式。");
  }
  const builtinById = new Map(
    [READRAY_DEFAULT_THEME, FLEXOKI_THEME, ...CODEX_BUILTIN_THEMES].map(
      (theme) => [theme.manifest.id, theme],
    ),
  );
  for (const [builtinId, canonical] of builtinById) {
    const found = themes.find((theme) => theme.manifest.id === builtinId);
    if (!found?.builtin || JSON.stringify(found) !== JSON.stringify(canonical)) {
      throw new Error(`${builtinId} 内置主题缺失或无效。`);
    }
  }
  const unknownBuiltin = themes.filter(
    (theme) => theme.builtin && !builtinById.has(theme.manifest.id),
  );
  if (unknownBuiltin.length > 0) {
    throw new Error("主题列表包含未知内置主题。");
  }
  return {
    revision: value.revision,
    currentThemeId: value.currentThemeId,
    currentMode: value.currentMode,
    themes,
  };
}

export function validateCustomTheme(value: ReadRayThemeV1): ReadRayThemeV1 {
  const validated = validateThemeSnapshot({
    ...DEFAULT_THEME_SNAPSHOT,
    themes: [...DEFAULT_THEME_SNAPSHOT.themes, value],
  });
  const theme = validated.themes.find(
    (candidate) => candidate.manifest.id === value.manifest.id,
  );
  if (!theme || theme.builtin) throw new Error("待导入的自定义主题无效。");
  return theme;
}

export function selectedThemeColors(snapshot: ThemeSnapshot) {
  const theme = snapshot.themes.find(
    (candidate) => candidate.manifest.id === snapshot.currentThemeId,
  );
  const colors = snapshot.currentMode === "light" ? theme?.light : theme?.dark;
  if (!theme || !colors) throw new Error("当前主题配色不可用。");
  return colors;
}

export const THEME_RUNTIME_VARIABLES = {
  canvas: "--rr-main-bg",
  sidebar: "--rr-main-sidebar",
  surface: "--rr-main-surface",
  surfaceElevated: "--rr-main-surface-warm",
  surfaceSubtle: "--rr-main-surface-subtle",
  surfaceContrast: "--rr-main-surface-contrast",
  textPrimary: "--rr-main-fg",
  textSecondary: "--rr-main-fg-secondary",
  textMuted: "--rr-main-muted",
  textSubtle: "--rr-main-meta",
  border: "--rr-main-border",
  borderSoft: "--rr-main-border-soft",
  accent: "--rr-main-accent",
  accentHover: "--rr-main-accent-hover",
  accentText: "--rr-main-accent-text",
  success: "--rr-main-success",
  successSoft: "--rr-main-success-soft",
  warning: "--rr-main-warning",
  warningSoft: "--rr-main-warning-soft",
  warningStrong: "--rr-main-warning-strong",
  danger: "--rr-main-danger",
  dangerSoft: "--rr-main-danger-soft",
  dangerStrong: "--rr-main-danger-strong",
  selection: "--rr-main-selection",
  diffAdded: "--rr-main-diff-added",
  diffRemoved: "--rr-main-diff-removed",
  scrim: "--rr-main-scrim",
  shadow: "--rr-main-shadow",
} as const satisfies Record<keyof ReadRayThemeColors, string>;

export function themeCssVariables(snapshot: ThemeSnapshot): Record<string, string> {
  const colors = selectedThemeColors(validateThemeSnapshot(snapshot));
  return Object.fromEntries(
    colorFields.map((field) => [THEME_RUNTIME_VARIABLES[field], colors[field]]),
  );
}

export type ThemeStyleTarget = {
  getPropertyValue(name: string): string;
  setProperty(name: string, value: string): void;
  removeProperty(name: string): void;
};

export function applyThemeVariables(target: ThemeStyleTarget, snapshot: ThemeSnapshot) {
  const variables = themeCssVariables(snapshot);
  const previous = new Map<string, string>();
  for (const name of Object.values(THEME_RUNTIME_VARIABLES)) {
    previous.set(name, target.getPropertyValue(name));
  }
  try {
    for (const [name, value] of Object.entries(variables)) {
      target.setProperty(name, value);
    }
  } catch (error) {
    for (const [name, value] of previous) {
      if (value) target.setProperty(name, value);
      else target.removeProperty(name);
    }
    throw error;
  }
}
