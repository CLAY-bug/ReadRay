# ReadRay 比赛分析摘要

最后更新：2026-06-22

## 资料来源

- `resource/official_2026_homepage.md`
- `resource/official_2026_topics_and_submission.md`
- `resource/official_2026_invitation_and_rules.md`
- `resource/attachment1_initial_submission_spec.txt`
- `resource/attachment2_project_document_template.txt`
- `resource/invitation_attachment1_participant_guide.txt`

## 参赛定位

ReadRay 更适合对齐开放赛题中的“生成式大语言模型与智能体”和“AI+X 应用”方向。

推荐叙事：

> ReadRay 将跨应用英文阅读中的即时理解障碍，转化为结构化、可本地保存、可复盘的学习事件。

这个方向的优势是：

- 痛点真实，用户本人可以长期自用和验证。
- 单人可完成，不依赖大规模训练或复杂硬件。
- 可以展示完整产品闭环，而不是只展示模型调用。
- 适合在面试中讲清楚产品、工程、Agent 设计和本地数据闭环。

## 核心展示闭环

```text
复制英文文本
-> 全局快捷键呼出 ReadRay
-> 读取剪贴板
-> 判断文本类型和解释意图
-> 调用 DeepSeek 生成结构化解释卡
-> 保存到 SQLite
-> 根据历史记录生成复盘建议
```

## MVP 优先级

第一优先级：

- Tauri 窗口显示和隐藏。
- 全局快捷键。
- 剪贴板读取。
- DeepSeek API 调用。
- 解释卡展示。
- SQLite 保存查询记录。

第二优先级：

- 历史记录搜索。
- 每日复盘列表。
- remembered / forgotten 反馈。
- Markdown 导出。

暂不做：

- OCR。
- 本地大模型。
- 浏览器插件。
- macOS 支持。
- 商业词典数据内置。

## 主要风险

风险：被评委认为只是查词工具。

应对：演示本地记忆、用户画像、复盘规划和跨应用快捷触发，把重点放在“学习事件闭环”而不是“单次翻译”。

风险：LLM 输出不稳定。

应对：使用结构化 JSON 输出、schema 校验和错误状态展示。

风险：词典版权。

应对：不内置商业词典内容，只保存用户主动触发文本和模型生成解释。

风险：单人开发范围失控。

应对：先完成可运行闭环，再做 UI 精修、复盘算法和比赛材料。

## 比赛材料建议

项目文档重点写：

- 真实场景痛点。
- ReadRay 与普通词典、浏览器翻译插件的区别。
- 本地数据闭环和隐私优势。
- Agent 层设计：意图判断、上下文构造、工具调用、本地记忆、复盘规划。
- MVP 演示流程和可量化指标。

演示视频建议控制在 5 分钟内：

1. 展示跨应用阅读场景。
2. 快捷键呼出 ReadRay。
3. 生成解释卡。
4. 保存历史记录。
5. 展示复盘列表。
6. 总结本地个性化学习 Agent 的价值。
