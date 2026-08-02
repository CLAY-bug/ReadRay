import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const FORMAL_FILES = [
  "src/writingViewModel.ts",
  "src/writingRepository.ts",
  "src/writingService.ts",
  "src/writingDraftSaveCoordinator.ts",
  "src/writingRequestIdentity.ts",
  "src/writingReviewState.ts",
  "src/components/WritingPage.tsx",
  "src/components/WritingEditor.tsx",
  "src/components/WritingCoach.tsx",
  "src/components/WritingCompareView.tsx",
];

test("正式写作路径不读取 localStorage、演示文章、硬编码问题或演示回答", async () => {
  for (const file of FORMAL_FILES) {
    const content = await readFile(file, "utf8");
    assert.doesNotMatch(content, /localStorage/);
    assert.doesNotMatch(content, /writingDocumentFixtures|writingIssues|answerWritingQuestion/);
    assert.doesNotMatch(content, /made me to wait|When Technology Learns to Stay Quiet/);
    assert.doesNotMatch(content, /writingFixtureService/);
  }
});

test("浏览器 writing fixture 仅由非 Tauri 动态装配分支加载", async () => {
  const app = await readFile("src/App.tsx", "utf8");
  const previewBranch = app.slice(
    app.indexOf("if (isTauriRuntime) {", app.indexOf("function MainAppWindow")),
    app.indexOf("return () => {", app.indexOf('import("./writingFixtureService")')),
  );
  assert.match(previewBranch, /if \(isTauriRuntime\) \{\s*return;/);
  assert.match(previewBranch, /import\("\.\/writingFixtureService"\)/);

  const repository = await readFile("src/writingRepository.ts", "utf8");
  assert.match(repository, /@tauri-apps\/api\/core/);
  assert.doesNotMatch(repository, /writingFixtureService|localStorage/);
});

test("写作辅导持续展开问答并让提问框按内容增高", async () => {
  const coach = await readFile("src/components/WritingCoach.tsx", "utf8");
  const styles = await readFile("src/styles/writing-page.css", "utf8");

  assert.match(coach, /rr-writing-agent-transcript/);
  assert.match(coach, /mergeWritingConversationAnswers/);
  assert.doesNotMatch(coach, /之前的问题|rr-writing-agent-history|<details/);
  assert.match(coach, /input\.style\.height = "auto"/);
  assert.match(coach, /input\.scrollHeight/);
  assert.match(styles, /\.rr-writing-agent-input textarea[\s\S]*overflow-y: hidden/);
  assert.match(styles, /textarea::-webkit-scrollbar[\s\S]*width: 0/);
});
