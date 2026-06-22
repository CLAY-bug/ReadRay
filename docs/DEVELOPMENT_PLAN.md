# ReadRay 开发计划

最后更新：2026-06-21

## 1. 项目定位

ReadRay 是一个 Windows 优先的本地英语学习 Agent。

它不是普通查词软件，也不是浏览器翻译插件，而是一个轻量的跨应用学习助手：

```text
在任意 App 中遇到英文理解障碍
-> 使用全局快捷键呼出
-> 捕获文本
-> 生成上下文解释卡
-> 保存为本地学习记忆
-> 生成个性化复盘任务
```

比赛叙事：

> ReadRay 将跨应用英文阅读中的即时障碍，转化为结构化、可本地保存、可复盘的学习事件。

中文定位：

> ReadRay 是面向跨应用英语阅读的本地个性化学习 Agent。

## 2. 核心原则

1. 优先面向 Windows 原生环境开发。
2. MVP 必须控制在单人可完成范围内。
3. 第一版不使用通用 Agent 框架。
4. 自己实现项目专属的轻量 Agent 业务层。
5. 不内置有版权风险的商业词典内容。
6. 第一版 LLM 供应商使用 DeepSeek。
7. 本地存储使用 SQLite。
8. 第一版不做 OCR、本地大模型部署、整页翻译或 macOS 支持。
9. 优先完成可自用、可演示、可比赛提交的闭环。

## 3. 技术栈

桌面框架：

```text
Tauri
```

ReadRay 当前采用 Tauri 作为桌面框架。遇到全局快捷键、窗口行为、剪贴板访问、SQLite 或 Windows 打包问题时，优先定位原因并解决，不在计划中预设替代框架。

前端：

```text
React + TypeScript
```

原生层：

```text
Rust / Tauri commands
```

数据库：

```text
SQLite
```

LLM：

```text
DeepSeek OpenAI-compatible API
```

包管理：

```text
Node：pnpm
Rust：rustup + cargo
Python 脚本：uv
```

Python 只用于辅助脚本、评测、数据处理或后续实验。桌面 App 运行时不应依赖 Python。

## 4. 轻量 Agent 设计

MVP 阶段不使用 LangChain、LangGraph、Pi、Agno 或 Pydantic AI。

ReadRay 自己实现一个小型 Agent 业务层：

```text
ReadRayAgent
├─ context_builder      构造短期和长期上下文
├─ intent_classifier    判断 word / phrase / sentence / paragraph / review
├─ prompt_router        选择提示词模板
├─ llm_client           调用 DeepSeek
├─ card_validator       校验结构化输出
├─ memory_manager       读写 SQLite
├─ profile_updater      更新学习者画像
└─ review_planner       生成复盘任务
```

初始工具能力：

```text
get_clipboard_text()
create_explanation_card()
save_card()
search_memory()
schedule_review()
export_markdown()
```

这已经足够支撑 Agent 叙事：系统具备输入感知、任务分类、工具调用、本地记忆和持续学习闭环，而不是简单的一次性 LLM 调用。

## 5. 数据来源与版权策略

ReadRay 不内置商业单词书，也不内置有版权风险的词典数据库。

MVP 的数据来源：

1. 用户主动选中或复制的文本。
2. 模型生成的解释。
3. 用户本地查询历史。
4. 用户后续编辑和反馈。

禁止事项：

1. 不爬取商业词典网站。
2. 不复制牛津、朗文、有道、欧路等商业词典释义。
3. 不把模型生成内容伪装成权威词典来源。

文档说明口径：

> ReadRay 不内置有版权风险的词典内容。它只在本地保存用户主动触发的学习记录和模型生成解释。

## 6. 最小文档结构

本项目刻意避免重型任务系统。

当前文档结构：

```text
ReadRay/
  AGENTS.md
  docs/
    DEVELOPMENT_PLAN.md
    HANDOFF.md
    RESOURCE_MAP.yml
  resource/
```

应用脚手架创建后的源码结构预计为：

```text
ReadRay/
  src/
  src-tauri/
  package.json
  pnpm-lock.yaml
```

除非用户明确要求后续增加管理结构，否则不要添加 `tasks/` 层级。

## 7. 开发阶段

### 阶段一：Tauri 基础能力打通

目标：

打通 ReadRay 在 Windows 上所需的 Tauri 基础能力。

验收标准：

- 全局快捷键可以显示和隐藏应用窗口。
- 窗口可以置顶，并能按需隐藏。
- 可以读取剪贴板文本。
- SQLite 可以写入和读取。
- 可以在应用流程中调用 DeepSeek API。
- Windows 本地开发构建可以运行。

### 阶段二：解释卡 MVP

目标：

完成最小可用的查询闭环。

验收标准：

- 用户复制英文文本后可以呼出 ReadRay。
- 文本可以发送给 DeepSeek。
- 应用返回结构化解释卡。
- 解释卡能处理单词、短语和句子。
- 输出包含释义、语境义、例句、搭配或语块、难度和复习建议。
- LLM 输出在保存前经过 schema 校验。

### 阶段三：本地记忆

目标：

把每次查询变成可检索的学习记录。

验收标准：

- SQLite 保存查询文本。
- SQLite 保存解释卡内容。
- 记录包含时间、类型、难度和可用的来源上下文。
- 历史页面支持搜索。
- 历史页面可以按单词、短语、句子、段落过滤。

### 阶段四：复盘闭环

目标：

让 ReadRay 明显区别于一次性查词工具。

验收标准：

- 基于历史记录生成每日复习列表。
- 自动聚合高频单词和短语。
- 支持 remembered / forgotten 反馈。
- 根据反馈更新复习优先级。
- 支持导出复盘内容为 Markdown。

### 阶段五：个性化

目标：

让 ReadRay 随着使用逐步更懂用户。

验收标准：

- 记录用户常查主题。
- 记录高频薄弱点。
- 记录解释偏好。
- 后续解释可以使用历史上下文。
- 生成个人学习摘要。

### 阶段六：比赛材料

目标：

准备比赛提交材料。

验收标准：

- 项目文档初稿。
- 300 字作品简介。
- 5 分钟演示视频脚本。
- 基础指标或小规模评测数据。
- 可选辅助 ZIP 材料。

## 8. 当前阶段推进方式

本计划按阶段推进，不按周或固定日期推进。个人开发过程中可能遇到环境、框架、实现和调试问题，阶段完成以验收标准为准，不以时间为准。

当前只推进阶段一：Tauri 基础能力打通。

阶段一不包含：

- UI 精修。
- OCR。
- 复盘算法。
- 本地大模型。
- 跨平台支持。

建议推进顺序：

1. 检查当前 Node、pnpm、Rust 和 Tauri 环境。
2. 初始化 Tauri + React + TypeScript。
3. 验证应用窗口显示和隐藏。
4. 验证全局快捷键。
5. 验证剪贴板读取。
6. 验证 SQLite 写入和读取。
7. 验证 DeepSeek API 调用。
8. 更新 `docs/HANDOFF.md`。

## 9. 主要风险

风险：Tauri 桌面能力接入过程中遇到问题。

应对：优先定位具体原因，查官方文档、最小复现和本地环境配置，不把切换框架作为默认计划。

风险：项目被认为只是简单查词软件。

应对：强调本地记忆、复盘规划、用户画像和可量化的操作流程缩短。

风险：LLM 输出不稳定。

应对：使用结构化 JSON 输出和 schema 校验。

风险：单人开发范围失控。

应对：MVP 闭环跑通前，不做 OCR、本地模型部署、插件生态或跨平台支持。

风险：词典版权问题。

应对：不内置商业词典内容，只使用用户主动触发的文本和模型生成解释。

## 10. 当前决策摘要

```text
开发系统：Windows 原生
桌面框架：Tauri
前端：React + TypeScript
原生层：Rust
数据库：SQLite
LLM：DeepSeek OpenAI-compatible API
Agent 框架：不用
Agent 层：自己实现轻量业务层
Python：只用于脚本，不作为运行时
项目流程：极简文档，不使用任务层级
```
