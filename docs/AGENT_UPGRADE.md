# ReadRay 内置 Agent 体验升级计划

最后更新：2026-08-07

> 本文件是"ReadRay 内置 Agent（Quick AI / 对话）体验升级"的讨论与任务执行档案。执行任务由独立会话承担，本文件作为该会话的任务书与验收依据；调度者负责方向与验收。

## 目标与背景

ReadRay 的对话链路（Quick AI）目前是纯文本、非流式、单段展示，系统提示词为静态常量。为提升"Agent 感"与可用性，按以下顺序逐项升级：

1. **流式输出**（含真实可停止）
2. **对话页面 Markdown 渲染**
3. **系统提示词构建方式**（组合式上下文，另行专门讨论）

执行顺序已与调度者确认；每完成一项，执行会话回报结果，调度者负责验收并更新本文件。

## 已确认决策

- 执行顺序：先流式输出 → 再 Markdown 渲染 → 最后专门讨论系统提示词构建。
- 流式与 Markdown 是同一链路（Rust Quick AI ↔ 前端 conversationService/ConversationPage）的连续升级，但作为两个独立任务分别完成、分别验收。
- 每个任务完成后，`docs/HANDOFF.md` 与 `docs/RESOURCE_MAP.yml` 由调度者更新。
- **本轮不实现、不启用记忆注入**：对话继续维持"不声称能访问本地学习记录/联网/长期记忆"的诚实边界。该边界在系统提示词任务中强化落实，接入时机等待阶段八复习模型与真实检索信号落地后再评估。
- **"重新生成"沿用覆盖式语义**（与 ChatGPT 主流一致）：重新生成会替换当前 assistant 回答，不保留多版本分页历史。理由：与 ChatGPT/主流客户端产品语义一致、实现与上下文管理简单；多版本保留会引入版本分页 UI 与检索复杂度，且当前对话上下文是纯追加式，为后续阶段留有余量。**本轮任务不实现"重新生成"**，仅确认语义边界，留给后续任务。

## 任务 1：流式输出（已完成并验收通过）

### 目标

Quick AI 对话从"一次性完整返回"升级为"SSE 流式增量返回"，并支持真实可停止；重试/重启恢复语义保持现状不变（user 已持久化、assistant 保持 pending 可重试）。

### 现状

- Rust 侧：`src-tauri/src/quick_ai.rs` 的 `send_quick_ai_message` 用 `post_tracked_chat_completion` 一次性请求完整回答后 `complete_turn`；`build_quick_ai_request_body` 固定 `"stream": false`；系统提示词当前要求"不要依赖 Markdown 渲染"（不随本任务改动）。
- 前端：`conversationService.ts` 的 `RepositoryConversationService` 是 `COMPLETE_DELIVERY_CAPABILITIES`（`delivery: "complete"`、`canStop: false`）；`ConversationPage` 的 `GenerationState` 已有 chunks/streamRemainingChunks 模拟流（fixture 用 520ms 定时器播放 chunks），正式路径走 `delivery === "complete"` 分支直接整体替换。

### 技术要求

**关键约束：复用现有轮次协议与 usage 统计机制，不改变消息持久化语义。**

- **传输**：用 Tauri `tauri::ipc::Channel` 把流式增量从 Rust 推送到前端，替代一次性返回值。拒绝自制 IPC 通道（如 stdout/文件/轮询）。
- **命令形态**：新增异步命令 `send_quick_ai_message_streaming(app, conversation_id, expected_user_sequence, content, channel)`（或等价命名）。`send_quick_ai_message` 保留，供测试与兼容；streaming 命令走同一 `prepare_turn` → 流式请求 → `complete_turn` 链路。
- **协议**：channel 只推送明确的协议事件（增量文本、完成、错误、中止），Rust 侧不向前端传 JSON 反序列化失败等中间态。
- **usage 统计**：流式响应结束时仍须计入 `model_usage_records`（QuickAi 分类）。SSE 流的 usage 通常在最后一个 chunk（或流结束标记）里；实现时复用 `deepseek_client` 的校验与记录逻辑，若现有 `post_tracked_chat_completion` 无法直接复用，在 `deepseek_client` 中新增流式请求函数，**保持同一 usage 严格校验与尽力写入语义**（合法 usage 即使业务解析失败也计入；统计写入失败不影响业务结果）。
- **停止语义**：
  - 前端可取消时，Rust 收到 channel 的取消/中止信号后终止模型请求。
  - 停止后：已流出的部分（若有）保存为 assistant 消息，或保持 pending 可由用户重试——与现有"模型失败保留 pending user"语义一致；**不得伪造完整回答**。
  - 停止按钮仅在流式、生成中可见可点（现状 UI 已有该结构，但 `canStop: false`；本任务将其接通为真实语义）。
- **错误与重试**：HTTP/网络/解析错误沿用现有分类与中文错误信息；重试沿用现有 `expected_user_sequence` 幂等协议。
- **模型上下文截断、max_tokens、temperature 保持现状**，不随本任务调整。

### 涉及文件（预计）

- `src-tauri/src/deepseek_client.rs`：新增流式请求函数（SSE 解析 + usage 校验/记录）。
- `src-tauri/src/quick_ai.rs`：新增 streaming 命令与流式上下文装配（复用 `build_request_messages`）。
- `src-tauri/src/lib.rs`：注册新命令。
- `src/conversationRepository.ts`：新增 streaming 调用（channel 订阅映射）。
- `src/conversationService.ts`：正式 service 增加流式路径，`capabilities` 更新为可停止。
- `src/components/ConversationPage.tsx`：正式路径接入真实流（替换/补充现有 `streamRemainingChunks` 模拟逻辑）。
- 测试：`tests/conversationService.test.mjs` 扩展；Rust 侧 streaming 相关测试（离线可测部分）。

### 验收标准

- 真实 Tauri 中，Quick AI 回答边生成边显示，非一次性出现。
- 生成过程中可点"停止生成"，停止后不出现伪造完整回答，且重新发送/重试语义与现状一致。
- 流式路径下 usage 统计仍正确计入"近 7 天/近 30 天/全部"（设置页使用量）。
- 重启恢复：生成中退出/崩溃后，user 消息保留、可重试（与现状 pending 语义一致）。
- 现有自动化测试全绿：前端（会话/写作/设置）+ Rust 全部通过；`pnpm build`、`cargo fmt --check`、`cargo check` 通过。
- 既有 `send_quick_ai_message` 非流式路径不受影响（或按任务结论决定保留/迁移）。

### 未决/后续

- 是否移除旧的非流式命令，由本任务结论决定（默认保留，避免破坏现有调用方与测试）。
- 停止后的"已生成部分保存"与"保持 pending"两者取舍，由实现按现有语义选择并说明理由。

### 验收结论（2026-08-06，调度者）

- **代码审查通过**：流式实现符合任务书。channel 事件协议四类事件；abort 标志注册表 + active 流门控；usage 严格校验与尽力写入保持非流式语义；SSE 解析器覆盖跨 chunk 缓冲、`[DONE]`、usage-only chunk、多 choices 拒绝、坏 JSON 拒绝；停止后保持 pending 可重试（与模型失败语义一致）；旧非流式命令保留。测试 24/24（会话）、Rust 122 通过、build/fmt/check 通过；报告数字与本机复跑一致。
- **发现真实 API 行为边界（用户报错根因）**：`deepseek-v4-flash` 是推理型模型（实测 `completion_tokens_details.reasoning_tokens` 非零，且 `delta.reasoning_content` 出现）。极端输入（如只问一个单词、浅层问题）下可能触发模型"纯推理、零内容"响应：**最后一个 chunk 只带 `finish_reason:"stop"`、无 usage 字段**。此时流式链路按任务书"缺 usage 视为错误"返回"DeepSeek 模型流式响应缺少 usage，无法计入使用量。"——用户实际遇到"DeepSeek 模型响应缺少 usage。你的输入仍然保留，可以直接重试。"即由此产生（模型请求本身成功，非额度问题）。
- **实际 bug 根因（已修复，2026-08-06）**：上述"推理模型零内容"解释是错误的。复核代码发现真正原因是：`stream_quick_ai_reply` 把 SSE 最后一个 chunk 的 **usage 对象本身**（`{"prompt_tokens":…}`）传给 `parse_model_token_usage`，而该函数期望**带 `usage` 键的完整响应体**，`.get("usage")` 永远取不到 → **每一次真实流式回答都会报"缺少 usage"**，与输入内容、系统提示词长度无关。修复：`deepseek_client` 新增 `parse_model_token_usage_value`（直接解析 usage 对象），流式路径改用它，原 `parse_model_token_usage` 保留给非流式完整响应体。新增 2 项单元测试覆盖流式最终 chunk 形状与不一致拒绝。验证：Rust 124 通过（原 122 + 新 2）、会话前端 24 通过、build/fmt/check 通过。
- **残余风险**：`deepseek-v4-flash` 推理模型"纯推理、零内容"导致无 usage 的真实边界仍存在（修复前被主 bug 掩盖）；按用户决策（方案 1），流式路径对缺失 usage 已降级为不记录使用量、仍保存回答，与"统计写入失败不影响业务结果"哲学一致。
- **待用户拍板的修复选项（已按方案 1 执行）**：
  1. 缺 usage 时降级为**不记录使用量、仍保存回答**（usage 统计会少计，但对话不中断）——推荐，符合"统计写入失败不影响业务结果"的项目哲学；
  2. 缺 usage 时重试一次（usage 可能随机缺失）——实现复杂、不保证命中；
  3. 换回非推理模型（`deepseek-chat`）——涉及模型选择决策，超出本任务范围。
- **与任务书冲突说明**：任务书原文"缺 usage 视为错误"基于非推理模型假设；真实 `deepseek-v4-flash` 推理模型行为与假设不符，属任务书技术要求与真实行为冲突，由调度者发现并记录。

## 任务 2：对话页面 Markdown 渲染（待执行）

### 背景

任务 1 流式输出已验收通过（2026-08-06）。当前对话页的 assistant 消息以纯文本段落展示（`mapMessage` 把完整回答塞进单个 paragraph 块），模型输出的 Markdown 标记（列表、粗体、代码块、表格等）原样显示，可读性差。系统提示词当前要求"不要依赖 Markdown 渲染"（`QUICK_AI_SYSTEM_PROMPT`），本任务需与其联动。

### 目标

对话页 assistant 回答支持**白名单子集的轻量 Markdown 渲染**，并同步调整系统提示词，让模型输出结构化内容。

### 范围边界（明确不做）

- 不引入重型 Markdown 渲染库（如 marked/markdown-it 的完整模式）；优先轻量自研或受控解析。
- 不执行任意 HTML / 脚本 / iframe / 图片 / 远程资源；渲染结果只包含受控元素与文本。
- 不做代码高亮、LaTeX、数学公式、HTML 标签透传、超链接跳转下载等扩展能力。
- 不改变流式链路、消息持久化、停止/重试语义（任务 1 已验收，不动）。
- 不改变解释卡（ExplanationCard）与写作分析的 JSON 结构化协议；Markdown 渲染只作用于 Quick AI 对话的 assistant 文本。
- 用户消息保持现有纯文本气泡，不渲染 Markdown。

### 渲染子集（白名单，需与调度者确认后细化）

支持以下 Markdown 子集的渲染，其余一律按纯文本处理或忽略标记：

- 段落与换行；`#`/`##`/`###` 标题
- `**粗体**`、`*斜体*`、`~~删除线~~`（如支持）
- 行内代码 `` `code` `` 与多行代码块 ```` ``` ````
- 无序列表 `-` / 有序列表 `1.`
- 引用 `>`
- 链接 `[text](url)`（仅渲染为文本 + 可见 URL，或默认不渲染）
- 分隔线 `---`
- 表格（`|` 语法，可列为非必须项，先确认模型输出中表格出现频率）

### 实现约束

- **安全边界与主题协议一致**：渲染器输出只生成受控 HTML（或 React 元素），输入永不作为 HTML 注入；对任何未知/畸形语法降级为纯文本。所有样式使用既有 `rr-conversation-*` 作用域与语义 token（`--rr-main-fg` 等），不引入固定色值、不改变布局/字体/字号体系。
- **流式兼容**：生成过程中 delta 逐段到达，渲染器必须对**不完整 Markdown 片段**健壮（例如代码块/列表只写了一半时不得崩坏或闪现原始标记）；需设计"流式下的渲染策略"（如拼接后整体渲染，或对未闭合元素降级），并在实现中说明选择。
- **与系统提示词联动**：`QUICK_AI_SYSTEM_PROMPT` 去掉"不要依赖 Markdown 渲染"，改为"可以使用简洁的 Markdown 结构化输出（列表、代码块、粗体、链接）"，并仍保持"不要声称访问互联网/本地记忆"等诚实边界。注意：**该提示词改动属于任务 2 的联动部分**，不是任务 3 的系统提示词重建；任务 3 仍按原计划专门讨论组合式构建。
- **现有 blocks 协议**：`ConversationAnswerBlock`（paragraph/list/example）是设计稿 fixture 时代的中间协议；真实路径的 assistant 消息目前整体映射为单 paragraph。本任务需决定：是保留 blocks 协议（渲染发生在映射后）还是在页面渲染层直接处理原始文本。选择需说明，并保持 fixture 路径（`conversationFixtureService.ts`）可继续工作。

### 涉及文件（预计）

- `src/` 新增 Markdown 渲染模块（解析器 + 渲染组件，职责单一，独立测试）。
- `src/conversationService.ts` / `src/conversationViewModel.ts`：assistant 消息渲染协议（如新增 `markdown` 文本类型或渲染入口）。
- `src/components/ConversationPage.tsx`：assistant 消息改用渲染组件（含流式 GenerationMessage 实时渲染）。
- `src-tauri/src/quick_ai.rs`：`QUICK_AI_SYSTEM_PROMPT` 联动修改。
- `src/styles/conversation-page.css`：新增渲染元素样式（代码块、列表、引用、表格等，均用既有 token）。
- 测试：渲染器单元测试 + 流式健壮性测试；Rust 提示词测试（`system_prompt_keeps_general_help_and_english_expertise_balanced` 等需同步）。

### 验收标准

- 真实 Tauri 中，模型回答中的列表/代码块/粗体/链接等正确渲染，无原始标记裸露、无乱码。
- 流式生成过程中渲染稳定：未完成片段不崩坏、不闪现原始标记。
- 任意用户输入/模型输出不触发 XSS 或非白名单 HTML 注入（测试覆盖恶意输入）。
- 系统提示词更新后，模型输出的结构化程度提升，且诚实边界（不声称访问互联网/本地记忆）保留。
- 现有测试全绿：前端（会话/写作/设置）+ Rust 全部通过；`pnpm build`、`cargo fmt --check`、`cargo check` 通过。
- fixture 预览路径（非 Tauri 浏览器预览）仍可正常展示设计稿示例。

### 未决/后续

- 表格渲染是否纳入白名单（视模型实际输出频率与实现成本）。
- 链接渲染策略（纯文本 vs 可点击，可点击需考虑 opener 能力与安全边界）。

## 任务 2：对话页面 Markdown 渲染（已完成并验收通过）

### 验收结论（2026-08-07，调度者）

- 任务 2 已由用户在真实 Tauri 中验收通过：Markdown 渲染正常、换行修复生效、200 词段落生成约 16 秒（性能优化前约 1 分钟）；任务 2 正式收口。

- **代码审查通过**：白名单渲染器职责单一、无 React 依赖、可独立测试；安全边界双层防护（解析层协议白名单 + React 转义，链接仅 http/https 且不可点击）；流式"拼接后整体渲染 + 未闭合降级"策略；blocks 协议双入口保留（fixture 预览不受影响）；系统提示词联动正确、诚实边界保留；测试 20（渲染器）+ 24（会话）+ 124（Rust）全绿，build/fmt/check 通过。
- **换行问题修复（用户报告 + 审查发现）**：
  1. 代码块/文本块不换行（用户报告主因）：`.rr-conversation-code-block code` 的 `white-space: pre` 只横向滚动不换行；已改 `pre-wrap + overflow-wrap: anywhere`。
  2. 流式生成中不换行（审查发现）：`.rr-conversation-generation-row` 的 `white-space: nowrap` 继承到生成中回答内容；已改 `normal`。
  3. 正文软换行丢失与超长内容溢出隐患（审查发现）：`.rr-conversation-assistant-copy p` 补 `white-space: pre-wrap` 保留模型输出的多行换行；`.rr-conversation-assistant-copy` 补 `overflow-wrap: anywhere`。
  - 修复后验证：会话 24、渲染器 20、Rust 124 通过，`pnpm build`、`cargo fmt --check` 通过。真实 Tauri 换行观感仍需人工确认。
- **finish_reason=length 截断误报修复（2026-08-07）**：用户遇到"DeepSeek Quick AI 生成未正常结束：finish_reason=length"。根因：`QUICK_AI_MAX_TOKENS = 2048` 硬编码，模型回答超过上限时标准行为是返回 `finish_reason=length` 截断（已生成部分仍有效），但代码把 `length` 当错误丢弃。修复：流式与非流式均把 `length` 降级为截断——正常消费完流、已生成部分保存为完整回答；新增 `truncated` 状态（Rust `Truncated` 事件、service `status: "truncated"`、页面 `truncated` 阶段），展示"回答达到长度上限被截断"提示 + 继续生成按钮；未知 finish_reason（如 content_filter）仍按错误处理。验证：前端 25（+1 truncated 测试）、Rust 126（+2 length/unknown 测试）、build/fmt/diff 通过。
- **QUICK_AI_MAX_TOKENS 调整 2048 → 8192（2026-08-07）**：调研确认 2048 远低于 DeepSeek 官方文档上限（8192，实测 `deepseek-v4-flash` 对 4096/8192/16384/32768 全部接受），且 `deepseek-v4-flash` 为推理模型（输出含 reasoning token，实测 190 completion 中 147 为推理），2048 实际留给正文的预算更少，长回答易触发 length 截断。max_tokens 是上限非目标，提高不改变正常回答长度与成本，只为长文本输出留空间；ReadRay 学习对话场景按文档上限 8192 设置，未采用 Claude Code 的 32K（其任务单次输出可达 8K+，场景不同）。单次完整输出质量优于多轮续写，超 8K 的长文本续写留待任务 3 讨论清单。验证：Rust 126 通过、fmt/check 通过；前端无依赖（仅主题 sourceUrl 长度校验引用 2048，无关）。
- **流式渲染性能优化（2026-08-07）**：用户反馈 200 词段落生成约 1 分钟。实测同参数直连 DeepSeek：200 词英文段落总耗时仅 **4.8 秒**（响应头 736ms、首 token 898ms、81 token/s），确认瓶颈在 ReadRay 前端而非模型/网络。前端累积性能问题：① `.rr-conversation-assistant-copy` 的 `text-wrap: pretty` 让每个 delta（~12ms 一次）都触发全量优化断行重排，随文本增长平方级变贵；② 每个 delta 强制 `scrollTop = scrollHeight`，用户上翻阅读时也被拉回底部。修复：去掉 `text-wrap: pretty`（改默认 normal，视觉几乎无感）；滚动改为"距底部 < 80px 才自动跟随"，用户主动上翻时暂停跟随。delta 节流（合并渲染）评估为"消除昂贵操作后的备选项"，未实施，留待实测仍有卡顿再加（50ms 左右保持平滑）。验证：会话 25、渲染器 20、build/diff 通过；真实 Tauri 生成速度需用户实测确认。

## 任务 3：系统提示词构建（已完成并验收通过）

### 验收结论（2026-08-07，调度者）

- 任务 3 已由用户在真实 Tauri 中验收通过：效果相比之前有明显提升；任务 3 正式收口。
- 代码审查通过：`src-tauri/src/quick_ai_prompt.rs` 实现质量高——5 分节常量 + `build_quick_ai_system_prompt()` 组合式组装，完全对齐研究结论（Claude Code / Codex / OpenCode / Pi 的组合式哲学）；诚实边界从"不要声称 X"升级为"负面 + 正面替代 + 回退行为"；output_format 精确对齐渲染器白名单（支持清单 + 表格/HTML/四级+标题/图片负面清单 + http/https 链接约束）；`<readray_context>` 空插槽 + `QuickAiDynamicContext` 预留；推理非空规则 + `reasoning_content` 捕获诊断日志（`READRAY_QUICK_AI_REASONING_SEEN/ONLY`）缓解并观测"纯推理零内容"边界。解释卡与写作分析提示词保持独立，未并入。
- 验证：Rust 137 通过 / 4 ignored（+11：prompt 10 + reasoning 1）、前端 25/30/38 全绿、build/fmt/check/diff 通过；4 项真实 DeepSeek live 测试（白名单、诚实边界、两轮上下文、解释卡回归）29s 通过。调度者复跑稳定全绿（首次偶发 1 失败为并行 build 资源竞争，连续 7 次单独跑全绿）。

### 背景与研究结论

任务 1（流式）、任务 2（Markdown 渲染）已验收通过。本任务把 Quick AI 的系统提示词从单一常量重构为"组合式 + 可注入"结构。2026-08-07 完成深度研究（Workflow，8/9 成功），基于 Claude Code / Codex CLI / OpenCode / Pi 的源码实证，核心结论：

- **没有任何顶级 Agent 用一条巨型常量提示词**——都是"静态基础 + 动态片段"组合式装配（Codex world-state 差分、Claude Code 精简基础 + 指令下沉、OpenCode 三明治拼装、Pi 标记化上下文块）。ReadRay 当前的 `QUICK_AI_SYSTEM_PROMPT` 单一常量正是反面教材。
- **Claude Code 为 Claude 5 系列删掉 80%+ 系统提示词，评测无损失**——"让判断力强的模型更简单"；删除低信号条款优于不断追加。
- **能力诚实是结构化而非声明式**——负面声明（无网络/无工具/无本地记忆）必须搭配正面替代与回退行为。
- **输出规则必须对齐真实管线**——ReadRay 的 Markdown 白名单渲染器（`src/markdownParse.ts`）就是真实能力，应精确写进提示词，模型就不会输出渲染不了的内容。
- **注入上下文用标记分隔**（如 `<readray_context>…</readray_context>`）——为未来记忆/学习画像注入预留插槽，模型把注入内容当数据而非指令。
- **提示词是软件工件**——分节、版本化、可测试；先写行为测试再写提示词。
- **一个触点一个 base**——对话 / 解释卡 / 写作分析各有自己的定制 base，不共享常量（解释卡与写作分析已各自正确，本任务不动它们）。

### 目标

把 Quick AI 系统提示词重构为：**静态 base（分节常量）+ 动态片段（可注入）+ 标记包裹**，由 `build_quick_ai_system_prompt()` 运行时组装。

### 推荐组装结构（单条 system message，静态→动态）

```
[persona]      一句话身份：通用助手 + 英语专长（≤2 行，第二人称、正面、不吹嘘）
[behavior]     专长分工 + 提问策略（直接回答普通问题；英语学习/考试/写作/翻译做专家教练；个性化计划缺上下文时只问 2-4 个必要问题；匹配用户语言）
[output_format] Markdown 白名单精确对齐 src/markdownParse.ts + 推理模型"必须产出非空回答"规则
[boundaries]   正面框架的诚实边界（无网络/无工具/无本地记忆 → 能做什么 + 做不到时的替代 + 回退行为）
[context]      动态槽 <readray_context>…</readray_context>（当前空，预留未来记忆/画像注入）
```

### 实现约束

- **组合式重构**：新建 `src-tauri/src/quick_ai_prompt.rs`，用命名分节常量（`QUICK_AI_PERSONA`、`QUICK_AI_BEHAVIOR`、`QUICK_AI_OUTPUT_FORMAT`、`QUICK_AI_BOUNDARIES`、`QUICK_AI_CONTEXT_MARKERS`）与 `pub fn build_quick_ai_system_prompt(context: &QuickAiDynamicContext) -> String`。在 `quick_ai.rs` 删除 `QUICK_AI_SYSTEM_PROMPT` 常量，`build_request_messages` 调用 builder（保持 `messages[0].role == "system"`，DeepSeek 用单条 system message）。`lib.rs` 添加 `pub mod quick_ai_prompt;`。
- **诚实边界升级**：从"不要声称能访问本地记忆"升级为"负面 + 正面替代 + 回退行为"。草案：`You run locally inside ReadRay. You have no tools and no internet access: do not claim you can browse the web, open other apps, or call external tools. You cannot read the user's local files, learning records, or long-term memory: do not claim to remember the user's past study history, saved words, or cards from other conversations. If asked to do something you cannot do, or asked for facts you do not know, say so briefly and honestly, then offer the closest useful alternative. Do not invent or fabricate dictionary definitions, translations, or exam facts.`
- **output_format 精确对齐渲染器**：列出支持子集（`#`/`##`/`###` 标题、`**粗体**`、`*斜体*`、`~~删除线~~`、行内 `code`、多行代码块、`-`/`1.` 列表、`>` 引用、`---` 分隔线、`[text](https://…)` 链接），并**明确列出会以纯文本显示的内容**（表格、HTML 标签、`####`+ 标题、图片）——模型就不会输出渲染不了的内容。链接只接受 http/https。
- **推理模型规则**：`deepseek-v4-flash` 是推理模型，`reasoning_content` 会被 SSE 路径丢弃（`StreamChunkDelta` 只读 content）。提示词加"永远不要返回空回答；推理是内部过程，绝不展示给用户；即使简单问题也必须产出实际内容"——直接缓解"纯推理零内容"边界。
- **动态上下文插槽**：新增 `QuickAiDynamicContext` 空结构体（预留 `learning_profile` / `recent_memory`），builder 拼接 `<readray_context>…</readray_context>` 标记片段，当前空、空回退渲染无内容。未来开启记忆注入只需填充该结构体，提示词文本不动。**本轮不注入记忆**（已确认决策）。
- **不注入日期**：保持诚实边界（"今天"会诱导模型答当前事件）且保持前缀稳定（Pi 因缓存稳定性删除了日期）。
- **保持会话历史注入**：最近 ≤40 条、user 开头的真实消息注入方式不变。
- **不新增依赖**。解释卡（`explanation_card_system_prompt`）与写作分析（`writing_analysis_system_prompt` / `writing_answer_system_prompt`）各自的系统提示词**不并入本任务**，保持独立（一致性重构留作后续可选）。

### 涉及文件（预计）

- `src-tauri/src/quick_ai_prompt.rs`（新增）：分节常量 + builder + `QuickAiDynamicContext` + 测试。
- `src-tauri/src/quick_ai.rs`：删除 `QUICK_AI_SYSTEM_PROMPT`，改用 builder；`StreamChunkDelta` 可选补 `reasoning_content` 捕获（仅捕获，不转发）。
- `src-tauri/src/lib.rs`：注册新模块。
- 测试：`src-tauri/src/quick_ai_prompt.rs` 内单测（组装顺序、静态→动态、标记、空回退、输出格式白名单契约、诚实边界正面框架）+ `quick_ai.rs` 既有提示词测试同步。

### 验收标准

- 现有测试全绿：Rust（原 126 + 新增）、`pnpm test:conversation` / `test:writing` / `test:settings`、`pnpm build`、`cargo fmt --check`、`cargo check`、`git diff --check`。
- 新增契约测试：组装后的提示词包含 persona 行、行为策略、诚实边界（负面 + 正面替代 + 回退）、Markdown 白名单（含"不支持表格/HTML"的负面清单）；标记存在且空回退无内容。
- 真实 DeepSeek（`deepseek-v4-flash`）验证（live 测试或用户人工）：模型按白名单输出（无表格/HTML/超深标题），始终产出非空回答，诚实边界生效（要求联网/查记忆时如实拒绝）。
- 提示词不注入日期；会话历史注入方式不变。
- 对话/解释卡/写作分析三个触点提示词保持独立。

### 未决/后续

- `reasoning_content` 捕获是否实现（仅验证丢弃，不强求）。
- 解释卡与写作分析提示词的一致性重构（拆分静态/动态）留作后续可选。
- 未来记忆注入时填充 `QuickAiDynamicContext`，并做字节预算截断（参考 Codex `project_doc_max_bytes`）。
- 超 8K 长文本的多轮续写方案（任务 2 遗留，本任务不实现，仅记录）。
