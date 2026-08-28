# Windows 开发环境说明

最后更新：2026-08-27

## 适用范围

本文记录 ReadRay 在 Windows 上开发 Tauri 应用所需的本机环境、验证命令、VS Build Tools 修复经验和磁盘占用策略。

ReadRay 当前仍以 Windows 优先推进；遇到 Tauri、Rust、pnpm、MSVC 或 Windows SDK 问题时，先定位原因，不切换技术栈。

## 当前已验证环境

- Node.js / pnpm 可用。
- Rust / Cargo 使用 `stable-x86_64-pc-windows-msvc` 工具链。
- Visual Studio 2022 Build Tools 已安装并完整可用。
- Tauri CLI 已能识别 WebView2、MSVC、Windows SDK、Rust 和 Cargo。
- `pnpm build` 已通过。
- `pnpm tauri dev` 已通过 `link.exe` 阶段，生成并启动过 `src-tauri/target/debug/readray.exe`。
- 2026-06-22 已完成阶段 A 磁盘迁移：Rust/Cargo 用户目录和 VS 包缓存已迁到 D 盘，原路径保留 Junction。

关键验证命令：

```powershell
pnpm build
pnpm tauri info
pnpm tauri dev
```

## 本机命令与发布基线

以下是 2026-06-27 在本机验证过的 ReadRay 开发基线。Codex 后续执行本仓库任务时应优先直接使用这些事实，不要每次先尝试通用默认路径。

### Node 与 pnpm

- 本机 Node.js 路径为 `D:\Application\nvm\nodejs\node.exe`。
- ReadRay 当前使用的本机 pnpm 为 `D:\Application\nvm\nodejs\pnpm.cmd`，版本为 `10.30.3`。
- Codex 命令环境中的 `pnpm` 首选项可能解析到 `C:\Users\19150\.cache\codex-runtimes\codex-primary-runtime\dependencies\bin\pnpm.cmd`。该运行时曾因当前 `node_modules` 由另一 pnpm 创建而触发非交互清理，报 `ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY`。
- 因此 Codex 运行 ReadRay 的安装、构建和 Tauri 命令时，默认显式调用本机 pnpm：

```powershell
& 'D:\Application\nvm\nodejs\pnpm.cmd' build
& 'D:\Application\nvm\nodejs\pnpm.cmd' tauri info
& 'D:\Application\nvm\nodejs\pnpm.cmd' tauri dev
```

只有该路径不存在或执行失败时，才重新检查 pnpm 安装和 PATH，不要先让 Codex bundled pnpm 接管现有 `node_modules`。

### Git 与 GitHub

- 当前远端固定为 `https://github.com/CLAY-bug/ReadRay.git`，普通 fetch/push 使用 Git Credential Manager 处理 HTTPS 凭据，不使用 SSH remote。
- 本机 Git for Windows 的全局 OpenSSL 后端曾在 HTTPS fetch 时出现 `TLS connect error`；ReadRay 仓库已用本地配置 `http.sslBackend=schannel` 验证可访问 GitHub，不需要修改系统代理或回退 SSH。
- 当前开发流程直接维护 `main`。用户只要求“提交并上传 GitHub”时，默认使用本地 Git 提交后执行 `git push origin main`。
- 本机当前没有 `gh`。普通 commit/push 不依赖 GitHub CLI，不需要为此先检查或安装 `gh`；只有明确需要创建或管理 PR，且现有 GitHub connector 不能完成时，才检查 `gh`。
- `design-open-design/` 不属于 ReadRay 提交范围；暂存时使用明确文件列表，不使用会把该目录带入提交的 `git add -A`。

### 应用内更新与发布基线（2026-08-27 起）

ReadRay 从 0.1.1 起接入官方 `tauri-plugin-updater`：应用定期/手动请求 GitHub Release 上的 `latest.json`，发现新版本后在“设置 → 关于”下载并安装，安装完成后由 NSIS 安装器自动重启应用；更新包先做 minisign 签名校验，校验失败拒绝安装。已安装 RC1（0.1.0）的用户需要手动下载一次带 updater 的版本，之后进入应用内更新通道。

- 签名密钥（一次性生成，勿提交仓库）：
  - 私钥：`C:\Users\19150\.tauri\readray-updater.key`（空密码）。必须离线备份；**私钥丢失后，已安装用户无法再收到应用内更新，只能重新手动下载一次**。私钥泄露则任何人可向用户推送恶意更新，与 GitHub Release 权限一起构成更新安全边界。
  - 公钥已固化在 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`。在第一个带 updater 的版本发布之前可以免费重新生成密钥对（换公钥无迁移成本）；发布后再换钥匙需要用户手动升级一次。
- 签名构建：release 构建前在同一 PowerShell 会话设置环境变量（`.env` 对 CLI 签名无效）：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "C:\Users\19150\.tauri\readray-updater.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
& 'D:\Application\nvm\nodejs\pnpm.cmd' release:build
```

  变量名必须是 `TAURI_SIGNING_PRIVATE_KEY`（值可以是私钥路径或私钥内容）；`TAURI_SIGNING_PRIVATE_KEY_PATH` 不存在，设置后 Tauri 仍报 "no private key"。密钥为空密码也必须把 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 设为空字符串。

  产物在 `src-tauri/target/release/bundle/nsis/`：`ReadRay_<版本>_x64-setup.exe` 与同名 `.sig` 签名文件。

- 每次发版步骤：
  1. 抬版本号：`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`package.json` 三处同步（updater 按 semver 比较，必须高于已发布版本）。
  2. 按上文完成签名 release 构建，记录安装包大小与 SHA-256。
  3. 在 GitHub 创建 Release（tag 形如 `v0.1.1`），上传**三个资产**：安装包 exe、`.sig` 文件、`latest.json`。
  4. `latest.json` 内容模板（`signature` 字段粘贴 `.sig` 文件全文；`url` 指向该 Release 的 exe 资产直链；`pub_date` 为 RFC 3339）：

```json
{
  "version": "0.1.1",
  "notes": "更新说明",
  "pub_date": "2026-08-27T12:00:00+08:00",
  "platforms": {
    "windows-x86_64": {
      "signature": "<ReadRay_0.1.1_x64-setup.exe.sig 文件全文>",
      "url": "https://github.com/CLAY-bug/ReadRay/releases/download/v0.1.1/ReadRay_0.1.1_x64-setup.exe"
    }
  }
}
```

- 应用端检查地址为 `https://github.com/CLAY-bug/ReadRay/releases/latest/download/latest.json`（写在 tauri.conf.json 的 `plugins.updater.endpoints`）。更新体验：设置 → 关于 → 立即更新 → 下载（显示百分比）→ 自动 flush 未落盘内容（写作草稿、偏好、复习反馈）→ NSIS passive 模式安装（小进度窗）→ 安装完成自动重启。Windows 安装器限制要求安装前退出应用，插件内部通过 `std::process::exit` 处理，前端代码在 install 之后不会继续执行。
- 当前安装包无 Authenticode 代码签名：浏览器首次下载仍会触发 SmartScreen 提示；应用内更新由 ReadRay 进程自身下载，不经过浏览器，不受 Mark-of-the-Web 影响。
- 国内直连 GitHub Releases 可能缓慢或不稳定：如用户反馈更新失败，可在 `plugins.updater.endpoints` 数组追加镜像地址（按序尝试），镜像需同时提供 `latest.json` 与安装包。

### 开发进程与端口

- Vite 开发端口为 `1420`，Tauri dev 会启动该 Vite 服务、Cargo 和 `src-tauri/target/debug/readray.exe`。
- 启动新的 `pnpm tauri dev` 或运行会占用 Cargo 构建锁的验证前，先定向检查 ReadRay 的 Vite、Cargo 和 `readray.exe` 是否已运行。
- 端口和进程属于动态状态，不能假设永远占用或永远空闲；发现已有实例时先判断能否复用，确需停止时只终止命令行或可执行路径明确指向 `D:\project\ReadRay` 的进程。

## VS Build Tools 必要组件

ReadRay 的 Tauri Windows 开发需要 MSVC 链接器和 Windows SDK。当前修复时确认需要的组件为：

- `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`
- `Microsoft.VisualStudio.Component.Windows11SDK.26100`

可用下面的命令检查组件是否能匹配到完整实例：

```powershell
& 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe' `
  -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
  -requires Microsoft.VisualStudio.Component.Windows11SDK.26100 `
  -products *
```

## 本次修复踩坑记录

`pnpm tauri dev` 失败于 `link.exe not found` 时，根因不是 Tauri 脚手架问题，而是 VS Build Tools / MSVC / Windows SDK 安装不完整或未被正确识别。

修复经验：

- `setup.exe modify --quiet` 必须从管理员权限启动；否则可能返回 `5007`。
- 不要假设已安装的 `setup.exe modify` 支持 `--wait`；本次通过 PowerShell `Start-Process -Wait` 等待安装器完成。
- `--installPath` 包含空格时，`Start-Process -ArgumentList` 的引用必须正确；否则路径可能被拆成 `C:\Program`。
- 普通终端里 `link` / `cl` 不一定在 `PATH` 中；只要 `pnpm tauri info` 能通过 VS 安装信息识别 MSVC，不需要强行把 MSVC bin 目录写入全局 `PATH`。

已验证可用的管理员修复命令形态：

```powershell
Start-Process `
  -FilePath 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\setup.exe' `
  -Verb RunAs `
  -Wait `
  -ArgumentList 'modify --installPath "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools" --channelId VisualStudio.17.Release --quiet --norestart --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.26100'
```

## 磁盘策略

不要手动移动 Visual Studio、Windows Kits、Rust 或 Cargo 目录。手动移动会破坏注册表、安装器状态、`vswhere` 发现逻辑或 PATH。

### 已执行：迁移会持续增长的缓存

当前 ReadRay 项目目录位于 D 盘，Tauri 编译产物也在 `D:\project\ReadRay\src-tauri\target`，不会继续占用 C 盘。

2026-06-22 已执行以下迁移：

- `C:\Users\19150\.cargo` 是 Junction，指向 `D:\app_cache\rust\.cargo`。
- `C:\Users\19150\.rustup` 是 Junction，指向 `D:\app_cache\rust\.rustup`。
- `C:\ProgramData\Microsoft\VisualStudio\Packages` 是 Junction，指向 `D:\app_cache\vs\Packages`。
- `HKLM\SOFTWARE\Microsoft\VisualStudio\Setup\CachePath` 已设置为 `D:\app_cache\vs\Packages`。
- 本次迁移创建的 C 盘备份目录已在验证通过后删除。
- C 盘剩余空间从约 4.94 GB 提升到约 8.21 GB。

迁移后已验证：

```powershell
rustup show home
cargo -V
rustc -V
pnpm build
pnpm tauri info
cargo check --manifest-path src-tauri\Cargo.toml
```

注意：Codex 沙箱内的普通命令访问 `C:\Users\19150\.cargo` 和 `C:\Users\19150\.rustup` Junction 时可能出现 `Access to the path is denied` 或 rustup home 创建误报；沙箱外同一用户验证正常。后续验证 Rust/Cargo 时，如遇该误报，优先用沙箱外命令确认真实系统状态。

### 后续策略

对 VS 包缓存，官方支持禁用或移动包缓存。包缓存的价值主要是离线修复；在网络可用时，安装器可以重新下载所需包。当前已采用 `CachePath` 迁移到 D 盘，而不是禁用缓存。

如果后续发现其他 AppData 或工具缓存继续挤占 C 盘，继续使用“复制到 D 盘、验证、原路径改 Junction、验证写入、再删除备份”的流程。不要迁移 whole app root，优先迁移可确认的缓存目录。

### 备选策略：卸载后重装 VS Build Tools 到 D 盘

Visual Studio 官方规则是：安装位置只能在首次安装时选择；已安装实例不能原地改盘符。若要把 Build Tools 主体迁到 D 盘，安全流程是：

1. 导出或记录当前组件。
2. 通过 Visual Studio Installer 卸载当前 Build Tools。
3. 使用 `vs_buildtools.exe` 重新安装到 D 盘，并同时设置缓存和共享组件路径。
4. 重新运行 `pnpm build`、`pnpm tauri info`、`pnpm tauri dev`。

即使指定 D 盘，部分 SDK 或工具仍可能按组件规则安装到系统盘，因此不能期望 C 盘完全不再占用空间。

## 官方参考

- Visual Studio 安装位置：https://learn.microsoft.com/en-us/visualstudio/install/change-installation-locations?view=vs-2022
- Visual Studio 命令行参数：https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio?view=vs-2022
- Visual Studio 包缓存：https://learn.microsoft.com/en-us/visualstudio/install/disable-or-move-the-package-cache?view=vs-2022
