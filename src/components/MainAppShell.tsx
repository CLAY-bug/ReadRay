import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
} from "react";
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
import {
  loadMainSidebarWidth,
  saveMainSidebarWidth,
} from "../mainSidebarWidth";
import {
  createSidebarAutoCollapseState,
  reduceSidebarAutoCollapse,
  releaseSidebarAutoCollapse,
  type SidebarAutoCollapseState,
} from "../sidebarAutoCollapse";
import type { MemoryPageViewModel } from "../memoryViewModel";
import type { MemoryService } from "../memoryService";
import type { ReviewService } from "../reviewService";
import type { ReviewPreparationCoordinator } from "../reviewPreparationCoordinator";
import type { ReviewQualityCoordinator } from "../reviewQualitySaveQueue";
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
import ReviewPage from "./ReviewPage";
import SettingsPage from "./SettingsPage";
import TodayPage from "./TodayPage";
import WritingPage from "./WritingPage";

export type MainResizeDirection =
  | "n"
  | "ne"
  | "e"
  | "se"
  | "s"
  | "sw"
  | "w"
  | "nw";

type MainAppShellProps = {
  viewModel: MainAppViewModel;
  memoryViewModel: MemoryPageViewModel;
  memoryService: MemoryService | null;
  memoryRefreshToken: number;
  reviewService: ReviewService | null;
  reviewPreparationCoordinator: ReviewPreparationCoordinator | null;
  reviewQualityCoordinator: ReviewQualityCoordinator | null;
  reviewRefreshToken: number;
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
  onStartResize?: (direction: MainResizeDirection) => void;
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
  reviewService,
  reviewPreparationCoordinator,
  reviewQualityCoordinator,
  reviewRefreshToken,
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
  onStartResize = noop,
  onMinimize = noop,
  onToggleMaximize = noop,
  onClose = noop,
}: MainAppShellProps) {
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [sidebarAutoCollapsed, setSidebarAutoCollapsed] = useState(false);
  const [sidebarResizing, setSidebarResizing] = useState(false);
  const [windowResizing, setWindowResizing] = useState(false);
  const [sidebarWidth, setSidebarWidth] = useState<number | null>(() =>
    loadMainSidebarWidth(),
  );
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
  const sidebarCollapsedRef = useRef(sidebarCollapsed);
  const sidebarAutoCollapsedRef = useRef(sidebarAutoCollapsed);
  const sidebarAutoCollapseRef = useRef<SidebarAutoCollapseState>(
    createSidebarAutoCollapseState(),
  );
  const windowResizingRef = useRef(false);
  const windowResizeSettleTimerRef = useRef<number | null>(null);
  const lastObservedWindowSizeRef = useRef<{
    width: number;
    height: number;
  } | null>(null);
  activePageIdRef.current = activePageId;
  activeConversationIdRef.current = activeConversationId;
  conversationRequestRef.current = conversationRequest;
  sidebarCollapsedRef.current = sidebarCollapsed;
  sidebarAutoCollapsedRef.current = sidebarAutoCollapsed;
  // 有效折叠 = 用户手动折叠或窗口过窄触发的自动折叠；手动切换优先表达用户意图。
  const sidebarEffectiveCollapsed = sidebarCollapsed || sidebarAutoCollapsed;

  function updateActivePage(
    nextPageId: MainAppNavigationId | "conversation" | "conversation-history",
  ) {
    activePageIdRef.current = nextPageId;
    setConversationMenuTarget(null);
    setActivePageId(nextPageId);
  }

  const updateActiveConversation = useCallback(
    (nextConversationId?: string) => {
      activeConversationIdRef.current = nextConversationId;
      setActiveConversationId(nextConversationId);
    },
    [],
  );

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
  const [sidebarPeekOpen, setSidebarPeekOpen] = useState(false);
  const sidebarPeekCloseTimerRef = useRef<number | null>(null);

  function clearSidebarPeekCloseTimer() {
    if (sidebarPeekCloseTimerRef.current === null) {
      return;
    }

    window.clearTimeout(sidebarPeekCloseTimerRef.current);
    sidebarPeekCloseTimerRef.current = null;
  }

  function toggleSidebar() {
    clearSidebarPeekCloseTimer();
    setSidebarPeekOpen(false);
    if (sidebarCollapsedRef.current || sidebarAutoCollapsedRef.current) {
      // 当前处于折叠（手动或自动）：用户意图是展开，并清除自动折叠状态。
      sidebarAutoCollapseRef.current = releaseSidebarAutoCollapse(
        sidebarAutoCollapseRef.current,
      );
      setSidebarAutoCollapsed(false);
      setSidebarCollapsed(false);
      return;
    }
    setSidebarCollapsed(true);
  }

  const commitSidebarWidth = useCallback((width: number) => {
    setSidebarResizing(false);
    setSidebarWidth(width);
    saveMainSidebarWidth(width);
  }, []);

  // resizer 逐帧回调期间挂出 is-sidebar-resizing，抑制宽度过渡追赶指针。
  const handleSidebarWidthChange = useCallback((width: number) => {
    setSidebarResizing(true);
    setSidebarWidth(width);
  }, []);

  function handleSidebarPeekEnter() {
    if (!sidebarEffectiveCollapsed) {
      return;
    }

    clearSidebarPeekCloseTimer();
    setSidebarPeekOpen(true);
  }

  function handleSidebarPeekLeave() {
    if (!sidebarEffectiveCollapsed) {
      return;
    }

    clearSidebarPeekCloseTimer();
    sidebarPeekCloseTimerRef.current = window.setTimeout(() => {
      sidebarPeekCloseTimerRef.current = null;
      setSidebarPeekOpen(false);
    }, 180);
  }

  useEffect(() => {
    return () => {
      clearSidebarPeekCloseTimer();
    };
  }, []);

  // 首次测量立即决定窄窗是否折叠；连续窗口缩放只记录最终宽度，待 120ms
  // 稳定后再执行响应式切换，避免侧栏过渡与 WebView2 尺寸追赶同时发生。
  useEffect(() => {
    const root = appRootRef.current;
    if (!root || typeof ResizeObserver === "undefined") {
      return;
    }

    const applySettledWidth = (width: number) => {
      const previous = sidebarAutoCollapseRef.current;
      const next = reduceSidebarAutoCollapse(
        previous,
        width,
        sidebarCollapsedRef.current,
      );
      if (next === previous) {
        return;
      }
      sidebarAutoCollapseRef.current = next;
      if (next.autoCollapsed !== previous.autoCollapsed) {
        setSidebarAutoCollapsed(next.autoCollapsed);
      }
    };

    const setWindowResizeActivity = (active: boolean) => {
      if (windowResizingRef.current === active) {
        return;
      }
      windowResizingRef.current = active;
      setWindowResizing(active);
    };

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) {
        return;
      }
      const width = Math.round(entry.contentRect.width);
      const height = Math.round(entry.contentRect.height);
      const previousSize = lastObservedWindowSizeRef.current;
      if (previousSize?.width === width && previousSize.height === height) {
        return;
      }
      lastObservedWindowSizeRef.current = { width, height };
      if (previousSize === null) {
        applySettledWidth(width);
        return;
      }

      setWindowResizeActivity(true);
      if (windowResizeSettleTimerRef.current !== null) {
        window.clearTimeout(windowResizeSettleTimerRef.current);
      }
      windowResizeSettleTimerRef.current = window.setTimeout(() => {
        windowResizeSettleTimerRef.current = null;
        applySettledWidth(width);
        setWindowResizeActivity(false);
      }, 120);
    });
    observer.observe(root);
    return () => {
      observer.disconnect();
      if (windowResizeSettleTimerRef.current !== null) {
        window.clearTimeout(windowResizeSettleTimerRef.current);
        windowResizeSettleTimerRef.current = null;
      }
      windowResizingRef.current = false;
      lastObservedWindowSizeRef.current = null;
    };
  }, []);

  // 自动展开后侧栏恢复固定，丢弃遗留的 hover 预览状态。
  useEffect(() => {
    if (!sidebarEffectiveCollapsed) {
      clearSidebarPeekCloseTimer();
      setSidebarPeekOpen(false);
    }
  }, [sidebarEffectiveCollapsed]);

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
        toggleSidebar();
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
      id === "review" ||
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

  function handleReviewSourceOpen(learningRecordId: string) {
    setRequestedMemoryRecordId(learningRecordId);
    updateActivePage("memory");
    onNavigate("memory");
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
      aria-label={sidebarEffectiveCollapsed ? "展开左侧栏" : "折叠左侧栏"}
      aria-expanded={!sidebarEffectiveCollapsed}
      title={`${sidebarEffectiveCollapsed ? "展开" : "折叠"}左侧栏（Ctrl+B）`}
      onClick={toggleSidebar}
      onPointerEnter={handleSidebarPeekEnter}
      onPointerLeave={handleSidebarPeekLeave}
    >
      <MainAppIcon
        name={sidebarEffectiveCollapsed ? "panel-closed" : "panel-open"}
      />
    </button>
  );

  const resizeDirections: MainResizeDirection[] = [
    "n",
    "ne",
    "e",
    "se",
    "s",
    "sw",
    "w",
    "nw",
  ];

  return (
    <div
      ref={appRootRef}
      inert={interactionBlocked ? true : undefined}
      aria-busy={interactionBlocked || undefined}
      className={`rr-main-app${isMaximized ? " is-maximized" : ""}${
        sidebarEffectiveCollapsed ? " is-sidebar-collapsed" : ""
      }${
        sidebarEffectiveCollapsed && sidebarPeekOpen ? " is-sidebar-peeking" : ""
      }${sidebarResizing ? " is-sidebar-resizing" : ""}${
        windowResizing ? " is-window-resizing" : ""
      }${
        activePageId === "conversation" ? " is-conversation-page" : ""
      }`}
      style={
        sidebarWidth === null
          ? undefined
          : ({
              "--rr-main-sidebar-width": `${sidebarWidth}px`,
            } as CSSProperties)
      }
    >
      {resizeDirections.map((direction) => (
        <div
          key={direction}
          className={`rr-main-resize-handle is-${direction}`}
          aria-hidden="true"
          onMouseDown={(event) => {
            if (event.button !== 0) return;
            onStartResize(direction);
          }}
        />
      ))}
      <header
        className="rr-main-titlebar"
        aria-label="ReadRay 窗口标题栏"
        onMouseDown={handleTitlebarMouseDown}
      >
        <div className="rr-main-titlebar-leading">
          <div className="rr-main-brand-zone">
            <img
              className="rr-main-brand-icon"
              src="/branding/readray-startup-icon.png"
              alt=""
              width="24"
              height="24"
            />
            <span className="rr-main-brand-name">ReadRay</span>
          </div>
          {collapseButton}
        </div>
        <div className="rr-main-drag-zone">
          {activePageId === "conversation"
            ? "对话"
            : activePageId === "conversation-history"
            ? "全部对话"
            : activePageId === "memory"
            ? "记忆"
            : activePageId === "review"
            ? "复习"
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
        <div className="rr-main-sidebar-slot">
          <MainSidebar
            collapsed={sidebarEffectiveCollapsed}
            width={sidebarWidth}
            onWidthChange={handleSidebarWidthChange}
            onWidthChangeEnd={commitSidebarWidth}
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
            onPeekEnter={handleSidebarPeekEnter}
            onPeekLeave={handleSidebarPeekLeave}
          />
        </div>
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
        ) : activePageId === "review" ? (
          <ReviewPage
            service={reviewService}
            preparationCoordinator={reviewPreparationCoordinator}
            qualityCoordinator={reviewQualityCoordinator}
            refreshToken={reviewRefreshToken}
            onOpenMemoryRecord={handleReviewSourceOpen}
            onReturnToday={() => {
              updateActivePage("today");
              onNavigate("today");
            }}
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
