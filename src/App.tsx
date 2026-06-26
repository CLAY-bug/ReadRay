import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import Database from "@tauri-apps/plugin-sql";
import AnchoredResultPopover, {
  type AnchorRect,
  type AnchoredResult,
} from "./components/AnchoredResultPopover";
import CenteredCommandInput from "./components/CenteredCommandInput";
import CenteredResultPanel, {
  type CenteredResult,
} from "./components/CenteredResultPanel";
import "./App.css";

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

type Stage1CheckRow = {
  id: number;
  label: string;
  value: string;
  created_at: string;
};

type PreviewMode = "anchored" | "command";
type CommandStage = "input" | "loading" | "result";

type QueryType = "word" | "phrase" | "sentence";

type CaptureInput = {
  queryText: string;
  contextText?: string | null;
  sourceType: "manual" | "clipboard" | "windows_uia" | "app_adapter" | "ocr";
};

type ExplanationCard = {
  queryType: QueryType;
  headword: string;
  phonetic?: string | null;
  basicMeaning: string;
  contextMeaning?: string | null;
  phrases: Array<{
    phrase: string;
    meaning: string;
  }>;
  nearMeanings: Array<{
    term: string;
    meaning: string;
  }>;
  examples: Array<{
    en: string;
    zh: string;
  }>;
  difficulty?: string | null;
  reviewHint?: string | null;
};

const idle: CheckResult = { state: "idle", detail: "未验证" };

const mockAnchoredResult: AnchoredResult = {
  word: "marketed",
  phonetic: "/ˈmɑːrkɪtɪd/",
  definition: "v. 宣传；推广；定位成",
  contextMeaning: "被宣传为；被定位为",
  usage: "marketed as = 被宣传为 / 被定位为",
  example: "The course is marketed as beginner-friendly.",
  exampleZh: "这门课程被宣传为适合初学者。",
  highlightText: "marketed as",
};

const mockCenteredResult: CenteredResult = {
  word: "marketed",
  phonetic: "/ˈmɑːrkɪtɪd/",
  partOfSpeech: "动词 market 的过去式 / 过去分词",
  definition: "宣传；推广；把……定位为",
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
  nearMeaningTitle: "近义理解",
  nearMeanings: [
    {
      phrase: "marketed",
      meaning: "强调宣传、推广、市场定位",
    },
    {
      phrase: "sold",
      meaning: "强调已经卖出或完成销售",
    },
    {
      phrase: "advertised",
      meaning: "强调投放广告，是 marketed 的一种方式",
    },
  ],
  example: "The product is marketed as eco-friendly.",
  exampleZh: "这个产品被宣传为环保的。",
};

function mapExplanationCardToCenteredResult(card: ExplanationCard): CenteredResult {
  const primaryExample = card.examples[0];
  const reviewHint = card.reviewHint?.trim();

  return {
    word: card.headword,
    phonetic: card.phonetic ?? "",
    partOfSpeech: reviewHint || undefined,
    definition: card.contextMeaning
      ? `${card.basicMeaning}｜语境：${card.contextMeaning}`
      : card.basicMeaning,
    phrases: card.phrases,
    nearMeaningTitle: "近义理解",
    nearMeanings: card.nearMeanings.map((item) => ({
      phrase: item.term,
      meaning: item.meaning,
    })),
    example: primaryExample.en,
    exampleZh: primaryExample.zh,
  };
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function App() {
  const previewAnchorRef = useRef<HTMLDivElement>(null);
  const [shortcutLabel, setShortcutLabel] = useState("Ctrl+Alt+R");
  const [windowCheck, setWindowCheck] = useState<CheckResult>(idle);
  const [clipboardCheck, setClipboardCheck] = useState<CheckResult>(idle);
  const [sqliteCheck, setSqliteCheck] = useState<CheckResult>(idle);
  const [deepseekCheck, setDeepseekCheck] = useState<CheckResult>(idle);
  const [alwaysOnTop, setAlwaysOnTop] = useState(false);
  const [previewMode, setPreviewMode] = useState<PreviewMode>("anchored");
  const [popoverOpen, setPopoverOpen] = useState(true);
  const [anchorRect, setAnchorRect] = useState<AnchorRect | null>(null);
  const [commandOpen, setCommandOpen] = useState(false);
  const [commandValue, setCommandValue] = useState("");
  const [commandStage, setCommandStage] = useState<CommandStage>("input");
  const [commandError, setCommandError] = useState<string | undefined>();
  const [centeredResult, setCenteredResult] =
    useState<CenteredResult>(mockCenteredResult);

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

  const clearCommandState = useCallback(() => {
    setCommandStage("input");
    setCommandError(undefined);
  }, []);

  function showPreviewPopover() {
    clearCommandState();
    setPreviewMode("anchored");
    updatePreviewAnchorRect();
    setPopoverOpen(true);
  }

  function showCommandInputPreview() {
    clearCommandState();
    setPreviewMode("command");
    setCommandOpen(true);
  }

  function handleCommandOpenChange(nextOpen: boolean) {
    clearCommandState();
    setCommandOpen(nextOpen);
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
      setCenteredResult(mapExplanationCardToCenteredResult(card));
      setCommandStage("result");
    } catch (error) {
      setCommandStage("input");
      setCommandError(formatError(error));
    }
  }

  async function toggleWindow() {
    setWindowCheck({ state: "running", detail: "正在切换窗口" });
    try {
      const visible = await invoke<boolean>("toggle_main_window");
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
      const state = await invoke<WindowState>("set_main_window_always_on_top", {
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
    setSqliteCheck({ state: "running", detail: "正在写入和读取 SQLite" });

    try {
      const db = await Database.load("sqlite:readray-stage1.db");
      await db.execute(
        "CREATE TABLE IF NOT EXISTS stage1_checks (id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT NOT NULL, value TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
      );

      const value = `sqlite-${Date.now()}`;
      await db.execute(
        "INSERT INTO stage1_checks (label, value) VALUES ($1, $2)",
        ["phase-one", value],
      );

      const rows = await db.select<Stage1CheckRow[]>(
        "SELECT id, label, value, created_at FROM stage1_checks WHERE value = $1 ORDER BY id DESC LIMIT 1",
        [value],
      );

      if (rows.length === 0) {
        setSqliteCheck({ state: "error", detail: "写入后没有读到记录" });
        return;
      }

      setSqliteCheck({
        state: "ok",
        detail: `读写成功：#${rows[0].id} ${rows[0].value}`,
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
      <section className="compact-preview" aria-label="ReadRay compact UI 桌面预览">
        {previewMode === "anchored" ? (
          <>
            <div className="mock-reader-line" aria-hidden="true" />
            <div className="compact-preview__anchor" ref={previewAnchorRef}>
              <span>marketed</span>
            </div>
            <AnchoredResultPopover
              result={mockAnchoredResult}
              anchorRect={anchorRect}
              open={popoverOpen}
              onOpenChange={setPopoverOpen}
            />
          </>
        ) : (
          <>
            <CenteredCommandInput
              value={commandValue}
              onValueChange={handleCommandValueChange}
              open={commandOpen && commandStage !== "result"}
              loading={commandStage === "loading"}
              error={commandError}
              onSubmit={submitCommand}
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
        )}
      </section>

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
    </main>
  );
}

export default App;
