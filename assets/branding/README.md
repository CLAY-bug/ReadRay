# ReadRay 图标资产

- `readray-icon-concept-v1.png`：第一稿原始概念图，包含大图标和小尺寸预览，仅用于保留设计来源。
- `readray-icon-master.png`：从第一稿大图标原像素提取的透明正方形母版；外围安全边距经过光学收紧，用于生成 Tauri 图标。
- `public/branding/readray-startup-icon.png`：由同一母版生成的 256px 主窗口静态启动图标。

生成命令：

```powershell
& 'D:\Application\nvm\nodejs\pnpm.cmd' tauri icon .\assets\branding\readray-icon-master.png
```

不要直接编辑 `src-tauri/icons/` 中的派生文件；需要调整图标时先更新母版，再重新生成。
