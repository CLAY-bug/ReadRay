# ReadRay

ReadRay 是一个 Windows 优先的本地英语学习 Agent。它从用户正在阅读、对话和写作的真实内容出发，使用 DeepSeek 提供结构化解释与写作辅助，并将学习记录、会话、文章和非敏感设置保存在本机 SQLite 中。

当前已完成开发计划的阶段一至阶段七，包括核心学习链路、写作、会话管理、设置和 Windows 桌面生命周期。下一步是在实现阶段八之前讨论复习对象、优先级信号和“记得/忘了”的反馈语义。

## 已实现能力

- 通过 Windows UI Automation 获取选区或上下文，生成单词、短语、句子和段落解释卡。
- 提供 Quick AI 与完整多轮对话，并支持历史浏览、重命名、删除和原生 Markdown 导出。
- 使用真实 SQLite 数据展示“今天”和“记忆”，保留可追溯的学习事件。
- 提供文章新建、搜索、自动保存、检查、选区问答、差异对比、完成版本和继续修改。
- 在设置页管理 DeepSeek API Key、余额、Token 使用量、数据库备份、字体字号、发送方式、全局快捷键、开机启动和关闭策略。
- 支持系统托盘、单实例、隐藏启动、主窗口恢复和保存失败可恢复的安全退出。

## 技术与数据边界

- 桌面框架：Tauri 2 + Rust。
- 前端：React + TypeScript + Vite。
- 本地数据：SQLite；数据库 ID、revision 和时间是正式路径的权威事实。
- 模型服务：DeepSeek；解释、Quick AI 和写作共用同一 HTTP 客户端。
- 正式页面遵循 `Page -> Service -> Repository -> typed Rust command`，React 组件不直接访问 SQLite 或调用模型。
- 浏览器预览 fixture 只在非 Tauri 分支动态加载，不进入正式桌面数据路径。
- API Key 保存在 Windows Credential Manager，不写入 SQLite、备份文件或普通日志。

ReadRay 的学习数据默认留在本机；调用 DeepSeek 的功能仍会把完成当前请求所需的内容发送给模型服务。

## 本地开发

环境要求：Windows、Node.js、pnpm、Rust stable MSVC 工具链、Microsoft C++ Build Tools、Windows SDK 和 WebView2。

```powershell
pnpm install
pnpm build
pnpm tauri dev
```

API Key 可以在应用设置页配置。开发环境也可复制 `.env.example` 为 `.env` 后填写 `DEEPSEEK_API_KEY`；真实 `.env` 不应提交。

## 自动验证

```powershell
pnpm test:conversation
pnpm test:writing
pnpm test:settings
pnpm build
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo check --manifest-path src-tauri\Cargo.toml
cargo test --manifest-path src-tauri\Cargo.toml
```

真实托盘、快捷键、开机启动、单实例和窗口生命周期的验收不能由自动测试或浏览器预览替代；后续修改这些能力时仍需在真实 Tauri 窗口中回归。

## 项目文档

- [开发计划](docs/DEVELOPMENT_PLAN.md)：产品方向、阶段边界与验收标准。
- [资源地图](docs/RESOURCE_MAP.yml)：按任务定位代码和文档。
- [交接记录](docs/HANDOFF.md)：当前状态、长期决策、下一步与已知限制。
- [Windows 环境](docs/WINDOWS_ENVIRONMENT.md)：本机工具链、验证基线和常见问题。
- [协作说明](AGENTS.md)：Codex 在本仓库中的工作规则。

## 当前边界

- 阶段八复习闭环尚未开始实现，需先完成产品语义讨论。
- 当前只以 Windows 为正式目标，不支持 macOS、OCR、本地大模型或浏览器插件。
- Quick AI 当前使用非流式纯文本响应；Markdown 渲染、回答重生成和记忆引用聚合仍是后续能力。
