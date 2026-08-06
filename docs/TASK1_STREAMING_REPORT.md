# ReadRay 任务 1：流式输出 — 执行回报

**日期**：2026-08-06 ｜ **任务书依据**：docs/AGENT_UPGRADE.md 任务 1

## 一、改动文件清单（共 12 个）

### Rust 侧（5 个）

| 文件 | 改动内容 |
|---|---|
| `src-tauri/Cargo.toml` | reqwest 启用 `stream` feature；新增直接依赖 `bytes`、`futures-util`（二者原为传递依赖，已在 Cargo.lock 中，无新增下载） |
| `src-tauri/Cargo.lock` | 随 Cargo.toml 自动更新 |
| `src-tauri/src/deepseek_client.rs` | 新增 SSE 流式请求 `stream_chat_completion_events`（字节级 SSE 解析器：跨 chunk 缓冲、`data:` 行解析、`[DONE]` 结束、usage-only chunk 支持）、`StreamChunk` 结构、`parse_model_token_usage` 提升为 `pub(crate)`；新增 5 个离线测试 |
| `src-tauri/src/quick_ai.rs` | 新增 `send_quick_ai_message_streaming`、`abort_quick_ai_streaming` 两个命令；`QuickAiStreamEvent` 协议；abort 标志注册表（含 active 流门控）；`build_quick_ai_streaming_request_body`；`stream_quick_ai_reply`；新增 3 个测试 |
| `src-tauri/src/lib.rs` | 注册两个新命令 |

### 前端（4 个）

| 文件 | 改动内容 |
|---|---|
| `src/conversationRepository.ts` | 新增 `sendStreaming`（Channel 订阅映射）、`abortStreaming`、`QuickAiStreamEvent` 类型 |
| `src/conversationService.ts` | 正式 service 改用流式路径，capabilities 更新为 `delivery: "streaming"`、`canStop: true`；delta 转发、停止识别、幂等重试保持 |
| `src/conversationViewModel.ts` | capabilities 增加 `"streaming"` 类型；`ConversationGenerationRequest` 增加 `onStreamDelta`；`ConversationService` 增加可选 `stopGeneration` |
| `src/components/ConversationPage.tsx` | 正式路径走 streaming 分支（delta 实时驱动 GenerationState 文本）；`stopGeneration` 接通真实 abort；停止后重试语义 |

### 测试（1 个）

| 文件 | 改动内容 |
|---|---|
| `tests/conversationService.test.mjs` | 既有测试适配流式接口 + 新增 4 个流式专项测试 |

## 二、实现要点

### 1. 流式传输机制（channel 事件协议）

- 采用 **Tauri `ipc::Channel`** 单向推送（Rust → 前端），替代一次性返回值，未引入自研 IPC / 文件轮询 / stdout。
- 新命令 `send_quick_ai_message_streaming` 复用既有 `send_with_reply_provider` 链路：`prepare_turn`（user 先落库）→ 流式请求 → `complete_turn`（assistant 落库），消息持久化语义与现状完全一致。
- 请求体：`stream: true` + `stream_options: { include_usage: true }`，`max_tokens` / `temperature` 保持现状。
- channel 只推送四类明确事件（tagged enum，camelCase）：

```typescript
type QuickAiStreamEvent =
  | { type: "delta"; text: string }   // 增量文本
  | { type: "done" }                   // 完整回答已保存
  | { type: "stopped" }                // 用户中止
  | { type: "error"; message: string };
```

- SSE 解析器处理跨网络 chunk 的缓冲拼接、`data:` 行解析、`[DONE]` 结束、usage-only chunk（choices 为空数组 + usage）、多 choices 拒绝、缺尾部 `[DONE]` 报错。

### 2. 停止语义（已选定：保持 pending）

- Tauri Channel 是单向的（无 `is_closed()`），故停止采用：前端点"停止生成"→ 调用 `abort_quick_ai_streaming` 命令 → Rust 置位 conversation 级 abort flag（`AtomicBool`）→ SSE 解析循环在每个 chunk 间轮询 flag 终止请求。
- **已流出部分不落库**：与"模型失败保留 pending user"语义一致。Rust 返回 `Err("回答已停止，已保留你的问题，可以直接重试。")`，前端转为 pending 失败态，不伪造完整回答、不保存残缺回答。
- abort 带 **active 流门控**：只有该 conversation 确有活跃流时才置位，避免停止信号污染下一次重试。
- 停止后 UI 显示"已停止/继续生成"，点击后走既有 `expected_user_sequence` 幂等协议重新生成。

### 3. usage 统计（流式下计入方式）

- SSE 流在 `include_usage` 下，usage 位于最后一个 chunk（finish 块之后、`[DONE]` 之前）。
- 循环**不提前 break**，消费完整流后以 `parse_model_token_usage` 严格校验（total = prompt + completion），再 `record_for_app`（QuickAi 分类）尽力写入。
- **合法 usage 即使后续业务失败也计入；统计写入失败不影响业务结果**——与非流式 `post_tracked_chat_completion` 语义完全一致。
- 缺 usage 视为错误（"流式响应缺少 usage，无法计入使用量"）。

### 4. 重试与重启恢复

- 完全复用既有协议：`prepare_turn` 幂等识别已完成轮次（不重复请求模型）；streaming 失败同样留下 pending user；重启加载后页面进入可重试失败态。
- `send_quick_ai_message` 非流式命令原样保留，供兼容与测试。

## 三、验证结果

| 项目 | 结果 |
|---|---|
| `pnpm test:conversation` | ✅ **24/24 通过**（含 4 项新增流式专项：delta 转发、停止后 pending 重试、abort 调用、capabilities） |
| `pnpm test:writing` | ✅ 30/30 通过 |
| `pnpm test:settings` | ⚠️ 37/38（1 个失败为**基线既有问题**，见第五节） |
| `cargo test` | ✅ **122 通过 / 0 失败 / 2 联网测试按既有 ignore 跳过** |
| `pnpm build` | ✅ 通过（6.3s） |
| `cargo fmt --check` | ✅ 通过（已运行 cargo fmt） |
| `cargo check` | ✅ 通过 |
| `git diff --check` | ✅ 通过 |

## 四、已自动验证 vs 需人工验收

### 已自动验证

- SSE 解析全边界：跨 chunk 缓冲、`[DONE]`、usage-only chunk、多 choices 拒绝、坏 JSON 拒绝
- 流式请求体参数保持（max_tokens / temperature / stream_options）
- abort 标志共享与 active 门控
- 前端 delta 事件转发、停止 → pending → 重试全链路（注入式 repository）
- Rust 全量离线测试

### 需真实 Tauri / DeepSeek 人工验收（未用模拟数据冒充）

1. 真实 DeepSeek 流式回答边生成边显示（2 个 `#[ignore]` 联网测试需真实 key）
2. 真实窗口内"停止生成"按钮的即时反馈与体验（本次未启动 Tauri 窗口）
3. 设置页"近 7 天/近 30 天/全部"用量在流式路径下正确计入
4. 生成中崩溃/退出后重启恢复 pending 重试

## 五、未验证风险与遗留问题

1. **基线既有测试失败**（与本次改动无关）：`tests/settingsService.test.mjs:792` 断言 `.rr-writing-editor-title` 的 `font-size: 34px`，但 `src/styles/writing-page.css:412` 实际为 `32px`（提交 `f763eaa "fine tune writting page"` 改了 CSS 未同步测试）。已在干净的 HEAD worktree 上复现同样失败。按任务范围边界未处理，留给调度者决定。
2. **停止延迟**：abort 为轮询式（每 chunk 检查一次），停止响应延迟取决于 chunk 到达频率，真实流体验需人工确认。
3. **SSE 宽容性**：解析器对非 `data:` 行（如 `event:` 行）静默跳过；若 DeepSeek 未来返回需显式处理的 SSE 事件会被忽略。真实响应通常只有 `data:` 行，风险低。
4. **channel 送达失败**：若 webview 中途销毁，`sender.send` 失败会以"流式事件无法送达"结束该轮（保留 pending 可重试），不会静默挂起。
5. **新增依赖说明**：`bytes` / `futures-util` 与 reqwest `stream` feature 均为编译期纯 Rust 依赖，不改变打包产物形态（无新原生库、无运行时下载），Cargo.lock 无新增 crate 下载。
