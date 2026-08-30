# ReadRay 交接记录

最后更新：2026-08-30

## TL;DR

- 解释卡片原地固定多窗口（2026-08-30，自动验证完成，真实 Tauri 待用户复验）：划词结果卡右上角提供无文字 pin，点击后当前可见 overlay 直接原地晋升为固定卡；不复制 ExplanationCard、不切换 WebView、不重载字体、不重新测量，也不隐藏、缩放或替换当前窗口，因此除 pin 状态外，卡片内容、尺寸和样式应保持不变。Rust 仅在后台异步创建隐藏的 `overlay-query-N` 作为下一次查询窗口，并把全局快捷键、UIA 准备和 overlay intent 路由到新的活动 label；原窗口进入固定卡注册表后忽略失焦隐藏，可继续拖动，点击 pin、聚焦后按 Esc 或双击卡片任意位置均可关闭。点击 pin 到后台窗口建成期间，原窗口先进入固定注册表，避免 WebView2 动态建窗导致的焦点变化把它隐藏；建窗失败则回滚注册状态。若快捷键由固定卡触发，Rust 会先恢复该卡记录的阅读源窗口再做 UIA 捕获。每次 pin 不重新请求模型、不写缓存或学习记录，最多同时固定 8 张，且不跨应用重启恢复。`pnpm test:overlay`、`pnpm build`、`cargo check --locked`、pinned_cards Rust 聚焦测试、fmt、YAML 与 diff 检查通过；由于 Rust 窗口生命周期已改变，真实复验前必须彻底结束旧进程再重启。

- 划词卡动态效果开关（2026-08-30，自动验证完成，真实 Tauri 待用户验收）：通用设置的“划词解释”分组只提供“减少动态效果”开关，不显示模式名称或说明小字；默认关闭，查询超过 100ms 时先显示 430×92 的紧凑加载卡片（选中词、`正在生成语境解释…`、呼吸状态点），缓存快速命中会跳过。开启后才使用低动态行为：Rust 以 alpha=0、鼠标穿透且不抢焦点的准备窗口唤醒隐藏 WebView2，ExplanationCard 完成渲染和测量后以最终尺寸一次显示。SQLite v23 在 `app_preferences` 增加 `selection_explanation_display_mode` 权威字段且默认 standard；加载卡不抢焦点、不接收鼠标，最终结果复用现有完整卡片与尺寸测量链路。失败、requestKey、取消、精确缓存、single-flight、学习记录与解释内容不变。`pnpm test:settings`、`pnpm test:overlay`、`pnpm build`、Rust settings、v23 升级测试、新库迁移测试、`cargo fmt --check` 与 `git diff --check` 通过；需用户在真实 Tauri 中切换开关，分别验证缓存命中/未命中、首次查询从加载卡切换到完整卡、双击 Alt 响应、焦点与点击穿透。

- 今天页记录刷新修复（2026-08-28，自动验证完成，真实 Tauri 待用户复验）：本机数据库只读核对确认当天实际存在 16 条 `learning_records`，且 16 条均有目标投影和 occurrence，日期边界与后端统计 SQL 无误；根因是主壳长期保留今天页首次读取的快照，只依赖可能在主窗口隐藏或监听尚未建立时错过的跨窗口事件。现在每次重新进入“今天”都会读取本机最新摘要，主窗口从 overlay/其他应用恢复焦点且当前位于今天页时也会刷新，同时继续保留 `readray://learning-record-created` 的即时刷新。聚焦消费者测试 6/6、设置聚合测试 60/60、复习测试 51/51、`pnpm build` 与 `git diff --check` 通过；需在真实 Tauri 中查询一条记录后返回今天页，确认数量和最近记录立即更新。
- 复习页面精修（2026-08-30，自动验证完成，真实 Tauri 视觉与轮换手感待用户验收）：复习首页改为无需翻页的稳定活跃书架，显示 6/4/2 张（3×2、2×2、1×2）；三列、两列和一列均以 `300×240` CSS 像素作为同一单卡基准，两列不再放大卡片，只增加书架外围留白，极窄窗口仍可由容器约束收缩。CSS 的 `--rr-review-column-count` 同时驱动网格列数和前台容量，修复侧栏状态下“四张卡片只占三列前两列”以及两种窗口间卡片尺寸大幅变化的问题；两列/一列书架、标题和完成统计整体居中。已显示卡片不会因后台制卡或加载换位，只有用户成功提交“想起来了/没想起来”并返回书架后，下一张未完成卡才补入原槽位。游标 Feed、后台制卡和真实调度顺序保持不变，pending 条目仍不发布。首页移除分页、常驻状态、阶段说明、装饰左竖线、纸张噪点和上浮 hover，单条语境开头孤立的 `•/●/◦/▪/▫/‣/⁃` 仅在展示层隐藏；专注页使用自然内容高度和受控最大高度，缩窄阅读行宽，移除重复关闭按钮并弱化结果按钮。既有完整语境、解释、结果/撤销、来源和质量反馈链路不变。`pnpm test:review`、`pnpm build` 已通过；未使用浏览器或 Computer Use，需用户在真实 Tauri 中确认三档书架单卡尺寸和正文换行稳定、完成后原槽补卡以及深浅主题观感。
- 应用内更新通道（2026-08-28，代码/文档/签名构建完成，真实更新流程待用户人工验收）：ReadRay 从 0.1.1 起接入官方 `tauri-plugin-updater`（Rust 注册于 `lib.rs`，capability 仅 `updater:default`，未引入 plugin-process——Windows 上由插件 `std::process::exit(0)` 结束进程，NSIS passive `/UPDATE /R` 静默安装并自动重启，`install` 之后的代码通常不执行）。检查端点为 GitHub Release 静态 `latest.json`（`https://github.com/CLAY-bug/ReadRay/releases/latest/download/latest.json`），更新包经 minisign 签名校验，公钥固化在 `tauri.conf.json`。签名密钥对在仓库外 `C:\Users\19150\.tauri\readray-updater.key`（空密码），**必须离线备份：私钥丢失后已安装用户永久收不到应用内更新**；在首个带 updater 的版本发布前可免费换钥，发布后换钥需用户再手动装一次。前端链路 `src/appUpdateRepository.ts`（updater JS API 薄封装，测试可注入 fake）→ `src/appUpdateService.ts`（单例状态机 idle/checking/upToDate/available/downloading/installing/failed；下载 progress 200ms 节流；`applyUpdate` 顺序为下载 → `desktopSaveCoordinator.flushAll()`（与安全退出共用落盘链路，flush 失败拒绝安装、可重试且复用已下载内容）→ install；启动静默检查失败回 idle，手动检查失败映射签名/网络等友好文案；失败为瞬态反馈，更新行卸载（切走页面/标签）即经 `dismissTransientFailure()` 回落 idle，available 与已下载内容保留）→ `src/useAppUpdateState.ts`（`useSyncExternalStore` hook）。UI：设置页关于区"更新"行从静态占位改为真实检查/下载/安装交互，与"许可证与第三方材料"保持同一行式布局（状态用 is-success/is-warn/is-danger 小胶囊 + muted 小字详情表达，不用大块红色错误文本）；主窗口启动 12s 后静默检查一次；发现新版本时侧栏设置入口显示红点。验证：`pnpm test:update` 14/14、`pnpm test:settings` 60/60、带签名 release 构建产出 `ReadRay_0.1.1_x64-setup.exe` + 同名 `.sig`。已知坑（已写入 WINDOWS_ENVIRONMENT.md）：签名变量名是 `TAURI_SIGNING_PRIVATE_KEY`（值=私钥路径或内容），`TAURI_SIGNING_PRIVATE_KEY_PATH` 不存在；空密码也必须显式 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""`。发版步骤（抬三处版本号 → 签名构建 → GitHub Release 上传 exe + .sig + latest.json 三资产 → latest.json 模板）见 `docs/WINDOWS_ENVIRONMENT.md`。待用户验收：安装 0.1.1 后发布下一个版本走一遍真实应用内更新；RC1 0.1.0 用户仍需手动下载一次 0.1.1 才进入更新通道。

- RC1 用户反馈修复轮（2026-08-27，自动验证已通过，待真实 Tauri 人工验收）：针对首批朋友试用反馈完成三项修复和一项新能力。① 划词解释卡错误友好化：`create_explanation_card` 出口统一映射为可操作的友好文案（未配置/无效 Key、余额不足、超时、网络、截断等），技术细节进 `READRAY_EXPLANATION_FAILED` stderr 日志，取消哨兵保持原样；② sourceSentence 部分成功降级：validator 错误仅涉及 sourceSentence/sourceSentenceZh 时在副本上清空这对字段重校验，成功即保留卡片主体并记 `READRAY_EXPLANATION_SOURCE_SENTENCE_DROPPED`，其他 schema 错误仍整卡失败；③ "中文主导"判定放宽为"含汉字即中文主导"（根因：`GLM Coding 7天体验卡` 这类中英混排句在旧 han×2≥latin 规则下被判英文主导，模型按 prompt 不给中译而 validator 整卡拒绝），Rust/前端 `sourceSentenceDisplay.js`/word+phrase prompt 三处同步，纯拉丁原句仍必须附中译；④ 划词锚定浮层支持拖动：App 在 anchored 非 mock 态渲染顶部透明热区，新增 `begin/drag/finish_anchored_window_drag` 复用 overlay 拖动状态但**不写 overlay 位置缓存**（拖动只对本次生效，下次划词重新锚定），拖动后跳过后续 resize 防止拉回锚定位置。⑤ 输入补全：`scripts/gen-vocabulary.mjs` 从 FrequencyWords（MIT，OpenSubtitles 语料）生成 48k 词频表 `vocabulary_data.txt` 随包嵌入（README 已标注归属），新增 `vocabulary.rs`（词频序前缀匹配 + ≥4 字符时 band 剪枝编辑距离 ≤2 纠错）与 `suggest_vocabulary_terms_command`（learning_targets 小写前缀命中按最近优先置顶）；`CenteredCommandInput` 输入 ≥2 拉丁字符防抖 200ms 显示最多 5 条候选，↑↓ 选择、Enter 提交高亮候选（无高亮提交原文）、Esc 分层关闭，候选显示时经 `set_overlay_input_window_height` 扩展输入窗口、关闭恢复。验证：Rust lib 426 通过/6 ignored、`cargo fmt --check`、`pnpm test:overlay` 22/22、`pnpm build`、RESOURCE_MAP YAML 与 `git diff --check` 均通过。待真实 Tauri 人工验收：划词混排样本（GLM Coding / CLAY-bug 类）出卡、拖动手感与下次重新锚定、补全候选手感与扩窗动画、历史词置顶。
- RC1 设置与快捷键体验返修（2026-08-27）：补全候选视觉已从独立滚动列表收敛为紧贴输入框向下展开的抽屉，最多 5 行且不显示内部滚动条，窗口高度继续随候选数同步。设置页移除单独“外观”分类和字体/字号编辑区，把主题并入“通用”顶部；历史字体偏好字段与运行时兼容链路不删除。普通偏好和开机启动保存成功后不再插入“已保存并应用”等状态行，控件当前值就是成功反馈，避免动态行高触发整页滚动锚定抖动；失败提示与重试保留。快捷键录制修复为 DOM capture 与 WebView2 `AcceleratorKeyPressed` 共用 Rust 状态机：通用 Ctrl/Alt/Shift 会恢复左右侧虚拟键，主窗口子类仅在录制及其 500ms 收尾窗口拦截 `SC_KEYMENU`，因此 `Alt+Win+Space` 不再弹出系统菜单。用户已在真实 Tauri 中确认任意组合键、双击左 Alt 和改回 `Alt+Win+Space` 均可录制，且系统菜单问题消失；设置成功反馈去除后的无抖动效果仍待一次真实界面复验。聚焦验证为 `advanced_shortcuts` 10/10、设置聚合测试 60/60、`cargo check --locked`、Cargo fmt、前端生产构建和 `git diff --check` 通过。
- 当前状态：阶段一至阶段八已经完成；阶段八基于真实学习记录的复习闭环已通过独立审核、自动验证和真实 Tauri/SQLite/DeepSeek 与视觉交互人工验收。
- 主题状态：ReadRayThemeV1、安全解析、SQLite v8、设置页导入/选择/删除、主窗口恢复以及随包 Flexoki（Light/Dark）与 17 个内置主题已通过独立审核和真实 Tauri 人工验收；当前内置列表保留 ReadRay Default、Flexoki 与 15 个同时支持 Light/Dark 的 Codex 主题；主题工作不改变 DEVELOPMENT_PLAN 阶段状态。
- 当前路线：Windows 原生，Tauri + React + TypeScript + Rust + SQLite。
- 比赛材料（2026-08-27）：`resource/readray_project_document.md` 已按官方附件 2 顺序完成第 1—4 章首次统一汇编，分散底稿继续保留作为证据与修改来源。汇编时已将 3.1 收敛为数据采集、知识来源、算力硬件、工程基础和风险，将架构、Agent Runtime、UI Automation、结构化解释、缓存和学习数据模型统一放在 3.2；正文不出现“为了参赛、用于提交、整理材料”等制作视角，3.3 仅按模板要求保留必要赛程和交付安排。当前主稿仍有两项明确待补：团队名称和效果评估实测数值；参考资料在最终 PDF 前还需核对并补全规范格式。计划8月31日首次上传，9月1日仅用于官网核对和必要重传。
- 划词优化：本轮方案已于 2026-08-12 正式收口。任务 1～4 均已完成实现、必要验证和用户真实使用验收；任务 4“SQLite 精确缓存与 single-flight”已合回主项目。SQLite v18 增加 7 天、256 条的 ExplanationCard 精确缓存；同 key 的 SQLite lookup/provider/usage/upsert 由 single-flight 共享，每个请求继续独立核对 requestKey/generation 并保存自己的真实学习事件，取消或迟到请求不能落库或写缓存。原任务 5 不含新功能，基于投入产出不再单独执行；其跨模块完整回归清单仅保留为未来正式发布、参赛演示打包或相关模块大改时的检查参考，不得记作本轮已运行。
- 单 token 语境消歧返修（2026-08-20）：在上述划词优化收口后，针对缩写、全大写词、点号/加号/井号标识符、下划线标识符和混合大小写标识符增加结构化的 context-sensitive word 判断，不绑定 FDE 等词库，也不新增 abbreviation 卡片类型，仍统一按 `word` 处理。UIA 对这类选区优先扩展 `TextUnit_Document`，失败再回退 `TextUnit_Paragraph`；模型在完整捕获上下文不超过 4096 字符且包含选中词时优先使用它，否则使用围绕选区的 bounded context。Prompt/cache identity 已升级到 v7，模型返回的 abbreviation/acronym/initialism 别名会归一为 `word`，并要求利用选中词之后的角色、产品、组织或领域线索消歧。`deepseek_explanation` 定向测试 28 项通过（1 项联网测试 ignored），`windows_uia` 定向测试 11 项通过，最终定向编译、fmt 与 diff 检查通过；用户已确认实际样本的翻译准确度明显改善。
- 学习目标聚合（2026-08-14 已验收）：SQLite v19、稳定 target/occurrence、目标级复习状态、历史 Feed/card/attempt/feedback 追溯、`legacy_compat` 隔离、Memory 聚合次数与全部真实出现、Review 同轮去重/跨轮 occurrence 轮换和精确目标相邻避让均已完成。Memory 搜索保留历史语境与解释命中，但优先精确目标、目标前缀/包含和原始查询，同级再按最近 occurrence 排序。实现经过自动验证、父任务代码审查、真实 v18→v19 启动修复及用户真实 Tauri/SQLite 人工验收；未进入画像、记忆注入、个性化排序、效果评估或语义聚类。
- 主窗口体验（2026-08-15 已验收）：今天页正文与底部输入框、完整对话输入区已共用稳定的内容宽度和离散断点；textarea 自动增高与最大化状态查询不再随每一帧 resize 强制布局。主窗口会在首次显示前恢复上次正常尺寸、位置和最大化状态，越界或显示器变化时自动约束到当前工作区；无历史状态时按工作区约 86% 自适应。展开态主侧栏宽度以 `readray.main.sidebar-width.v1` 独立记忆，折叠和 hover 预览不覆盖该值。用户已完成真实 Tauri 拖拽、重启和侧栏宽度人工验收；窗口状态仅属于 `main`，不影响 overlay、SQLite schema 或阶段九范围。
- 侧栏自动收放与缩放抖动修复（2026-08-16 已完成真实 Tauri 验收）：首轮已把写作/对话逐帧强制布局改为 120ms resize-settle，并加入 `src/sidebarAutoCollapse.ts`（<1000 收、>1080 放、80px 迟滞、手动折叠优先）与 `is-sidebar-resizing`。真实视频复验曾出现强烈黑/白 L 形色带，逐帧对照 Codex 桌面后确认主因是 Windows 外层窗口先变化、WebView2 内容面迟一两帧，而 ReadRay 暴露了高对比默认底板；并非侧栏状态机本身。第二轮直接按应用收口：main 恢复 `transparent:true` 合成路径，静态 `backgroundColor:#f2f1ed` 兜住启动与默认主题；新增 `src/mainWindowBackground.ts`，每次应用主题时同时同步原生窗口层和 WebView2 默认底色；`MainAppShell` 在宽度或高度连续变化时挂出 `is-window-resizing`，120ms 稳定后才结算侧栏自动收放，期间禁用侧栏/标题栏宽度过渡。用户 16:21 复验确认效果已经好很多；随后参照同机 150% 缩放下 Codex 约 720px 的物理最小宽度，把 main 逻辑最小宽度从 840 调为 480（高度仍为 600），并同步 Rust 历史窗口恢复约束。折叠态的 hover 预览入口也从整条左边缘收窄到状态按钮；按钮打开后移入侧栏可继续保持，移开后关闭。16:29 视频进一步暴露两个窄窗细节：临时预览关闭时侧栏从 absolute 切回 flex 再做宽度动画，导致主页面被短暂推动；现改为整个折叠/预览生命周期始终 absolute。另因 Tauri 480 是外层窗口宽度、实际 WebView 客户区略小，CSS 再设 480px min-width 会产生横向溢出并裁掉输入框右边距；现由原生层独占窗口下限，`.rr-main-app` 使用 `min-width:0` 服从实际客户区，composer 继续按 `calc(100% - 28px)` 和 `margin-inline:auto` 保持左右对称。最终又完成固定侧栏连续推出/收回、hover 覆盖预览、两态按钮图标，以及“正式应用 Icon + ReadRay + 右侧状态按钮”的标题栏视觉收口；用户已确认整体效果验收完毕。本轮验证：`test:settings` 57/57、`test:conversation` 28/28、`test:writing` 30/30、`pnpm build`、YAML 与 diff 检查均通过。
- 阶段八点五（Agent Runtime）：任务 0 协议、任务 1 Rust Agent Kernel、任务 2 通用对话接入与 SQLite run/step 恢复、任务 3 自动联网纵切、任务 4 日常使用交互、任务 5 长上下文与 compaction、任务 6 Writing Coach 适配均已通过调度者独立评审并正式收口（任务 3/4 含真实 Tauri 使用验收；任务 5 为预算驱动长上下文投影 + 方案 A/方案三最简折叠兜底；任务 6 合并"检查慢"流式状态/可取消与检查方式 B(1) 修正，含已知问题"检查曾返回 0 个问题"记录待后续处理）。**阶段八点五 Agent Runtime 升级全部完成**。实现最初集中在 `agent-runtime` 分支（原 `feat/agent-runtime-task1` 改名），按约定任务 1-6 全部完成评审后已一次性合入 main，main 保持干净。
- 事实型学习记忆第一阶段纵切（2026-08-28，自动验证完成，真实 Tauri 待用户验收）：主应用完整对话的共享 Agent Runtime 新增 `query_learning_history` L0 本地只读工具；只有主对话 active set 激活，Quick AI overlay 不激活。工具按 `recent/target/repeated` 和 `today/last_7_days/last_30_days/all` 下推 SQLite，只读打开现有 `readray.sqlite3`，复用 v19 稳定 target/occurrence，最多返回 20 个目标、每目标 5 条代表 occurrence，保留类型、范围内/总次数、时间、来源、代表语境与结果摘要；空结果和读取失败诚实降级。ContextAssembler 按 active tool 分别声明联网与本地学习历史能力，普通聊天不得调用，查询事实不得外推为掌握度、薄弱点、画像、排序或主动提醒；无 migration、依赖、设置页或新数据权威。验证：Rust 全量 441 passed / 6 ignored，`pnpm test:conversation` 38/38、`pnpm build`、Cargo fmt/check 与 diff 检查通过；未使用浏览器或 Computer Use。
- 首版发布候选（2026-08-24）：已按 `v0.1.0 Preview` 重新生成 Windows x64 NSIS 安装包 `src-tauri/target/release/bundle/nsis/ReadRay_0.1.0_x64-setup.exe`，包含未配置 API Key 时的主窗口引导卡片和直达“设置 → AI 服务”入口。本次构建产物大小为 41,650,032 bytes，SHA-256 为 `D0B38D14BD3C91BE890163434C7AA43ACB0F65CBF1EF5F99A7593F1106B139EC`。安装包尚未代码签名、上传或创建 GitHub Release；干净 Windows 的真实安装/升级/卸载验收仍需发布者完成。
- RC1 安装候选（2026-08-25）：用户确认恢复发布验证后，已在提交 `54fe415` 对应源码上完成 `pnpm release:build`，生成未签名的 Windows x64 NSIS 安装包 `src-tauri/target/release/bundle/nsis/ReadRay_0.1.0_x64-setup.exe`；大小 41,658,804 bytes，SHA-256 为 `1ADD481473D461679C9BC85FC92C4AF31A837E88FFF1C32A89464291B573522B`。release 已隔离开发期 `.env` 和进程环境 Key 回退，静态字节扫描确认安装包与 `readray.exe` 均不含本机 `.env` 中的真实 Key 字面值或 `D:\project\ReadRay` 构建路径。自动验证为前端 203 项通过、Rust 418 项通过/6 项 ignored，`cargo fmt --check` 与 `git diff --check` 通过；安装包 Authenticode 状态为 `NotSigned`，尚未安装、上传、打 tag 或创建 GitHub Release，下一门禁是用户亲自完成当前机器的安装/升级与核心功能体验。
- 主题精简（2026-08-24）：按用户决定，随包 Codex 主题只保留同时提供 Light/Dark 配色的 15 个主题，移除 Ayu、Dracula、Lobster、Material、Matrix、Monokai、Night Owl、Nord、Oscurange、Proof、Sentry、Tokyo Night、Temple；ReadRay Default 作为默认浅色主题继续保留。生成脚本、前端/Rust 主题数据、测试与当前主题协议说明已同步更新，未执行 release 构建。
- 主题选择预览（2026-08-24）：设置页主题与主题模式下拉在上下键、Home/End 或鼠标悬停移动高亮项时，只临时应用当前 WebView/原生背景配色；Enter、空格或点击选项才提交 SQLite，Esc、再次关闭下拉或点击外部区域会恢复数据库权威主题。预览不推进 `snapshotRef`，不会把浏览过程写成多次主题保存；主题列表键盘移动会自动滚动到当前项。
- 深色主题下拉阴影修复（2026-08-24）：设置页自定义下拉菜单不再用深色主题的浅色前景色生成阴影，改用 `--rr-main-shadow` 语义 token，消除菜单周围泛白光晕；浅色/深色主题均保持随主题切换的边缘层次。
- ReadRay Default 深色模式（2026-08-24）：Default 主题现补齐 `light + dark` 双模式，深色配色以浅色主题的语义层级为骨架，采用 overlay 的 Graphite 基底与 Amber 强调色（不直接复制 overlay 紧凑浮层样式）；前后端 canonical、模式校验和主题测试已同步，未执行 release 构建。
- DeepSeek 余额查询稳定性（2026-08-25）：确认当前保存的 Windows 凭据通过与应用相同的 Rust `get_deepseek_balance` 路径可正常取得官方 `/user/balance` 响应；设置页现按官方 JSON 请求显式声明 `Accept`，首次查询失败后 3 秒自动重试一次，仍失败则回到 5 分钟刷新节奏，并在余额卡片展示非敏感错误详情，便于区分瞬时网络问题、HTTP 拒绝和响应结构问题。`pnpm test:settings` 61/61、设置 Rust 17/17、`cargo check`、`cargo fmt --check`、`pnpm build` 均通过；未执行 release 构建。
- 复习原始语境过滤返修（2026-08-24）：确认 Windows Terminal 的 UIA `contextText` 可能把约 4,000 字符的 PowerShell/Powerline 原始窗口文本带入单词记录；目标词本身仍正确（例如 `features`），问题发生在 Review 映射把原始上下文直接当作单词完形语境。`src/reviewService.ts` 现对已记录的单词/短语英文语境设置 640 字符上限并拒绝私有区终端装饰，异常上下文回退到目标词并交给现有后台制卡，原始 `learning_records` 不删除、不改写。`pnpm test:review` 51/51、`pnpm build` 已通过；真实 Tauri 视觉复验仍由用户完成，Release 继续暂缓。
- 下一步：先由用户在真实 Tauri 主对话验收事实查询（近期单词、今日/近期学习、精确目标是否查过、重复目标与普通聊天不误调用）；事实型读取之外的阶段九仍暂停。`docs/STAGE_NINE_LEARNER_MODEL_PLAN.md` 继续保存学习证据、熟练状态、自动复习与写作强化草案，不得把本次事实纵切扩展成画像、评分或个性化实现。
- 阶段八：基于真实 `learning_records` 的最小复习闭环、后台英文制卡、学习结果/撤销、来源追溯、卡片质量反馈、缓存与重启恢复均已收口；写作、Quick AI、长期学习者记忆、主动表达和 Markdown 未并入本阶段，基于长期记忆的个性化排序属于阶段九。
- 整体性能探查（2026-08-07）：按四层（启动/前端渲染/SQLite/内存体积）实测，当前真实数据规模下**无用户可感知瓶颈**。SQLite 查询全部走索引且冷热均 <4ms；每 command 重开连接 + 8 次迁移检查经真实 rusqlite 基准测得约 1.45ms（迁移去重实测无效，1.450 vs 1.452ms，已回退）；前端流式渲染每 delta 约 0.68ms（520ms 节流下无感知）；首屏 204 DOM 节点；全部对话页实际只渲染 26 个有标题会话。明确**不要**为性能引入 SQLite 连接复用（rusqlite Connection 虽 Send，但 guard 跨 await 编译失败、流式 record_for_app 持锁会冻结 UI、Mutex 非重入、backup VACUUM 长持锁，四重风险而收益 <10ms/操作）。后续若数据规模显著增长（如数万学习记录），再重测 `list_all_quick_ai_conversations` 全量列表与流式重建成本。
- 主题启动三段式加载修复（2026-08-07）：主窗口启动曾出现"透明空白 → ReadRay Default 硬编码色 → 已选主题"的闪烁。根因：`.rr-main-app` 的 CSS 硬编码了默认主题变量（`main-app.css`），真实主题要等 `useAppTheme` 挂载后的异步 IPC `get_theme_snapshot`（SQLite 读 ~6ms）才用 inline style 覆盖，且窗口在 setup 时即 `show()`，与前端挂载并行赛跑。修复：新增 `src/themePrefetch.ts`，`main.tsx` 在 React 挂载前（仅 `view=main`）用 Tauri IPC 预取已选主题（带 2s 超时兜底、失败静默回退），`useAppTheme` 挂载 effect 改为 `useLayoutEffect`，首帧绘制前即应用预取快照（`.rr-main-app` 首帧即为已选主题），随后仍 `reload()` 权威重读。overlay / 非 Tauri 预览路径零改动（跳过预取）。验证：前端 26/30/43 测试全绿（settings 含 5 个新增预取用例）、`pnpm build` 通过、Rust 未改动（140/4 同基线）；独立审计确认无启动失败/竞态/语义破坏风险。
- 主窗口静态品牌启动层与图标光学放大（2026-08-13）：`index.html` 在 React 模块前为 `view=main` 提供静态品牌层，背景调整为与图标深色底座一致的 `#141412`，底座融入整屏；启动图标由原 80～112px 提升为响应式 132～176px。`MainAppWindow` 首帧后通过 `src/startupBrand.ts` 添加就绪类并在 160ms 内淡出；不设最短展示时间、不创建第二个 Tauri 窗口，overlay 不显示，开机启动仍保持隐藏。正式图标母版在不重绘内部图形的前提下收紧透明安全边距，小尺寸视觉约放大 4%，并重新生成全部 Tauri 图标及 256px 启动图标。此前终端中 38.98～55.66s 的等待发生在图标资源变更触发的 Rust debug 重编译/链接阶段、早于 `readray.exe` 启动，不等同于安装版应用的运行期启动耗时；静态品牌层只能覆盖 EXE 启动后的前端挂载阶段。验证：`test:startup` 3/3、`test:settings` 43/43、`test:overlay` 20/20、`pnpm build` 与 `git diff --check` 通过；用户已于 2026-08-13 确认真实 Tauri 视觉验收通过。
- WebView 默认右键菜单屏蔽（2026-08-13）：`src/main.tsx` 仅在 Tauri 桌面运行时安装 `src/desktopContextMenu.ts`，全局阻止 WebView2 的返回、刷新、另存为、打印、检查等默认菜单；守卫不停止事件传播，因此会话列表既有“重命名、导出、删除”自定义右键菜单保持可用，普通浏览器预览不受影响。验证：右键守卫 2/2、设置聚合测试 45/45、会话回归 27/27、`pnpm build` 与 `git diff --check` 通过；用户已于 2026-08-13 确认真实 Tauri 点击验收通过。
- 写作辅助编辑型对话 UI（2026-08-13）：用户问题、选区上下文与 Agent 回答已收敛为克制的编辑型对话；去除重复角色/摘要标题、左右竖线与轮次横线，回答正文提高可读性，选区并入用户提示块，内容表达使用普通列表且仅操作保留胶囊按钮。写作测试 30 项、生产构建与 `git diff --check` 通过；用户已确认真实桌面视觉验收通过。本轮未修改写作数据库、Rust command 或问答协议。
- DeepSeek 请求超时兜底（2026-08-07）：对话流式生成曾出现"一直生成中不中断"。根因：所有 reqwest 调用用 `Client::new()`（默认无超时），DeepSeek 流不关闭或网络半开时 `stream.next().await` 永久挂起，前端 invoke 永不 settle，UI 停在"正在生成"。修复：`deepseek_client.rs` 新增 `shared_http_client()`（`Client::builder().timeout(180s).read_timeout(60s)`，零新增依赖），替换全部 3 处 `Client::new()`（流式/非流式 chat + 余额 GET）及 `lib.rs` smoke test；`read_timeout` 每次读到 chunk 后重置，即"60s 无数据则中断"的流空闲检测，长回答不受影响。超时触发后链路：Rust Err → invoke reject → 前端恢复逻辑读库 → assistant 不存在 → 返回 pending → UI 显示"生成中断可重试"（既有闭环，无需改前端）。验证：Rust 141 pass / 4 ignored（含新增客户端创建用例）、前端 26/30/43 全绿、build/fmt/check 通过。
- 当前约束：不使用通用 Agent 框架，不内置商业词典，不做 OCR、本地大模型或跨平台支持。
- 交接原则：`HANDOFF.md` 只记录会影响下一次恢复上下文的信息，小型文档措辞和格式调整不记录。

## 当前阶段入口

完整文件职责和按任务检索入口以 `docs/RESOURCE_MAP.yml` 为准；这里保留已验收的阶段八复习闭环和学习目标聚合恢复入口，阶段九仍暂停，避免维护第二份资源地图。

- `AGENTS.md`：协作规则；开始任务时先读。
- `docs/DEVELOPMENT_PLAN.md`：项目方向、阶段边界和验收标准的权威来源。
- `docs/RESOURCE_MAP.yml`：完整资源索引；未在本节列出的文件从这里查找。
- `docs/WINDOWS_ENVIRONMENT.md`：本机 pnpm、Rust、Tauri、构建和发布命令基线。
- `docs/THEME_PROTOCOL.md` / `src-tauri/src/themes.rs` / `src/themeProtocol.ts`：ReadRayThemeV1、安全解析、规范化持久化和主窗口应用边界。
- `src/App.tsx` / `src/components/MainAppShell.tsx` / `src/components/MainSidebar.tsx` / `src/mainSidebarWidth.ts` / `src/sidebarAutoCollapse.ts` / `src/components/useAutoResizeTextarea.ts`：主窗口装配、页面导航、缩放期间的稳定布局、侧栏宽度记忆与窄窗自动收放、今天/对话共用 textarea 自动增高边界。
- `src/components/ReviewPage.tsx` / `src/reviewBackgroundPreparation.ts` / `src/reviewPreparationCoordinator.ts` / `src/reviewAuthorityRefresh.ts` / `src/reviewQualitySaveQueue.ts` / `src/reviewService.ts` / `src/reviewRepository.ts`：复习内容区、进入页面前的首屏预热、后台制卡协调、外部刷新延后、应用级卡片质量反馈协调（跨 ReviewPage 卸载存活）、业务映射和正式 Tauri 读取/写回链路；页面不得直接调用 command。
- `src-tauri/src/review.rs` / `src-tauri/src/learning_records.rs` / `src-tauri/src/explanation_cache.rs`：复习 Feed、追加式学习事件与 SQLite v19 schema。正常 `learning_targets` 唯一身份为 canonicalizationVersion + queryType + normalizedTargetText；无可靠英文投影的旧记录使用每记录独占、不可聚合且不可调度的 `legacy_compat` 身份。`learning_target_occurrences` 保留真实记录绑定和未来重绑 revision，`learning_target_review_states` 由全部未撤销 attempt 顺序重放；Feed、attempt、generated card 同时冻结 target/record 身份。Memory target commands 与 Review target 调度只消费正常目标，缓存仍由 explanation_cache.rs 独立负责。
- `src-tauri/src/lib.rs` / `src-tauri/src/pinned_cards.rs` / `src-tauri/src/desktop_lifecycle.rs` / `src-tauri/tauri.conf.json`：主窗口、可原地晋升的 overlay、后台替代查询窗口的命令、快捷键、关闭/隐藏、拖动、主窗口状态恢复和生命周期入口。
- `.env.example` / `src-tauri/src/deepseek_client.rs` / `src-tauri/src/secret_store.rs`：DeepSeek 开发环境回退、共享请求和 Windows 安全存储边界；不得把真实密钥写入仓库、SQLite、前端持久化或普通日志。
- `package.json` / `src-tauri/Cargo.toml` / `src-tauri/capabilities/default.json`：前端、Rust 插件和最小权限装配；新增设置能力前先确认是否能复用现有依赖。

## 已确认决策

- ReadRay 先做 Windows-first 桌面应用。
- 桌面框架使用 Tauri；遇到问题时优先定位和解决，不在计划中预设替代框架。
- UI 使用 React + TypeScript。
- 原生桌面能力通过 Rust / Tauri commands 实现。
- 本地存储使用 SQLite。
- ReadRayThemeV1 是唯一稳定内部主题协议；Codex、Obsidian 或其他来源以后只能通过各自独立 adapter 转换，不能把外部 CSS 直接交给浏览器。
- 主题只影响主应用配色，不改变 overlay、划词卡、UIA、布局、字体、字号或业务交互；当前内置 ReadRay Default 与 Flexoki 均支持真实浅色/深色模式。
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
- Tauri 初始静态窗口角色为 `main` 与 `overlay`：`main` 加载 `index.html?view=main`，显示在任务栏并允许调整大小；`overlay` 加载 `index.html`，启动隐藏、置顶且跳过任务栏。用户固定解释卡时，当前 overlay 原地转为固定窗口；Rust 在后台以相同 overlay 页面创建隐藏的 `overlay-query-N`，并将它设为后续查询的唯一活动窗口。固定卡可拖动、失焦保留但不跨应用重启恢复。`main` 只持久化 SIZE、POSITION、MAXIMIZED，并在首次显示前恢复和约束到可用工作区；overlay 与固定卡不进入该状态文件。窗口命令按活动 label 或固定注册表校验，主窗口和固定卡片状态不得写入 overlay 输入态的位置缓存。
- 主窗口默认关闭策略为隐藏到托盘，使全局快捷键和隐藏的 overlay 继续存活；设置可改为安全退出。托盘已提供恢复主窗口、快速查询和真正退出三项入口。
- Windows UIA 捕获必须在 ReadRay show/focus 前完成；`Ctrl+Alt+U` 触发划词捕获并显示选区附近的真实 DeepSeek 解释卡，`Ctrl+Alt+R` 保持无选区居中输入流程。两条链路共享 create_explanation_card，不接 SQLite、OCR 或剪贴板辅助。
- 正式交互分为两种状态：有选区和 `anchorRect` 时显示锚定结果浮层；无选区时通过快捷键呼出居中输入框，用户手动输入后再切换到结果态。
- ExplanationCard 是 ReadRay 的中间协议，服务 DeepSeek 结构化输出、compact UI 映射和后续 SQLite 本地记忆；它不是某个前端组件的 props。
- ExplanationCard 使用 `queryType` 判别联合：word 保存词义、语境、原句、搭配和例句；phrase 保存整体义、语境义和构成；sentence/paragraph 以完整中文翻译为第一信息，不强制生成单词卡字段。
- 查询类型只在本地判断，不增加第二次 LLM 请求：单个普通词或标识符形态（包括 camelCase、缩写和代码/产品标识符）判为 word；较短非完整多词内容判为 phrase；完整单句判为 sentence；多句、换行或较长内容判为 paragraph。缩写不单独增加卡片类型。
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

- 划词速度优化任务 4 已于 2026-08-12 完成父任务技术验收、合回主项目和用户真实使用验收：SQLite v18 新增 `explanation_card_cache`；canonical identity 精确覆盖规范原文、原始查询确定的方向/类型、完整最小上下文 fingerprint、模型 ID/revision、Prompt/schema version，来源应用和来源类型不参与模型输入或缓存键。命中后按当前请求重绑原文/英文目标并复用 validator；损坏、过期和身份异常项按 miss，后台条件删除与单调 touch 不会覆盖新 upsert。TTL 7 天、容量 256 条，淘汰在独立 blocking 维护中确定性收敛。single-flight 在首次 await 前加入，同 key 的 lookup/provider/usage/upsert 只执行一次；waiter 分别持有 request authority，authority 锁内 cache commit 保证取消先发生时不缓存，有效 follower 不受 leader 取消影响，每个有效完成请求仍各自保存学习事件。聚焦缓存/migration 10 项、ExplanationCard/并发 25 项（另 1 ignored 联网）、learning_records 26 项、DeepSeek client 17 项通过；`cargo fmt --check`、`cargo check`、RESOURCE_MAP YAML 和 `git diff --check` 通过。未运行全量测试、前端测试/build 或 ignored 联网测试；用户已确认任务 4 收口。原任务 5 不再单独执行，也不得表述为测试已通过；完整回归移至未来正式发布前统一进行。

- 划词速度优化任务 3 已于 2026-08-11 通过代码审查、聚焦验证、合回主项目和用户真实使用验收：纯中文查询由 Rust 本地判为 `zhToEn` 并使用四类方向化 Prompt，模型返回的 `learningTargetText` 先合并首尾和连续空白、写回卡片并同步主结果，再验证必须有有效 ASCII 拉丁字母且不得含汉字；存在英文目标的查询保持 `enToZh`，目标由 Rust 确定性规范化并覆盖模型改写，中文 `contextText` 不改变英文方向。混合中文查询保守保留 `C++`、`.NET`、`C#`、`node.js` 等代码或专有名词边界。SQLite v17 通过 `learning_record_targets` 保存 `queryDirection` 和规范英文目标，新事件与投影同事务提交；历史英文 query 确定性回填，历史中文不批量调用模型、不猜译文、不物理删除。Memory、Today、Review、搜索与复习制卡均经正式 page → service → repository → typed Rust command 消费英文目标，原始中文仍可在详情来源中追溯。聚焦 Rust 85 项通过，另有 1 项联网测试 ignored；前端 72 项与 build 通过。未运行全量测试或 ignored 联网测试；用户确认真实使用效果良好，任务 3 正式收口。

- 划词速度优化任务 2 于 2026-08-11 完成重复原句验收返修、代码审查、聚焦验证和用户真实桌面验收；用户确认返修后的体验明显改善。既有最小可靠上下文和双层请求身份保持不变。word/phrase 只允许中文主导的 `sourceSentence` 单独存在；普通英文原句仍必须同时有 `sourceSentenceZh`，译文不得脱离原句。Rust 在模型卡片进入 validator/落库前按汉字与 ASCII 拉丁字母计数清除中文主导原句的冗余中译，前端 `sourceSentenceForDisplay` 对即时结果和 SQLite 旧卡片应用同一规则，overlay 与 Memory 均只显示中文主导原句一次，普通英文原句仍保留中译。任务 2 正式收口，任务 3 未开始。

- 划词速度优化任务 2 已于 2026-08-11 完成代码审查、聚焦验证并合入主项目：选区 TextRange 存活时从同一 Paragraph 精确取得前缀/后缀，按 word/phrase/sentence/paragraph 派生模型专用 `minimalContext`；不可靠时保守回退，原始 `contextText` 不被覆盖。ExplanationCard 请求采用双层身份：前端每个 authority 实例以原生加密随机 nonce + sequence 生成分作用域唯一 key，旧实例迟到 cancel 不会命中新实例；Rust 注册时另分配内部 generation，使同客户端 key 复用时旧 guard 的 checkpoint、commit 和 Drop 均失效。注册新请求会中止同作用域旧 future，窗口隐藏、关闭、编辑、切换模式和卸载也会取消；模型响应后、usage 处理前后与学习记录同步保存前均复核权威，前端只为当前 key 发出学习记录刷新通知。未运行全量测试、真实 Tauri/UIA/SQLite/DeepSeek 或 ignored 联网测试，任务 3 未开始。

- 划词速度优化任务 1 已于 2026-08-10 完成：ExplanationCard 使用官方非 thinking 请求体，按 word/phrase/sentence/paragraph 设置输出预算，并保留 `response_format=json_object` 与完整 validator；共享 `deepseek_client` 通过 `OnceLock` 复用同一连接池，ExplanationCard 单独使用 10 秒总超时，只对连接/响应读取失败、408、429 和 5xx 最多快速重试一次。重试发生在 usage 解析和学习记录保存之前，不重复统计或落库；400、非法 JSON 和 schema 失败不重试。Rust 聚焦测试、`cargo fmt --check`、`cargo check` 与 `git diff --check` 通过；未运行全量测试、ignored 联网测试或真实 Tauri。用户已确认真实阅读中速度明显提升且质量无明显下降，真实使用验收通过。

- compact UI 从静态 mock 演进为两条正式交互：有选区时显示贴近 `anchorRect` 的 `AnchoredResultPopover`，无选区时由 `CenteredCommandInput` 进入真实解释结果；早期大背景预览仅是定位交互边界的开发阶段，不是当前产品壳。
- `ExplanationCard` 是 `word` / `phrase` / `sentence` / `paragraph` 四类 serde tagged enum；Rust validator 按类型检查必填、长度、数组、双语例句和上下文约束，`create_explanation_card` 对请求、HTTP、响应结构、JSON 和 validator 错误分别诊断。
- overlay 已形成独立桌面窗口：无边框、透明、置顶、跳过任务栏，输入/loading/error/result 使用不同窗口尺寸；默认位于屏幕偏上区域，用户拖动位置在当前进程内优先恢复，失焦或 Esc 隐藏。该位置方案已经接受，不再重复校准。
- Windows UIA 捕获优先 `TextPattern2`、回退 `TextPattern`，返回选区、上下文和物理屏幕坐标；捕获必须发生在 ReadRay show/focus 之前，锚定窗口按显示器 DPI 和工作区放置。对 context-sensitive 的单 token word 优先读取 `TextUnit_Document`，失败才回退 Paragraph，并在模型输入侧保留不超过 4096 字符的选区相关语境。
- Obsidian 1.12.7 的编辑与阅读模式已真实验证；阅读模式的关键经验是沿 Raw View 祖先链找到 Document 的 `TextPattern2`，不能只检查焦点元素。成功链路未依赖 MSAA/IAccessible2 或剪贴板辅助。
- Codex App 渲染内容区可通过 `TextPattern` 取得选区和上下文；ProseMirror 编辑区虽能取得选区和坐标，但 Paragraph 上下文可能退化为选中文本并混入对象替换字符，正式链路必须清理 U+200B/U+FFFC，并把退化上下文降级为 `null`。
- DeepSeek Flash 按本地 `queryType` 返回严格 JSON；句子/段落以完整翻译为第一信息，模型返回的 `sourceText` 在 serde 前由捕获输入覆盖，避免模型改写原文。锚定结果按内容和类型自适应宽高，超出工作区时内部滚动。
- 不联网 Rust 测试和 word、camelCase、长句、段落四类真实 DeepSeek 请求均完成过验证；Codex App 与 Obsidian 阅读区也分别取得过真实锚定解释卡。

### 本地数据、主应用与 Quick AI

- `learning_records` 使用 `rusqlite` bundled 和 `schema_migrations`；v1 把每次成功解释保存为不可覆盖的独立事件，保留原文、标准化文本、类型、来源、可选上下文、完整 ExplanationCard JSON、schemaVersion 和时间。未定义的 difficulty 保存为 `NULL`。
- `create_explanation_card` 先完成 DeepSeek 解析和 validator，再写学习记录；请求、解析、校验或存储任一失败都不留下记录。manual 与 windows_uia 共用同一正式链路。
- 学习记录 commands 已覆盖分页、搜索、单条、删除和只读今日摘要；“记忆”“今天”分别经 repository/service 读取真实数据，页面不接触 SQL。正式 Tauri 路径不读取 fixture，overlay 保存成功会通知已打开页面刷新。
- “今天”只陈述当天真实查询数量、最近记录、类型、来源和时间，不生成当前 schema 无法证明的复习数量、高频词或趋势；“记忆”以聚合学习目标展示详情，并在“过去的出现”中列出该目标的全部真实 occurrence、原始来源、上下文和时间。
- Quick AI 使用独立普通 chat/completions 请求，不复用 ExplanationCard JSON；两者只共享 `.env`、模型/API key 和 HTTP 错误边界。`Ctrl+Alt+R` overlay 支持解释与 Quick AI 切换、新对话、多轮发送和隐藏，真实 DeepSeek 两轮上下文已验证。
- SQLite v2 的 `quick_ai_conversations` / `quick_ai_messages` 与 `learning_records` 分表，消息保存 role、content、sequence 和时间，为完整对话和后续管理提供权威数据。
- `main` 与 `overlay` 是独立窗口；overlay 的呼出意图先由 Rust 原子保存，再由前端在事件、获焦或挂载时领取，避免隐藏 WebView 漏事件或沿用错误模式。只有 overlay 失焦自动隐藏。
- 主应用侧栏默认宽 252px，可在 180–360px 内拖拽；拖拽结束后把展开态宽度保存到 WebView localStorage 的版本化键 `readray.main.sidebar-width.v1`，无效、损坏或不可用时安全回退默认值。手动折叠后主内容完全铺开，折叠宽度 0 和左缘 hover 临时预览都不覆盖已保存的展开宽度；点击标题栏按钮恢复固定侧栏。“今天”“记忆”“写作”“对话”共享同一外壳。最近对话标题来自真实 SQLite，空标题排除，溢出时才渐隐。
- 1440×900 / scale 1 继续只是主应用设计基线，不再是每次启动强制覆盖的窗口尺寸。主窗口最小 480×600（本机 150% 缩放约为 720×900 物理像素）；存在有效历史状态时恢复上次正常尺寸、位置和最大化状态，首次运行或状态无效时按当前工作区约 86% 自适应并以 1440×900 为上限，断开显示器、DPI 或工作区变化时会把窗口约束回可见区域。标题栏已接原生拖动、最小化、最大化/恢复和隐藏。
- 应用随包内嵌 Geist、Geist Mono、Newsreader、思源黑体和思源宋体及对应 OFL，正式 UI 不依赖联网或本机字体；浏览器预览按完整设计画布整体缩放，响应式断点由应用容器触发。

### 阶段五：写作正式接线

- 已按 `design-open-design/readray-writing-2.html` 在现有 MainAppShell 中实现“写作”页：写作导航和“今天”页写作入口进入本地文章库，支持空白稿/已有稿、标题与正文编辑、文档切换、选区菜单、“问 ReadRay”多轮追问、真实写作教练问题、定位/修改/进一步提示/参考/忽略、多轮检查、双栏文本差异、随完成版本保存的“本次写作要点”、完成版本和继续修改；要点明确未加入复习，也没有全局模式库。
- 写作编辑区沿用 1440×900 / scale 1：纸张宽 680px、正文 18px / 1.68 行高、编辑列上限 736px；草稿/完成稿未打开辅助栏时纸张视觉居中，检查或辅助打开后自然重排。容器宽度不超过 1120px 时辅导区自动收起，宽窗口支持拖拽调整编辑区宽度。
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
- 阶段四收口时真实 Quick AI 还是非流式完整响应；当前已升级为可停止的 SSE 流式输出并支持中断后重试，阶段六也已补上原生导出。回答重生成和记忆引用仍保持禁用。
- Quick AI 发送使用 `conversationId + expectedUserSequence` 作为稳定轮次身份：`prepare_turn` 先单独提交 user，再调用 DeepSeek；`complete_turn` 校验 message ID、sequence 和当前尾版本后只补一条 assistant。模型、进程或 assistant 保存失败都会留下可恢复 pending user。
- 同一轮重试复用既有 user message ID/sequence；若 assistant 已存在，后端直接返回权威快照且不再次请求模型。若事务已提交但 IPC 回传和随后读取都失败，页面仍保留原 expected sequence，下一次重试同样由后端幂等识别，不依赖 prompt 文本猜测。
- 本轮未追加 migration：现有 v2 `quick_ai_messages` 的自增 ID 和 `UNIQUE(conversation_id, sequence)` 已足够提供 pending 身份与 expected-version 约束；旧历史、并发不同内容或错误 message ID 会明确冲突，不会静默写入。
- 完成态 fixture 导出会生成用户可下载的 Markdown 文件，并按顺序包含 thread 的全部 user/assistant 消息；空结果或异常不会触发下载或成功提示。
- fixture 通过 `conversationFailure=create|load|generate|export` 显式注入一次性失败，正常重新生成不再强制失败；`[fixture:slow]` 只用于停止/继续演示。记忆抽屉关闭时会移除内部焦点并恢复到原引用按钮。
- 完整对话页此前完成的是 fixture 路径的视觉与交互人工验收；本机 pnpm 构建与 Headless Playwright 回归均通过，覆盖生成中重复发送、连续两轮、生成中导出、完整 Markdown 下载、导出失败重试、停止/继续、重新生成和抽屉焦点。1440×900、840×600 与两档侧栏无页面级横纵溢出或重叠；`preview=responsive` 只用于浏览器真实容器验收，不改变默认等比预览和 Tauri。阶段四收口时已另行完成真实 Tauri/SQLite/DeepSeek 功能验收。
- 阶段四返修自动验证：前端 repository/service 共 9 项测试通过；Rust 40 项通过、2 项需真实网络的 live test 按既有标记忽略。覆盖模型失败后重开仍有 pending user、重启重试只补一条 assistant、已提交但调用方未确认时不重复、assistant 保存失败保留 pending、旧版本/并发冲突拒绝写入，以及 41/43 条消息长对话截断后仍从 user 开始。
- 阶段四真实 Tauri 功能验收已经完成：真实创建、最近对话加载、多轮续聊和侧栏会话身份均可用；用户接受当前对话体验作为后续优化项，不再阻塞阶段五。
- 2026-08-06 对话回归修复：`MainAppShell` 以稳定回调向 `ConversationPage` 传递会话身份，避免父级重渲染重新启动创建/加载 effect；对话页 composer 复用今天页 `.rr-main-composer` 并移除旧的独立样式。会话前端回归 19 项、`pnpm build` 已通过；尚待真实 Tauri 窗口确认新对话不重复创建、输入可持续输入和侧栏历史不再被重复新会话顶出。已有数据库中的重复会话未自动删除。
- 2026-08-06 对话创建幂等与历史数据修复：`ConversationPage` 按 request key/service 缓存创建 promise，避免 React StrictMode 的 effect 重放再次创建 SQLite 会话；创建失败会清除缓存，重试仍可重新创建。已对本机数据库完成精确备份和清理：备份为 `readray.sqlite3.before-conversation-repair-20260806-161736.bak`，删除 ID 966–14097 的 13,132 条已确认故障批次（331 条重复 `who are you?`、12,801 条空会话），保留旧历史和 ID 14098 之后的修复后记录；清理后 SQLite `integrity_check` 通过，剩 11 条有标题会话。

### 阶段六：会话管理闭环（已完成）

- Quick AI Overlay 来源隔离返修已收口：Overlay 最近列表和完整历史只查询 `overlay`；主窗口侧栏最近对话显式查询 `main`，主窗口“全部对话”继续统一展示 `main + overlay` 并留在主窗口内打开。SQLite v10 一次性删除早期无法追溯且仅用于测试的 `legacy` 会话，消息由现有外键级联清理；前端不再暴露旧会话来源或筛选入口，后续创建入口只允许 `main` / `overlay`。
- “查看全部对话”已接入 `list_all_quick_ai_conversations`，直接读取 SQLite 中全部有标题会话并按 `updated_at_unix_ms DESC, id DESC` 排列；独立页面覆盖 loading、empty、error 和 retry，不从侧栏最近六条推导历史。
- 重命名和删除复用现有 `ConversationService` / `ConversationStore`，所有操作只提交数据库 conversation ID。侧栏最近项与全部历史均为左键打开、右键显示“重命名/导出/删除”，列表不保留常驻文字按钮或悬停更多按钮；两处共用 Shell 级管理浮层，不复制业务请求。当前会话标题单击后原地编辑，Enter 保存，Esc 或失焦安全取消。重命名成功后用 Rust 返回的完整权威快照同步当前 thread；删除依赖既有外键级联清理消息，删除当前会话后进入新的空会话。成功操作触发既有刷新令牌，使侧栏与全部历史重新读取。
- 当前页的管理操作同时绑定 mounted、request key 与 conversation ID；父级再用实时页面、请求和会话 ref 复核删除回调。在操作期间切换到其他会话或“今天/记忆/写作/全部对话”后，迟到结果不会回写已卸载页面，也不会创建新对话或把用户带回会话页。重命名、删除失败保留当前对话和弹窗状态，可原地重试；不存在的删除不会提示成功。
- 原生导出使用官方 Tauri 2 dialog 插件让用户选择 Markdown 路径并提供明确取消语义；取消时不调用 Rust 导出 command，也不显示成功。用户确认路径后，Rust 按 conversation ID 重新读取 SQLite 权威快照并按 sequence 写出完整 user/assistant 消息，不使用前端 messages 重建文件。空白会话在菜单和 service 入口两层禁用，不会打开保存对话框；无效结果和写文件失败均不修改会话。
- 新增依赖仅为 `@tauri-apps/plugin-dialog` / `tauri-plugin-dialog` 及其 capability `dialog:allow-save`；替代方案是固定写入下载目录，但无法覆盖用户选择路径和取消验收。SQLite、DeepSeek 客户端和会话表均未新增第二套实现，也未追加 migration。
- 正式 Tauri 会话路径的静态测试确认不读取 fixture/localStorage；非 Tauri 预览仍只通过动态 import 加载 `conversationFixtureService.ts`，继续与正式路径隔离。
- 阶段六自动验证已通过：会话前端 18 项、写作前端回归 25 项、完整 Rust 65 项通过且 2 项真实 DeepSeek 联网测试按既有 ignored 标记跳过；`pnpm build`、`cargo fmt --check`、`cargo check`、RESOURCE_MAP YAML 解析和 `git diff --check` 均通过。没有使用浏览器或 Computer Use，也没有启动 Tauri 窗口替代人工验收。
- 用户已在真实 Tauri 主窗口完成人工验收：全部历史、侧栏与历史页左键打开/右键管理、当前标题原地重命名、删除确认、原生导出及列表同步符合预期；阶段六已正式收口。

### 阶段七：设置与桌面生命周期（已完成）

- 已按 `design-open-design/readray-settings.html` 把设置入口接入现有 `MainAppShell` / `MainSidebar`，没有复制外壳。当前保留通用、AI 服务、数据、关于四类导航，主题选择与浅/深色模式已并入“通用”顶部；字体与字号编辑控件暂时从页面移除，但既有偏好字段、运行时应用和存储兼容链路保留。Key、余额、Token、数据、发送键与生命周期设置均已完成；本轮只在原先禁用位置接通两组全局快捷键、开机启动和关闭策略，没有重做设置页或 App Shell。
- 正式前端路径为 `SettingsPage -> SettingsService -> SettingsRepository -> Rust commands`。页面覆盖 loading、读取失败/重试、Key 首次配置、真实验证、成功保存、更新时保留旧配置、失败原地重试、清除确认/失败重试和卸载后迟到结果拒绝；组件不直接 invoke，也不读取 localStorage/sessionStorage。
- `src-tauri/src/secret_store.rs` 使用 Windows Credential Manager 的 generic credential 持久化 DeepSeek Key；前端快照只返回 configured/source，不返回明文或尾号。清除时先写非敏感禁用标记，再删除已保存 Key，因此开发机 `.env` 不会在清除后偷偷重新生效；保存的新 Key 优先于标记，失败清除不会误报为已停用。
- 候选 Key 先通过共享 `deepseek_client` 向当前 ReadRay 模型执行真实 `chat/completions` 验证，成功后才写入凭据管理器；失败不会替换现有配置。正常解释、Quick AI 和写作请求已统一改为优先读取安全存储，只有尚未形成保存/清除决定时才兼容开发期 `DEEPSEEK_API_KEY`。
- API Key 状态一致性返修已把保存/清除命令统一为“先读取完整非敏感快照，再执行凭据变更，成功后仅在内存更新 Key configured/source”的顺序。这样 SQLite 概览失败会发生在凭据变更前，不会出现凭据已经成功但前端收到失败；凭据操作失败仍返回错误并保留原配置。
- 余额 command 复用安全存储和 `deepseek_client` 访问 DeepSeek 官方 `GET /user/balance`；Rust 严格校验 `is_available`、唯一三字符币种和三个非负十进制金额字段，支持同时返回多币种。页面仅展示总余额和币种，不展示赠送/充值来源拆分；进入已配置 Key 的 AI 服务栏目立即查询，窗口可见期间在每次请求完成 5 分钟后自动刷新，手动刷新会重新计时。Key 首次配置、更新和清除会显式切换余额凭据代次并立即清空旧账户余额，不依赖 configured 布尔值变化；旧请求结束后才使用新 Key 查询，避免请求重叠和旧结果回写。隐藏、离开栏目和卸载停止计时并隔离迟到结果，后续刷新失败保留同一 Key 的上次成功余额；余额只存在页面内存，不保存或返回 Key。
- 设置快照从 Tauri 运行时返回实际模型、应用版本和数据目录，并直接从 SQLite 读取学习记录、Quick AI 会话、写作文档计数；没有从前端列表推算。“打开数据目录”无前端路径参数，Rust 重新解析真实 `app_data_dir` 后调用既有 opener，失败会留在页面明确重试。
- SQLite 备份复用既有原生保存对话框；取消时不调用 Rust、无文件且无成功提示。确认路径后，Rust 在阻塞线程对权威 `readray.sqlite3` 执行 `VACUUM INTO` 同目录临时文件，完成 `PRAGMA quick_check` 后才替换目标；失败清理临时数据库及 journal/WAL/SHM，不修改源库。快照覆盖学习记录、对话、写作及 SQLite 内非敏感设置，不包含 Windows 凭据管理器中的 API Key；恢复、清空和全量结构化导出仍未实现。
- SQLite schema v5 新增 `model_usage_records`，只保存 DeepSeek 响应中的 promptTokens、completionTokens、totalTokens、三类业务枚举和数据库写入时间；旧数据库升级后表为空，不补造历史，也不保存提示词、回答、Key 或费用。分类固定为解释查询、Quick AI、写作；候选 Key 验证和余额 GET 没有统计入口。
- 三类正式模型请求统一通过 `deepseek_client::post_tracked_chat_completion`：成功 HTTP 响应先严格校验 usage 及 total=prompt+completion，在共享边界尽力写入 SQLite，再反序列化业务响应。因此合法 usage 即使后续 ExplanationCard/写作 JSON 或 Quick AI 结构校验失败仍会计入；统计写入失败不会让模型业务结果失败。
- 设置页使用量提供今天、近 7 天、近 30 天、全部四档。Service 使用本机日历生成 `[本地零点, 下一本地零点)` 半开边界，Rust 按边界聚合总 Token、请求数和三类输入/输出/总量明细；`statisticsStartUnixMs` 始终取当前范围内第一条真实 usage 的 `created_at`，空范围返回 `null` 并显示“暂无记录”，不再用筛选范围起点冒充统计开始日期。页面覆盖 loading、error、retry，不统计其他应用。备份默认文件名继续使用本地年月日，显式 UTC+8 测试覆盖 23:59:59 与 00:00:00 跨日，不使用 `toISOString()`。
- SQLite schema v6 新增单行 `app_preferences`，保存界面字体/14px 默认字号、学习内容字体/17px 默认字号、发送快捷键和 revision。字体枚举只允许随包的 Geist + 思源黑体或纯思源黑体、Newsreader + 思源宋体或纯思源宋体；界面字号限制 12–20px，学习内容字号限制 14–24px。Rust 在事务中按 expected revision 更新并拒绝陈旧写入；旧数据库升级后得到默认值，不改写既有数据。
- 字体与字号通过 `--rr-ui-*` / `--rr-learning-*` 语义变量分开作用：界面字号按原有层级缩放主窗口与 overlay 的界面文字，学习变量只进入阅读、对话与写作内容，写作工具栏继续使用界面变量。字号候选必须先通过整数和范围校验才会乐观应用；保存协调由主窗口持久的 `AppPreferenceSaveCoordinator` 承担，设置页卸载后失败仍会读取并全局应用 SQLite 权威值，旧失败则由跨页面 generation 拒绝，不能覆盖后续成功设置。页面只在仍挂载且请求身份匹配时更新局部提示。主窗口与 overlay 都只经 SettingsService 读取偏好，监听提交事件，并在获焦或重新可见时重读，因此重启、隐藏后重新显示和跨窗口使用同一数据库状态；不使用 localStorage/sessionStorage。
- 发送快捷键支持 Enter 发送/Shift+Enter 换行，或 Ctrl+Enter 发送/Enter 换行；今天、完整对话、overlay Quick AI 和写作辅导共用同一 `shouldSendMultilineMessage`，`nativeEvent.isComposing` 为真时始终不发送。单行解释查询未接入该偏好，继续固定 Enter。
- 设置页响应式规则继续以应用容器为准：分类导航默认是 52px 顶部横向导航；900px 以下表单和数据行纵向排列。
- 第三批自动验证通过：设置前端 15 项、会话前端回归 18 项、写作前端回归 25 项；完整 Rust 91 项通过，2 项真实 DeepSeek 联网测试按既有 ignored 标记跳过；`pnpm build`、`cargo fmt --check`、`cargo check`、RESOURCE_MAP YAML 解析和 `git diff --check` 均通过。设置测试覆盖页面卸载后的全局回滚、旧失败与新成功隔离及卸载组件不更新状态；没有使用浏览器、Computer Use 或 Tauri 窗口替代人工验收。
- 桌面生命周期新增官方 `tauri-plugin-single-instance`、`tauri-plugin-autostart` 和 `tauri` 的 `tray-icon` feature；替代方案分别是自建 Windows mutex/IPC、手写注册表和 Win32 Shell_NotifyIcon，均会复制平台能力。两个插件只在 Rust 使用，未新增前端依赖或 capability；single-instance 按官方要求最先注册，第二进程在 setup/SQLite/托盘前即被拦截，再次手动启动只恢复、显示并聚焦已有 main。
- 托盘使用现有应用图标，左键恢复主窗口，右键菜单严格只有“打开 ReadRay”“快速查询”“退出 ReadRay”；快速查询复用既有 overlay intent/尺寸/聚焦链路。overlay 失焦/Esc 隐藏和锚定窗口边界未改。main 静态配置先隐藏，手动启动由 Rust setup 正常显示；仅携带专用 autostart 参数时 main/overlay 都保持隐藏。
- SQLite v7 只向 v6 `app_preferences` 追加 `close_behavior`、`quick_query_shortcut` 和 `selection_explanation_shortcut`，保留同一 revision 权威。开机启动不写 SQLite，设置快照和切换 command 每次读取官方插件的 Windows 实际状态。快捷键运行时元数据同时保存 SQLite 映射、实际 `registered_shortcuts` 和两项独立 startup error；任意偏好保存失败时，物理注册与完整 active 元数据一起恢复，两组快捷键无需重启即可继续响应。启动时两项都冲突也不改写 SQLite；修改一项只尝试注册该项，另一项错误继续显示，之后可单独恢复第二项。
- 全局快捷键升级（2026-08-25，2026-08-27 录制返修）：快捷键权威模型为 `version=2` 的 `chord` / `modifierDoubleTap` 判别联合。SQLite v22 追加两项 binding JSON 列并保留 v7 历史字符串列：新数据库默认快速查询 `Alt+Super+Space`（设置页显示为 Alt+Win+Space）、选区解释双击左 Alt；已有数据库只把自己的两项历史字符串转换成 chord JSON，不替换旧默认或用户自定义值。普通组合键继续由 Tauri global-shortcut 注册，双击左 Alt 由 `advanced_shortcuts.rs` 的 Windows `WH_KEYBOARD_LL` 层识别，运行时不吞键。设置页录制时，主窗口 capture 阶段的 `keydown/keyup` 经 typed command 提交到同一 Rust 状态机；WebView2 `AcceleratorKeyPressed` 补获会在 DOM 前被 Windows 消费的系统组合键，并按 lParam 把通用 Ctrl/Alt/Shift 归一到左右键。主窗口 `SetWindowSubclass` 只在录制中及完成后 500ms 内拦截 `WM_SYSCOMMAND/SC_KEYMENU`，避免 `Alt+Space` 菜单闪现；平时系统菜单不受影响。结果、取消和错误仍通过 `readray://shortcut-recorded` 收口，切页或卸载会取消录制。
- 初始升级的自动验证覆盖新装/旧库迁移、普通注册集合与高级动作分流、350ms 双击状态机及插入键取消、原生录制接线、偏好校验和前端构建；2026-08-27 返修后又通过 `advanced_shortcuts` 10/10、`pnpm test:settings` 60/60、`pnpm build`、`cargo check --locked`、`cargo fmt --check`、RESOURCE_MAP YAML 与 `git diff --check`。用户已在真实 Tauri 设置页验收普通组合键、双击左 Alt、`Alt+Win+Space` 录制及系统菜单抑制。尚未因此自动覆盖的运行期矩阵仍包括：中文/英文输入法切换、浏览器/编辑器/Office/Obsidian 等外部应用误触与菜单行为、隐藏 main、重启、快捷键冲突，以及管理员窗口的 UIA 选区捕获。安全桌面（UAC 提示、锁屏）不属于支持范围，普通权限进程也不能保证读取高权限窗口选区；本轮未构建 release。
- 默认关闭 main 隐藏到托盘并保留快捷键和后台保存；选择“退出 ReadRay”后与托盘退出共用安全退出。持续存活的应用级协调器跟踪偏好、Key 保存/清除、开机启动写入并 flush 全部防抖写作草稿，切离 SettingsPage 后仍会等待操作，卸载组件不接收迟到状态；模型请求不加入等待。收到退出请求即激活 mutation gate，并显示阻断交互的“正在保存并退出”，设置和写作编辑入口拒绝新修改，flush 以 generation 确认静默。失败后解除 gate、恢复 main，并提供重试、取消和仍然退出。取消先让 Rust 清除 pending ID；窗口显示/聚焦失败只记录警告并仍返回取消成功，前端失败分支还会重读 pending 状态，避免困在过期请求。
- 本轮复审修复后自动验证通过：设置/生命周期前端 25 项、会话前端回归 18 项、写作前端回归 26 项；完整 Rust 103 项通过，2 项真实 DeepSeek 联网测试按既有 ignored 标记跳过；`pnpm build`、`cargo fmt --check`、`cargo check`、RESOURCE_MAP YAML 解析和 `git diff --check` 均通过。自动测试未修改本机开机启动项，也没有使用浏览器、Computer Use 或真实 Tauri 窗口替代人工验收。
- 阶段七复审与真实 Tauri 人工验收已经通过：托盘左右键与菜单、两种关闭策略、安全退出成功/失败/取消/强制退出、开机启动注册/注销及隐藏启动、第二次手动启动恢复、快捷键录制/冲突/逐项恢复/重启恢复，以及隐藏主窗口后的快捷键和后台自动保存均已验收；阶段七正式收口。
- 阶段七完成后的主窗口体验补强已于 2026-08-15 收口：引入官方 `tauri-plugin-window-state`，仅为 `main` 使用独立 `.main-window-state.json` 保存 SIZE、POSITION、MAXIMIZED；恢复发生在隐藏的 setup 阶段并早于首次显示，显示器变化时修正越界几何，正常关闭到托盘的既有语义不变。侧栏宽度使用前端版本化 localStorage 独立保存，不进入 SQLite，也不新增 migration。窗口缩放期间的今天/对话输入框和正文采用共同宽度变量、离散断点与 resize-settle 更新，降低连续重排。聚焦验证包括 Rust 窗口几何 11 项、桌面生命周期前端 9 项、侧栏宽度 3 项、主外壳回归 28 项及生产构建；用户已确认窗口缩放体验、窗口状态和侧栏宽度重启恢复均无阻断问题。

### 阶段八：真实学习记录的最小复习闭环（已完成）

- 现有 `MainAppShell` 只增加复习内容区和导航装配，没有复制设计稿外壳。正式前端链路固定为 `ReviewPage -> ReviewService -> ReviewRepository -> typed Rust commands -> SQLite`；非 Tauri 预览只动态导入 `reviewFixtureService.ts`，内存 fixture 不使用 `localStorage`，也不会回退到正式路径。
- SQLite v11、v12 与已执行的 v13 均保持不改；v13 为生成卡增加有效期、最后使用时间和使用次数，并把质量反馈迁移到 Feed 条目与 recorded/generated 卡片语境身份。v14 只清理“前序轮次不完整且自身没有任何 attempt 历史”的旧版提前后续 Feed，并新增按稳定制卡 request key 保存的失败次数与退避状态；存在 active 或 undone attempt 的后续条目、以及任何持久质量反馈或幂等日志的条目都会保留，反馈不再作为学习结果的一部分被级联删除。v15 兼容已经执行旧 v14 的本机数据库：按仍存在的 active attempts 重放调度并审计 target 聚合，不一致时只递增 revision 后修复；已被旧 v14 物理删除的行为无法恢复，因此新 v13→latest 必须依赖收窄后的 v14 先避免丢失。V1 严格保持一个 `learning_record` 对应一个复习目标，不按 `normalizedText` 合并；Feed、生成语境、状态、attempt、下次复习时间和质量反馈都持久化，`learning_records` 不回填、不覆盖。
- SQLite v16 兼容“schema_migrations 已登记 1～15，但 review_quality_feedback 仍是旧中间版”的真实数据库：按实际列与唯一索引检测结构；缺少 `card_context_key` 时在同一迁移事务中重建并保留全部反馈字段，`generated_card_id IS NULL` 回填 `recorded`，否则回填 `generated:<id>`，唯一约束改为 `(feed_item_id, card_context_key)`。已经是最终结构的数据库只登记 v16，不重建反馈表或幂等日志；禁止手工改写真实数据库。
- 当日本地日历使用游标分页持续生成 Feed，不设每日条数上限。每轮每条真实记录只出现一次；先取已经到期或将在今天结束前到期的目标，再排新记录与继续练习。当前轮所有条目都有生效 attempt 前，仓库不创建下一轮；完成整轮后同一目标才可再次出现。当前排序只陈述这些真实状态，不声称薄弱项或个性化策略；阶段九才用长期学习者记忆做可解释推荐排序。
- cycle 0 只直接使用 `learning_record.context_text` 中保存的完整英文语境；ExplanationCard 的 AI 例句和 sourceSentence 不标成学习时原始语境。Tauri 主应用启动及 `learning-record-created` 更新后，在 ReviewPage 未挂载时先通过正式 ReviewService 读取首批 Feed，并用同一个应用级协调器填充首屏缓冲；页面挂载后立即接管，迟到预热读取不得覆盖页面 Feed。缺失可用英文语境或进入后续轮次时，协调器在条目发布前通过正式 service/repository/typed command 按需请求 DeepSeek。尚未浏览的 Ready、queued 与 working 低于 6 才补到目标 12，并发上限 2；每张可见卡按 feedItemId 独立登记浏览，后序 Ready 不会越过并删除前序 pending/失败。制卡失败在 SQLite 按稳定 key 记录次数和指数退避；启动预热直接跳过仍在退避期的条目，显式重试携带 `explicitRetry` 但继续复用原 key。day/feed item/learning record/cycle 组成队列身份与稳定 request key，跨日迟到结果不能进入当前页。
- `review_generated_cards` 有效期为 30 天，每个 learning record 的 3 张与全局 256 张均是 LRU 软上限：仍被任意可恢复 Feed 引用的有效卡受保护，只让未引用复用缓存占用剩余容量，真正过期后仍可淘汰。每个真实 occurrence 的前三张优先提供不同语境，第四轮及以后只轮换复用同一 learning record 的卡；257 个不同 learning record 各有一张绑定卡时不会解绑第一张、重新制卡并逐张振荡。受保护卡可暂时超过软上限，长期数据由有效期、每记录复用与未引用缓存 LRU 共同收缩。
- 只有“想起来了 / 没想起来”事务写回成功才完成一项；打开、来源、关闭和质量反馈均不计完成。复习详情现已直接展示完整语境、翻译和解释，不再挖空、提示或翻面；两个结果按钮在阶段九接入自动证据与调度前暂时保留。因为答案已经可见，新提交保守地记录 `usedHint=true`，避免旧调度把它当成无辅助回忆成功。写回失败保留当前卡片并用同一 request key 原地重试；expected revision、稳定 request key、页面与队列条目身份共同拒绝双击、重复 attempt、模糊成功和迟到结果。
- 每次学习结果（包括较早轮次和最后一项）都可撤销；撤销在事务内按仍生效的 attempt 重算目标并推进 revision。外部 learning-record 刷新在 outcome、undo 或质量反馈写回期间延后，写回结束后通过 `get_review_feed_item_state` 读取 SQLite 权威条目和全局完成统计，避免吞掉已提交或模糊成功的结果。
- 卡片质量赞踩点击即写入，原因和详情完全可选，关闭详情不会取消在途或已保存反馈。质量反馈由主应用装配的应用级 `ReviewQualityCoordinator` 承担，跨 ReviewPage 卸载继续运行。协调器已收口为明确异步状态机：尚未发出的同卡 save/undo 意图可合并为最后选择；一旦发送，requestKey、expectedRevision 与 payload 整体冻结。直接成功和“SQLite 已提交但 IPC 失败”的权威确认成功共用恰好一次 finished 收口；对账仍未知的失败保留冻结请求，显式重试先预读目标状态，未达成时复用完全相同输入。确定失败发出 failed 终态、按卡片身份独立展示且不阻塞其他卡片；ReviewPage 在所有终态读取 SQLite 权威条目，并在全队列结束后释放外部刷新门禁。App 已把协调器接入 `DesktopSaveCoordinator`：退出开始后拒绝新反馈，flush 等待 queued/预读/写回/对账全部完成，未解决失败携带卡片身份阻止安全退出。Rust command 继续核对 cardContextKey，旧生成语境的迟到请求不能误写到新卡。反馈仍使用独立表和幂等日志，不改变学习结果或调度，本阶段不声称已用于个性化。
- 来源抽屉展示该卡片对应的真实 occurrence，并可带稳定学习目标进入 `MemoryPage`。复习页按最终英文内容稳定映射紧凑/普通/长文密度与三种 editorial 样式，自然形成错落多列；主题派生纸张纹理、弱阴影和浅遮罩均不使用随机布局。聚焦详情直接展开完整内容，短内容自然收缩，长内容到上限后内部滚动。没有复制主外壳。本轮没有接写作、Quick AI、长期学习者记忆、主动表达或 Markdown；长期学习者记忆已明确移到阶段九。
- v14/v15、受保护卡片池与多卡反馈协调器复审修复后自动验证通过：复习聚焦前端 49 项、会话 27 项、Markdown 解析 20 项、写作 30 项、设置/主题/生命周期 43 项、overlay 8 项；完整 Rust 171 项通过且 4 项真实 DeepSeek 联网测试按既有规则忽略；`pnpm build`、`cargo fmt --check`、`cargo check --all-targets`、RESOURCE_MAP YAML 解析和 `git diff --check` 均正常。迁移行为测试包含 v13 安全升级保留 active/undone 后续 attempt、后续轮次“反馈但未作答”条目及其反馈保留、旧 v14 可检测 target 修复；卡片池测试真实绑定 257 个不同目标并重复维护/重载最早条目；反馈协调器测试覆盖 A working→B/C、同卡 save/undo 合并、失败隔离、模糊 save 唯一成功终态、模糊 undo 冻结输入重试、跨页面卸载、close 与安全退出全队列等待。会话套件两条 CSS 静态断言已允许 Windows CRLF，不改变产品样式。Vite 仍只有既有主 chunk 超过 500 kB 的非阻断警告。该轮自动验证没有启动浏览器、Computer Use 或真实 Tauri，随后已完成独立 code review 和用户真实桌面验收。
- SQLite v16 兼容修复先在临时合成库验证：精确登记 v1～v15 并重建中间版反馈表，recorded/generated 两类行的 ID、revision、active、原因、详情和时间均保留，复合唯一约束及升级后的 save/undo 通过；最终结构无损、新库、v12→latest 和既有卡片语境反馈回归也通过。`cargo fmt --check`、`cargo check --lib` 与 `git diff --check` 正常。随后用户彻底退出并重启真实 Tauri，确认真实旧数据库完成 v16 升级，赞踩与可选详情可以正常写入；完整复习流程、后台英文制卡、交互、结果/撤销、来源和重启恢复均人工验收无问题，阶段八正式完成。

### ReadRayThemeV1 主题基础设施（已通过审核与人工验收）

- `docs/THEME_PROTOCOL.md` 定义版本化 manifest、light/dark 模式、语义配色、必填/可选 token 和确定性回退；内置 `ReadRay Default` 保留当前浅色变量并留在代码资源中，不写 SQLite。真实 Flexoki Obsidian/Codex 样本只用于核对表达能力，本轮没有实现或导入任何外部主题 adapter。
- 正式路径为 `SettingsPage -> AppThemeController -> ThemeService -> ThemeRepository -> typed Rust commands`。repository 只打开原生单目录选择器；Rust 只读取该目录直属的普通 `manifest.json` / `theme.css`，符号链接和目录外路径拒绝。页面不直接 invoke、读文件、写 SQLite 或使用 localStorage/sessionStorage。
- `theme.css` 只作文本解析：仅允许 `:root`、`body`、`.theme-light`、`.theme-dark` 中的白名单颜色变量；未知选择器、未知变量和普通属性不进入运行时并返回警告。所有 at-rule、`url()`、远程字体/图片、脚本、嵌套规则、非法颜色、超限、重复声明和低可读性主题均拒绝；浏览器只接收规范化后的逐项 CSS 变量，不接收原始 CSS。
- Rust 与 TypeScript 现共用严格的规范颜色语法：十六进制颜色使用小写最短形式，RGB 整数不保留前导零，rgba alpha 收敛为 `0`、`1` 或无尾随零的 `0.x`；Rust 不再把 `00.5` 一类前端会拒绝的值写入 SQLite。
- SQLite v8 新建 `custom_themes` 与单行 `theme_preferences`，只保存规范化 manifest、完整 light/dark 颜色、警告、当前 themeId/mode 和 revision；不保存原始 CSS。导入、选择、删除均按 expected revision 事务更新；同 ID 明确拒绝，默认主题不可删除，删除正在使用的自定义主题会原子恢复默认浅色。
- 导入先由 typed Rust command 对用户所选目录执行只读安全预检，取得规范化的精确目标，再重新解析并核对 ID 后写入；前端仍不读取主题文件。应用级 `ThemeMutationCoordinator` 和 `useAppTheme` 负责即时应用、跨事件/可见性重读、启动恢复、安全退出 flush、generation 隔离和失败后重读 SQLite 权威值。报错后的对账只有在 revision 恰好推进一次且目标主题、选择和存在性满足完整后置条件时确认已提交；数据库未变化才允许显式重试，并发冲突不自动重试，任意新增主题不能冒充本次 import。主题变量只写入主窗口 `.rr-main-app`；overlay 不实例化 ThemeService。
- 主应用中会随主题变化的输入区渐变、焦点边框、成功/危险状态色及写作页对比表面已改用既有语义 token 或基于 token 的 `color-mix`；功能性遮罩和中性阴影保留，未改变布局、字体、overlay 或交互。
- 自动验证通过：设置/主题/生命周期前端 31 项、会话前端回归 18 项、写作前端回归 26 项；完整 Rust 109 项通过，2 项真实 DeepSeek 联网测试按既有 ignored 标记跳过；`pnpm build`、`cargo fmt --check` 和 `cargo check` 通过。没有使用浏览器、Computer Use 或真实 Tauri 窗口替代人工验收。
- 本次三项审核返修验证通过：设置/主题/生命周期前端 35 项、Rust themes 专项 7 项，以及 `pnpm build`、`cargo fmt --check`、`cargo check`、RESOURCE_MAP YAML 解析和 `git diff --check`；完整 Rust 测试沿用上一轮 109 项基线，本次按审核要求未扩跑。没有使用浏览器、Computer Use 或真实 Tauri 窗口替代人工验收。
- 本轮审核返修统一了字符串长度语义：前端 `assertString` 与警告长度按 Unicode code point 计数（`Array.from(value).length`），与 Rust `chars().count()` 一致，含 emoji 等非 BMP 字符的 manifest 在安全预检与前端校验得到一致结果，并补了上限内通过/超限拒绝边界测试。设置、主应用和写作 CSS 中绕过主题协议的固定石墨色阴影（`rgba(38, 37, 30, …)` / `rgb(38 37 30 / …)`）已改用 `--rr-main-shadow` 或基于 `--rr-main-fg` / `--rr-writing-fg` 的 `color-mix` 派生语义变量，并扩充静态测试防止默认阴影色重新进入正式主题区域。聚焦验证：设置/主题/生命周期前端 36 项、Rust themes 8 项通过，`pnpm build` 通过；仍待独立审核与真实 Tauri 人工验收。
- Flexoki 已作为随包内置主题接入：从本机 Obsidian `Flexoki` 主题目录只读核对配色并映射到 ReadRayThemeV1 语义 token，名称/版本/作者/来源/许可证均取自实际文件（Flexoki 1.1.0，Steph Ango，MIT，stephango.com/flexoki），未引入 Obsidian 的布局、字体、插件变量或任意 CSS。内置主题由单个 `default_theme` 扩展为已知随包列表（`readray-default` 与 `flexoki`）：Rust 端用 `builtin_theme_ids()` 统一处理 ID 冲突、模式支持和删除拒绝；前端 `READRAY_BUILTIN_THEME_IDS` 同步，`validateThemeSnapshot` 校验所有已知内置主题与 canonical 完全一致并拒绝未知内置标记，`validateCustomTheme` 按目标 ID 精确定位。Flexoki 支持 Light/Dark 双模式，`ReadRay Default` 仍为默认主题；选择、模式切换、重启恢复、revision 与迟到结果隔离继续走既有正式链路。随包主题不可删除，自定义主题删除不受影响；Flexoki 配色中的浅色/深色 scrim 与 shadow 语义值保持规范化（深色用 `#000` 而非 `#000000`）。聚焦验证：设置/主题/生命周期前端 37 项、Rust themes 9 项通过，`pnpm build`、RESOURCE_MAP YAML 解析和 `git diff --check` 通过；未使用浏览器或 Computer Use，真实 Tauri 视觉验收留给用户。
- 已接入全部 28 个 Codex 预设主题作为随包内置主题：通过一次性只读脚本（`scripts/extract-asar.mjs`）从当前 Codex app.asar 动态定位并提取主题注册表（`app-initial--9zpGYoP.js` 内 `hCi` 数组）与各主题 chunk，按注册表权威映射保留真实 light/dark 可用性（Ayu/Dracula/Lobster/Material/Matrix/Monokai/Night Owl/Nord/Oscurange/Sentry/Tokyo Night/Temple 仅 dark，Proof 仅 light，其余双模式）。核心调色板经 `scripts/derive-themes.mjs` 确定性展开为完整 28-token 并生成 `scripts/codex-theme-extract/core-palette.json`，再由 `scripts/gen-themes.mjs` 生成 Rust（`src-tauri/src/codex_themes_data.rs`）与前端（`src/codexThemeData.ts`）两份字节级一致的完整配色数据，避免运行时派生的浮点分叉；不执行或导入原始 CSS/JS/TextMate scope。每个主题标注来源与许可证：16 个社区 MIT 开源主题（Ayu、Catppuccin、Dracula、Everforest、GitHub、Gruvbox、Material、Monokai、Night Owl、Nord、One、Rose Pine、Solarized、Tokyo Night、VS Code Plus、Xcode），12 个 OpenAI Codex 产品内置主题（Absolutely、Codex、Linear、Lobster、Matrix、Notion、Oscurange、Proof、Raycast、Sentry、Temple、Vercel，再分发许可未从 ASAR 确认，仅供本地内置使用并在 README 归属中标注）。Rust `builtin_theme_ids()` 与前端 `READRAY_BUILTIN_THEME_IDS` 现包含全部 30 个内置主题；`validateThemeSnapshot` 要求所有内置主题与 canonical 完全一致；内置不可删除、不可被自定义 ID 冲突覆盖，重启恢复与模式切换走既有链路。聚焦验证：设置/主题/生命周期前端 38 项、Rust themes 10 项通过，`pnpm build`、`cargo fmt --check`、`cargo check`、RESOURCE_MAP YAML 解析和 `git diff --check` 通过；未使用浏览器或 Computer Use，真实 Tauri 视觉验收留给用户。
- 主题区 UI 本轮收窄：鉴于已有 30 个随包内置主题，暂时移除设置页主题区的"导入主题"与"删除主题"按钮及对应说明文字，只保留随包主题的选择与 light/dark 模式切换，避免为导入/删除专门做额外检查。Rust 侧的导入/删除 command、前端 service/协调器与测试仍保留（未删除），仅不再暴露 UI 入口；相关静态测试已改为断言页面不直接调用 `themeController.importPackage`/`delete`。
- UI 细节修复三项：① 设置页主题模式下拉框（"浅色/深色"）此前 82px 宽 + 41px padding 且用系统原生箭头，文字被挤压截断；已改为 88px、padding-right 收紧到 26px，并加 `appearance: none` + 自定义 chevron，实测内容区 51px 足够容纳文字。② 主侧边栏增加 180–360px 的可拖拽宽度手柄（MainSidebar 右缘 resizer，pointer 事件 + setPointerCapture，经 onWidthChange 上报外壳通过 `--rr-main-sidebar-width` 应用，折叠态隐藏），设置页功能导航栏从固定 192px 调窄到 160px。③ 设置页所有 select 统一 `appearance: none` + 自定义 chevron、hover 边框过渡、主题色 focus 光晕和 pointer 光标，与文本输入框风格一致。验证：设置/主题/生命周期前端 38 项通过、`pnpm build` 通过、类型检查与 RESOURCE_MAP YAML 解析通过；未使用真实 Tauri 窗口，拖拽手感与最终视觉需人工验收。
- 侧边栏折叠冲突与动效已修复并于 2026-08-16 完成真实 Tauri 验收：折叠态不再沿用展开时的拖拽宽度，主内容完全铺开；固定收放不再直接把侧栏宽度切到 0，而是由独立布局槽与侧栏整体位移共用 260ms 缓动同步变化，使正文连续推开/收回。hover 预览时布局槽保持 0，只让侧栏作为覆盖层滑入，因此移开鼠标不会牵动主页面；整条左边缘不触发。展开态标题栏采用“正式应用 Icon + ReadRay + 右侧状态按钮”的平衡结构；折叠时品牌淡出，按钮贴随标题区右缘连续移动到左上角，之后由该按钮 hover 临时显示侧栏、点击恢复固定侧栏。按钮使用 panel-open/panel-closed 两枚图标明确表达当前固定状态，不再旋转同一图标。聚焦验证：设置/主题/生命周期 57 项、会话 28 项、写作 30 项和 `pnpm build` 通过。
- 主窗口边界已改为系统窗口语义：main 继续使用 `transparent: true`，但 `tauri.conf.json` 开启 `shadow: true` 使用 Windows 原生无装饰阴影；真实 `.rr-main-app` 改为 `position:absolute + inset:0`，不再在透明窗口内额外留 16px 阴影边界，最大化时由 `is-maximized` 去掉圆角。浏览器预览画布继续使用固定 1440×900、独立留白和柔和 CSS 阴影；overlay 仍保持 `shadow:false`。主窗口四周的自定义 resize 手柄保留。前端 build、Rust fmt/check 和桌面生命周期静态测试通过；最终截图选区、最大化贴边、还原和原生阴影观感仍需真实 Tauri 人工验收。
- 透明窗口边缘 resize：`transparent: true` 后 Windows 系统 resize 命中区失效（透明区不参与 hit-test），因此外壳在 `.rr-main-app` 四周渲染 8 个方向的自定义 `.rr-main-resize-handle`（onMouseDown 触发 `onStartResize`，App.tsx 里 `getCurrentWindow().startResizeDragging(direction)`），鼠标移到主窗口边缘即可缩放。关键点：必须新增 `core:window:allow-start-resize-dragging` capability（否则前端调用被拒并静默 catch），且用 `onMouseDown` 而非 `onPointerDown` + `preventDefault`。浏览器实测 8 个手柄位置与光标正确；真实拖拽手感需 Tauri 人工验收。
- 主题已通过独立审核与真实 Tauri 人工验收（2026-08-06）：30 个随包内置主题的列表、Light/Dark 模式切换、重启恢复、Flexoki 深色模式实际观感，以及透明窗口阴影、最大化圆角与边缘 resize 拖拽手感均已完成人工确认；主题基础设施收口。

### 任务 1：流式输出（已完成并验收通过）

- 任务书与验收记录详见 `docs/AGENT_UPGRADE.md`。Quick AI 对话已从一次性返回升级为 Tauri `ipc::Channel` SSE 流式输出，支持真实可停止；channel 只推送 delta/done/stopped/error 四类事件。
- 新命令 `send_quick_ai_message_streaming` 复用 `prepare_turn → 流式请求 → complete_turn` 链路，消息持久化语义（user 先落库、assistant 保持 pending、幂等重试、崩溃恢复）与现状完全一致；停止后保持 pending 可重试，不伪造完整回答。旧 `send_quick_ai_message` 非流式命令保留。
- 停止采用 conversation 级 abort 原子标志 + active 流门控；usage 在流末尾 chunk 严格校验后尽力写入（QuickAi 分类），合法 usage 即使业务失败也计入，统计失败不影响业务结果。
- 修复流式 usage 解析 bug：`stream_quick_ai_reply` 原先把 SSE 最终 chunk 的 usage 对象本身传给期望"带 usage 键完整响应体"的 `parse_model_token_usage`，导致**每次**真实流式回答都报"缺少 usage"；新增 `parse_model_token_usage_value` 直解析 usage 对象，流式路径改用它，原函数保留给非流式。`deepseek-v4-flash` 为推理模型（`reasoning_content`/`reasoning_tokens`），极简输入下可能"纯推理零内容"缺 usage，按用户方案 1 降级为不记录使用量、仍保存回答。
- 验收：真实 Tauri 边生成边显示、可停止、重试恢复均正常；Rust 124 项通过（含新增 2 项）、会话前端 24 项通过、`pnpm build` / `cargo fmt --check` / `cargo check` 通过。对话页视觉随流式调整（消息列/输入框容器查询宽度、字号微调）一并确认无 bug 隐患，保留。

### 任务 2：对话页面 Markdown 渲染（已完成并验收通过）

- 任务书与验收记录详见 `docs/AGENT_UPGRADE.md`。新增轻量自研白名单 Markdown 解析器 `src/markdownParse.ts`（无依赖、类型化 token、Node 可测）与渲染组件 `src/components/MarkdownContent.tsx`；白名单子集覆盖段落/标题/粗体斜体删除线/行内与多行代码/列表/引用/分隔线/链接（仅 http/https 且不可点击），表格与 HTML 一律降级纯文本，输入永不作为 HTML 注入（React 转义 + 解析层协议白名单双层防护）。
- assistant 消息新增可选 `markdown` 字段（真实回答原文），页面有则优先渲染、否则回退既有 blocks 协议，fixture 预览路径零改动；流式采用"拼接后整体渲染 + 未闭合降级"（streaming 模式未闭合代码块按代码块渲染、未闭合行内标记隐藏起始符号，不闪现原始标记）。
- `QUICK_AI_SYSTEM_PROMPT` 联动："不要依赖 Markdown 渲染"改为"可使用简洁 Markdown 结构化输出"，诚实边界（不声称访问互联网/本地学习记录/长期记忆）保留。
- 验收后修复（2026-08-07）：① 代码块/文本块不换行（`white-space: pre` 只横向滚动，改 `pre-wrap + overflow-wrap: anywhere`）；② 流式生成中不换行（`.rr-conversation-generation-row` 的 `nowrap` 继承，改 `normal`）；③ 正文软换行丢失与超长内容溢出（补 `pre-wrap` / `overflow-wrap: anywhere`）。
- 后续性能优化（2026-08-07）：`finish_reason=length` 截断误报修复（降级为 truncated 状态保留已生成内容）；`QUICK_AI_MAX_TOKENS` 2048 → 8192（DeepSeek 文档上限，实测 v4-flash 接受）；流式渲染性能优化（去掉 `text-wrap: pretty`、滚动距底部 <80px 才跟随），200 词段落生成从约 1 分钟降至约 16 秒。
- 验收：渲染器 20、会话 25、Rust 126 项通过，`pnpm build` / `cargo fmt --check` / `cargo check` / `git diff --check` 全绿；真实 Tauri 生成速度与渲染观感用户实测确认，任务 2 收口。

### 任务 3：系统提示词构建（已完成并验收通过）

- 任务书、研究结论与验收记录详见 `docs/AGENT_UPGRADE.md`。基于 Claude Code / Codex / OpenCode / Pi 源码研究（Workflow 8/9 成功），把 Quick AI 系统提示词从单一常量重构为组合式分节构建。
- 新建 `src-tauri/src/quick_ai_prompt.rs`：5 个分节常量（persona → behavior → output_format → boundaries）+ `<readray_context>` 动态插槽 + `QuickAiDynamicContext` 空结构体（预留 learning_profile/recent_memory）+ `build_quick_ai_system_prompt()` 按静态→动态组装。`quick_ai.rs` 删常量、调用 builder，`messages[0].role=="system"` 保持。
- 诚实边界升级为"负面（无网络/无工具/无本地记忆）+ 正面替代 + 回退行为（不知道就诚实说并提供最近可行替代，不虚构词典释义/翻译/考试事实）"；output_format 精确对齐 `src/markdownParse.ts` 白名单（支持清单 + 表格/HTML/四级+标题/图片负面清单 + http/https 链接约束）；推理模型非空规则（永不返回空回答、推理不外显）缓解"纯推理零内容"边界；不注入日期。
- `deepseek_client.rs` 的 `StreamChunk` 新增 `reasoning` 捕获（`delta.reasoning_content`，与 content 严格分离不混入回答），`quick_ai.rs` 仅捕获验证丢弃并打诊断日志 `READRAY_QUICK_AI_REASONING_SEEN/ONLY`（纯推理零内容可观测）。
- 解释卡与写作分析提示词保持独立未并入（一致性重构留作后续可选）。
- 验收：用户真实 Tauri 确认效果提升；Rust 137 通过 / 4 ignored、前端 25/30/38 全绿、build/fmt/check/diff 通过；4 项真实 DeepSeek live 测试（白名单、诚实边界、两轮上下文、解释卡回归）通过。

### Quick AI 浮层体验升级（任务 7 已完成）

- 浮层对话窗口已调整为约 `780 × 500px`，面板铺满原生窗口；输入栏密度、发送按钮和多行向上扩展已收紧，同时保留中文输入法和现有发送快捷键语义。
- 浮层现已分离显示状态、页面状态和活动会话：失焦只隐藏，重新呼出恢复页面、会话、草稿与合理滚动位置；对话页 Esc/返回按钮回到搜索入口，搜索入口 Esc 隐藏。隐藏不会取消流式生成，迟到结果继续按会话和请求身份隔离。
- 历史按钮提供新对话、Overlay 最近会话和完整历史；Overlay 历史只读取 `overlay` 来源，主窗口侧栏只读取 `main` 来源，主窗口全部历史仍统一归档两类会话。SQLite v10 已删除无法追溯且仅用于测试的 `legacy` 会话，后续创建入口只允许 `main` / `overlay`。
- 任务 7 自动回归已覆盖窗口/抽屉外壳、单多行输入、失焦隐藏恢复、两层 Esc、返回、历史切换、`Ctrl+N`、流式停止恢复及 loading/error/stopped 状态。Overlay 8 项、会话 27 项、写作 30 项、设置/主题/生命周期 43 项通过；Rust 143 项通过、4 项真实联网测试按既有规则忽略；生产构建、Cargo fmt/check、RESOURCE_MAP YAML、SQLite v10 完整性和 `git diff --check` 均正常。用户已于 2026-08-13 确认真实 Tauri 视觉与交互验收通过，任务 7 正式完成。

### 阶段八点五任务 1/2（Rust Agent Kernel 与通用对话接入，已通过评审）

- 任务 1 交付无持久化副作用的最小循环内核（`src-tauri/src/agent_runtime/`）：`coordinator.rs` 的 `AgentRunCoordinator` 只经 `AgentEventSink` 发布事件，不写 SQLite、不注册 command；`run()` 契约是"所有声明失败以 `Ok(outcome)`+唯一终态事件收尾，仅 sink 拒绝返回 Err"；运行期工具失败（ToolExecutionFailed/ToolTimeout）可恢复——失败结果按原调用顺序回传模型、继续下一 turn（预算/超时兜底），授权/schema 失败（未知工具/策略拒绝/schema 无效）保持 fail-fast；`context.rs` 组合式 capability-aware 系统提示词；`gateway.rs` 定义 `ModelGateway` trait 与 `ProviderMessage` 投影；`tool.rs`+`tool_schema.rs` 提供 ToolRegistry/ToolPolicy/风险分级与白名单 JSON Schema 子集（默认拒绝未知字段）；`fake_gateway.rs` 仅测试编译，确定性覆盖全部场景。三类预算（模型轮数/工具总数/单批并行）与 run 超时、取消均由内核强制；`max_context_bytes`/`tool_timeout_ms` 尚未强制（任务 5 与真实异步 executor 职责，代码已标注）；瞬态 provider 错误重试未实现（协议常量保留，任务 2 未启用）。
- 任务 2 把 Runtime 接入主应用完整对话与 Quick AI overlay 共享的会话权威：SQLite v20 新增 `agent_runs`/`agent_steps`/`agent_sources`（纯新增表，CHECK 约束：authority 身份完整性、status 枚举、`completed ↔ completed_at`；`retry_of_run_id` 自引用 FK；step/source 按身份幂等写入、级联清理；迁移集中设计、带 v19→v20 升级保留旧数据测试）；`ChatSurfaceAdapter` 复用 `prepare_turn/complete_turn` 幂等边界，assistant 已落库则重试返回权威快照并对账 run 终态（模糊成功）；`PersistingSink` 只推进中间状态（prepared→model_streaming→tool_running→synthesizing），终态由调用方在业务写入成功后落库，completed 严格对应已持久化的最终 assistant；`DeepSeekChatGateway` 走已验证的 chat/completions（复用 `deepseek_client` 的 SSE 解析与 `shared_http_client`，流式 tool_calls 跨分片折叠，`ChatCompletionStreamer` 注入可离线验证；usage 尽力记录且"存在但非法"只降级不杀 run；无工具时省略 tools 键）；`send_quick_ai_message_agent` 命令在 `quick_ai.rs` 注册，同步内核跑在 `spawn_blocking`，事件经 `AgentUiSink` 映射回既有 `QuickAiStreamEvent` 协议（前端统一 envelope 展示延后）；run_id 含 pid+unix_ms+计数器，重启后不碰撞；持久化/UI sink 失败先落 `failed(persistence_failed)` 再返回错误，pending user 保留可重试。
- 待验收项：旧非 Agent 路径（`send_quick_ai_message`/`send_quick_ai_message_streaming`）保留为受控回退；DeepSeek Responses API 因 live spike 返回 400 未确认，任务 2 及以后继续以 chat/completions 为依据；任务 2 全部验收由离线测试达成（fake gateway + 真实 SQLite），首次真实 DeepSeek 请求验证已在任务 3 的真实使用中完成（多轮追问、联网搜索、失败恢复），任务 2 链路随之获得真实验证。
- 分支纪律：任务 1-6 全部在 `agent-runtime` 分支累计实现（任务 1 起名 `feat/agent-runtime-task1`，2026-08-17 改名），每个任务经调度者评审后进入下一个；全部完成后再一次性合入 main。当前分支 tip `ceccd96`（任务 3 收口，含评审修复与真实使用修复），main 仅含 `a15db94`（learning_records v17 回填测试修复已 cherry-pick 并推送，main 全绿 303/0）。任务 2 收口时验证：全量 Rust lib 334/0、agent_runtime 84/0；任务 3 收口时全量 Rust 372/0、前端 conversation 33/33、build/fmt/check/diff 全绿。

### 阶段八点五任务 3（自动联网纵切，已正式收口）

- **Provider 决策（2026-08-17 live spike）**：重跑 `READRAY_RUN_DEEPSEEK_RESPONSES_SPIKE=1` 仅 Responses spike——`/responses` 仍 400；补充 chat/completions `tools:[{"type":"web_search"}]` 变体探测，错误明确 `unknown variant 'web_search', expected 'function'`：当前 DeepSeek 端点只接受 function 工具，内置 server-side web_search 不存在，来源要求不满足。经调度者确认：按方案 §14.1 在相同 `web_search` ToolDefinition 后替换受控 provider，不改变 Agent loop 与 UI 协议；本机无搜索服务 key，采用 Wikipedia API 无 key provider + 完整受控 fetch_web_page。
- **网络模块 `src-tauri/src/agent_runtime/network.rs`（任务 3 新增）**：`SearchProvider` trait 可替换（未来 Tavily 等只换实现）；WikipediaSearchProvider（zh/en search API，诚实标注"维基百科覆盖，非通用搜索"）；`WebFetcher` 逐跳重新校验 URL 与解析后全部 IP、连接固定到已验证 IP（防 DNS rebinding）、重定向 5 跳/2 MiB/连接 10s 整体 30s/文本内容类型白名单/无 cookies/剥离 script 等 active content；来源以 `details.sources` 携带，source_id 为不暴露 URL 的确定性 hash；`validate_fetch_url` 拒绝私网/回环/保留网段/云 metadata/userinfo/敏感查询参数。
- **内核接线**：`conversation_l1_tools` 注册 web_search/fetch_web_page（L1 ExternalReadOnly），对话 capability 提升到 ExternalReadOnly（`conversation_capability`）；coordinator 在工具完成时把 `details.sources` 投影为 SourcesUpdated（先于 ToolCallCompleted）；PersistingSink 从 ToolCallCompleted/Failed 提取来源落库并关联 tool_call_id（移除空串落库）；protocol.rs 未改。AgentUiSink 事件映射抽为 `project_ui_event` 纯函数，QuickAiStreamEvent 扩展 `sources_updated`/`tool_state`（"正在搜索相关资料…/正在读取网页内容…/正在整理答案…"）；`open_agent_source` command 校验 HTTP(S)/凭据/保留网段后交受控 opener。
- **前端**：正式对话链路切换到 `send_quick_ai_message_agent`（旧 streaming 命令保留为受控回退）；ConversationPage 展示来源卡片（`AgentSourceList.tsx`，标题/站点/URL，点击受控打开）与工具状态文案；来源/状态以 ref 为权威累积（修复来源被后续 setGeneration 覆盖的缺陷，streaming 完成态合并来源到最终 assistant）；fixture 增加 `[fixture:sources]` 演示（沿用 `[fixture:slow]` 模式）。
- **验证**：Rust lib 362/0（网络 15 项、来源事件/落库/UI 投影等新增约 28 项；6 ignored 为既有 live）、前端 conversation 30/30 与其他套件全绿（overlay 20/review 50/writing 30/settings 57/startup 3）、tsc/vite build、cargo fmt/check --all-targets、YAML 解析、git diff --check 全通过；浏览器预览（vite preview + fixture）实测来源卡片渲染（生成中与完成态）、"正在整理答案…"状态与点击无错误。未运行其他 live 测试；真实 Tauri/DeepSeek 人工验收（模型自主搜索、停止贯穿、网络离线/限流）留待评审后停点。
- **评审修复轮（2026-08-17，已复审通过）**：① 真实使用发现多轮对话 400——历史 assistant 投影为 `tool_calls: []`（DeepSeek 要求长度 ≥1），`project_message` 空 tool_calls 省略该键（调度者直接修复 `59791be`）；② 失败/中断后 composer 被 `generation !== null` 锁死且失败态只有"重试"无出口——改为仅 generating 阻塞发送，`prepare_turn` 允许尾 pending user 时以 `current_max+2` 开启新轮次（旧 pending 保留可审计、重试复用同 sequence/id、stale 拒绝），未加"放弃"按钮（用户要求根因解决）；③ fetch_web_page 运行时失败归 `NetworkBlocked` 会 fail-fast 杀死 run——运行时失败（DNS/传输/非 200/内容类型/重定向超限）归 `ToolExecutionFailed` 可恢复，`NetworkBlocked` 只留安全拒绝（SSRF/私网/凭据/重定向到非 HTTP(S) 协议）；④ Wikipedia 解析失败被当"无结果"——区分三态（非法 JSON/缺列表 → ToolExecutionFailed；空列表 → 无结果）；⑤ `stable_source_id` 收敛为 network.rs 共享实现；⑥ 全局网络权限门（§5.3）本轮只标注边界不实现，未来经 app_preferences 回落 L0。真实使用另暴露并修复：DeepSeek 拒绝无 `type:"object"` 的 function schema、多工具轮次 `ToolRunning→ToolRunning` 自环缺失导致非法迁移、Wikipedia 无 User-Agent 返回 403（统一 `READRAY_USER_AGENT`）、模型对"能联网吗"保守回答"不能"（有网络工具时提示词据实声明联网能力）。
- **收口（2026-08-17）**：用户已完成真实 Tauri 人工验收——多轮追问不再 400、一轮失败后直接输入新消息可发出、联网搜索展示来源卡片可点击打开、"你能联网吗"据实回答"能"。收口时全量 Rust 372/0、前端 conversation 33/33、`pnpm build`/fmt/check/diff 全绿；任务 3 正式收口，下一步进入任务 4。

### 阶段八点五任务 4（日常使用交互，已正式收口）

- **SQLite v21（集中一次）**：`quick_ai_messages` 追加 `superseded_by_id`（替代链）、`sources_json`（来源随 assistant 落库）、`truncated`（finish_reason=length 截断标志）三列；带新库约束测试与 v20→v21 旧库升级数据保留测试（沿用项目"回滚测试"模式）。
- **编辑并重新生成（2026-08-18 设计变更修复轮，用户确认方案 B）**：交互为"最后一条用户输入下方的 hover 小按钮 → 行内编辑 → 发送 → 新回答替代旧回答"，仅最后一条输入展示入口；菜单内"重新生成回答"入口与 `canRegenerate` 死代码已移除。编辑后的问题以新消息行替代旧问题行（方案 B：复用 v21 `superseded_by_id`，被替代者记录指向替代者，与 v21 方向一致，**无新 migration**），新回答替代旧回答；旧问题+旧回答均保留在库可审计；可见快照与导出只取未替代消息；序列保持 user 奇/assistant 偶。`prepare_regeneration` 插入编辑后 pending 问题行（下一个奇数），`complete_regeneration` 在同一事务写新答（下一个偶数）+ 标记旧问/旧答被替代；失败时旧问+旧答仍可见、编辑 pending 行保留可重试（重试复用同一行；同一 pending 行再次编辑成不同内容时 UPDATE 覆盖其 content——pending 行是本次编辑产生、尚无回答，覆盖无历史价值损失）；run 复用该轮 user 身份（retry_of_run_id 指向该轮最近 run）；模型上下文排除旧问+旧答、以编辑后问题为尾。
- **错误呈现友好化**：失败分支技术细节进 `eprintln!("READRAY_AGENT_RUN_FAILED=…")`，UI 只显示友好文案（"暂时无法回答，请重试"/"回答已停止…"/"回答未完成…"）；`project_ui_event` 的 RunFailed 不再透出技术原文；前端 failed 气泡与打开失败路径同规则。
- **来源持久化回看**：SourceCollectingSink 按 source_id 去重累积 SourcesUpdated，随 complete 事务写入 assistant 行 `sources_json`；快照返回 `sources`（camelCase），损坏 blob 记日志降级不阻断加载；重启与历史对话来源卡片直接回看，不从 agent_sources 审计表重建。
- **截断诚实提示**：coordinator 捕获最终轮 `ModelFinishReason::Length` → `RunOutcome.truncated`（protocol.rs 冻结协议未改）；回答照常持久化 + truncated 标志，前端消息显示"回答可能不完整"轻微提示，删除"继续生成"按钮；预算截断保持 pending 可重试。
- **验证与收口（2026-08-18）**：Rust lib 387/0、前端 conversation 38/38 与其他套件全绿、`pnpm build`/cargo fmt/check --all-targets/`git diff --check` 全通过；浏览器预览实测最后一条输入 hover 编辑按钮 → 行内编辑 → 发送后回答原地替换、菜单仅剩导出/删除。用户已完成真实 Tauri 人工验收：最后一条输入 hover 出现编辑按钮、行内编辑并发送后回答原地替换、来源随历史回看可用、"回答可能不完整"截断提示正确。任务 4 正式收口，下一步进入任务 5 长上下文与 compaction；截断继续功能归任务 5。

### 阶段八点五任务 5（长上下文与 compaction，已正式收口）

范围按用户确认的方案 A 大幅简化：ReadRay 是文字阅读/学习工具（非 coding agent），对话 token 增长慢，且 DeepSeek V4 上下文为 **1M tokens**，正常对话几乎吃不满窗口，因此**不做完整 compaction 子系统**，只做"让用户永远感受不到上下文限制"的最简兜底。折叠摘要只存在投影/内存层，不落库、不做 compaction 持久化表、不新增 migration（`DATABASE_SCHEMA_VERSION` 保持 21）。

- **预算驱动投影**：`chat_surface.rs::transcript` 把固定 `MAX_CONTEXT_MESSAGES=40` 改为 token/字符预算驱动（`estimate_text_tokens` 字符数/4 向上取整），`max_suffix_fitting` 从尾部向前累积能放下的最近完整后缀，**永远保留当前 pending user、永不从开头断**；预算 = 1M 窗口 − 50k 安全余量。正常快速路径（完整历史在预算内）直接全量投影，绝大多数真实对话走这里。
- **折叠兜底（方案 A + 方案三）**：超出预算时最旧一段被确定性折叠为 `CompactionSummary`（不调用 LLM，保留最近若干条 user 问题摘录 + 如实说明共 N 条较早消息被折叠，不编造语义总结）；经用户确认**投影为真实 user 历史消息**（`deepseek_gateway.rs::project_message` 映射为 `user`），并紧跟空 assistant 配对维持 user/assistant 交替，措辞标"较早已折叠、仅供回顾参考、以当前对话为准"——既避开消息数组中第二条 system 的 shape 风险（项目曾因 `tool_calls:[]` 踩 DeepSeek 400），又不把过时内容抬成全局权威背景。折叠失败（空段/无 user 摘录）或摘要+配对+尾部超预算时安全回退为只投影最近完整尾部，不假装成功。
- **持久化与隔离**：摘要只在 `transcript()` 投影层生成，`snapshot.messages` 原始历史完整不删改；无 migration、无 compaction 落库；摘要与阶段九学习者记忆无数据或语义混用。
- **共享面**：主应用对话与 Quick AI overlay 都经 `ChatSurfaceAdapter::transcript`（正式路径 `send_quick_ai_message_agent`），共享同一预算投影；旧 `send_quick_ai_message`/`_streaming`（含旧 40 条截断）作为受控回退保留，前端正式链路只走 Agent 版本，不受影响。
- **验证与收口（2026-08-18）**：Rust lib 392/0（新增 5：全部投影/折叠/保留 pending/失败回退/token 估算，含方案三投影验收：单条 system、摘要为 user 且措辞标"仅供回顾参考/以当前对话为准"、末尾 pending、严格 user/assistant 交替）、cargo check 0 警告、cargo fmt 通过；评审修复轮把初版的第二条中间 system 投影改为方案三（user 历史消息 + 配对 assistant + 投影验收测试），已复审通过。未运行 live 测试；真实 Tauri 长对话人工验收（本场景接近 1M 窗口，日常几乎不会触发）留待整体停点。任务 5 正式收口，下一步进入任务 6 Writing Coach 适配。

### 阶段八点五任务 6（Writing Coach 适配，已正式收口，阶段八点五全部完成）

范围按用户确认合并两项产品体验收敛：解决"检查慢"（流式进度状态 + 可取消，不逐字渲染结构 JSON）与修正检查方式（B(1)：只改 prompt 语义、不动 schema/不迁移——先抓明显语法问题；表达类由模型从文章内容推断场景、给单条场景判断建议，不并列"正式/地道"两个选项）。不新增"放弃"按钮（中止并入检查按钮，符合用户既有偏好）。

- **共享内核接入**：新增 `writing_surface.rs`（WritingSurfaceAdapter 最小适配层：投影 `[System, User]` + 写作专属空工具集，默认不联网、L0）；`writing.rs` 分析/问答从旧非流式 `post_tracked_chat_completion` 改为经 `AgentRunCoordinator` + `DeepSeekChatGateway::for_surface` + `WritingSurfaceAdapter` 驱动，最终收口结构化 JSON，经 `parse_writing_analysis_content_salvage` 校验与 `save_analysis_if_current` 的 expectedRevision 事务保存（写作权威保留）；`AuthorityRef::writing(document_id, expected_revision, snapshot_digest, ...)` 承载身份。
- **JSON 面参数**：gateway 增加 `for_surface(operation, category, output_json_object)` / `with_generation_params` / `with_thinking_disabled`；写作面强制 `response_format: json_object` + 关闭思考 + 4096/0.2。真实联调发现 `deepseek-v4-flash` 开启思考会把全部输出预算烧在 `reasoning_content` 上（正文为空），故 JSON 面必须关思考，加 `READRAY_JSON_STREAM_DIAG` 诊断。
- **流式状态与取消**：`WritingStreamEvent`（status/done/stopped/error）；`WritingUiSink` 把 AgentEvent 投影为友好进度（"正在检查语法…/正在判断表达是否地道、更正式…/正在整理检查结果…"）；按 document_id 键控 abort flag + `abort_writing_analysis` command；前端检查按钮 checking 时显示进度文案、title"停止检查"、点击中止（删除单独的"放弃"按钮，用户此前明确不增加按钮）。
- **prompt 语义修正（B(1)）**：`writing_analysis_system_prompt` 重写——先抓明显语法问题；表达类由模型推断场景给单条建议（不并列两选项）；强化逐字复制（source/targetText 原文逐字、改写只进 reference、标题不属于检查正文）；输出精炼（字段长度上限）但**不得漏报**（漏报比多报糟、学习者草稿典型 4-6 个真实问题、空数组仅当无可挑剔）；issues 为空或极少时仍给 1-2 条真实可迁移要点。
- **诚实部分成功**：`parse_writing_analysis_content_salvage` 丢弃无法在正文中逐字定位的个别问题（记日志）、保留合法问题，避免整次检查作废；无合法问题时不伪装成功。
- **验证与收口（2026-08-18）**：Rust lib 405/0（新增 13）、cargo check 0 警告、cargo fmt 通过；前端 writing 30/30 与其他套件全绿（conversation 38/settings 57/overlay 20/review 50/startup 3）、`pnpm build`、tsc 通过。评审修复轮：① 死代码警告加 `#[cfg(test)]`；② prompt 收紧误伤（初版 "high-value/never padding" 措辞导致模型返回空 issues）改为"漏报比多报糟 + 典型 4-6 个 + 空 issues 仍给要点"；③ 删除"放弃"按钮（中止并入检查按钮）。**已知问题（暂不处理，已记录）**：真实使用中检查曾返回 0 个问题（示例文章 "What is Love ?"，用户感知"和没审查一样"）——prompt 已强化但未经真实 Tauri 复验；若复验仍异常需重新评估关闭思考模式对检查质量的影响。任务 6 正式收口，**阶段八点五 Agent Runtime 升级全部完成**，等待合入 main。

## 下一步

ReadRay Agent Runtime 的完整阶段八点五方案已于 2026-08-16 写入 `docs/AGENT_RUNTIME_UPGRADE_PLAN.md`，并同步到 `DEVELOPMENT_PLAN.md` 与 `RESOURCE_MAP.yml`。方向是参考 pi Agent Core 的循环、事件、工具和上下文协议，在 Rust/Tauri/SQLite 内实现原生共享 Runtime；不直接嵌入 pi coding-agent，不增加 Node/Python sidecar，也不开放 Bash、任意文件读写或动态代码扩展。主应用完整对话与 Quick AI overlay 共享现有会话后端和 ChatSurfaceAdapter，属于首要迁移面；Runtime 稳定后，作文中的 Writing Coach 通过独立 adapter 接入，同时继续以 `writing.rs`、文章 revision、完成版本和现有请求身份为权威。模型在用户已授权的能力集合内自主选择低风险只读工具，运行时负责权限、预算、来源、取消、幂等和恢复。

阶段八点五任务 0 已于 2026-08-16 落地协议草案、测试专用 deterministic fake-provider replay 和默认忽略的 DeepSeek Responses API spike，并通过协议评审。`src-tauri/src/agent_runtime/protocol.rs` 冻结了 surface-neutral authority、统一事件 envelope、tool call/result、错误/终止分类、第一版预算和只留内存的 provider continuation state；`ToolCallCompleted/Failed` 的结果方向与 `tool_name` 均有校验，terminal reason/error 矩阵也已冻结；replay 覆盖无工具、单/多工具（controlled completion-order fixture，不声称真实并行）、未知工具、坏参数、失败、超时、model/tool abort 与三类独立预算上限。DeepSeek Responses 按 stateless 处理，只观察 provider response ID，不使用 `previous_response_id`；parser 严格配对 `event:` 与 `data.type`，只观察 function/web-search action、SSE sequence/终态和 usage；只有 provider 明确返回 citation/annotation 时才记录来源。2026-08-17 修复了 live spike 未加载项目根 `.env` 的问题（此前因未提供可用 key 在发出请求前停止），并用 `.env` 中的可用 key 显式执行一次：请求已真实发出，但 DeepSeek `POST https://api.deepseek.com/responses` 对当前请求形状返回 HTTP 400，错误 body 按设计不读取，本次无法区分端点/请求体/账户作用域原因；web_search 行动与来源元数据未能 live 确认，内置 Web Search 是否满足来源要求仍未验收，该开放问题移入任务 3 自动联网纵切时重新验证。

任务 1（Rust Agent Kernel）与任务 2（通用对话接入与 SQLite run/step 恢复）已分别于 2026-08-17 完成实现并通过调度者独立评审（各含一轮修复复审）。任务 1 交付无持久化副作用的最小循环内核：`coordinator.rs`（AgentRunCoordinator，run() 契约：声明失败一律以 `Ok(outcome)`+唯一终态事件收尾、仅 sink 拒绝返回 Err；运行期工具失败可恢复、授权/schema 失败 fail-fast）、`context.rs`、`gateway.rs`、`tool.rs`+`tool_schema.rs`、`fake_gateway.rs`（仅测试）；事件序列逐项通过 `validate_event_sequence`。任务 2 新增 SQLite v20（`agent_runs`/`agent_steps`/`agent_sources`，CHECK 约束：authority 身份完整性、status 枚举、completed↔completed_at；run 状态迁移由 repository 校验，终态冻结）、`chat_surface.rs`（ChatSurfaceAdapter 复用 `prepare_turn/complete_turn` 幂等边界，含模糊成功对账）、`deepseek_gateway.rs`（chat/completions 真实 gateway，`ChatCompletionStreamer` 注入可离线验证；Responses API 因 400 未确认不作为依据）、`run_repository.rs`（PersistingSink 只推进中间状态，终态由调用方在业务写入成功后落库）与 `send_quick_ai_message_agent` 命令（spawn_blocking 同步内核 + 既有 QuickAiStreamEvent 协议映射）。恢复语义按 §17：最终 assistant 已落库则重试返回权威快照并对账 run 终态；仅 pending user 则创建 `retry_of_run_id` 新 run；run_id 含 pid+unix_ms+计数器保证重启不碰撞；持久化/UI sink 失败先落 `failed(persistence_failed)` 再返回错误。任务 2 验收全部达成（重试不重复 user/assistant/tool result、停止/崩溃/重启后 pending user 可恢复、completed 与最终 assistant 严格一致、迁移/回滚/旧库审计有测试）。

任务 3（自动联网纵切）已于 2026-08-17 完成实现、验证、浏览器预览、评审修复轮（空 tool_calls 400、composer 锁死、fetch 失败杀死 run、Wikipedia 解析/UA、schema type、ToolRunning 自环、联网诚实回答等全部修复并通过复审）与真实 Tauri 人工验收（多轮追问不再 400、失败后直接发新消息、联网来源卡片、"能联网吗"据实回答），**正式收口**。Provider 决策：live spike 证实 DeepSeek 内置 web_search 不存在（Responses 与 chat/completions 均 HTTP 400，chat/completions 明确 `unknown variant 'web_search', expected 'function'`），按 §14.1 换受控 provider——本机无搜索 key，采用 Wikipedia API 无 key provider + 完整受控 fetch_web_page（`agent_runtime/network.rs`：SearchProvider 可替换 trait、SSRF/DNS rebinding/重定向/大小/内容类型/隐私防护）。L1 工具注册（web_search/fetch_web_page，ExternalReadOnly）、对话 capability 提升、coordinator 把工具 `details.sources` 投影为 SourcesUpdated、PersistingSink 来源落库关联 tool_call_id；前端正式对话切到 `send_quick_ai_message_agent`，来源卡片（AgentSourceList.tsx）与"正在搜索/正在读取/正在整理"状态经 `sources_updated`/`tool_state` 事件展示，来源打开走受控 `open_agent_source`。任务 4（日常使用交互）已正式收口（2026-08-18）：编辑并重新生成（2026-08-18 设计变更修复轮——hover 入口 + 行内编辑，方案 B 新问题行替代旧问题行，复用 v21 无新 migration）、错误呈现友好化（READRAY_AGENT_* 日志 + 友好文案）、来源随 assistant 落库回看、截断诚实提示（finish_reason=length 照常持久化 + "回答可能不完整"，不做继续生成，截断继续归任务 5）。任务 5（长上下文与 compaction）已正式收口（2026-08-18）：预算驱动 `chat_surface.rs::transcript` 替换固定 40 条截断（token/字符估算 + 50k 余量，永不从头断、永远保留 pending user）；超窗时最旧段折叠为 `CompactionSummary` 极简摘要，经用户确认方案三投影为**真实 user 历史消息**（非第二条 system，规避中间 system shape 风险且不把过时内容抬成权威背景），空 assistant 配对维持交替、措辞标"仅供回顾参考/以当前对话为准"；摘要只存投影/内存层，不落库、不新增 migration（schema 保持 21）、不进学习者记忆，原始消息永不删除；折叠失败/超预算安全回退。任务 6（Writing Coach 适配）已正式收口（2026-08-18）：写作分析/问答经 `WritingSurfaceAdapter` + `AgentRunCoordinator` + `DeepSeekChatGateway::for_surface` 复用共享 Runtime 内核（写作权威仍归 `writing.rs` 的 expectedRevision 事务）；JSON 面强制 `response_format: json_object` + 关闭思考（live 发现推理模型烧预算导致空正文）+ 4096/0.2；流式友好进度（"正在检查语法…/正在判断表达是否地道、更正式…"）与按 document_id 取消（检查按钮 checking 时点击即停止，无新增"放弃"按钮）；prompt 重写——先抓明显语法问题、表达类按文章场景给单条建议（不并列正式/地道两选项）、逐字复制约束、字段精炼但不得漏报；`parse_writing_analysis_content_salvage` 诚实部分成功。**阶段八点五 Agent Runtime 升级全部完成**。已知问题：真实使用中写作检查曾返回 0 个问题（示例文章 "What is Love ?"），prompt 已强化（漏报比多报糟 + 空 issues 仍给 1-2 条可迁移要点）但未经真实 Tauri 复验，已记录待后续处理（可能需重新评估关闭思考模式的影响），不阻塞合入 main。阶段九继续暂停，学习者画像、长期记忆、记忆注入和个性化工具不属于阶段八点五。

阶段八与阶段九前的“学习目标聚合”均已完成独立审核、自动验证和用户真实 Tauri/SQLite 人工验收。真实数据库已成功登记 v19，迁移保护性审计、历史兼容身份、Memory 去重/查询次数/历史出现、Review 同目标去重与搜索相关度均已通过实际使用确认；本任务正式收口。阶段九继续暂停，不得在用户明确前扩展到画像、记忆注入、个性化排序、效果评估或语义聚类。

当前验证：初始实现的 Rust lib 234/234、前端 Review 50/50、Memory 3/3、生产构建、Cargo fmt/check、YAML 与 diff 检查均通过；返修阶段按不重复整套测试的约束，补跑 active 槽、卡池隔离、v19 attempt/追溯、跨日兼容迁移/重启幂等、rollback、Memory/Review 隔离和搜索相关度等聚焦测试，均通过。用户已于 2026-08-14 完成真实 Tauri/SQLite 人工验收。

Quick AI 浮层升级任务 7、主窗口静态品牌启动层、WebView 默认右键菜单屏蔽及写作辅助编辑型对话 UI 均已于 2026-08-13 通过用户真实桌面验收。

窗口尺寸/位置/最大化恢复和主侧栏宽度记忆已于 2026-08-15 通过用户真实桌面验收；这些持久化能力没有因 2026-08-16 的缩放视觉返修而回退。后续恢复时不要把 1440×900 设计基线误当成固定启动尺寸。

侧栏窄窗自动收放与 resize 逐帧布局修复（2026-08-16）在首轮真实 Tauri 视频中仍有明显黑/白色带，已追加透明合成、主题同色原生/WebView 底板和窗口级 120ms settle。自动验证已通过，正等待第二轮真实 Tauri 拖拽手感人工验收；验收标准是允许 Codex 参考视频级别的一两帧内容追赶，但不再出现强烈高对比闪动。

- 当前主应用对话已具备能力感知的事实型本地学习历史读取：仅在 `query_learning_history` 实际处于 active set 且用户主动询问时按需查询；它不等于长期记忆、学习画像或个性化。Quick AI overlay 与无该工具的 surface 仍必须声明不能访问学习记录。联网能力继续按 `docs/AGENT_RUNTIME_UPGRADE_PLAN.md` 任务 3 开放给真实对话会话（Wikipedia 受控搜索 + 受控网页抓取，覆盖范围以工具描述为准，模型不得把覆盖不到的内容冒充已核实事实）；`docs/AGENT_UPGRADE.md` 中无网络/无工具是旧任务及受控回退路径的历史边界。”编辑并重新生成”已按任务 4 正式开放（最后一条输入 hover 入口 + 行内编辑；方案 B：编辑后的问题以新消息行替代旧问题行、新回答替代旧回答，旧问+旧答保留可审计；导出显示当前答案）。
- 新环境仍可复制 `.env.example` 为 `.env` 填写开发期 `DEEPSEEK_API_KEY`，但该加载与进程环境回退都只在 debug 构建启用；release 不嵌入或读取项目 `.env`/构建路径，只以 Windows Credential Manager 为 Key 权威。真实 `.env` 不提交。

## 当前已知限制与后续边界

以下内容是已经确认的能力边界或后续阶段入口，不代表当前阶段都应立即解决。除非 `DEVELOPMENT_PLAN.md` 或用户明确调整范围，Agent 不应因看到这些条目而顺手扩项。

- **长期数据原则**：`learning_records` 是追加式原始事件。未来若增加重复聚合、复盘状态或时间线，应新建独立表并追加 migration，不回填或覆盖原始事件。
- **长期 UIA 观察项**：Obsidian 编辑模式可从焦点 Edit 读取 `TextPattern2`，阅读模式需沿 Raw View 祖先链查找 Document；两者当前可用，但不同主题、页面结构和版本仍需逐步验证。
- **长期 UIA 观察项**：Codex App 渲染区通常使用 `TextPattern`；ProseMirror 编辑区的选区和坐标可用，但 Paragraph 上下文仍可能不完整。当前对 context-sensitive 单 token 会先尝试 Document、失败后回退 Paragraph；不同应用的 Document/Paragraph 结构和可读范围仍需逐步验证，不能把任何退化结果当成完整语境。
- **跨应用扩展边界**：不要求一次支持所有 Windows 应用。先逐个验证 VS Code、Obsidian、Notion Desktop、WPS/Word、PDF 阅读器等高价值场景；继续以 UIA 为主，高价值应用再做专门适配，剪贴板仅作 fallback，OCR 仍不在当前路线内。
- **解释体验限制**：查询类型依赖本地结构启发式规则；缩写和标识符保持 `word`，不创建单独卡片类型，也不为 FDE 等个别词增加词库特判。很短的多句文本或缺少句末标点的长句仍可能落入相邻类型；优先用真实样本调整，不为分类再增加一次 LLM 请求。
- **解释体验限制**：CaptureInput 和 ExplanationCard 当前上限为 4096 字符，长段落还受模型 JSON 稳定性与浮窗最大高度约束，不代表整页翻译能力。
- **阶段八与 v19 聚合边界**：原始 `learning_records` 继续作为追加式真实查询事件完整保留；正式聚合只合并 queryType 相同且英文学习目标在大小写、首尾空白和连续空白规范化后精确一致的记录，不做语义、相近词、同义词或词形聚类。聚合目标详情可追溯全部真实 occurrence、原始来源、上下文和时间；Review 可在后续轮次轮换该目标的不同真实英文语境，但不伪造历史出现。长期学习者记忆、记忆注入与个性化 Feed 排序仍属于阶段九，不得由当前复习状态或质量反馈冒充。
- **Agent 后续边界**：Quick AI 已支持流式输出、停止/重试、白名单 Markdown 渲染、组合式系统提示词、编辑并重新生成（hover 入口 + 行内编辑，方案 B 替代链审计）与截断诚实提示；主应用对话可按用户主动问题调用事实型本地学习历史只读工具，但没有无条件 prompt 注入。阶段八点五任务 0-6 已通过评审并合入 main，任务 3/4 含真实 Tauri 验收。长期记忆注入、画像、掌握度和个性化仍属于阶段九；截断继续/超 8K 续写和长上下文按任务 5 处理，不因文档建立自动并入正式能力。
- **主题后续边界**：Flexoki 与 Codex 主题已作为随包内置主题接入；当前不包含外部主题 adapter、社区商店、在线下载或自动更新。新增 adapter 仍必须转换到 ReadRayThemeV1 并通过同一安全校验，不能放宽任意 CSS、字体、图片或网络资源边界。
- **对话后续边界**：阶段六的查看全部、重命名、删除和原生导出已经完成；"编辑并重新生成"已按任务 4 开放（最后一条输入 hover 入口 + 行内编辑，方案 B 替代链审计），记忆引用聚合仍属更后续能力，当前 UI 继续诚实禁用。
- **阶段七范围**：三批设置功能与桌面生命周期已经通过复审和真实 Tauri 人工验收，阶段七已完成；阶段八新增实现不得回改其已验收行为。
- **Windows 缩放视觉边界**：透明无边框主窗口在 Windows WebView2 持续拖动期间仍可能有一两帧内容面追赶，这是当前接受的平台边界；应用以主题同色的原生窗口/WebView 底板掩盖迟帧区域，并在缩放会话中延后响应式状态切换。第二轮真实 Tauri 验收前不得写成已解决；若仍有强烈黑/白色带，优先核对背景同步权限与实际窗口/WebView 底色，不要把它误判为数据刷新问题或继续扩大业务 CSS 重构。

## 暂时不要做

- 不做 OCR。
- 不做本地 LLM 运行时。
- 不做 macOS 支持。
- 不做浏览器插件。
- 不添加复杂任务管理文件夹。
- 不添加通用 Agent 框架。
