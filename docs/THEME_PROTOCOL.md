# ReadRayThemeV1 主题协议

ReadRayThemeV1 是 ReadRay 唯一稳定的内部主题协议。主题包由同一目录中的 `manifest.json` 与 `theme.css` 组成；ReadRay 只把 `theme.css` 当作文本解析，绝不把原始 CSS 插入页面或交给浏览器执行。

当前随包提供 30 个内置主题：`ReadRay Default`（浅色）、`Flexoki`（浅色/深色双模式）和 28 个 Codex 预设主题（来自 OpenAI Codex 应用的代码配色主题，如 Ayu、Catppuccin、Dracula、GitHub、Nord、Solarized、Tokyo Night 等，各按注册表保留真实 light/dark 可用性）。内置主题数据由 `scripts/gen-themes.mjs` 从 `scripts/codex-theme-extract/core-palette.json` 生成，Rust 与前端共用同一份数据，不执行或导入原始 CSS/JS/TextMate scope。`ReadRay Default` 始终是默认主题；所有内置主题都不可删除，且自定义主题不能使用任何内置主题 ID。Codex、Obsidian 或其他来源以后必须通过各自独立 adapter 转换为 ReadRayThemeV1，本协议不承诺兼容任何外部主题格式。

## manifest.json

示例：

```json
{
  "formatVersion": 1,
  "id": "my-local-theme",
  "name": "My Local Theme",
  "version": "1.0.0",
  "author": "Author Name",
  "modes": ["light"],
  "license": "MIT",
  "sourceUrl": "https://example.com/theme"
}
```

必填字段：

- `formatVersion`：必须为整数 `1`。
- `id`：1–64 个字符，只允许小写 ASCII 字母、数字和连字符，且必须以字母或数字开头；`readray-default`、`flexoki` 与全部 28 个 Codex 预设主题 ID 均保留给内置主题。
- `name`：1–80 个字符。
- `version`：1–32 个字符。
- `author`：1–80 个字符。
- `modes`：非空且不重复，只允许 `light`、`dark`，最多两个。

可选字段：

- `license`：1–80 个字符。
- `sourceUrl`：最长 2048 个字符，只允许 `http://` 或 `https://`。它仅作为来源元数据保存，ReadRay 不会访问或下载该地址。

manifest 不接受未知字段、空必填字段或控制字符。

## theme.css

允许的选择器只有 `:root`、`body`、`.theme-light` 和 `.theme-dark`。`:root` 与 `body` 是所有已声明模式的基础值；对应模式选择器可以覆盖基础值。其他选择器会被忽略并返回警告。

示例：

```css
:root {
  --rr-theme-canvas: #f2f1ed;
  --rr-theme-sidebar: #ebeae5;
  --rr-theme-surface: #e6e5e0;
  --rr-theme-text-primary: #26251e;
  --rr-theme-text-secondary: rgba(38, 37, 30, 0.9);
  --rr-theme-border: rgba(38, 37, 30, 0.1);
  --rr-theme-accent: #f54e00;
}

.theme-light {
  --rr-theme-selection: rgba(245, 78, 0, 0.12);
}
```

只接受以下严格颜色格式，并在保存前规范化：

- `#RGB`、`#RGBA`、`#RRGGBB`、`#RRGGBBAA`；
- 0–255 整数分量的 `rgb(r, g, b)`；
- 0–255 整数分量和 0–1 alpha 的 `rgba(r, g, b, a)`。

规范化结果使用唯一写法：十六进制颜色转为小写、移除不必要的 `ff` alpha，并在每个通道可缩写时使用短格式；`rgb()` 分量不保留前导零；`rgba()` alpha 只输出 `0`、`1` 或 `0.` 开头且没有尾随零的小数。因此 `rgba(1, 2, 3, 00.5000)` 会保存为 `rgba(1, 2, 3, 0.5)`。前端只接受这些 Rust 可输出的规范形式，不会再次放宽颜色语法。

### 必填变量

每个已声明模式最终都必须具备：

- `--rr-theme-canvas`
- `--rr-theme-sidebar`
- `--rr-theme-surface`
- `--rr-theme-text-primary`
- `--rr-theme-text-secondary`
- `--rr-theme-border`
- `--rr-theme-accent`

### 可选变量与回退

| 变量 | 缺失时回退 |
| --- | --- |
| `--rr-theme-surface-elevated` | `surface` |
| `--rr-theme-surface-subtle` | `surface` |
| `--rr-theme-surface-contrast` | `surface` |
| `--rr-theme-text-muted` | `text-secondary` |
| `--rr-theme-text-subtle` | `text-muted`，再回退 `text-secondary` |
| `--rr-theme-border-soft` | `border` |
| `--rr-theme-accent-hover` | `accent` |
| `--rr-theme-accent-text` | `canvas` |
| `--rr-theme-success`、`--rr-theme-warning`、`--rr-theme-danger` | `accent` |
| `--rr-theme-success-soft`、`--rr-theme-warning-soft`、`--rr-theme-danger-soft` | `border` |
| `--rr-theme-warning-strong` | `warning` |
| `--rr-theme-danger-strong` | `danger` |
| `--rr-theme-selection` | `border` |
| `--rr-theme-diff-added` | `success` |
| `--rr-theme-diff-removed` | `danger` |
| `--rr-theme-scrim`、`--rr-theme-shadow` | `border` |

导入时会把这些回退解析为完整规范化数据后再保存。因此旧包缺少可选 token 时仍可恢复；以后新增 token 时也必须提供同样的确定性回退，不能要求用户执行原始 CSS。

## 安全边界

- `manifest.json` 上限 16 KiB，`theme.css` 上限 64 KiB；最多保存 64 个自定义主题。
- 单个 CSS 最多 128 条声明、64 条白名单主题变量；同一基础分区或模式分区内的重复白名单变量直接拒绝。
- 禁止所有 at-rule，包括 `@import` 和 `@font-face`；禁止 `url()`、脚本、远程字体、图片、表达式和网络请求。
- 普通 CSS 属性和未知主题变量不进入运行时，只返回警告；未知选择器的整个规则块均忽略。
- 禁止嵌套规则和未配对花括号。注释和普通空白允许。
- `canvas`、`sidebar`、`surface` 必须不透明；主文字与这三个背景的对比度均不得低于 4.5:1。
- Rust 只读取用户在原生目录对话框中明确选择目录直属的 `manifest.json` 和 `theme.css`；两个文件必须是普通文件，符号链接和目录外路径均拒绝。
- 导入在写入前由 Rust 安全预检同一目录并返回规范化目标；正式写入会重新解析目录并核对主题 ID，前端不会自行读取文件。该目标身份同时用于 IPC 报错后的精确对账，不能用任意新增主题推断本次导入成功。
- SQLite v8 只保存规范化 manifest、light/dark 颜色和解析警告，不保存原始 CSS。内置主题（ReadRay Default、Flexoki 与 28 个 Codex 预设）只存在于代码资源中，不写入 SQLite，也不重复保存。
- 页面只通过 `ThemeService → ThemeRepository → typed Rust commands` 操作主题，不直接 invoke、读文件或写 SQLite。
- 解析、选择或应用失败时，应用级协调器重读并恢复 SQLite 权威主题；只有 revision 恰好推进一次且目标主题、当前选择和主题存在性满足本次操作的完整后置条件时，才确认“已提交但调用方未确认”。数据库未变化时保留显式重试，并发冲突则不自动重试；旧结果不能覆盖较新的成功操作。

## 外部 adapter 边界

未来每种来源使用独立 adapter，例如 Obsidian adapter 或 Codex adapter。adapter 的职责仅是把来源事实映射为 ReadRayThemeV1；映射结果仍必须进入同一安全校验、规范化和持久化路径。

adapter 不得：

- 扩大允许选择器或变量白名单；
- 传递布局、字体、字号、图片、远程资源或任意 CSS；
- 绕过可读性、大小、路径、revision 或重复 ID 校验；
- 让 ReadRay 运行时长期依赖外部协议。
