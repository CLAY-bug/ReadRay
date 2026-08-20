import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

function section(source, start, end) {
  const startIndex = source.indexOf(start);
  const endIndex = source.indexOf(end, startIndex + start.length);
  assert.notEqual(startIndex, -1, `缺少起始标记：${start}`);
  assert.notEqual(endIndex, -1, `缺少结束标记：${end}`);
  return source.slice(startIndex, endIndex);
}

test("隐藏与重新呼出只改变可见性，不清理页面或 Quick AI 会话", async () => {
  const app = await readFile("src/App.tsx", "utf8");
  const hide = section(
    app,
    "async function handleCommandOpenChange",
    "function handleCommandValueChange",
  );
  assert.doesNotMatch(hide, /quickAiRequestId|setQuickAi|clearCommandState/);
  assert.match(hide, /setCommandOpen\(nextOpen\)/);
  assert.match(hide, /hide_overlay_window/);

  const reopen = section(
    app,
    "const showCommandOverlay",
    "const closeAnchoredOverlay",
  );
  assert.match(reopen, /setPreviewMode\("command"\)/);
  assert.match(reopen, /setCommandOpen\(true\)/);
  assert.doesNotMatch(reopen, /setCenteredMode|clearQuickAiState|clearCommandState/);
});

test("Quick AI 只在首次发送时建库，明确新对话不产生空白记录", async () => {
  const app = await readFile("src/App.tsx", "utf8");
  const send = section(app, "async function sendQuickAiMessage", "async function enterQuickAi");
  const enter = section(app, "async function enterQuickAi", "function returnToCommandInput");
  const startNew = section(
    app,
    "const startNewQuickAiConversation",
    "const showCommandOverlay",
  );

  assert.match(send, /create_quick_ai_conversation/);
  assert.match(send, /origin: "overlay"/);
  assert.match(send, /quickAiConversationRef\.current\?\.id/);
  assert.doesNotMatch(enter, /create_quick_ai_conversation/);
  assert.match(enter, /sendQuickAiMessage\(initialMessage\)/);
  assert.doesNotMatch(startNew, /invoke|create_quick_ai_conversation/);
});

test("对话页 Esc 返回搜索，滚动位置与原生窗口阶段均可恢复", async () => {
  const [app, panel, rust] = await Promise.all([
    readFile("src/App.tsx", "utf8"),
    readFile("src/components/QuickAiPanel.tsx", "utf8"),
    readFile("src-tauri/src/lib.rs", "utf8"),
  ]);

  assert.match(panel, /event\.key === "Escape"[\s\S]*onBack\(\)/);
  assert.match(panel, /className="quick-ai-panel__back"[\s\S]*onClick=\{onBack\}/);
  assert.match(panel, /aria-label="返回搜索"/);
  assert.match(panel, /messageScrollStateRef/);
  assert.match(panel, /distanceFromBottom < 80/);
  assert.match(app, /onBack=\{returnToCommandInput\}/);
  assert.match(app, /centeredMode === "explanation"[\s\S]*commandStage !== "result"/);
  assert.match(rust, /current_overlay_window_stage/);
  assert.match(rust, /resize_overlay_window\(&window, get_current_overlay_window_stage\(\)\?\)/);
});

test("首次 Quick AI 等待态左对齐，搜索入口使用两层 Esc", async () => {
  const [app, panel, commandInput, resultPanel] = await Promise.all([
    readFile("src/App.tsx", "utf8"),
    readFile("src/components/QuickAiPanel.tsx", "utf8"),
    readFile("src/components/CenteredCommandInput.tsx", "utf8"),
    readFile("src/components/CenteredResultPanel.tsx", "utf8"),
  ]);

  assert.match(panel, /const conversationIsEmpty = messages\.length === 0 && !pendingMessage/);
  assert.match(panel, /quick-ai-panel__messages\$\{conversationIsEmpty \? " is-empty" : ""\}/);
  assert.match(commandInput, /event\.key === "Escape"[\s\S]*if \(value \|\| loading \|\| error\)[\s\S]*onValueChange\(""\)[\s\S]*onOpenChange\(false\)/);
  assert.match(resultPanel, /event\.key === "Escape"[\s\S]*onQueryChange\(""\)/);
  assert.doesNotMatch(resultPanel, /onOpenChange/);

  const change = section(
    app,
    "function handleCommandValueChange",
    "async function submitCommand",
  );
  const submit = section(
    app,
    "async function submitCommand",
    "async function sendQuickAiMessage",
  );
  assert.match(change, /explanationRequests\.invalidate\("manual"\)/);
  assert.match(submit, /explanationRequests\.begin\("manual"\)/);
  assert.match(submit, /explanationRequests\.isCurrent\("manual", requestKey\)/);
  assert.match(submit, /isExplanationRequestCancelled/);
});

test("ExplanationCard 通知、原始上下文与 Rust requestKey 权威保持同一链路", async () => {
  const [app, rust, lib, uia] = await Promise.all([
    readFile("src/App.tsx", "utf8"),
    readFile("src-tauri/src/deepseek_explanation.rs", "utf8"),
    readFile("src-tauri/src/lib.rs", "utf8"),
    readFile("src-tauri/src/windows_uia.rs", "utf8"),
  ]);
  const anchored = section(app, "const runAnchoredQuery", "const handleAnchoredContentSizeChange");
  const manual = section(app, "async function submitCommand", "async function sendQuickAiMessage");

  assert.match(anchored, /contextText: capturedContextForRecord\(capture\.contextText\)/);
  assert.match(anchored, /minimalContextText: capture\.minimalContext \?\? null/);
  assert.match(anchored, /requestScope: "anchored"/);
  assert.match(anchored, /isCurrent\("anchored", requestKey\)[\s\S]*notifyLearningRecordCreated\(\)/);
  assert.match(manual, /requestScope: "manual"/);
  assert.match(manual, /minimalContextText: null/);
  assert.match(manual, /isCurrent\("manual", requestKey\)[\s\S]*notifyLearningRecordCreated\(\)/);
  assert.match(rust, /commit_if_current\([\s\S]*learning_records::save_for_app/);
  assert.match(rust, /Abortable::new\(provider_request, abort_registration\)/);
  assert.match(lib, /fn hide_overlay_window[\s\S]*cancel_explanation_scope/);
  assert.match(lib, /fn hide_anchored_overlay_window[\s\S]*cancel_explanation_scope/);
  assert.match(lib, /WindowEvent::Focused\(false\)[\s\S]*cancel_all_explanation_requests/);
  assert.match(lib, /WindowEvent::CloseRequested[\s\S]*OVERLAY_WINDOW_LABEL[\s\S]*cancel_all_explanation_requests/);
  assert.doesNotMatch(lib, /serde_json::to_string\(&capture\)/);
  assert.match(lib, /READRAY_UIA_CAPTURE ok=\{\} selected_chars=\{\} has_context=\{\}/);
  assert.match(lib, /windows_uia::capture_foreground_with_retry\(\)/);
  assert.match(uia, /const UIA_CAPTURE_RETRY_DELAYS_MS: \[u64; 2\] = \[40, 80\]/);
  assert.match(uia, /pub fn capture_foreground_with_retry\(\)/);
  assert.match(uia, /retry\.foreground\.hwnd != initial_foreground_hwnd/);
});

test("划词结果先渲染再按稳定尺寸调整浮层，避免结果阶段重复重排", async () => {
  const [app, popover] = await Promise.all([
    readFile("src/App.tsx", "utf8"),
    readFile("src/components/AnchoredResultPopover.tsx", "utf8"),
  ]);
  const anchored = section(
    app,
    "const runAnchoredQuery",
    "const handleAnchoredContentSizeChange",
  );

  assert.doesNotMatch(anchored, /stage: "result"/);
  assert.match(anchored, /setAnchoredResult\(mapExplanationCard\(card\)\)/);
  assert.match(anchored, /setAnchoredStage\("result"\)/);
  assert.match(app, /anchoredResizePending/);
  assert.match(app, /anchoredResizeGeneration/);
  assert.match(popover, /element\.style\.maxWidth = "none"/);
  assert.match(popover, /settleTimer = window\.setTimeout/);
});

test("搜索框与 Quick AI 同宽同首行高，并从固定顶边向下展开", async () => {
  const [rust, configSource, styles] = await Promise.all([
    readFile("src-tauri/src/lib.rs", "utf8"),
    readFile("src-tauri/tauri.conf.json", "utf8"),
    readFile("src/App.css", "utf8"),
  ]);
  const config = JSON.parse(configSource);
  const overlay = config.app.windows.find((window) => window.label === "overlay");

  assert.equal(overlay.width, 750);
  assert.equal(overlay.height, 58);
  assert.equal(overlay.minHeight, 58);
  assert.match(rust, /Self::Input => LogicalSize::new\(750\.0, 58\.0\)/);
  assert.match(rust, /Self::QuickAi => LogicalSize::new\(750\.0, 500\.0\)/);
  assert.match(rust, /uses_drawer_anchor_with/);
  assert.match(rust, /\(previous_size\.width - next_size\.width\) \/ 2\.0/);
  assert.match(rust, /y: f64::from\(position\.y\) \/ scale_factor/);
  assert.match(styles, /\.centered-command-input \{[\s\S]*inset: 0;[\s\S]*width: 100vw;/);
  assert.match(styles, /\.centered-command-input__box \{[\s\S]*min-height: 58px;/);
  assert.match(styles, /\.quick-ai-panel \{[\s\S]*grid-template-rows: 58px/);
  const focusedInput = section(
    styles,
    ".centered-command-input__box:focus-within,",
    ".centered-command-input__box:focus-within::before",
  );
  assert.doesNotMatch(focusedInput, /transform:/);
  assert.match(styles, /animation: quick-ai-drawer-open 180ms/);
  assert.match(styles, /@keyframes quick-ai-drawer-open/);
  assert.match(styles, /calc\(100% - 58px\)/);
  assert.match(styles, /prefers-reduced-motion[\s\S]*\.quick-ai-panel \{\s*animation: none;/);
});

test("Overlay 历史按来源查询并在当前窗口打开完整历史页", async () => {
  const [app, panel, shell, rust, historyPage] = await Promise.all([
    readFile("src/App.tsx", "utf8"),
    readFile("src/components/QuickAiPanel.tsx", "utf8"),
    readFile("src/components/MainAppShell.tsx", "utf8"),
    readFile("src-tauri/src/lib.rs", "utf8"),
    readFile("src/components/ConversationHistoryPage.tsx", "utf8"),
  ]);

  assert.match(app, /list_recent_quick_ai_conversations[\s\S]*limit: 8, origin: "overlay"/);
  assert.match(app, /list_all_quick_ai_conversations[\s\S]*origin: "overlay"/);
  assert.match(app, /get_quick_ai_conversation/);
  assert.match(app, /conversation\.origin !== "overlay"/);
  assert.match(app, /quickAiSelectionRequestId/);
  assert.match(app, /setQuickAiPage\("history"\)/);
  assert.match(panel, /className="quick-ai-panel__history"/);
  assert.match(panel, /className="quick-ai-panel__history-menu"/);
  assert.match(panel, /quick-ai-panel__history-page/);
  assert.match(panel, /onHistoryBack/);
  assert.match(panel, /onNewConversation\(\)/);
  assert.match(panel, /onConversationSelect\(conversationId\)/);
  assert.match(panel, /document\.addEventListener\("pointerdown"/);
  assert.doesNotMatch(panel, /quick-ai-panel__new-chat/);
  assert.doesNotMatch(rust, /open_main_conversation_history/);
  assert.doesNotMatch(rust, /OPEN_CONVERSATION_HISTORY_EVENT/);
  assert.doesNotMatch(shell, /conversationHistoryRequestKey/);
  assert.match(historyPage, /rr-conversation-history-source-filter/);
  assert.match(historyPage, /conversationOriginLabels/);
});

test("Overlay 验收修正保留当前窗口边界与可恢复的交互反馈", async () => {
  const [app, panel, styles] = await Promise.all([
    readFile("src/App.tsx", "utf8"),
    readFile("src/components/QuickAiPanel.tsx", "utf8"),
    readFile("src/App.css", "utf8"),
  ]);

  assert.match(app, /onContextMenu=\{\(event\) => event\.preventDefault\(\)\}/);
  assert.match(app, /rename_quick_ai_conversation/);
  assert.match(app, /quickAiRenameRequestId/);
  assert.match(app, /quickAiConversationRef\.current\?\.id !== conversationId/);
  assert.match(panel, /className="quick-ai-panel__title-button"/);
  assert.match(panel, /className="quick-ai-panel__title-input"/);
  assert.match(panel, /size=\{getTitleInputSize\(renameDraft\)\}/);
  assert.match(panel, /codePoint <= 0xff \? 1 : 2/);
  assert.match(panel, /maxLength=\{80\}/);
  assert.match(styles, /\.quick-ai-panel__identity \{[\s\S]*grid-template-rows: calc\(20px \* var\(--rr-ui-font-scale\)\) auto;/);
  assert.match(styles, /\.quick-ai-panel__title-button \{[\s\S]*width: fit-content;[\s\S]*justify-self: start;/);
  assert.match(styles, /\.quick-ai-panel__title-button:active \{\s*background: transparent;/);
  assert.match(panel, /event\.key === "Enter" && !event\.nativeEvent\.isComposing/);
  assert.match(panel, /event\.key === "Escape"[\s\S]*event\.stopPropagation\(\)/);
  assert.match(panel, /className="quick-ai-panel__send"[\s\S]*<svg viewBox="0 0 24 24"/);
  assert.doesNotMatch(panel, />\s*↗\s*</);
  assert.match(panel, /<path d="M12 19V5" \/>/);
  assert.match(styles, /\.quick-ai-panel__send \{[\s\S]*padding: 0;[\s\S]*line-height: 0;/);
  assert.match(styles, /\.quick-ai-panel__send svg \{[\s\S]*display: block;/);
  assert.doesNotMatch(panel, /<span>\{message\.role === "user" \? "你" : "AI"\}<\/span>/);
  assert.doesNotMatch(panel, /<span>你<\/span>|<span>AI<\/span>/);
  assert.match(panel, /data-text="Thinking…"/);
  assert.match(styles, /@keyframes quick-ai-thinking-shimmer/);
  assert.match(styles, /background-position: -125% 0/);
  assert.match(styles, /background-position: 225% 0/);
  assert.match(styles, /prefers-reduced-motion[\s\S]*quick-ai-panel__thinking-label::after/);
});

test("集成回归覆盖多行输入、Ctrl+N 与 loading/error/stopped 状态", async () => {
  const [app, panel, styles, rust, conversationTests] = await Promise.all([
    readFile("src/App.tsx", "utf8"),
    readFile("src/components/QuickAiPanel.tsx", "utf8"),
    readFile("src/App.css", "utf8"),
    readFile("src-tauri/src/quick_ai.rs", "utf8"),
    readFile("tests/conversationService.test.mjs", "utf8"),
  ]);

  assert.match(panel, /const contentHeight = input\.scrollHeight/);
  assert.match(panel, /Math\.min\(contentHeight, maxHeight\)/);
  assert.match(panel, /contentHeight > maxHeight \? "auto" : "hidden"/);
  assert.match(panel, /shouldSendMultilineMessage\([\s\S]*isComposing: event\.nativeEvent\.isComposing/);
  assert.match(styles, /\.quick-ai-panel__composer textarea \{[\s\S]*max-height: 112px;[\s\S]*overflow-y: hidden;/);

  assert.match(panel, /event\.ctrlKey && event\.key\.toLowerCase\(\) === "n"/);
  assert.match(panel, /event\.preventDefault\(\);\s*startNewConversation\(\);/);
  const startNew = section(
    app,
    "const startNewQuickAiConversation",
    "const showCommandOverlay",
  );
  assert.match(startNew, /quickAiRequestId\.current \+= 1/);
  assert.match(startNew, /setQuickAiConversation\(null\)/);
  assert.match(startNew, /setQuickAiDraft\(""\)/);
  assert.doesNotMatch(startNew, /invoke|create_quick_ai_conversation/);

  assert.match(panel, /pendingMessage \? \([\s\S]*role="status"[\s\S]*aria-label="AI 正在思考"/);
  assert.match(panel, /disabled=\{loading\}/);
  assert.match(panel, /error \? <p className="quick-ai-panel__error">\{error\}<\/p>/);
  assert.match(panel, /historyStatus === "loading" \|\| historyStatus === "idle"/);
  assert.match(panel, /historyStatus === "error"/);
  assert.match(panel, /allHistoryStatus === "loading" \|\| allHistoryStatus === "idle"/);
  assert.match(panel, /allHistoryStatus === "error"/);
  assert.match(panel, /onClick=\{onAllHistoryRetry\}/);

  assert.match(rust, /pub enum QuickAiStreamEvent \{[\s\S]*Stopped/);
  assert.match(rust, /sender\.send\(QuickAiStreamEvent::Stopped\)/);
  assert.match(conversationTests, /用户停止后 assistant 未落库时返回 pending 且可重试/);
  assert.match(conversationTests, /assert\.equal\(stoppedResult\.status, "pending"\)/);
});
