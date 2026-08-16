import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  selectedThemeColors,
  type ThemeSnapshot,
} from "./themeProtocol.ts";

type MainWindowBackgroundColor =
  | [number, number, number]
  | [number, number, number, number];

type MainWindowBackgroundSetter = (
  color: MainWindowBackgroundColor,
) => Promise<void>;

let setMainWindowBackground: MainWindowBackgroundSetter = (color) =>
  getCurrentWebviewWindow().setBackgroundColor(color);

function expandHex(source: string): string {
  return source.length <= 4
    ? [...source].map((digit) => `${digit}${digit}`).join("")
    : source;
}

/** 将主题协议允许的规范 CSS 颜色转换为 Tauri 原生颜色。 */
export function toMainWindowBackgroundColor(
  value: string,
): MainWindowBackgroundColor {
  const hex = value.match(/^#([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/);
  if (hex) {
    const expanded = expandHex(hex[1]);
    const channels = [...expanded.matchAll(/../g)].map(([pair]) =>
      Number.parseInt(pair, 16),
    );
    return channels.length === 3
      ? [channels[0], channels[1], channels[2]]
      : [channels[0], channels[1], channels[2], channels[3]];
  }

  const rgb = value.match(
    /^rgba?\((\d{1,3}), (\d{1,3}), (\d{1,3})(?:, (0|1|0\.\d*[1-9]))?\)$/,
  );
  if (!rgb) {
    throw new Error("主窗口背景色不是主题协议允许的规范颜色。");
  }
  const color: [number, number, number] = [
    Number(rgb[1]),
    Number(rgb[2]),
    Number(rgb[3]),
  ];
  return rgb[4] === undefined
    ? color
    : [...color, Math.round(Number(rgb[4]) * 255)];
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 让原生窗口层与 WebView2 默认底色跟随当前主题画布色。
 *
 * 这是视觉降级路径：主题本身已经应用成功时，原生背景同步失败不能反向
 * 阻断主题切换，因此错误仅记录，不向主题保存流程抛出。
 */
export async function syncMainWindowBackground(
  snapshot: ThemeSnapshot,
): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  try {
    const canvas = selectedThemeColors(snapshot).canvas;
    await setMainWindowBackground(toMainWindowBackgroundColor(canvas));
  } catch (error) {
    console.error("ReadRay 主窗口背景同步失败：", error);
  }
}

/** 测试注入点：避免 Node 测试调用真实 Tauri IPC。 */
export function __setMainWindowBackgroundForTest(
  setter: MainWindowBackgroundSetter,
): void {
  setMainWindowBackground = setter;
}
