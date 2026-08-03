# ReadRay 交接记录

最后更新：2026-08-03

## TL;DR

- 当前状态：阶段一至阶段六已经完成；阶段七设置页的 Key 管理已通过上一轮审核，第一批确定性能力“官方余额、打开数据目录、SQLite 备份”已实现，正等待独立审核与真实 Tauri 人工验收。桌面生命周期和其余设置仍未实现，因此阶段七未完成。
- 当前路线：Windows 原生，Tauri + React + TypeScript + Rust + SQLite。
- 下一步：先独立审核并人工验收余额、目录打开和备份；使用量、字体、发送快捷键与桌面生命周期继续留给后续独立任务，不与本批混做。
- 后续顺序：阶段七完成后进入复习闭环；其后的阶段顺序以 `docs/DEVELOPMENT_PLAN.md` 为准。
- 当前约束：不使用通用 Agent 框架，不内置商业词典，不做 OCR、本地大模型或跨平台支持。
- 交接原则：`HANDOFF.md` 只记录会影响下一次恢复上下文的信息，小型文档措辞和格式调整不记录。

## 当前阶段入口

完整文件职责和按任务检索入口以 `docs/RESOURCE_MAP.yml` 为准；这里仅保留恢复阶段七时最先需要的入口，避免维护第二份资源地图。

- `AGENTS.md`：协作规则；开始任务时先读。
- `docs/DEVELOPMENT_PLAN.md`：项目方向、阶段边界和验收标准的权威来源。
- `docs/RESOURCE_MAP.yml`：完整资源索引；未在本节列出的文件从这里查找。
- `docs/WINDOWS_ENVIRONMENT.md`：本机 pnpm、Rust、Tauri、构建和发布命令基线。
- `src/App.tsx` / `src/components/MainAppShell.tsx` / `src/components/MainSidebar.tsx`：设置入口、主窗口装配和页面导航边界。
- `src-tauri/src/lib.rs` / `src-tauri/tauri.conf.json`：主窗口与 overlay 的命令、快捷键、关闭/隐藏和生命周期入口。
- `.env.example` / `src-tauri/src/deepseek_client.rs` / `src-tauri/src/secret_store.rs`：DeepSeek 开发环境回退、共享请求和 Windows 安全存储边界；不得把真实密钥写入仓库、SQLite、前端持久化或普通日志。
- `package.json` / `src-tauri/Cargo.toml` / `src-tauri/capabilities/default.json`：前端、Rust 插件和最小权限装配；新增设置能力前先确认是否能复用现有依赖。

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

## 已完成能力与关键经验

这一节保留已经形成长期价值的能力、实现约束和验收经验；阶段过程中的临时状态、重复命令和已经被后续实现取代的描述不再逐条保留。

### 基础环境与项目恢复

- `AGENTS.md`、`DEVELOPMENT_PLAN.md`、`RESOURCE_MAP.yml` 和本交接记录已经形成项目恢复基线；项目方向与阶段范围不再依赖聊天历史重建。
- Tauri + React + TypeScript 脚手架、pnpm 依赖和 Rust stable MSVC toolchain 已可用；Visual Studio 2022 Build Tools、Windows 11 SDK、WebView2、`link.exe` 与 `pnpm tauri dev` 均完成过真实验证。
- 2026-06-22 已将 Cargo、rustup 和 Visual Studio package cache 通过 Junction/CachePath 迁到 D 盘并完成构建验证；路径、环境异常和磁盘策略的细节统一由 `docs/WINDOWS_ENVIRONMENT.md` 维护。
- Tauri 使用官方 `global-shortcut`、`clipboard-manager` 插件；SQLite 统一由 Rust `rusqlite` bundled 负责，不建立前端 SQL 路径。
- Rust 通过 `dotenvy` 加载项目根目录 `.env`；真实 key 不提交，默认模型为 `deepseek-v4-flash`，可用 `DEEPSEEK_MODEL` 覆盖。真实 DeepSeek smoke test 与窗口、快捷键、剪贴板、SQLite 基础能力均完成过人工验证。
- 大赛官网资源、附件和文本抽取已恢复到 `resource/`，Git 仓库已修复，`src-tauri/target` 已忽略。

### Overlay、解释卡与 UIA

- compact UI 从静态 mock 演进为两条正式交互：有选区时显示贴近 `anchorRect` 的 `AnchoredResultPopover`，无选区时由 `CenteredCommandInput` 进入真实解释结果；早期大背景预览仅是定位交互边界的开发阶段，不是当前产品壳。
- `ExplanationCard` 是 `word` / `phrase` / `sentence` / `paragraph` 四类 serde tagged enum；Rust validator 按类型检查必填、长度、数组、双语例句和上下文约束，`create_explanation_card` 对请求、HTTP、响应结构、JSON 和 validator 错误分别诊断。
- overlay 已形成独立桌面窗口：无边框、透明、置顶、跳过任务栏，输入/loading/error/result 使用不同窗口尺寸；默认位于屏幕偏上区域，用户拖动位置在当前进程内优先恢复，失焦或 Esc 隐藏。该位置方案已经接受，不再重复校准。
- Windows UIA 捕获优先 `TextPattern2`、回退 `TextPattern`，返回选区、段落上下文和物理屏幕坐标；捕获必须发生在 ReadRay show/focus 之前，锚定窗口按显示器 DPI 和工作区放置。
- Obsidian 1.12.7 的编辑与阅读模式已真实验证；阅读模式的关键经验是沿 Raw View 祖先链找到 Document 的 `TextPattern2`，不能只检查焦点元素。成功链路未依赖 MSAA/IAccessible2 或剪贴板辅助。
- Codex App 渲染内容区可通过 `TextPattern` 取得选区和上下文；ProseMirror 编辑区虽能取得选区和坐标，但 Paragraph 上下文可能退化为选中文本并混入对象替换字符，正式链路必须清理 U+200B/U+FFFC，并把退化上下文降级为 `null`。
- DeepSeek Flash 按本地 `queryType` 返回严格 JSON；句子/段落以完整翻译为第一信息，模型返回的 `sourceText` 在 serde 前由捕获输入覆盖，避免模型改写原文。锚定结果按内容和类型自适应宽高，超出工作区时内部滚动。
- 不联网 Rust 测试和 word、camelCase、长句、段落四类真实 DeepSeek 请求均完成过验证；Codex App 与 Obsidian 阅读区也分别取得过真实锚定解释卡。

### 本地数据、主应用与 Quick AI

- `learning_records` 使用 `rusqlite` bundled 和 `schema_migrations`；v1 把每次成功解释保存为不可覆盖的独立事件，保留原文、标准化文本、类型、来源、可选上下文、完整 ExplanationCard JSON、schemaVersion 和时间。未定义的 difficulty 保存为 `NULL`。
- `create_explanation_card` 先完成 DeepSeek 解析和 validator，再写学习记录；请求、解析、校验或存储任一失败都不留下记录。manual 与 windows_uia 共用同一正式链路。
- 学习记录 commands 已覆盖分页、搜索、单条、删除和只读今日摘要；“记忆”“今天”分别经 repository/service 读取真实数据，页面不接触 SQL。正式 Tauri 路径不读取 fixture，overlay 保存成功会通知已打开页面刷新。
- “今天”只陈述当天真实查询数量、最近记录、类型、来源和时间，不生成当前 schema 无法证明的复习数量、高频词或趋势；“过去的出现”同样保持隐藏。
- Quick AI 使用独立普通 chat/completions 请求，不复用 ExplanationCard JSON；两者只共享 `.env`、模型/API key 和 HTTP 错误边界。`Ctrl+Alt+R` overlay 支持解释与 Quick AI 切换、新对话、多轮发送和隐藏，真实 DeepSeek 两轮上下文已验证。
- SQLite v2 的 `quick_ai_conversations` / `quick_ai_messages` 与 `learning_records` 分表，消息保存 role、content、sequence 和时间，为完整对话和后续管理提供权威数据。
- `main` 与 `overlay` 是独立窗口；overlay 的呼出意图先由 Rust 原子保存，再由前端在事件、获焦或挂载时领取，避免隐藏 WebView 漏事件或沿用错误模式。只有 overlay 失焦自动隐藏。
- 主应用侧栏只允许用户手动在 252/72px 间折叠；“今天”“记忆”“写作”“对话”共享同一外壳。最近对话标题来自真实 SQLite，空标题排除，溢出时才渐隐。
- 主应用统一以 1440×900 / scale 1 为设计基线，最小窗口 840×600；标题栏已接原生拖动、最小化、最大化/恢复和隐藏。本机 150% DPI 真实 Tauri WebView 已完成尺寸与字体验收。
- 应用随包内嵌 Geist、Geist Mono、Newsreader、思源黑体和思源宋体及对应 OFL，正式 UI 不依赖联网或本机字体；浏览器预览按完整设计画布整体缩放，响应式断点由应用容器触发。

### 阶段五：写作正式接线

- 已按 `design-open-design/readray-writing-2.html` 在现有 MainAppShell 中实现“写作”页：写作导航和“今天”页写作入口进入本地文章库，支持空白稿/已有稿、标题与正文编辑、文档切换、选区菜单、“问 ReadRay”多轮追问、真实写作教练问题、定位/修改/进一步提示/参考/忽略、多轮检查、双栏文本差异、随完成版本保存的“本次写作要点”、完成版本和继续修改；要点明确未加入复习，也没有全局模式库。
- 写作编辑区沿用 1440×900 / scale 1：纸张宽 680px、正文 18px / 1.68 行高、编辑列上限 736px；草稿/完成稿未打开辅助栏时纸张视觉居中，检查或辅助打开后自然重排。900px 以下教练保持 320px 右侧覆盖层，正文列宽不变；全局侧栏仍只允许用户手动在 252/72px 间折叠。
- 阶段五已追加 SQLite v3 migration：`writing_documents` 保存文章、当前草稿、完成稿、revision 和对比基线；`writing_versions` 保存不可变完成版本；`writing_analyses` 保存通过 schema/validator 的整篇检查；`writing_assistant_answers` 保存选区问答与追问。返修继续追加 v4，保存对比基线/分析 revision 元数据和回答目标 versionId；v3 旧数据无法可靠证明的基线 revision 与版本 analysis revision 均保持 `null`，不从 source revision 伪造来源。写作数据不写入 `learning_records` 或 Quick AI 表。
- 正式 Tauri 写作路径使用 `TauriWritingRepository` / `RepositoryWritingService` 和 `src-tauri/src/writing.rs`；页面组件不直接 invoke。非 Tauri 预览通过动态 import 单独加载 `writingFixtureService.ts`，正式模块已静态检查不含 localStorage、演示文章、硬编码问题或演示回答。
- 草稿自动保存使用每篇文章独立 revision 和已落盘快照的防抖串行协调器；调用失败后先读回文章对账：数据库已推进且目标快照一致则确认成功，仍为旧 revision 才允许安全重试。分析先提交且权威结果证明正文仍为送检基线时，在途 pending 会自动基于新 revision 重试；权威正文不同仍保留当前正文并阻止覆盖。切换文章和返回文章库前先 flush；应用卸载时仍会尝试落盘且不回写已卸载组件。
- 检查和问答均先确认正文已保存，再记录 documentId、数据库 revision、屏幕可见快照、可选完成版本 ID 和本地编辑 generation 后调用 DeepSeek。模型 JSON 先经 Rust serde schema 和 validator；正文、文章或版本在请求期间变化时，后端 revision 与前端可见身份会分别拒绝旧结果。当前草稿的辅导会话按文章和最近完成版本边界保持，自动保存推进 revision 后已接受回答及 parentAnswerId 仍可连续追问；前端按时间持续展开完整问答，不再折叠成“之前的问题”，Rust 沿 parentAnswerId 向模型提供最近 8 条同一可见身份问答。新一次完成会切断旧草稿回答，历史版本问答继续按 versionId 严格隔离。提问框按内容自动增高和回缩，常规多行不显示原生滚动条，极长输入达到安全上限后保留无可见滚动条的内部滚动。写作页 hidden 时不再接管其他页面的 Ctrl+J/Escape。
- 分析保存会在事务中推进文章 revision，`activeAnalysis` 只读取与当前草稿 revision 相同的结果；另以 `baselineAnalysis` 保留 comparisonBaselineRevision 绑定的本轮检查，因此检查后编辑和重启不会把它冒充当前分析，也不会丢失问题、模式和差异基线。完成与分析写事务串行化，同一 expectedRevision 只能一方提交；不可变版本固化当前 source revision，以及同一基线绑定的 analysis/baseline revision、问题和模式，检查后编辑时 source 可晚于二者。legacy 基线 revision 为 null 时，完成操作只接受与 expectedRevision 精确匹配的分析并保存其 analysisRevision，未知基线继续为 null。历史版本切换会重建只读“基线问题/处理回顾”、清理辅助面板，不把基线问题标为当前待处理项或在完成稿中定位；问答携带 versionId 和同目标请求序号，使用屏幕所见版本正文并拒绝乱序旧结果。
- 已有修改中草稿时，Rust `continue_editing` 默认拒绝用完成版本覆盖；前端明确提示并提供“回到草稿”。文章搜索分别检查 draft/completed 标题与正文；分析 validator 拒绝正文中不存在的 source、与 targetText 无法验证的原文，以及当前 UI 无法定位的标题目标。
- 阶段五本轮返修自动验证已通过：写作前端 25 项、既有完整对话前端 9 项；完整 Rust 测试 62 项通过、2 项真实 DeepSeek 联网测试按既有 ignored 标记跳过；`pnpm build`、`cargo fmt --check`、`cargo check` 均通过。覆盖 v2→v3→v4 升级及旧 analysis revision 保持 null、CRUD/双字段搜索、重启恢复、继续修改防覆盖、不可变版本、当前/基线分析身份、检查后编辑再完成、历史版本问题/回答重建、自动保存推进 revision 后连续追问、新完成版本边界隔离、完整问答顺序/去重、最近 8 条 parentAnswerId 上下文、辅导 transcript 与输入框自动增高、同目标问答乱序、结构化 source 校验、自动保存模糊提交对账/分析 revision 组合竞态、迟到分析/回答和 hidden 快捷键。
- 阶段五已经由用户在真实 Tauri 窗口完成人工验收；正式 SQLite/DeepSeek 写作链路与既有视觉、连续辅导和“本次写作要点”布局均已确认，不再作为进入阶段六的阻断项。

### 阶段四能力：完整对话正式接线（阶段六基础）

- 已按 `design-open-design/readray-conversation-2.html` 在现有 MainAppShell 中实现完整对话页：保留 1440×900 外壳、736px 消息列和输入区，覆盖空对话、设计示例消息、长提示折叠、生成/停止/失败/重试、更多菜单、导出提示和记忆引用抽屉。
- 今天页输入、新对话和最近对话均已进入完整对话页；正式 Tauri 装配通过 `TauriConversationRepository` 调用现有 create/get/send Quick AI commands，页面组件不直接 invoke。
- `RepositoryConversationService` 将 Rust camelCase `ConversationSnapshot` 映射为现有 thread，以 SQLite 返回的真实消息 ID、标题、时间和 sequence 覆盖临时页面状态；尾部为 user 时映射为明确 pendingTurn，重启加载后页面直接进入可重试失败态。
- Tauri 正式路径不静态读取或实例化 `FixtureConversationService`；fixture 被 Vite 拆为非 Tauri 预览动态 chunk，继续保留原有分片、停止/继续、重生成、导出和故障注入演示。
- 阶段四收口时真实 Quick AI 是非流式完整响应，正式路径不显示可用的停止/继续，也未开放重生成、原生导出或记忆引用；阶段六现已补上原生导出，但停止、重生成和记忆引用仍保持禁用。
- Quick AI 发送使用 `conversationId + expectedUserSequence` 作为稳定轮次身份：`prepare_turn` 先单独提交 user，再调用 DeepSeek；`complete_turn` 校验 message ID、sequence 和当前尾版本后只补一条 assistant。模型、进程或 assistant 保存失败都会留下可恢复 pending user。
- 同一轮重试复用既有 user message ID/sequence；若 assistant 已存在，后端直接返回权威快照且不再次请求模型。若事务已提交但 IPC 回传和随后读取都失败，页面仍保留原 expected sequence，下一次重试同样由后端幂等识别，不依赖 prompt 文本猜测。
- 本轮未追加 migration：现有 v2 `quick_ai_messages` 的自增 ID 和 `UNIQUE(conversation_id, sequence)` 已足够提供 pending 身份与 expected-version 约束；旧历史、并发不同内容或错误 message ID 会明确冲突，不会静默写入。
- 完成态 fixture 导出会生成用户可下载的 Markdown 文件，并按顺序包含 thread 的全部 user/assistant 消息；空结果或异常不会触发下载或成功提示。
- fixture 通过 `conversationFailure=create|load|generate|export` 显式注入一次性失败，正常重新生成不再强制失败；`[fixture:slow]` 只用于停止/继续演示。记忆抽屉关闭时会移除内部焦点并恢复到原引用按钮。
- 完整对话页此前完成的是 fixture 路径的视觉与交互人工验收；本机 pnpm 构建与 Headless Playwright 回归均通过，覆盖生成中重复发送、连续两轮、生成中导出、完整 Markdown 下载、导出失败重试、停止/继续、重新生成和抽屉焦点。1440×900、840×600 与两档侧栏无页面级横纵溢出或重叠；`preview=responsive` 只用于浏览器真实容器验收，不改变默认等比预览和 Tauri。阶段四收口时已另行完成真实 Tauri/SQLite/DeepSeek 功能验收。
- 阶段四返修自动验证：前端 repository/service 共 9 项测试通过；Rust 40 项通过、2 项需真实网络的 live test 按既有标记忽略。覆盖模型失败后重开仍有 pending user、重启重试只补一条 assistant、已提交但调用方未确认时不重复、assistant 保存失败保留 pending、旧版本/并发冲突拒绝写入，以及 41/43 条消息长对话截断后仍从 user 开始。
- 阶段四真实 Tauri 功能验收已经完成：真实创建、最近对话加载、多轮续聊和侧栏会话身份均可用；用户接受当前对话体验作为后续优化项，不再阻塞阶段五。

### 阶段六：会话管理闭环（已完成）

- “查看全部对话”已接入 `list_all_quick_ai_conversations`，直接读取 SQLite 中全部有标题会话并按 `updated_at_unix_ms DESC, id DESC` 排列；独立页面覆盖 loading、empty、error 和 retry，不从侧栏最近六条推导历史。
- 重命名和删除复用现有 `ConversationService` / `ConversationStore`，所有操作只提交数据库 conversation ID。侧栏最近项与全部历史均为左键打开、右键显示“重命名/导出/删除”，列表不保留常驻文字按钮或悬停更多按钮；两处共用 Shell 级管理浮层，不复制业务请求。当前会话标题单击后原地编辑，Enter 保存，Esc 或失焦安全取消。重命名成功后用 Rust 返回的完整权威快照同步当前 thread；删除依赖既有外键级联清理消息，删除当前会话后进入新的空会话。成功操作触发既有刷新令牌，使侧栏与全部历史重新读取。
- 当前页的管理操作同时绑定 mounted、request key 与 conversation ID；父级再用实时页面、请求和会话 ref 复核删除回调。在操作期间切换到其他会话或“今天/记忆/写作/全部对话”后，迟到结果不会回写已卸载页面，也不会创建新对话或把用户带回会话页。重命名、删除失败保留当前对话和弹窗状态，可原地重试；不存在的删除不会提示成功。
- 原生导出使用官方 Tauri 2 dialog 插件让用户选择 Markdown 路径并提供明确取消语义；取消时不调用 Rust 导出 command，也不显示成功。用户确认路径后，Rust 按 conversation ID 重新读取 SQLite 权威快照并按 sequence 写出完整 user/assistant 消息，不使用前端 messages 重建文件。空白会话在菜单和 service 入口两层禁用，不会打开保存对话框；无效结果和写文件失败均不修改会话。
- 新增依赖仅为 `@tauri-apps/plugin-dialog` / `tauri-plugin-dialog` 及其 capability `dialog:allow-save`；替代方案是固定写入下载目录，但无法覆盖用户选择路径和取消验收。SQLite、DeepSeek 客户端和会话表均未新增第二套实现，也未追加 migration。
- 正式 Tauri 会话路径的静态测试确认不读取 fixture/localStorage；非 Tauri 预览仍只通过动态 import 加载 `conversationFixtureService.ts`，继续与正式路径隔离。
- 阶段六自动验证已通过：会话前端 18 项、写作前端回归 25 项、完整 Rust 65 项通过且 2 项真实 DeepSeek 联网测试按既有 ignored 标记跳过；`pnpm build`、`cargo fmt --check`、`cargo check`、RESOURCE_MAP YAML 解析和 `git diff --check` 均通过。没有使用浏览器或 Computer Use，也没有启动 Tauri 窗口替代人工验收。
- 用户已在真实 Tauri 主窗口完成人工验收：全部历史、侧栏与历史页左键打开/右键管理、当前标题原地重命名、删除确认、原生导出及列表同步符合预期；阶段六已正式收口。

### 阶段七：设置页面（待独立审核与人工验收）

- 已按 `design-open-design/readray-settings.html` 把设置入口接入现有 `MainAppShell` / `MainSidebar`，没有复制外壳。视觉返修后保留通用、外观、AI 服务、数据、关于五类导航，以及设计稿的 192px 分类栏、820px 内容列、卡片层级和页面密度。Key 配置/清除、官方余额、SQLite 概览、打开数据目录、SQLite 备份和随包字体许可证弹窗现已形成真实闭环；当前主题、字体、语言和固定快捷键只读展示，使用量及桌面生命周期操作继续诚实禁用。
- 正式前端路径为 `SettingsPage -> SettingsService -> SettingsRepository -> Rust commands`。页面覆盖 loading、读取失败/重试、Key 首次配置、真实验证、成功保存、更新时保留旧配置、失败原地重试、清除确认/失败重试和卸载后迟到结果拒绝；组件不直接 invoke，也不读取 localStorage/sessionStorage。
- `src-tauri/src/secret_store.rs` 使用 Windows Credential Manager 的 generic credential 持久化 DeepSeek Key；前端快照只返回 configured/source，不返回明文或尾号。清除时先写非敏感禁用标记，再删除已保存 Key，因此开发机 `.env` 不会在清除后偷偷重新生效；保存的新 Key 优先于标记，失败清除不会误报为已停用。
- 候选 Key 先通过共享 `deepseek_client` 向当前 ReadRay 模型执行真实 `chat/completions` 验证，成功后才写入凭据管理器；失败不会替换现有配置。正常解释、Quick AI 和写作请求已统一改为优先读取安全存储，只有尚未形成保存/清除决定时才兼容开发期 `DEEPSEEK_API_KEY`。
- API Key 状态一致性返修已把保存/清除命令统一为“先读取完整非敏感快照，再执行凭据变更，成功后仅在内存更新 Key configured/source”的顺序。这样 SQLite 概览失败会发生在凭据变更前，不会出现凭据已经成功但前端收到失败；凭据操作失败仍返回错误并保留原配置。
- 余额 command 复用安全存储和 `deepseek_client` 访问 DeepSeek 官方 `GET /user/balance`；Rust 严格校验 `is_available`、唯一三字符币种和三个非负十进制金额字段，支持同时返回多币种。页面只在内存显示结果，覆盖 loading、失败、重试和手动刷新，不保存余额或返回 Key。
- 设置快照从 Tauri 运行时返回实际模型、应用版本和数据目录，并直接从 SQLite 读取学习记录、Quick AI 会话、写作文档计数；没有从前端列表推算。“打开数据目录”无前端路径参数，Rust 重新解析真实 `app_data_dir` 后调用既有 opener，失败会留在页面明确重试。
- SQLite 备份复用既有原生保存对话框；取消时不调用 Rust、无文件且无成功提示。确认路径后，Rust 在阻塞线程对权威 `readray.sqlite3` 执行 `VACUUM INTO` 同目录临时文件，完成 `PRAGMA quick_check` 后才替换目标；失败清理临时数据库及 journal/WAL/SHM，不修改源库。快照覆盖学习记录、对话、写作及 SQLite 内非敏感设置，不包含 Windows 凭据管理器中的 API Key；恢复、清空和全量结构化导出仍未实现。
- 设置页响应式规则继续以应用容器为准：1440×900 使用 192px 分类栏；840×600 时分类改为顶部横向，900px 以下表单和数据行纵向排列；全局侧栏仍只按用户操作在 252/72px 间切换。
- 本轮自动验证通过：设置前端 6 项、会话前端回归 18 项、写作前端回归 25 项；设置聚焦 Rust 13 项及完整 Rust 81 项通过，2 项真实 DeepSeek 联网测试按既有 ignored 标记跳过；`pnpm build`、`cargo fmt --check`、`cargo check` 均通过。没有使用浏览器、Computer Use 或 Tauri 窗口替代人工验收。
- 尚未完成：Windows 凭据管理器真实写入/更新/清除、Key 失败后旧配置保留、重启恢复、AI 三条正式链路切换，以及 1440×900 / 840×600 与两档侧栏的真实 Tauri 人工验收；这些应在独立审核后由用户执行。阶段七不得因设置页代码完成而标记完成。

## 下一步

阶段六已经完成，阶段七设置页第一批确定性能力正等待独立审核和真实 Tauri 人工验收；阶段七状态不变，桌面生命周期及其余设置留给后续独立任务。

- 本批独立审核应重点检查余额响应严格映射且不持久化、目录 command 不接受前端任意路径、保存对话框取消语义，以及临时快照校验/替换/清理顺序；同时继续确认 Key 不进入前端返回、日志或 SQLite。
- 用户人工验收在真实 Tauri 中覆盖 CNY/USD 或实际返回币种、余额失败重试、Explorer 打开真实目录、备份取消/成功/覆盖已有备份/不可写路径失败，以及生成文件可由 SQLite 打开并包含三类数据；人工验收前不要把本批写成已验收。
- 后续桌面生命周期任务在现有主窗口与 overlay 边界上独立实现快捷键管理、开机启动、关闭/隐藏、托盘退出、单实例和重新打开主窗口；不得因设置页已有说明文字而视为接线。
- 新环境仍可复制 `.env.example` 为 `.env` 填写开发期 `DEEPSEEK_API_KEY`；用户在设置页保存或清除后，以 Windows 安全存储中的持久化决定为准。真实 `.env` 不提交。

## 当前已知限制与后续边界

以下内容是已经确认的能力边界或后续阶段入口，不代表当前阶段都应立即解决。除非 `DEVELOPMENT_PLAN.md` 或用户明确调整范围，Agent 不应因看到这些条目而顺手扩项。

- **长期数据原则**：`learning_records` 是追加式原始事件。未来若增加重复聚合、复盘状态或时间线，应新建独立表并追加 migration，不回填或覆盖原始事件。
- **长期 UIA 观察项**：Obsidian 编辑模式可从焦点 Edit 读取 `TextPattern2`，阅读模式需沿 Raw View 祖先链查找 Document；两者当前可用，但不同主题、页面结构和版本仍需逐步验证。
- **长期 UIA 观察项**：Codex App 渲染区通常使用 `TextPattern`；ProseMirror 编辑区的选区和坐标可用，但 Paragraph 上下文不完整，不能把退化结果当成完整语境。
- **跨应用扩展边界**：不要求一次支持所有 Windows 应用。先逐个验证 VS Code、Obsidian、Notion Desktop、WPS/Word、PDF 阅读器等高价值场景；继续以 UIA 为主，高价值应用再做专门适配，剪贴板仅作 fallback，OCR 仍不在当前路线内。
- **解释体验限制**：查询类型依赖本地启发式规则，缩写、很短的多句文本或缺少句末标点的长句可能落入相邻类型；优先用真实样本调整，不为分类再增加一次 LLM 请求。
- **解释体验限制**：CaptureInput 和 ExplanationCard 当前上限为 4096 字符，长段落还受模型 JSON 稳定性与浮窗最大高度约束，不代表整页翻译能力。
- **阶段八边界**：记忆页还不能可靠生成重复出现次数或时间线，“过去的出现”入口保持隐藏；复习、去重和长期记忆留到对应阶段统一设计。
- **后续体验优化**：Quick AI 当前按纯文本、非流式展示，模型仍可能返回 Markdown 标记；Markdown 渲染/规范化、真正流式输出和更可靠的对话策略不自动并入阶段六。
- **对话后续边界**：阶段六的查看全部、重命名、删除和原生导出已经完成；回答重生成和记忆引用聚合属于更后续能力，当前 UI 继续诚实禁用。
- **阶段七范围**：Key 管理及第一批余额、目录、备份已实现但本批尚待独立审核与真实窗口验收；使用量、字体、发送快捷键未接线。主窗口关闭后仍只隐藏并保留后台快捷键，托盘、单实例、开机启动、关闭策略和重新显示主窗口入口尚未形成完整桌面生命周期，留给后续独立任务。

## 暂时不要做

- 不做 OCR。
- 不做本地 LLM 运行时。
- 不做 macOS 支持。
- 不做浏览器插件。
- 不添加复杂任务管理文件夹。
- 不添加通用 Agent 框架。
