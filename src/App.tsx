import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import Database from "@tauri-apps/plugin-sql";
import AnchoredResultPopover, {
  type AnchorRect,
  type AnchoredResult,
} from "./components/AnchoredResultPopover";
import CenteredCommandInput from "./components/CenteredCommandInput";
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

const idle: CheckResult = { state: "idle", detail: "未验证" };
const mockCommandError = "暂时无法解释，请稍后再试。";

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

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function App() {
  const previewAnchorRef = useRef<HTMLDivElement>(null);
  const commandMockTimerRef = useRef<number | null>(null);
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
  const [commandValue, setCommandValue] = useState("marketed");
  const [commandLoading, setCommandLoading] = useState(false);
  const [commandError, setCommandError] = useState<string | undefined>();

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
    return () => {
      if (commandMockTimerRef.current !== null) {
        window.clearTimeout(commandMockTimerRef.current);
        commandMockTimerRef.current = null;
      }
    };
  }, []);

  const clearCommandMockState = useCallback(() => {
    if (commandMockTimerRef.current !== null) {
      window.clearTimeout(commandMockTimerRef.current);
      commandMockTimerRef.current = null;
    }

    setCommandLoading(false);
    setCommandError(undefined);
  }, []);

  function showPreviewPopover() {
    clearCommandMockState();
    setPreviewMode("anchored");
    updatePreviewAnchorRect();
    setPopoverOpen(true);
  }

  function showCommandInputPreview() {
    clearCommandMockState();
    setPreviewMode("command");
    setCommandOpen(true);
  }

  function handleCommandOpenChange(nextOpen: boolean) {
    clearCommandMockState();
    setCommandOpen(nextOpen);
  }

  function handleCommandValueChange(nextValue: string) {
    setCommandValue(nextValue);
    setCommandError(undefined);
  }

  function submitMockCommand(value: string) {
    if (commandLoading) {
      return;
    }

    if (commandMockTimerRef.current !== null) {
      window.clearTimeout(commandMockTimerRef.current);
    }

    setCommandValue(value);
    setCommandError(undefined);
    setCommandLoading(true);
    commandMockTimerRef.current = window.setTimeout(() => {
      setCommandLoading(false);
      setCommandError(mockCommandError);
      commandMockTimerRef.current = null;
    }, 1350);
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
          <CenteredCommandInput
            value={commandValue}
            onValueChange={handleCommandValueChange}
            open={commandOpen}
            loading={commandLoading}
            error={commandError}
            onSubmit={submitMockCommand}
            onOpenChange={handleCommandOpenChange}
          />
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
