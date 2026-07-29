# ReadRay 交接记录

最后更新：2026-07-29

## TL;DR

- 当前状态：主应用“记忆”页与“今天”页均已接入真实 Tauri/SQLite 数据；完整对话页已按 OpenDesign HTML 在同一外壳内实现，前端 fixture 已具备正确的多轮 thread、停止/继续、重生成、失败/重试、完整导出和抽屉焦点状态。
- 当前路线：Windows 原生，Tauri + React + TypeScript + Rust + SQLite。
- 下一步：在单独任务中设计真实 Quick AI repository/service 接线；复习页仍需单独设计。
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
- `src/components/AnchoredResultPopover.tsx`：锚定划词结果浮层，按结果类型选择宽度并测量内容高度。
- `src/components/ExplanationResultContent.tsx`：四类解释结果共享展示结构。
- `src/explanationViewModel.ts` / `src/types/explanation.ts`：Rust 协议的前端判别类型和展示映射。
- `src/components/CenteredCommandInput.tsx`：MVP compact UI 的无选区居中输入组件骨架，当前由 App 传入真实查询状态。
- `src/components/CenteredResultPanel.tsx`：MVP compact UI 的无选区输入后居中结果面板骨架，当前由 App 传入 ExplanationCard 映射后的真实查询结果。
- `src/components/QuickAiPanel.tsx` / `src/types/quickAi.ts`：Quick AI 多轮对话视图及前端协议。
- `src/components/MainAppShell.tsx` / `MainSidebar.tsx` / `TodayPage.tsx`：正常主应用壳、真实最近对话、今天事实摘要与状态装配。
- `src/components/ConversationPage.tsx` / `src/conversationViewModel.ts` / `src/conversationFixtureService.ts`：完整对话页、类型协议和本轮可替换前端 fixture service。
- `src/todayRepository.ts` / `src/todayService.ts`：今日学习记录与 Quick AI 最近标题的 Tauri repository、本机日期范围和展示映射。
- `src/mainAppFixture.ts` / `src/todayPreviewService.ts`：仅浏览器预览或测试使用，不进入正式 Tauri 数据路径。
- `src/components/MemoryPage.tsx` / `src/memoryRepository.ts` / `src/memoryService.ts` / `src/types/learningRecord.ts`：记忆页真实分页/搜索/筛选/详情 UI，Tauri repository、四类 ExplanationCard 映射和 Rust camelCase 返回协议。
- `src/memoryPageFixture.ts` / `src/memoryPreviewService.ts`：仅浏览器预览或测试使用的记忆 fixture，不进入正式 Tauri 数据路径。
- `src/mainAppViewModel.ts` / `src/styles/main-app.css`：主应用展示类型、静态导航和独立浅色视觉样式。
- `src/styles/conversation-page.css`：完整对话内容区、消息流、输入框、抽屉和窄窗口的独立 `rr-conversation-*` 样式。
- `src/components/WritingPage.tsx` / `WritingEditor.tsx` / `WritingCoach.tsx` / `WritingCompareView.tsx` / `WritingLibrary.tsx`：写作草稿、辅助问答、文章检查、对比、完成稿和文章库交互骨架。
- `src/writingViewModel.ts` / `src/writingRepository.ts` / `src/styles/writing-page.css`：写作类型与 fixture、可替换前端演示 repository、独立 `rr-writing-*` 样式；当前 repository 使用 localStorage 只为演示刷新恢复，不是正式 SQLite 方案。
- `src/assets/fonts/`：应用随包内嵌的 Geist、Geist Mono、Newsreader、思源黑体和思源宋体变量字体及对应 OFL 许可证。
- `src/styles/tokens.css`：ReadRay Graphite + Amber 轻量样式 token。
- `src-tauri/`：Tauri v2 / Rust 原生层脚手架源码；当前同时配置正常主窗口 `main` 与快捷浮窗 `overlay`。
- `src-tauri/src/windows_uia.rs`：Windows UI Automation 上下文捕获与正式划词输入来源；当前已接入 DeepSeek 锚定解释卡。
- `src-tauri/src/explanation.rs`：四类 ExplanationCard 中间协议、CaptureInput、查询类型判断和 Rust validator。
- `src-tauri/src/deepseek_explanation.rs`：分类型 DeepSeek 结构化查询、prompt、响应解析和 validator 装配。
- `src-tauri/src/deepseek_client.rs`：ExplanationCard 与 Quick AI 共用的 DeepSeek HTTP 边界。
- `src-tauri/src/conversations.rs` / `src-tauri/src/quick_ai.rs`：Quick AI SQLite 对话仓库、多轮上下文请求和 Tauri commands。
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
- 本机命令和发布流程优先采用 `docs/WINDOWS_ENVIRONMENT.md` 的已验证基线：Codex 显式使用 D 盘本机 pnpm；普通 GitHub commit/push 通过 HTTPS remote 直推 `main`，不因本机缺少 `gh` 而阻塞；启动 Tauri dev 前先检查 ReadRay 相关进程和 1420 端口。
- 不得在非空项目根目录使用带覆盖或强制语义的初始化命令；如确需使用，必须先确认 Git 可用或完成备份，并说明会影响哪些文件。
- ReadRay 的差异化不能停留在“复制一个单词后快捷查词”；后续需要研究 Windows 跨应用划词上下文捕获：用户只选中单词时，尽可能获取所在句子或段落作为 `contextText`，再生成语境义。
- 暂不做浏览器插件方向，因为浏览器已有沉浸式翻译、陪读蛙等成熟同类工具；优先面向 Windows 桌面应用，尤其是 Electron 类应用和常用阅读/写作软件。
- 原 Tauri compact preview 曾作为开发模拟舞台：外层 ReadRay 窗口模拟桌面/阅读环境，mock selected word 模拟真实划词，AnchoredResultPopover 模拟未来贴近真实选区出现的结果浮层；当前默认主体验已切到无选区桌面 overlay，最终产品不应出现大背景舞台。
- 无选区 overlay 由 `Ctrl+Alt+R` 显式呼出输入态，Esc 或窗口失焦隐藏；输入态/结果态可通过浮层顶部拖动，拖动后的位置会在当前进程内记住；结果态由前端请求 Rust 调整窗口尺寸。
- 当前窗口位置方案已经接受：无拖动记录时使用屏幕偏上区域作为默认位置，拖动后优先恢复当前进程内记录的位置；现阶段不再继续校准默认位置。
- Tauri 窗口角色固定为 `main` 与 `overlay`：`main` 加载 `index.html?view=main`，显示在任务栏并允许调整大小；`overlay` 加载 `index.html`，启动隐藏、置顶且跳过任务栏。两类窗口命令按 label 校验，主窗口状态不得写入 overlay 位置缓存。
- 主窗口关闭策略暂定为隐藏而非退出进程，使全局快捷键和隐藏的 overlay 继续存活；当前没有托盘或“重新打开主窗口”入口，该生命周期缺口留待后续单独处理。
- Windows UIA 捕获必须在 ReadRay show/focus 前完成；`Ctrl+Alt+U` 触发划词捕获并显示选区附近的真实 DeepSeek 解释卡，`Ctrl+Alt+R` 保持无选区居中输入流程。两条链路共享 create_explanation_card，不接 SQLite、OCR 或剪贴板辅助。
- 正式交互分为两种状态：有选区和 `anchorRect` 时显示锚定结果浮层；无选区时通过快捷键呼出居中输入框，用户手动输入后再切换到结果态。
- ExplanationCard 是 ReadRay 的中间协议，服务 DeepSeek 结构化输出、compact UI 映射和后续 SQLite 本地记忆；它不是某个前端组件的 props。
- ExplanationCard 使用 `queryType` 判别联合：word 保存词义、语境、原句、搭配和例句；phrase 保存整体义、语境义和构成；sentence/paragraph 以完整中文翻译为第一信息，不强制生成单词卡字段。
- 查询类型只在本地判断，不增加第二次 LLM 请求：单个普通词或 camelCase 标识符判为 word；较短非完整多词内容判为 phrase；完整单句判为 sentence；多句、换行或较长内容判为 paragraph。
- 解释卡上下文规则：只有输入侧存在 `contextText` 时，输出侧才允许 `contextMeaning`；无上下文时必须降级为普通解释。
- CaptureInput 的 queryText/contextText 上限为 4096 字符；本轮支持用户主动选择的长句和段落，不做整页翻译。
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
- 已接入 Tauri 官方插件：`global-shortcut`、`clipboard-manager`；SQLite 由 Rust `rusqlite` bundled 负责，不再使用前端 SQL 插件。
- 已新增阶段一验证面板：窗口显示/隐藏、窗口置顶、剪贴板读写、SQLite 读写、DeepSeek API smoke test。
- 已在 Rust 层注册全局快捷键 `Ctrl+Alt+R`，用于呼出快捷 overlay 的无选区输入态。
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
- 已将 ExplanationCard 重构为 `word` / `phrase` / `sentence` / `paragraph` 四类 serde tagged enum；validator 按类型执行必填、文本限长、数组限长、双语例句和上下文约束，CaptureInput 上限与 UIA 对齐到 4096 字符。
- 已新增 Rust command `create_explanation_card`：输入 `CaptureInput`，调用 DeepSeek JSON Output，解析为 `ExplanationCard`，再调用 `validate_explanation_card`；失败时区分请求失败、HTTP 非成功、响应结构解析失败、JSON 内容解析失败和 validator 失败；当前未接 compact UI。
- 无选区居中输入已接入真实查询：提交时构造 `CaptureInput { queryText, contextText: null, sourceType: manual }`，调用 `create_explanation_card`，成功后映射到 `CenteredResultPanel`，失败时复用输入框的 error-lite 状态；当前不展示未定义标准的 difficulty，短语行已修复长英文和中文解释重叠问题；未改划词浮层、UI Automation、SQLite 或历史功能。
- 已把 compact preview 迁移为真实桌面 overlay 壳：`overlay` 窗口无边框、透明、置顶、不可缩放并跳过任务栏；输入/loading 态窗口为 720×104，错误态 720×132，结果态 800×560；前端通过 `set_overlay_window_stage`、`hide_overlay_window` 控制尺寸和隐藏。
- 已完成第一轮 overlay 行为校准：无拖动记录时输入框定位到屏幕偏上区域；输入态和结果态提供拖拽区域，前端拖动手势调用 Rust commands 移动窗口并在当前进程内显式记住位置，后续快捷键呼出优先恢复该位置；窗口失焦时自动隐藏；产品浮层默认不显示右上角开发控件，开发期可用 `Ctrl+Shift+D` 临时显示；默认浮层厚黑阴影已移除，仅保留边界线。当前体验已接受，不再继续微调窗口位置。
- 已新增 Windows UI Automation spike：Windows target 直接依赖 `windows 0.61.3`，只启用 Foundation、COM、Ole、Threading、Accessibility 和 WindowsAndMessaging feature；捕获前台进程、窗口标题、焦点元素诊断，优先 `TextPattern2`、回退 `TextPattern`，并分别返回 `selectedText`、Paragraph `contextText` 和物理屏幕坐标 `anchorRect`。
- Obsidian 1.12.7 真实验证结果：编辑模式和阅读模式的单词、短语、无选区、滚动后选区均按预期通过；有选区时分别取得 `selectedText`、Paragraph `contextText` 和物理屏幕像素 `anchorRect`，无选区时三项均为 `null`。阅读模式不能只检查焦点元素本身，需要沿 Raw View 祖先链找到 Document 的 `TextPattern2`；成功样本未使用 MSAA/IAccessible2 或剪贴板辅助。
- Codex AppX `26.623.5546.0` 真实验证结果：渲染消息区的单词、短语、无选区和滚动后选区均按预期通过，命中 `TextPattern`，捕获耗时 35-60 ms；编辑输入区选区也能取得文本和坐标，耗时 26 ms，但 Paragraph 上下文只包含选中文本并夹有对象替换字符。渲染消息区可直接复用通用 UIA，编辑区若要提供完整上下文仍需后续降级或区域适配。
- 已完成正式划词接线：前端收到 UIA 捕获后清理 U+200B/U+FFFC，将退化上下文降级为 null，以 SourceType::WindowsUia 调用现有 DeepSeek command；Tauri 按物理 anchorRect、显示器 DPI 和工作区放置 anchored loading/result/error 小窗口。Codex App 的 `DeepSeek`、Obsidian 阅读区的 `hot session` 均真实返回解释卡。
- 已完成分类型解释与自适应窗口第一轮实现：DeepSeek Flash 按本地 queryType 输出严格 JSON；前端不再把第一个 phrase 映射为“用法”，句子/段落优先展示完整翻译；锚定窗口按内容测量收缩或扩展，宽度随类型约为 500-700 像素，最大高度约为显示器工作区 70%，超出时内部滚动。
- 已完成不联网 Rust 测试，并用本机 `.env` 对 word、camelCase 标识符、超过 120 字符的句子和段落完成四次真实 DeepSeek Flash 请求；模型返回的 sourceText 在 serde 前由捕获输入覆盖，避免模型改写原文。
- 已完成阶段三第一轮 SQLite 本地记忆数据底座：Rust `learning_records` 使用 `rusqlite` bundled，在 Tauri 应用数据目录创建 `readray.sqlite3`；`schema_migrations` 记录已执行迁移，v1 追加保存每一次成功查询事件，不以重复文本覆盖旧记录。
- 学习事件字段包含自增 ID、原始 queryText、标准化文本、queryType、sourceType、可选 sourceApp、可选 contextText、完整 ExplanationCard JSON、ExplanationCard schemaVersion、创建时间和可空 difficulty；当前不生成虚假难度，统一保存 `NULL`。
- `create_explanation_card` 仍先完成 DeepSeek 解析与 validator，只有成功后才调用学习记录写入；manual 与 windows_uia 共用同一 command 链路。请求、解析、validator 或存储失败均不留下学习记录，并返回可诊断错误。
- 学习记录 Rust/Tauri commands 已覆盖分页、搜索、单条、删除与只读今日摘要；主应用“记忆”和“今天”均通过各自 repository/service 调用，前端页面不接触 SQL。
- 已完成 Ctrl+Alt+R 居中窗口 Quick AI 第一版：默认仍为解释输入，按 Tab 进入 Quick AI；有输入时自动作为首条消息，空输入创建空白对话；支持 Enter 发送、Shift+Enter 换行、Ctrl+N 新建对话和 Esc 隐藏。
- Quick AI 使用独立普通 chat/completions 请求，不复用 ExplanationCard JSON；ExplanationCard 与 Quick AI 仅共用 `.env`、DeepSeek model/API key 和 HTTP 错误边界，默认模型为 `deepseek-v4-flash`。
- 正常主应用与快捷 overlay 已建立独立 Tauri 窗口：正常启动只显示 `main` 主应用，隐藏的 `overlay` 继续监听全局快捷键和 UIA 事件；主窗口失焦不隐藏，只有 overlay 失焦自动隐藏。
- 快捷 overlay 的呼出意图由 Rust 先原子保存，再由前端在事件、窗口获焦或挂载时领取；程序化呼出有短暂焦点保护。这避免隐藏 WebView 漏事件导致 `Ctrl+Alt+R` 只能呼出一次，或 `Ctrl+Alt+U` 错误沿用居中输入态。
- 主应用左侧栏只允许用户手动折叠，不根据窗口宽度自动折叠；默认 1440×900 / scale 1 下展开与折叠宽度为 252/72px。展开态显示品牌和折叠按钮，折叠态隐藏品牌、只在侧栏顶部居中显示展开按钮。
- SQLite migration v2 新增 `quick_ai_conversations` 和 `quick_ai_messages`，每条消息保存 role、content、sequence 和时间；与 `learning_records` 完全分表，结构可供未来主应用读取并继续对话。
- Quick AI 已完成真实 DeepSeek Flash 两轮连续对话和真实窗口 smoke：第二轮能使用第一轮上下文；Ctrl+N 与 Esc 行为通过。当前响应按纯文本展示，不解析 Markdown。
- “今天”首页已通过独立 Today repository/service 接入真实数据：前端按本机日期传入当天起止时间，Rust 只读返回今日查询总数和最新 LearningRecord；摘要只陈述数量、最近查询、类型、来源与时间，不生成复习数量、高频词或趋势。
- 首页原复习卡改为“查看今天的学习记录”，最近查询入口进入现有记忆页并选中对应记录，写作入口仍只进入文章库；无今日记录时显示诚实空状态并禁用无目标入口。overlay 保存成功事件会同时刷新已打开的记忆页与今天页。
- 侧栏最近对话通过最小只读 list_recent_quick_ai_conversations command 获取真实 Quick AI 标题，排除无标题空对话；点击最近对话、新对话与首页输入已进入完整对话页，查看全部仍保留 callback。
- 主应用全局外壳、“今天”页与“记忆”页统一以 1440×900 和 `--rr-main-design-scale: 1` 为默认基线，不再混用外壳 0.75、内容区 1.0 两套比例。浏览器预览始终排版完整 1440×900 画布，再按可用空间做单一整体缩放；主应用响应式断点改由应用容器尺寸触发，避免较小的预览视口错误压缩页面节奏。
- 应用默认字体已更新为随包内嵌的 Geist + Source Han Sans SC（界面）、Newsreader + Source Han Serif SC（阅读正文）、Geist Mono + Source Han Sans SC（元信息），并提交四份对应 OFL。五个字体文件与本机 OpenDesign 源资源的 SHA-256 一致，完整字体约增加 34MB 应用资源，正式 UI 不依赖联网或本机安装。
- 最近对话标题只在 `scrollWidth > clientWidth` 时应用右侧渐隐，未溢出的短标题完整显示；“查看全部对话”不再使用前置省略号。侧栏折叠后只显示居中的展开控件，不在内容标题区放置侧栏开关。
- 首页输入框已移除底部快捷键和本地保存提示；输入内容变化或窗口缩放时按真实 `scrollHeight` 自动增高和回缩，高度上限为随窗口变化的 120–240px，达到上限后保留内部滚动但隐藏原生滚动条，不再出现上下箭头。
- 主应用标题栏已接通原生拖动、最小化、最大化/恢复和隐藏；主窗口初始尺寸为 1440×900、最小尺寸为 840×600，无边框、可缩放并显示在任务栏，Windows/Tauri 原生阴影和页面外阴影均已关闭。
- 本机 150% DPI 的真实 Tauri WebView 实测为 `innerWidth=1440`、`innerHeight=900`、`devicePixelRatio=1.5`；主应用 1440×900、标题栏 44px、展开侧栏 252px、导航行 38px、记忆内容壳 1048px、搜索框 44px、记录列 368px。品牌/导航/最近对话 computed font-size 分别为 14/14/13px，均实际命中随包字体。`pnpm build` 与 `pnpm tauri dev` 均通过，未改 overlay、Quick AI、UIA、DeepSeek、SQLite 或快捷键行为。
- 主应用“记忆”内容区已接入真实 Tauri/SQLite 学习记录：TypeScript 协议与 Rust camelCase 返回一致，独立 repository/service 负责 list/search/get commands 和四类 ExplanationCard 到 MemoryRecordItem 的映射；总数、分页、关键词、queryType、选择详情、真实来源应用和今天/昨天/更早时间分组均来自后端记录。
- 正式 Tauri 路径不再读取 memory fixture；fixture 仅由非 Tauri 浏览器预览动态加载。overlay 成功保存 ExplanationCard 后广播记录更新，已打开的主窗口记忆页会重新读取当前后端查询。当前 schema 没有可靠的重复出现聚合，正式记录的“过去的出现”入口保持隐藏。
- “记忆”与“今天”在同一主应用外壳内切换；侧栏仍只允许手动折叠，折叠态继续隐藏品牌并保留导航/设置图标，记忆选中态保持可见。记忆页样式只使用 `rr-memory-*` 作用域，没有改变 overlay、Quick AI、UIA、快捷键或原生窗口行为。
- 已按 `design-open-design/readray-writing-2.html` 在现有 MainAppShell 中实现“写作”页：写作导航和“今天”页写作入口进入本地文章库，支持空白稿/已有稿、标题与正文编辑、自动保存演示、文档切换、选区菜单、“问 ReadRay”多轮追问、四类写作教练问题、定位/修改/进一步提示/参考/忽略、多轮检查、双栏文本差异、写作模式总结、完成版本和继续修改。
- 写作编辑区沿用 1440×900 / scale 1：纸张宽 680px、正文 18px / 1.68 行高、编辑列上限 736px；草稿/完成稿未打开辅助栏时纸张视觉居中，检查或辅助打开后自然重排。900px 以下教练保持 320px 右侧覆盖层，正文列宽不变；全局侧栏仍只允许用户手动在 252/72px 间折叠。
- 写作状态和分析内容来自 `writingViewModel.ts` 的有类型 fixture；`WritingRepository` 隔离了页面与刷新恢复实现，当前 `BrowserWritingDemoRepository` 使用 localStorage，仅作为前端演示，未接 Rust、SQLite、DeepSeek、Quick AI、UIA 或真实写作分析接口。
- 写作页已通过本机 pnpm 构建，并在浏览器 1440×900 与模拟真实应用 840×600 容器完成交互验收：文章库搜索/筛选/排序、空稿、已有稿、选区辅助、追问、问题操作、本轮改动保留、第二轮重新筛选、删除/新增差异、完成稿版本回看、长标题、720 词长正文、侧栏展开/折叠和窄窗教练覆盖均通过；未修改 Tauri 窗口、overlay、快捷键或设计稿目录。
- 已按 `design-open-design/readray-conversation-2.html` 在现有 MainAppShell 中实现完整对话页：保留 1440×900 外壳、736px 消息列和输入区，覆盖空对话、设计示例消息、长提示折叠、生成/停止/失败/重试、更多菜单、导出提示和记忆引用抽屉。
- 今天页输入、新对话和最近对话均已进入完整对话页；最近对话继续由现有 TodayService 提供标题，对话正文当前由有类型 `FixtureConversationService` 映射，不调用 Tauri、SQLite、DeepSeek 或真实 Quick AI commands。
- `ConversationService.generateReply` 现显式接收 conversationId、完整消息上下文、prompt 和 append/regenerate 模式；续问保留全历史，完成回答写回 thread，重生成只替换当前最后一轮 assistant。生成未结束时输入草稿保持可编辑但禁止提交和导出；停止/继续保留已生成分片，create/load/generate/export 异常均有保留内容的失败状态。
- 完成态 fixture 导出会生成用户可下载的 Markdown 文件，并按顺序包含 thread 的全部 user/assistant 消息；空结果或异常不会触发下载或成功提示。
- fixture 通过 `conversationFailure=create|load|generate|export` 显式注入一次性失败，正常重新生成不再强制失败；`[fixture:slow]` 只用于停止/继续演示。记忆抽屉关闭时会移除内部焦点并恢复到原引用按钮。
- 完整对话页已通过人工验收；本机 pnpm 构建与 Headless Playwright 回归均通过，覆盖生成中重复发送、连续两轮、生成中导出、完整 Markdown 下载、导出失败重试、停止/继续、重新生成和抽屉焦点。1440×900、840×600 与两档侧栏无页面级横纵溢出或重叠；`preview=responsive` 只用于浏览器真实容器验收，不改变默认等比预览和 Tauri。

## 下一步

继续推进：完整对话页视觉、前端状态和人工验收均已完成；下一项是在保持现有 `ConversationService` 边界的前提下，单独设计真实对话 repository/service 接线和复习页。

- 记忆 UI 已通过 repository/service 调用 Rust 的分页、关键词搜索、queryType 筛选和单条读取 commands，不向前端暴露 SQL 或数据库路径；删除 command 本轮未增加页面入口。
- 继续保持每次成功查询为独立事件；重复查询聚合、高频词、趋势和复盘规划属于后续阶段。
- Quick AI 现已提供最小最近标题列表；对话页已具备 UI，但完整历史读取、真实继续对话、导出、删除和重命名仍待 repository/service 与 commands。
- 首页输入已经进入完整对话页，不再丢失可见响应；当前响应仍是 fixture，接真实发送时必须替换 ConversationService，不能从页面组件直接 invoke。
- 新环境仍需复制 `.env.example` 为 `.env` 后自行填写 `DEEPSEEK_API_KEY`；真实 `.env` 不提交。

阶段一完成标准：

- 全局快捷键显示和隐藏窗口。
- 窗口置顶和隐藏行为。
- 剪贴板读取。
- SQLite 读写。
- DeepSeek API 调用。
- Windows 本地开发构建。

## 待解决问题

- SQLite v1 已按追加事件定义；后续若需要聚合或复盘状态，应增加独立表和新 migration，不回填或覆盖原始 learning_records 事件。
- Obsidian 的 UIA provider 路径存在模式差异：编辑模式可直接从焦点 Edit 取得 TextPattern2，阅读模式需要从焦点/光标元素沿 Raw View 祖先链查找 Document。当前两种模式均可用，但仍需观察不同主题、页面结构和 Obsidian 版本下的稳定性。
- Codex App 的渲染内容区使用 `TextPattern`，常见来源为 `cursorPoint` 或滚动容器祖先；编辑输入区使用 ProseMirror，选区和坐标可用，但 Paragraph 上下文不完整。正式接线不能把这种退化上下文当成完整语境。
- 该能力不要求一开始支持所有应用，应先验证常用 Windows 桌面场景，例如 VS Code、Obsidian、Notion Desktop、WPS/Word、PDF 阅读器等。Electron 应用本质基于 Chromium，通常可能暴露无障碍树，但仍需逐应用验证。
- 后续技术路线建议按优先级评估：Windows UI Automation 读取前台窗口/焦点控件/选区和文本范围；高价值应用做专门适配器；OCR 仅作为更后期兜底；剪贴板查词只作为 fallback，不作为最终差异化。
- 仓库不提交真实 `.env`；本机已配置 DeepSeek key，但新环境仍需复制 `.env.example` 为 `.env` 后自行填写。
- Codex 沙箱内普通命令访问 `C:\Users\19150\.cargo` / `.rustup` Junction 可能报权限或 rustup home 创建误报；沙箱外同一用户验证正常，后续环境验证优先以沙箱外命令为准。
- C 盘空间已缓解但仍需留意；VS Build Tools 主体和 Windows Kits 仍在 C 盘，若空间再次吃紧，再评估卸载后重装 VS Build Tools 到 D 盘。
- 本地查询分类是启发式规则，缩写、多句但很短的文本、缺少句末标点的长句仍可能被相邻类型吸收；优先通过真实样本调整规则，不增加第二次 LLM 分类请求。
- 长段落输出受模型 JSON 稳定性和窗口最大高度约束；当前上限 4096 字符，不代表整页翻译能力。
- 记忆页的重复出现聚合仍未实现；当前 learning_records 只保存独立查询事件，不能可靠生成“过去的出现”次数或时间线，因此正式 UI 隐藏该入口。
- Quick AI 当前不渲染 Markdown，也不做流式输出；长回复需要等待完整响应后一次显示，这是后续体验优化点，不影响当前多轮对话闭环。
- 主应用完整对话页当前使用前端 fixture service：页面状态和 Markdown 下载已验收，但刷新持久化、真实历史、真实模型发送、原生导出持久化和记忆引用聚合尚未接线。
- 主窗口关闭后当前只隐藏并保持后台快捷键能力，但没有托盘、单实例或重新显示主窗口入口；发布前需要确定“关闭即隐藏”是否为正式产品策略，并补齐重新进入主应用的路径。

## 暂时不要做

- 不做 OCR。
- 不做本地 LLM 运行时。
- 不做 macOS 支持。
- 不做浏览器插件。
- 不添加复杂任务管理文件夹。
- 不添加通用 Agent 框架。
