import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  RepositoryConversationService,
  mapQuickAiConversation,
} from "../src/conversationService.ts";
import { TauriConversationRepository } from "../src/conversationRepository.ts";
import {
  conversationExportUnavailableReason,
  conversationTitleEditAction,
  isActiveConversationOperation,
  isConversationOperationCurrent,
  shouldResetDeletedConversation,
} from "../src/conversationViewModel.ts";

const NOW = new Date(2026, 6, 30, 12, 0, 0);

function snapshot(messages = [], overrides = {}) {
  return {
    id: 17,
    title: "真实会话",
    model: "deepseek-v4-flash",
    origin: "main",
    createdAtUnixMs: NOW.getTime(),
    updatedAtUnixMs: NOW.getTime(),
    messages,
    ...overrides,
  };
}

function message(id, role, content, sequence) {
  return {
    id,
    conversationId: 17,
    role,
    content,
    sequence,
    createdAtUnixMs: NOW.getTime(),
  };
}

// 注入式 repository 的流式实现：把 sendStreaming 转接到 send 语义，
// 并保留 onEvent 回调以模拟 channel 事件。
function withStreaming(repository, options = {}) {
  return {
    ...repository,
    sendStreaming: async (
      conversationId,
      expectedUserSequence,
      content,
      onEvent,
    ) => {
      const result = await repository.send(
        conversationId,
        expectedUserSequence,
        content,
      );
      if (options.emitDelta) {
        for (const piece of options.emitDelta) {
          onEvent({ type: "delta", text: piece });
        }
      }
      if (options.stopped) {
        onEvent({ type: "stopped" });
      }
      if (options.truncated) {
        onEvent({ type: "truncated" });
      }
      return result;
    },
    abortStreaming:
      options.abortStreaming ??
      repository.abortStreaming ??
      (async () => undefined),
  };
}

test("正式 Tauri 会话路径不读取 fixture 或 localStorage", async () => {
  const formalFiles = [
    "src/components/ConversationPage.tsx",
    "src/components/ConversationHistoryPage.tsx",
    "src/conversationRepository.ts",
    "src/conversationService.ts",
  ];
  for (const file of formalFiles) {
    const content = await readFile(file, "utf8");
    assert.doesNotMatch(
      content,
      /conversationFixtureService|FixtureConversationService|localStorage/,
    );
  }

  const app = await readFile("src/App.tsx", "utf8");
  const previewBranch = app.slice(
    app.indexOf("if (isTauriRuntime) {", app.indexOf("function MainAppWindow")),
    app.indexOf(
      "return () => {",
      app.indexOf('import("./conversationFixtureService")'),
    ),
  );
  assert.match(previewBranch, /if \(isTauriRuntime\) \{\s*return;/);
  assert.match(previewBranch, /import\("\.\/conversationFixtureService"\)/);
  assert.match(app, /new RepositoryConversationService\(\s*new TauriConversationRepository\(\)/);
});

test("主应用稳定会话身份回调并复用统一 composer", async () => {
  const shell = await readFile("src/components/MainAppShell.tsx", "utf8");
  const conversationPage = await readFile(
    "src/components/ConversationPage.tsx",
    "utf8",
  );
  const conversationStyles = await readFile(
    "src/styles/conversation-page.css",
    "utf8",
  );

  assert.match(shell, /const updateActiveConversation = useCallback\(/);
  assert.match(shell, /onThreadIdentityChange=\{updateActiveConversation\}/);
  assert.match(conversationPage, /className="rr-main-composer-area"/);
  assert.match(conversationPage, /className="rr-main-composer"/);
  assert.match(
    conversationPage,
    /rr-conversation-scroll-to-bottom\$\{showScrollToBottom \? " is-visible" : ""\}/,
  );
  assert.match(conversationPage, /aria-label="滚动到底部"/);
  assert.match(conversationPage, /scrollToBottom/);
  assert.match(conversationPage, /requestAnimationFrame\(step\)/);
  assert.match(conversationPage, /1 - Math\.pow\(1 - progress, 3\)/);
  assert.match(conversationPage, /distanceFromBottom > 96/);
  assert.match(conversationPage, /distanceFromBottom < 24/);
  assert.match(conversationPage, /conversationCreationRef/);
  assert.match(conversationPage, /cachedCreation\?\.requestKey === request\.key/);
  assert.match(
    conversationStyles,
    /--rr-conversation-content-width:[\s\S]*?\.rr-conversation-page \.rr-main-composer-inner/,
  );
  assert.match(conversationPage, /rr-conversation-scroll-wrap/);
  assert.match(conversationStyles, /\.rr-conversation-scroll-wrap \{[\s\S]*?position: relative;/);
  assert.match(conversationStyles, /\.rr-conversation-scroll \{[\s\S]*?position: relative;/);
  assert.match(
    conversationStyles,
    /\.rr-conversation-scroll-to-bottom \{[\s\S]*?opacity: 0;[\s\S]*?pointer-events: none;/,
  );
  assert.match(
    conversationStyles,
    /\.rr-conversation-scroll-to-bottom\.is-visible \{[\s\S]*?opacity: 1;[\s\S]*?pointer-events: auto;/,
  );
  assert.match(conversationStyles, /\.rr-conversation-scroll-to-bottom svg \{[\s\S]*?transform: rotate\(180deg\);/);
  assert.match(
    conversationStyles,
    /\.rr-conversation-scroll-to-bottom \{[\s\S]*?left: 50%;[\s\S]*?bottom: 16px;[\s\S]*?border-radius: 50%;/,
  );
  assert.match(
    conversationStyles,
    /\.rr-conversation-scroll-to-bottom \{[\s\S]*?transform: translateX\(-50%\) translateY\(4px\);/,
  );
  assert.doesNotMatch(
    conversationPage,
    /className="rr-main-send rr-conversation-scroll-to-bottom"/,
  );
  assert.match(
    conversationStyles,
    /\.rr-conversation-user-bubble \{[\s\S]*?font-size: calc\(14px \* var\(--rr-ui-font-scale\)\);[\s\S]*?line-height: 1\.5/,
  );
  assert.match(
    conversationStyles,
    /\.rr-conversation-assistant-copy \{[\s\S]*?font-size: calc\(var\(--rr-learning-font-size\) - 2px\);[\s\S]*?line-height: 1\.6/,
  );
  assert.match(conversationPage, /function ConversationGenerationIndicator/);
  assert.match(conversationPage, /className="rr-conversation-pixel-grid"/);
  assert.match(conversationPage, /onStop=\{canStop \? onStop : undefined\}/);
  assert.match(conversationStyles, /\.rr-conversation-generation-indicator \{/);
  assert.match(conversationStyles, /@keyframes rr-conversation-pixel-on/);
  assert.match(conversationStyles, /@keyframes rr-conversation-shimmer/);
  assert.match(conversationStyles, /prefers-reduced-motion/);
  assert.doesNotMatch(conversationPage, /rr-conversation-stream-line/);
  assert.doesNotMatch(conversationStyles, /rr-conversation-stream-line/);
  assert.doesNotMatch(conversationPage, /rr-conversation-composer/);
  assert.doesNotMatch(conversationStyles, /rr-conversation-composer/);
});

test("assistant 回答悬停浮现复制按钮且优先复制原始 Markdown", async () => {
  const conversationPage = await readFile(
    "src/components/ConversationPage.tsx",
    "utf8",
  );
  const conversationStyles = await readFile(
    "src/styles/conversation-page.css",
    "utf8",
  );

  assert.match(conversationPage, /rr-conversation-assistant-copy-button/);
  assert.match(conversationPage, /renderAnswerText\(message\)/);
  assert.match(conversationPage, /message\.markdown !== undefined/);
  assert.match(conversationPage, /writeText\(/);
  assert.match(conversationPage, /已复制/);
  assert.match(conversationPage, /<path d="m5 12 4 4L19 6" \/>/);
  assert.match(
    conversationStyles,
    /\.rr-conversation-message \{\n  position: relative;\n  margin-top: 24px;\n\}/,
  );
  assert.match(
    conversationStyles,
    /\.rr-conversation-assistant-copy-button \{[\s\S]*?opacity: 0;[\s\S]*?pointer-events: none;/,
  );
  assert.match(
    conversationStyles,
    /\.rr-conversation-message\.is-assistant:hover[\s\S]*?\.rr-conversation-assistant-copy-button:hover,[\s\S]*?\.rr-conversation-assistant-copy-button:focus-visible \{[\s\S]*?opacity: 1;[\s\S]*?pointer-events: auto;/,
  );
  assert.match(
    conversationStyles,
    /\.rr-conversation-message\.is-assistant::after \{[\s\S]*?position: absolute;[\s\S]*?right: 0;[\s\S]*?bottom: -38px;[\s\S]*?height: 38px/,
  );
  assert.match(
    conversationStyles,
    /\.rr-conversation-assistant-copy-button \{[\s\S]*?position: absolute;[\s\S]*?left: 0;[\s\S]*?bottom: -/,
  );
});

test("全部对话使用搜索、最近/其他分组和紧凑固定行", async () => {
  const historyPage = await readFile(
    "src/components/ConversationHistoryPage.tsx",
    "utf8",
  );
  const historyStyles = await readFile(
    "src/styles/conversation-page.css",
    "utf8",
  );
  const rustConversations = await readFile(
    "src-tauri/src/conversations.rs",
    "utf8",
  );

  assert.match(historyPage, /placeholder="搜索对话"/);
  assert.match(historyPage, /label="最近对话"/);
  assert.match(historyPage, /label="其他对话"/);
  assert.match(historyPage, /label: "标题"/);
  assert.match(historyPage, /role="combobox"/);
  assert.doesNotMatch(historyPage, /保留你的提问与学习记录/);
  assert.match(historyStyles, /grid-template-columns: minmax\(0, 1fr\) 20px/);
  assert.match(historyStyles, /grid-template-rows: auto auto/);
  assert.match(historyStyles, /text-overflow: clip/);
  assert.match(historyStyles, /font-size: calc\(14px \* var\(--rr-ui-font-scale\)\);\n  font-weight: 500/);
  assert.match(historyStyles, /\.rr-conversation-history-date \{[\s\S]*?font-family: var\(--rr-main-font-display\)/);
  assert.match(rustConversations, /const AUTO_TITLE_LEN: usize = 18/);
  assert.match(rustConversations, /\.take\(AUTO_TITLE_LEN\)/);
});

test("Tauri repository 使用既有 Quick AI commands 和 camelCase 参数", async () => {
  const calls = [];
  const saveRequests = [];
  const repository = new TauriConversationRepository(
    async (command, args) => {
      calls.push({ command, args });
      if (command === "get_quick_ai_conversation") {
        return snapshot();
      }
      return snapshot();
    },
    async (options) => {
      saveRequests.push(options);
      return "D:\\Exports\\真实会话.md";
    },
  );

  await repository.create();
  await repository.get(17);
  await repository.list();
  await repository.rename(17, "新名称");
  await repository.delete(17);
  await repository.export(17, "真实会话.md");
  await repository.send(17, 3, "继续问题");

  assert.deepEqual(calls, [
    { command: "create_quick_ai_conversation", args: { origin: "main" } },
    {
      command: "get_quick_ai_conversation",
      args: { conversationId: 17 },
    },
    { command: "list_all_quick_ai_conversations", args: undefined },
    {
      command: "rename_quick_ai_conversation",
      args: { conversationId: 17, title: "新名称" },
    },
    {
      command: "delete_quick_ai_conversation",
      args: { conversationId: 17 },
    },
    {
      command: "export_quick_ai_conversation",
      args: {
        conversationId: 17,
        filePath: "D:\\Exports\\真实会话.md",
      },
    },
    {
      command: "send_quick_ai_message",
      args: {
        conversationId: 17,
        expectedUserSequence: 3,
        content: "继续问题",
      },
    },
  ]);
  assert.deepEqual(saveRequests, [
    {
      title: "导出 ReadRay 对话",
      defaultPath: "真实会话.md",
      filters: [{ name: "Markdown", extensions: ["md"] }],
    },
  ]);
});

test("主窗口最近会话只查询 main，全部历史保留 main 与 overlay", async () => {
  const todayRepository = await readFile("src/todayRepository.ts", "utf8");
  const historyPage = await readFile(
    "src/components/ConversationHistoryPage.tsx",
    "utf8",
  );
  const quickAiTypes = await readFile("src/types/quickAi.ts", "utf8");

  assert.match(
    todayRepository,
    /list_recent_quick_ai_conversations[\s\S]*?\{ limit, origin: "main" \}/,
  );
  assert.match(historyPage, /\["main", "主窗口"\]/);
  assert.match(historyPage, /\["overlay", "Quick AI"\]/);
  assert.doesNotMatch(historyPage, /legacy|旧会话/);
  assert.match(
    quickAiTypes,
    /ConversationOrigin = "overlay" \| "main";/,
  );
});

test("原生保存对话框取消后不调用 Rust 导出 command", async () => {
  const calls = [];
  const repository = new TauriConversationRepository(
    async (command, args) => {
      calls.push({ command, args });
      return snapshot();
    },
    async () => null,
  );

  const result = await repository.export(17, "真实会话.md");

  assert.equal(result, null);
  assert.deepEqual(calls, []);
});

test("迟到重命名和删除必须同时匹配挂载、请求与会话身份", () => {
  const operation = { requestKey: 8, conversationId: "17" };

  assert.equal(isConversationOperationCurrent(true, operation, 8, "17"), true);
  assert.equal(isConversationOperationCurrent(false, operation, 8, "17"), false);
  assert.equal(isConversationOperationCurrent(true, operation, 9, "17"), false);
  assert.equal(isConversationOperationCurrent(true, operation, 8, "23"), false);
  assert.equal(
    shouldResetDeletedConversation("conversation", "17", 8, operation),
    true,
  );
  for (const page of ["today", "memory", "writing", "conversation-history"]) {
    assert.equal(
      shouldResetDeletedConversation(page, "17", 8, operation),
      false,
      `迟到删除不得从 ${page} 返回会话页`,
    );
  }
  assert.equal(
    shouldResetDeletedConversation("conversation", "23", 8, operation),
    false,
  );
  assert.equal(
    shouldResetDeletedConversation("conversation", "17", 9, operation),
    false,
  );
  assert.equal(
    isActiveConversationOperation("conversation", "17", 8, operation),
    true,
  );
  assert.equal(isActiveConversationOperation("today", "17", 8, operation), false);
});

test("会话列表仅由左键打开，右键统一进入三项管理菜单", async () => {
  const sidebar = await readFile("src/components/MainSidebar.tsx", "utf8");
  const history = await readFile(
    "src/components/ConversationHistoryPage.tsx",
    "utf8",
  );
  const menu = await readFile(
    "src/components/ConversationManagementMenu.tsx",
    "utf8",
  );

  assert.match(sidebar, /onClick=\{\(\) => onRecentConversationSelect/);
  assert.match(sidebar, /onContextMenu=\{\(event\) =>\s*onRecentConversationContextMenu/);
  assert.doesNotMatch(sidebar, /暂无 Quick AI 对话/);
  assert.match(history, /onClick=\{\(\) => onOpenConversation/);
  assert.match(history, /onContextMenu=\{\(event\) =>\s*onConversationContextMenu/);
  assert.doesNotMatch(history, /rr-conversation-history-actions/);
  assert.doesNotMatch(`${sidebar}\n${history}`, /Shift\+F10|aria-label="更多操作"/);
  assert.match(menu, />\s*重命名\s*<\/button>/);
  assert.match(menu, />\s*导出\s*<\/button>/);
  assert.match(menu, />\s*删除\s*<\/button>/);

  const loadIndex = menu.indexOf("service.loadConversation(");
  const exportIndex = menu.indexOf("service.exportConversation(thread)");
  assert.ok(loadIndex >= 0 && exportIndex > loadIndex);
  assert.match(menu, /conversationExportUnavailableReason\(/);
  assert.match(menu, /isConversationOperationCurrent\(/);
});

test("当前会话标题原地编辑支持 Enter 保存、Esc 和失焦取消", async () => {
  assert.equal(conversationTitleEditAction("Enter", false), "save");
  assert.equal(conversationTitleEditAction("Escape", false), "cancel");
  assert.equal(conversationTitleEditAction("Enter", true), undefined);
  assert.equal(conversationTitleEditAction("Tab", false), undefined);

  const page = await readFile("src/components/ConversationPage.tsx", "utf8");
  assert.match(page, /className="rr-conversation-title-edit"/);
  assert.match(page, /onSubmit=\{renameConversation\}/);
  assert.match(page, /onBlur=\{cancelInlineRename\}/);
  assert.match(page, /conversationTitleEditAction\(/);
  assert.doesNotMatch(page, /rr-conversation-rename-title/);
});

test("空白会话导出在按钮判断和 service 入口均被阻断", async () => {
  let exportCalls = 0;
  const service = new RepositoryConversationService({
    create: async () => snapshot(),
    get: async () => snapshot(),
    list: async () => [],
    rename: async () => snapshot(),
    delete: async () => true,
    export: async () => {
      exportCalls += 1;
      throw new Error("空白会话不应打开保存对话框");
    },
    send: async () => snapshot(),
  });
  const emptyThread = { id: "17", title: "新对话", messages: [] };

  assert.equal(
    conversationExportUnavailableReason(emptyThread, false, true),
    "空白会话没有可导出的消息",
  );
  assert.equal(
    conversationExportUnavailableReason(emptyThread, false, false),
    "空白会话没有可导出的消息",
  );
  assert.equal(
    conversationExportUnavailableReason(emptyThread, true, true),
    "回答生成期间不能导出",
  );
  assert.deepEqual(await service.exportConversation(emptyThread), {
    exported: false,
    reason: "unavailable",
  });
  assert.equal(exportCalls, 0);
});

test("Rust 会话快照映射为现有页面模型", () => {
  const thread = mapQuickAiConversation(
    snapshot([
      message(41, "user", "context 怎么用？", 1),
      message(42, "assistant", "Use it in this context.", 2),
    ]),
    NOW,
  );

  assert.equal(thread.id, "17");
  assert.equal(thread.title, "真实会话");
  assert.deepEqual(thread.messages[0], {
    id: "quick-ai-message-41",
    role: "user",
    content: "context 怎么用？",
    meta: "今天 · 12:00",
    sequence: 1,
  });
  assert.equal(thread.messages[1].role, "assistant");
  assert.equal(thread.messages[1].sequence, 2);
  assert.equal(thread.messages[1].blocks[0].content[0].text, "Use it in this context.");
});

test("以 user 结尾的快照映射为稳定 pending 轮次", () => {
  const thread = mapQuickAiConversation(
    snapshot([message(71, "user", "重启后继续回答", 1)]),
    NOW,
  );

  assert.deepEqual(thread.pendingTurn, {
    userMessageId: "quick-ai-message-71",
    prompt: "重启后继续回答",
    expectedUserSequence: 1,
  });
});

test("发送成功返回数据库权威快照而不是前端复制消息", async () => {
  let updated = 0;
  const saved = snapshot([
    message(41, "user", "第一问", 1),
    message(42, "assistant", "第一答", 2),
  ]);
  const service = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => snapshot(),
      send: async (conversationId, expectedUserSequence, content) => {
        assert.equal(conversationId, 17);
        assert.equal(expectedUserSequence, 1);
        assert.equal(content, "第一问");
        return saved;
      },
    }),
    { onConversationUpdated: () => updated += 1 },
  );

  const reply = await service.generateReply({
    conversationId: "17",
    messages: [
      {
        id: "temporary-user",
        role: "user",
        content: "第一问",
      },
    ],
    prompt: "第一问",
    mode: "append",
  });

  assert.equal(reply.status, "complete");
  assert.equal(reply.assistantMessageId, "quick-ai-message-42");
  assert.equal(reply.persistedThread.messages.length, 2);
  assert.equal(updated, 1);
});

test("请求失败可用同一输入重试且 service 不复制前端消息", async () => {
  let attempts = 0;
  let updated = 0;
  const expectedSequences = [];
  const saved = snapshot([
    message(51, "user", "保留这条输入", 1),
    message(52, "assistant", "重试成功", 2),
  ]);
  const service = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => snapshot(),
      send: async (_conversationId, expectedUserSequence) => {
        expectedSequences.push(expectedUserSequence);
        attempts += 1;
        if (attempts === 1) {
          throw new Error("模型请求失败");
        }
        return saved;
      },
    }),
    { onConversationUpdated: () => updated += 1 },
  );
  const request = {
    conversationId: "17",
    messages: [
      {
        id: "temporary-user",
        role: "user",
        content: "保留这条输入",
      },
    ],
    prompt: "保留这条输入",
    mode: "append",
  };

  await assert.rejects(service.generateReply(request), /模型请求失败/);
  const reply = await service.generateReply(request);

  assert.equal(attempts, 2);
  assert.deepEqual(expectedSequences, [1, 1]);
  assert.equal(updated, 2);
  assert.deepEqual(
    reply.persistedThread.messages.map((item) => item.role),
    ["user", "assistant"],
  );
});

test("IPC 报错但数据库已完成该轮时读取权威快照避免重复重试", async () => {
  let updated = 0;
  const saved = snapshot([
    message(61, "user", "只保存一次", 1),
    message(62, "assistant", "已经保存", 2),
  ]);
  const service = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => saved,
      send: async () => {
        throw new Error("响应回传失败");
      },
    }),
    { onConversationUpdated: () => updated += 1 },
  );

  const reply = await service.generateReply({
    conversationId: "17",
    messages: [
      {
        id: "temporary-user",
        role: "user",
        content: "只保存一次",
      },
    ],
    prompt: "只保存一次",
    mode: "append",
  });

  assert.equal(reply.status, "complete");
  assert.equal(reply.persistedThread.messages.length, 2);
  assert.equal(reply.assistantMessageId, "quick-ai-message-62");
  assert.equal(updated, 1);
});

test("模型失败后返回已持久化 pending，重启重试复用同一 sequence", async () => {
  const pending = snapshot([message(81, "user", "失败后仍保留", 1)]);
  const completed = snapshot([
    message(81, "user", "失败后仍保留", 1),
    message(82, "assistant", "重启后完成", 2),
  ]);
  const firstService = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => pending,
      send: async () => {
        throw new Error("模型请求失败");
      },
    }),
  );
  const firstResult = await firstService.generateReply({
    conversationId: "17",
    messages: [
      {
        id: "temporary-user",
        role: "user",
        content: "失败后仍保留",
      },
    ],
    prompt: "失败后仍保留",
    mode: "append",
  });

  assert.equal(firstResult.status, "pending");
  assert.equal(firstResult.persistedThread.messages[0].id, "quick-ai-message-81");
  const retriedSequences = [];
  const restartedService = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => pending,
      send: async (_conversationId, expectedUserSequence) => {
        retriedSequences.push(expectedUserSequence);
        return completed;
      },
    }),
  );
  const retryResult = await restartedService.generateReply({
    conversationId: "17",
    messages: firstResult.persistedThread.messages,
    prompt: firstResult.persistedThread.pendingTurn.prompt,
    mode: "append",
  });

  assert.equal(retryResult.status, "complete");
  assert.deepEqual(retriedSequences, [1]);
  assert.deepEqual(
    retryResult.persistedThread.messages.map((item) => item.role),
    ["user", "assistant"],
  );
});

test("提交成功但 IPC 与随后读取均失败时再次重试仍使用原 sequence", async () => {
  const request = {
    conversationId: "17",
    messages: [
      {
        id: "temporary-user",
        role: "user",
        content: "模糊成功路径",
      },
    ],
    prompt: "模糊成功路径",
    mode: "append",
  };
  const unconfirmedService = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => {
        throw new Error("随后读取也失败");
      },
      send: async () => {
        throw new Error("IPC 回传失败");
      },
    }),
  );
  await assert.rejects(
    unconfirmedService.generateReply(request),
    /IPC 回传失败/,
  );

  const committed = snapshot([
    message(91, "user", "模糊成功路径", 1),
    message(92, "assistant", "数据库已经提交", 2),
  ]);
  const retrySequences = [];
  const retryService = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => committed,
      send: async (_conversationId, expectedUserSequence) => {
        retrySequences.push(expectedUserSequence);
        return committed;
      },
    }),
  );
  const result = await retryService.generateReply(request);

  assert.equal(result.status, "complete");
  assert.deepEqual(retrySequences, [1]);
  assert.equal(result.persistedThread.messages.length, 2);
});

test("缺失会话和真实后端不支持的重新生成会明确失败", async () => {
  const service = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => null,
      send: async () => snapshot(),
    }),
  );

  await assert.rejects(
    service.loadConversation("17", "旧标题"),
    /不存在或已无法读取/,
  );
  await assert.rejects(
    service.generateReply({
      conversationId: "17",
      messages: [],
      prompt: "重生成",
      mode: "regenerate",
    }),
    /暂不支持重新生成/,
  );
});

test("全部会话、重命名和删除只使用数据库 ID 并在成功后刷新", async () => {
  const calls = [];
  let updated = 0;
  const service = new RepositoryConversationService(
    {
      create: async () => snapshot(),
      get: async () => snapshot(),
      list: async () => [
        {
          id: 17,
          title: "  第一段历史  ",
          origin: "main",
          updatedAtUnixMs: NOW.getTime(),
        },
        {
          id: 23,
          title: "第二段历史",
          origin: "overlay",
          updatedAtUnixMs: NOW.getTime() - 1,
        },
      ],
      rename: async (conversationId, title) => {
        calls.push({ operation: "rename", conversationId, title });
        return snapshot(
          [
            message(41, "user", "第一问", 1),
            message(42, "assistant", "第一答", 2),
          ],
          { id: conversationId, title },
        );
      },
      delete: async (conversationId) => {
        calls.push({ operation: "delete", conversationId });
        return true;
      },
      export: async () => null,
      send: async () => snapshot(),
    },
    { onConversationUpdated: () => updated += 1 },
  );

  const all = await service.listConversations();
  const renamed = await service.renameConversation("17", "  新名称  ");
  await service.deleteConversation("23");

  assert.deepEqual(all, [
    {
      id: "17",
      title: "第一段历史",
      origin: "main",
      updatedAtUnixMs: NOW.getTime(),
    },
    {
      id: "23",
      title: "第二段历史",
      origin: "overlay",
      updatedAtUnixMs: NOW.getTime() - 1,
    },
  ]);
  assert.equal(renamed.id, "17");
  assert.equal(renamed.title, "新名称");
  assert.deepEqual(calls, [
    { operation: "rename", conversationId: 17, title: "新名称" },
    { operation: "delete", conversationId: 23 },
  ]);
  assert.equal(updated, 2);
});

test("重命名或删除失败不刷新且可以安全重试", async () => {
  let renameAttempts = 0;
  let deleteAttempts = 0;
  let updated = 0;
  const service = new RepositoryConversationService(
    {
      create: async () => snapshot(),
      get: async () => snapshot(),
      list: async () => [],
      rename: async (_conversationId, title) => {
        renameAttempts += 1;
        if (renameAttempts === 1) {
          throw new Error("模拟重命名失败");
        }
        return snapshot(
          [
            message(41, "user", "第一问", 1),
            message(42, "assistant", "第一答", 2),
          ],
          { title },
        );
      },
      delete: async () => {
        deleteAttempts += 1;
        return deleteAttempts > 1;
      },
      export: async () => null,
      send: async () => snapshot(),
    },
    { onConversationUpdated: () => updated += 1 },
  );

  await assert.rejects(
    service.renameConversation("17", "重试名称"),
    /模拟重命名失败/,
  );
  const renamed = await service.renameConversation("17", "重试名称");
  await assert.rejects(service.deleteConversation("17"), /已经被删除/);
  await service.deleteConversation("17");

  assert.equal(renamed.title, "重试名称");
  assert.equal(renameAttempts, 2);
  assert.equal(deleteAttempts, 2);
  assert.equal(updated, 2);
});

test("正式导出只提交目标数据库 ID，取消不返回成功，失败可重试", async () => {
  const exports = [];
  let attempts = 0;
  const service = new RepositoryConversationService({
    create: async () => snapshot(),
    get: async () => snapshot(),
    list: async () => [],
    rename: async () => snapshot(),
    delete: async () => true,
    export: async (conversationId, suggestedFileName) => {
      exports.push({ conversationId, suggestedFileName });
      attempts += 1;
      if (attempts === 1) {
        return null;
      }
      if (attempts === 2) {
        throw new Error("模拟写文件失败");
      }
      return {
        conversationId,
        fileName: "真实会话.md",
        filePath: "D:\\Exports\\真实会话.md",
        messageCount: 2,
      };
    },
    send: async () => snapshot(),
  });
  const stalePageThread = {
    id: "17",
    title: "真实:会话",
    messages: [{ id: "temporary", role: "user", content: "页面临时消息" }],
  };

  const cancelled = await service.exportConversation(stalePageThread);
  await assert.rejects(
    service.exportConversation(stalePageThread),
    /模拟写文件失败/,
  );
  const exported = await service.exportConversation(stalePageThread);

  assert.deepEqual(cancelled, { exported: false, reason: "cancelled" });
  assert.deepEqual(exported, {
    exported: true,
    fileName: "真实会话.md",
    messageCount: 2,
    nativeFilePath: "D:\\Exports\\真实会话.md",
  });
  assert.deepEqual(exports, [
    { conversationId: 17, suggestedFileName: "真实-会话.md" },
    { conversationId: 17, suggestedFileName: "真实-会话.md" },
    { conversationId: 17, suggestedFileName: "真实-会话.md" },
  ]);
});

test("流式回答达到长度上限时返回 truncated 且保留已生成内容", async () => {
  const saved = snapshot([
    message(101, "user", "超长问题", 1),
    message(102, "assistant", "已生成的前半部分", 2),
  ]);
  const service = new RepositoryConversationService(
    withStreaming(
      {
        create: async () => snapshot(),
        get: async () => saved,
        send: async () => saved,
      },
      { emitDelta: ["已生成的前半部分"], truncated: true },
    ),
  );

  const reply = await service.generateReply({
    conversationId: "17",
    messages: [{ id: "temporary-user", role: "user", content: "超长问题" }],
    prompt: "超长问题",
    mode: "append",
  });

  assert.equal(reply.status, "truncated");
  assert.equal(reply.assistantMessageId, "quick-ai-message-102");
  assert.deepEqual(reply.chunks, ["已生成的前半部分"]);
  assert.equal(reply.persistedThread.messages.length, 2);
});

test("流式增量通过 onStreamDelta 转发且完成后返回已保存回答", async () => {
  const deltas = [];
  const saved = snapshot([
    message(101, "user", "流式问题", 1),
    message(102, "assistant", "边生成边显示", 2),
  ]);
  const service = new RepositoryConversationService(
    withStreaming(
      {
        create: async () => snapshot(),
        get: async () => saved,
        send: async () => saved,
      },
      { emitDelta: ["边生成", "边显示"] },
    ),
  );

  const reply = await service.generateReply({
    conversationId: "17",
    messages: [
      { id: "temporary-user", role: "user", content: "流式问题" },
    ],
    prompt: "流式问题",
    mode: "append",
    onStreamDelta: (delta) => deltas.push(delta),
  });

  assert.equal(reply.status, "complete");
  assert.equal(reply.assistantMessageId, "quick-ai-message-102");
  assert.deepEqual(deltas, ["边生成", "边显示"]);
  assert.deepEqual(reply.chunks, ["边生成边显示"]);
  assert.equal(reply.persistedThread.messages.length, 2);
});

test("用户停止后 assistant 未落库时返回 pending 且可重试", async () => {
  const pending = snapshot([message(111, "user", "停止后重试", 1)]);
  const completed = snapshot([
    message(111, "user", "停止后重试", 1),
    message(112, "assistant", "重试完成", 2),
  ]);
  let streamAttempts = 0;
  const stopService = new RepositoryConversationService(
    withStreaming(
      {
        create: async () => snapshot(),
        get: async () => pending,
        send: async () => {
          streamAttempts += 1;
          throw new Error("回答已停止，已保留你的问题，可以直接重试。");
        },
      },
      { emitDelta: ["部分内容"] },
    ),
  );

  const stoppedResult = await stopService.generateReply({
    conversationId: "17",
    messages: [{ id: "temporary-user", role: "user", content: "停止后重试" }],
    prompt: "停止后重试",
    mode: "append",
  });
  assert.equal(stoppedResult.status, "pending");
  assert.match(stoppedResult.errorMessage, /回答已停止/);

  const retryService = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => pending,
      send: async () => {
        streamAttempts += 1;
        return completed;
      },
    }),
  );
  const retryResult = await retryService.generateReply({
    conversationId: "17",
    messages: [
      { id: "temporary-user", role: "user", content: "停止后重试" },
    ],
    prompt: "停止后重试",
    mode: "append",
  });

  assert.equal(streamAttempts, 2);
  assert.equal(retryResult.status, "complete");
  assert.deepEqual(
    retryResult.persistedThread.messages.map((item) => item.role),
    ["user", "assistant"],
  );
});

test("stopGeneration 通过 abort 命令请求后端停止", async () => {
  let aborts = 0;
  let abortConversationId;
  const service = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => snapshot(),
      send: async () => snapshot(),
      abortStreaming: async (conversationId) => {
        aborts += 1;
        abortConversationId = conversationId;
      },
    }),
  );

  await service.stopGeneration?.("17");

  assert.equal(aborts, 1);
  assert.equal(abortConversationId, 17);
});

test("正式 service 能力为流式可停止且导出仍可用", async () => {
  const service = new RepositoryConversationService(
    withStreaming({
      create: async () => snapshot(),
      get: async () => snapshot(),
      send: async () => snapshot(),
    }),
  );

  assert.equal(service.capabilities.delivery, "streaming");
  assert.equal(service.capabilities.canStop, true);
  assert.equal(service.capabilities.canRegenerate, false);
  assert.equal(service.capabilities.canExport, true);
});
