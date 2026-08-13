type ContextMenuTarget = Pick<Window, "addEventListener" | "removeEventListener">;

/**
 * 屏蔽 WebView2 自带的浏览器右键菜单，但不停止事件传播。
 *
 * ReadRay 的会话列表仍可在组件层接收 contextmenu 事件并显示自己的菜单。
 */
export function installDesktopContextMenuGuard(
  target: ContextMenuTarget = window,
): () => void {
  const preventWebviewMenu = (event: Event) => {
    event.preventDefault();
  };

  target.addEventListener("contextmenu", preventWebviewMenu, true);

  return () => {
    target.removeEventListener("contextmenu", preventWebviewMenu, true);
  };
}
