import assert from "node:assert/strict";
import test from "node:test";
import { mapLearningRecordToMemoryItem } from "../src/memoryService.ts";
import { mapReviewFeedPage } from "../src/reviewService.ts";
import { RepositoryTodayService } from "../src/todayService.ts";

const NOW = new Date(2026, 7, 11, 12, 0, 0);
const DAY_START = new Date(2026, 7, 11).getTime();
const DAY_END = new Date(2026, 7, 12).getTime();

function chineseSourceRecord() {
  return {
    id: 31,
    queryText: "界面",
    learningTargetText: "interface",
    queryDirection: "zhToEn",
    normalizedText: "界面",
    queryType: "word",
    sourceType: "windows_uia",
    sourceApp: "Obsidian.exe",
    contextText: "这个界面支持本地学习记录。",
    explanationCard: {
      queryType: "word",
      sourceText: "界面",
      learningTargetText: "interface",
      headword: "interface",
      partOfSpeech: "noun",
      basicMeanings: ["界面"],
      contextMeaning: "interface",
      sourceSentence: "这个界面支持本地学习记录。",
      sourceSentenceZh: null,
      phrases: [],
      nearMeanings: [],
      examples: [],
      reviewHint: null,
    },
    schemaVersion: 2,
    createdAtUnixMs: DAY_START + 1_000,
    difficulty: null,
  };
}

test("Memory 主标题使用规范英文并保留原始中文来源", () => {
  const item = mapLearningRecordToMemoryItem(chineseSourceRecord(), NOW);
  assert.equal(item.query, "interface");
  assert.equal(item.sentence, "这个界面支持本地学习记录。");
  assert.notEqual(item.query, "界面");
});

function paragraphRecord(summary) {
  const source = "ReadRay turns real reading into a continuous learning loop.";
  const translation = "ReadRay 将真实阅读转化为持续的学习循环。";
  return {
    id: 91,
    queryText: source,
    learningTargetText: source,
    queryDirection: "enToZh",
    normalizedText: source.toLowerCase(),
    queryType: "paragraph",
    sourceType: "windows_uia",
    sourceApp: "ChatGPT.exe",
    contextText: source,
    explanationCard: {
      queryType: "paragraph",
      sourceText: source,
      learningTargetText: source,
      translation,
      summary,
    },
    schemaVersion: 2,
    createdAtUnixMs: DAY_START + 2_000,
    difficulty: null,
  };
}

test("段落详情将原文、翻译和摘要映射为不重复的展示字段", () => {
  const item = mapLearningRecordToMemoryItem(
    paragraphRecord("它将真实阅读纳入持续学习流程。"),
    NOW,
  );
  assert.equal(item.sentence, paragraphRecord(null).queryText);
  assert.equal(item.definition, "ReadRay 将真实阅读转化为持续的学习循环。");
  assert.equal(item.translation, item.definition);
  assert.equal(item.meaning, "它将真实阅读纳入持续学习流程。");
});

test("段落摘要与翻译相同时隐藏重复的核心理解", () => {
  const translation = "ReadRay 将真实阅读转化为持续的学习循环。";
  const item = mapLearningRecordToMemoryItem(paragraphRecord(`  ${translation}  `), NOW);
  assert.equal(item.meaning, "");
  assert.equal(item.summary, translation);
});

test("Today 摘要与最近入口只展示规范英文目标", async () => {
  const service = new RepositoryTodayService({
    async getLearningSummary() {
      return { recordCount: 1, latestRecord: chineseSourceRecord() };
    },
    async listRecentConversations() {
      return [];
    },
  });
  const model = await service.loadToday(NOW);
  assert.match(model.summary, /interface/);
  assert.doesNotMatch(model.summary, /最近一次查询是“界面”/);
  assert.match(model.actions[2].title, /interface/);
});

test("Review 标题、提示答案与后台制卡目标使用规范英文", () => {
  const record = chineseSourceRecord();
  const model = mapReviewFeedPage(
    {
      dayStartUnixMs: DAY_START,
      dayEndUnixMs: DAY_END,
      pageSize: 12,
      items: [
        {
          id: 51,
          ordinal: 0,
          cycleIndex: 0,
          reasonCode: "newRecord",
          learningRecord: record,
          target: {
            learningRecordId: record.id,
            revision: 0,
            nextReviewAtUnixMs: DAY_START,
            attemptCount: 0,
            rememberedCount: 0,
            forgottenCount: 0,
            successStreak: 0,
            lastReviewedAtUnixMs: null,
            lastOutcome: null,
            lastUsedHint: null,
            lastAttemptId: null,
          },
          attempt: null,
          qualityFeedback: null,
          generatedCard: null,
          generationFailure: null,
        },
      ],
      nextCursor: 0,
      canContinue: true,
      completedCount: 0,
      rememberedCount: 0,
      forgottenCount: 0,
    },
    NOW,
  );
  assert.equal(model.cards[0].query, "interface");
  assert.equal(model.cards[0].promptText, "interface");
  assert.equal(model.cards[0].promptAnswer, "interface");
  assert.equal(model.cards[0].sourceExcerpt, "这个界面支持本地学习记录。");
});
