import { useEffect, useRef, useState, type MouseEvent } from "react";
import type {
  ConversationOperationIdentity,
  ConversationRequest,
  ConversationService,
} from "../conversationViewModel";
import {
  isActiveConversationOperation,
  shouldResetDeletedConversation,
} from "../conversationViewModel";
import type {
  MainAppNavigationId,
  MainAppViewModel,
  RecentConversationItem,
  TodayActionId,
} from "../mainAppViewModel";
import type { MemoryPageViewModel } from "../memoryViewModel";
import type { MemoryService } from "../memoryService";
import {
  createTodayLoadingViewModel,
  type TodayService,
} from "../todayService";
import type { WritingService } from "../writingService";
import type { SettingsService } from "../settingsService";
import type { AppPreferences } from "../appPreferences";
import type { AppPreferenceSaveOutcome } from "../appPreferenceSaveCoordinator";
import type { AppThemeController } from "../useAppTheme";
import ConversationPage from "./ConversationPage";
import ConversationHistoryPage from "./ConversationHistoryPage";
import ConversationManagementMenu, {
  type ConversationManagementTarget,
} from "./ConversationManagementMenu";
import MainAppIcon from "./MainAppIcon";
import MainSidebar from "./MainSidebar";
import MemoryPage from "./MemoryPage";
import SettingsPage from "./SettingsPage";
import TodayPage from "./TodayPage";
import WritingPage from "./WritingPage";

type MainAppShellProps = {
  viewModel: MainAppViewModel;
  memoryViewModel: MemoryPageViewModel;
  memoryService: MemoryService | null;
  memoryRefreshToken: number;
  todayService: TodayService | null;
  learningRecordsRefreshToken: number;
  conversationRefreshToken: number;
  conversationService: ConversationService | null;
  writingService: WritingService | null;
  settingsService: SettingsService | null;
  themeController: AppThemeController;
  preferences: AppPreferences;
  onPreferencesSave: (
    candidate: AppPreferences,
    previousAuthority: AppPreferences,
  ) => Promise<AppPreferenceSaveOutcome>;
  interactionBlocked?: boolean;
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
  memoryService,
  memoryRefreshToken,
  todayService,
  learningRecordsRefreshToken,
  conversationRefreshToken,
  conversationService,
  writingService,
  settingsService,
  themeController,
  preferences,
  onPreferencesSave,
  interactionBlocked = false,
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
  const [activePageId, setActivePageId] =
    useState<MainAppNavigationId | "conversation" | "conversation-history">(
      "today",
    );
  const [activeConversationId, setActiveConversationId] = useState<string>();
  const conversationRequestKeyRef = useRef(0);
  const [conversationRequest, setConversationRequest] =
    useState<ConversationRequest>({
      key: 0,
      kind: "new",
    });
  const appRootRef = useRef<HTMLDivElement>(null);
  const conversationMenuKeyRef = useRef(0);
  const conversationTitleUpdateKeyRef = useRef(0);
  const [conversationMenuTarget, setConversationMenuTarget] =
    useState<ConversationManagementTarget | null>(null);
  const [externalConversationTitle, setExternalConversationTitle] = useState<{
    key: number;
    conversationId: string;
    title: string;
  }>();
  const activePageIdRef = useRef(activePageId);
  const activeConversationIdRef = useRef(activeConversationId);
  const conversationRequestRef = useRef(conversationRequest);
  activePageIdRef.current = activePageId;
  activeConversationIdRef.current = activeConversationId;
  conversationRequestRef.current = conversationRequest;

  function updateActivePage(
    nextPageId: MainAppNavigationId | "conversation" | "conversation-history",
  ) {
    activePageIdRef.current = nextPageId;
    setConversationMenuTarget(null);
    setActivePageId(nextPageId);
  }

  function updateActiveConversation(nextConversationId?: string) {
    activeConversationIdRef.current = nextConversationId;
    setActiveConversationId(nextConversationId);
  }

  function updateConversationRequest(nextRequest: ConversationRequest) {
    conversationRequestRef.current = nextRequest;
    setConversationRequest(nextRequest);
  }
  const [writingLibraryRequest, setWritingLibraryRequest] = useState(0);
  const [writingWindowTitle, setWritingWindowTitle] = useState("写作");
  const [requestedMemoryRecordId, setRequestedMemoryRecordId] = useState<string>();
  const [todayViewModel, setTodayViewModel] = useState(() =>
    createTodayLoadingViewModel(),
  );
  const [todayStatus, setTodayStatus] = useState<
    "loading" | "ready" | "error"
  >("loading");
  const [todayError, setTodayError] = useState<string>();
  const [todayRetryToken, setTodayRetryToken] = useState(0);
  const [recentConversations, setRecentConversations] =
    useState<RecentConversationItem[]>([]);
  const [recentStatus, setRecentStatus] = useState<
    "loading" | "ready" | "error"
  >("loading");
  const [recentError, setRecentError] = useState<string>();
  const [recentRetryToken, setRecentRetryToken] = useState(0);

  useEffect(() => {
    let ignore = false;
    if (!todayService) {
      setTodayStatus("loading");
      return;
    }

    setTodayStatus("loading");
    setTodayError(undefined);
    setTodayViewModel(createTodayLoadingViewModel());
    todayService.loadToday().then(
      (nextViewModel) => {
        if (!ignore) {
          setTodayViewModel(nextViewModel);
          setTodayStatus("ready");
        }
      },
      (error) => {
        if (!ignore) {
          setTodayError(error instanceof Error ? error.message : String(error));
          setTodayStatus("error");
        }
      },
    );
    return () => {
      ignore = true;
    };
  }, [learningRecordsRefreshToken, todayRetryToken, todayService]);

  useEffect(() => {
    let ignore = false;
    if (!todayService) {
      setRecentStatus("loading");
      return;
    }

    setRecentStatus("loading");
    setRecentError(undefined);
    todayService.listRecentConversations().then(
      (conversations) => {
        if (!ignore) {
          setRecentConversations(conversations);
          setRecentStatus("ready");
        }
      },
      (error) => {
        if (!ignore) {
          setRecentError(error instanceof Error ? error.message : String(error));
          setRecentStatus("error");
        }
      },
    );
    return () => {
      ignore = true;
    };
  }, [conversationRefreshToken, recentRetryToken, todayService]);

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
    if (
      id === "today" ||
      id === "memory" ||
      id === "writing" ||
      id === "settings"
    ) {
      updateActivePage(id);
    }
    if (id === "memory") {
      setRequestedMemoryRecordId(undefined);
    }
    if (id === "writing") {
      setWritingLibraryRequest((request) => request + 1);
    }
    onNavigate(id);
  }

  function handleTodayActionSelect(id: TodayActionId) {
    if (id === "writing") {
      handleNavigate("writing");
    } else {
      const action = todayViewModel.actions.find((item) => item.id === id);
      if (action && !action.disabled) {
        setRequestedMemoryRecordId(action.recordId);
        updateActivePage("memory");
        onNavigate("memory");
      }
    }
    onTodayActionSelect(id);
  }

  function nextConversationRequestKey() {
    conversationRequestKeyRef.current += 1;
    return conversationRequestKeyRef.current;
  }

  function handleNewConversation() {
    updateActiveConversation(undefined);
    updateConversationRequest({
      key: nextConversationRequestKey(),
      kind: "new",
    });
    updateActivePage("conversation");
    onNewConversation();
  }

  function handleRecentConversationSelect(id: string) {
    const conversation = recentConversations.find((item) => item.id === id);
    if (!conversation) {
      return;
    }
    updateActiveConversation(id);
    updateConversationRequest({
      key: nextConversationRequestKey(),
      kind: "existing",
      conversationId: id,
      title: conversation.title,
    });
    updateActivePage("conversation");
    onRecentConversationSelect(id);
  }

  function handleConversationOpen(id: string, title: string) {
    updateActiveConversation(id);
    updateConversationRequest({
      key: nextConversationRequestKey(),
      kind: "existing",
      conversationId: id,
      title,
    });
    updateActivePage("conversation");
    onRecentConversationSelect(id);
  }

  function handleViewAllConversations() {
    updateActiveConversation(undefined);
    updateActivePage("conversation-history");
    onViewAllConversations();
  }

  function handleConversationContextMenu(
    conversation: { id: string; title: string },
    event: MouseEvent<HTMLElement>,
  ) {
    event.preventDefault();
    event.stopPropagation();
    const root = appRootRef.current;
    if (!root) {
      return;
    }
    const bounds = root.getBoundingClientRect();
    const scaleX = bounds.width / root.offsetWidth || 1;
    const scaleY = bounds.height / root.offsetHeight || 1;
    const localX = (event.clientX - bounds.left) / scaleX;
    const localY = (event.clientY - bounds.top) / scaleY;
    conversationMenuKeyRef.current += 1;
    setConversationMenuTarget({
      interactionKey: conversationMenuKeyRef.current,
      conversationId: conversation.id,
      title: conversation.title,
      x: Math.max(8, Math.min(localX, root.offsetWidth - 184)),
      y: Math.max(8, Math.min(localY, root.offsetHeight - 120)),
      routeIdentity: {
        requestKey: conversationRequestRef.current.key,
        conversationId: conversation.id,
      },
    });
  }

  function handleCurrentConversationDeleted(
    operation: ConversationOperationIdentity,
  ) {
    if (
      shouldResetDeletedConversation(
        activePageIdRef.current,
        activeConversationIdRef.current,
        conversationRequestRef.current.key,
        operation,
      )
    ) {
      handleNewConversation();
    }
  }

  function handleManagedConversationRenamed(
    conversationId: string,
    title: string,
    operation: ConversationOperationIdentity,
  ) {
    if (
      isActiveConversationOperation(
        activePageIdRef.current,
        activeConversationIdRef.current,
        conversationRequestRef.current.key,
        operation,
      )
    ) {
      conversationTitleUpdateKeyRef.current += 1;
      setExternalConversationTitle({
        key: conversationTitleUpdateKeyRef.current,
        conversationId,
        title,
      });
    }
  }

  function handleSubmitPrompt(value: string) {
    updateActiveConversation(undefined);
    updateConversationRequest({
      key: nextConversationRequestKey(),
      kind: "prompt",
      prompt: value,
    });
    updateActivePage("conversation");
    onSubmitPrompt(value);
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
    <div
      ref={appRootRef}
      inert={interactionBlocked ? true : undefined}
      aria-busy={interactionBlocked || undefined}
      className={`rr-main-app${sidebarCollapsed ? " is-sidebar-collapsed" : ""}${
        activePageId === "conversation" ? " is-conversation-page" : ""
      }`}
    >
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
          {activePageId === "conversation"
            ? "对话"
            : activePageId === "conversation-history"
            ? "全部对话"
            : activePageId === "memory"
            ? "记忆"
            : activePageId === "writing"
              ? writingWindowTitle
              : activePageId === "settings"
                ? "设置"
                : "今天"}
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
          recentConversations={recentConversations}
          recentStatus={recentStatus}
          recentError={recentError}
          activeNavigationId={
            activePageId === "conversation" ||
            activePageId === "conversation-history"
              ? undefined
              : activePageId
          }
          activeConversationId={
            activePageId === "conversation" ? activeConversationId : undefined
          }
          onNewConversation={handleNewConversation}
          onNavigate={handleNavigate}
          onRecentConversationSelect={handleRecentConversationSelect}
          onRecentConversationContextMenu={handleConversationContextMenu}
          onViewAllConversations={handleViewAllConversations}
          onRecentRetry={() => setRecentRetryToken((token) => token + 1)}
        />
        {activePageId === "conversation" ? (
          conversationService ? (
            <ConversationPage
              request={conversationRequest}
              service={conversationService}
              onThreadIdentityChange={updateActiveConversation}
              onConversationDeleted={handleCurrentConversationDeleted}
              externalTitleUpdate={externalConversationTitle}
              sendShortcut={preferences.sendShortcut}
            />
          ) : (
            <main
              className="rr-main-panel rr-conversation-page"
              aria-label="ReadRay 对话"
            >
              <div className="rr-conversation-empty">
                <div className="rr-conversation-empty-copy">
                  <h2>正在准备对话</h2>
                  <p>浏览器预览数据正在加载。</p>
                </div>
              </div>
            </main>
          )
        ) : activePageId === "conversation-history" ? (
          conversationService ? (
            <ConversationHistoryPage
              service={conversationService}
              refreshToken={conversationRefreshToken}
              onOpenConversation={(conversation) =>
                handleConversationOpen(conversation.id, conversation.title)
              }
              onConversationContextMenu={handleConversationContextMenu}
            />
          ) : (
            <main className="rr-main-panel rr-conversation-history">
              <div className="rr-conversation-history-state">
                正在准备对话历史…
              </div>
            </main>
          )
        ) : activePageId === "memory" ? (
          <MemoryPage
            viewModel={memoryViewModel}
            service={memoryService}
            refreshToken={memoryRefreshToken}
            requestedRecordId={requestedMemoryRecordId}
          />
        ) : activePageId === "settings" ? (
          <SettingsPage
            service={settingsService}
            themeController={themeController}
            onPreferencesSave={onPreferencesSave}
          />
        ) : activePageId === "writing" ? null : (
          <TodayPage
            viewModel={todayViewModel}
            status={todayStatus}
            error={todayError}
            onRetry={() => setTodayRetryToken((token) => token + 1)}
            onActionSelect={handleTodayActionSelect}
            onSubmitPrompt={handleSubmitPrompt}
            sendShortcut={preferences.sendShortcut}
          />
        )}
        <WritingPage
          hidden={activePageId !== "writing"}
          libraryRequest={writingLibraryRequest}
          service={writingService}
          onWindowTitleChange={setWritingWindowTitle}
          sendShortcut={preferences.sendShortcut}
        />
      </div>
      {conversationService ? (
        <ConversationManagementMenu
          service={conversationService}
          target={conversationMenuTarget}
          pageIdentity={`${activePageId}:${conversationRequest.key}`}
          onCloseMenu={() => setConversationMenuTarget(null)}
          onRenamed={handleManagedConversationRenamed}
          onDeleted={handleCurrentConversationDeleted}
        />
      ) : null}
    </div>
  );
}

export default MainAppShell;
