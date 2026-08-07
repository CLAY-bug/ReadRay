# 任务 2 回报：对话页面 Markdown 渲染

执行会话：2026-08-07（ReadRay 内置 Agent 体验升级 · 任务 2）
任务书：`docs/AGENT_UPGRADE.md` 任务 2；验收者：调度者

## 1. 改了哪些文件

| 文件 | 改动 |
| --- | --- |
| `src/markdownParse.ts` | **新增**。轻量自研 Markdown 白名单解析器（不依赖任何库），输出类型化 token，无 React 依赖，Node 测试可直接运行。 |
| `src/components/MarkdownContent.tsx` | **新增**。渲染组件：把解析器 token 映射为受控 React 元素，样式全部由 `rr-conversation-*` 作用域承担。 |
| `tests/markdownParse.test.mjs` | **新增**。渲染器单元测试 20 项：白名单子集、流式不完整片段、恶意输入（XSS）、畸形语法降级、表格降级、千行稳定性。 |
| `src/conversationViewModel.ts` | `ConversationAssistantMessage` 新增可选字段 `markdown?: string`（真实回答原始文本；存在时页面优先渲染它，否则回退 blocks）。 |
| `src/conversationService.ts` | `mapMessage` 对 assistant 消息携带 `markdown: message.content`，同时保留 blocks 单 paragraph 映射。 |
| `src/components/ConversationPage.tsx` | `AssistantMessage` 优先走 `MarkdownContent`（有 markdown 字段时），否则渲染 blocks（fixture 路径不变）；`GenerationMessage` 的流式 `state.text` 改用 `MarkdownContent streaming`；`commitAssistantReply` 生成的 assistant 消息携带 markdown 原文。 |
| `src/styles/conversation-page.css` | 新增渲染元素样式：`.rr-conversation-markdown-h1/h2/h3`、`.rr-conversation-markdown-list`、`.rr-conversation-markdown-quote`、`.rr-conversation-code-block`、`.rr-conversation-markdown-hr`、`.rr-conversation-markdown-link/.rr-conversation-markdown-url`。全部使用 `rr-conversation-*` 作用域与 `--rr-main-*` / `--rr-learning-*` 语义 token，无固定色值，未改变布局/字体/字号体系。 |
| `src-tauri/src/quick_ai.rs` | `QUICK_AI_SYSTEM_PROMPT` 联动修改 + 测试同步更新（详见第 2 节）。 |

未改：流式链路、消息持久化、停止/重试语义、ExplanationCard / 写作分析协议、用户消息气泡、fixture service、既有协议测试。

## 2. 实现要点

### 渲染子集与安全边界

- 白名单：段落与软换行、`#`/`##`/`###` 标题、`**粗体**`、`*斜体*`、`~~删除线~~`、行内代码、多行代码块（```）、无序/有序列表、`>` 引用、`---` 分隔线、链接 `[text](url)`。
- 未纳入白名单一律按纯文本：表格（`|` 语法原样保留）、HTML 标签（`<script>`、`<img onerror>` 等原样显示为文本）、`####` 以上标题。
- **链接**（按任务书默认处理）：渲染为可见文本 + 完整 URL（`链接（https://…）`），**不生成可点击元素**。解析层只对 `http://` / `https://` 协议产出 link token，`javascript:`、`data:` 等协议直接按纯文本降级（双层防护）。
- **安全边界**：解析器只输出类型化 token（text/code/strong/em/del/link/codeBlock/list/quote/hr），渲染组件只把这些 token 映射为固定 React 元素；输入文本永不作为 HTML 注入（React 转义 + 解析层协议白名单）。测试覆盖 `<script>`、`onerror=`、`javascript:` URL、`data:` URL、iframe 等恶意样本，全部降级为文本。
- **表格**（按任务书默认处理）：不纳入白名单，按纯文本降级。理由：表格在当前模型输出中不构成高频需求（本次无真实 DeepSeek 输出样本可验证频率），且流式下半闭合表格的降级处理需要额外状态，与"优先轻量自研"约束不符；实现成本可控时再评估。

### 流式不完整片段的处理策略（设计点 1）

采用**"拼接后整体渲染 + 未闭合降级"组合**：

- 每个 delta 到达后对整个 `state.text` 重新解析并整体渲染（每次渲染是纯函数调用，无状态累积，片段天然稳定）。
- 渲染器接受 `streaming` 标志：
  - `streaming=true`（GenerationMessage 生成中）：未闭合代码块**按代码块渲染**（不显示 ``` 围栏、不解析内部标记）；未闭合行内标记（`**`、`` ` ``、`~~`、`*`）**隐藏起始符号**，内容先以纯文本展示，闭合后自然切换为样式元素——全程不闪现原始标记。
  - `streaming=false`（完成态/历史消息）：未闭合代码块整体降级为原文纯文本段落，未闭合行内标记保留原文标记字符——完成态诚实展示"不完整"内容。
- 流式中途的列表、标题、引用按已到达内容稳定渲染，下一条继续追加，不做"列表只写一半时降级"的复杂状态机。

### blocks 协议去留（设计点 2）

**保留协议，采用双入口**。`ConversationAnswerBlock`（paragraph/list/example）是设计稿 fixture 时代的结构化协议，fixture 的 `example`（英文句 + 译文双行）和 `lead`（段首引导样式）无法用当前 Markdown 子集表达；删除协议会破坏 `conversationFixtureService.ts` 的设计稿示例展示，违反任务书"保持 fixture 预览路径可继续工作"。

方案：`ConversationAssistantMessage` 新增可选 `markdown?: string` 字段，真实路径（`conversationService.ts` 的 `mapMessage`）在保留 blocks 单 paragraph 映射的同时携带原始文本；页面渲染层判断——**有 markdown 字段走白名单渲染，否则渲染 blocks**。fixture 消息不带 markdown 字段，走既有 blocks 渲染，零改动。`markdown` 字段不进入导出链路（导出仍由 Rust 按 SQLite 权威快照生成）。

### 系统提示词联动

`QUICK_AI_SYSTEM_PROMPT` 原句：
> `Use plain text with short paragraphs, line breaks, and clear numbered lists when useful; avoid dense walls of text and do not rely on Markdown rendering.`

改为：
> `You may use concise Markdown to structure your answer when it helps readability: short paragraphs, headings, lists, code blocks, bold, and links are fine; keep answers readable and do not rely on complex formatting.`

诚实边界原句 `Do not claim access to the internet, tools, local learning records, or long-term memory.` 完整保留。

测试 `system_prompt_keeps_general_help_and_english_expertise_balanced` 同步更新：删除对 `short paragraphs` / `clear numbered lists` / `do not rely on markdown rendering` 的断言，新增对 `concise markdown` / `headings, lists, code blocks, bold, and links` / `do not rely on complex formatting` 的断言，并显式断言提示词**不含** `do not rely on markdown rendering`。其余断言（general-purpose assistant、english expertise、2-4 questions、诚实边界三项）全部保留。

## 3. 验证结果

| 验证项 | 结果 |
| --- | --- |
| `node --test tests/markdownParse.test.mjs`（新增渲染器测试） | **20 通过 / 0 失败** |
| `pnpm test:conversation` | **24 通过 / 0 失败**（既有测试全绿，含协议与映射断言） |
| `pnpm test:writing` | **30 通过 / 0 失败**（回归） |
| `pnpm test:settings` | 37 通过 / **1 失败：存量问题**（详见第 5 节） |
| `cargo test`（src-tauri） | **124 通过 / 0 失败 / 2 ignored**（含更新后的提示词测试；2 项真实 DeepSeek 联网测试按既有 ignore 跳过） |
| `pnpm build`（tsc + vite） | 通过 |
| `cargo fmt --check` | 通过 |
| `cargo check` | 通过 |
| `git diff --check` | 通过 |
| 浏览器预览（真实 React 渲染） | 通过（见下） |

### 浏览器预览验证（非 Tauri 分支，fixture 路径）

经 Vite dev server 在浏览器真实渲染验证：

- **完整 Markdown 样本**：`h3` 标题、`strong`/`em`/`del`/`code` 行内、链接（文本 + `（URL）`）、无序列表、引用、代码块全部正确渲染，无原始标记裸露。
- **XSS 样本**：`<script>` / `<img onerror>` / `javascript:` 链接渲染为纯文本（`&lt;script&gt;…`），无脚本执行、无 img 元素。
- **流式不完整片段**：未闭合代码块以 `pre > code` 渲染且不含 ``` 围栏；未闭合 `**` 隐藏起始符号、内容以纯文本展示。
- **语义 token**：在 `.rr-main-app` 作用域内标题 20px/600 字重、正文 16px 主前景色、代码块带 `--rr-main-border-soft` 边框 + 柔和背景、URL 使用次前景色 55% 透明度——无固定色值。
- **fixture 流式回答**（`[fixture:slow]`）在生成中/完成后均正常展示。

## 4. 已自动验证 vs 需人工验收

**已自动验证**：渲染器正确性（单元测试 20 项）、流式健壮性（单元测试 + 浏览器实测）、XSS 防护（单元测试 + 浏览器实测）、白名单边界（表格/HTML 降级）、blocks/fixture 路径兼容（conversation 24 项 + 浏览器实测）、Rust 提示词联动（测试 124 项）、构建与静态检查全绿。

**仍需真实 Tauri / DeepSeek 人工验收**：

1. **真实 Tauri 窗口视觉**：代码块、列表、引用、链接在真实主题（ReadRay Default 及切换后的深色主题）下的最终观感；流式生成过程中渲染稳定性（本机浏览器预览是 fixture 数据，真实窗口需确认无闪动/重排）。
2. **真实 DeepSeek 输出形态**：提示词更新后模型实际输出的 Markdown 结构（列表/代码块/粗体出现频率、表格是否出现、链接格式是否符合 `[text](https://…)` 白名单），以及多轮对话中渲染稳定性。
3. 设置页存量失败项（见第 5 节）与本任务无关，但验收时可一并留意。

## 5. 未验证的风险和遗留问题

- **test:settings 存量失败 1 项**（`正式设置页保持五类设计结构…`）：断言 `writing-page.css` 编辑器标题字号为 `34px`，实际文件为 `32px`。已用 `git stash` 在**干净工作区（无本任务改动）复跑确认同样 37/1**，与本任务无关，未修复（超出任务范围，留待调度者处理）。
- **行尾问题（已修复，供记录）**：验证过程中 `git stash/pop` 在 `core.autocrlf=true` 下把工作区文件转成 CRLF，导致既有 CSS 正则测试失败；已把所有改动文件恢复为 LF 并复跑全绿。报告此现象供调度者知悉。
- **表格未纳入白名单**：真实模型若频繁输出表格，需按任务书约定重新评估（可在任务 3 或后续任务中追加）。
- **链接不可点击**：按任务书默认处理（可见文本 + 完整 URL），不引入 opener 与安全边界；如后续需要可点击链接，需走 Tauri opener 权限评估。
- **未闭合语法在完成态的展示**：完成态未闭合代码块/行内标记保留原文标记字符（诚实展示），真实 DeepSeek 极少产生此形态，如验收中发现影响观感可再调整。
- **浏览器预览验证的边界**：预览基于 fixture 数据（不请求 DeepSeek），页面级 XSS/流式实测是直接渲染验证，与真实 Tauri 的差异仅为数据源与窗口环境。
