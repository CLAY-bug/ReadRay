import { useEffect, useState } from "react";
import type {
  MainAppNavigationId,
  MainAppViewModel,
  TodayActionId,
} from "../mainAppViewModel";
import MainAppIcon from "./MainAppIcon";
import MainSidebar from "./MainSidebar";
import TodayPage from "./TodayPage";

type MainAppShellProps = {
  viewModel: MainAppViewModel;
  onNewConversation?: () => void;
  onNavigate?: (id: MainAppNavigationId) => void;
  onRecentConversationSelect?: (id: string) => void;
  onViewAllConversations?: () => void;
  onTodayActionSelect?: (id: TodayActionId) => void;
  onSubmitPrompt?: (value: string) => void;
  onMinimize?: () => void;
  onMaximize?: () => void;
  onClose?: () => void;
};

const noop = () => undefined;

function MainAppShell({
  viewModel,
  onNewConversation = noop,
  onNavigate = noop,
  onRecentConversationSelect = noop,
  onViewAllConversations = noop,
  onTodayActionSelect = noop,
  onSubmitPrompt = noop,
  onMinimize = noop,
  onMaximize = noop,
  onClose = noop,
}: MainAppShellProps) {
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

  useEffect(() => {
    function handleShortcut(event: KeyboardEvent) {
      if (event.ctrlKey && event.key.toLowerCase() === "b") {
        event.preventDefault();
        setSidebarCollapsed((collapsed) => !collapsed);
      }
    }

    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  }, []);

  return (
    <div className={`rr-main-app${sidebarCollapsed ? " is-sidebar-collapsed" : ""}`}>
      <header className="rr-main-titlebar" aria-label="ReadRay 窗口标题栏">
        <div className="rr-main-brand-zone">
          <span className="rr-main-brand-mark">R</span>
          <span className="rr-main-brand-name">ReadRay</span>
          <button
            className="rr-main-collapse"
            type="button"
            aria-label={sidebarCollapsed ? "展开左侧栏" : "折叠左侧栏"}
            aria-expanded={!sidebarCollapsed}
            title={`${sidebarCollapsed ? "展开" : "折叠"}左侧栏（Ctrl+B）`}
            onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
          >
            <MainAppIcon name="panel" />
          </button>
        </div>
        <div className="rr-main-drag-zone">今天</div>
        <div className="rr-main-window-controls" aria-label="窗口控制">
          <button className="rr-main-window-control" type="button" aria-label="最小化" onClick={onMinimize}>
            <MainAppIcon name="minimize" />
          </button>
          <button className="rr-main-window-control" type="button" aria-label="最大化" onClick={onMaximize}>
            <MainAppIcon name="maximize" />
          </button>
          <button className="rr-main-window-control is-close" type="button" aria-label="关闭" onClick={onClose}>
            <MainAppIcon name="close" />
          </button>
        </div>
      </header>

      <div className="rr-main-workspace">
        <MainSidebar
          collapsed={sidebarCollapsed}
          navigation={viewModel.navigation}
          recentConversations={viewModel.recentConversations}
          activeNavigationId="today"
          onNewConversation={onNewConversation}
          onNavigate={onNavigate}
          onRecentConversationSelect={onRecentConversationSelect}
          onViewAllConversations={onViewAllConversations}
        />
        <TodayPage
          viewModel={viewModel.today}
          onActionSelect={onTodayActionSelect}
          onSubmitPrompt={onSubmitPrompt}
        />
      </div>
    </div>
  );
}

export default MainAppShell;
