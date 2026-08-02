import assert from "node:assert/strict";
import test from "node:test";
import { TauriWritingRepository } from "../src/writingRepository.ts";
import {
  RepositoryWritingService,
  mapWritingDocument,
} from "../src/writingService.ts";
import { WritingDraftSaveCoordinator } from "../src/writingDraftSaveCoordinator.ts";
import { loadWritingLibrary } from "../src/writingLibraryLoader.ts";
import {
  captureWritingRequestIdentity,
  runGuardedWritingRequest,
  shouldHandleWritingShortcut,
} from "../src/writingRequestIdentity.ts";

const NOW = new Date(2026, 6, 30, 12, 0, 0).getTime();

function snapshot(title = "Real title", body = "Real body") {
  return { title, paragraphs: [body] };
}

function documentRecord(overrides = {}) {
  return {
    id: 17,
    revision: 2,
    createdAtUnixMs: NOW,
    updatedAtUnixMs: NOW,
    lastOpenedAtUnixMs: NOW,
    draftUpdatedAtUnixMs: NOW,
    draftSnapshot: snapshot(),
    comparisonBaselineRevision: 1,
    comparisonBaseline: snapshot("Baseline", "Baseline body"),
    versions: [],
    answers: [],
    ...overrides,
  };
}

function rustDraftDocument(overrides = {}) {
  return {
    id: 71,
    revision: 0,
    createdAtUnixMs: NOW,
    updatedAtUnixMs: NOW,
    lastOpenedAtUnixMs: null,
    draftUpdatedAtUnixMs: NOW,
    completedAtUnixMs: null,
    draftSnapshot: snapshot("", ""),
    completedSnapshot: null,
    comparisonBaselineRevision: 0,
    comparisonBaseline: snapshot("", ""),
    versions: [],
    activeAnalysis: null,
    baselineAnalysis: null,
    answers: [],
    ...overrides,
  };
}

function rustCompletedDocument(overrides = {}) {
  return rustDraftDocument({
    revision: 4,
    lastOpenedAtUnixMs: null,
    draftUpdatedAtUnixMs: null,
    completedAtUnixMs: NOW,
    draftSnapshot: null,
    completedSnapshot: snapshot("Completed", "Completed body"),
    comparisonBaselineRevision: 3,
    versions: [
      {
        id: 91,
        documentId: 71,
        ordinal: 1,
        sourceRevision: 3,
        analysisRevision: null,
        comparisonBaselineRevision: 3,
        snapshot: snapshot("Completed", "Completed body"),
        comparisonBaseline: snapshot("Baseline", "Baseline body"),
        issues: [],
        patterns: [],
        completedAtUnixMs: NOW,
      },
    ],
    activeAnalysis: null,
    answers: [
      {
        id: 101,
        documentId: 71,
        documentRevision: 3,
        versionId: 91,
        parentAnswerId: null,
        question: "Is this clear?",
        scope: "document",
        scopeLabel: "整篇文章",
        selectionText: null,
        title: "Answer",
        copy: "Yes.",
        map: null,
        createdAtUnixMs: NOW,
      },
    ],
    ...overrides,
  });
}

test("service 接受并规范化真实 Rust 新建草稿与完成稿中的 null", () => {
  const draft = mapWritingDocument(rustDraftDocument());
  assert.equal(draft.lastOpenedAtUnixMs, undefined);
  assert.equal(draft.completedAtUnixMs, undefined);
  assert.equal(draft.completedSnapshot, undefined);
  assert.equal(draft.activeAnalysis, undefined);
  assert.equal(draft.baselineAnalysis, undefined);

  const completed = mapWritingDocument(rustCompletedDocument());
  assert.equal(completed.draftUpdatedAtUnixMs, undefined);
  assert.equal(completed.draftSnapshot, undefined);
  assert.equal(completed.versions[0].analysisRevision, undefined);
  assert.equal(completed.answers[0].parentAnswerId, undefined);
  assert.equal(completed.answers[0].selectionText, undefined);
  assert.equal(completed.answers[0].map, undefined);
});

test("正式 service 的 create/list/load/save/complete/continue 均接受 Rust null 协议", async () => {
  const draft = rustDraftDocument();
  const completed = rustCompletedDocument();
  const service = new RepositoryWritingService({
    create: async () => draft,
    list: async () => [draft, completed],
    get: async () => draft,
    saveDraft: async () => ({ ...draft, revision: 1 }),
    delete: async () => true,
    analyze: async () => ({ ...draft, revision: 1 }),
    ask: async () => completed.answers[0],
    complete: async () => completed,
    continueEditing: async () => ({
      ...draft,
      revision: 5,
      completedAtUnixMs: NOW,
      completedSnapshot: completed.completedSnapshot,
    }),
  });

  assert.equal((await service.createDocument()).id, 71);
  assert.equal((await service.listDocuments()).length, 2);
  assert.equal((await service.loadDocument(71)).completedAtUnixMs, undefined);
  assert.equal(
    (await service.saveDraft(71, 0, snapshot("", ""))).revision,
    1,
  );
  assert.equal((await service.completeDocument(71, 0)).versions.length, 1);
  assert.equal(
    (await service.continueEditing(71, 4, 91)).draftSnapshot.title,
    "",
  );
});

test("Tauri writing repository 使用有类型 commands 与 camelCase 参数", async () => {
  const calls = [];
  const repository = new TauriWritingRepository(async (command, args) => {
    calls.push({ command, args });
    if (command === "list_writing_documents") {
      return [documentRecord()];
    }
    if (command === "delete_writing_document") {
      return true;
    }
    if (command === "ask_writing_question") {
      return {
        id: 99,
        documentId: 17,
        documentRevision: 2,
        question: "这里自然吗？",
        scope: "selection",
        scopeLabel: "所选内容",
        selectionText: "Real body",
        title: "回答",
        copy: "说明",
        createdAtUnixMs: NOW,
      };
    }
    return documentRecord();
  });

  await repository.create();
  await repository.list(" body ");
  await repository.get(17);
  await repository.saveDraft(17, 2, snapshot("Next", "Next body"));
  await repository.delete(17, 3);
  await repository.analyze(17, 3);
  await repository.ask({
    documentId: 17,
    expectedRevision: 3,
    versionId: 21,
    question: "这里自然吗？",
    scope: "selection",
    selectionText: "Next body",
    parentAnswerId: 9,
  });
  await repository.complete(17, 3);
  await repository.continueEditing(17, 4, 21);

  assert.deepEqual(calls, [
    { command: "create_writing_document", args: undefined },
    { command: "list_writing_documents", args: { query: "body" } },
    { command: "get_writing_document", args: { documentId: 17 } },
    {
      command: "save_writing_draft",
      args: {
        documentId: 17,
        expectedRevision: 2,
        snapshot: snapshot("Next", "Next body"),
      },
    },
    {
      command: "delete_writing_document",
      args: { documentId: 17, expectedRevision: 3 },
    },
    {
      command: "analyze_writing_document",
      args: { documentId: 17, expectedRevision: 3 },
    },
    {
      command: "ask_writing_question",
      args: {
        request: {
          documentId: 17,
          expectedRevision: 3,
          versionId: 21,
          question: "这里自然吗？",
          scope: "selection",
          selectionText: "Next body",
          parentAnswerId: 9,
        },
      },
    },
    {
      command: "complete_writing_document",
      args: { documentId: 17, expectedRevision: 3 },
    },
    {
      command: "continue_writing_document",
      args: {
        documentId: 17,
        expectedRevision: 4,
        versionId: 21,
      },
    },
  ]);
});

test("service 深拷贝并拒绝串写文章的分析、版本和回答", async () => {
  const mapped = mapWritingDocument(
    documentRecord({
      activeAnalysis: {
        id: 31,
        documentId: 17,
        documentRevision: 2,
        round: 1,
        issues: [
          {
            id: "issue-1",
            category: "清晰度",
            source: "Real body",
            targetText: "Real body",
            explanation: "说明",
            hint: "提示",
            deeperHint: "进一步提示",
            reference: "Reference",
          },
        ],
        patterns: [
          { id: "01", title: "模式", description: "说明" },
        ],
        createdAtUnixMs: NOW,
      },
      versions: [
        {
          id: 41,
          documentId: 17,
          ordinal: 1,
          sourceRevision: 2,
          analysisRevision: 2,
          comparisonBaselineRevision: 2,
          snapshot: snapshot(),
          comparisonBaseline: snapshot("Old", "Old body"),
          issues: [],
          patterns: [],
          completedAtUnixMs: NOW,
        },
      ],
      answers: [
        {
          id: 51,
          documentId: 17,
          documentRevision: 2,
          question: "问题",
          scope: "paragraph",
          scopeLabel: "当前段落",
          title: "回答",
          copy: "内容",
          createdAtUnixMs: NOW,
        },
      ],
    }),
  );
  mapped.draftSnapshot.paragraphs[0] = "Changed only in mapped copy";
  assert.equal(documentRecord().draftSnapshot.paragraphs[0], "Real body");

  assert.throws(
    () =>
      mapWritingDocument(
        documentRecord({
          activeAnalysis: {
            id: 31,
            documentId: 99,
            documentRevision: 2,
            round: 1,
            issues: [],
            patterns: [],
            createdAtUnixMs: NOW,
          },
        }),
      ),
    /串写到其他文章/,
  );
});

test("service 分离当前分析与基线检查，并接受基线早于完成正文", () => {
  const checkedIssue = {
    id: "baseline-issue",
    category: "清晰度",
    source: "Checked body",
    targetText: "Checked body",
    explanation: "说明",
    hint: "提示",
    deeperHint: "进一步提示",
    reference: "Reference",
  };
  const mapped = mapWritingDocument(
    documentRecord({
      revision: 4,
      comparisonBaselineRevision: 3,
      activeAnalysis: null,
      baselineAnalysis: {
        id: 61,
        documentId: 17,
        documentRevision: 3,
        round: 2,
        issues: [checkedIssue],
        patterns: [
          { id: "02", title: "基线模式", description: "仍应保留" },
        ],
        createdAtUnixMs: NOW,
      },
      versions: [
        {
          id: 62,
          documentId: 17,
          ordinal: 1,
          sourceRevision: 4,
          analysisRevision: 3,
          comparisonBaselineRevision: 3,
          snapshot: snapshot("Edited", "Edited after check"),
          comparisonBaseline: snapshot("Checked", "Checked body"),
          issues: [checkedIssue],
          patterns: [],
          completedAtUnixMs: NOW,
        },
      ],
    }),
  );

  assert.equal(mapped.activeAnalysis, undefined);
  assert.equal(mapped.baselineAnalysis.documentRevision, 3);
  assert.equal(mapped.versions[0].sourceRevision, 4);
  assert.equal(mapped.versions[0].analysisRevision, 3);
  checkedIssue.explanation = "mutated source payload";
  assert.equal(mapped.baselineAnalysis.issues[0].explanation, "说明");
});

test("service 明确映射缺失文章与无效 revision", async () => {
  const service = new RepositoryWritingService({
    create: async () => documentRecord(),
    list: async () => [],
    get: async () => null,
    saveDraft: async () => documentRecord(),
    delete: async () => true,
    analyze: async () => documentRecord(),
    ask: async () => {
      throw new Error("not used");
    },
    complete: async () => documentRecord(),
    continueEditing: async () => documentRecord(),
  });

  await assert.rejects(service.loadDocument(17), /不存在或已被删除/);
  await assert.rejects(
    service.saveDraft(17, -1, snapshot()),
    /revision 无效/,
  );
});

test("library loader 覆盖 loading、empty、error 与 retry", async () => {
  const states = [];
  await loadWritingLibrary(
    { listDocuments: async () => [] },
    "",
    (state) => states.push(state),
  );
  assert.deepEqual(
    states.map((state) => [state.status, state.records.length]),
    [
      ["loading", 0],
      ["ready", 0],
    ],
  );

  let attempts = 0;
  const retryStates = [];
  const service = {
    listDocuments: async () => {
      attempts += 1;
      if (attempts === 1) {
        throw new Error("database unavailable");
      }
      return [documentRecord()];
    },
  };
  await assert.rejects(
    loadWritingLibrary(service, "", (state) => retryStates.push(state)),
    /database unavailable/,
  );
  await loadWritingLibrary(service, "", (state) => retryStates.push(state));
  assert.deepEqual(
    retryStates.map((state) => state.status),
    ["loading", "error", "loading", "ready"],
  );
});

test("library loader 保留搜索词对应的 loading 与结果状态", async () => {
  const queries = [];
  const states = [];
  await loadWritingLibrary(
    {
      listDocuments: async (query) => {
        queries.push(query);
        return [documentRecord({ id: 23 })];
      },
    },
    " completed needle ",
    (state) => states.push(state),
  );
  assert.deepEqual(queries, [" completed needle "]);
  assert.deepEqual(
    states.map((state) => [state.status, state.records.map((record) => record.id)]),
    [
      ["loading", []],
      ["ready", [23]],
    ],
  );
});

test("防抖自动保存只提交最新正文并推进数据库 revision", async () => {
  const calls = [];
  const saved = [];
  const coordinator = new WritingDraftSaveCoordinator(
    {
      saveDraft: async (documentId, expectedRevision, nextSnapshot) => {
        calls.push({ documentId, expectedRevision, nextSnapshot });
        return documentRecord({
          id: documentId,
          revision: expectedRevision + 1,
          draftSnapshot: nextSnapshot,
        });
      },
    },
    {
      delayMs: 10,
      onSaved: (document) => saved.push(document),
    },
  );
  coordinator.register(documentRecord());
  coordinator.schedule(17, snapshot("First", "First body"));
  coordinator.schedule(17, snapshot("Latest", "Latest body"));
  await new Promise((resolve) => setTimeout(resolve, 30));
  await coordinator.flush(17);

  assert.equal(calls.length, 1);
  assert.equal(calls[0].expectedRevision, 2);
  assert.equal(calls[0].nextSnapshot.title, "Latest");
  assert.equal(coordinator.currentRevision(17), 3);
  assert.equal(saved[0].draftSnapshot.title, "Latest");
});

test("切换 flush、失败重试和 dispose 均保留文章身份与最新正文", async () => {
  let attempts = 0;
  const calls = [];
  const remote = new Map([
    [17, documentRecord()],
    [18, documentRecord({ id: 18, revision: 5 })],
  ]);
  const coordinator = new WritingDraftSaveCoordinator(
    {
      saveDraft: async (documentId, expectedRevision, nextSnapshot) => {
        calls.push({ documentId, expectedRevision, nextSnapshot });
        attempts += 1;
        if (attempts === 1) {
          throw new Error("forced save failure");
        }
        const savedDocument = documentRecord({
          id: documentId,
          revision: expectedRevision + 1,
          draftSnapshot: nextSnapshot,
        });
        remote.set(documentId, savedDocument);
        return savedDocument;
      },
      loadDocument: async (documentId) => remote.get(documentId),
    },
    { delayMs: 10_000 },
  );
  coordinator.register(documentRecord());
  coordinator.register(documentRecord({ id: 18, revision: 5 }));
  coordinator.schedule(17, snapshot("Keep me", "Unsaved body"));
  assert.equal(await coordinator.flush(17), false);
  assert.equal(coordinator.currentRevision(17), 2);
  assert.equal(await coordinator.retry(17), true);
  coordinator.schedule(18, snapshot("Second doc", "Second body"));
  await coordinator.dispose();

  assert.deepEqual(
    calls.map((call) => [call.documentId, call.expectedRevision, call.nextSnapshot.title]),
    [
      [17, 2, "Keep me"],
      [17, 2, "Keep me"],
      [18, 5, "Second doc"],
    ],
  );
  assert.equal(coordinator.currentRevision(17), 3);
  assert.equal(coordinator.currentRevision(18), 6);
});

test("自动保存对账已提交但调用方未确认的结果，不重复覆盖数据库", async () => {
  const attempted = snapshot("Committed", "Committed body");
  let remote = documentRecord();
  let saveCalls = 0;
  let loadCalls = 0;
  const coordinator = new WritingDraftSaveCoordinator(
    {
      saveDraft: async (_documentId, expectedRevision, nextSnapshot) => {
        saveCalls += 1;
        remote = documentRecord({
          revision: expectedRevision + 1,
          draftSnapshot: nextSnapshot,
        });
        throw new Error("invoke result mapping failed after commit");
      },
      loadDocument: async () => {
        loadCalls += 1;
        return remote;
      },
    },
    { delayMs: 10_000 },
  );
  coordinator.register(documentRecord());
  coordinator.schedule(17, attempted);

  assert.equal(await coordinator.flush(17), true);
  assert.equal(saveCalls, 1);
  assert.equal(loadCalls, 1);
  assert.equal(coordinator.currentRevision(17), 3);
});

test("自动保存提交前失败时读回旧 revision，重试仍使用安全 revision", async () => {
  const attempted = snapshot("Retry safely", "Still local");
  let remote = documentRecord();
  let attempts = 0;
  let loadCalls = 0;
  const coordinator = new WritingDraftSaveCoordinator(
    {
      saveDraft: async (_documentId, expectedRevision, nextSnapshot) => {
        attempts += 1;
        if (attempts === 1) {
          throw new Error("failed before commit");
        }
        remote = documentRecord({
          revision: expectedRevision + 1,
          draftSnapshot: nextSnapshot,
        });
        return remote;
      },
      loadDocument: async () => {
        loadCalls += 1;
        return remote;
      },
    },
    { delayMs: 10_000 },
  );
  coordinator.register(documentRecord());
  coordinator.schedule(17, attempted);

  assert.equal(await coordinator.flush(17), false);
  assert.equal(loadCalls, 1);
  assert.equal(coordinator.currentRevision(17), 2);
  assert.equal(await coordinator.retry(17), true);
  assert.deepEqual(
    [attempts, coordinator.currentRevision(17), remote.draftSnapshot],
    [2, 3, attempted],
  );
});

test("自动保存对账发现远端内容不同后保留本地正文且禁止旧 revision 覆盖", async () => {
  const attempted = snapshot("Local", "Do not lose me");
  const remote = documentRecord({
    revision: 3,
    draftSnapshot: snapshot("Remote", "Different committed body"),
  });
  let saveCalls = 0;
  const errors = [];
  const coordinator = new WritingDraftSaveCoordinator(
    {
      saveDraft: async () => {
        saveCalls += 1;
        throw new Error("ambiguous save result");
      },
      loadDocument: async () => remote,
    },
    {
      delayMs: 10_000,
      onStateChange: (_id, state, error) => {
        if (state === "error") {
          errors.push(error);
        }
      },
    },
  );
  coordinator.register(documentRecord());
  coordinator.schedule(17, attempted);

  assert.equal(await coordinator.flush(17), false);
  assert.equal(await coordinator.retry(17), false);
  assert.equal(saveCalls, 1);
  assert.match(errors.at(-1), /冲突|不同/);
});

test("分析先推进 revision 后，在途自动保存基于权威 revision 自动重试", async () => {
  const checked = snapshot("Checked", "Body before local edit");
  const pendingEdit = snapshot("Checked", "Pending local edit");
  let remote = documentRecord({
    revision: 3,
    draftSnapshot: checked,
  });
  let rejectFirstSave;
  const firstSave = new Promise((_resolve, reject) => {
    rejectFirstSave = reject;
  });
  const calls = [];
  const errors = [];
  const coordinator = new WritingDraftSaveCoordinator(
    {
      saveDraft: async (documentId, expectedRevision, nextSnapshot) => {
        calls.push({ documentId, expectedRevision, nextSnapshot });
        if (calls.length === 1) {
          return firstSave;
        }
        remote = documentRecord({
          id: documentId,
          revision: expectedRevision + 1,
          draftSnapshot: nextSnapshot,
        });
        return remote;
      },
      loadDocument: async () => remote,
    },
    {
      delayMs: 10_000,
      onStateChange: (_id, state, error) => {
        if (state === "error") {
          errors.push(error);
        }
      },
    },
  );
  coordinator.register(
    documentRecord({
      revision: 2,
      draftSnapshot: checked,
    }),
  );
  coordinator.schedule(17, pendingEdit);
  const flushing = coordinator.flush(17);
  await new Promise((resolve) => setImmediate(resolve));

  coordinator.acceptAuthoritative(remote);
  rejectFirstSave(new Error("revision conflict after analysis commit"));

  assert.equal(await flushing, true);
  assert.deepEqual(
    calls.map((call) => call.expectedRevision),
    [2, 3],
  );
  assert.equal(coordinator.currentRevision(17), 4);
  assert.deepEqual(remote.draftSnapshot, pendingEdit);
  assert.equal(errors.length, 0);
});

test("分析权威结果若改变送检正文，不得为在途自动保存解锁覆盖", async () => {
  const checked = snapshot("Checked", "Original checked body");
  const pendingEdit = snapshot("Checked", "Pending local edit");
  const remoteChanged = documentRecord({
    revision: 3,
    draftSnapshot: snapshot("Remote", "Different remote body"),
  });
  let rejectFirstSave;
  const firstSave = new Promise((_resolve, reject) => {
    rejectFirstSave = reject;
  });
  let saveCalls = 0;
  const coordinator = new WritingDraftSaveCoordinator(
    {
      saveDraft: async () => {
        saveCalls += 1;
        return firstSave;
      },
      loadDocument: async () => remoteChanged,
    },
    { delayMs: 10_000 },
  );
  coordinator.register(
    documentRecord({
      revision: 2,
      draftSnapshot: checked,
    }),
  );
  coordinator.schedule(17, pendingEdit);
  const flushing = coordinator.flush(17);
  await new Promise((resolve) => setImmediate(resolve));

  await assert.rejects(
    coordinator.acceptAuthoritative(remoteChanged),
    /正文保持不变|不同正文/,
  );
  rejectFirstSave(new Error("revision conflict"));

  assert.equal(await flushing, false);
  assert.equal(await coordinator.retry(17), false);
  assert.equal(saveCalls, 1);
});

function deferred() {
  let resolve;
  const promise = new Promise((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

test("迟到检查遇到防抖未落盘编辑时由本地 generation 与快照拒绝", async () => {
  let generation = 5;
  let visibleSnapshot = snapshot("Visible", "Before request");
  const model = deferred();
  const captured = captureWritingRequestIdentity({
    documentId: 17,
    revision: 2,
    generation,
    snapshot: visibleSnapshot,
  });
  const pending = runGuardedWritingRequest(
    captured,
    () =>
      captureWritingRequestIdentity({
        documentId: 17,
        revision: 2,
        generation,
        snapshot: visibleSnapshot,
      }),
    model.promise,
    "写作检查",
  );
  generation += 1;
  visibleSnapshot = snapshot("Visible", "Unsaved debounce edit");
  model.resolve(documentRecord());

  await assert.rejects(pending, /当前可见|已过期/);
});

test("迟到问答在本地编辑、文章或历史版本身份变化后不得显示", async () => {
  let visible = {
    documentId: 17,
    revision: 4,
    generation: 9,
    snapshot: snapshot("Version one", "Immutable V1"),
    versionId: 91,
  };
  const model = deferred();
  const pending = runGuardedWritingRequest(
    captureWritingRequestIdentity(visible),
    () => captureWritingRequestIdentity(visible),
    model.promise,
    "写作问答",
  );
  visible = {
    ...visible,
    generation: 10,
    versionId: 92,
    snapshot: snapshot("Version two", "Immutable V2"),
  };
  model.resolve({ id: 101 });

  await assert.rejects(pending, /当前可见|已过期/);
});

test("隐藏 WritingPage 不接管 Ctrl+J 或 Escape", () => {
  assert.equal(
    shouldHandleWritingShortcut(true, "draft", {
      key: "j",
      ctrlKey: true,
    }),
    false,
  );
  assert.equal(
    shouldHandleWritingShortcut(true, "draft", {
      key: "Escape",
      ctrlKey: false,
    }),
    false,
  );
  assert.equal(
    shouldHandleWritingShortcut(false, "draft", {
      key: "j",
      ctrlKey: true,
    }),
    true,
  );
});
