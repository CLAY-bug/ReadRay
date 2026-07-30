import assert from "node:assert/strict";
import test from "node:test";
import {
  RepositoryConversationService,
  mapQuickAiConversation,
} from "../src/conversationService.ts";
import { TauriConversationRepository } from "../src/conversationRepository.ts";

const NOW = new Date(2026, 6, 30, 12, 0, 0);

function snapshot(messages = [], overrides = {}) {
  return {
    id: 17,
    title: "真实会话",
    model: "deepseek-v4-flash",
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

test("Tauri repository 使用既有 Quick AI commands 和 camelCase 参数", async () => {
  const calls = [];
  const repository = new TauriConversationRepository(
    async (command, args) => {
      calls.push({ command, args });
      if (command === "get_quick_ai_conversation") {
        return snapshot();
      }
      return snapshot();
    },
  );

  await repository.create();
  await repository.get(17);
  await repository.send(17, 3, "继续问题");

  assert.deepEqual(calls, [
    { command: "create_quick_ai_conversation", args: undefined },
    {
      command: "get_quick_ai_conversation",
      args: { conversationId: 17 },
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
    {
      create: async () => snapshot(),
      get: async () => snapshot(),
      send: async (conversationId, expectedUserSequence, content) => {
        assert.equal(conversationId, 17);
        assert.equal(expectedUserSequence, 1);
        assert.equal(content, "第一问");
        return saved;
      },
    },
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
    {
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
    },
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
    {
      create: async () => snapshot(),
      get: async () => saved,
      send: async () => {
        throw new Error("响应回传失败");
      },
    },
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
  const firstService = new RepositoryConversationService({
    create: async () => snapshot(),
    get: async () => pending,
    send: async () => {
      throw new Error("模型请求失败");
    },
  });
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
  const restartedService = new RepositoryConversationService({
    create: async () => snapshot(),
    get: async () => pending,
    send: async (_conversationId, expectedUserSequence) => {
      retriedSequences.push(expectedUserSequence);
      return completed;
    },
  });
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
  const unconfirmedService = new RepositoryConversationService({
    create: async () => snapshot(),
    get: async () => {
      throw new Error("随后读取也失败");
    },
    send: async () => {
      throw new Error("IPC 回传失败");
    },
  });
  await assert.rejects(
    unconfirmedService.generateReply(request),
    /IPC 回传失败/,
  );

  const committed = snapshot([
    message(91, "user", "模糊成功路径", 1),
    message(92, "assistant", "数据库已经提交", 2),
  ]);
  const retrySequences = [];
  const retryService = new RepositoryConversationService({
    create: async () => snapshot(),
    get: async () => committed,
    send: async (_conversationId, expectedUserSequence) => {
      retrySequences.push(expectedUserSequence);
      return committed;
    },
  });
  const result = await retryService.generateReply(request);

  assert.equal(result.status, "complete");
  assert.deepEqual(retrySequences, [1]);
  assert.equal(result.persistedThread.messages.length, 2);
});

test("缺失会话和真实后端不支持的重新生成会明确失败", async () => {
  const service = new RepositoryConversationService({
    create: async () => snapshot(),
    get: async () => null,
    send: async () => snapshot(),
  });

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
