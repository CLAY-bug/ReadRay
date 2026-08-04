export type UiFont = "geistSourceHanSans" | "sourceHanSans";
export type LearningFont = "newsreaderSourceHanSerif" | "sourceHanSerif";
export type SendShortcut = "enter" | "ctrlEnter";

export type AppPreferences = {
  revision: number;
  uiFont: UiFont;
  uiFontSize: number;
  learningFont: LearningFont;
  learningFontSize: number;
  sendShortcut: SendShortcut;
};

export const UI_FONT_SIZE_MIN = 12;
export const UI_FONT_SIZE_MAX = 20;
export const LEARNING_FONT_SIZE_MIN = 14;
export const LEARNING_FONT_SIZE_MAX = 24;

export function parseFontSizeCandidate(
  rawValue: string,
  minimum: number,
  maximum: number,
): number | undefined {
  const trimmed = rawValue.trim();
  if (!/^\d+$/.test(trimmed)) return undefined;
  const value = Number(trimmed);
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    return undefined;
  }
  return value;
}

export const DEFAULT_APP_PREFERENCES: AppPreferences = {
  revision: 0,
  uiFont: "geistSourceHanSans",
  uiFontSize: 14,
  learningFont: "newsreaderSourceHanSerif",
  learningFontSize: 17,
  sendShortcut: "enter",
};

const uiFonts: UiFont[] = ["geistSourceHanSans", "sourceHanSans"];
const learningFonts: LearningFont[] = [
  "newsreaderSourceHanSerif",
  "sourceHanSerif",
];
const sendShortcuts: SendShortcut[] = ["enter", "ctrlEnter"];

function assertIntegerInRange(
  value: number,
  minimum: number,
  maximum: number,
  label: string,
) {
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    throw new Error(`${label}必须在 ${minimum}–${maximum} px 之间。`);
  }
}

export function validateAppPreferences(
  preferences: AppPreferences,
): AppPreferences {
  if (!preferences || typeof preferences !== "object") {
    throw new Error("设置返回的应用偏好结构无效。");
  }
  if (!Number.isSafeInteger(preferences.revision) || preferences.revision < 0) {
    throw new Error("设置版本无效，请重新读取后重试。");
  }
  if (!uiFonts.includes(preferences.uiFont)) {
    throw new Error("设置返回了未知的界面字体。");
  }
  if (!learningFonts.includes(preferences.learningFont)) {
    throw new Error("设置返回了未知的学习内容字体。");
  }
  if (!sendShortcuts.includes(preferences.sendShortcut)) {
    throw new Error("设置返回了未知的发送快捷键。");
  }
  assertIntegerInRange(
    preferences.uiFontSize,
    UI_FONT_SIZE_MIN,
    UI_FONT_SIZE_MAX,
    "界面字号",
  );
  assertIntegerInRange(
    preferences.learningFontSize,
    LEARNING_FONT_SIZE_MIN,
    LEARNING_FONT_SIZE_MAX,
    "学习内容字号",
  );
  return { ...preferences };
}

export function appPreferenceCssVariables(
  preferences: AppPreferences,
): Record<string, string> {
  const uiFont =
    preferences.uiFont === "sourceHanSans"
      ? '"ReadRay Source Han Sans SC", system-ui, sans-serif'
      : '"ReadRay Geist", "ReadRay Source Han Sans SC", system-ui, sans-serif';
  const uiMonoFont =
    preferences.uiFont === "sourceHanSans"
      ? '"ReadRay Source Han Sans SC", ui-monospace, monospace'
      : '"ReadRay Geist Mono", "ReadRay Source Han Sans SC", ui-monospace, monospace';
  const learningFont =
    preferences.learningFont === "sourceHanSerif"
      ? '"ReadRay Source Han Serif SC", ui-serif, serif'
      : '"ReadRay Newsreader", "ReadRay Source Han Serif SC", ui-serif, serif';
  return {
    "--rr-ui-font-family": uiFont,
    "--rr-ui-mono-font-family": uiMonoFont,
    "--rr-learning-font-family": learningFont,
    "--rr-ui-font-size": `${preferences.uiFontSize}px`,
    "--rr-learning-font-size": `${preferences.learningFontSize}px`,
    "--rr-ui-font-scale": String(preferences.uiFontSize / 14),
    "--rr-learning-font-scale": String(preferences.learningFontSize / 17),
  };
}

export type SendKeyEvent = {
  key: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  isComposing: boolean;
};

export function shouldSendMultilineMessage(
  event: SendKeyEvent,
  shortcut: SendShortcut,
) {
  if (event.key !== "Enter" || event.isComposing) {
    return false;
  }
  if (shortcut === "ctrlEnter") {
    return event.ctrlKey && !event.shiftKey;
  }
  return !event.shiftKey;
}
