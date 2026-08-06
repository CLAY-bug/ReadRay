# ReadRay 内置 Agent 体验升级计划

最后更新：2026-08-06

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

## 任务 1：流式输出（执行中）

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

> 占位。目标与验收标准在任务 1 完成后补充。

## 任务 3：系统提示词构建（待执行）

> 占位。目标与验收标准在任务 2 完成后补充；将专门讨论组合式上下文与诚实边界。
