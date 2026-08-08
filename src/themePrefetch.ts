import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import {
  validateThemeSnapshot,
  type ThemeSnapshot,
} from "./themeProtocol.ts";

/**
 * 主应用首帧前预取的已选主题快照。
 *
 * 主应用入口在 React 挂载前通过 `prefetchThemeSnapshot` 用 Tauri IPC
 * 读取 SQLite 中的已选主题；`useAppTheme` 在首帧 layout effect 里优先
 * 使用该快照，使 `.rr-main-app` 首次绘制即为已选主题，避免
 * "CSS 硬编码默认主题 → 异步应用已选主题" 的启动闪烁。
 *
 * 该快照不消费：重复读取幂等，StrictMode 双跑挂载也能得到一致结果。
 */

type ThemeInvoke = <T>(command: string) => Promise<T>;

const PREFETCH_TIMEOUT_MS = 2000;

let prefetched: ThemeSnapshot | null = null;
let invokeTheme: ThemeInvoke = tauriInvoke;
let prefetchTimeoutMs: number = PREFETCH_TIMEOUT_MS;

/** 测试注入点：替换 Tauri invoke，使预取逻辑可被 Node 测试直接驱动。 */
export function __setThemeInvokeForTest(next: ThemeInvoke): void {
  invokeTheme = next;
}

/** 测试注入点：缩短超时窗口，避免挂起用例等待完整 2 秒。 */
export function __setThemePrefetchTimeoutForTest(ms: number): void {
  prefetchTimeoutMs = ms;
}

/** 测试重置点：清空已预取快照，避免跨用例状态泄漏。 */
export function __resetPrefetchedThemeForTest(): void {
  prefetched = null;
}

export function getPrefetchedThemeSnapshot(): ThemeSnapshot | null {
  return prefetched;
}

/** 浏览器/Node 均可安全调用：非浏览器环境视为非 Tauri。 */
function isTauriRuntime(): boolean {
  return (
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window
  );
}

/**
 * 仅在 Tauri 运行时预取主应用主题；浏览器预览 / overlay / Node 立即返回。
 * 预取失败或超时静默回退（返回 null），由 useAppTheme 走原有 reload 路径。
 * 超时兜底防止 IPC 异常挂起时阻塞 React 挂载导致白屏。
 */
export async function prefetchThemeSnapshot(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  try {
    const snapshot = await Promise.race([
      invokeTheme<ThemeSnapshot>("get_theme_snapshot"),
      new Promise<never>((_, reject) => {
        setTimeout(
          () => reject(new Error("主题预取超时")),
          prefetchTimeoutMs,
        );
      }),
    ]);
    prefetched = validateThemeSnapshot(snapshot);
  } catch (error) {
    console.error("ReadRay 主题预取失败，将使用默认主题：", error);
  }
}
