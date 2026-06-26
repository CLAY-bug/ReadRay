# ReadRay 交接记录

最后更新：2026-06-26

## TL;DR

- 当前状态：Tauri + React + TypeScript 脚手架已创建，前端构建通过，Git 已修复，比赛资料已恢复；Windows 上的 VS Build Tools / MSVC / Windows SDK 已修复，阶段 A 磁盘迁移已完成；阶段一桌面基础能力已完成第一轮工程验证。
- 当前路线：Windows 原生，Tauri + React + TypeScript + Rust + SQLite。
- 下一步：把当前 compact preview 迁移成真实桌面 overlay 壳，让无选区输入框先浮在真实桌面上；本机 `.env` 已配置 `DEEPSEEK_API_KEY` 且无选区真实查询已接通。
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
- `src/components/AnchoredResultPopover.tsx`：MVP compact UI 的锚定划词结果浮层组件骨架，当前由 App 传入静态 mock 数据和 mock anchorRect。
- `src/components/CenteredCommandInput.tsx`：MVP compact UI 的无选区居中输入组件骨架，当前由 App 传入真实查询状态。
- `src/components/CenteredResultPanel.tsx`：MVP compact UI 的无选区输入后居中结果面板骨架，当前由 App 传入 ExplanationCard 映射后的真实查询结果。
- `src/styles/tokens.css`：ReadRay Graphite + Amber 轻量样式 token。
- `src-tauri/`：Tauri v2 / Rust 原生层脚手架源码；当前 Tauri 主窗口配置已调整为 compact 预览尺寸。
- `src-tauri/src/explanation.rs`：阶段二 ExplanationCard 中间协议、CaptureInput 类型和 Rust validator；当前只做 schema 与校验，不接真实 DeepSeek。
- `src-tauri/src/deepseek_explanation.rs`：DeepSeek 结构化 ExplanationCard 查询 command、prompt、响应解析和 validator 装配；当前不接前端 UI。
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
- ReadRay 的差异化不能停留在“复制一个单词后快捷查词”；后续需要研究 Windows 跨应用划词上下文捕获：用户只选中单词时，尽可能获取所在句子或段落作为 `contextText`，再生成语境义。
- 暂不做浏览器插件方向，因为浏览器已有沉浸式翻译、陪读蛙等成熟同类工具；优先面向 Windows 桌面应用，尤其是 Electron 类应用和常用阅读/写作软件。
- 当前 Tauri compact preview 只是开发模拟舞台：外层 ReadRay 窗口模拟桌面/阅读环境，mock selected word 模拟真实划词，AnchoredResultPopover 模拟未来贴近真实选区出现的结果浮层；最终产品不应出现这个大背景舞台。
- 正式交互分为两种状态：有选区和 `anchorRect` 时显示锚定结果浮层；无选区时通过快捷键呼出居中输入框，用户手动输入后再切换到结果态。
- ExplanationCard 是 ReadRay 的中间协议，服务 DeepSeek 结构化输出、compact UI 映射和后续 SQLite 本地记忆；它不是某个前端组件的 props。
- 解释卡上下文规则：只有输入侧存在 `contextText` 时，输出侧才允许 `contextMeaning`；无上下文时必须降级为普通解释。
- UI 信息原则：不要为了填充而展示低信息标签；例如未定义 ReadRay 难度体系前，不展示模型自由生成的 CEFR 难度，结果头部右侧只有在 `reviewHint` 有实际内容时才显示。

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
- 仓库不提交真实 `.env`；本机 `.env` 已配置 `DEEPSEEK_API_KEY`，DeepSeek API smoke test 已通过，默认模型为 `deepseek-v4-flash`，返回 `status=200`。未创建真实 `.env` 的新环境仍会提示跳过真实 API 调用。
- 已从大赛官网恢复 `resource/` 页面、附件和文本抽取，并补回 `readray_competition_analysis.md`。
- 已修复原空 `.git` 目录，重新初始化为有效 Git 仓库。
- 已在 `.gitignore` 中忽略 `src-tauri/target`。
- 已建立 MVP compact UI 第一版前端基础：轻量样式 token、`AnchoredResultPopover` 静态 mock 组件，并在当前 App 中作为主视觉展示；阶段一验证面板保留为辅助区。
- 已扩展 `AnchoredResultPopover` 交互骨架：支持 `result`、`anchorRect`、`open`、`onOpenChange` props，组件内完成 fixed 定位、下方优先、空间不足向上翻转、横向 viewport 限制、Esc 隐藏和 `highlightText` 例句高亮；当前 App 使用 mock 锚点 DOM 获取 rect，并提供“重新显示”预览按钮。
- 已接入并修正 Tauri 桌面端 compact 预览壳：App 默认只展示 mock selected word 和 AnchoredResultPopover，阶段一验证能力收进右上角“开发验证”入口；Tauri 主窗口初始尺寸调整为 430×350，最小尺寸为 420×320，未改 Rust 捕获逻辑。
- 已新增无选区 `CenteredCommandInput` 交互骨架：支持打开自动聚焦、Esc 关闭、Enter 非空提交、loading 轻量呼吸点和 error-lite 轻提示；当前 App 可在 mock 划词浮层和无选区输入之间切换，无选区输入默认为空。
- 已新增无选区输入后的 `CenteredResultPanel` 结果态骨架：当前 App 的无选区模式从 input/loading 进入 result，结果面板与输入框同宽，顶部对齐输入框顶部并向下展开，顶部保留可编辑 query，内容区展示语块、近义理解和例句，内部可滚动；查询完成后可在顶部输入框 Backspace 删除并继续输入下一次查询；不做聊天流或底部操作入口。
- 已新增阶段二 ExplanationCard schema 与 Rust validator：支持 `word` / `phrase` / `sentence` 查询类型和 `manual` / `clipboard` / `windows_uia` / `app_adapter` / `ocr` 来源类型；当前 validator 覆盖必填非空、文本限长、数组限长、例句至少 1 个最多 2 个且英中双语必填，以及 `contextText` / `contextMeaning` 约束；未做 SQLite schema。
- 已新增 Rust command `create_explanation_card`：输入 `CaptureInput`，调用 DeepSeek JSON Output，解析为 `ExplanationCard`，再调用 `validate_explanation_card`；失败时区分请求失败、HTTP 非成功、响应结构解析失败、JSON 内容解析失败和 validator 失败；当前未接 compact UI。
- 无选区居中输入已接入真实查询：提交时构造 `CaptureInput { queryText, contextText: null, sourceType: manual }`，调用 `create_explanation_card`，成功后映射到 `CenteredResultPanel`，失败时复用输入框的 error-lite 状态；当前不展示未定义标准的 difficulty，短语行已修复长英文和中文解释重叠问题；未改划词浮层、UI Automation、SQLite 或历史功能。

## 下一步

继续推进：阶段一基础桌面能力已经完成第一轮工程验证。当前最先要处理的阻塞：

- 下一步优先做真实桌面 overlay 壳：移除当前模拟舞台背景，把 Tauri 主窗口改成透明、无边框、置顶、默认隐藏，并让无选区输入框先真实浮在桌面上。
- DeepSeek key 不再是当前本机阻塞；后续阶段二解释卡 MVP 可以复用现有 command，但不要把 overlay 壳、SQLite schema 和上下文捕获混在一次大改里。
- 新环境仍需复制 `.env.example` 为 `.env` 后自行填写 `DEEPSEEK_API_KEY`；真实 `.env` 不提交。

阶段一完成标准：

- 全局快捷键显示和隐藏窗口。
- 窗口置顶和隐藏行为。
- 剪贴板读取。
- SQLite 读写。
- DeepSeek API 调用。
- Windows 本地开发构建。

## 待解决问题

- UI 设计完成后，专门研究“划词获取上下文”的技术 spike。目标输入模型应区分 `selectedText` 和 `contextText`：只拿到剪贴板单词时降级为普通查词；能通过 Windows UI Automation、应用适配器或后续 OCR 取得上下文时，才展示语境义。
- 该能力不要求一开始支持所有应用，应先验证常用 Windows 桌面场景，例如 VS Code、Obsidian、Notion Desktop、WPS/Word、PDF 阅读器等。Electron 应用本质基于 Chromium，通常可能暴露无障碍树，但仍需逐应用验证。
- 后续技术路线建议按优先级评估：Windows UI Automation 读取前台窗口/焦点控件/选区和文本范围；高价值应用做专门适配器；OCR 仅作为更后期兜底；剪贴板查词只作为 fallback，不作为最终差异化。
- 仓库不提交真实 `.env`；本机已配置 DeepSeek key，但新环境仍需复制 `.env.example` 为 `.env` 后自行填写。
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
