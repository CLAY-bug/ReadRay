import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { installDesktopContextMenuGuard } from "./desktopContextMenu";
import { prefetchThemeSnapshot } from "./themePrefetch";

const rootElement = document.getElementById("root") as HTMLElement;

if ("__TAURI_INTERNALS__" in window) {
  installDesktopContextMenuGuard();
}

async function bootstrap() {
  // 主应用视图在 React 挂载前预取已选主题，使首帧绘制即为已选主题，
  // 避免 "CSS 硬编码默认主题 → 异步 IPC 应用已选主题" 的启动闪烁。
  // 非 Tauri 预览与 overlay 视图立即跳过；预取失败静默回退到默认主题。
  const isMainView =
    new URLSearchParams(window.location.search).get("view") === "main";
  if (isMainView) {
    await prefetchThemeSnapshot();
  }

  ReactDOM.createRoot(rootElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void bootstrap();
