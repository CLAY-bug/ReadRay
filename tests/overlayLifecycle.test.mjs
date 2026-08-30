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
  assert.match(lib, /WindowEvent::CloseRequested[\s\S]*is_active_overlay_label[\s\S]*cancel_all_explanation_requests/);
  assert.doesNotMatch(lib, /serde_json::to_string\(&capture\)/);
  assert.match(lib, /READRAY_UIA_CAPTURE ok=\{\} selected_chars=\{\} has_context=\{\}/);
  assert.match(lib, /windows_uia::capture_foreground_with_retry\(\)/);
  assert.match(uia, /const UIA_CAPTURE_RETRY_DELAYS_MS: \[u64; 2\] = \[40, 80\]/);
  assert.match(uia, /pub fn capture_foreground_with_retry\(\)/);
  assert.match(uia, /retry\.foreground\.hwnd != initial_foreground_hwnd/);
});

test("划词卡片默认延迟显示紧凑加载卡，减少动态效果时等待最终结果", async () => {
  const [app, preferences, popover, lib, styles] = await Promise.all([
    readFile("src/App.tsx", "utf8"),
    readFile("src/appPreferences.ts", "utf8"),
    readFile("src/components/AnchoredResultPopover.tsx", "utf8"),
    readFile("src-tauri/src/lib.rs", "utf8"),
    readFile("src/App.css", "utf8"),
  ]);
  const anchored = section(
    app,
    "const runAnchoredQuery",
    "const handleAnchoredContentSizeChange",
  );
  const beforeProviderRequest = section(
    anchored,
    "const runAnchoredQuery",
    'const card = await invoke<ExplanationCard>("create_explanation_card"',
  );

  assert.doesNotMatch(anchored, /stage: "loading"/);
  assert.match(preferences, /selectionExplanationDisplayMode: "standard"/);
  assert.match(beforeProviderRequest, /selectionExplanationDisplayMode === "standard"/);
  assert.match(
    beforeProviderRequest,
    /window\.setTimeout[\s\S]*ANCHORED_LOADING_GRACE_MS/,
  );
  assert.match(beforeProviderRequest, /present_anchored_loading_window/);
  assert.match(app, /const ANCHORED_LOADING_GRACE_MS = 100/);
  assert.match(anchored, /setAnchoredResult\(mapExplanationCard\(card\)\)/);
  assert.match(anchored, /setAnchoredStage\("result"\)/);
  assert.match(app, /anchoredResizePending/);
  assert.match(app, /anchoredResizeGeneration/);
  assert.match(app, /anchoredWindowPresented/);
  assert.match(app, /prepare_anchored_overlay_window/);
  assert.match(app, /setAnchoredPreparing\(true\)/);
  assert.match(app, /shouldPresent[\s\S]*present_anchored_overlay_window[\s\S]*resize_anchored_overlay_window/);
  assert.match(popover, /element\.style\.maxWidth = "none"/);
  assert.match(popover, /settleTimer = window\.setTimeout/);
  assert.match(app, /正在生成语境解释/);
  assert.doesNotMatch(lib, /AnchoredOverlayStage::Loading/);
  assert.doesNotMatch(lib, /READRAY_ANCHORED_OVERLAY_WAKE/);
  assert.match(lib, /fn set_native_window_opacity[\s\S]*SetLayeredWindowAttributes/);
  assert.match(lib, /fn show_anchored_measurement_window[\s\S]*set_native_window_opacity\(window, 0\)[\s\S]*set_ignore_cursor_events\(true\)[\s\S]*window\.show\(\)/);
  assert.match(lib, /preparation_anchor[\s\S]*show_anchored_measurement_window[\s\S]*READRAY_ANCHORED_OVERLAY_PREPARE=ok/);
  assert.match(lib, /fn show_and_focus[\s\S]*set_native_window_opacity\(window, u8::MAX\)[\s\S]*set_ignore_cursor_events\(false\)/);
  assert.match(lib, /fn present_anchored_loading_window[\s\S]*LogicalSize::new\(ANCHORED_LOADING_WIDTH, ANCHORED_LOADING_HEIGHT\)[\s\S]*set_ignore_cursor_events\(true\)[\s\S]*set_native_window_opacity\(&window, u8::MAX\)[\s\S]*window\.show\(\)/);
  assert.doesNotMatch(
    section(lib, "fn present_anchored_loading_window", "fn present_anchored_overlay_window"),
    /show_and_focus|set_focus/,
  );
  assert.match(lib, /fn present_anchored_overlay_window[\s\S]*place_anchored_overlay_window[\s\S]*show_and_focus/);
  assert.match(lib, /READRAY_ANCHORED_OVERLAY_PRESENT=ok width=/);
  assert.match(styles, /html\.is-anchored-preparing[\s\S]*visibility:\s*hidden\s*!important[\s\S]*pointer-events:\s*none\s*!important/);
});

test("解释卡片原窗口晋升为固定卡，后台补建下一查询窗口", async () => {
  const [app, popover, drag, pinned, lib, styles] = await Promise.all([
    readFile("src/App.tsx", "utf8"),
    readFile("src/components/AnchoredResultPopover.tsx", "utf8"),
    readFile("src/overlayWindowDrag.ts", "utf8"),
    readFile("src-tauri/src/pinned_cards.rs", "utf8"),
    readFile("src-tauri/src/lib.rs", "utf8"),
    readFile("src/App.css", "utf8"),
  ]);
  const pinAction = section(
    app,
    "const pinAnchoredCard",
    "const consumeOverlayIntent",
  );
  const promotion = section(
    pinned,
    "pub(crate) async fn promote_overlay_to_pinned_card",
    "fn ensure_pinned_card_window",
  );
  const pinnedClose = section(
    pinned,
    "pub(crate) fn close_pinned_card",
    "pub(crate) fn begin_pinned_card_drag",
  );
  const focusLost = section(
    lib,
    "WindowEvent::Focused(false)",
    ".invoke_handler(tauri::generate_handler![",
  );

  assert.match(
    pinAction,
    /invoke\("promote_overlay_to_pinned_card", \{[\s\S]*card: anchoredCard[\s\S]*sourceWindowHwnd: anchoredSourceWindowHwnd\.current/,
  );
  assert.doesNotMatch(
    pinAction,
    /create_explanation_card|save_for_app|learning_record|DeepSeek|provider_request/,
  );
  assert.match(pinAction, /setAnchoredPinned\(true\)/);
  assert.match(pinAction, /anchoredResizeGeneration\.current \+= 1/);
  assert.doesNotMatch(app, /function PinnedCardApp|view === "pinned-card"/);
  assert.doesNotMatch(
    app,
    /get_pinned_card_payload|present_pinned_card_window|complete_pinned_card_handoff/,
  );
  assert.match(app, /onMouseDownCapture=\{handlePromotedPinnedMouseDownCapture\}/);
  assert.match(app, /onDoubleClick=\{anchoredPinned \? closePromotedPinnedCard/);
  assert.match(app, /current\.at - previous\.at <= 400/);
  assert.match(app, /Math\.abs\(current\.screenX - previous\.screenX\) <= 6/);
  assert.match(app, /anchoredPinned[\s\S]*pinnedCardDragCommands[\s\S]*anchoredWindowDragCommands/);
  assert.match(app, /anchoredPinned \? undefined : handleAnchoredContentSizeChange/);
  assert.match(app, /pinned: anchoredPinned \|\| anchoredPinPending/);
  assert.doesNotMatch(app, /pending: anchoredPinPending/);
  assert.match(app, /onChange: anchoredPinned[\s\S]*closePromotedPinnedCard/);

  assert.match(popover, /event\.key === "Escape"[\s\S]*onOpenChange\(false\)/);
  assert.match(popover, /className=\{`anchored-pin-button/);
  assert.match(popover, /aria-pressed=\{pinControl\.pinned\}/);
  assert.doesNotMatch(popover, /disabled=\{pinControl\.pending\}/);
  assert.doesNotMatch(popover, /title=\{pinControl|固定成功|已固定/);
  assert.match(drag, /begin: "begin_pinned_card_drag"/);
  assert.match(drag, /drag: "drag_pinned_card"/);
  assert.match(drag, /finish: "finish_pinned_card_drag"/);

  assert.match(pinned, /const PINNED_CARD_LIMIT: usize = 8/);
  assert.match(pinned, /pub\(crate\) async fn promote_overlay_to_pinned_card/);
  assert.match(pinned, /ACTIVE_OVERLAY_LABEL/);
  assert.match(pinned, /HashMap<String, PinnedCardEntry>/);
  assert.match(promotion, /let pinned_label = window\.label\(\)\.to_string\(\)/);
  assert.match(promotion, /WebviewWindowBuilder::new\([\s\S]*index\.html\?view=overlay/);
  assert.doesNotMatch(promotion, /view=pinned-card/);
  assert.match(promotion, /\.always_on_top\(true\)/);
  assert.match(promotion, /\.skip_taskbar\(true\)/);
  assert.match(promotion, /\.visible\(false\)/);
  assert.ok(
    promotion.indexOf("cards.insert") <
      promotion.indexOf("WebviewWindowBuilder::new"),
  );
  assert.ok(
    promotion.indexOf("WebviewWindowBuilder::new") <
      promotion.indexOf("*active_label = next_overlay_label.clone()"),
  );
  assert.doesNotMatch(promotion, /window\.hide\(\)|window\.close\(\)|window\.set_size/);
  assert.match(promotion, /window\.set_title\("ReadRay 固定解释"\)/);
  assert.match(promotion, /restore_source_window_focus/);
  assert.match(pinned, /fn restore_source_before_selection_capture/);
  assert.match(lib, /restore_source_before_selection_capture\(app\)[\s\S]*Duration::from_millis\(24\)[\s\S]*capture_foreground_with_retry/);
  assert.match(pinnedClose, /window\.hide\(\)[\s\S]*forget_pinned_card/);
  assert.match(pinnedClose, /std::thread::spawn[\s\S]*PINNED_CARD_CLOSE_DELAY_MS[\s\S]*closing_window\.close\(\)/);
  assert.match(
    focusLost,
    /is_pinned_card_window\(window\.label\(\)\)[\s\S]*PINNED_CARD_FOCUS=lost_ignored[\s\S]*is_active_overlay_label\(window\.label\(\)\)[\s\S]*overlay_focus_grace_active/,
  );
  assert.match(lib, /active_overlay_window\(app\)[\s\S]*show_anchored_measurement_window/);
  assert.match(lib, /active_overlay_label\(\)[\s\S]*emit_to\(label\.as_str\(\)/);
  assert.match(lib, /CloseRequested \{ \.\. \}[\s\S]*is_pinned_card_window[\s\S]*forget_pinned_card/);
  assert.match(lib, /promote_overlay_to_pinned_card/);

  assert.match(styles, /\.anchored-pin-button svg[\s\S]*rotate\(-14deg\)/);
  assert.match(styles, /\.anchored-pin-button\.is-pinned[\s\S]*var\(--rr-color-amber\)/);
  assert.match(styles, /\.anchored-pin-button\.is-pinned svg[\s\S]*rotate\(0deg\)/);
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
