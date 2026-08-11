# ReadRay 划词翻译速度优化任务书

最后更新：2026-08-11  
状态：实施中；任务 1 已通过真实使用验收；任务 2 已完成重复原句验收返修、代码审查与聚焦验证并合入主项目，等待真实桌面验收；任务 3～5 未开始

## 目标

优化 `Ctrl+Alt+U` 划词查询从捕获选区到显示 ExplanationCard 的完整链路，在保留必要上下文消歧能力的前提下显著缩短等待时间，并保证纯中文选区最终形成英文学习目标。

产品体验目标：

- 快捷键触发后立即显示锚定的 loading 浮层，不让用户怀疑快捷键是否生效。
- 正常网络下，常见单词、短语和普通句子尽量在约 3 秒内显示完整结果。
- 完全相同的缓存查询应接近即时返回。
- 网络或模型异常时在 `8～12s` 内明确失败，不再等待数十秒或数分钟。
- 新查询、隐藏或关闭浮层后，旧请求不得覆盖新结果，也不得作为新的学习记录落库。
- 纯中文选区直接返回自然英文；Memory、Review、Today 等学习目标消费者统一显示英文目标，不再把中文选区、内部字段名或错误占位内容当作学习目标。

最终速度和翻译效果以用户在真实阅读场景中的持续使用体验为准，不另建量化评测体系。

## 已确认产品决策

- ExplanationCard 请求显式关闭 thinking。
- 不做 thinking 开关、不同 Prompt 或不同 Provider 的翻译质量对照测评。
- 不增加“深入理解”入口。长难句深入分析继续由 Quick AI 或 ReadRay 主应用对话承担。
- 划词查询继续使用独立、无状态请求，不维护滚动 LLM 会话，不注入历史查词、主应用对话或长期记忆。
- 纯中文 UIA 选区直接按“中文 → 英文”处理，不弹出语言方向选择。
- 英文或包含明确英文学习目标的选区按“英文 → 中文解释”处理。
- 不接入非公开翻译接口，不在第一版增加双 Provider 分阶段显示。
- 不建立专门的性能研究、长期监控或统计系统；只保留排查真实故障所需的最小错误分类和安全日志。

## 当前问题

当前主要链路：

```text
Ctrl+Alt+U
→ Windows UIA 捕获选区、上下文和锚点
→ 显示锚定 loading 浮层
→ 前端 invoke create_explanation_card
→ DeepSeek 生成完整 ExplanationCard JSON
→ Rust 解析和 schema 校验
→ 保存 learning_record
→ 前端显示结果并通知主应用刷新
```

已确认需要修复的问题：

1. `src-tauri/src/deepseek_explanation.rs` 没有显式关闭 thinking，所有类型统一使用 `max_tokens = 4096`。推理内容会增加输出 token、总时延，并可能产生 reasoning 相关错误。
2. `src-tauri/src/deepseek_client.rs` 的 `shared_http_client()` 当前每次调用都会重新构造 `reqwest::Client`，没有实际复用连接池、TCP/TLS 连接和会话。
3. 全局 HTTP 总超时为 `180s`、读取超时为 `60s`，这是为 Quick AI 长流式回答设置的预算，不适合临时划词查询。
4. `src/App.tsx` 的 `contextForQuery()` 基本原样传入 UIA paragraph，可能包含远超消歧所需的无关窗口文本。
5. 前端 `anchoredRequestId` 只能阻止迟到结果更新 UI；Rust command 会在返回前保存学习记录，因此过期请求仍可能落库。
6. Memory 和 Review 当前直接把原始 `learning_records.query_text` 当学习目标，导致中文误选或中文反查显示在学习页面。
7. 当前没有 ExplanationCard 本地结果缓存，也没有相同进行中请求的 single-flight 合并。

## 目标链路与数据权威

```text
快捷键
→ UIA 捕获原始选区、上下文和锚点
→ 本地确定 queryDirection
→ 立即显示 loading
→ 生成 requestKey 和 cacheKey
→ 内存 single-flight / SQLite 精确缓存
   ├─ 命中：校验缓存卡片
   └─ 未命中：DeepSeek 非 thinking 短请求 → 校验 → 写缓存
→ 核对 requestKey 仍为当前请求
→ 原子保存学习事件 + 规范英文学习目标
→ 显示结果并通知主应用刷新
```

权威字段：

| 字段 | 语义 | 规则 |
| --- | --- | --- |
| `queryText` | 用户原始选区 | 原样保留，不因翻译方向覆盖 |
| `contextText` | 原始来源上下文 | 保存真实捕获值；模型只接收裁剪后的必要上下文 |
| `queryDirection` | `enToZh` 或 `zhToEn` | Rust 本地规则确定，不调用模型判断 |
| `learningTargetText` | 规范英文学习目标 | 新的成功学习记录必须具备；学习页面统一消费 |
| `ExplanationCard.sourceText` | 本次原始查询 | 继续与 `queryText` 一致 |
| `requestKey` | 一次交互请求身份 | 控制取消、迟到结果和是否允许落库 |
| `cacheKey` | 可复用解释结果身份 | 由所有会影响模型结果的稳定输入计算 |

为保护 `learning_records` 追加式原始事件，规范英文目标优先使用新的伴生投影表持久化，例如 `learning_record_targets`，通过新的顺序 migration 创建；不覆盖或删除原始 `query_text`。正式页面继续走 repository/service/typed Rust command，不直接读取 SQLite，也不使用 `localStorage`。

## 执行规则

- 严格按任务 1 → 5 执行，一次只实现一个任务。
- 每个任务完成代码审查和必要自动验证后，立即合回 `D:\project\ReadRay` 供用户真实体验；不等待全部任务完成后再统一合并。
- 合回主项目后停止并交给用户在真实场景中验收；只有用户明确说“继续”才进入下一任务。
- 开始每个任务前重新阅读 `AGENTS.md`、`docs/RESOURCE_MAP.yml`、本文件和 `docs/HANDOFF.md`，检查 `git status --short`。
- 先定位现有扩展点，不建立第二套 DeepSeek 客户端、ExplanationCard schema 或学习记录事实源。
- 不新增依赖；现有 Rust、Tokio、reqwest 和 rusqlite 能完成时，不引入缓存、取消或语言检测库。
- 自动验证使用 `docs/WINDOWS_ENVIRONMENT.md` 已记录的本机 pnpm、Cargo 和 Tauri 命令。
- 自动测试只用于防止功能、并发和数据回归；翻译是否自然、速度是否满意由用户真实使用验收。
- 自动测试不替代真实 Tauri、UIA、SQLite、DeepSeek 与跨应用人工验收。

## 任务 1：模型与 HTTP 快路径

状态：已完成代码审查、聚焦验证和真实使用验收（2026-08-10）

验证边界：本任务只运行 DeepSeek 客户端、ExplanationCard 请求体/解析/校验及 Quick AI 请求体回归的 Rust 聚焦测试，并通过 `cargo fmt --check`、`cargo check` 与 `git diff --check`；未运行全量测试，未启动或操作真实 Tauri，也未运行 ignored 的真实联网测试。

### 目标

直接去掉当前最主要的推理输出和重复连接开销，并让异常请求尽快结束。

### 实现内容

- 按当前 DeepSeek 模型接口支持的格式，对 ExplanationCard 显式设置 thinking disabled；Quick AI、写作和复习保持现状。
- 把 `shared_http_client()` 改成进程级真正单例，例如使用 `OnceLock<reqwest::Client>`；所有现有调用复用同一连接池，不跨 `await` 持有额外锁。
- 保留 Quick AI 长流式回答现有 `180s/60s` 预算；ExplanationCard 在调用层增加独立 `8～12s` 总超时，不全局缩短其他功能。
- 只对连接失败、429 和 5xx 等瞬态失败最多快速重试一次；用户取消、4xx 配置错误、非法 JSON 和 schema 错误不自动重试。
- 根据四种 ExplanationCard schema 设置合理输出预算，不再给所有查询统一预留 4096 token；必须保留较长段落的完整翻译能力。
- 压缩 Prompt 中不必要的重复说明，但保留本地 query type、严格 JSON、字段上限、上下文语义和不虚构内容等 validator 所需约束。
- 保留 `response_format=json_object` 和完整 schema 校验，不为流式首字拆分当前结构化协议。
- 错误只区分取消、超时、网络、模型输出、schema 和保存失败等产品需要的类别；不记录原文、上下文、API Key、完整 Prompt 或回答。

### 必要自动验证

- 请求体明确关闭 thinking，Quick AI 等其他请求体不受影响。
- 连续请求复用同一个 HTTP Client。
- 超时和重试只作用于 ExplanationCard，非瞬态失败不会反复请求。
- 四种 ExplanationCard 均能通过现有 parser/validator，输出预算不会截断正常卡片。
- Rust 聚焦测试、`pnpm build`、`cargo fmt --check`、`cargo check`、`git diff --check` 通过。

### 真实使用验收

- 用户在日常阅读应用中查询常见单词、短语、普通句子和较长段落。
- 确认等待明显缩短、翻译仍能根据上下文消歧、异常时不再长时间卡住。
- 确认不再直接显示 DeepSeek reasoning 内部错误。

## 任务 2：最小上下文、请求取消与迟到结果隔离

状态：已完成重复原句验收返修、代码审查与聚焦验证并合入主项目（2026-08-11），等待用户真实桌面验收

验证边界：UIA 上下文聚焦测试 8 项、ExplanationCard 请求权威测试 13 项通过且 1 项 ignored、DeepSeek 客户端 17 项、Quick AI 请求体回归 1 项、前端请求权威 8 项、overlay 生命周期 14 项均通过；`pnpm build`、`cargo fmt --check`、`cargo check`、RESOURCE_MAP YAML 解析与 `git diff --check` 通过。未运行全量测试，未启动或操作真实 Tauri，也未运行真实 UIA/SQLite/DeepSeek 与 ignored 联网测试。

重复原句返修验证边界：前端原句展示聚焦测试 3 项、Rust 原句协议与解析聚焦测试 8 项通过；`pnpm build`、`cargo fmt --check`、`cargo check`、RESOURCE_MAP YAML 解析与 `git diff --check` 通过。未运行全量测试、ignored 联网测试或真实 Tauri/历史 SQLite 视觉测试。

### 目标

只发送能够可靠定位且足以消歧的上下文，并让已经失去交互意义的请求停止且无法显示、通知或落库。这里追求“最小可靠上下文”，不对自然语言做无法保证正确的语义猜测。

### 实现内容

- 在选区 `TextRange` 仍有效时，用 UIA `MoveEndpointByRange` 从同一 Paragraph 精确取得选区前缀和后缀，不通过 `find(queryText)` 猜测重复文本中的第一次出现。
- 在现有 UIA 捕获结果上派生模型专用 `minimalContext`，不修改原始 `contextText`：
  - 单词/短语：使用精确包含选区的完整句；句界无法可靠确定时保守回退完整 paragraph，不自行挑选所谓“更相关”的相邻句。
  - 句子：使用目标句及其前一个可可靠取得的完整句，帮助代词和承接关系消歧；不追加后句。
  - 段落：只使用选区自身，不重复发送同一 paragraph。
  - 只做确定性的零宽字符、对象占位符和空白清理；不做摘要、UI 文本语义分类或第二次模型调用。
- 原始 `contextText` 继续独立落库；`minimalContext` 只用于本次模型请求。空上下文按无上下文处理，超过 4096 字符在联网前拒绝。
- 固定 system prompt 和 user prompt 的字段顺序；服务端前缀缓存只作为可能收益，不成为正确性依赖。
- manual 与 anchored 查询分别维护作用域权威。前端每个 authority 实例使用原生加密随机 nonce + sequence 生成唯一 `requestKey`；新请求、编辑、隐藏/关闭浮层、切换模式或组件销毁时使旧 key 失效并调用 Rust 取消。
- Rust 用可取消 future 中止同作用域旧请求，并为每次注册另分配内部单调 `generation`；checkpoint、commit 和 guard Drop 同时核对作用域、客户端 key 与 generation，避免同 key 复用时的 ABA 竞态，旧实例迟到 cancel 也不能取消新实例请求。
- 模型返回后、usage 处理前后和学习记录同步保存前均核对 Rust 权威。只有当前请求可以保存学习记录、发出 `learning-record-created` 和显示结果；仅在前端忽略迟到结果不算完成。
- word/phrase 原句字段采用非对称关系：中文主导的 `sourceSentence` 可单独存在；非中文主导的普通英文原句仍必须同时提供 `sourceSentenceZh`；`sourceSentenceZh` 永远不能脱离原句。主要为中文、仅夹少量英文查询词或标识符的原句只保留原句一次。
- “主要为中文”使用前后端一致的确定性字符规则：统计汉字与 ASCII 拉丁字母，汉字存在且两倍汉字数不小于拉丁字母数时判为中文主导，标点、数字和空白不参与；不使用模型、词表或语义相似度。新模型卡片在 validator 和落库前清除冗余 `sourceSentenceZh`，前端即时映射和 Memory 历史 SQLite 卡片应用同一展示规则。

### 必要自动验证

- 上下文派生覆盖英文/中文标点、换行、无句末标点、目标重复出现、UIA 占位字符和不可靠边界的保守回退。
- 请求状态覆盖 A→B、编辑、关闭、隐藏、重新呼出、模式切换、组件卸载、失败重试、旧实例迟到取消和同 key ABA。
- 迟到请求不能显示结果、发出成功通知或保存学习记录，取消不会留下无法重试的 loading 状态。
- 覆盖中文主导夹 `Rust/generation`、`Memory/Review` 的原句单行展示、普通英文原句保留中译、中文原句单独合法、英文原句无中译非法、孤立译文非法、Prompt 规则和既有英文 ExplanationCard 回归。

### 真实使用验收

- 用户在常用的两个或更多应用中检查多义词和句子消歧。
- 快速连续查询时只显示、保存最后一次结果。
- 查询过程中关闭或隐藏浮层后，不出现迟到结果和多余学习记录。
- 在中文主导、夹有英文查询词的原句中，overlay 与 Memory 只显示原句一次；普通英文原句仍显示英文原句和中文翻译。

## 任务 3：中文反查与规范英文学习目标

状态：未开始

### 目标

让纯中文查询直接输出英文，并从数据源头保证学习页面只消费英文目标。

### 实现内容

- 在 Rust 使用本地字符规则确定方向，不增加语言检测模型调用：
  - 清理后含中文且不含拉丁字母：`zhToEn`。
  - 含明确拉丁英文目标：`enToZh`；中文只作为上下文时不改变英文目标。
  - 只有数字、符号或空白：本地拒绝，不调用模型、不保存记录。
- ExplanationCard schema 增加统一的 `learningTargetText`：
  - `enToZh`：由本地从英文 query 规范化得到，不信任模型改写。
  - `zhToEn`：模型返回自然英文，Rust 验证它必须包含有效英文且不含中文；原始中文继续保存在 `sourceText/queryText`。
- 为中文单词、短语、句子和段落建立方向化 Prompt，中文输入的主结果必须是英文。
- 通过新的顺序 migration 创建规范英文目标伴生表；新记录在同一事务中写入原始 learning record 和 target projection，任一失败都不产生半条学习事件。
- Memory、Review、Today、搜索和其他学习目标消费者通过 repository/service 读取 `learningTargetText`，不直接以 `queryText` 作为标题或复习答案。
- 历史数据兼容：
  - 旧英文 `query_text` 可按严格本地规则确定性建立英文 target projection。
  - 旧中文记录不调用模型批量修复、不猜测英文；没有可靠英文目标时，不进入学习目标列表。
  - 原始记录保留在 SQLite，不物理删除、不回写中文 query。
- 搜索同时支持规范英文目标和原始来源文本，但列表主标题始终使用英文目标。

### 必要自动验证

- 方向判断覆盖纯中文、纯英文、英文目标加中文上下文、中英混合专有名词、数字/标点和代码标识符。
- validator 拒绝中文或空的 `learningTargetText`，同时保持 `sourceText` 原样。
- migration 覆盖新库、真实前序版本升级、旧英文投影、旧中文过滤和事务失败回滚。
- Memory、Review、Today 和搜索只把规范英文作为学习目标。

### 真实使用验收

- 划选“界面”“微调”和中文完整句时，浮层主结果是自然英文。
- 新中文查询在 Memory、Review 和 Today 中显示英文，详情仍可追溯原始中文。
- 截图中已有的旧中文记录不再出现在 Memory/Review 主列表，英文历史记录保持正常。
- 英文查询原有解释体验没有明显退化。

## 任务 4：SQLite 精确缓存与 single-flight

状态：未开始

### 目标

让完全相同的查询无需重复调用模型，并合并短时间内相同的进行中请求。缓存由 ReadRay 本地控制，不依赖长会话或服务端缓存。

### 实现内容

- 通过新的顺序 migration 增加 ExplanationCard 缓存表，不修改已发布 migration。
- cache key 至少包含：

```text
normalizedSourceText
queryDirection
queryType
minimalContextFingerprint
modelId + modelRevision
promptVersion
schemaVersion
```

- `sourceApp`、窗口坐标和创建时间等不影响模型输出的字段不进入 cache key。
- 只缓存已经通过 JSON 解析和 ExplanationCard validator 的结果；错误、取消、超时、半成品和 reasoning 不缓存。
- 命中后重新校验 schema/version；损坏或过期条目删除后重新请求模型。
- 首版 TTL 采用 7 天，并设置有限容量和非阻塞清理；不增加用户可见的缓存设置。
- 内存 single-flight 按同一 cache key 合并进行中的 provider 请求；每个等待者仍保留自己的 requestKey。
- 缓存复用的是解释结果，不是学习事件。用户每次明确完成的查询仍可新增一条真实 learning record；取消、迟到或快捷键抖动不新增记录。
- Prompt、模型或 schema 变更通过版本号自然失效。

### 必要自动验证

- 覆盖缓存命中/未命中、TTL、容量清理、版本隔离、损坏恢复、single-flight、取消和失败不覆盖。
- 相同文本但不同上下文不能误用语境义。
- 两个并发相同请求最多产生一次 provider 调用，但无效等待者不能显示或保存。
- migration 覆盖实际前序数据库版本到 latest。

### 真实使用验收

- 连续查询同一表达时明显接近即时返回。
- 重启 ReadRay 后，相同查询仍可命中本地缓存。
- 同一个词在不同句子中不会串用错误语境。

## 任务 5：完整回归与真实场景收口

状态：未开始

### 目标

确认优化没有破坏上下文翻译、学习记录和其他 DeepSeek 功能，并以用户真实阅读体验决定是否完成。

### 必要自动验证

- ExplanationCard：四种 query type、两种方向、最小上下文、非法输出、超时、取消、重试和缓存。
- UIA/overlay：loading 锚定、连续查询、隐藏、关闭、重呼、错误恢复、请求身份和刷新事件。
- 数据：新库与历史升级、英文 target、旧中文过滤、Memory/Today/Review/search 映射和重启恢复。
- 回归：Quick AI、写作、复习制卡、设置 Token 分类和桌面生命周期现有聚焦套件。
- 执行本机既有验证命令：

```powershell
& 'D:\Application\nvm\nodejs\pnpm.cmd' test:overlay
& 'D:\Application\nvm\nodejs\pnpm.cmd' test:conversation
& 'D:\Application\nvm\nodejs\pnpm.cmd' test:review
& 'D:\Application\nvm\nodejs\pnpm.cmd' test:writing
& 'D:\Application\nvm\nodejs\pnpm.cmd' test:settings
& 'D:\Application\nvm\nodejs\pnpm.cmd' build
cargo test --manifest-path src-tauri\Cargo.toml
cargo fmt --manifest-path src-tauri\Cargo.toml -- --check
cargo check --manifest-path src-tauri\Cargo.toml
git diff --check
```

### 真实使用验收

由用户在日常阅读过程中自然使用，不进行专门的翻译质量打分或对照实验。重点感受：

- 常见查询是否能在可接受时间内出现，约 3 秒目标是否基本达到。
- 多义词和句子是否仍能结合上下文正确翻译。
- 中文查询是否自然转换为英文学习目标。
- 连续查询、隐藏、断网和失败恢复是否顺畅。
- Memory、Today、Review、搜索和重启恢复是否符合预期。

只有用户确认真实体验满意，整个优化才标记完成。

## 完成后再考虑的可选项

只有任务 1～5 完成后，真实使用仍明显慢于预期，才讨论：

1. 合规、稳定的正式专用翻译 API，用于先显示核心译文；必须先定义它与 ExplanationCard 和学习记录的权威关系。
2. 对非常长的段落采用与单词、短语不同的等待预期。
3. 根据真实缓存规模决定是否需要清理入口。

继续排除：滚动 LLM 翻译会话、历史记忆注入、非公开翻译接口、整页翻译批处理、为流式显示拆坏结构化 JSON，以及额外的语言检测或摘要模型调用。

## 每轮交付格式

1. 当前任务结果与关键判断；
2. 修改文件；
3. 必要自动验证；
4. 尚未验证的风险；
5. 用户需要完成的真实使用验收；
6. “已停止，等待验收”。
