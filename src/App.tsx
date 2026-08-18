import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import AnchoredResultPopover, {
  type AnchorRect,
} from "./components/AnchoredResultPopover";
import CenteredCommandInput from "./components/CenteredCommandInput";
import CenteredResultPanel, {
  type CenteredResult,
} from "./components/CenteredResultPanel";
import MainAppShell, {
  type MainResizeDirection,
} from "./components/MainAppShell";
import QuickAiPanel from "./components/QuickAiPanel";
import { TauriConversationRepository } from "./conversationRepository";
import { RepositoryConversationService } from "./conversationService";
import type { ConversationService } from "./conversationViewModel";
import {
  mapExplanationCard,
  type ExplanationResult,
} from "./explanationViewModel";
import {
  ExplanationRequestAuthority,
  isExplanationRequestCancelled,
} from "./explanationRequestAuthority";
import type {
  CaptureInput,
  ExplanationCard,
} from "./types/explanation";
import type {
  QuickAiConversation,
  RecentQuickAiConversation,
} from "./types/quickAi";
import { mainAppViewModel } from "./mainAppViewModel";
import { memoryPageViewModel } from "./memoryViewModel";
import { TauriMemoryRepository } from "./memoryRepository";
import {
  RepositoryMemoryService,
  type MemoryService,
} from "./memoryService";
import { TauriReviewRepository } from "./reviewRepository";
import {
  RepositoryReviewService,
  type ReviewService,
} from "./reviewService";
import { ReviewPreparationCoordinator } from "./reviewPreparationCoordinator";
import { ReviewBackgroundPreparationController } from "./reviewBackgroundPreparation";
import { ReviewQualityCoordinator } from "./reviewQualitySaveQueue";
import { TauriTodayRepository } from "./todayRepository";
import {
  RepositoryTodayService,
  type TodayService,
} from "./todayService";
import { TauriWritingRepository } from "./writingRepository";
import {
  RepositoryWritingService,
  type WritingService,
} from "./writingService";
import { TauriSettingsRepository } from "./settingsRepository";
import {
  RepositorySettingsService,
  type SettingsService,
} from "./settingsService";
import { useAppPreferences } from "./useAppPreferences";
import { TauriThemeRepository } from "./themeRepository";
import { RepositoryThemeService, type ThemeService } from "./themeService";
import { useAppTheme } from "./useAppTheme";
import {
  desktopSaveCoordinator,
  runForcedExit,
  runSafeExit,
} from "./desktopLifecycle";
import { markMainStartupReady } from "./startupBrand";
import "./App.css";
import "./styles/main-app.css";
import "./styles/conversation-page.css";
import "./styles/writing-page.css";
import "./styles/settings-page.css";
import "./styles/review-page.css";

type CheckState = "idle" | "running" | "ok" | "warn" | "error";

type CheckResult = {
  state: CheckState;
  detail: string;
};

type WindowState = {
  visible: boolean;
  alwaysOnTop: boolean;
};

type SafeExitRequest = { requestId: number };

type SafeExitFailure = {
  requestId: number;
  message?: string;
  retrying: boolean;
};

const MAIN_APP_DESIGN_WIDTH = 1440;
const MAIN_APP_DESIGN_HEIGHT = 900;
const MAIN_APP_PREVIEW_GUTTER = 48;

type TauriResizeDirection = Parameters<
  ReturnType<typeof getCurrentWindow>["startResizeDragging"]
>[0];

const MAIN_RESIZE_DIRECTION_MAP: Record<
  MainResizeDirection,
  TauriResizeDirection
> = {
  n: "North",
  ne: "NorthEast",
  e: "East",
  se: "SouthEast",
  s: "South",
  sw: "SouthWest",
  w: "West",
  nw: "NorthWest",
};

type DeepSeekSmokeResult = {
  configured: boolean;
  ok: boolean;
  model: string;
  status?: number;
  message: string;
  contentPreview?: string;
};

type PreviewMode = "anchored" | "command";
type CenteredMode = "explanation" | "quick-ai";
type QuickAiPage = "conversation" | "history";
type CommandStage = "input" | "loading" | "result";
type AnchoredStage = "mock" | "loading" | "result" | "error";
type OverlayWindowStage =
  | "input"
  | "loading"
  | "result"
  | "error"
  | "quick-ai";

type WindowsUiaCapture = {
  selectedText?: string | null;
  contextText?: string | null;
  minimalContext?: string | null;
  anchorRect?: AnchorRect | null;
  foreground?: {
    executablePath?: string | null;
    windowTitle?: string | null;
  };
};

type OverlayIntent =
  | { kind: "showInput"; capture?: null }
  | { kind: "uiaCapture"; capture?: WindowsUiaCapture | null };

const idle: CheckResult = { state: "idle", detail: "未验证" };

const mockExplanationResult: ExplanationResult = {
  kind: "word",
  sourceText: "marketed",
  learningTargetText: "marketed",
  headword: "market",
  phonetic: "/ˈmɑːrkɪtɪd/",
  partOfSpeech: "动词 market 的过去式 / 过去分词",
  basicMeanings: ["宣传；推广", "把……定位为"],
  contextMeaning: "被宣传为；被定位为",
  sourceSentence: "The course is marketed as beginner-friendly.",
  sourceSentenceZh: "这门课程被宣传为适合初学者。",
  phrases: [
    {
      phrase: "marketed as",
      meaning: "被宣传为；被定位为",
    },
    {
      phrase: "marketed to",
      meaning: "向……推广；面向……营销",
    },
  ],
  nearMeanings: [
    {
      term: "marketed",
      meaning: "强调宣传、推广、市场定位",
    },
    {
      term: "sold",
      meaning: "强调已经卖出或完成销售",
    },
    {
      term: "advertised",
      meaning: "强调投放广告，是 marketed 的一种方式",
    },
  ],
  examples: [
    {
      en: "The product is marketed as eco-friendly.",
      zh: "这个产品被宣传为环保的。",
    },
  ],
};

function normalizeUiaText(value?: string | null) {
  return value
    ?.replace(/[\u200B\uFFFC]/g, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function capturedContextForRecord(contextText?: string | null) {
  return contextText?.trim() ? contextText : null;
}

function sourceAppForCapture(capture: WindowsUiaCapture) {
  const executablePath = capture.foreground?.executablePath;
  if (executablePath) {
    const segments = executablePath.split(/[\\/]/);
    return segments[segments.length - 1] || null;
  }

  return null;
}

function notifyLearningRecordCreated() {
  void emit("readray://learning-record-created").catch((error) => {
    console.error("ReadRay 学习记录更新通知失败：", error);
  });
}

function notifyQuickAiConversationUpdated() {
  void emit("readray://quick-ai-conversation-updated").catch((error) => {
    console.error("ReadRay Quick AI 对话更新通知失败：", error);
  });
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function OverlayApp() {
  const isTauriRuntime = "__TAURI_INTERNALS__" in window;
  const [settingsService] = useState<SettingsService | null>(() =>
    isTauriRuntime
      ? new RepositorySettingsService(new TauriSettingsRepository())
      : null,
  );
  const { preferences } = useAppPreferences(settingsService);
  const previewAnchorRef = useRef<HTMLDivElement>(null);
  const [shortcutLabel, setShortcutLabel] = useState("Ctrl+Alt+R");
  const [windowCheck, setWindowCheck] = useState<CheckResult>(idle);
  const [clipboardCheck, setClipboardCheck] = useState<CheckResult>(idle);
  const [sqliteCheck, setSqliteCheck] = useState<CheckResult>(idle);
  const [deepseekCheck, setDeepseekCheck] = useState<CheckResult>(idle);
  const [alwaysOnTop, setAlwaysOnTop] = useState(false);
  const [previewMode, setPreviewMode] = useState<PreviewMode>("command");
  const [popoverOpen, setPopoverOpen] = useState(true);
  const [anchorRect, setAnchorRect] = useState<AnchorRect | null>(null);
  const [anchoredStage, setAnchoredStage] = useState<AnchoredStage>("mock");
  const [anchoredQuery, setAnchoredQuery] = useState("");
  const [anchoredResult, setAnchoredResult] =
    useState<ExplanationResult>(mockExplanationResult);
  const [anchoredError, setAnchoredError] = useState<string>();
  const [showDevControls, setShowDevControls] = useState(false);
  const [commandOpen, setCommandOpen] = useState(true);
  const [centeredMode, setCenteredMode] =
    useState<CenteredMode>("explanation");
  const [commandValue, setCommandValue] = useState("");
  const [commandStage, setCommandStage] = useState<CommandStage>("input");
  const [commandError, setCommandError] = useState<string | undefined>();
  const [centeredResult, setCenteredResult] =
    useState<CenteredResult>(mockExplanationResult);
  const [quickAiConversation, setQuickAiConversationState] =
    useState<QuickAiConversation | null>(null);
  const [quickAiPage, setQuickAiPage] =
    useState<QuickAiPage>("conversation");
  const [quickAiDraft, setQuickAiDraft] = useState("");
  const [quickAiPendingMessage, setQuickAiPendingMessage] = useState<string>();
  const [quickAiLoading, setQuickAiLoading] = useState(false);
  const [quickAiError, setQuickAiError] = useState<string>();
  const [quickAiConversationLoading, setQuickAiConversationLoading] =
    useState(false);
  const [quickAiRenaming, setQuickAiRenaming] = useState(false);
  const [quickAiRecentConversations, setQuickAiRecentConversations] = useState<
    RecentQuickAiConversation[]
  >([]);
  const [quickAiHistoryStatus, setQuickAiHistoryStatus] = useState<
    "idle" | "loading" | "ready" | "error"
  >("idle");
  const [quickAiHistoryError, setQuickAiHistoryError] = useState<string>();
  const [quickAiAllConversations, setQuickAiAllConversations] = useState<
    RecentQuickAiConversation[]
  >([]);
  const [quickAiAllHistoryStatus, setQuickAiAllHistoryStatus] = useState<
    "idle" | "loading" | "ready" | "error"
  >("idle");
  const [quickAiAllHistoryError, setQuickAiAllHistoryError] = useState<string>();
  const [explanationRequests] = useState(
    () =>
      new ExplanationRequestAuthority((requestScope, requestKey) => {
        void invoke("cancel_explanation_request", {
          requestScope,
          requestKey,
        }).catch(() => undefined);
      }),
  );
  const quickAiRequestId = useRef(0);
  const quickAiHistoryRequestId = useRef(0);
  const quickAiSelectionRequestId = useRef(0);
  const quickAiRenameRequestId = useRef(0);
  const quickAiConversationRef = useRef<QuickAiConversation | null>(null);
  const anchoredSourceRect = useRef<AnchorRect | null>(null);

  const setQuickAiConversation = useCallback(
    (conversation: QuickAiConversation | null) => {
      quickAiConversationRef.current = conversation;
      setQuickAiConversationState(conversation);
    },
    [],
  );

  const updatePreviewAnchorRect = useCallback(() => {
    const element = previewAnchorRef.current;
    if (!element) {
      return;
    }

    const rect = element.getBoundingClientRect();
    setAnchorRect({
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
    });
  }, []);

  useEffect(() => {
    invoke<string>("shortcut_label")
      .then(setShortcutLabel)
      .catch((error) => {
        setWindowCheck({ state: "warn", detail: formatError(error) });
      });

    invoke<WindowState>("stage1_status")
      .then((state) => setAlwaysOnTop(state.alwaysOnTop))
      .catch(() => undefined);

  }, []);

  useEffect(() => {
    updatePreviewAnchorRect();
    window.addEventListener("resize", updatePreviewAnchorRect);
    window.addEventListener("scroll", updatePreviewAnchorRect, true);

    return () => {
      window.removeEventListener("resize", updatePreviewAnchorRect);
      window.removeEventListener("scroll", updatePreviewAnchorRect, true);
    };
  }, [updatePreviewAnchorRect]);

  useEffect(() => {
    function handleDevControlsToggle(event: globalThis.KeyboardEvent) {
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "d") {
        event.preventDefault();
        setShowDevControls((visible) => !visible);
      }
    }

    window.addEventListener("keydown", handleDevControlsToggle);
    return () => window.removeEventListener("keydown", handleDevControlsToggle);
  }, []);

  const overlayWindowStage: OverlayWindowStage =
    centeredMode === "quick-ai"
      ? "quick-ai"
      : commandError
        ? "error"
        : commandStage;

  useEffect(() => {
    if (!commandOpen || previewMode !== "command") {
      return;
    }

    invoke("set_overlay_window_stage", { stage: overlayWindowStage }).catch(
      (error) => {
        setWindowCheck({ state: "warn", detail: formatError(error) });
      },
    );
  }, [commandOpen, overlayWindowStage, previewMode]);

  const clearCommandState = useCallback(() => {
    setCommandStage("input");
    setCommandError(undefined);
  }, []);

  const startNewQuickAiConversation = useCallback(() => {
    quickAiRequestId.current += 1;
    quickAiSelectionRequestId.current += 1;
    quickAiRenameRequestId.current += 1;
    setQuickAiPage("conversation");
    setQuickAiConversation(null);
    setQuickAiDraft("");
    setQuickAiPendingMessage(undefined);
    setQuickAiLoading(false);
    setQuickAiConversationLoading(false);
    setQuickAiRenaming(false);
    setQuickAiError(undefined);
  }, [setQuickAiConversation]);

  const showCommandOverlay = useCallback(() => {
    explanationRequests.invalidateAll();
    setPreviewMode("command");
    setCommandOpen(true);
  }, [explanationRequests]);

  const closeAnchoredOverlay = useCallback(async () => {
    explanationRequests.invalidate("anchored");
    setPopoverOpen(false);
    try {
      await invoke("hide_anchored_overlay_window");
    } catch (error) {
      setWindowCheck({ state: "warn", detail: formatError(error) });
    }
  }, [explanationRequests]);

  const runAnchoredQuery = useCallback(async (capture: WindowsUiaCapture) => {
    const selectedText = normalizeUiaText(capture.selectedText);
    const capturedAnchorRect = capture.anchorRect;
    if (!selectedText || !capturedAnchorRect) {
      return;
    }

    explanationRequests.invalidate("manual");
    const requestKey = explanationRequests.begin("anchored");
    anchoredSourceRect.current = capturedAnchorRect;
    setAnchoredQuery(selectedText);
    setAnchoredError(undefined);
    setAnchoredStage("loading");
    setPopoverOpen(true);
    setPreviewMode("anchored");

    await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));

    try {
      await invoke("present_anchored_overlay_window", {
        stage: "loading",
        anchorRect: capturedAnchorRect,
      });
    } catch (error) {
      if (explanationRequests.isCurrent("anchored", requestKey)) {
        explanationRequests.finish("anchored", requestKey);
        setAnchoredStage("error");
        setAnchoredError(`无法显示划词浮层：${formatError(error)}`);
      }
      return;
    }

    const input: CaptureInput = {
      queryText: selectedText,
      contextText: capturedContextForRecord(capture.contextText),
      sourceType: "windows_uia",
      sourceApp: sourceAppForCapture(capture),
    };

    try {
      const card = await invoke<ExplanationCard>("create_explanation_card", {
        input,
        requestKey,
        requestScope: "anchored",
        minimalContextText: capture.minimalContext ?? null,
      });
      if (!explanationRequests.isCurrent("anchored", requestKey)) {
        return;
      }
      notifyLearningRecordCreated();

      await invoke("present_anchored_overlay_window", {
        stage: "result",
        anchorRect: capturedAnchorRect,
      });
      if (!explanationRequests.isCurrent("anchored", requestKey)) {
        return;
      }
      setAnchoredResult(mapExplanationCard(card));
      setAnchoredStage("result");
      explanationRequests.finish("anchored", requestKey);
    } catch (error) {
      if (!explanationRequests.isCurrent("anchored", requestKey)) {
        return;
      }
      explanationRequests.finish("anchored", requestKey);
      if (isExplanationRequestCancelled(error)) {
        setPopoverOpen(false);
        return;
      }

      setAnchoredStage("error");
      setAnchoredError(formatError(error));
      await new Promise<void>((resolve) =>
        window.requestAnimationFrame(() => resolve()),
      );
      await invoke("present_anchored_overlay_window", {
        stage: "error",
        anchorRect: capturedAnchorRect,
      }).catch(() => undefined);
    }
  }, [explanationRequests]);

  const handleAnchoredContentSizeChange = useCallback(
    (size: { width: number; height: number }) => {
      const currentAnchorRect = anchoredSourceRect.current;
      if (!currentAnchorRect) {
        return;
      }

      invoke("resize_anchored_overlay_window", {
        width: size.width,
        height: size.height,
        anchorRect: currentAnchorRect,
      }).catch((error) => {
        setWindowCheck({ state: "warn", detail: formatError(error) });
      });
    },
    [],
  );

  const consumeOverlayIntent = useCallback(async () => {
    try {
      const intent = await invoke<OverlayIntent | null>("take_overlay_intent");
      if (!intent) {
        return;
      }

      if (intent.kind === "showInput") {
        showCommandOverlay();
        return;
      }

      if (intent.capture) {
        await runAnchoredQuery(intent.capture);
      }
    } catch (error) {
      setWindowCheck({ state: "warn", detail: formatError(error) });
    }
  }, [runAnchoredQuery, showCommandOverlay]);

  useEffect(() => {
    let unlistenOverlayIntent: (() => void) | undefined;
    let unlistenHidden: (() => void) | undefined;

    const handleOverlayIntent = () => {
      void consumeOverlayIntent();
    };

    listen("readray://overlay-intent", handleOverlayIntent)
      .then((dispose) => {
        unlistenOverlayIntent = dispose;
      })
      .catch((error) => {
        setWindowCheck({ state: "warn", detail: formatError(error) });
      });

    window.addEventListener("focus", handleOverlayIntent);
    void consumeOverlayIntent();

    listen("readray://hidden", () => {
      setCommandOpen(false);
      setPopoverOpen(false);
      explanationRequests.invalidateAll();
    })
      .then((dispose) => {
        unlistenHidden = dispose;
      })
      .catch((error) => {
        setWindowCheck({ state: "warn", detail: formatError(error) });
      });

    return () => {
      window.removeEventListener("focus", handleOverlayIntent);
      unlistenOverlayIntent?.();
      unlistenHidden?.();
    };
  }, [consumeOverlayIntent, explanationRequests]);

  useEffect(
    () => () => {
      explanationRequests.invalidateAll();
    },
    [explanationRequests],
  );

  function showPreviewPopover() {
    explanationRequests.invalidateAll();
    clearCommandState();
    setPreviewMode("anchored");
    setAnchoredStage("mock");
    setAnchoredResult(mockExplanationResult);
    setAnchoredError(undefined);
    updatePreviewAnchorRect();
    setPopoverOpen(true);
  }

  function showCommandInputPreview() {
    showCommandOverlay();
  }

  function handleAnchoredOpenChange(nextOpen: boolean) {
    if (anchoredStage === "mock") {
      setPopoverOpen(nextOpen);
      return;
    }

    if (!nextOpen) {
      void closeAnchoredOverlay();
    }
  }

  useEffect(() => {
    if (
      previewMode !== "anchored" ||
      anchoredStage === "mock" ||
      anchoredStage === "result"
    ) {
      return;
    }

    function handleAnchoredEscape(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        void closeAnchoredOverlay();
      }
    }

    document.addEventListener("keydown", handleAnchoredEscape);
    return () => document.removeEventListener("keydown", handleAnchoredEscape);
  }, [anchoredStage, closeAnchoredOverlay, previewMode]);

  async function handleCommandOpenChange(nextOpen: boolean) {
    setCommandOpen(nextOpen);
    if (!nextOpen) {
      explanationRequests.invalidate("manual");
      try {
        await invoke("hide_overlay_window");
      } catch (error) {
        setWindowCheck({ state: "warn", detail: formatError(error) });
      }
    }
  }

  function handleCommandValueChange(nextValue: string) {
    explanationRequests.invalidate("manual");
    setCommandValue(nextValue);
    setCommandStage("input");
    setCommandError(undefined);
  }

  async function submitCommand(value: string) {
    if (commandStage === "loading") {
      return;
    }

    const requestKey = explanationRequests.begin("manual");
    setCommandValue(value);
    setCommandStage("loading");
    setCommandError(undefined);

    const input: CaptureInput = {
      queryText: value,
      contextText: null,
      sourceType: "manual",
    };

    try {
      const card = await invoke<ExplanationCard>("create_explanation_card", {
        input,
        requestKey,
        requestScope: "manual",
        minimalContextText: null,
      });
      if (!explanationRequests.isCurrent("manual", requestKey)) {
        return;
      }
      notifyLearningRecordCreated();
      setCenteredResult(mapExplanationCard(card));
      setCommandStage("result");
      explanationRequests.finish("manual", requestKey);
    } catch (error) {
      if (!explanationRequests.isCurrent("manual", requestKey)) {
        return;
      }
      explanationRequests.finish("manual", requestKey);
      setCommandStage("input");
      setCommandError(
        isExplanationRequestCancelled(error) ? undefined : formatError(error),
      );
    }
  }

  async function sendQuickAiMessage(
    value: string,
    conversation: QuickAiConversation | null = quickAiConversationRef.current,
  ) {
    const content = value.trim();
    if (!content || quickAiLoading || quickAiConversationLoading) {
      return;
    }

    const requestId = quickAiRequestId.current + 1;
    quickAiRequestId.current = requestId;
    setQuickAiDraft("");
    setQuickAiPendingMessage(content);
    setQuickAiLoading(true);
    setQuickAiError(undefined);

    try {
      let targetConversation = conversation;
      if (!targetConversation) {
        targetConversation = await invoke<QuickAiConversation>(
          "create_quick_ai_conversation",
          { origin: "overlay" },
        );
        if (quickAiRequestId.current === requestId) {
          setQuickAiConversation(targetConversation);
        }
      }

      const lastMessage =
        targetConversation.messages[targetConversation.messages.length - 1];
      const expectedUserSequence =
        lastMessage?.role === "user" &&
        lastMessage.content.trim() === content
          ? lastMessage.sequence
          : (lastMessage?.sequence ?? 0) +
            (lastMessage?.role === "user" ? 2 : 1);
      // Agent 链路（与主应用对话一致）：overlay 也通过 ChatSurfaceAdapter 走
      // send_quick_ai_message_agent，使联网能力对 overlay 可用。overlay 保持
      // 非流式 UI，Channel 只用于满足命令签名，增量事件被丢弃，最终以返回的
      // 权威快照对齐。
      const channel = new Channel(() => undefined);
      const updatedConversation = await invoke<QuickAiConversation>(
        "send_quick_ai_message_agent",
        {
          conversationId: targetConversation.id,
          expectedUserSequence,
          content,
          channel,
        },
      );
      notifyQuickAiConversationUpdated();
      if (
        quickAiRequestId.current !== requestId ||
        quickAiConversationRef.current?.id !== targetConversation.id
      ) {
        return;
      }
      setQuickAiConversation(updatedConversation);
      setQuickAiPendingMessage(undefined);
      setQuickAiLoading(false);
    } catch (error) {
      notifyQuickAiConversationUpdated();
      if (
        quickAiRequestId.current !== requestId ||
        (conversation !== null &&
          quickAiConversationRef.current?.id !== conversation.id)
      ) {
        return;
      }
      setQuickAiDraft(content);
      setQuickAiPendingMessage(undefined);
      setQuickAiLoading(false);
      setQuickAiError(formatError(error));
    }
  }

  async function enterQuickAi(initialValue: string) {
    const initialMessage = initialValue.trim();
    explanationRequests.invalidateAll();
    setCenteredMode("quick-ai");
    setQuickAiPage("conversation");
    setCommandValue("");
    setCommandStage("input");
    setCommandError(undefined);

    if (!initialMessage) {
      return;
    }
    if (quickAiLoading || quickAiConversationLoading) {
      setQuickAiDraft(initialMessage);
      return;
    }
    await sendQuickAiMessage(initialMessage);
  }

  function returnToCommandInput() {
    setCenteredMode("explanation");
    setCommandStage("input");
    setCommandError(undefined);
    setCommandOpen(true);
  }

  async function loadQuickAiHistory() {
    const requestId = quickAiHistoryRequestId.current + 1;
    quickAiHistoryRequestId.current = requestId;
    setQuickAiHistoryStatus("loading");
    setQuickAiHistoryError(undefined);
    try {
      const conversations = await invoke<RecentQuickAiConversation[]>(
        "list_recent_quick_ai_conversations",
        { limit: 8, origin: "overlay" },
      );
      if (quickAiHistoryRequestId.current !== requestId) {
        return;
      }
      setQuickAiRecentConversations(conversations);
      setQuickAiHistoryStatus("ready");
    } catch (error) {
      if (quickAiHistoryRequestId.current !== requestId) {
        return;
      }
      setQuickAiHistoryStatus("error");
      setQuickAiHistoryError(formatError(error));
    }
  }

  async function selectQuickAiConversation(conversationId: number) {
    if (quickAiConversationRef.current?.id === conversationId) {
      setQuickAiPage("conversation");
      return;
    }

    const selectionRequestId = quickAiSelectionRequestId.current + 1;
    quickAiSelectionRequestId.current = selectionRequestId;
    quickAiRenameRequestId.current += 1;
    setQuickAiRenaming(false);
    setQuickAiConversationLoading(true);
    setQuickAiError(undefined);
    try {
      const conversation = await invoke<QuickAiConversation | null>(
        "get_quick_ai_conversation",
        { conversationId },
      );
      if (quickAiSelectionRequestId.current !== selectionRequestId) {
        return;
      }
      if (!conversation) {
        throw new Error("所选 Quick AI 对话不存在或已被删除。");
      }
      if (conversation.origin !== "overlay") {
        throw new Error("所选会话不属于 Overlay Quick AI 历史。");
      }

      quickAiRequestId.current += 1;
      setQuickAiConversation(conversation);
      setQuickAiPage("conversation");
      setQuickAiDraft("");
      setQuickAiPendingMessage(undefined);
      setQuickAiLoading(false);
    } catch (error) {
      if (quickAiSelectionRequestId.current !== selectionRequestId) {
        return;
      }
      setQuickAiError(formatError(error));
    } finally {
      if (quickAiSelectionRequestId.current === selectionRequestId) {
        setQuickAiConversationLoading(false);
      }
    }
  }

  async function renameQuickAiConversation(title: string) {
    const conversation = quickAiConversationRef.current;
    const normalizedTitle = title.trim();
    if (!conversation?.title || !normalizedTitle) {
      setQuickAiError("会话名称不能为空。");
      return false;
    }
    if (normalizedTitle === conversation.title.trim()) {
      return true;
    }

    const requestId = quickAiRenameRequestId.current + 1;
    quickAiRenameRequestId.current = requestId;
    const conversationId = conversation.id;
    setQuickAiRenaming(true);
    setQuickAiError(undefined);
    try {
      const renamed = await invoke<QuickAiConversation>(
        "rename_quick_ai_conversation",
        { conversationId, title: normalizedTitle },
      );
      if (
        quickAiRenameRequestId.current !== requestId ||
        quickAiConversationRef.current?.id !== conversationId
      ) {
        return false;
      }
      if (renamed.origin !== "overlay") {
        throw new Error("重命名结果不属于当前 Overlay 会话。");
      }

      setQuickAiConversation(renamed);
      setQuickAiRecentConversations((items) =>
        items.map((item) =>
          item.id === conversationId
            ? { ...item, title: renamed.title || normalizedTitle }
            : item,
        ),
      );
      setQuickAiAllConversations((items) =>
        items.map((item) =>
          item.id === conversationId
            ? { ...item, title: renamed.title || normalizedTitle }
            : item,
        ),
      );
      notifyQuickAiConversationUpdated();
      return true;
    } catch (error) {
      if (
        quickAiRenameRequestId.current === requestId &&
        quickAiConversationRef.current?.id === conversationId
      ) {
        setQuickAiError(formatError(error));
      }
      return false;
    } finally {
      if (quickAiRenameRequestId.current === requestId) {
        setQuickAiRenaming(false);
      }
    }
  }

  async function loadAllQuickAiConversations() {
    const requestId = quickAiHistoryRequestId.current + 1;
    quickAiHistoryRequestId.current = requestId;
    setQuickAiAllHistoryStatus("loading");
    setQuickAiAllHistoryError(undefined);
    try {
      const conversations = await invoke<RecentQuickAiConversation[]>(
        "list_all_quick_ai_conversations",
        { origin: "overlay" },
      );
      if (quickAiHistoryRequestId.current !== requestId) {
        return;
      }
      setQuickAiAllConversations(conversations);
      setQuickAiAllHistoryStatus("ready");
    } catch (error) {
      if (quickAiHistoryRequestId.current !== requestId) {
        return;
      }
      setQuickAiAllHistoryStatus("error");
      setQuickAiAllHistoryError(formatError(error));
    }
  }

  function openAllQuickAiConversations() {
    setQuickAiPage("history");
    void loadAllQuickAiConversations();
  }

  async function toggleWindow() {
    setWindowCheck({ state: "running", detail: "正在切换窗口" });
    try {
      const visible = await invoke<boolean>("toggle_overlay_window");
      setWindowCheck({
        state: "ok",
        detail: visible ? "窗口已显示并聚焦" : "窗口已隐藏",
      });
    } catch (error) {
      setWindowCheck({ state: "error", detail: formatError(error) });
    }
  }

  async function toggleAlwaysOnTop() {
    const nextValue = !alwaysOnTop;
    setWindowCheck({
      state: "running",
      detail: nextValue ? "正在置顶窗口" : "正在取消置顶",
    });

    try {
      const state = await invoke<WindowState>("set_overlay_window_always_on_top", {
        enabled: nextValue,
      });
      setAlwaysOnTop(state.alwaysOnTop);
      setWindowCheck({
        state: "ok",
        detail: state.alwaysOnTop ? "窗口已置顶" : "窗口已取消置顶",
      });
    } catch (error) {
      setWindowCheck({ state: "error", detail: formatError(error) });
    }
  }

  async function testClipboard() {
    setClipboardCheck({ state: "running", detail: "正在读写剪贴板" });
    const text = `ReadRay clipboard check ${new Date().toISOString()}`;

    try {
      await writeText(text);
      const content = await readText();
      setClipboardCheck({
        state: content === text ? "ok" : "warn",
        detail:
          content === text
            ? `读取成功：${content}`
            : `读取结果与写入值不一致：${content}`,
      });
    } catch (error) {
      setClipboardCheck({ state: "error", detail: formatError(error) });
    }
  }

  async function testSqlite() {
    setSqliteCheck({ state: "running", detail: "正在检查学习记录 SQLite" });

    try {
      const result = await invoke<{ total: number }>("list_learning_records", {
        page: 1,
        pageSize: 1,
      });

      setSqliteCheck({
        state: "ok",
        detail: `学习记录数据库可读取，当前共 ${result.total} 条记录`,
      });
    } catch (error) {
      setSqliteCheck({ state: "error", detail: formatError(error) });
    }
  }

  async function testDeepSeek() {
    setDeepseekCheck({ state: "running", detail: "正在检查 DeepSeek" });

    try {
      const result = await invoke<DeepSeekSmokeResult>("deepseek_smoke_test", {
        prompt: "Reply with: ReadRay API smoke test passed.",
      });

      if (!result.configured) {
        setDeepseekCheck({ state: "warn", detail: result.message });
        return;
      }

      setDeepseekCheck({
        state: result.ok ? "ok" : "error",
        detail: [
          result.message,
          `model=${result.model}`,
          result.status ? `status=${result.status}` : "",
          result.contentPreview ?? "",
        ]
          .filter(Boolean)
          .join(" | "),
      });
    } catch (error) {
      setDeepseekCheck({ state: "error", detail: formatError(error) });
    }
  }

  return (
    <main
      className="app-shell"
      onContextMenu={(event) => event.preventDefault()}
    >
      <section className="compact-preview" aria-label="ReadRay 桌面浮层">
        <QuickAiPanel
          open={
            previewMode === "command" &&
            centeredMode === "quick-ai" &&
            commandOpen
          }
          conversation={quickAiConversation}
          page={quickAiPage}
          recentConversations={quickAiRecentConversations}
          historyStatus={quickAiHistoryStatus}
          historyError={quickAiHistoryError}
          allConversations={quickAiAllConversations}
          allHistoryStatus={quickAiAllHistoryStatus}
          allHistoryError={quickAiAllHistoryError}
          conversationLoading={quickAiConversationLoading}
          renaming={quickAiRenaming}
          draft={quickAiDraft}
          pendingMessage={quickAiPendingMessage}
          loading={quickAiLoading || quickAiConversationLoading}
          error={quickAiError}
          onDraftChange={setQuickAiDraft}
          onSend={(value) => void sendQuickAiMessage(value)}
          onNewConversation={startNewQuickAiConversation}
          onHistoryRequest={() => void loadQuickAiHistory()}
          onConversationSelect={(conversationId) =>
            void selectQuickAiConversation(conversationId)
          }
          onRename={renameQuickAiConversation}
          onViewAllConversations={openAllQuickAiConversations}
          onAllHistoryRetry={() => void loadAllQuickAiConversations()}
          onHistoryBack={() => setQuickAiPage("conversation")}
          onBack={returnToCommandInput}
          sendShortcut={preferences.sendShortcut}
        />
        {previewMode === "anchored" ? (
          anchoredStage === "mock" ? (
            <>
              <div className="mock-reader-line" aria-hidden="true" />
              <div className="compact-preview__anchor" ref={previewAnchorRef}>
                <span>marketed</span>
              </div>
              <AnchoredResultPopover
                result={anchoredResult}
                anchorRect={anchorRect}
                open={popoverOpen}
                onOpenChange={handleAnchoredOpenChange}
                highlightText="marketed"
              />
            </>
          ) : anchoredStage === "result" ? (
            <AnchoredResultPopover
              result={anchoredResult}
              anchorRect={null}
              open={popoverOpen}
              onOpenChange={handleAnchoredOpenChange}
              embedded
              highlightText={anchoredQuery}
              onContentSizeChange={handleAnchoredContentSizeChange}
            />
          ) : (
            <section
              className={`anchored-query-status is-${anchoredStage}`}
              aria-live="polite"
              aria-busy={anchoredStage === "loading"}
            >
              <div className="anchored-query-status__header">
                <span className="anchored-query-status__query">
                  {anchoredQuery}
                </span>
                {anchoredStage === "loading" ? (
                  <span
                    className="anchored-query-status__loading-dot"
                    aria-hidden="true"
                  />
                ) : null}
              </div>
              <p className="anchored-query-status__message">
                {anchoredStage === "loading"
                  ? "正在生成语境解释…"
                  : anchoredError ?? "解释失败，请稍后重试。"}
              </p>
            </section>
          )
        ) : (
          <>
            <CenteredCommandInput
              value={commandValue}
              onValueChange={handleCommandValueChange}
              open={
                centeredMode === "explanation" &&
                commandOpen &&
                commandStage !== "result"
              }
              loading={commandStage === "loading"}
              error={commandError}
              onSubmit={submitCommand}
              onQuickAi={(value) => void enterQuickAi(value)}
              onOpenChange={handleCommandOpenChange}
            />
            <CenteredResultPanel
              query={commandValue}
              result={centeredResult}
              open={
                centeredMode === "explanation" &&
                commandOpen &&
                commandStage === "result"
              }
              onQueryChange={handleCommandValueChange}
              onSubmit={submitCommand}
            />
          </>
        )}
      </section>

      {showDevControls ? (
      <div className="preview-controls" aria-label="预览辅助控制">
        <div className="preview-controls__modes" aria-label="预览模式">
          <button
            className={previewMode === "anchored" ? "is-active" : ""}
            type="button"
            onClick={showPreviewPopover}
          >
            划词
          </button>
          <button
            className={previewMode === "command" ? "is-active" : ""}
            type="button"
            onClick={showCommandInputPreview}
          >
            无选区
          </button>
        </div>
        <button
          className="compact-preview__show"
          type="button"
          onClick={
            previewMode === "anchored" ? showPreviewPopover : showCommandInputPreview
          }
        >
          重新显示
        </button>

        <details className="stage-diagnostics">
          <summary>
            <span>开发验证</span>
            <span className="shortcut">{shortcutLabel}</span>
          </summary>

          <section className="check-grid">
            <article className="check">
              <div>
                <h3>窗口</h3>
                <p className={`status ${windowCheck.state}`}>
                  {windowCheck.detail}
                </p>
              </div>
              <div className="actions">
                <button type="button" onClick={toggleWindow}>
                  显示 / 隐藏
                </button>
                <button type="button" onClick={toggleAlwaysOnTop}>
                  {alwaysOnTop ? "取消置顶" : "置顶"}
                </button>
              </div>
            </article>

            <article className="check">
              <div>
                <h3>剪贴板</h3>
                <p className={`status ${clipboardCheck.state}`}>
                  {clipboardCheck.detail}
                </p>
              </div>
              <button type="button" onClick={testClipboard}>
                读写验证
              </button>
            </article>

            <article className="check">
              <div>
                <h3>SQLite</h3>
                <p className={`status ${sqliteCheck.state}`}>
                  {sqliteCheck.detail}
                </p>
              </div>
              <button type="button" onClick={testSqlite}>
                读写验证
              </button>
            </article>

            <article className="check">
              <div>
                <h3>DeepSeek</h3>
                <p className={`status ${deepseekCheck.state}`}>
                  {deepseekCheck.detail}
                </p>
              </div>
              <button type="button" onClick={testDeepSeek}>
                API 验证
              </button>
            </article>
          </section>
        </details>
      </div>
      ) : null}
    </main>
  );
}

function App() {
  const view = new URLSearchParams(window.location.search).get("view");

  if (view === "main") {
    return <MainAppWindow />;
  }

  return <OverlayApp />;
}

function MainAppWindow() {
  const [isMaximized, setIsMaximized] = useState(false);
  const maximizedStateRequestRef = useRef(0);
  const maximizedTogglePendingRef = useRef(false);
  const [previewScale, setPreviewScale] = useState(1);
  const isTauriRuntime = "__TAURI_INTERNALS__" in window;
  const isResponsivePreview =
    !isTauriRuntime &&
    new URLSearchParams(window.location.search).get("preview") === "responsive";

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      markMainStartupReady();
    });
    return () => window.cancelAnimationFrame(frame);
  }, []);
  const [memoryService, setMemoryService] = useState<MemoryService | null>(() =>
    isTauriRuntime
      ? new RepositoryMemoryService(new TauriMemoryRepository())
      : null,
  );
  const [todayService, setTodayService] = useState<TodayService | null>(() =>
    isTauriRuntime
      ? new RepositoryTodayService(new TauriTodayRepository())
      : null,
  );
  const [reviewService, setReviewService] = useState<ReviewService | null>(() =>
    isTauriRuntime
      ? new RepositoryReviewService(new TauriReviewRepository())
      : null,
  );
  const reviewPreparationCoordinator = useMemo(
    () =>
      reviewService ? new ReviewPreparationCoordinator(reviewService) : null,
    [reviewService],
  );
  const reviewQualityCoordinator = useMemo(
    () =>
      reviewService
        ? new ReviewQualityCoordinator(reviewService, {
            recordMutation: () => desktopSaveCoordinator.recordMutation(),
          })
        : null,
    [reviewService],
  );
  const reviewBackgroundPreparation = useMemo(
    () =>
      reviewService && reviewPreparationCoordinator
        ? new ReviewBackgroundPreparationController(
            reviewService,
            reviewPreparationCoordinator,
          )
        : null,
    [reviewPreparationCoordinator, reviewService],
  );
  const [memoryRefreshToken, setMemoryRefreshToken] = useState(0);
  const [learningRecordsRefreshToken, setLearningRecordsRefreshToken] =
    useState(0);

  useEffect(() => {
    if (!isTauriRuntime || !reviewBackgroundPreparation) return;
    void reviewBackgroundPreparation.warmFirstPage().catch((error) => {
      console.error("ReadRay 复习卡片后台预热失败：", error);
    });
    return () => reviewBackgroundPreparation.invalidate();
  }, [
    isTauriRuntime,
    learningRecordsRefreshToken,
    reviewBackgroundPreparation,
  ]);

  useEffect(() => {
    if (!reviewQualityCoordinator) return;
    return desktopSaveCoordinator.register({
      label: "复习卡片质量反馈",
      flush: () => reviewQualityCoordinator.flush(),
    });
  }, [reviewQualityCoordinator]);
  const [conversationRefreshToken, setConversationRefreshToken] = useState(0);
  const [conversationService, setConversationService] =
    useState<ConversationService | null>(() =>
      isTauriRuntime
        ? new RepositoryConversationService(
            new TauriConversationRepository(),
            {
              onConversationUpdated: () =>
                setConversationRefreshToken((token) => token + 1),
            },
          )
        : null,
  );
  const [writingService, setWritingService] =
    useState<WritingService | null>(() =>
      isTauriRuntime
        ? new RepositoryWritingService(new TauriWritingRepository())
        : null,
    );
  const [settingsService] = useState<SettingsService | null>(() =>
    isTauriRuntime
      ? new RepositorySettingsService(new TauriSettingsRepository())
      : null,
  );
  const [themeService] = useState<ThemeService | null>(() =>
    isTauriRuntime
      ? new RepositoryThemeService(new TauriThemeRepository())
      : null,
  );
  const { preferences, savePreferences } = useAppPreferences(settingsService);
  const themeController = useAppTheme(themeService);
  const [safeExitFailure, setSafeExitFailure] = useState<SafeExitFailure>();
  const safeExitGenerationRef = useRef(0);
  const handledSafeExitRequestRef = useRef<number | undefined>(undefined);

  const performSafeExit = useCallback(async (requestId: number) => {
    const generation = safeExitGenerationRef.current + 1;
    safeExitGenerationRef.current = generation;
    try {
      desktopSaveCoordinator.beginExit(requestId);
    } catch (error) {
      setSafeExitFailure({
        requestId,
        message: formatError(error),
        retrying: false,
      });
      return;
    }
    setSafeExitFailure({ requestId, retrying: true });
    const outcome = await runSafeExit(requestId, {
      flush: () => desktopSaveCoordinator.flushAll(),
      complete: (currentRequestId) =>
        invoke<void>("complete_app_exit", { requestId: currentRequestId }),
      isCurrent: () => safeExitGenerationRef.current === generation,
    });
    if (outcome.status === "failed") {
      desktopSaveCoordinator.endExit(requestId);
      await invoke<void>("restore_main_window").catch((error) => {
        console.error("ReadRay 退出失败后恢复主窗口失败：", error);
      });
      if (safeExitGenerationRef.current === generation) {
        setSafeExitFailure({
          requestId,
          message: outcome.message,
          retrying: false,
        });
      }
    } else if (outcome.status === "stale") {
      desktopSaveCoordinator.endExit(requestId);
    }
  }, []);

  const handleSafeExitRequest = useCallback((requestId: number) => {
    if (handledSafeExitRequestRef.current === requestId) return;
    handledSafeExitRequestRef.current = requestId;
    void performSafeExit(requestId);
  }, [performSafeExit]);

  useEffect(() => {
    if (!isTauriRuntime) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<SafeExitRequest>("readray://safe-exit-requested", (event) => {
      if (!disposed) handleSafeExitRequest(event.payload.requestId);
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
      } else {
        unlisten = cleanup;
        void invoke<number | null>("get_pending_app_exit_request").then(
          (requestId) => {
            if (!disposed && requestId !== null) handleSafeExitRequest(requestId);
          },
          (error) => console.error("ReadRay 待处理退出请求读取失败：", error),
        );
      }
    }).catch((error) => {
      console.error("ReadRay 安全退出监听失败：", error);
    });
    return () => {
      disposed = true;
      safeExitGenerationRef.current += 1;
      unlisten?.();
    };
  }, [handleSafeExitRequest, isTauriRuntime]);

  useEffect(() => {
    if (!isTauriRuntime) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("readray://learning-record-created", () => {
      setMemoryRefreshToken((token) => token + 1);
      setLearningRecordsRefreshToken((token) => token + 1);
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
      } else {
        unlisten = cleanup;
      }
    }).catch((error) => {
      console.error("ReadRay 学习记录更新监听失败：", error);
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isTauriRuntime]);

  useEffect(() => {
    if (!isTauriRuntime) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("readray://quick-ai-conversation-updated", () => {
      setConversationRefreshToken((token) => token + 1);
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
      } else {
        unlisten = cleanup;
      }
    }).catch((error) => {
      console.error("ReadRay Quick AI 对话更新监听失败：", error);
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isTauriRuntime]);

  useEffect(() => {
    if (isTauriRuntime) {
      return;
    }

    let ignore = false;
    void import("./memoryPreviewService").then(
      ({ createBrowserPreviewMemoryService }) => {
        if (!ignore) {
          setMemoryService(createBrowserPreviewMemoryService());
        }
      },
    );
    void import("./todayPreviewService").then(
      ({ createBrowserPreviewTodayService }) => {
        if (!ignore) {
          setTodayService(createBrowserPreviewTodayService());
        }
      },
    );
    void import("./reviewFixtureService").then(
      ({ createBrowserPreviewReviewService }) => {
        if (!ignore) {
          setReviewService(createBrowserPreviewReviewService());
        }
      },
    );
    void import("./conversationFixtureService").then(
      ({ FixtureConversationService }) => {
        if (ignore) {
          return;
        }
        const requestedFailure = new URLSearchParams(
          window.location.search,
        ).get("conversationFailure");
        const failOnce = ["create", "load", "generate", "export"].includes(
          requestedFailure ?? "",
        )
          ? (requestedFailure as "create" | "load" | "generate" | "export")
          : undefined;
        const failureCount =
          import.meta.env.DEV &&
          (failOnce === "create" || failOnce === "load")
            ? 2
            : 1;
        setConversationService(
          new FixtureConversationService({ failOnce, failureCount }),
        );
      },
    );
    void import("./writingFixtureService").then(
      ({ createBrowserPreviewWritingService }) => {
        if (!ignore) {
          setWritingService(createBrowserPreviewWritingService());
        }
      },
    );
    return () => {
      ignore = true;
    };
  }, [isTauriRuntime]);

  useEffect(() => {
    if (isTauriRuntime || isResponsivePreview) {
      return;
    }

    function syncPreviewScale() {
      const availableWidth = Math.max(
        window.innerWidth - MAIN_APP_PREVIEW_GUTTER,
        1,
      );
      const availableHeight = Math.max(
        window.innerHeight - MAIN_APP_PREVIEW_GUTTER,
        1,
      );
      setPreviewScale(
        Math.min(
          availableWidth / MAIN_APP_DESIGN_WIDTH,
          availableHeight / MAIN_APP_DESIGN_HEIGHT,
          1,
        ),
      );
    }

    syncPreviewScale();
    window.addEventListener("resize", syncPreviewScale);
    return () => window.removeEventListener("resize", syncPreviewScale);
  }, [isResponsivePreview, isTauriRuntime]);

  useEffect(() => {
    if (!isTauriRuntime) {
      return;
    }

    let resizeTimer: number | undefined;

    function syncMaximizedState() {
      if (maximizedTogglePendingRef.current) {
        scheduleMaximizedStateSync();
        return;
      }

      const requestId = ++maximizedStateRequestRef.current;
      invoke<boolean>("main_window_is_maximized")
        .then((nextState) => {
          if (requestId === maximizedStateRequestRef.current) {
            setIsMaximized(nextState);
          }
        })
        .catch((error) => console.error("ReadRay 主窗口状态读取失败：", error));
    }

    function scheduleMaximizedStateSync() {
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(syncMaximizedState, 120);
    }

    syncMaximizedState();
    window.addEventListener("resize", scheduleMaximizedStateSync);
    return () => {
      window.removeEventListener("resize", scheduleMaximizedStateSync);
      window.clearTimeout(resizeTimer);
      maximizedStateRequestRef.current += 1;
    };
  }, [isTauriRuntime]);

  const runMainWindowCommand = useCallback(
    async <T,>(command: string): Promise<T | undefined> => {
      if (!isTauriRuntime) {
        return undefined;
      }

      try {
        return await invoke<T>(command);
      } catch (error) {
        console.error(`ReadRay 主窗口命令失败（${command}）：`, error);
        return undefined;
      }
    },
    [isTauriRuntime],
  );

  const toggleMaximized = useCallback(async () => {
    if (maximizedTogglePendingRef.current) {
      return;
    }

    maximizedTogglePendingRef.current = true;
    const requestId = ++maximizedStateRequestRef.current;
    try {
      const nextState = await runMainWindowCommand<boolean>(
        "toggle_main_window_maximized",
      );
      if (
        typeof nextState === "boolean" &&
        requestId === maximizedStateRequestRef.current
      ) {
        setIsMaximized(nextState);
      }
    } finally {
      maximizedTogglePendingRef.current = false;
    }
  }, [runMainWindowCommand]);

  const forceExit = useCallback(async () => {
    const failure = safeExitFailure;
    if (!failure) return;
    const confirmed = window.confirm(
      "仍然退出 ReadRay？尚未落盘的设置、写作或复习反馈可能丢失。",
    );
    if (!confirmed) return;
    safeExitGenerationRef.current += 1;
    setSafeExitFailure({ ...failure, retrying: true });
    try {
      desktopSaveCoordinator.beginExit(failure.requestId);
      await runForcedExit(failure.requestId, true, (requestId) =>
        invoke<void>("force_app_exit", { requestId }),
      );
    } catch (error) {
      desktopSaveCoordinator.endExit(failure.requestId);
      setSafeExitFailure({
        ...failure,
        retrying: false,
        message: `仍然退出失败：${formatError(error)}`,
      });
    }
  }, [safeExitFailure]);

  const cancelExit = useCallback(async () => {
    const failure = safeExitFailure;
    if (!failure) return;
    safeExitGenerationRef.current += 1;
    setSafeExitFailure({ ...failure, retrying: true });
    try {
      desktopSaveCoordinator.beginExit(failure.requestId);
      await invoke<void>("cancel_app_exit", { requestId: failure.requestId });
      desktopSaveCoordinator.endExit(failure.requestId);
      handledSafeExitRequestRef.current = undefined;
      setSafeExitFailure(undefined);
    } catch (error) {
      desktopSaveCoordinator.endExit(failure.requestId);
      let pendingRequestId: number | null | undefined;
      try {
        pendingRequestId = await invoke<number | null>(
          "get_pending_app_exit_request",
        );
      } catch (readError) {
        console.error("ReadRay 取消退出失败后读取 pending 状态失败：", readError);
      }
      if (pendingRequestId !== undefined && pendingRequestId !== failure.requestId) {
        desktopSaveCoordinator.endExit(failure.requestId);
        handledSafeExitRequestRef.current = undefined;
        setSafeExitFailure(undefined);
        return;
      }
      setSafeExitFailure({
        ...failure,
        retrying: false,
        message: `取消退出失败：${formatError(error)}`,
      });
    }
  }, [safeExitFailure]);

  const mainApp = (
    <>
      <MainAppShell
      viewModel={mainAppViewModel}
      memoryViewModel={memoryPageViewModel}
      memoryService={memoryService}
      memoryRefreshToken={memoryRefreshToken}
      reviewService={reviewService}
      reviewPreparationCoordinator={reviewPreparationCoordinator}
      reviewQualityCoordinator={reviewQualityCoordinator}
      reviewRefreshToken={learningRecordsRefreshToken}
      todayService={todayService}
      learningRecordsRefreshToken={learningRecordsRefreshToken}
      conversationRefreshToken={conversationRefreshToken}
      conversationService={conversationService}
      writingService={writingService}
      settingsService={settingsService}
      themeController={themeController}
      preferences={preferences}
      onPreferencesSave={savePreferences}
      interactionBlocked={safeExitFailure?.retrying === true}
      isMaximized={isMaximized}
      onStartDragging={() => {
        void runMainWindowCommand("start_main_window_drag");
      }}
      onStartResize={(direction) => {
        if (!isTauriRuntime) return;
        void getCurrentWindow()
          .startResizeDragging(MAIN_RESIZE_DIRECTION_MAP[direction])
          .catch((error) => {
            console.error("ReadRay 主窗口缩放失败：", error);
          });
      }}
      onMinimize={() => {
        void runMainWindowCommand("minimize_main_window");
      }}
      onToggleMaximize={toggleMaximized}
      onClose={() => {
        void runMainWindowCommand("apply_main_window_close_behavior");
      }}
      />
      {safeExitFailure ? (
        <div className="rr-safe-exit-backdrop" role="presentation">
          <section
            className="rr-safe-exit-dialog"
            role={safeExitFailure.message ? "alertdialog" : "dialog"}
            aria-modal="true"
            aria-labelledby="rr-safe-exit-title"
            aria-describedby="rr-safe-exit-message"
          >
            <h2 id="rr-safe-exit-title">
              {safeExitFailure.message
                ? "保存失败，ReadRay 尚未退出"
                : "正在保存并退出"}
            </h2>
            <p id="rr-safe-exit-message">
              {safeExitFailure.message ?? "正在等待设置操作和写作草稿安全落盘…"}
            </p>
            {safeExitFailure.message ? <div className="rr-safe-exit-actions">
              <button
                type="button"
                disabled={safeExitFailure.retrying}
                onClick={() => void performSafeExit(safeExitFailure.requestId)}
              >
                {safeExitFailure.retrying ? "正在重试…" : "重试保存"}
              </button>
              <button
                type="button"
                disabled={safeExitFailure.retrying}
                onClick={() => void cancelExit()}
              >
                取消退出，继续使用
              </button>
              <button
                className="is-danger"
                type="button"
                disabled={safeExitFailure.retrying}
                onClick={() => void forceExit()}
              >
                仍然退出
              </button>
            </div> : null}
            {safeExitFailure.message ? (
              <small>仍然退出可能丢失尚未落盘的修改。</small>
            ) : null}
          </section>
        </div>
      ) : null}
    </>
  );

  if (isTauriRuntime) {
    return mainApp;
  }

  if (isResponsivePreview) {
    return mainApp;
  }

  return (
    <div
      className="rr-main-preview-canvas"
      style={
        {
          "--rr-main-preview-scale": previewScale,
        } as CSSProperties
      }
    >
      {mainApp}
    </div>
  );
}

export default App;
