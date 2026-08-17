# ReadRay Agent Runtime 升级方案

最后更新：2026-08-17

状态：任务 0/1/2/3 已完成并通过评审（任务 3 已正式收口，含真实 Tauri 使用验收）；下一步进入任务 4

## 1. 文档定位

本文定义 ReadRay 在进入阶段九之前的共享 Agent Runtime 升级方案。它解决的不是某一个联网功能，也不只服务 Quick AI 浮层，而是为主应用完整对话、Quick AI overlay 和作文中的 Writing Coach 提供同一套自主工具选择、循环执行、可靠恢复和安全边界。

本文术语约定：

- **通用对话 Agent**：主应用 `ConversationPage` 与 Quick AI overlay。两者入口和 UI 不同，但正式路径共享现有 Quick AI Rust/SQLite 会话后端，因此属于同一个首要迁移面。
- **Writing Coach**：作文页中的分析、问答和辅导 Agent。它使用独立的 `writing.rs`、文章 revision、完成版本和请求身份，是同一 Runtime 的第二个业务适配面，不能被当成普通 conversation 直接迁移。
- **Agent Runtime**：跨业务面共享的模型循环、工具、事件、预算、取消和运行记录内核；它不替代各业务自己的 repository/service 和持久化权威。

本文是新的架构与任务入口。`docs/AGENT_UPGRADE.md` 仍记录已经完成并验收的流式输出、Markdown 渲染和组合式系统提示词工作；其中“当轮不实现联网、工具调用和记忆注入”是当时任务的范围边界，不再阻止本文规划的新阶段。长期学习者记忆、熟练状态和个性化仍以 `docs/STAGE_NINE_LEARNER_MODEL_PLAN.md` 为准，本文不得提前实现。

本方案的核心结论：

> 参考 pi Agent Core 的循环、事件、工具和上下文协议，在 ReadRay Rust 后端实现原生 Agent Runtime；不直接嵌入 pi coding-agent，不引入 Node 或 Python 桌面运行时，不开放 Bash、任意文件读写或动态代码扩展。

## 2. 为什么现在需要升级

ReadRay 当前已经具备一套可靠的正式对话基础：

- Quick AI 与完整对话页使用真实 DeepSeek 和 SQLite。
- 用户消息先通过 `prepare_turn` 持久化，模型成功后再通过 `complete_turn` 追加 assistant。
- `conversationId + expectedUserSequence + userMessageId` 构成轮次身份，重试、重启和模糊成功不会重复写入 assistant。
- 回答支持 SSE 流式输出、真实停止、截断提示和白名单 Markdown 渲染。
- 系统提示词已经采用组合式分节构建，并预留受标记保护的动态上下文入口。
- API Key 由 Windows Credential Manager 管理，不进入 SQLite、普通日志或前端长期状态。

但当前链路仍然是一次模型请求：

```text
用户输入
  -> 持久化 pending user
  -> 取最近不超过 40 条 user/assistant 消息
  -> 调用一次 DeepSeek chat/completions
  -> 流式显示正文
  -> 持久化最终 assistant
  -> 结束
```

它还不具备现代 Agent 的关键运行时能力：

- 模型输出 tool call 后执行工具并继续推理的循环。
- `tool call`、`tool result`、来源、运行状态等内部消息类型。
- 工具 schema 校验、能力注册、激活列表和执行权限策略。
- 工具开始、进度、结束、失败等可观察事件。
- 对实时、外部或应用环境信息的自主判断与获取。
- 工具调用超时、批次并行、预算限制和确定性回传。
- Agent run/step 的持久化、崩溃恢复和副作用幂等性。
- token-aware 长上下文管理和持久化压缩。
- 运行中追加要求、steering 和 follow-up。

因此，联网只是第一项可见能力；真正需要建设的是通用、可靠、受约束的 Agent Runtime。

## 3. 产品目标

升级完成后，ReadRay Agent 应当能够：

1. 理解用户任务并判断当前信息是否足够。
2. 在已授权、已激活的工具中自主选择是否调用工具。
3. 使用工具结果继续推理，直到形成最终回答或达到明确终止条件。
4. 对实时公共信息自动检索并提供可追溯来源。
5. 对无需工具的稳定知识直接回答，不为了“显得像 Agent”滥用工具。
6. 在工具失败、取消或信息不足时诚实降级，不用模型记忆冒充最新事实。
7. 让用户看到真实的执行状态、来源和失败原因，但不展示私有思维链。
8. 在停止、重试、应用重启、IPC 丢失和迟到结果下保持一致。
9. 只允许安全、领域化能力进入产品，不向模型交出通用系统权限。

### 3.1 业务面范围

| 业务面 | 当前权威 | 在本方案中的位置 |
| --- | --- | --- |
| 主应用完整对话 | `ConversationService -> quick_ai.rs -> conversations.rs` | 首要迁移面，完整接入 Runtime |
| Quick AI overlay | 与主应用对话共享 Quick AI 会话后端 | 与主应用对话同步接入，不建立第二套 Agent |
| Writing Coach | `WritingService -> writing.rs`，文章/revision/version 为权威 | Runtime 稳定后的第二适配面，保留写作专属状态机 |
| ExplanationCard | 结构化解释卡协议与 validator | 继续使用专用管线，不因共享 Runtime 自动改成 Agent loop |
| Review 制卡 | Review 专用生成、缓存、退避和调度 | 继续使用专用管线，不因共享 Runtime 自动迁移 |

共享 Runtime 不要求所有 LLM 调用都变成 Agent。只有需要自主判断、工具循环和连续交互的业务面才接入；确定性的结构化生成继续保留现有专用协议。

面向用户的核心体验应是：

```text
提出任务
  -> Agent 判断信息缺口
  -> 必要时自动使用合适工具
  -> 展示简洁的执行状态
  -> 基于工具事实生成回答
  -> 给出来源或明确的失败说明
```

## 4. 非目标

本阶段明确不做：

- 不直接嵌入 pi coding-agent、pi CLI 或 pi TUI。
- 不在桌面应用中增加 Node、Python 或 Bun sidecar。
- 不开放 Bash、PowerShell、cmd 或任意进程执行。
- 不开放任意绝对路径的 read/write/edit。
- 不允许模型安装 npm、pip、Git 包或加载任意 TypeScript/Python 扩展。
- 不建设通用插件商店、Skill 市场或多 Agent 编排。
- 不实现浏览器自动化、登录态网页操作或任意外部账户写入。
- 不提前注入学习者画像、长期学习记忆或阶段九个性化排序。
- 不让模型直接写入熟练度、复习统计或任意 SQLite 表。
- 不用新的 JSONL 或前端状态替换现有 SQLite 会话权威。
- 不把 ExplanationCard 或 Review 的既有结构化模型协议自动迁入 Agent loop。
- 不在共享内核稳定前改写 Writing Coach；Writing 通过本文任务 6 单独适配，不与通用对话共用 conversation schema。

## 5. 技术路线决策

### 5.1 采用 Rust 原生 Runtime

Agent Runtime 在 Rust/Tauri 后端实现，React + TypeScript 只消费有类型的 service 和事件，不承担模型循环、权限判断或持久化权威。

选择 Rust 的原因不是追求可感知的模型推理加速，而是：

- 模型和网络延迟远大于本地状态机开销，Rust 不会成为回答速度瓶颈。
- 可以复用现有 DeepSeek 客户端、SQLite 事务、请求身份和取消边界。
- 不引入第二个运行时、子进程生命周期、IPC 密钥传递和双重会话存储。
- 更容易用类型和状态机约束工具参数、运行状态和终止条件。
- 网络请求、工具执行、取消和迟到结果可以在同一权威边界内收口。
- 保持普通 Windows 安装包，不要求用户安装开发环境。

Rust Agent 生态比 Python/TypeScript 小，主要影响开发速度和现成 provider adapter 数量，不会损害 Agent 的执行质量。第一版必须严格控制内核规模，不能在仓库内重造一个通用 Agent 框架。

### 5.2 参考 pi，但不复制产品外壳

本次研究基于 pi 仓库提交 `086c32e74530564922d011ade23ff582c9d63116`。值得借鉴的机制：

- Agent/turn/message/tool 的事件生命周期。
- 模型输出工具调用后继续执行的外层与内层循环。
- AgentMessage 与 provider message 的转换边界。
- 工具注册和当前激活工具分离。
- schema 校验、执行前后 hook、标准化错误和增量进度。
- 多个只读工具并行执行、结果按原 tool call 顺序回传。
- steering/follow-up 队列和 abort 后等待 idle。
- token-aware compaction 和可恢复 session entry 思想。

不采用的部分：

- coding-agent 的 CLI、TUI、交互模式和 workspace 产品逻辑。
- 默认 `bash/read/write/edit` 工具。
- JSONL session manager 作为 ReadRay 持久化层。
- 无沙箱的 Extension、Skill 脚本和 Package 安装能力。
- Node 运行时及其 sidecar/RPC 装配。

pi 的默认现代感来自稳定循环和可插拔工具，而不是内置 Web Search。ReadRay 必须自行实现适合普通桌面用户的安全策略。

### 5.3 模型自主决策，运行时负责约束

正常使用中不在输入框提供“联网开/关”按钮。模型根据任务和当前能力自主决定是否调用低风险只读工具。

需要区分两个职责：

- 用户决定应用是否被允许使用某一类能力，例如全局隐私设置禁止网络访问。
- 在已经授权的能力集合内，模型决定当前任务是否需要使用工具。

运行时仍需提供确定性的环境事实、能力清单和规则，不能只靠模型自由猜测。对于本机可以可靠获得的信息，优先使用本地事实或本地只读工具；对于最新公共事实，使用网络工具；对于稳定知识，直接回答。

## 6. 核心设计原则

1. **SQLite 继续是权威**：页面状态、流式文本和模型返回都不能成为第二套持久化事实。
2. **内部消息与展示消息分离**：工具步骤和来源可以参与模型上下文，但不必伪装成 user/assistant 气泡。
3. **能力注册与激活分离**：工具存在不代表当前 run 可以使用；只有策略允许的工具才发送给模型。
4. **最终执行边界重新校验**：即使参数已在上游验证，工具执行前仍需再次验证 schema、权限和运行身份。
5. **自主不等于无限权限**：只读工具可自动执行，有副作用工具按风险要求确认，通用高风险能力不提供。
6. **外部内容是不可信数据**：网页、搜索摘要和附件不能覆盖系统策略或提升权限。
7. **最终回答只写一次**：工具循环不得破坏现有 pending user 和 assistant 幂等语义。
8. **停止必须贯穿全链路**：取消模型后还要取消网络、解析、工具和等待中的重试。
9. **不显示私有推理**：UI 展示操作、来源和结果，不展示 reasoning_content 或内部思维链。
10. **先协议、后 migration**：先用 fake provider 固定状态机，再一次性设计并审计数据库迁移。
11. **先只读、后写入**：先证明检索、来源、取消和恢复可靠，再开放领域化副作用。
12. **阶段九隔离**：通用 Runtime 只提供扩展点，不定义学习者模型语义。

## 7. 目标架构

```text
React Agent Surfaces
  |- Main ConversationPage -> ConversationService / Repository
  |- Quick AI overlay      -> ConversationService / Repository
  `- Writing Coach         -> WritingService / Repository
  -> Tauri typed command + ipc::Channel
  -> AgentSurfaceAdapter
  -> AgentRunCoordinator
       |- ContextAssembler
       |- ModelGateway
       |- ToolRegistry
       |- ToolPolicy
       |- AgentEventSink
       |- RunRepository
       `- RunBudget / Cancellation
            |- DeepSeek provider
            |- Web search provider
            |- Controlled web fetcher
            `- ReadRay-native tools
  -> SQLite
       |- existing conversations/messages
       |- existing writing documents/versions/answers
       |- agent runs/steps
       |- source metadata
       `- context compactions
```

各模块职责：

### `AgentSurfaceAdapter`

- 把主应用对话、Quick AI overlay 或 Writing Coach 的业务身份转换为统一 run 输入。
- 提供当前业务面的 ContextAssembler 输入、active tool policy 和最终提交边界。
- 通用对话适配器复用 `prepare_turn/complete_turn`。
- Writing Coach 适配器复用 document ID、expectedRevision、可见快照、version ID 和既有问答身份。
- 不允许 Runtime 绕过 surface adapter 直接修改业务表。

### `AgentRunCoordinator`

- 拥有一个 Agent run 的唯一生命周期。
- 驱动模型请求、tool call、工具结果回传和下一轮请求。
- 维护 run/turn/step 身份、预算、取消和终止原因。
- 只通过 repository 写入稳定状态，只通过 event sink 发布事件。

### `ContextAssembler`

- 从 SQLite 权威快照构造 AgentMessage。
- 注入当前已授权能力、必要的运行环境事实和未来的 compaction。
- 把内部 AgentMessage 投影为 provider 所需消息。
- 保证外部内容被标记为不可信数据。
- 阶段九前不读取学习者画像或长期记忆。

### `ModelGateway`

- 屏蔽 DeepSeek Chat Completions 与 Responses API 的协议差异。
- 统一输出文本增量、tool call 增量、完成原因、usage 和 provider 错误。
- 不把 provider 私有推理发送给 UI。
- 允许 fake provider 驱动全部离线状态机测试。

### `ToolRegistry`

- 注册工具名称、描述、参数 schema、风险等级和执行器。
- 根据当前 run 的 capability policy 生成 active tool set。
- 不包含用户授权、确认 UI 或网络安全判断本身。

### `ToolPolicy`

- 在最终执行边界检查 active tool、风险级别、参数、run 身份和预算。
- 决定自动允许、需要确认或拒绝。
- 网络工具还需经过 URL、DNS/IP、重定向、大小和类型策略。

### `RunRepository`

- 复用同一 SQLite 数据库。
- 保存 run、step、工具结果元数据、来源和 compaction。
- 维护幂等身份和状态迁移，不保存 API Key 或私有思维链。

## 8. Agent 执行模型

### 8.1 身份层级

```text
surface_kind + authority_ref
  -> authority_revision / request_identity
      -> run_id
          -> turn_id
              -> step_sequence
                  -> tool_call_id
```

- 通用对话的 `authority_ref` 是 `conversation_id + expected_user_sequence + user_message_id`。
- Writing Coach 的 `authority_ref` 由 document ID、数据库 revision、可见正文快照、本地 generation、可选 version ID 和问答请求序号组成，继续以现有写作身份规则为准。
- `run_id` 标识该轮的一次执行尝试；重试必须保留 `retry_of_run_id`。
- `turn_id` 标识一次模型输出及其紧随的工具批次。
- `step_sequence` 保证持久化和 UI 事件有确定顺序。
- `tool_call_id` 使用 provider ID 或 ReadRay 生成的稳定映射，不依赖工具参数文本猜测。

### 8.2 核心循环

```text
surface.prepare_authority
  -> 创建或恢复 AgentRun
  -> 组装上下文与 active tools
  -> 请求模型并流式发布文本/tool call
  -> 如果只有最终文本：校验并 surface.commit_final
  -> 如果包含 tool call：
       -> 校验所有调用
       -> 执行允许的工具批次
       -> 持久化标准化 ToolResult
       -> 将结果投影回模型上下文
       -> 进入下一 turn
  -> 直到最终回答、停止、失败或超过预算
```

通用对话中，`surface.prepare_authority/commit_final` 分别映射现有 `prepare_turn/complete_turn`；Writing Coach 中则映射现有 writing repository 的 expectedRevision、问答持久化和迟到结果检查。共享的是中间循环，不是业务写入方式。

### 8.3 终止条件

任一条件成立即停止循环：

- 模型产生非空最终回答且没有待执行工具。
- 用户请求停止。
- provider 返回不可重试错误。
- 工具策略拒绝且模型无法在下一轮安全降级。
- 达到最大模型轮数、工具调用数、总时间或上下文字节预算。
- 会话、用户消息、run generation 或页面请求身份已失效。
- 持久化发生无法安全恢复的错误。

### 8.4 初始预算

精确数值由任务 0 的协议验证决定，第一版必须存在以下硬上限：

- 单个 run 最大模型轮数。
- 单个 run 最大工具调用数。
- 单批最大并行工具数。
- 单个工具超时和整个 run 总超时。
- 单个工具结果最大字节数。
- 单次抓取最大响应体和最大重定向次数。
- 发送给模型的总上下文预算。
- 可重试错误的最大次数和退避上限。

预算耗尽是正式终止原因，不应表现为无限转圈或笼统网络错误。

## 9. 消息与上下文协议

### 9.1 展示消息与 AgentMessage 分离

现有 `ConversationMessage` 只有 `user/assistant + content`，适合作为用户可见 transcript，不应为了工具运行细节直接膨胀成万能结构。

Runtime 内部需要独立的 AgentMessage 投影，概念类型包括：

```text
SystemContext
UserMessage
AssistantText
AssistantToolCall
ToolResult
CompactionSummary
RuntimeNotice
```

- `conversations/messages` 继续保存用户可见 user/assistant 内容。
- tool call、tool result、来源、预算和错误存入 run/step 结构。
- `ContextAssembler` 合并两类事实，生成 provider context。
- 对话导出默认仍以用户可见 transcript 为主；未来是否包含工具审计附录另行决定。

### 9.2 reasoning_content 边界

当前 DeepSeek 推理增量只用于诊断并丢弃。进入工具循环后，如果 provider 协议要求在同一 run 的后续请求中回传 reasoning 内容，Runtime 可以在内存中的 provider turn state 暂存并原样回传，但必须遵守：

- 不发送给前端。
- 不写入普通会话、导出或日志。
- 不把 reasoning 当成工具参数或权限依据。
- 进程崩溃后不尝试恢复私有推理；从最后一个可安全重放的稳定边界重新执行。
- 优先使用 provider 支持的非思维链 continuation 标识或结构化 tool-call 状态。

### 9.3 长上下文

当前最近 40 条消息只是临时窗口，不是长期方案。Runtime 稳定后改为 token-aware 策略：

```text
系统与能力说明
+ 持久化 compaction summary
+ 需要保留的关键工具事实/来源
+ 最近完整对话 tail
+ 当前 pending user
```

Compaction 必须：

- 作为可追溯的持久化 entry 保存。
- 记录覆盖的消息范围、生成模型、算法版本和创建时间。
- 不删除原始会话消息或工具步骤。
- 失败时继续使用可计算的安全 tail，不覆盖旧摘要。
- 与阶段九学习者记忆严格分离；对话摘要不是学习者画像。

## 10. 统一事件协议

前端不再只接收 `delta/done/stopped/truncated/error`，而是消费统一 envelope：

```text
AgentEvent {
  run_id
  turn_id?
  step_sequence
  event_type
  payload
}
```

第一版事件至少覆盖：

```text
run_started
turn_started
assistant_text_delta
assistant_text_completed
tool_call_started
tool_call_progress
tool_call_completed
tool_call_failed
sources_updated
run_stopped
run_truncated
run_failed
run_completed
```

事件规则：

- 同一 run 的 `step_sequence` 单调递增。
- 工具完成事件可按真实完成顺序发布，但回传给模型和稳定落库按原 tool call 顺序组织。
- UI 只显示对用户有意义的状态，不展示内部 prompt、私有推理、API Key 或未经裁剪的工具参数。
- 页面卸载、切换会话或请求身份变化后，迟到事件不得更新当前页面。
- `run_completed` 只有在最终 assistant 已通过现有完整性校验持久化后才能发布。

## 11. 工具协议

### 11.1 ToolDefinition

每个工具必须静态声明：

```text
name
description
input_schema
risk_level
parallel_policy
timeout_policy
result_size_limit
executor
```

要求：

- 工具名称稳定并可版本化。
- 参数使用严格 schema，拒绝未知或越界字段。
- 工具描述只陈述真实能力，不能让模型认为它拥有未实现权限。
- 参数经 hook 修改后必须在最终执行边界重新验证。
- 工具错误转为标准 ToolResult，不能让整个进程 panic。

### 11.2 ToolResult

工具结果分成两部分：

```text
content   给模型的经过裁剪、标记和预算控制的内容
details   给 UI/审计的结构化元数据
```

公共字段至少包括：

```text
tool_call_id
is_error
is_truncated
content
provenance
started_at
finished_at
```

网络来源的 `details` 可包含：

```text
source_id
title
url
site_name
published_at?
retrieved_at
content_type
```

不得把整篇网页、响应 headers、cookies 或搜索服务密钥写入模型上下文或普通日志。

## 12. 权限与风险分级

### L0：可信本地只读

示例：运行环境事实、应用版本、用户已经明确提供的当前选区。

- 模型可自动使用。
- 不访问用户未选择的文件或数据库范围。
- 可以静默执行，但结果必须带来源类型。

### L1：外部只读

示例：公开 Web Search、受控网页读取。

- 在全局网络权限允许时由模型自动调用。
- UI 显示正在搜索/读取和最终来源。
- 必须经过 NetworkGateway 安全策略。
- 失败时不得伪造最新事实。

### L2：ReadRay 领域写入

未来示例：保存引用、创建学习卡、加入复习队列。

- 只能调用 Rust/Tauri typed domain command。
- 参数、目标对象和预期写入对用户可见。
- 根据可逆性和影响决定逐次确认或预授权。
- 必须有幂等键、来源和失败恢复。
- 不允许工具直接执行任意 SQL。

### L3：禁止能力

```text
bash / PowerShell / cmd
任意进程执行
任意文件系统 read/write/edit
安装包或动态代码扩展
读取浏览器登录态、cookies 或系统凭据
关闭安全软件、修改系统设置等高权限操作
```

ReadRay 第一版和本方案均不提供 L3 能力。

## 13. 自动工具决策

自动工具选择必须解决通用的信息缺口，而不是围绕某个示例问题创建专用功能。

模型请求中应包含：

- 当前 active tools 的真实描述。
- 必要的运行环境事实及其来源。
- 动态信息需要核实、工具失败不得编造的行为规则。
- 外部内容不能覆盖系统与工具策略的边界。

期望行为：

- 本机可确定的事实使用本地可信信息。
- 涉及当前、最新、近期变化的公共事实时自动搜索。
- 稳定知识直接回答。
- 高风险或高准确性问题优先检索并交叉验证。
- 用户明确要求搜索时，只要权限允许就执行。
- 工具不可用、权限关闭或搜索失败时说明限制。

不能仅通过提示词主观判断“看起来合理”。任务 0 和后续回归必须用固定样本评测工具选择正确率、无必要调用率和失败诚实度。

## 14. 首批工具范围

### 14.1 `web_search`

建议输入：

```text
query
domains?
recency?
max_results?
```

输出：

- 标题、URL、摘要、站点、可用发布时间。
- 稳定 source ID 和检索时间。
- 结果截断、部分失败和 provider 信息。

首先验证 DeepSeek Responses API 的 server-side `web_search` 是否满足：

- 能稳定返回真实 URL 和标题。
- 引用元数据可映射到 UI 来源卡片。
- 流式事件、usage、停止和错误可可靠解析。
- 搜索失败不会被包装成普通模型答案。

如果内置搜索的来源或稳定性达不到验收要求，在相同 `web_search` ToolDefinition 后替换为受控搜索 provider，不改变 Agent loop 和 UI 协议。

### 14.2 `fetch_web_page`

只有在搜索摘要不足且安全策略实现后才开放。最低要求：

- 只允许 HTTP/HTTPS，默认优先 HTTPS。
- 拒绝 localhost、loopback、私网、link-local、保留地址和云 metadata。
- 每次解析和每次重定向都重新校验域名与最终 IP，防止 DNS rebinding。
- 限制重定向次数、连接/读取/总超时和响应字节数。
- 只接受明确允许的文本内容类型，拒绝可执行文件和任意下载。
- 不携带浏览器 cookies，不允许模型自定义授权 header。
- 移除 script、iframe 和 active content，只抽取受控正文。
- 返回 canonical URL、标题、抓取时间、截断状态和正文摘要。
- 网页正文始终包装为不可信外部数据。

如果 DeepSeek 内置 Web Search 已能提供充足内容和来源，第一阶段可以不开放任意 URL 抓取。

### 14.3 Runtime facts

运行环境事实是 ContextAssembler 的通用输入，不把某个日期、星期或设备问题设计成独立产品功能。可包含当前本地日期时间、时区、应用版本和当前已授权能力，用于避免模型猜测环境状态。

### 14.4 阶段九后的工具

以下能力只保留扩展点，不在本文阶段实现：

```text
search_learning_memory
get_learning_evidence
save_quote
create_learning_card
add_to_review_queue
```

是否存在、输入输出和权限语义必须由阶段九或单独产品任务决定。

## 15. 网络安全边界

需要显式覆盖：

1. **Prompt injection**：搜索结果和网页只能作为数据，不能授权新工具或修改系统规则。
2. **SSRF**：禁止访问本机服务、路由器、Docker、WLAN 管理页面和云 metadata。
3. **重定向绕过**：每一跳都重新检查协议、主机和 IP。
4. **DNS rebinding**：解析后固定并校验连接目标，不能只校验初始域名字符串。
5. **凭据泄露**：API Key 不进入 URL、工具参数、日志、SQLite、网页正文或模型上下文。
6. **大响应与压缩炸弹**：限制压缩前后大小、解析深度和最大输出。
7. **内容类型欺骗**：响应头和内容探测共同限制，不下载或执行二进制。
8. **外链打开**：只允许经过解析的 HTTP/HTTPS URL 交给受控 opener，不拼接 Shell 命令。
9. **迟到网络结果**：结果发布和落库前复核 run、conversation 和 generation 身份。
10. **隐私**：搜索请求只发送完成任务所需的最小文本，不自动上传完整会话、学习记录或未选择文档。

## 16. 持久化方案

### 16.1 保留现有业务权威

- 主应用对话和 Quick AI overlay 的用户可见消息继续走 `conversations.rs`。
- `prepare_turn/complete_turn` 继续承担通用对话用户轮次与最终 assistant 的原子边界。
- Writing Coach 继续由 `writing.rs` 保存文章、revision、版本、分析和问答；Runtime 不能把它改存到 conversation 表。
- Writing 结果发布前仍需核对 document ID、数据库 revision、可见快照、本地 generation、可选 version ID 和请求序号。
- 不把通用对话或 Writing Coach 消息写入 `learning_records`。
- 不用工具步骤重建或覆盖用户可见消息、文章或版本。

### 16.2 新增运行记录

最终表名和字段在任务 0 后冻结，概念上需要：

#### Agent run

```text
run_id
surface_kind
authority_kind
authority_id
authority_revision?
conversation_id?
user_message_id?
expected_user_sequence?
writing_document_id?
writing_version_id?
retry_of_run_id?
provider
model
status
termination_reason?
started_at
updated_at
completed_at?
```

#### Agent step

```text
run_id
step_sequence
turn_index
kind
status
tool_call_id?
tool_name?
input_json?
result_json?
error_code?
started_at
completed_at?
```

#### Agent source

```text
source_id
run_id
tool_call_id
title
url
site_name?
published_at?
retrieved_at
metadata_json
```

#### Context compaction

```text
conversation_id
covered_through_sequence
summary
model
algorithm_version
created_at
```

### 16.3 状态迁移

Run 状态至少包括：

```text
prepared
model_streaming
tool_running
awaiting_approval
synthesizing
completed
stopped
failed
```

要求：

- 状态迁移必须由 repository 校验，不能任意倒退。
- completed 只能对应已持久化的最终 assistant。
- stopped/failed 保留 pending user 和稳定工具事实，允许安全重试。
- 同一个 tool call 的写入使用唯一幂等身份。
- 有副作用工具必须记录“未开始、已开始、已确认完成、结果未知”，不能把超时直接当失败重放。
- migration 集中设计、带事务、回滚测试和真实旧数据库兼容夹具；不得边实现边零碎追加多版 schema。

## 17. 停止、错误与恢复

### 停止

用户停止后：

- 取消当前 provider stream。
- 取消网络请求、正文抽取和等待中的 retry backoff。
- 不再启动新的工具或模型 turn。
- 等待当前 executor 到达可确认的稳定边界。
- 迟到事件不得更新 UI 或写入最终 assistant。
- 保留 pending user，允许用户从同一轮重试。

### 错误分类

至少区分：

```text
user_aborted
provider_timeout
provider_rate_limited
provider_auth_failed
provider_protocol_error
context_overflow
unknown_tool
tool_schema_invalid
tool_policy_denied
tool_timeout
tool_execution_failed
network_blocked
content_extract_failed
persistence_failed
run_budget_exceeded
```

只有明确瞬态且无副作用的错误可以有限重试。取消、schema 错误、权限拒绝、上下文溢出和结果未知的写操作不得盲目重试。

### 重启恢复

- 如果最终 assistant 已落库，重试直接返回权威快照。
- 如果只有 pending user 且没有不可重放副作用，从最后稳定边界创建新的 retry run。
- 如果只读工具已完成，可按策略复用仍在有效期内且来源完整的结果，或重新执行。
- 如果副作用执行结果未知，必须进入用户确认/对账流程，不能自动重放。
- 不尝试恢复 provider 私有推理；必要时重新请求模型。

## 18. 前端体验

### 18.1 正常状态展示

UI 只展示简洁、可验证的操作：

- “正在搜索相关资料……”
- “正在读取 2 个来源……”
- “正在整理答案……”
- 某来源失败或结果被截断。

不展示：

- 私有思维链。
- 完整 system prompt。
- 原始工具参数中的敏感内容。
- provider 内部事件和无意义调试日志。

### 18.2 来源

- 回答中的动态事实应关联稳定 source ID。
- 回答下方展示标题、站点和安全 URL。
- 来源卡片来自结构化 metadata，不从模型生成的 Markdown 链接反推。
- 打开来源必须走受控 HTTP/HTTPS opener。
- 搜索只部分成功时保留成功来源并说明范围。

### 18.3 输入队列

Runtime 与网络纵切稳定后，再开放：

- steering：当前工具批次结束后尽快影响本次 run。
- follow-up：当前 run 正常结束后开始下一轮。

第一版可以先实现单一排队输入，但不能把运行中的用户输入静默丢弃。队列消息必须有持久身份、取消语义和 UI 状态。

### 18.4 重新生成与继续

- 重新生成沿用已确认的覆盖式产品语义。
- 底层不得物理覆盖原始事实；实现前需设计 retry/替代关系和导出语义。
- 超长回答继续生成应使用同一 run/context 边界，不通过拼接提示词假装恢复。
- 这两项在 Agent loop 和 compaction 稳定后实施。

## 19. 分阶段实施计划

本文整体作为“阶段八点五：Agent Runtime Upgrade”，阶段九继续暂停。

### 任务 0：协议与 provider spike

目标：不改正式会话/写作 schema、不改正式 UI，先证明与具体业务面解耦的 Agent 协议可行。

工作内容：

- 定义最小 `ModelEvent`、`AgentEvent`、`ToolCall`、`ToolResult` 和 `AgentSurfaceAdapter` 草案。
- 用 fake provider 覆盖无工具、单工具、多工具和错误流程。
- 对 DeepSeek Responses API 做独立 live spike，验证 function tool、server-side web search、流式事件、usage、停止和来源元数据。
- 验证推理模型工具链需要保留的 provider state，决定 Responses API 或 Chat Completions 的正式适配方式。
- 明确第一版预算、错误分类和终止原因。

验收：

- 离线夹具可确定性重放全部协议分支。
- live spike 不写用户真实会话数据库。
- 可以明确回答 DeepSeek 内置 Web Search 是否满足来源要求。
- 形成一次协议评审结论后停止，不提前 migration。

#### 实施/评审记录（任务 0，2026-08-16）

- **协议结论**：新增 `src-tauri/src/agent_runtime/protocol.rs` 与测试模块。`AgentSurface` 只表达主应用对话、Quick AI overlay 和 Writing Coach 三类入口；`AuthorityRef` 以 typed authority 保存 conversation 的 `conversation_id + expected_user_sequence + user_message_id`，或 Writing 的 document/revision/可见正文摘要/generation/version/request 序号。`AgentSurfaceAdapter` 只做身份校验，不读取或写入业务表。
- **事件与工具结论**：`ModelEvent` 覆盖文本、tool call、usage、来源和完成原因；`AgentEvent` 使用 `run_id + turn_id + 严格递增 step_sequence + event_type/payload` envelope。`ToolCall` 在最终执行边界要求对象参数，`ToolResult` 固定错误、截断、provenance、时间和 details 字段；`ToolCallCompleted` 只能携带成功结果，`ToolCallFailed` 只能携带错误结果，并核对 `tool_call_id -> tool_name`。terminal 矩阵冻结为 `FinalAnswer -> RunCompleted`、`UserAborted -> RunStopped`、`ContextOverflow/RunBudgetExceeded -> RunTruncated`，其他 provider/tool 错误进入 `RunFailed`；私有 reasoning 不进入 AgentEvent。
- **第一版建议预算**：单 run 最多 8 个模型 turn、16 个 tool call、单批 4 个并行工具；单工具 30 秒、全 run 180 秒；单工具结果 64 KiB、模型上下文 128 KiB；瞬态 provider 错误最多重试 1 次，退避上限 2 秒。该数值是协议硬上限的第一版建议，正式调优留给任务 1/真实样本。
- **错误/终止分类**：按方案固定 `user_aborted`、provider timeout/network/rate limit/auth/protocol、context overflow、unknown tool、tool schema/policy/timeout/execution、network blocked、content extract failed、persistence failed、run budget exceeded；终止原因保持同名可审计分类，最终回答单独为 `final_answer`。只有 provider timeout/network/rate limit 默认具备无副作用重试资格。
- **DeepSeek continuation state 决策**：DeepSeek Responses 按 stateless 请求处理，不发送或推断 `previous_response_id`；只在内存中观察 provider 返回的 response ID，并保留结构化 tool-call state 与推理模型工具链所需的私有 reasoning。`previous_response_id` 只属于其他明确支持该扩展的 provider state，不是 DeepSeek 结论；所有 continuation state 不序列化、不发 UI、不写会话/日志，崩溃后从安全边界重放。
- **离线与 live 状态**：deterministic fake-provider replay 已覆盖 final-only、single/multiple tools（当前是 controlled completion-order fixture，不声称真实并行）、unknown tool、invalid arguments、tool failure、timeout、model/tool abort 和三类独立预算上限；Responses parser fixture 严格配对 `event:` 与 `data.type`，校验 stream sequence 与终态，并观察 web_search action、function event、usage 和 provider response ID。只有 provider 明确返回 citation/annotation 时才记录来源，source ID 使用不暴露 URL 的确定性 hash，未自造 `web_search_call.sources`。live test 默认 `#[ignore]`，另需显式 `READRAY_RUN_DEEPSEEK_RESPONSES_SPIKE=1`；本次按授权尝试时直接读取现有 `secret_store`，因未配置可用 DeepSeek key 在发出请求前停止（未改变认证、未写用户数据库）。因此离线只能观察 web_search action，来源能力未知，需 live，不能把 DeepSeek 内置 Web Search 记为已满足来源验收。
- **live 结果（2026-08-17）**：修复 live spike 未加载项目根 `.env` 的问题——`responses_spike.rs` 与既有 live 测试一致，在读取 key 前用 `dotenvy::from_path_override` 加载项目根 `.env`。项目根 `.env` 已配置可用 `DEEPSEEK_API_KEY` 时显式执行一次 spike（`READRAY_RUN_DEEPSEEK_RESPONSES_SPIKE=1`）：请求已真实发出（证明 `.env` 修复生效），但 DeepSeek `POST https://api.deepseek.com/responses` 对当前函数/web_search 请求形状返回 HTTP 400。spike 按设计不读取/打印错误 body，本次结果无法区分是端点路径、请求体形状还是账户/模型作用域原因；因此 function/web_search 行动与 citation/annotation 来源元数据本次未能 live 观测，内置 Web Search 是否满足来源验收仍未确认。该开放问题不阻塞任务 1（Rust Agent Kernel 为纯离线 fake-provider 循环），自然移到任务 3 自动联网纵切时重新验证。
- **评审结论与停点**：任务 0 已通过协议评审，下一实施入口为任务 1 Rust Agent Kernel。任务 0 未新增 migration、生产 Agent loop、工具注册/执行、正式会话/写作适配或 UI；在任务 1 完成前，不新增 Agent migration、不改正式对话或写作 UI、不向用户真实会话开放 Web Search。

### 任务 1：Rust Agent Kernel

目标：实现无持久化副作用、可用 fake provider 验证的最小循环。

工作内容：

- `AgentRunCoordinator`、`ContextAssembler`、`ModelGateway` 接口。
- ToolRegistry、active tool set、schema 校验和 ToolPolicy。
- 文本与 tool call 流式事件。
- 串行/并行工具批次和确定性结果顺序。
- run budget、取消、超时和标准错误。
- capability-aware 系统提示词构建。

验收：

- 无工具回答只执行一轮。
- 工具调用能在结果回传后继续生成最终回答。
- 多工具完成顺序不同也不改变模型看到的结果顺序。
- 未知工具、坏参数、超时、取消和循环超限都有明确终止。
- 不存在 Bash、任意文件工具或动态扩展入口。

### 任务 2：通用对话接入与 SQLite run/step 恢复

目标：把 Runtime 接到主应用完整对话与 Quick AI overlay 共享的现有会话权威，保持原有幂等与重启语义。

工作内容：

- 在协议冻结后集中设计 migration。
- 保存 run/step/source 状态。
- 把 `prepare_turn -> AgentRun -> complete_turn` 串成同一正式链路。
- 主应用完整对话与 Quick AI overlay 复用同一 ChatSurfaceAdapter、run/step schema 和工具协议，只保留各自现有入口/显示生命周期差异。
- 实现 IPC 丢失、应用重启、迟到工具结果和持久化失败恢复。
- 保留旧非 Agent 路径作为受控回退，直到正式验收完成。

验收：

- 重试不重复 user、assistant 或 tool result。
- 停止/崩溃后 pending user 可恢复。
- completed run 与最终 assistant 严格一致。
- 旧数据库迁移、回滚、重启幂等和真实数据库只读审计通过。

### 任务 3：自动联网纵切

目标：完成第一个用户可感知的现代 Agent 能力。

工作内容：

- 接入 `web_search`，必要时接入受控 `fetch_web_page`。
- 默认由模型判断是否搜索，不提供会话级联网开关。
- 实现搜索/来源事件、来源卡片和回答引用。
- 实现网络权限、SSRF、重定向、大小、超时、内容类型和隐私策略。
- 动态事实搜索失败时阻止模型伪装为已核实回答。

验收样本至少覆盖：

- 稳定知识不调用工具。
- 最新公共事实自动搜索并展示来源。
- 本地可确定的信息不进行无意义 Web Search。
- 用户明确要求搜索时执行搜索。
- 搜索失败时诚实说明无法核实。
- 恶意网页提示无法提升权限或触发禁止工具。
- 停止后网络和模型都终止，迟到结果不发布。

#### 实施/评审记录（任务 3，2026-08-17）

- **Provider 决策（live spike 证据）**：重跑 Responses spike（`READRAY_RUN_DEEPSEEK_RESPONSES_SPIKE=1`，仅该测试），`POST /responses` 仍返回 HTTP 400；随后在 spike 中补充 chat/completions web_search 变体探测，错误明确为 `tools[0].type: unknown variant 'web_search', expected 'function'`——当前 DeepSeek 端点只接受 function 工具，内置 server-side web_search 不存在，来源要求不满足。经调度者确认采用方案 A：在相同 `web_search` ToolDefinition 后替换为受控 provider，不改变 Agent loop 与 UI 协议。
- **受控搜索 provider**：`network.rs` 定义 `SearchProvider` trait（可替换，未来 Tavily 等 key 服务只换实现）；任务 3 只实现无 key 的 Wikipedia API provider（zh/en 的 search API，返回条目标题/URL/摘要），工具描述诚实标注"维基百科覆盖，非通用搜索"，覆盖不到时模型必须如实说明，不得用模型记忆冒充已核实事实。
- **受控抓取 fetch_web_page**：逐跳重新校验 URL 与解析后的全部 IP（防 DNS rebinding），连接固定到已验证 IP；限制重定向 5 跳、响应 2 MiB、连接 10s/整体 30s 超时；只接受文本内容类型白名单；不携带 cookies、拒绝 userinfo 与敏感查询参数；剥离 script/style/iframe/svg/template 与注释；正文始终作为不可信外部数据回传模型。
- **内核与协议投影**：L1 工具在 `conversation_l1_tools` 注册（web_search/fetch_web_page，ExternalReadOnly），对话 capability 提升到 ExternalReadOnly；coordinator 在工具完成时把 `details.sources` 投影为 SourcesUpdated 事件（先于 ToolCallCompleted）；PersistingSink 改为从 ToolCallCompleted/Failed 提取来源落库并关联 tool_call_id（移除空串落库）。未改 protocol.rs。
- **前端**：正式对话切换到 `send_quick_ai_message_agent`；QuickAiStreamEvent 扩展 `sources_updated`/`tool_state`；ConversationPage 展示来源卡片（标题/站点/URL，点击走受控 `open_agent_source` command）与"正在搜索/正在读取/正在整理"状态；来源以 ref 为权威累积（修复了来源被后续 setGeneration 覆盖的缺陷）；fixture 增加 `[fixture:sources]` 演示。
- **验证**：Rust lib 362/0（含网络 15 项、来源事件/落库/投影等新增约 28 项）、前端 conversation 30/30 与其他套件全绿、tsc/vite build、fmt/check/diff 通过；浏览器预览验证来源卡片与工具状态展示。未运行其他 live 测试；真实 Tauri/DeepSeek 人工验收留待停点。
- **评审修复轮（2026-08-17）**：调度者评审后执行会话逐项修复并通过复审：① 真实使用发现多轮对话 400（历史 assistant 投影为 `tool_calls: []`，DeepSeek 要求长度 ≥1）——`project_message` 空 tool_calls 省略该键；② 失败/中断后 composer 被 `generation !== null` 锁死且无出口——改为仅 generating 阻塞发送，`prepare_turn` 允许尾 pending user 时以 `current_max+2` 开启新轮次（旧 pending 保留可审计、重试复用同 sequence/id、stale 拒绝），不添加"放弃"按钮；③ fetch_web_page 运行时失败归 `NetworkBlocked` 会 fail-fast 杀死 run——改为运行时失败（DNS/传输/非 200/内容类型/重定向超限）归 `ToolExecutionFailed` 可恢复，`NetworkBlocked` 只留安全拒绝（SSRF/私网/凭据/重定向到非 HTTP(S) 协议）；④ Wikipedia 解析失败被当"无结果"——区分三态（非法 JSON/缺列表 → ToolExecutionFailed；空列表 → 无结果）；⑤ `stable_source_id` 收敛为 network.rs 共享实现；⑥ 全局网络权限门（方案 §5.3）本轮只标注边界不实现，未来经 app_preferences 回落 L0。真实使用另暴露并修复：DeepSeek 拒绝无 `type: "object"` 的 function schema、多工具轮次 `ToolRunning→ToolRunning` 自环缺失导致整轮非法迁移、Wikipedia 无 User-Agent 返回 403、模型对"能联网吗"保守回答"不能"（有网络工具时提示词据实声明联网能力）。收口时全量 Rust 372/0、前端 conversation 33/33、build/fmt/check/diff 全绿；用户已完成真实 Tauri 多轮追问、失败后发新消息、联网来源卡片与"能联网吗"人工验收，任务 3 正式收口。


### 任务 4：日常使用交互

目标：消除“必须等待一次回答结束”的 demo 交互。

工作内容：

- steering/follow-up 或第一版统一排队输入。
- 重新生成的覆盖式正式实现。
- 截断后的安全继续。
- 工具失败的局部可重试与清晰状态。
- 对话中的工具步骤折叠显示和来源回看。

验收：

- 运行中追加内容不会丢失或污染其他会话。
- stop、steer、follow-up 的顺序可复现。
- 重新生成不破坏原始用户轮次和导出一致性。
- 页面卸载、切换会话和窗口隐藏后事件身份仍正确。

### 任务 5：长上下文与 compaction

目标：替换固定 40 条消息截断，支持长期稳定对话。

工作内容：

- token/字节预算估算。
- 最近完整 tail + 持久化 summary。
- 工具事实、来源和当前 pending user 的保留规则。
- compaction 失败回退、版本化和重新生成。

验收：

- 长会话不会突然从 assistant 开头或丢失当前 user。
- 重启前后模型上下文投影一致。
- 原始消息与工具步骤永不因压缩删除。
- 摘要与阶段九学习者记忆没有数据或语义混用。

### 任务 6：Writing Coach 适配

目标：让作文中的分析、问答和辅导 Agent 复用已经验收的 Runtime，同时保留写作文章、revision、完成版本和请求身份权威。

工作内容：

- 实现 WritingCoachSurfaceAdapter，不复制第二套 Agent loop、ModelGateway、ToolRegistry 或网络工具。
- 将当前文章、选区、分析问题、最近问答和可选历史版本投影为写作专属上下文。
- 为 Writing surface 配置独立的 active tool set；只开放与写作任务相关的只读能力，不能继承通用对话的全部工具。
- 允许 Writing Coach 在确有事实核实需求时复用安全 Web Search，并把来源与建议绑定，而不是自动改写正文。
- 最终建议、分析或回答继续走 `writing.rs` 的结构校验、expectedRevision 和持久化边界。
- 任何正文修改都保留现有用户编辑/接受流程；Agent 不直接覆盖草稿。
- 页面切换、自动保存推进 revision、完成版本切换或本地继续编辑后，旧工具和模型结果必须失效。

验收：

- 主应用对话、Quick AI overlay 与 Writing Coach 共享同一 Runtime 内核，但状态和数据互不串写。
- Writing Coach 的搜索、停止、失败和来源展示可用，不破坏已有连续问答。
- 自动保存推进 revision 后，仍可接受属于同一可见身份的合法回答；正文或版本变化后迟到结果被拒绝。
- Agent 建议不会被冒充为用户独立写作，也不会自动写入学习者能力证据。
- 现有写作创建、保存、分析、问答、完成、版本回看和继续修改回归通过。

## 20. 测试策略

### 离线单元测试

- Agent 状态迁移。
- 工具 schema 和最终执行前重校验。
- active tool allowlist。
- 预算和终止条件。
- provider event 解析。
- 并行工具确定性排序。
- URL、IP、重定向和内容类型策略。
- 上下文投影和 compaction。

### SQLite 集成测试

- migration 与旧数据库夹具。
- run/step 幂等写入。
- pending/completed/stopped/failed 恢复。
- IPC 模糊成功和应用重启。
- 工具结果未知与副作用不可重放。
- 数据库写入失败后不伪装成功。

### Fake provider 场景

```text
final_text_only
single_tool_then_final
multiple_parallel_tools
unknown_tool
invalid_arguments
tool_error_then_recover
tool_timeout
provider_rate_limit
user_abort_during_model
user_abort_during_tool
loop_budget_exceeded
late_event_after_new_run
```

### Live DeepSeek 测试

- 默认 ignored，显式环境开关后运行。
- 不读取或修改用户真实学习数据库。
- 覆盖 function tool、web search、流式 usage、取消、来源和推理模型 continuation。
- 记录模型/接口版本和测试时间，避免把时效行为写成永久假设。

### 真实 Tauri 人工验收

- 主应用完整对话、Quick AI overlay 和 Writing Coach 都要测试。
- 流式文本、工具状态、来源打开、停止和重试。
- 窗口隐藏/恢复、切换会话和应用重启。
- 网络离线、限流、Key 失效和搜索部分失败。
- 中文输入法、长输入、长回答和高 DPI/缩放下 UI。

自动测试不能替代真实 Tauri/SQLite/DeepSeek 验收。

## 21. 质量与性能指标

性能主要由模型轮数、上下文大小和网络工具决定，不能只测 Rust 函数耗时。至少记录：

- 首个可见文本或工具状态延迟。
- 单 run 的模型轮数和工具调用数。
- Web Search 与 fetch 的耗时、成功率和截断率。
- 发送给模型的上下文字节/token 估算。
- 取消到完全 idle 的时间。
- run 重试率、恢复率和重复副作用数。
- 需要实时信息的样本中正确调用工具的比例。
- 稳定知识样本中的无必要工具调用比例。
- 动态事实回答中的来源覆盖率。
- 搜索失败时是否出现无来源的“最新事实”断言。

不在方案阶段预设虚假达标数字。任务 0 建立基线，后续任务在真实样本上设阈值。

## 22. 依赖、许可证与打包

- 第一版优先复用现有 Rust HTTP、serde、异步和 SQLite 能力。
- 如确需新增 crate，必须先说明必要性、无依赖替代方案、安装包与编译影响。
- 不把 pi 包作为运行时依赖。
- 如果直接移植 pi 的具体实现代码，必须单独审查其 MIT 许可证、保留归属并记录修改；仅参考公开架构与协议思想时，也应在设计文档保留研究来源。
- Windows 构建仍产出普通 Tauri 安装包，不要求用户安装 Node/Python/Rust。
- 新网络能力不得改变 API Key 的 Windows Credential Manager 权威。

## 23. 与阶段九的边界

阶段八点五可以提供：

- 通用 Agent loop。
- ToolRegistry 和 ToolPolicy。
- 自动使用外部只读工具。
- 对话上下文压缩。
- Writing Coach 对共享 Runtime、只读工具和来源能力的受控适配。
- 未来本地工具的安全扩展点。

阶段八点五不能决定：

- 哪些学习行为代表熟练或薄弱。
- 学习者画像、证据权重、时间衰减和算法版本。
- 哪些学习记录可以注入当前回答。
- 个性化复习排序和推荐原因。
- Agent 是否可以创建或修改学习证据。

阶段九开始后，只能通过已经验收的 ToolRegistry/ContextAssembler 扩展学习能力；模型不得绕过阶段九的事实、派生状态、回退和审计边界。

## 24. 完成定义

只有同时满足以下条件，才能认定 Agent Runtime 升级完成：

- Agent 可以完成多轮“模型 -> 工具 -> 模型”循环。
- 主应用完整对话与 Quick AI overlay 共享同一 ChatSurfaceAdapter，Writing Coach 通过独立 surface adapter 复用同一 Runtime 内核。
- 工具由模型自主选择，但只能在当前授权和 active tool set 内执行。
- 实时公共信息能够自动搜索并提供结构化来源。
- 无需工具的请求不会被强制联网。
- 工具失败、权限关闭或信息不足时不编造已核实结论。
- UI 展示真实工具状态和来源，不展示私有思维链。
- 停止能贯穿模型、网络、工具和重试，并等待稳定 idle。
- 重试、重启和 IPC 丢失不会重复 user、assistant、tool result 或副作用。
- 长上下文通过可追溯 compaction 管理，原始历史不被删除。
- API Key、系统凭据和敏感上下文不进入日志、SQLite 或工具结果。
- 不存在 Bash、任意文件读写、动态代码扩展等高风险入口。
- 通过离线测试、SQLite 集成测试、显式 live 测试和真实 Tauri 人工验收。
- `docs/DEVELOPMENT_PLAN.md`、`docs/HANDOFF.md` 和 `docs/RESOURCE_MAP.yml` 已同步最终状态。

## 25. 实施停点

本方案不得一次性整包实现。每个任务完成后必须：

1. 检查实际 diff 和未提交用户修改。
2. 运行与风险相称的聚焦测试。
3. 独立审查状态机、权限、迁移或 UI 协议。
4. 明确哪些 live/真实 Tauri 验收尚未执行。
5. 更新本文任务状态和必要的恢复文档。
6. 停止等待用户确认，再进入下一任务。

任务 0/1/2/3 已通过评审（任务 3 已正式收口），当前实施入口是"任务 4：日常使用交互"。任务 4 完成前不开放 steering/follow-up、重新生成、截断继续等交互能力；Web Search 已通过 Wikipedia provider 开放给真实会话（覆盖范围以工具描述为准）。Writing Coach 必须等通用对话 Runtime、自动联网与长上下文完成各自验收后，再进入任务 6。

## 26. 研究来源

- pi coding-agent：<https://github.com/earendil-works/pi/tree/086c32e74530564922d011ade23ff582c9d63116/packages/coding-agent>
- pi Agent loop：<https://github.com/earendil-works/pi/blob/086c32e74530564922d011ade23ff582c9d63116/packages/agent/src/agent-loop.ts>
- pi security：<https://github.com/earendil-works/pi/blob/086c32e74530564922d011ade23ff582c9d63116/packages/coding-agent/docs/security.md>
- pi SDK：<https://github.com/earendil-works/pi/blob/086c32e74530564922d011ade23ff582c9d63116/packages/coding-agent/docs/sdk.md>
- DeepSeek Responses API：<https://api-docs.deepseek.com/guides/responses_api>
- DeepSeek Thinking Mode：<https://api-docs.deepseek.com/guides/thinking_mode>
