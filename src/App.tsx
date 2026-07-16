import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import AnchoredResultPopover, {
  type AnchorRect,
} from "./components/AnchoredResultPopover";
import CenteredCommandInput from "./components/CenteredCommandInput";
import CenteredResultPanel, {
  type CenteredResult,
} from "./components/CenteredResultPanel";
import MainAppShell from "./components/MainAppShell";
import QuickAiPanel from "./components/QuickAiPanel";
import {
  mapExplanationCard,
  type ExplanationResult,
} from "./explanationViewModel";
import type {
  CaptureInput,
  ExplanationCard,
} from "./types/explanation";
import type { QuickAiConversation } from "./types/quickAi";
import { mainAppFixture } from "./mainAppViewModel";
import "./App.css";
import "./styles/main-app.css";

type CheckState = "idle" | "running" | "ok" | "warn" | "error";

type CheckResult = {
  state: CheckState;
  detail: string;
};

type WindowState = {
  visible: boolean;
  alwaysOnTop: boolean;
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
type CommandStage = "input" | "loading" | "result";
type AnchoredStage = "mock" | "loading" | "result" | "error";
type OverlayWindowStage = "input" | "loading" | "result" | "error";

type WindowsUiaCapture = {
  selectedText?: string | null;
  contextText?: string | null;
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

function contextForQuery(selectedText: string, contextText?: string | null) {
  const normalized = normalizeUiaText(contextText);
  return normalized && normalized !== selectedText ? normalized : null;
}

function sourceAppForCapture(capture: WindowsUiaCapture) {
  const executablePath = capture.foreground?.executablePath;
  if (executablePath) {
    const segments = executablePath.split(/[\\/]/);
    return segments[segments.length - 1] || null;
  }

  return null;
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function OverlayApp() {
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
  const [quickAiConversation, setQuickAiConversation] =
    useState<QuickAiConversation | null>(null);
  const [quickAiDraft, setQuickAiDraft] = useState("");
  const [quickAiPendingMessage, setQuickAiPendingMessage] = useState<string>();
  const [quickAiLoading, setQuickAiLoading] = useState(false);
  const [quickAiError, setQuickAiError] = useState<string>();
  const anchoredRequestId = useRef(0);
  const quickAiRequestId = useRef(0);
  const anchoredSourceRect = useRef<AnchorRect | null>(null);

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
      ? "result"
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

  const clearQuickAiState = useCallback(() => {
    quickAiRequestId.current += 1;
    setQuickAiConversation(null);
    setQuickAiDraft("");
    setQuickAiPendingMessage(undefined);
    setQuickAiLoading(false);
    setQuickAiError(undefined);
  }, []);

  const showCommandOverlay = useCallback(() => {
    anchoredRequestId.current += 1;
    clearCommandState();
    clearQuickAiState();
    setCenteredMode("explanation");
    setPreviewMode("command");
    setCommandOpen(true);
  }, [clearCommandState, clearQuickAiState]);

  const closeAnchoredOverlay = useCallback(async () => {
    anchoredRequestId.current += 1;
    setPopoverOpen(false);
    try {
      await invoke("hide_anchored_overlay_window");
    } catch (error) {
      setWindowCheck({ state: "warn", detail: formatError(error) });
    }
  }, []);

  const runAnchoredQuery = useCallback(async (capture: WindowsUiaCapture) => {
    const selectedText = normalizeUiaText(capture.selectedText);
    const capturedAnchorRect = capture.anchorRect;
    if (!selectedText || !capturedAnchorRect) {
      return;
    }

    const requestId = anchoredRequestId.current + 1;
    anchoredRequestId.current = requestId;
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
      if (anchoredRequestId.current === requestId) {
        setAnchoredStage("error");
        setAnchoredError(`无法显示划词浮层：${formatError(error)}`);
      }
      return;
    }

    const input: CaptureInput = {
      queryText: selectedText,
      contextText: contextForQuery(selectedText, capture.contextText),
      sourceType: "windows_uia",
      sourceApp: sourceAppForCapture(capture),
    };

    try {
      const card = await invoke<ExplanationCard>("create_explanation_card", {
        input,
      });
      if (anchoredRequestId.current !== requestId) {
        return;
      }

      await invoke("present_anchored_overlay_window", {
        stage: "result",
        anchorRect: capturedAnchorRect,
      });
      if (anchoredRequestId.current !== requestId) {
        return;
      }
      setAnchoredResult(mapExplanationCard(card));
      setAnchoredStage("result");
    } catch (error) {
      if (anchoredRequestId.current !== requestId) {
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
  }, []);

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
      anchoredRequestId.current += 1;
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
  }, [consumeOverlayIntent]);

  function showPreviewPopover() {
    anchoredRequestId.current += 1;
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
    clearCommandState();
    if (!nextOpen) {
      quickAiRequestId.current += 1;
      setQuickAiPendingMessage(undefined);
      setQuickAiLoading(false);
    }
    setCommandOpen(nextOpen);
    if (!nextOpen) {
      try {
        await invoke("hide_overlay_window");
      } catch (error) {
        setWindowCheck({ state: "warn", detail: formatError(error) });
      }
    }
  }

  function handleCommandValueChange(nextValue: string) {
    setCommandValue(nextValue);
    setCommandStage("input");
    setCommandError(undefined);
  }

  async function submitCommand(value: string) {
    if (commandStage === "loading") {
      return;
    }

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
      });
      setCenteredResult(mapExplanationCard(card));
      setCommandStage("result");
    } catch (error) {
      setCommandStage("input");
      setCommandError(formatError(error));
    }
  }

  async function sendQuickAiMessage(
    value: string,
    conversationId: number | null,
  ) {
    const content = value.trim();
    if (!content || quickAiLoading) {
      return;
    }

    const requestId = quickAiRequestId.current + 1;
    quickAiRequestId.current = requestId;
    setQuickAiDraft("");
    setQuickAiPendingMessage(content);
    setQuickAiLoading(true);
    setQuickAiError(undefined);

    try {
      const conversation = await invoke<QuickAiConversation>(
        "send_quick_ai_message",
        {
          conversationId,
          content,
        },
      );
      if (quickAiRequestId.current !== requestId) {
        return;
      }
      setQuickAiConversation(conversation);
      setQuickAiPendingMessage(undefined);
      setQuickAiLoading(false);
    } catch (error) {
      if (quickAiRequestId.current !== requestId) {
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
    clearQuickAiState();
    setCenteredMode("quick-ai");
    setCommandValue("");
    setCommandError(undefined);

    if (initialMessage) {
      await sendQuickAiMessage(initialMessage, null);
      return;
    }

    const requestId = quickAiRequestId.current + 1;
    quickAiRequestId.current = requestId;
    setQuickAiLoading(true);
    try {
      const conversation = await invoke<QuickAiConversation>(
        "create_quick_ai_conversation",
      );
      if (quickAiRequestId.current !== requestId) {
        return;
      }
      setQuickAiConversation(conversation);
      setQuickAiLoading(false);
    } catch (error) {
      if (quickAiRequestId.current !== requestId) {
        return;
      }
      setQuickAiLoading(false);
      setQuickAiError(formatError(error));
    }
  }

  async function createNewQuickAiConversation() {
    const requestId = quickAiRequestId.current + 1;
    quickAiRequestId.current = requestId;
    setQuickAiConversation(null);
    setQuickAiDraft("");
    setQuickAiPendingMessage(undefined);
    setQuickAiError(undefined);
    setQuickAiLoading(true);
    try {
      const conversation = await invoke<QuickAiConversation>(
        "create_quick_ai_conversation",
      );
      if (quickAiRequestId.current !== requestId) {
        return;
      }
      setQuickAiConversation(conversation);
      setQuickAiLoading(false);
    } catch (error) {
      if (quickAiRequestId.current !== requestId) {
        return;
      }
      setQuickAiLoading(false);
      setQuickAiError(formatError(error));
    }
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
    <main className="app-shell">
      <section className="compact-preview" aria-label="ReadRay 桌面浮层">
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
          centeredMode === "quick-ai" ? (
            <QuickAiPanel
              open={commandOpen}
              conversation={quickAiConversation}
              draft={quickAiDraft}
              pendingMessage={quickAiPendingMessage}
              loading={quickAiLoading}
              error={quickAiError}
              onDraftChange={setQuickAiDraft}
              onSend={(value) =>
                void sendQuickAiMessage(value, quickAiConversation?.id ?? null)
              }
              onNewConversation={() => void createNewQuickAiConversation()}
              onOpenChange={handleCommandOpenChange}
            />
          ) : (
            <>
              <CenteredCommandInput
                value={commandValue}
                onValueChange={handleCommandValueChange}
                open={commandOpen && commandStage !== "result"}
                loading={commandStage === "loading"}
                error={commandError}
                onSubmit={submitCommand}
                onQuickAi={(value) => void enterQuickAi(value)}
                onOpenChange={handleCommandOpenChange}
              />
              <CenteredResultPanel
                query={commandValue}
                result={centeredResult}
                open={commandOpen && commandStage === "result"}
                onQueryChange={handleCommandValueChange}
                onSubmit={submitCommand}
                onOpenChange={handleCommandOpenChange}
              />
            </>
          )
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
  const isTauriRuntime = "__TAURI_INTERNALS__" in window;

  useEffect(() => {
    if (!isTauriRuntime) {
      return;
    }

    function syncMaximizedState() {
      invoke<boolean>("main_window_is_maximized")
        .then(setIsMaximized)
        .catch((error) => console.error("ReadRay 主窗口状态读取失败：", error));
    }

    syncMaximizedState();
    window.addEventListener("resize", syncMaximizedState);
    return () => window.removeEventListener("resize", syncMaximizedState);
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
    const nextState = await runMainWindowCommand<boolean>(
      "toggle_main_window_maximized",
    );
    if (typeof nextState === "boolean") {
      setIsMaximized(nextState);
    }
  }, [runMainWindowCommand]);

  const mainApp = (
    <MainAppShell
      viewModel={mainAppFixture}
      isMaximized={isMaximized}
      onStartDragging={() => {
        void runMainWindowCommand("start_main_window_drag");
      }}
      onMinimize={() => {
        void runMainWindowCommand("minimize_main_window");
      }}
      onToggleMaximize={toggleMaximized}
      onClose={() => {
        void runMainWindowCommand("hide_main_window");
      }}
    />
  );

  if (isTauriRuntime) {
    return mainApp;
  }

  return <div className="rr-main-preview-canvas">{mainApp}</div>;
}

export default App;
