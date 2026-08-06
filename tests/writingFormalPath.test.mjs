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

test("写作页在窄容器进入覆盖层断点时自动收起辅导区", async () => {
  const page = await readFile("src/components/WritingPage.tsx", "utf8");
  const styles = await readFile("src/styles/writing-page.css", "utf8");

  assert.match(page, /WRITING_ASSIST_AUTO_COLLAPSE_WIDTH = 1120/);
  assert.match(page, /new ResizeObserver\(/);
  assert.match(page, /setResponsiveCoachCollapsed\(true\)/);
  assert.match(page, /setAssistOpen\(false\)/);
  assert.match(page, /ref=\{writingPageRef\}/);
  assert.match(
    page,
    /setResponsiveCoachCollapsed\(false\)[\s\S]*?setAgentRequest/,
  );
  assert.match(
    styles,
    /@container \(max-width: 1120px\)[\s\S]*?\.rr-writing-page\.is-responsive-coach-collapsed \.rr-writing-coach-column[\s\S]*?visibility: hidden/,
  );
});

test("宽窗口写作页提供受边界约束的编辑区拖拽分隔条", async () => {
  const page = await readFile("src/components/WritingPage.tsx", "utf8");
  const styles = await readFile("src/styles/writing-page.css", "utf8");

  assert.match(page, /WRITING_EDITOR_MIN_WIDTH = 520/);
  assert.match(page, /WRITING_EDITOR_MAX_WIDTH = 960/);
  assert.match(page, /data-testid="writing-layout-resizer"/);
  assert.match(page, /setPointerCapture\(event\.pointerId\)/);
  assert.match(page, /aria-label="调整写作编辑区宽度"/);
  assert.match(styles, /--rr-writing-editor-column-width: 736px/);
  assert.match(
    styles,
    /grid-template-columns:[\s\S]*var\(--rr-writing-editor-column-width\)/,
  );
  assert.match(styles, /\.rr-writing-grid \{[\s\S]*?height: 100%;/);
  assert.match(
    styles,
    /\.rr-writing-editor-column \{[\s\S]*?height: 100%;/,
  );
  assert.match(
    styles,
    /\.rr-writing-editor-page \{[\s\S]*?min-height: max\(760px, calc\(100% - 70px\)\)/,
  );
  assert.match(styles, /\.rr-writing-layout-resizer[\s\S]*justify-self: end/);
  assert.match(styles, /\.rr-writing-layout-resizer[\s\S]*right: -3px/);
  assert.match(
    styles,
    /@container \(max-width: 1120px\)[\s\S]*?\.rr-writing-layout-resizer[\s\S]*?display: none/,
  );
  assert.match(
    styles,
    /\.rr-writing-page\.is-draft:not\(\.has-assist\) \.rr-writing-layout-resizer[\s\S]*?display: none/,
  );
  assert.match(
    styles,
    /\.rr-writing-page\.is-responsive-coach-collapsed \.rr-writing-layout-resizer[\s\S]*?display: none/,
  );
});

test("写作页隐藏无关元数据并在文章切换菜单外点击时收起", async () => {
  const page = await readFile("src/components/WritingPage.tsx", "utf8");
  const editor = await readFile("src/components/WritingEditor.tsx", "utf8");
  const styles = await readFile("src/styles/writing-page.css", "utf8");

  assert.doesNotMatch(page, /documentStatus|基于完成稿修改|kicker=/);
  assert.doesNotMatch(editor, /rr-writing-editor-kicker|kicker/);
  assert.match(page, /documentSwitcherRef/);
  assert.match(page, /document\.addEventListener\("pointerdown", handlePointerDown\)/);
  assert.match(page, /setDocumentSwitcherOpen\(false\)/);
  assert.match(page, /formatDocumentTime\([\s\S]*?"更新于 "/);
  assert.match(styles, /\.rr-writing-document-name \{[\s\S]*?max-width: 320px/);
  assert.doesNotMatch(
    styles,
    /@container \(max-width: 1120px\)[\s\S]*?\.rr-writing-document-name[\s\S]*?max-width: 150px/,
  );
});

test("写作文章库标题区只保留必要的页面标题", async () => {
  const library = await readFile("src/components/WritingLibrary.tsx", "utf8");
  const styles = await readFile("src/styles/writing-page.css", "utf8");

  assert.match(library, /<h1 id="rr-writing-library-heading">写作<\/h1>/);
  assert.doesNotMatch(library, /本地写作归档|未完成的文章从这里继续/);
  assert.doesNotMatch(
    styles,
    /\.rr-writing-library-head > div:first-child > (?:p|span)\s*\{/,
  );
});
