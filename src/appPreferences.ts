export type UiFont = "geistSourceHanSans" | "sourceHanSans";
export type LearningFont = "newsreaderSourceHanSerif" | "sourceHanSerif";
export type SendShortcut = "enter" | "ctrlEnter";
export type CloseBehavior = "hideToTray" | "exit";
export type SelectionExplanationDisplayMode = "reducedMotion" | "standard";

export type ShortcutBinding =
  | {
      version: 2;
      kind: "chord";
      accelerator: string;
    }
  | {
      version: 2;
      kind: "modifierDoubleTap";
      modifier: "Alt";
      side: "left";
    };

export type AppPreferences = {
  revision: number;
  uiFont: UiFont;
  uiFontSize: number;
  learningFont: LearningFont;
  learningFontSize: number;
  sendShortcut: SendShortcut;
  closeBehavior: CloseBehavior;
  selectionExplanationDisplayMode: SelectionExplanationDisplayMode;
  quickQueryBinding: ShortcutBinding;
  selectionExplanationBinding: ShortcutBinding;
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
  closeBehavior: "hideToTray",
  selectionExplanationDisplayMode: "standard",
  quickQueryBinding: {
    version: 2,
    kind: "chord",
    accelerator: "Alt+Super+Space",
  },
  selectionExplanationBinding: {
    version: 2,
    kind: "modifierDoubleTap",
    modifier: "Alt",
    side: "left",
  },
};

const uiFonts: UiFont[] = ["geistSourceHanSans", "sourceHanSans"];
const learningFonts: LearningFont[] = [
  "newsreaderSourceHanSerif",
  "sourceHanSerif",
];
const sendShortcuts: SendShortcut[] = ["enter", "ctrlEnter"];
const closeBehaviors: CloseBehavior[] = ["hideToTray", "exit"];
const selectionExplanationDisplayModes: SelectionExplanationDisplayMode[] = [
  "reducedMotion",
  "standard",
];

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
  if (!closeBehaviors.includes(preferences.closeBehavior)) {
    throw new Error("设置返回了未知的主窗口关闭策略。");
  }
  if (
    !selectionExplanationDisplayModes.includes(
      preferences.selectionExplanationDisplayMode,
    )
  ) {
    throw new Error("设置返回了未知的划词卡片显示模式。");
  }
  validateShortcutBinding(preferences.quickQueryBinding, "快速查询");
  validateShortcutBinding(preferences.selectionExplanationBinding, "选区解释");
  if (
    shortcutBindingIdentity(preferences.quickQueryBinding) ===
    shortcutBindingIdentity(preferences.selectionExplanationBinding)
  ) {
    throw new Error("快速查询和选区解释不能使用同一个快捷键。");
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

export function validateShortcutBinding(
  binding: ShortcutBinding,
  label = "全局",
): ShortcutBinding {
  if (!binding || typeof binding !== "object" || binding.version !== 2) {
    throw new Error("设置返回的全局快捷键不完整。");
  }
  if (binding.kind === "chord") {
    if (!binding.accelerator?.trim() || !binding.accelerator.includes("+")) {
      throw new Error(`${label}快捷键必须包含修饰键。`);
    }
    return binding;
  }
  if (
    binding.kind === "modifierDoubleTap" &&
    binding.modifier === "Alt" &&
    binding.side === "left"
  ) {
    return binding;
  }
  throw new Error(`${label}快捷键包含尚未支持的高级手势。`);
}

export function shortcutBindingIdentity(binding: ShortcutBinding) {
  return binding.kind === "chord"
    ? `chord:${binding.accelerator}`
    : `double:${binding.side}:${binding.modifier}`;
}

export function shortcutBindingLabel(binding: ShortcutBinding) {
  return binding.kind === "chord"
    ? binding.accelerator.replace(/\bSuper\b/g, "Win")
    : "左 Alt × 2";
}

export function shortcutBindingParts(binding: ShortcutBinding) {
  return binding.kind === "chord"
    ? binding.accelerator
        .split("+")
        .filter(Boolean)
        .map((part) => part === "Super" ? "Win" : part)
    : ["左 Alt", "×2"];
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
