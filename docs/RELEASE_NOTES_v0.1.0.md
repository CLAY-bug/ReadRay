# ReadRay v0.1.0 Preview

ReadRay 是一个 Windows 优先、本地优先的英语学习 Agent。它把真实阅读、写作和对话中的内容转成可追溯的学习记录、复习卡片与语境化解释。

## 这一版包含

- Windows 桌面主窗口、系统托盘、全局快捷键和 Quick AI 浮层。
- 新安装默认使用双击左 Alt 进行跨应用划词解释，使用 `Alt+Win+Space` 打开 Quick AI；从旧版升级时保留原有快捷键和用户自定义。
- DeepSeek 对话、联网来源卡片、写作检查与连续问答。
- 今天、记忆和复习页面，以及本地 SQLite 学习记录。
- 主题、窗口状态、会话恢复和失败/取消后的安全重试。

## 安装与首次使用

1. 下载 `ReadRay_0.1.0_x64-setup.exe` 并选择安装目录。
2. 启动 ReadRay；如果尚未配置 API Key，点击主窗口右上方的“配置 API Key”引导卡片即可直达“设置 → AI 服务”。
3. 填写并验证自己的 DeepSeek API Key；密钥保存在 Windows Credential Manager，不写入 SQLite 或普通日志。
4. 回到今天、对话或 Quick AI 开始使用。

安装包是 Windows x64 NSIS 当前用户安装版本，不要求管理员权限。

## 已知边界

- 这是 Preview 版本，阶段九的长期学习者画像、个性化排序和效果评估尚未承诺。
- 当前只支持 Windows x64；不包含 OCR、本地大模型、浏览器插件或 macOS 支持。
- 安装包若未完成代码签名，Windows SmartScreen 可能显示额外提示；请以发布页公布的 SHA-256 校验文件完整性。

## 文件校验

```text
文件：ReadRay_0.1.0_x64-setup.exe
SHA-256：1ADD481473D461679C9BC85FC92C4AF31A837E88FFF1C32A89464291B573522B
```

以上 SHA-256 对应本次未签名的 RC1 安装包；如果后续进行代码签名或重新构建，必须重新计算并更新该值。

欢迎通过 GitHub Issues 提交安装、首次配置、快捷键、解释质量或复习体验反馈。Preview 的目的就是先观察真实使用，再决定阶段九和后续优化的优先级。
