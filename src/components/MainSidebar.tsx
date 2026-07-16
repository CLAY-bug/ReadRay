import { useLayoutEffect, useRef, useState } from "react";
import type {
  MainAppNavigationId,
  MainAppNavigationItem,
  RecentConversationItem,
} from "../mainAppViewModel";
import MainAppIcon from "./MainAppIcon";

type MainSidebarProps = {
  collapsed: boolean;
  navigation: MainAppNavigationItem[];
  recentConversations: RecentConversationItem[];
  activeNavigationId: MainAppNavigationId;
  onNewConversation: () => void;
  onNavigate: (id: MainAppNavigationId) => void;
  onRecentConversationSelect: (id: string) => void;
  onViewAllConversations: () => void;
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
  navigation,
  recentConversations,
  activeNavigationId,
  onNewConversation,
  onNavigate,
  onRecentConversationSelect,
  onViewAllConversations,
}: MainSidebarProps) {
  return (
    <aside className="rr-main-sidebar" aria-label="全局导航">
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
          {recentConversations.map((conversation) => (
            <button
              className="rr-main-recent-item"
              type="button"
              key={conversation.id}
              title={conversation.title}
              onClick={() => onRecentConversationSelect(conversation.id)}
            >
              <RecentConversationTitle title={conversation.title} />
            </button>
          ))}
        </div>
        <button
          className="rr-main-view-all"
          type="button"
          onClick={onViewAllConversations}
        >
          <span className="rr-main-view-all-label">查看全部对话</span>
        </button>
      </section>

      <button
        className="rr-main-settings"
        type="button"
        title="设置"
        aria-label={collapsed ? "设置" : undefined}
        onClick={() => onNavigate("settings")}
      >
        <span className="rr-main-nav-icon"><MainAppIcon name="settings" /></span>
        <span className="rr-main-nav-label">设置</span>
      </button>
    </aside>
  );
}

export default MainSidebar;
