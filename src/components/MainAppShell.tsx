import { useEffect, useState, type MouseEvent } from "react";
import type {
  MainAppNavigationId,
  MainAppViewModel,
  TodayActionId,
} from "../mainAppViewModel";
import type { MemoryPageViewModel } from "../memoryViewModel";
import MainAppIcon from "./MainAppIcon";
import MainSidebar from "./MainSidebar";
import MemoryPage from "./MemoryPage";
import TodayPage from "./TodayPage";
import WritingPage from "./WritingPage";

type MainAppShellProps = {
  viewModel: MainAppViewModel;
  memoryViewModel: MemoryPageViewModel;
  onNewConversation?: () => void;
  onNavigate?: (id: MainAppNavigationId) => void;
  onRecentConversationSelect?: (id: string) => void;
  onViewAllConversations?: () => void;
  onTodayActionSelect?: (id: TodayActionId) => void;
  onSubmitPrompt?: (value: string) => void;
  isMaximized?: boolean;
  onStartDragging?: () => void;
  onMinimize?: () => void;
  onToggleMaximize?: () => void;
  onClose?: () => void;
};

const noop = () => undefined;

function MainAppShell({
  viewModel,
  memoryViewModel,
  onNewConversation = noop,
  onNavigate = noop,
  onRecentConversationSelect = noop,
  onViewAllConversations = noop,
  onTodayActionSelect = noop,
  onSubmitPrompt = noop,
  isMaximized = false,
  onStartDragging = noop,
  onMinimize = noop,
  onToggleMaximize = noop,
  onClose = noop,
}: MainAppShellProps) {
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [activeNavigationId, setActiveNavigationId] =
    useState<MainAppNavigationId>("today");
  const [writingLibraryRequest, setWritingLibraryRequest] = useState(0);
  const [writingWindowTitle, setWritingWindowTitle] = useState("写作");

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

  function handleTitlebarMouseDown(event: MouseEvent<HTMLElement>) {
    const target = event.target as HTMLElement;
    if (event.button !== 0 || target.closest("button")) {
      return;
    }

    if (event.detail === 2) {
      onToggleMaximize();
    } else {
      onStartDragging();
    }
  }

  function handleNavigate(id: MainAppNavigationId) {
    if (id === "today" || id === "memory" || id === "writing") {
      setActiveNavigationId(id);
    }
    if (id === "writing") {
      setWritingLibraryRequest((request) => request + 1);
    }
    onNavigate(id);
  }

  function handleTodayActionSelect(id: TodayActionId) {
    if (id === "writing") {
      handleNavigate("writing");
    }
    onTodayActionSelect(id);
  }

  const collapseButton = (
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
  );

  return (
    <div className={`rr-main-app${sidebarCollapsed ? " is-sidebar-collapsed" : ""}`}>
      <header
        className="rr-main-titlebar"
        aria-label="ReadRay 窗口标题栏"
        onMouseDown={handleTitlebarMouseDown}
      >
        <div className="rr-main-brand-zone">
          {!sidebarCollapsed && <span className="rr-main-brand-mark">R</span>}
          {!sidebarCollapsed && <span className="rr-main-brand-name">ReadRay</span>}
          {collapseButton}
        </div>
        <div className="rr-main-drag-zone">
          {activeNavigationId === "memory"
            ? "记忆"
            : activeNavigationId === "writing" ? writingWindowTitle : "今天"}
        </div>
        <div className="rr-main-window-controls" aria-label="窗口控制">
          <button className="rr-main-window-control" type="button" aria-label="最小化" onClick={onMinimize}>
            <MainAppIcon name="minimize" />
          </button>
          <button
            className="rr-main-window-control"
            type="button"
            aria-label={isMaximized ? "还原" : "最大化"}
            onClick={onToggleMaximize}
          >
            <MainAppIcon name={isMaximized ? "restore" : "maximize"} />
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
          activeNavigationId={activeNavigationId}
          onNewConversation={onNewConversation}
          onNavigate={handleNavigate}
          onRecentConversationSelect={onRecentConversationSelect}
          onViewAllConversations={onViewAllConversations}
        />
        {activeNavigationId === "memory" ? (
          <MemoryPage viewModel={memoryViewModel} />
        ) : activeNavigationId === "writing" ? (
          <WritingPage
            libraryRequest={writingLibraryRequest}
            onWindowTitleChange={setWritingWindowTitle}
          />
        ) : (
          <TodayPage
            viewModel={viewModel.today}
            onActionSelect={handleTodayActionSelect}
            onSubmitPrompt={onSubmitPrompt}
          />
        )}
      </div>
    </div>
  );
}

export default MainAppShell;
