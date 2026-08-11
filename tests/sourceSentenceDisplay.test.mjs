import assert from "node:assert/strict";
import test from "node:test";
import {
  isPrimarilyChineseSourceSentence,
  sourceSentenceForDisplay,
} from "../src/sourceSentenceDisplay.js";

test("中文主导且夹英文术语的旧卡片只显示原句一次", () => {
  for (const sourceSentence of [
    "ReadRay 会在请求结束前检查 Rust generation，避免旧结果覆盖新结果。",
    "这个入口会同时刷新 Memory 和 Review，但不会修改原始学习记录。",
  ]) {
    const displayed = sourceSentenceForDisplay(
      sourceSentence,
      "这是一条模型重复生成的中文译文。",
    );

    assert.equal(isPrimarilyChineseSourceSentence(sourceSentence), true);
    assert.equal(displayed.sourceSentence, sourceSentence);
    assert.equal(displayed.sourceSentenceZh, undefined);
  }
});

test("普通英文原句继续显示中文翻译", () => {
  const sourceSentence =
    "The request generation prevents an older result from replacing a newer one.";
  const sourceSentenceZh = "请求代次可以防止旧结果覆盖新结果。";

  assert.equal(isPrimarilyChineseSourceSentence(sourceSentence), false);
  assert.deepEqual(sourceSentenceForDisplay(sourceSentence, sourceSentenceZh), {
    sourceSentence,
    sourceSentenceZh,
  });
});

test("原句可单独显示，只有译文时不显示孤立译文", () => {
  assert.deepEqual(sourceSentenceForDisplay("这是一条中文原句。", null), {
    sourceSentence: "这是一条中文原句。",
    sourceSentenceZh: undefined,
  });
  assert.deepEqual(sourceSentenceForDisplay(null, "孤立译文"), {
    sourceSentence: undefined,
    sourceSentenceZh: undefined,
  });
});
