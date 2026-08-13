const MAIN_READY_CLASS = "rr-main-ready";

/**
 * 在主应用完成第一次 React 绘制后移除静态品牌启动层。
 *
 * 启动层本身由 index.html 提供，因此不依赖 React、主题 IPC 或业务数据；
 * 这里只负责发布“主界面已经可以接管绘制”的单向信号。
 */
export function markMainStartupReady(
  root: Pick<HTMLElement, "classList"> = document.documentElement,
): void {
  root.classList.add(MAIN_READY_CLASS);
}
