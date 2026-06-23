# ReadRay 交接记录

最后更新：2026-06-23

## TL;DR

- 当前状态：Tauri + React + TypeScript 脚手架已创建，前端构建通过，Git 已修复，比赛资料已恢复；Windows 上的 VS Build Tools / MSVC / Windows SDK 已修复，阶段 A 磁盘迁移已完成；阶段一桌面基础能力已完成第一轮工程验证。
- 当前路线：Windows 原生，Tauri + React + TypeScript + Rust + SQLite。
- 下一步：复制 `.env.example` 为 `.env` 并设置 `DEEPSEEK_API_KEY` 后做 DeepSeek 真实 API 调用验证；随后进入解释卡 MVP 的最小闭环。
- 当前约束：不使用通用 Agent 框架，不内置商业词典，不做 OCR、本地大模型或跨平台支持。
- 交接原则：`HANDOFF.md` 只记录会影响下一次恢复上下文的信息，小型文档措辞和格式调整不记录。

## 当前文件

- `AGENTS.md`：给 Codex 使用的协作说明。
- `docs/DEVELOPMENT_PLAN.md`：项目计划和技术方向。
- `docs/RESOURCE_MAP.yml`：重要本地资源索引。
- `docs/HANDOFF.md`：当前交接记录，也就是本文件。
- `docs/WINDOWS_ENVIRONMENT.md`：Windows 开发环境、VS Build Tools 修复经验和磁盘策略。
- `.env.example`：本地开发环境变量占位示例；真实 `.env` 不提交。
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
- 2026-06-22 重启 Windows 后，已通过 Visual Studio Installer 修复并补齐 Visual Studio 2022 Build Tools。
- `pnpm tauri info` 已能识别 WebView2、MSVC、Rust、Cargo 和 stable MSVC Rust toolchain。
- `vswhere -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -requires Microsoft.VisualStudio.Component.Windows11SDK.26100` 已能匹配完整 Build Tools 实例。
- `pnpm tauri dev` 已通过 `link.exe` 阶段，生成 `src-tauri/target/debug/readray.exe` 并启动应用进程；验证后已停止本次测试进程。
- 已完成阶段 A 磁盘迁移：`C:\Users\19150\.cargo`、`C:\Users\19150\.rustup` 和 `C:\ProgramData\Microsoft\VisualStudio\Packages` 通过 Junction 指向 D 盘缓存目录。
- VS `CachePath` 已设置为 `D:\app_cache\vs\Packages`，迁移备份已删除；C 盘剩余空间从约 4.94 GB 提升到约 8.21 GB。
- 迁移后已验证 `rustup show home`、`cargo -V`、`rustc -V`、`pnpm build`、`pnpm tauri info` 和 `cargo check --manifest-path src-tauri\Cargo.toml`。
- 已接入 Tauri 官方插件：`global-shortcut`、`clipboard-manager`、`sql`。
- 已新增阶段一验证面板：窗口显示/隐藏、窗口置顶、剪贴板读写、SQLite 读写、DeepSeek API smoke test。
- 已在 Rust 层注册全局快捷键 `Ctrl+Alt+R`，用于显示/隐藏主窗口。
- DeepSeek smoke test 通过 Rust command 调用，读取 `DEEPSEEK_API_KEY`；默认模型为 `deepseek-v4-flash`，可用 `DEEPSEEK_MODEL` 覆盖。
- Tauri 启动时已通过 Rust `dotenvy` 从项目根目录加载 `.env`，因此 DeepSeek smoke test 可读取 `.env` 中的 `DEEPSEEK_API_KEY`。
- 新增验证后已通过 `cargo fmt --manifest-path src-tauri\Cargo.toml`、`pnpm build`、`cargo check --manifest-path src-tauri\Cargo.toml`、`pnpm tauri info`。
- 新增验证后 `pnpm tauri dev` 已完成编译并启动 `src-tauri/target/debug/readray.exe`；验证后已停止本次测试进程。
- 已在运行窗口中完成第一轮人工点击验证：窗口显示/隐藏、`Ctrl+Alt+R` 恢复窗口、剪贴板读写、SQLite 写入读取均可用。
- 仓库不提交真实 `.env`；未创建真实 `.env` 时，DeepSeek smoke test 能正确提示跳过真实 API 调用。
- 已从大赛官网恢复 `resource/` 页面、附件和文本抽取，并补回 `readray_competition_analysis.md`。
- 已修复原空 `.git` 目录，重新初始化为有效 Git 仓库。
- 已在 `.gitignore` 中忽略 `src-tauri/target`。

## 下一步

继续推进：阶段一基础桌面能力已经完成第一轮工程验证。当前最先要处理的阻塞：

- 仓库不包含真实 `DEEPSEEK_API_KEY`；本地开发可复制 `.env.example` 为 `.env` 后填写密钥，再验证真实 DeepSeek 调用。
- 设置 API key 并验证真实 DeepSeek 调用后，可以进入阶段二解释卡 MVP。

阶段一完成标准：

- 全局快捷键显示和隐藏窗口。
- 窗口置顶和隐藏行为。
- 剪贴板读取。
- SQLite 读写。
- DeepSeek API 调用。
- Windows 本地开发构建。

## 待解决问题

- 仓库不提交真实 `.env`；未创建真实 `.env` 时，DeepSeek API smoke test 会提示跳过真实调用。
- Codex 沙箱内普通命令访问 `C:\Users\19150\.cargo` / `.rustup` Junction 可能报权限或 rustup home 创建误报；沙箱外同一用户验证正常，后续环境验证优先以沙箱外命令为准。
- C 盘空间已缓解但仍需留意；VS Build Tools 主体和 Windows Kits 仍在 C 盘，若空间再次吃紧，再评估卸载后重装 VS Build Tools 到 D 盘。
- UI 具体设计暂时不定。
- SQLite schema 在解释卡和本地记忆阶段再设计。

## 暂时不要做

- 不做 OCR。
- 不做本地 LLM 运行时。
- 不做 macOS 支持。
- 不做浏览器插件。
- 不添加复杂任务管理文件夹。
- 不添加通用 Agent 框架。
