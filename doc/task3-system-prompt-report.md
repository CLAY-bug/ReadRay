# 任务 3：系统提示词构建 — 完成报告

- 任务书：`docs/AGENT_UPGRADE.md`「任务 3：系统提示词构建」
- 执行日期：2026-08-07
- 状态：实现完成，自动验证与真实 DeepSeek live 验证均通过

## 1. 改了哪些文件

| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/quick_ai_prompt.rs` | **新增**。分节常量（`QUICK_AI_PERSONA` / `QUICK_AI_BEHAVIOR` / `QUICK_AI_OUTPUT_FORMAT` / `QUICK_AI_BOUNDARIES` / `QUICK_AI_CONTEXT_MARKERS` + `QUICK_AI_CONTEXT_END_MARKER`）、`QuickAiDynamicContext` 空结构体（预留 `learning_profile` / `recent_memory`）、`build_quick_ai_system_prompt()` builder、10 项模块内单测 |
| `src-tauri/src/quick_ai.rs` | 删除 `QUICK_AI_SYSTEM_PROMPT` 常量；`build_request_messages` 改为调用 `build_quick_ai_system_prompt(&QuickAiDynamicContext::default())`（`messages[0].role == "system"` 保持）；`stream_quick_ai_reply` 捕获 `chunk.reasoning` 仅验证丢弃（不转发 UI）；同步更新既有提示词测试；新增 1 项真实 DeepSeek live 测试（markdown 重负载白名单）+ 1 项 live 测试（诚实边界） |
| `src-tauri/src/deepseek_client.rs` | `StreamChunk` 新增 `reasoning: Option<String>` 字段；`StreamChunkDelta` 解析 `delta.reasoning_content`（与 `content` 分离，不混入回答）；仅含推理无正文的 chunk 也产出（供调用方统计"纯推理零内容"）；新增 1 项单测 |
| `src-tauri/src/lib.rs` | 添加 `pub mod quick_ai_prompt;` |

未修改：解释卡（`explanation_card_system_prompt`）与写作分析（`writing_analysis_system_prompt` / `writing_answer_system_prompt`）的系统提示词保持独立；未新增任何依赖；未修改 AGENTS.md / docs/ 下文档（由调度者更新）。

## 2. 实现要点

### 2.1 分节结构与 builder

五个命名分节常量按「静态 → 动态」顺序组装成单条 system message（DeepSeek 用单条 system message，`messages[0].role == "system"` 不变）：

```
QUICK_AI_PERSONA
QUICK_AI_BEHAVIOR
QUICK_AI_OUTPUT_FORMAT
QUICK_AI_BOUNDARIES
<readray_context>…</readray_context>   ← 动态插槽（当前空）
```

`build_quick_ai_system_prompt(context: &QuickAiDynamicContext) -> String` 用 `write!` 拼接分节（每节之间空行），最后附加上下文标记。分节常量是 `pub const`，可在单测与未来其他装配点复用。

### 2.2 诚实边界（负面 + 正面替代 + 回退）

采用任务书草案措辞（`QUICK_AI_BOUNDARIES`）：

- **负面声明**：`You run locally inside ReadRay. You have no tools and no internet access: do not claim you can browse the web, open other apps, or call external tools. You cannot read the user's local files, learning records, or long-term memory: do not claim to remember the user's past study history, saved words, or cards from other conversations.`
- **正面替代 + 回退行为**：`If asked to do something you cannot do, or asked for facts you do not know, say so briefly and honestly, then offer the closest useful alternative. Do not invent or fabricate dictionary definitions, translations, or exam facts.`

### 2.3 output_format 精确对齐渲染器白名单

`QUICK_AI_OUTPUT_FORMAT` 把 `src/markdownParse.ts` 支持的全部子集逐项列出，并把不支持的内容明确写成"以纯文本显示"的负面清单：

- **支持（会正确渲染）**：`#`/`##`/`###` 标题、`**粗体**`、`*斜体*`、`~~删除线~~`、行内代码 `` `code` ``、多行代码块 ```` ``` ````、有序 `1.` / 无序 `-` 列表、`>` 引用、`---` 分隔线、`[text](https://…)` 链接（渲染为可见文本 + URL）
- **负面清单（不会渲染，按纯文本显示）**：表格 `|`、原始 HTML 标签、`####` 四级以上标题、图片
- **链接约束**：`links must use http or https only`（与解析器 `/^https?:\/\//i` 一致）

### 2.4 <readray_context> 插槽与 QuickAiDynamicContext

- `QuickAiDynamicContext` 为空结构体，预留 `learning_profile: Option<String>` / `recent_memory: Option<String>` 字段，默认全空（本轮不注入记忆）。
- builder 始终以 `<readray_context>…</readray_context>` 标记包裹：空上下文渲染为带标记的空插槽（模型读到的是占位而非指令）；单测断言空上下文时标记之间无任何内容。
- 未来开启记忆注入只需填充该结构体并做字节预算截断，提示词文本与装配点不动。
- **不注入日期**：单测断言组装提示词不含任何日期字符串（保持前缀稳定，避免诱导模型回答当前事件）。

### 2.5 推理模型非空规则

`QUICK_AI_OUTPUT_FORMAT` 末尾：`Never return an empty answer: reasoning is an internal process and must never be shown to the user; always produce actual content, even for simple questions.` —— 直接缓解 `deepseek-v4-flash` "纯推理零内容"边界。

### 2.6 reasoning_content 捕获（仅验证丢弃）

- `deepseek_client.rs`：`StreamChunkDelta` 新增 `reasoning_content` 字段（默认空），解析后放入 `StreamChunk.reasoning`；与 `delta.content` 严格分离，**绝不混入回答文本**；只有 `reasoning_content` 没有正文的 chunk 也会产出（此前会被 `delta.is_empty() && ...` 直接丢弃，现在可被调用方统计到）。
- `quick_ai.rs`：`stream_quick_ai_reply` 捕获 `chunk.reasoning` 置 `reasoning_seen`，**不转发任何事件给 UI**；回答非空时打 `READRAY_QUICK_AI_REASONING_SEEN=1`，回答为空时打 `READRAY_QUICK_AI_REASONING_ONLY=1`（用于真实环境确认"纯推理零内容"边界是否触发）。
- 非流式路径不受影响（`QuickAiChatResponse` 未解析 reasoning）。

### 2.7 未改动的部分

- 会话历史注入方式不变：最近 ≤40 条、user 开头（`QUICK_AI_MAX_CONTEXT_MESSAGES` 与截断逻辑未动）。
- 流式链路、消息持久化、停止/重试语义（任务 1）、Markdown 渲染（任务 2）未动。
- 解释卡 / 写作分析提示词未并入（保持各自 base 独立，一致性重构留作后续可选）。

## 3. 验证结果

### 自动验证（全部通过）

| 验证项 | 结果 |
| --- | --- |
| `cargo test` | **137 通过 / 0 失败 / 4 ignored**（原基线 126 + 新增 11：quick_ai_prompt 10 项 + deepseek_client reasoning 1 项；ignored 为 3 项既有 live + 1 项新增 live） |
| `pnpm test:conversation` | 25/25 通过 |
| `pnpm test:writing` | 30/30 通过 |
| `pnpm test:settings` | 38/38 通过 |
| `pnpm build` | 通过（3.38s） |
| `cargo fmt --check` | 通过（已自动格式化） |
| `cargo check` | 通过 |
| `git diff --check` | 通过（仅有 LF→CRLF 换行警告，非错误） |

新增契约测试覆盖：组装顺序（静态→动态）、persona 平衡、行为策略（2-4 个必要问题/语言匹配）、诚实边界（负面+正面+回退+不虚构）、output_format 白名单（支持清单 + "不支持表格/HTML/四级+标题/图片"负面清单 + http/https 链接）、推理非空规则、`<readray_context>` 标记存在且空回退无内容、动态注入内容落在标记内、不注入日期。`quick_ai.rs` 既有测试（`system_prompt_keeps_general_help_and_english_expertise_balanced`、`multi_round_request_contains_ordered_history` 等）已同步改用 builder 并更新措辞断言。

### 真实 DeepSeek 验证（live 测试，非模拟）

有 `.env` 真实 Key，全部 4 项 `#[ignore]` live 测试在 `deepseek-v4-flash` 上真实运行通过（29s）：

- `live_two_turn_quick_ai_conversation`（既有，两轮上下文 + codeword 记忆验证）✅
- `live_markdown_heavy_reply_stays_within_whitelist`（新增：要求富 Markdown 输出，断言回答非空、无表格/HTML/四级+标题/图片语法）✅
- `live_honesty_boundary_refuses_fabricated_abilities`（新增：要求联网搜新闻 + 查学习记录，断言模型如实拒绝而非虚构能力）✅
- `live_create_explanation_cards_with_flash_model`（既有，解释卡链路回归）✅

## 4. 已自动验证 vs 仍需人工验收

**已自动验证（含真实 DeepSeek）**：
- 提示词组装结构、分节顺序、诚实边界措辞、白名单契约、标记插槽（Rust 单测 + quick_ai.rs 装配测试）。
- 真实模型按白名单输出（无表格/HTML/深标题/图片）、非空回答、诚实边界生效（live 测试）。
- reasoning_content 捕获解析不混入正文（deepseek_client 单测 + 流式路径实现）。
- 前端三套、build、fmt、check、diff 全绿。

**仍需真实 Tauri / 人工验收**：
- 流式窗口中的实际观感：白名单语法在真实对话页渲染、流式生成中未闭合片段不闪现标记（任务 2 已验收，本轮提示词改动不影响渲染器，但模型输出结构化程度提升后的观感需用户确认）。
- 诚实边界在真实多轮对话中的行为（live 测试为单轮，多轮追问的边界表现建议人工抽验）。
- 诊断日志 `READRAY_QUICK_AI_REASONING_SEEN/ONLY` 的触发情况（真实窗口中使用时如遇"纯推理零内容"，可查 stderr 确认）。
- 超长/极端输入下模型仍遵守白名单（live 测试用富 Markdown 提示词覆盖了常见场景，但非穷举）。

## 5. 未验证的风险与遗留问题

1. **"纯推理零内容"边界仅部分缓解**：提示词加了非空规则，但无法从代码层保证 deepseek-v4-flash 永不触发；若触发，回答仍按任务 1 决策降级为不记录 usage、保留回答可重试。`READRAY_QUICK_AI_REASONING_ONLY=1` 日志用于真实环境确认是否仍发生。
2. **live 测试的断言是启发式**：诚实边界断言基于常见拒绝措辞的启发式匹配，模型措辞变化可能导致断言误报（测试失败）或漏报（模型恰好用其他措辞假装能力，目前未观察到）。表格检测同样依赖 `|` 行首启发式。
3. **多轮诚实边界**：live 诚实边界测试为单轮；多轮上下文中模型是否持续遵守（如用户两次追问"你真的查不到吗"）未自动覆盖。
4. **提示词文本测试是合同式的**：单测断言具体措辞，未来措辞修改需同步更新测试（这是"提示词是软件工件"的预期成本）。
5. **遗留**：解释卡与写作分析提示词的一致性重构（拆分静态/动态）留作后续可选；未来记忆注入需填充 `QuickAiDynamicContext` 并做字节预算截断；超 8K 长文本多轮续写方案仍记录在任务书未决区。
