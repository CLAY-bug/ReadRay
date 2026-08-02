import assert from "node:assert/strict";
import test from "node:test";
import {
  LatestWritingRequestSequence,
  createWritingReviewTargetState,
  mergeWritingConversationAnswers,
} from "../src/writingReviewState.ts";

function snapshot(title, body) {
  return { title, paragraphs: [body] };
}

function issue(id, targetText) {
  return {
    id,
    category: "clarity",
    source: targetText,
    targetText,
    explanation: "explanation",
    hint: "hint",
    deeperHint: "deeper",
    reference: "reference",
  };
}

function answer(id, versionId, copy) {
  return {
    id,
    documentId: 17,
    documentRevision: versionId === 91 ? 2 : 4,
    versionId,
    question: "question",
    scope: "document",
    scopeLabel: "整篇文章",
    title: "answer",
    copy,
    createdAtUnixMs: id,
  };
}

const document = {
  id: 17,
  revision: 6,
  createdAtUnixMs: 1,
  updatedAtUnixMs: 2,
  draftUpdatedAtUnixMs: 2,
  draftSnapshot: snapshot("Draft", "Hidden draft"),
  completedAtUnixMs: 2,
  completedSnapshot: snapshot("V2", "Second version"),
  comparisonBaseline: snapshot("Draft", "Baseline"),
  comparisonBaselineRevision: 5,
  versions: [
    {
      id: 91,
      documentId: 17,
      ordinal: 1,
      sourceRevision: 2,
      comparisonBaselineRevision: 1,
      snapshot: snapshot("V1", "First version"),
      comparisonBaseline: snapshot("V1 baseline", "First baseline"),
      issues: [issue("v1-issue", "First version")],
      patterns: [{ id: "v1-pattern", title: "V1", description: "one" }],
      completedAtUnixMs: 1,
    },
    {
      id: 92,
      documentId: 17,
      ordinal: 2,
      sourceRevision: 4,
      comparisonBaselineRevision: 3,
      snapshot: snapshot("V2", "Second version"),
      comparisonBaseline: snapshot("V2 baseline", "Second baseline"),
      issues: [issue("v2-issue", "Second version")],
      patterns: [{ id: "v2-pattern", title: "V2", description: "two" }],
      completedAtUnixMs: 2,
    },
  ],
  answers: [
    answer(101, 91, "V1 answer"),
    answer(102, 92, "V2 answer"),
  ],
};

test("历史版本切换重建问题状态并只保留目标版本回答", () => {
  const first = createWritingReviewTargetState(document, 91);
  assert.deepEqual(Object.keys(first.issueStates), ["v1-issue"]);
  assert.equal(first.activeIssueId, null);
  assert.deepEqual(first.issueStates["v1-issue"], {
    status: "baseline",
    showDeeperHint: false,
    showReference: false,
  });
  assert.deepEqual(first.answers.map((entry) => entry.id), [101]);

  const second = createWritingReviewTargetState(document, 92);
  assert.deepEqual(Object.keys(second.issueStates), ["v2-issue"]);
  assert.equal(second.issueStates["v1-issue"], undefined);
  assert.equal(second.activeIssueId, null);
  assert.deepEqual(second.answers.map((entry) => entry.id), [102]);
  assert.notEqual(first.targetKey, second.targetKey);
});

test("草稿用基线检查重建问题，并在自动保存后保留本轮辅导会话", () => {
  const baselineIssue = issue("baseline-issue", "Hidden draft");
  const draft = {
    ...document,
    activeAnalysis: undefined,
    baselineAnalysis: {
      id: 110,
      documentId: 17,
      documentRevision: 5,
      round: 3,
      issues: [baselineIssue],
      patterns: [
        { id: "baseline-pattern", title: "Baseline", description: "kept" },
      ],
      createdAtUnixMs: 3,
    },
    answers: [
      ...document.answers,
      {
        ...answer(100, undefined, "previous completed draft answer"),
        documentRevision: 4,
      },
      {
        ...answer(103, undefined, "current draft answer"),
        documentRevision: 5,
      },
    ],
  };

  const state = createWritingReviewTargetState(draft);
  assert.deepEqual(state.issues.map((entry) => entry.id), ["baseline-issue"]);
  assert.deepEqual(state.patterns.map((entry) => entry.id), ["baseline-pattern"]);
  assert.deepEqual(state.answers.map((entry) => entry.id), [103]);

  const afterAutoSave = createWritingReviewTargetState({
    ...draft,
    revision: 7,
    answers: [
      ...draft.answers,
      {
        ...answer(104, undefined, "follow-up after auto save"),
        documentRevision: 7,
        parentAnswerId: 103,
      },
    ],
  });
  assert.equal(afterAutoSave.targetKey, state.targetKey);
  assert.deepEqual(afterAutoSave.answers.map((entry) => entry.id), [103, 104]);
  assert.equal(afterAutoSave.answers[1].parentAnswerId, 103);
});

test("连续辅导合并权威回答时保持完整顺序且不折叠旧轮次", () => {
  const first = answer(201, undefined, "first");
  const second = {
    ...answer(202, undefined, "second"),
    parentAnswerId: first.id,
  };
  const third = {
    ...answer(203, undefined, "third"),
    parentAnswerId: second.id,
  };
  const merged = mergeWritingConversationAnswers(
    [first, second],
    [second, third],
  );

  assert.deepEqual(merged.map((entry) => entry.id), [201, 202, 203]);
  assert.deepEqual(merged.map((entry) => entry.parentAnswerId), [
    undefined,
    201,
    202,
  ]);
});

test("同一可见身份的多次问答只接受最新请求序号", async () => {
  const sequence = new LatestWritingRequestSequence();
  let resolveFirst;
  let resolveSecond;
  const firstModel = new Promise((resolve) => {
    resolveFirst = resolve;
  });
  const secondModel = new Promise((resolve) => {
    resolveSecond = resolve;
  });
  const run = async (model) => {
    const requestSequence = sequence.begin();
    const result = await model;
    sequence.requireCurrent(requestSequence);
    return result;
  };

  const first = run(firstModel);
  const second = run(secondModel);
  resolveSecond("new answer");
  assert.equal(await second, "new answer");
  resolveFirst("old answer");
  await assert.rejects(first, /更新|过期|较新的问答/);
});
