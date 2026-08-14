import assert from "node:assert/strict";
import test from "node:test";
import {
  RepositoryMemoryService,
  mapLearningTargetToMemoryItem,
} from "../src/memoryService.ts";

const NOW = new Date(2026, 7, 13, 12, 0, 0);

function record(id, targetId, queryText, createdAtUnixMs, contextText) {
  return {
    id,
    learningTargetId: targetId,
    queryText,
    learningTargetText: "SQLite",
    queryDirection: "enToZh",
    normalizedText: queryText.trim().toLowerCase(),
    queryType: "word",
    sourceType: "windows_uia",
    sourceApp: id === 11 ? "Code.exe" : "Obsidian.exe",
    contextText,
    explanationCard: {
      queryType: "word",
      sourceText: queryText,
      learningTargetText: "SQLite",
      headword: "SQLite",
      partOfSpeech: "noun",
      phonetic: "/ˌes kjuː el ˈaɪt/",
      basicMeanings: ["嵌入式数据库"],
      contextMeaning: "本地嵌入式数据库",
      sourceSentence: contextText,
      sourceSentenceZh: "应用把数据保存在本地数据库中。",
      phrases: [],
      nearMeanings: [],
      examples: [],
      reviewHint: "回忆本地数据库。",
    },
    schemaVersion: 2,
    createdAtUnixMs,
    difficulty: null,
  };
}

function targetSummary(overrides = {}) {
  const older = record(
    10,
    7,
    "  sqlite  ",
    NOW.getTime() - 60_000,
    "The app stores data in SQLite locally.",
  );
  const latest = record(
    11,
    7,
    "SQLite",
    NOW.getTime() - 1_000,
    "SQLite keeps the learning history on this device.",
  );
  return {
    id: 7,
    stableKey: "v1:word:sqlite",
    canonicalizationVersion: 1,
    queryType: "word",
    learningTargetText: "SQLite",
    normalizedTargetText: "sqlite",
    queryCount: 2,
    firstSeenAtUnixMs: older.createdAtUnixMs,
    lastSeenAtUnixMs: latest.createdAtUnixMs,
    representativeRecord: latest,
    ...overrides,
  };
}

test("Memory 列表以稳定目标为身份并显示查询次数和最近代表记录", () => {
  const item = mapLearningTargetToMemoryItem(targetSummary(), NOW);
  assert.equal(item.id, "7");
  assert.equal(item.representativeLearningRecordId, "11");
  assert.equal(item.queryCount, 2);
  assert.equal(item.query, "SQLite");
  assert.equal(item.app, "Code");
  assert.match(item.sentence, /learning history/);
  assert.deepEqual(item.history, []);
});

test("Memory 分页总数来自聚合目标且详情保留全部真实 occurrence", async () => {
  const summary = targetSummary();
  const occurrences = [
    summary.representativeRecord,
    record(
      10,
      7,
      "  sqlite  ",
      NOW.getTime() - 60_000,
      "The app stores data in SQLite locally.",
    ),
  ];
  const calls = [];
  const service = new RepositoryMemoryService({
    async list(query) {
      calls.push(["list", query]);
      return { targets: [summary], page: 2, pageSize: 1, total: 9 };
    },
    async get(id) {
      calls.push(["get", id]);
      return { target: summary, occurrences };
    },
  });

  const page = await service.listRecords({
    page: 2,
    pageSize: 1,
    keyword: "history",
    queryType: "word",
  });
  assert.equal(page.total, 9);
  assert.equal(page.page, 2);
  assert.equal(page.records.length, 1);

  const detail = await service.getRecord("7");
  assert.equal(detail.queryCount, 2);
  assert.deepEqual(
    detail.history.map((occurrence) => occurrence.learningRecordId),
    ["11", "10"],
  );
  assert.deepEqual(
    detail.history.map((occurrence) => occurrence.query),
    ["SQLite", "sqlite"],
  );
  assert.match(detail.history[1].context, /stores data/);
  assert.deepEqual(calls, [
    [
      "list",
      { page: 2, pageSize: 1, keyword: "history", queryType: "word" },
    ],
    ["get", 7],
  ]);
});

test("Memory 详情拒绝非整数目标身份", async () => {
  const service = new RepositoryMemoryService({
    async list() {
      throw new Error("unused");
    },
    async get() {
      throw new Error("unused");
    },
  });
  await assert.rejects(() => service.getRecord("7.5"), /学习目标 ID 无效/);
});
