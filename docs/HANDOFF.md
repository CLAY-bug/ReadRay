# ReadRay 交接记录

最后更新：2026-06-28

## TL;DR

- 当前状态：Tauri + React + TypeScript 脚手架已创建，前端构建通过，Git 已修复，比赛资料已恢复；Windows 上的 VS Build Tools / MSVC / Windows SDK 已修复，阶段 A 磁盘迁移已完成；阶段一桌面基础能力已完成第一轮工程验证。
- 当前路线：Windows 原生，Tauri + React + TypeScript + Rust + SQLite。
- 下一步：`UIA 捕获 -> DeepSeek -> 选区附近解释卡` 已完成第一轮正式接线；解释协议已按 word/phrase/sentence/paragraph 分型，并支持最长 4096 字符选区和内容驱动窗口尺寸。后续优先补齐更多应用兼容验证与长文本真实使用反馈。本机 `.env` 已配置 `DEEPSEEK_API_KEY`，无选区与划词查询均已接通。
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
- `src/styles/tokens.css`：ReadRay Graphite + Amber 轻量样式 token。
- `src-tauri/`：Tauri v2 / Rust 原生层脚手架源码；当前 Tauri 主窗口配置已调整为无边框、透明、置顶 overlay 壳。
- `src-tauri/src/windows_uia.rs`：Windows UI Automation 上下文捕获与正式划词输入来源；当前已接入 DeepSeek 锚定解释卡。
- `src-tauri/src/explanation.rs`：四类 ExplanationCard 中间协议、CaptureInput、查询类型判断和 Rust validator。
- `src-tauri/src/deepseek_explanation.rs`：分类型 DeepSeek 结构化查询、prompt、响应解析和 validator 装配。
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
- 本机命令和发布流程优先采用 `docs/WINDOWS_ENVIRONMENT.md` 的已验证基线：Codex 显式使用 D 盘本机 pnpm；普通 GitHub commit/push 通过现有 SSH remote 直推 `main`，不因本机缺少 `gh` 而阻塞；启动 Tauri dev 前先检查 ReadRay 相关进程和 1420 端口。
- 不得在非空项目根目录使用带覆盖或强制语义的初始化命令；如确需使用，必须先确认 Git 可用或完成备份，并说明会影响哪些文件。
- ReadRay 的差异化不能停留在“复制一个单词后快捷查词”；后续需要研究 Windows 跨应用划词上下文捕获：用户只选中单词时，尽可能获取所在句子或段落作为 `contextText`，再生成语境义。
- 暂不做浏览器插件方向，因为浏览器已有沉浸式翻译、陪读蛙等成熟同类工具；优先面向 Windows 桌面应用，尤其是 Electron 类应用和常用阅读/写作软件。
- 原 Tauri compact preview 曾作为开发模拟舞台：外层 ReadRay 窗口模拟桌面/阅读环境，mock selected word 模拟真实划词，AnchoredResultPopover 模拟未来贴近真实选区出现的结果浮层；当前默认主体验已切到无选区桌面 overlay，最终产品不应出现大背景舞台。
- 无选区 overlay 是当前优先体验：启动显示输入态浮层，Esc 或窗口失焦隐藏窗口，`Ctrl+Alt+R` 重新呼出输入态；输入态/结果态可通过浮层顶部拖动，拖动后的位置会在当前进程内记住；结果态由前端请求 Rust 调整窗口尺寸。
- 当前窗口位置方案已经接受：无拖动记录时使用屏幕偏上区域作为默认位置，拖动后优先恢复当前进程内记录的位置；现阶段不再继续校准默认位置。
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
- 已将 ExplanationCard 重构为 `word` / `phrase` / `sentence` / `paragraph` 四类 serde tagged enum；validator 按类型执行必填、文本限长、数组限长、双语例句和上下文约束，CaptureInput 上限与 UIA 对齐到 4096 字符。
- 已新增 Rust command `create_explanation_card`：输入 `CaptureInput`，调用 DeepSeek JSON Output，解析为 `ExplanationCard`，再调用 `validate_explanation_card`；失败时区分请求失败、HTTP 非成功、响应结构解析失败、JSON 内容解析失败和 validator 失败；当前未接 compact UI。
- 无选区居中输入已接入真实查询：提交时构造 `CaptureInput { queryText, contextText: null, sourceType: manual }`，调用 `create_explanation_card`，成功后映射到 `CenteredResultPanel`，失败时复用输入框的 error-lite 状态；当前不展示未定义标准的 difficulty，短语行已修复长英文和中文解释重叠问题；未改划词浮层、UI Automation、SQLite 或历史功能。
- 已把 compact preview 迁移为真实桌面 overlay 壳：Tauri 主窗口无边框、透明、置顶、不可缩放并跳过任务栏；输入/loading 态窗口为 720×104，错误态 720×132，结果态 800×560；前端通过 `prepare_overlay_input_window`、`set_overlay_window_stage`、`hide_overlay_window` 控制显示、尺寸和隐藏。
- 已完成第一轮 overlay 行为校准：无拖动记录时输入框定位到屏幕偏上区域；输入态和结果态提供拖拽区域，前端拖动手势调用 Rust commands 移动窗口并在当前进程内显式记住位置，后续快捷键呼出优先恢复该位置；窗口失焦时自动隐藏；产品浮层默认不显示右上角开发控件，开发期可用 `Ctrl+Shift+D` 临时显示；默认浮层厚黑阴影已移除，仅保留边界线。当前体验已接受，不再继续微调窗口位置。
- 已新增 Windows UI Automation spike：Windows target 直接依赖 `windows 0.61.3`，只启用 Foundation、COM、Ole、Threading、Accessibility 和 WindowsAndMessaging feature；捕获前台进程、窗口标题、焦点元素诊断，优先 `TextPattern2`、回退 `TextPattern`，并分别返回 `selectedText`、Paragraph `contextText` 和物理屏幕坐标 `anchorRect`。
- Obsidian 1.12.7 真实验证结果：编辑模式和阅读模式的单词、短语、无选区、滚动后选区均按预期通过；有选区时分别取得 `selectedText`、Paragraph `contextText` 和物理屏幕像素 `anchorRect`，无选区时三项均为 `null`。阅读模式不能只检查焦点元素本身，需要沿 Raw View 祖先链找到 Document 的 `TextPattern2`；成功样本未使用 MSAA/IAccessible2 或剪贴板辅助。
- Codex AppX `26.623.5546.0` 真实验证结果：渲染消息区的单词、短语、无选区和滚动后选区均按预期通过，命中 `TextPattern`，捕获耗时 35-60 ms；编辑输入区选区也能取得文本和坐标，耗时 26 ms，但 Paragraph 上下文只包含选中文本并夹有对象替换字符。渲染消息区可直接复用通用 UIA，编辑区若要提供完整上下文仍需后续降级或区域适配。
- 已完成正式划词接线：前端收到 UIA 捕获后清理 U+200B/U+FFFC，将退化上下文降级为 null，以 SourceType::WindowsUia 调用现有 DeepSeek command；Tauri 按物理 anchorRect、显示器 DPI 和工作区放置 anchored loading/result/error 小窗口。Codex App 的 `DeepSeek`、Obsidian 阅读区的 `hot session` 均真实返回解释卡。
- 已完成分类型解释与自适应窗口第一轮实现：DeepSeek Flash 按本地 queryType 输出严格 JSON；前端不再把第一个 phrase 映射为“用法”，句子/段落优先展示完整翻译；锚定窗口按内容测量收缩或扩展，宽度随类型约为 500-700 像素，最大高度约为显示器工作区 70%，超出时内部滚动。
- 已完成不联网 Rust 测试，并用本机 `.env` 对 word、camelCase 标识符、超过 120 字符的句子和段落完成四次真实 DeepSeek Flash 请求；模型返回的 sourceText 在 serde 前由捕获输入覆盖，避免模型改写原文。

## 下一步

继续推进：阶段一基础桌面能力已经完成第一轮工程验证。当前最先要处理的阻塞：

- overlay 第一轮体验校准到此结束：拖动、当前进程内位置记忆、点击外部隐藏、`Ctrl+Alt+R` 重新呼出和真实查询链路已形成可继续开发的基础；现阶段不再继续调整默认窗口位置。
- 划词链路首批范围限定为已验证的 Obsidian 与 Codex App 渲染内容区；锚定结果窗口已按实际内容自适应，下一步继续验证其他桌面应用和不同 DPI/多显示器边缘位置。
- DeepSeek key 不再是当前本机阻塞；后续阶段二解释卡 MVP 可以复用现有 command，但不要把 SQLite schema 和上下文捕获混在 overlay 校准里。
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
- Obsidian 的 UIA provider 路径存在模式差异：编辑模式可直接从焦点 Edit 取得 TextPattern2，阅读模式需要从焦点/光标元素沿 Raw View 祖先链查找 Document。当前两种模式均可用，但仍需观察不同主题、页面结构和 Obsidian 版本下的稳定性。
- Codex App 的渲染内容区使用 `TextPattern`，常见来源为 `cursorPoint` 或滚动容器祖先；编辑输入区使用 ProseMirror，选区和坐标可用，但 Paragraph 上下文不完整。正式接线不能把这种退化上下文当成完整语境。
- 该能力不要求一开始支持所有应用，应先验证常用 Windows 桌面场景，例如 VS Code、Obsidian、Notion Desktop、WPS/Word、PDF 阅读器等。Electron 应用本质基于 Chromium，通常可能暴露无障碍树，但仍需逐应用验证。
- 后续技术路线建议按优先级评估：Windows UI Automation 读取前台窗口/焦点控件/选区和文本范围；高价值应用做专门适配器；OCR 仅作为更后期兜底；剪贴板查词只作为 fallback，不作为最终差异化。
- 仓库不提交真实 `.env`；本机已配置 DeepSeek key，但新环境仍需复制 `.env.example` 为 `.env` 后自行填写。
- Codex 沙箱内普通命令访问 `C:\Users\19150\.cargo` / `.rustup` Junction 可能报权限或 rustup home 创建误报；沙箱外同一用户验证正常，后续环境验证优先以沙箱外命令为准。
- C 盘空间已缓解但仍需留意；VS Build Tools 主体和 Windows Kits 仍在 C 盘，若空间再次吃紧，再评估卸载后重装 VS Build Tools 到 D 盘。
- 本地查询分类是启发式规则，缩写、多句但很短的文本、缺少句末标点的长句仍可能被相邻类型吸收；优先通过真实样本调整规则，不增加第二次 LLM 分类请求。
- 长段落输出受模型 JSON 稳定性和窗口最大高度约束；当前上限 4096 字符，不代表整页翻译能力。
- UI 具体设计暂时不定。
- SQLite schema 在解释卡和本地记忆阶段再设计。

## 暂时不要做

- 不做 OCR。
- 不做本地 LLM 运行时。
- 不做 macOS 支持。
- 不做浏览器插件。
- 不添加复杂任务管理文件夹。
- 不添加通用 Agent 框架。
