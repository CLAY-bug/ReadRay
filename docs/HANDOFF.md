# ReadRay 交接记录

最后更新：2026-06-22

## TL;DR

- 当前状态：Tauri + React + TypeScript 脚手架已创建，前端构建通过，Git 已修复，比赛资料已恢复；Tauri dev 仍阻塞在缺少 MSVC linker。
- 当前路线：Windows 原生，Tauri + React + TypeScript + Rust + SQLite。
- 下一步：由工程实现会话安装 Visual Studio 2022 Build Tools 的 “Desktop development with C++” 工作负载后，重跑 `pnpm tauri dev`。
- 当前约束：不使用通用 Agent 框架，不内置商业词典，不做 OCR、本地大模型或跨平台支持。
- 交接原则：`HANDOFF.md` 只记录会影响下一次恢复上下文的信息，小型文档措辞和格式调整不记录。

## 当前文件

- `AGENTS.md`：给 Codex 使用的协作说明。
- `docs/DEVELOPMENT_PLAN.md`：项目计划和技术方向。
- `docs/RESOURCE_MAP.yml`：重要本地资源索引。
- `docs/HANDOFF.md`：当前交接记录，也就是本文件。
- `src/`：React + TypeScript 前端脚手架源码。
- `src-tauri/`：Tauri v2 / Rust 原生层脚手架源码。
- `package.json` / `pnpm-lock.yaml`：pnpm 前端依赖和脚本。
- `src-tauri/Cargo.lock`：Rust / Tauri 依赖锁定文件。
- `resource/`：已恢复的比赛官网页面、附件和文本抽取。
- `scripts/restore_competition_resources.py`：resource 恢复脚本。

## 已确认决策

- ReadRay 先做 Windows-first 桌面应用。
- 桌面框架使用 Tauri；遇到问题时优先定位和解决，不在计划中预设替代框架。
- UI 使用 React + TypeScript。
- 原生桌面能力通过 Rust / Tauri commands 实现。
- 本地存储使用 SQLite。
- 第一版 LLM 供应商使用 DeepSeek OpenAI-compatible API。
- MVP 阶段不使用 LangChain、LangGraph、Pi、Agno 等通用 Agent 框架。
- 自己实现 ReadRay 专属的轻量 Agent 层。
- 不内置商业词典数据。
- 项目管理保持轻量：不使用 `tasks/` 目录、不使用 `P-001` 结构、不完整复制 Code Relay。
- 所有项目文档必须使用中文。
- `AGENTS.md` 只保留会改变 Codex 行为的规则；背景信息、阶段计划和技术细节放到 `docs/`。
- `HANDOFF.md` 只记录会影响恢复上下文的信息，不作为操作流水账。
- 写新代码前先找现有扩展点，复用优先，单一职责，最小改动，不无理由新增依赖。
- 不得在非空项目根目录使用带覆盖或强制语义的初始化命令；如确需使用，必须先确认 Git 可用或完成备份，并说明会影响哪些文件。

## 已完成准备

- 项目计划已经恢复到 `docs/DEVELOPMENT_PLAN.md`。
- 资源地图已经恢复到 `docs/RESOURCE_MAP.yml`。
- 协作规则已经恢复到 `AGENTS.md`。
- 已用 `pnpm create tauri-app` 初始化 Tauri + React + TypeScript 脚手架。
- 已安装 pnpm 依赖，`pnpm build` 通过。
- 已安装并修复 Rust stable MSVC toolchain；当前 `rustc` 和 `cargo` 可用。
- `pnpm tauri info` 显示 WebView2 和 Rust 可用，但未检测到 Visual Studio / VS Build Tools 的 MSVC 和 SDK 组件。
- `pnpm tauri dev` 可启动 Vite，并能进入 Cargo 编译阶段；当前失败于 `link.exe not found`。
- 已从大赛官网恢复 `resource/` 页面、附件和文本抽取，并补回 `readray_competition_analysis.md`。
- 已修复原空 `.git` 目录，重新初始化为有效 Git 仓库。
- 已在 `.gitignore` 中忽略 `src-tauri/target`。

## 下一步

继续阶段一：Tauri 基础能力打通。当前最先要处理的阻塞：

- 安装 Visual Studio 2022 Build Tools，并选择 “Desktop development with C++” 工作负载。
- 安装完成后重新打开终端，确认 `link.exe` 可被 Rust MSVC toolchain 找到。
- 在仓库根目录重跑 `pnpm tauri info` 和 `pnpm tauri dev`。

阶段一完成标准：

- 全局快捷键显示和隐藏窗口。
- 窗口置顶和隐藏行为。
- 剪贴板读取。
- SQLite 读写。
- DeepSeek API 调用。
- Windows 本地开发构建。

## 待解决问题

- Tauri dev 当前失败于 `link.exe not found`，原因是缺少 Visual Studio C++ Build Tools / MSVC linker。
- UI 具体设计暂时不定。
- SQLite schema 在解释卡和本地记忆阶段再设计。

## 暂时不要做

- 不做 OCR。
- 不做本地 LLM 运行时。
- 不做 macOS 支持。
- 不做浏览器插件。
- 不添加复杂任务管理文件夹。
- 不添加通用 Agent 框架。
