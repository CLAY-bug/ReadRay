import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent,
  type PointerEvent,
} from "react";
import type {
  MainAppNavigationId,
  MainAppNavigationItem,
  RecentConversationItem,
} from "../mainAppViewModel";
import MainAppIcon from "./MainAppIcon";

const SIDEBAR_MIN_WIDTH = 180;
const SIDEBAR_MAX_WIDTH = 360;

type MainSidebarProps = {
  collapsed: boolean;
  width: number | null;
  onWidthChange: (width: number) => void;
  navigation: MainAppNavigationItem[];
  recentConversations: RecentConversationItem[];
  recentStatus: "loading" | "ready" | "error";
  recentError?: string;
  activeNavigationId?: MainAppNavigationId;
  activeConversationId?: string;
  onNewConversation: () => void;
  onNavigate: (id: MainAppNavigationId) => void;
  onRecentConversationSelect: (id: string) => void;
  onRecentConversationContextMenu: (
    conversation: RecentConversationItem,
    event: MouseEvent<HTMLElement>,
  ) => void;
  onViewAllConversations: () => void;
  onRecentRetry: () => void;
  onPeekEnter?: () => void;
  onPeekLeave?: () => void;
};

type RecentConversationTitleProps = {
  title: string;
};

function RecentConversationTitle({ title }: RecentConversationTitleProps) {
  const titleRef = useRef<HTMLSpanElement>(null);
  const [isOverflowing, setIsOverflowing] = useState(false);

  useLayoutEffect(() => {
    const titleElement = titleRef.current;
    if (!titleElement) {
      return;
    }

    const updateOverflow = () => {
      setIsOverflowing(titleElement.scrollWidth > titleElement.clientWidth);
    };

    updateOverflow();
    void document.fonts.ready.then(updateOverflow);
    const observer = new ResizeObserver(updateOverflow);
    observer.observe(titleElement);
    return () => observer.disconnect();
  }, [title]);

  return (
    <span
      ref={titleRef}
      className={`rr-main-recent-text${isOverflowing ? " is-overflowing" : ""}`}
    >
      {title}
    </span>
  );
}

function MainSidebar({
  collapsed,
  width,
  onWidthChange,
  navigation,
  recentConversations,
  recentStatus,
  recentError,
  activeNavigationId,
  activeConversationId,
  onNewConversation,
  onNavigate,
  onRecentConversationSelect,
  onRecentConversationContextMenu,
  onViewAllConversations,
  onRecentRetry,
  onPeekEnter,
  onPeekLeave,
}: MainSidebarProps) {
  const settingsActive = activeNavigationId === "settings";
  const dragState = useRef<{ startX: number; startWidth: number } | null>(null);

  const handlePointerDown = useCallback((event: PointerEvent<HTMLDivElement>) => {
    if (collapsed) return;
    dragState.current = {
      startX: event.clientX,
      startWidth: width ?? 252,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  }, [collapsed, width]);

  const handlePointerMove = useCallback((event: PointerEvent<HTMLDivElement>) => {
    const state = dragState.current;
    if (!state) return;
    const next = Math.round(
      Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, state.startWidth + (event.clientX - state.startX))),
    );
    onWidthChange(next);
  }, [onWidthChange]);

  const handlePointerUp = useCallback((event: PointerEvent<HTMLDivElement>) => {
    dragState.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  }, []);

  return (
    <aside
      className="rr-main-sidebar"
      aria-label="全局导航"
      onPointerEnter={onPeekEnter}
      onPointerLeave={onPeekLeave}
    >
      <button
        className="rr-main-new-chat"
        type="button"
        title="新对话"
        aria-label={collapsed ? "新对话" : undefined}
        onClick={onNewConversation}
      >
        <span className="rr-main-nav-icon"><MainAppIcon name="plus" /></span>
        <span className="rr-main-nav-label">新对话</span>
      </button>

      <nav aria-label="主要功能">
        {navigation.map((item) => {
          const active = item.id === activeNavigationId;
          return (
            <button
              className={`rr-main-nav-item${active ? " is-active" : ""}`}
              type="button"
              title={item.label}
              aria-current={active ? "page" : undefined}
              aria-label={collapsed ? item.label : undefined}
              key={item.id}
              onClick={() => onNavigate(item.id)}
            >
              <span className="rr-main-nav-icon"><MainAppIcon name={item.icon} /></span>
              <span className="rr-main-nav-label">{item.label}</span>
            </button>
          );
        })}
      </nav>

      <div className="rr-main-sidebar-rule" aria-hidden="true" />

      <section className="rr-main-recent" aria-labelledby="rr-main-recent-label">
        <div className="rr-main-section-label" id="rr-main-recent-label">最近对话</div>
        <div className="rr-main-recent-list">
          {recentStatus === "loading" ? (
            <div className="rr-main-recent-state">正在读取…</div>
          ) : recentStatus === "error" ? (
            <div className="rr-main-recent-state" title={recentError}>
              <span>暂时无法读取</span>
              <button type="button" onClick={onRecentRetry}>重试</button>
            </div>
          ) : recentConversations.length ? recentConversations.map((conversation) => (
            <button
              className={`rr-main-recent-item${
                conversation.id === activeConversationId ? " is-active" : ""
              }`}
              type="button"
              key={conversation.id}
              title={conversation.title}
              aria-current={
                conversation.id === activeConversationId ? "page" : undefined
              }
              onClick={() => onRecentConversationSelect(conversation.id)}
              onContextMenu={(event) =>
                onRecentConversationContextMenu(conversation, event)
              }
            >
              <RecentConversationTitle title={conversation.title} />
            </button>
          )) : (
            <div className="rr-main-recent-state">暂无 Quick AI 对话</div>
          )}
        </div>
        <button
          className="rr-main-view-all"
          type="button"
          onClick={onViewAllConversations}
        >
          <span className="rr-main-view-all-label">查看全部对话</span>
        </button>
      </section>

      <div className="rr-main-settings-footer">
        <button
          className={`rr-main-settings${settingsActive ? " is-active" : ""}`}
          type="button"
          title="设置"
          aria-current={settingsActive ? "page" : undefined}
          aria-label={collapsed ? "设置" : undefined}
          onClick={() => onNavigate("settings")}
        >
          <span className="rr-main-nav-icon"><MainAppIcon name="settings" /></span>
          <span className="rr-main-nav-label">设置</span>
        </button>
      </div>

      <div
        className="rr-main-sidebar-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label="调整侧边栏宽度"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
      />
    </aside>
  );
}

export default MainSidebar;
