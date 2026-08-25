use crate::learning_records;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

#[path = "codex_themes_data.rs"]
mod codex_themes_data;
use codex_themes_data::CODEX_BUILTIN_FULL_THEMES;

const THEME_FORMAT_VERSION: i64 = 1;
const DEFAULT_THEME_ID: &str = "readray-default";
const FLEXOKI_THEME_ID: &str = "flexoki";
// 仅用于从旧版本已保存的单模式 Codex 主题选择平滑回退；这些 ID 不再进入随包主题列表。
const RETIRED_BUILTIN_THEME_IDS: &[&str] = &[
    "ayu",
    "dracula",
    "lobster",
    "material",
    "matrix",
    "monokai",
    "night-owl",
    "nord",
    "oscurange",
    "proof",
    "sentry",
    "tokyo-night",
    "temple",
];
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
const MAX_CSS_BYTES: u64 = 64 * 1024;
const MAX_CUSTOM_THEMES: i64 = 64;
const MAX_DECLARATIONS: usize = 128;
const MAX_THEME_VARIABLES: usize = 64;
const MIN_TEXT_CONTRAST: f64 = 4.5;

fn builtin_theme_ids() -> Vec<&'static str> {
    let mut ids = vec![DEFAULT_THEME_ID, FLEXOKI_THEME_ID];
    ids.extend(
        CODEX_BUILTIN_FULL_THEMES
            .iter()
            .map(|(manifest, _, _)| manifest.id),
    );
    ids
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
}

impl ThemeMode {
    fn storage_value(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    fn from_storage(value: &str) -> Result<Self, String> {
        match value {
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err("数据库包含未知的主题模式。".to_string()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadRayThemeManifestV1 {
    format_version: i64,
    id: String,
    name: String,
    version: String,
    author: String,
    modes: Vec<ThemeMode>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    source_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadRayThemeColors {
    canvas: String,
    sidebar: String,
    surface: String,
    surface_elevated: String,
    surface_subtle: String,
    surface_contrast: String,
    text_primary: String,
    text_secondary: String,
    text_muted: String,
    text_subtle: String,
    border: String,
    border_soft: String,
    accent: String,
    accent_hover: String,
    accent_text: String,
    success: String,
    success_soft: String,
    warning: String,
    warning_soft: String,
    warning_strong: String,
    danger: String,
    danger_soft: String,
    danger_strong: String,
    selection: String,
    diff_added: String,
    diff_removed: String,
    scrim: String,
    shadow: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadRayThemeV1 {
    manifest: ReadRayThemeManifestV1,
    light: Option<ReadRayThemeColors>,
    dark: Option<ReadRayThemeColors>,
    builtin: bool,
    warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSnapshot {
    revision: i64,
    current_theme_id: String,
    current_mode: ThemeMode,
    themes: Vec<ReadRayThemeV1>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ThemeColorKey {
    Canvas,
    Sidebar,
    Surface,
    SurfaceElevated,
    SurfaceSubtle,
    SurfaceContrast,
    TextPrimary,
    TextSecondary,
    TextMuted,
    TextSubtle,
    Border,
    BorderSoft,
    Accent,
    AccentHover,
    AccentText,
    Success,
    SuccessSoft,
    Warning,
    WarningSoft,
    WarningStrong,
    Danger,
    DangerSoft,
    DangerStrong,
    Selection,
    DiffAdded,
    DiffRemoved,
    Scrim,
    Shadow,
}

impl ThemeColorKey {
    fn from_css_name(value: &str) -> Option<Self> {
        match value {
            "--rr-theme-canvas" => Some(Self::Canvas),
            "--rr-theme-sidebar" => Some(Self::Sidebar),
            "--rr-theme-surface" => Some(Self::Surface),
            "--rr-theme-surface-elevated" => Some(Self::SurfaceElevated),
            "--rr-theme-surface-subtle" => Some(Self::SurfaceSubtle),
            "--rr-theme-surface-contrast" => Some(Self::SurfaceContrast),
            "--rr-theme-text-primary" => Some(Self::TextPrimary),
            "--rr-theme-text-secondary" => Some(Self::TextSecondary),
            "--rr-theme-text-muted" => Some(Self::TextMuted),
            "--rr-theme-text-subtle" => Some(Self::TextSubtle),
            "--rr-theme-border" => Some(Self::Border),
            "--rr-theme-border-soft" => Some(Self::BorderSoft),
            "--rr-theme-accent" => Some(Self::Accent),
            "--rr-theme-accent-hover" => Some(Self::AccentHover),
            "--rr-theme-accent-text" => Some(Self::AccentText),
            "--rr-theme-success" => Some(Self::Success),
            "--rr-theme-success-soft" => Some(Self::SuccessSoft),
            "--rr-theme-warning" => Some(Self::Warning),
            "--rr-theme-warning-soft" => Some(Self::WarningSoft),
            "--rr-theme-warning-strong" => Some(Self::WarningStrong),
            "--rr-theme-danger" => Some(Self::Danger),
            "--rr-theme-danger-soft" => Some(Self::DangerSoft),
            "--rr-theme-danger-strong" => Some(Self::DangerStrong),
            "--rr-theme-selection" => Some(Self::Selection),
            "--rr-theme-diff-added" => Some(Self::DiffAdded),
            "--rr-theme-diff-removed" => Some(Self::DiffRemoved),
            "--rr-theme-scrim" => Some(Self::Scrim),
            "--rr-theme-shadow" => Some(Self::Shadow),
            _ => None,
        }
    }
}

type ColorMap = BTreeMap<&'static str, String>;

fn default_theme() -> ReadRayThemeV1 {
    ReadRayThemeV1 {
        manifest: ReadRayThemeManifestV1 {
            format_version: THEME_FORMAT_VERSION,
            id: DEFAULT_THEME_ID.to_string(),
            name: "ReadRay Default".to_string(),
            version: "1.1.0".to_string(),
            author: "ReadRay".to_string(),
            modes: vec![ThemeMode::Light, ThemeMode::Dark],
            license: None,
            source_url: None,
        },
        light: Some(ReadRayThemeColors {
            canvas: "#f2f1ed".to_string(),
            sidebar: "#ebeae5".to_string(),
            surface: "#e6e5e0".to_string(),
            surface_elevated: "#ebeae5".to_string(),
            surface_subtle: "#f0efeb".to_string(),
            surface_contrast: "#fff".to_string(),
            text_primary: "#26251e".to_string(),
            text_secondary: "rgba(38, 37, 30, 0.9)".to_string(),
            text_muted: "rgba(38, 37, 30, 0.55)".to_string(),
            text_subtle: "rgba(38, 37, 30, 0.4)".to_string(),
            border: "rgba(38, 37, 30, 0.1)".to_string(),
            border_soft: "rgba(38, 37, 30, 0.06)".to_string(),
            accent: "#f54e00".to_string(),
            accent_hover: "#e84800".to_string(),
            accent_text: "#fff".to_string(),
            success: "#277250".to_string(),
            success_soft: "rgba(39, 114, 80, 0.11)".to_string(),
            warning: "#9a6400".to_string(),
            warning_soft: "rgba(154, 100, 0, 0.11)".to_string(),
            warning_strong: "#eab308".to_string(),
            danger: "#cf2d56".to_string(),
            danger_soft: "rgba(207, 45, 86, 0.09)".to_string(),
            danger_strong: "#a2382a".to_string(),
            selection: "rgba(245, 78, 0, 0.12)".to_string(),
            diff_added: "#1f8a65".to_string(),
            diff_removed: "#cf2d56".to_string(),
            scrim: "rgba(28, 27, 23, 0.32)".to_string(),
            shadow: "rgba(38, 37, 30, 0.1)".to_string(),
        }),
        dark: Some(ReadRayThemeColors {
            canvas: "#0d0d0b".to_string(),
            sidebar: "#171512".to_string(),
            surface: "#1f1b18".to_string(),
            surface_elevated: "#27211d".to_string(),
            surface_subtle: "#171512".to_string(),
            surface_contrast: "#332821".to_string(),
            text_primary: "#f6f0e8".to_string(),
            text_secondary: "#d5c9bb".to_string(),
            text_muted: "#8f8579".to_string(),
            text_subtle: "#6f665c".to_string(),
            border: "rgba(246, 240, 232, 0.12)".to_string(),
            border_soft: "rgba(246, 240, 232, 0.07)".to_string(),
            accent: "#ff6a32".to_string(),
            accent_hover: "#ff8150".to_string(),
            accent_text: "#0d0d0b".to_string(),
            success: "#68c08d".to_string(),
            success_soft: "rgba(104, 192, 141, 0.14)".to_string(),
            warning: "#e3ab52".to_string(),
            warning_soft: "rgba(227, 171, 82, 0.14)".to_string(),
            warning_strong: "#f2c25f".to_string(),
            danger: "#ef7783".to_string(),
            danger_soft: "rgba(239, 119, 131, 0.14)".to_string(),
            danger_strong: "#ff9a8b".to_string(),
            selection: "rgba(255, 106, 50, 0.22)".to_string(),
            diff_added: "#72c99a".to_string(),
            diff_removed: "#ef7783".to_string(),
            scrim: "rgba(0, 0, 0, 0.5)".to_string(),
            shadow: "rgba(0, 0, 0, 0.32)".to_string(),
        }),
        builtin: true,
        warnings: Vec::new(),
    }
}

fn flexoki_theme() -> ReadRayThemeV1 {
    ReadRayThemeV1 {
        manifest: ReadRayThemeManifestV1 {
            format_version: THEME_FORMAT_VERSION,
            id: FLEXOKI_THEME_ID.to_string(),
            name: "Flexoki".to_string(),
            version: "1.1.0".to_string(),
            author: "Steph Ango".to_string(),
            modes: vec![ThemeMode::Light, ThemeMode::Dark],
            license: Some("MIT".to_string()),
            source_url: Some("https://stephango.com/flexoki".to_string()),
        },
        light: Some(ReadRayThemeColors {
            canvas: "#fffcf0".to_string(),
            sidebar: "#f2f0e5".to_string(),
            surface: "#f2f0e5".to_string(),
            surface_elevated: "#f2f0e5".to_string(),
            surface_subtle: "#fffcf0".to_string(),
            surface_contrast: "#e6e4d9".to_string(),
            text_primary: "#100f0f".to_string(),
            text_secondary: "#575653".to_string(),
            text_muted: "#878580".to_string(),
            text_subtle: "#b7b5ac".to_string(),
            border: "#dad8ce".to_string(),
            border_soft: "#e6e4d9".to_string(),
            accent: "#24837b".to_string(),
            accent_hover: "#24837b".to_string(),
            accent_text: "#fffcf0".to_string(),
            success: "#66800b".to_string(),
            success_soft: "#f2f0e5".to_string(),
            warning: "#ad8301".to_string(),
            warning_soft: "#f2f0e5".to_string(),
            warning_strong: "#ad8301".to_string(),
            danger: "#af3029".to_string(),
            danger_soft: "#f2f0e5".to_string(),
            danger_strong: "#af3029".to_string(),
            selection: "#f2f0e5".to_string(),
            diff_added: "#66800b".to_string(),
            diff_removed: "#af3029".to_string(),
            scrim: "#100f0f".to_string(),
            shadow: "#100f0f".to_string(),
        }),
        dark: Some(ReadRayThemeColors {
            canvas: "#100f0f".to_string(),
            sidebar: "#1c1b1a".to_string(),
            surface: "#1c1b1a".to_string(),
            surface_elevated: "#1c1b1a".to_string(),
            surface_subtle: "#100f0f".to_string(),
            surface_contrast: "#282726".to_string(),
            text_primary: "#cecdc3".to_string(),
            text_secondary: "#878580".to_string(),
            text_muted: "#6f6e69".to_string(),
            text_subtle: "#575653".to_string(),
            border: "#343331".to_string(),
            border_soft: "#282726".to_string(),
            accent: "#3aa99f".to_string(),
            accent_hover: "#3aa99f".to_string(),
            accent_text: "#100f0f".to_string(),
            success: "#879a39".to_string(),
            success_soft: "#1c1b1a".to_string(),
            warning: "#d0a215".to_string(),
            warning_soft: "#1c1b1a".to_string(),
            warning_strong: "#d0a215".to_string(),
            danger: "#d14d41".to_string(),
            danger_soft: "#1c1b1a".to_string(),
            danger_strong: "#d14d41".to_string(),
            selection: "#282726".to_string(),
            diff_added: "#879a39".to_string(),
            diff_removed: "#d14d41".to_string(),
            scrim: "#000".to_string(),
            shadow: "#000".to_string(),
        }),
        builtin: true,
        warnings: Vec::new(),
    }
}

/// 随包 Codex 内置主题的完整展开配色（&'static str，满足 const 静态表）。
/// 由 scripts/gen-themes.mjs 从 scripts/codex-theme-extract/core-palette.json 生成，
/// 与前端 src/codexThemeData.ts 保持字节级一致（避免运行时派生的浮点分叉）。
#[derive(Clone, Debug)]
struct CodexThemeFullColors {
    canvas: &'static str,
    sidebar: &'static str,
    surface: &'static str,
    surface_elevated: &'static str,
    surface_subtle: &'static str,
    surface_contrast: &'static str,
    text_primary: &'static str,
    text_secondary: &'static str,
    text_muted: &'static str,
    text_subtle: &'static str,
    border: &'static str,
    border_soft: &'static str,
    accent: &'static str,
    accent_hover: &'static str,
    accent_text: &'static str,
    success: &'static str,
    success_soft: &'static str,
    warning: &'static str,
    warning_soft: &'static str,
    warning_strong: &'static str,
    danger: &'static str,
    danger_soft: &'static str,
    danger_strong: &'static str,
    selection: &'static str,
    diff_added: &'static str,
    diff_removed: &'static str,
    scrim: &'static str,
    shadow: &'static str,
}

impl CodexThemeFullColors {
    fn to_colors(&self) -> ReadRayThemeColors {
        ReadRayThemeColors {
            canvas: self.canvas.to_string(),
            sidebar: self.sidebar.to_string(),
            surface: self.surface.to_string(),
            surface_elevated: self.surface_elevated.to_string(),
            surface_subtle: self.surface_subtle.to_string(),
            surface_contrast: self.surface_contrast.to_string(),
            text_primary: self.text_primary.to_string(),
            text_secondary: self.text_secondary.to_string(),
            text_muted: self.text_muted.to_string(),
            text_subtle: self.text_subtle.to_string(),
            border: self.border.to_string(),
            border_soft: self.border_soft.to_string(),
            accent: self.accent.to_string(),
            accent_hover: self.accent_hover.to_string(),
            accent_text: self.accent_text.to_string(),
            success: self.success.to_string(),
            success_soft: self.success_soft.to_string(),
            warning: self.warning.to_string(),
            warning_soft: self.warning_soft.to_string(),
            warning_strong: self.warning_strong.to_string(),
            danger: self.danger.to_string(),
            danger_soft: self.danger_soft.to_string(),
            danger_strong: self.danger_strong.to_string(),
            selection: self.selection.to_string(),
            diff_added: self.diff_added.to_string(),
            diff_removed: self.diff_removed.to_string(),
            scrim: self.scrim.to_string(),
            shadow: self.shadow.to_string(),
        }
    }
}

/// 数据驱动注册表用的精简 manifest（&'static str，满足 const 静态表）。
#[derive(Clone, Copy, Debug)]
struct CodexThemeManifest {
    id: &'static str,
    name: &'static str,
    version: &'static str,
    author: &'static str,
    modes: &'static [ThemeMode],
    license: Option<&'static str>,
    source_url: Option<&'static str>,
}

fn codex_builtin_themes() -> Vec<ReadRayThemeV1> {
    CODEX_BUILTIN_FULL_THEMES
        .iter()
        .map(|(manifest, dark, light)| ReadRayThemeV1 {
            manifest: ReadRayThemeManifestV1 {
                format_version: THEME_FORMAT_VERSION,
                id: manifest.id.to_string(),
                name: manifest.name.to_string(),
                version: manifest.version.to_string(),
                author: manifest.author.to_string(),
                modes: manifest.modes.to_vec(),
                license: manifest.license.map(str::to_string),
                source_url: manifest.source_url.map(str::to_string),
            },
            light: light.as_ref().map(CodexThemeFullColors::to_colors),
            dark: dark.as_ref().map(CodexThemeFullColors::to_colors),
            builtin: true,
            warnings: Vec::new(),
        })
        .collect()
}

fn validate_text_field(value: &str, label: &str, maximum: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > maximum {
        return Err(format!("{label}不能为空且不能超过 {maximum} 个字符。"));
    }
    if value.chars().any(|character| character.is_control()) {
        return Err(format!("{label}不能包含控制字符。"));
    }
    Ok(())
}

fn validate_manifest(manifest: &ReadRayThemeManifestV1) -> Result<(), String> {
    if manifest.format_version != THEME_FORMAT_VERSION {
        return Err(format!(
            "仅支持 ReadRayThemeV1（formatVersion = {THEME_FORMAT_VERSION}）。"
        ));
    }
    validate_text_field(&manifest.id, "主题 ID", 64)?;
    let mut id_characters = manifest.id.chars();
    if !id_characters
        .next()
        .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        || !manifest.id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        return Err(
            "主题 ID 只能使用小写 ASCII 字母、数字和连字符，且必须以字母或数字开头。".to_string(),
        );
    }
    if builtin_theme_ids().contains(&manifest.id.as_str()) {
        return Err(format!(
            "自定义主题不能使用内置主题 {} 的 ID。",
            manifest.id
        ));
    }
    validate_text_field(&manifest.name, "主题名称", 80)?;
    validate_text_field(&manifest.version, "主题版本", 32)?;
    validate_text_field(&manifest.author, "主题作者", 80)?;
    if manifest.modes.is_empty() || manifest.modes.len() > 2 {
        return Err("主题必须声明 light 和/或 dark 模式。".to_string());
    }
    let mut modes = HashSet::new();
    if manifest.modes.iter().any(|mode| !modes.insert(*mode)) {
        return Err("主题 modes 不能重复。".to_string());
    }
    if let Some(license) = &manifest.license {
        validate_text_field(license, "主题许可证", 80)?;
    }
    if let Some(source_url) = &manifest.source_url {
        validate_text_field(source_url, "主题来源 URL", 2_048)?;
        if !(source_url.starts_with("https://") || source_url.starts_with("http://")) {
            return Err("sourceUrl 只允许 http:// 或 https:// 地址。".to_string());
        }
    }
    Ok(())
}

fn strip_css_comments(css: &str) -> Result<String, String> {
    let bytes = css.as_bytes();
    let mut output = String::with_capacity(css.len());
    let mut index = 0;
    while index < bytes.len() {
        if index + 1 < bytes.len() && bytes[index] == b'/' && bytes[index + 1] == b'*' {
            let Some(relative_end) = css[index + 2..].find("*/") else {
                return Err("theme.css 包含未闭合注释。".to_string());
            };
            index += 2 + relative_end + 2;
        } else {
            let character = css[index..]
                .chars()
                .next()
                .ok_or_else(|| "theme.css 包含无效 UTF-8。".to_string())?;
            output.push(character);
            index += character.len_utf8();
        }
    }
    Ok(output)
}

fn reject_executable_css(css: &str) -> Result<(), String> {
    if css
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("theme.css 包含不允许的控制字符。".to_string());
    }
    let lower = css.to_ascii_lowercase();
    let compact: String = lower
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    if lower.contains('@') {
        return Err("theme.css 禁止 @import、@font-face 及其他 at-rule。".to_string());
    }
    if compact.contains("url(")
        || compact.contains("expression(")
        || compact.contains("javascript:")
        || compact.contains("<script")
        || compact.contains("</style")
    {
        return Err("theme.css 禁止 URL、脚本、远程字体、图片或可执行表达式。".to_string());
    }
    Ok(())
}

fn normalize_color(value: &str) -> Result<(String, [f64; 4]), String> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        if !matches!(hex.len(), 3 | 4 | 6 | 8)
            || !hex.chars().all(|character| character.is_ascii_hexdigit())
        {
            return Err("只允许 3/4/6/8 位十六进制颜色或严格 rgb()/rgba()。".to_string());
        }
        let expanded = match hex.len() {
            3 | 4 => hex
                .chars()
                .flat_map(|character| [character, character])
                .collect::<String>(),
            _ => hex.to_string(),
        };
        let component = |range: std::ops::Range<usize>| -> Result<f64, String> {
            u8::from_str_radix(&expanded[range], 16)
                .map(|number| f64::from(number) / 255.0)
                .map_err(|error| format!("颜色解析失败：{error}"))
        };
        let red = u8::from_str_radix(&expanded[0..2], 16)
            .map_err(|error| format!("颜色解析失败：{error}"))?;
        let green = u8::from_str_radix(&expanded[2..4], 16)
            .map_err(|error| format!("颜色解析失败：{error}"))?;
        let blue = u8::from_str_radix(&expanded[4..6], 16)
            .map_err(|error| format!("颜色解析失败：{error}"))?;
        let alpha_byte = if expanded.len() == 8 {
            u8::from_str_radix(&expanded[6..8], 16)
                .map_err(|error| format!("颜色解析失败：{error}"))?
        } else {
            u8::MAX
        };
        let channels = if alpha_byte == u8::MAX {
            vec![red, green, blue]
        } else {
            vec![red, green, blue, alpha_byte]
        };
        let can_shorten = channels
            .iter()
            .all(|channel| *channel >> 4 == *channel & 0x0f);
        let normalized = if can_shorten {
            format!(
                "#{}",
                channels
                    .iter()
                    .map(|channel| format!("{:x}", channel >> 4))
                    .collect::<String>()
            )
        } else {
            format!(
                "#{}",
                channels
                    .iter()
                    .map(|channel| format!("{channel:02x}"))
                    .collect::<String>()
            )
        };
        return Ok((
            normalized,
            [
                component(0..2)?,
                component(2..4)?,
                component(4..6)?,
                f64::from(alpha_byte) / 255.0,
            ],
        ));
    }

    let lower = value.to_ascii_lowercase();
    let (function, body) = if let Some(body) = lower
        .strip_prefix("rgba(")
        .and_then(|body| body.strip_suffix(')'))
    {
        ("rgba", body)
    } else if let Some(body) = lower
        .strip_prefix("rgb(")
        .and_then(|body| body.strip_suffix(')'))
    {
        ("rgb", body)
    } else {
        return Err("只允许十六进制颜色或严格 rgb()/rgba()。".to_string());
    };
    let parts: Vec<_> = body.split(',').map(str::trim).collect();
    let expected = if function == "rgba" { 4 } else { 3 };
    if parts.len() != expected {
        return Err("rgb()/rgba() 颜色分量数量无效。".to_string());
    }
    let parse_byte = |part: &str| -> Result<u8, String> {
        if part.is_empty() || !part.chars().all(|character| character.is_ascii_digit()) {
            return Err("RGB 分量必须是 0–255 的整数。".to_string());
        }
        part.parse::<u8>()
            .map_err(|_| "RGB 分量必须是 0–255 的整数。".to_string())
    };
    let red = parse_byte(parts[0])?;
    let green = parse_byte(parts[1])?;
    let blue = parse_byte(parts[2])?;
    let (alpha, normalized_alpha) = if function == "rgba" {
        if parts[3].is_empty()
            || !parts[3]
                .chars()
                .all(|character| character.is_ascii_digit() || character == '.')
            || parts[3].matches('.').count() > 1
        {
            return Err("Alpha 分量必须是 0–1 的小数。".to_string());
        }
        let alpha = parts[3]
            .parse::<f64>()
            .map_err(|_| "Alpha 分量必须是 0–1 的小数。".to_string())?;
        if !(0.0..=1.0).contains(&alpha) {
            return Err("Alpha 分量必须是 0–1 的小数。".to_string());
        }
        let normalized_alpha = if alpha == 0.0 {
            "0".to_string()
        } else if alpha == 1.0 {
            "1".to_string()
        } else {
            let fraction = parts[3]
                .split_once('.')
                .map(|(_, fraction)| fraction)
                .ok_or_else(|| "Alpha 分量必须是 0–1 的小数。".to_string())?
                .trim_end_matches('0');
            format!("0.{fraction}")
        };
        (alpha, Some(normalized_alpha))
    } else {
        (1.0, None)
    };
    let normalized = if function == "rgba" {
        format!(
            "rgba({red}, {green}, {blue}, {})",
            normalized_alpha.expect("rgba alpha was normalized")
        )
    } else {
        format!("rgb({red}, {green}, {blue})")
    };
    Ok((
        normalized,
        [
            f64::from(red) / 255.0,
            f64::from(green) / 255.0,
            f64::from(blue) / 255.0,
            alpha,
        ],
    ))
}

fn color_map_key(key: ThemeColorKey) -> &'static str {
    match key {
        ThemeColorKey::Canvas => "canvas",
        ThemeColorKey::Sidebar => "sidebar",
        ThemeColorKey::Surface => "surface",
        ThemeColorKey::SurfaceElevated => "surfaceElevated",
        ThemeColorKey::SurfaceSubtle => "surfaceSubtle",
        ThemeColorKey::SurfaceContrast => "surfaceContrast",
        ThemeColorKey::TextPrimary => "textPrimary",
        ThemeColorKey::TextSecondary => "textSecondary",
        ThemeColorKey::TextMuted => "textMuted",
        ThemeColorKey::TextSubtle => "textSubtle",
        ThemeColorKey::Border => "border",
        ThemeColorKey::BorderSoft => "borderSoft",
        ThemeColorKey::Accent => "accent",
        ThemeColorKey::AccentHover => "accentHover",
        ThemeColorKey::AccentText => "accentText",
        ThemeColorKey::Success => "success",
        ThemeColorKey::SuccessSoft => "successSoft",
        ThemeColorKey::Warning => "warning",
        ThemeColorKey::WarningSoft => "warningSoft",
        ThemeColorKey::WarningStrong => "warningStrong",
        ThemeColorKey::Danger => "danger",
        ThemeColorKey::DangerSoft => "dangerSoft",
        ThemeColorKey::DangerStrong => "dangerStrong",
        ThemeColorKey::Selection => "selection",
        ThemeColorKey::DiffAdded => "diffAdded",
        ThemeColorKey::DiffRemoved => "diffRemoved",
        ThemeColorKey::Scrim => "scrim",
        ThemeColorKey::Shadow => "shadow",
    }
}

fn fallback(map: &ColorMap, primary: &'static str, secondary: &'static str) -> String {
    map.get(primary)
        .or_else(|| map.get(secondary))
        .expect("required theme fallback must exist")
        .clone()
}

fn build_colors(map: &ColorMap, mode: ThemeMode) -> Result<ReadRayThemeColors, String> {
    for required in [
        "canvas",
        "sidebar",
        "surface",
        "textPrimary",
        "textSecondary",
        "border",
        "accent",
    ] {
        if !map.contains_key(required) {
            return Err(format!(
                "{} 模式缺少必填主题变量：--rr-theme-{}。",
                mode.storage_value(),
                required
                    .chars()
                    .flat_map(|character| {
                        if character.is_ascii_uppercase() {
                            vec!['-', character.to_ascii_lowercase()]
                        } else {
                            vec![character]
                        }
                    })
                    .collect::<String>()
            ));
        }
    }
    let accent = map["accent"].clone();
    let surface = map["surface"].clone();
    let secondary = map["textSecondary"].clone();
    let border = map["border"].clone();
    let success = map
        .get("success")
        .cloned()
        .unwrap_or_else(|| accent.clone());
    let warning = map
        .get("warning")
        .cloned()
        .unwrap_or_else(|| accent.clone());
    let danger = map.get("danger").cloned().unwrap_or_else(|| accent.clone());
    Ok(ReadRayThemeColors {
        canvas: map["canvas"].clone(),
        sidebar: map["sidebar"].clone(),
        surface: surface.clone(),
        surface_elevated: fallback(map, "surfaceElevated", "surface"),
        surface_subtle: fallback(map, "surfaceSubtle", "surface"),
        surface_contrast: fallback(map, "surfaceContrast", "surface"),
        text_primary: map["textPrimary"].clone(),
        text_secondary: secondary.clone(),
        text_muted: map
            .get("textMuted")
            .cloned()
            .unwrap_or_else(|| secondary.clone()),
        text_subtle: map
            .get("textSubtle")
            .or_else(|| map.get("textMuted"))
            .cloned()
            .unwrap_or(secondary),
        border: border.clone(),
        border_soft: map
            .get("borderSoft")
            .cloned()
            .unwrap_or_else(|| border.clone()),
        accent: accent.clone(),
        accent_hover: map
            .get("accentHover")
            .cloned()
            .unwrap_or_else(|| accent.clone()),
        accent_text: map
            .get("accentText")
            .cloned()
            .unwrap_or_else(|| map["canvas"].clone()),
        success: success.clone(),
        success_soft: map
            .get("successSoft")
            .cloned()
            .unwrap_or_else(|| border.clone()),
        warning: warning.clone(),
        warning_soft: map
            .get("warningSoft")
            .cloned()
            .unwrap_or_else(|| border.clone()),
        warning_strong: map.get("warningStrong").cloned().unwrap_or(warning),
        danger: danger.clone(),
        danger_soft: map
            .get("dangerSoft")
            .cloned()
            .unwrap_or_else(|| border.clone()),
        danger_strong: map
            .get("dangerStrong")
            .cloned()
            .unwrap_or_else(|| danger.clone()),
        selection: map
            .get("selection")
            .cloned()
            .unwrap_or_else(|| border.clone()),
        diff_added: map.get("diffAdded").cloned().unwrap_or(success),
        diff_removed: map.get("diffRemoved").cloned().unwrap_or(danger),
        scrim: map.get("scrim").cloned().unwrap_or_else(|| border.clone()),
        shadow: map.get("shadow").cloned().unwrap_or(border),
    })
}

fn linear_component(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(color: [f64; 4]) -> f64 {
    0.2126 * linear_component(color[0])
        + 0.7152 * linear_component(color[1])
        + 0.0722 * linear_component(color[2])
}

fn contrast_ratio(foreground: [f64; 4], background: [f64; 4]) -> Result<f64, String> {
    if background[3] < 1.0 {
        return Err("canvas、sidebar 和 surface 必须使用不透明颜色。".to_string());
    }
    let composited = [
        foreground[0] * foreground[3] + background[0] * (1.0 - foreground[3]),
        foreground[1] * foreground[3] + background[1] * (1.0 - foreground[3]),
        foreground[2] * foreground[3] + background[2] * (1.0 - foreground[3]),
        1.0,
    ];
    let foreground_luminance = luminance(composited);
    let background_luminance = luminance(background);
    let (lighter, darker) = if foreground_luminance >= background_luminance {
        (foreground_luminance, background_luminance)
    } else {
        (background_luminance, foreground_luminance)
    };
    Ok((lighter + 0.05) / (darker + 0.05))
}

fn validate_readability(colors: &ReadRayThemeColors, mode: ThemeMode) -> Result<(), String> {
    let (_, primary) = normalize_color(&colors.text_primary)?;
    for (label, value) in [
        ("canvas", &colors.canvas),
        ("sidebar", &colors.sidebar),
        ("surface", &colors.surface),
    ] {
        let (_, background) = normalize_color(value)?;
        let contrast = contrast_ratio(primary, background)?;
        if contrast < MIN_TEXT_CONTRAST {
            return Err(format!(
                "{} 模式的主文字与 {label} 对比度仅为 {contrast:.2}:1，至少需要 {MIN_TEXT_CONTRAST:.1}:1。",
                mode.storage_value()
            ));
        }
    }
    Ok(())
}

fn parse_theme_css(
    manifest: &ReadRayThemeManifestV1,
    css: &str,
) -> Result<
    (
        Option<ReadRayThemeColors>,
        Option<ReadRayThemeColors>,
        Vec<String>,
    ),
    String,
> {
    let stripped = strip_css_comments(css)?;
    reject_executable_css(&stripped)?;

    let mut base: ColorMap = BTreeMap::new();
    let mut light: ColorMap = BTreeMap::new();
    let mut dark: ColorMap = BTreeMap::new();
    let mut duplicates = HashSet::<(String, ThemeColorKey)>::new();
    let mut warnings = Vec::new();
    let mut declaration_count = 0;
    let mut accepted_count = 0;
    let mut remaining = stripped.as_str();

    while !remaining.trim().is_empty() {
        remaining = remaining.trim_start();
        let open = remaining
            .find('{')
            .ok_or_else(|| "theme.css 在选择器后缺少左花括号。".to_string())?;
        if remaining[..open].contains('}') {
            return Err("theme.css 包含未配对花括号。".to_string());
        }
        let selector = remaining[..open].trim();
        if selector.is_empty() || selector.len() > 128 {
            return Err("theme.css 选择器为空或过长。".to_string());
        }
        let after_open = &remaining[open + 1..];
        let close = after_open
            .find('}')
            .ok_or_else(|| "theme.css 包含未闭合规则块。".to_string())?;
        if after_open[..close].contains('{') {
            return Err("theme.css 禁止嵌套规则和任意 CSS 结构。".to_string());
        }
        let declarations = &after_open[..close];
        remaining = &after_open[close + 1..];

        let selector_group = match selector {
            ":root" | "body" => Some("base"),
            ".theme-light" => Some("light"),
            ".theme-dark" => Some("dark"),
            _ => None,
        };
        if selector_group.is_none() {
            warnings.push(format!("已忽略不允许的选择器：{selector}"));
            continue;
        }

        for declaration in declarations
            .split(';')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            declaration_count += 1;
            if declaration_count > MAX_DECLARATIONS {
                return Err(format!("theme.css 声明数量不能超过 {MAX_DECLARATIONS}。"));
            }
            if declaration.len() > 1_024 {
                return Err("theme.css 单条声明不能超过 1024 字节。".to_string());
            }
            let Some((name, value)) = declaration.split_once(':') else {
                return Err(format!("theme.css 包含无效声明：{declaration}"));
            };
            let name = name.trim();
            if name.is_empty() || name.len() > 128 {
                return Err("theme.css 属性名为空或过长。".to_string());
            }
            if !name.starts_with("--") {
                warnings.push(format!("已忽略普通 CSS 属性：{name}"));
                continue;
            }
            let Some(key) = ThemeColorKey::from_css_name(name) else {
                warnings.push(format!("已忽略未知主题变量：{name}"));
                continue;
            };
            let group = selector_group.expect("allowed selector has group");
            if !duplicates.insert((group.to_string(), key)) {
                return Err(format!("theme.css 包含重复主题变量：{group} / {name}"));
            }
            accepted_count += 1;
            if accepted_count > MAX_THEME_VARIABLES {
                return Err(format!(
                    "theme.css 主题变量数量不能超过 {MAX_THEME_VARIABLES}。"
                ));
            }
            if value.trim().len() > 64 {
                return Err(format!("{name} 颜色值不能超过 64 字节。"));
            }
            let (normalized, _) =
                normalize_color(value).map_err(|error| format!("{name} 颜色无效：{error}"))?;
            let target = match group {
                "base" => &mut base,
                "light" => &mut light,
                "dark" => &mut dark,
                _ => unreachable!(),
            };
            target.insert(color_map_key(key), normalized);
        }
    }

    let supports_light = manifest.modes.contains(&ThemeMode::Light);
    let supports_dark = manifest.modes.contains(&ThemeMode::Dark);
    if !supports_light && !light.is_empty() {
        warnings.push("已忽略 manifest 未声明的 .theme-light 配色。".to_string());
    }
    if !supports_dark && !dark.is_empty() {
        warnings.push("已忽略 manifest 未声明的 .theme-dark 配色。".to_string());
    }
    let build_mode = |mode_values: &ColorMap, mode: ThemeMode| {
        let mut merged = base.clone();
        merged.extend(mode_values.clone());
        let colors = build_colors(&merged, mode)?;
        validate_readability(&colors, mode)?;
        Ok::<_, String>(colors)
    };
    let light = if supports_light {
        Some(build_mode(&light, ThemeMode::Light)?)
    } else {
        None
    };
    let dark = if supports_dark {
        Some(build_mode(&dark, ThemeMode::Dark)?)
    } else {
        None
    };
    warnings.sort();
    warnings.dedup();
    Ok((light, dark, warnings))
}

fn parse_theme_package_files(manifest_json: &str, css: &str) -> Result<ReadRayThemeV1, String> {
    if manifest_json.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(format!(
            "manifest.json 不能超过 {MAX_MANIFEST_BYTES} 字节。"
        ));
    }
    if css.len() as u64 > MAX_CSS_BYTES {
        return Err(format!("theme.css 不能超过 {MAX_CSS_BYTES} 字节。"));
    }
    let manifest: ReadRayThemeManifestV1 = serde_json::from_str(manifest_json)
        .map_err(|error| format!("manifest.json 不是有效的 ReadRayThemeV1：{error}"))?;
    validate_manifest(&manifest)?;
    let (light, dark, warnings) = parse_theme_css(&manifest, css)?;
    Ok(ReadRayThemeV1 {
        manifest,
        light,
        dark,
        builtin: false,
        warnings,
    })
}

fn read_direct_package_file(
    directory: &Path,
    file_name: &str,
    maximum_bytes: u64,
) -> Result<String, String> {
    let path = directory.join(file_name);
    let symlink_metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("无法读取主题包中的 {file_name}：{error}"))?;
    if symlink_metadata.file_type().is_symlink() || !symlink_metadata.is_file() {
        return Err(format!(
            "{file_name} 必须是所选目录内的普通文件，不能是符号链接。"
        ));
    }
    if symlink_metadata.len() > maximum_bytes {
        return Err(format!("{file_name} 不能超过 {maximum_bytes} 字节。"));
    }
    let canonical_file = path
        .canonicalize()
        .map_err(|error| format!("无法确认 {file_name} 的真实路径：{error}"))?;
    if canonical_file.parent() != Some(directory) {
        return Err(format!("{file_name} 不在用户明确选择的主题目录内。"));
    }
    let bytes =
        fs::read(&canonical_file).map_err(|error| format!("无法读取 {file_name}：{error}"))?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(format!("{file_name} 读取后超过 {maximum_bytes} 字节限制。"));
    }
    String::from_utf8(bytes).map_err(|_| format!("{file_name} 必须使用 UTF-8 编码。"))
}

fn parse_theme_package(directory_path: &str) -> Result<ReadRayThemeV1, String> {
    validate_text_field(directory_path, "主题目录", 32_768)?;
    let requested = PathBuf::from(directory_path);
    let directory = requested
        .canonicalize()
        .map_err(|error| format!("无法确认主题目录：{error}"))?;
    if !directory.is_dir() {
        return Err("用户选择的主题路径不是目录。".to_string());
    }
    let manifest = read_direct_package_file(&directory, "manifest.json", MAX_MANIFEST_BYTES)?;
    let css = read_direct_package_file(&directory, "theme.css", MAX_CSS_BYTES)?;
    parse_theme_package_files(&manifest, &css)
}

fn validate_persisted_theme(theme: &ReadRayThemeV1) -> Result<(), String> {
    if theme.builtin {
        return Err("数据库自定义主题不能标记为内置主题。".to_string());
    }
    validate_manifest(&theme.manifest)?;
    if theme.manifest.modes.contains(&ThemeMode::Light) != theme.light.is_some()
        || theme.manifest.modes.contains(&ThemeMode::Dark) != theme.dark.is_some()
    {
        return Err("数据库主题的模式与配色不一致。".to_string());
    }
    if theme.warnings.len() > MAX_DECLARATIONS
        || theme.warnings.iter().any(|warning| {
            warning.chars().count() > 512 || warning.chars().any(|character| character.is_control())
        })
    {
        return Err("数据库主题警告数量或长度无效。".to_string());
    }
    if let Some(colors) = &theme.light {
        validate_normalized_colors(colors)?;
        validate_readability(colors, ThemeMode::Light)?;
    }
    if let Some(colors) = &theme.dark {
        validate_normalized_colors(colors)?;
        validate_readability(colors, ThemeMode::Dark)?;
    }
    Ok(())
}

fn validate_normalized_colors(colors: &ReadRayThemeColors) -> Result<(), String> {
    for value in [
        &colors.canvas,
        &colors.sidebar,
        &colors.surface,
        &colors.surface_elevated,
        &colors.surface_subtle,
        &colors.surface_contrast,
        &colors.text_primary,
        &colors.text_secondary,
        &colors.text_muted,
        &colors.text_subtle,
        &colors.border,
        &colors.border_soft,
        &colors.accent,
        &colors.accent_hover,
        &colors.accent_text,
        &colors.success,
        &colors.success_soft,
        &colors.warning,
        &colors.warning_soft,
        &colors.warning_strong,
        &colors.danger,
        &colors.danger_soft,
        &colors.danger_strong,
        &colors.selection,
        &colors.diff_added,
        &colors.diff_removed,
        &colors.scrim,
        &colors.shadow,
    ] {
        let (normalized, _) = normalize_color(value)?;
        if normalized != *value {
            return Err("数据库主题包含未规范化颜色。".to_string());
        }
    }
    Ok(())
}

fn load_custom_themes(connection: &Connection) -> Result<Vec<ReadRayThemeV1>, String> {
    let mut statement = connection
        .prepare(
            "SELECT manifest_json, light_colors_json, dark_colors_json, warnings_json \
             FROM custom_themes ORDER BY lower(id), id",
        )
        .map_err(|error| format!("无法准备读取自定义主题：{error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| format!("无法读取自定义主题：{error}"))?;
    let mut themes = Vec::new();
    for row in rows {
        let (manifest_json, light_json, dark_json, warnings_json) =
            row.map_err(|error| format!("无法读取自定义主题行：{error}"))?;
        let theme = ReadRayThemeV1 {
            manifest: serde_json::from_str(&manifest_json)
                .map_err(|error| format!("数据库主题 manifest 无效：{error}"))?,
            light: light_json
                .map(|value| {
                    serde_json::from_str(&value)
                        .map_err(|error| format!("数据库 light 配色无效：{error}"))
                })
                .transpose()?,
            dark: dark_json
                .map(|value| {
                    serde_json::from_str(&value)
                        .map_err(|error| format!("数据库 dark 配色无效：{error}"))
                })
                .transpose()?,
            builtin: false,
            warnings: serde_json::from_str(&warnings_json)
                .map_err(|error| format!("数据库主题警告无效：{error}"))?,
        };
        validate_persisted_theme(&theme)?;
        themes.push(theme);
    }
    Ok(themes)
}

fn read_preference(connection: &Connection) -> Result<(i64, String, ThemeMode), String> {
    let (revision, theme_id, mode) = connection
        .query_row(
            "SELECT revision, theme_id, mode FROM theme_preferences WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|error| format!("无法读取主题偏好：{error}"))?;
    if revision < 0 {
        return Err("数据库主题 revision 无效。".to_string());
    }
    Ok((revision, theme_id, ThemeMode::from_storage(&mode)?))
}

fn read_theme_snapshot(connection: &Connection) -> Result<ThemeSnapshot, String> {
    let (revision, current_theme_id, current_mode) = read_preference(connection)?;
    let mut themes = vec![default_theme(), flexoki_theme()];
    themes.extend(codex_builtin_themes());
    themes.extend(load_custom_themes(connection)?);
    let current = themes
        .iter()
        .find(|theme| theme.manifest.id == current_theme_id)
        .ok_or_else(|| "数据库当前主题不存在。".to_string());
    let current = match current {
        Ok(theme) => theme,
        Err(_) if RETIRED_BUILTIN_THEME_IDS.contains(&current_theme_id.as_str()) => {
            let changed = connection
                .execute(
                    "UPDATE theme_preferences \
                     SET revision = revision + 1, theme_id = ?1, mode = ?2 \
                     WHERE id = 1 AND revision = ?3",
                    params![DEFAULT_THEME_ID, ThemeMode::Light.storage_value(), revision],
                )
                .map_err(|error| format!("无法恢复已移除的主题偏好：{error}"))?;
            if changed == 1 {
                return read_theme_snapshot(connection);
            }
            let (_, latest_theme_id, _) = read_preference(connection)?;
            if latest_theme_id != current_theme_id {
                return read_theme_snapshot(connection);
            }
            return Err("主题偏好已在另一个窗口更新，请重新读取后重试。".to_string());
        }
        Err(error) => return Err(error),
    };
    if !current.manifest.modes.contains(&current_mode) {
        return Err("数据库当前主题不支持已保存的模式。".to_string());
    }
    Ok(ThemeSnapshot {
        revision,
        current_theme_id,
        current_mode,
        themes,
    })
}

fn ensure_revision(transaction: &Transaction<'_>, expected_revision: i64) -> Result<(), String> {
    if expected_revision < 0 {
        return Err("主题版本无效，请重新读取后重试。".to_string());
    }
    let actual: i64 = transaction
        .query_row(
            "SELECT revision FROM theme_preferences WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("无法读取主题 revision：{error}"))?;
    if actual != expected_revision {
        return Err("主题已在另一个窗口更新，请重新读取后重试。".to_string());
    }
    Ok(())
}

fn insert_theme(
    connection: &mut Connection,
    theme: &ReadRayThemeV1,
    expected_revision: i64,
) -> Result<ThemeSnapshot, String> {
    validate_persisted_theme(theme)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("无法开始导入主题：{error}"))?;
    ensure_revision(&transaction, expected_revision)?;
    let count: i64 = transaction
        .query_row("SELECT COUNT(*) FROM custom_themes", [], |row| row.get(0))
        .map_err(|error| format!("无法统计自定义主题：{error}"))?;
    if count >= MAX_CUSTOM_THEMES {
        return Err(format!("自定义主题数量不能超过 {MAX_CUSTOM_THEMES}。"));
    }
    let duplicate = transaction
        .query_row(
            "SELECT 1 FROM custom_themes WHERE id = ?1",
            params![theme.manifest.id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| format!("无法检查主题 ID：{error}"))?;
    if duplicate.is_some() {
        return Err(format!(
            "主题 ID '{}' 已存在；本轮不会静默覆盖，请先删除旧主题或更换 ID。",
            theme.manifest.id
        ));
    }
    let manifest_json = serde_json::to_string(&theme.manifest)
        .map_err(|error| format!("无法规范化主题 manifest：{error}"))?;
    let light_json = theme
        .light
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| format!("无法规范化 light 配色：{error}"))?;
    let dark_json = theme
        .dark
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| format!("无法规范化 dark 配色：{error}"))?;
    let warnings_json = serde_json::to_string(&theme.warnings)
        .map_err(|error| format!("无法保存主题警告：{error}"))?;
    transaction
        .execute(
            "INSERT INTO custom_themes (
               id, manifest_json, light_colors_json, dark_colors_json, warnings_json, imported_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                theme.manifest.id,
                manifest_json,
                light_json,
                dark_json,
                warnings_json,
                learning_records::unix_time_ms()?,
            ],
        )
        .map_err(|error| format!("无法保存规范化主题：{error}"))?;
    let changed = transaction
        .execute(
            "UPDATE theme_preferences SET revision = revision + 1 \
             WHERE id = 1 AND revision = ?1",
            params![expected_revision],
        )
        .map_err(|error| format!("无法推进主题 revision：{error}"))?;
    if changed != 1 {
        return Err("主题已在另一个窗口更新，请重新读取后重试。".to_string());
    }
    transaction
        .commit()
        .map_err(|error| format!("无法提交主题导入：{error}"))?;
    read_theme_snapshot(connection)
}

fn select_theme_in_database(
    connection: &mut Connection,
    theme_id: &str,
    mode: ThemeMode,
    expected_revision: i64,
) -> Result<ThemeSnapshot, String> {
    validate_text_field(theme_id, "主题 ID", 64)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("无法开始选择主题：{error}"))?;
    ensure_revision(&transaction, expected_revision)?;
    let supports_mode = if builtin_theme_ids().contains(&theme_id) {
        // 内置主题按自身 manifest 校验模式
        let builtin = std::iter::once(default_theme())
            .chain(std::iter::once(flexoki_theme()))
            .chain(codex_builtin_themes())
            .find(|theme| theme.manifest.id == theme_id)
            .ok_or_else(|| "所选主题不存在，当前主题未改变。".to_string())?;
        builtin.manifest.modes.contains(&mode)
    } else {
        let manifest_json = transaction
            .query_row(
                "SELECT manifest_json FROM custom_themes WHERE id = ?1",
                params![theme_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("无法查找主题：{error}"))?
            .ok_or_else(|| "所选主题不存在，当前主题未改变。".to_string())?;
        let manifest: ReadRayThemeManifestV1 = serde_json::from_str(&manifest_json)
            .map_err(|error| format!("所选主题 manifest 无效：{error}"))?;
        manifest.modes.contains(&mode)
    };
    if !supports_mode {
        return Err("所选主题不支持该模式，当前主题未改变。".to_string());
    }
    let changed = transaction
        .execute(
            "UPDATE theme_preferences \
             SET revision = revision + 1, theme_id = ?1, mode = ?2 \
             WHERE id = 1 AND revision = ?3",
            params![theme_id, mode.storage_value(), expected_revision],
        )
        .map_err(|error| format!("无法保存当前主题：{error}"))?;
    if changed != 1 {
        return Err("主题已在另一个窗口更新，请重新读取后重试。".to_string());
    }
    transaction
        .commit()
        .map_err(|error| format!("无法提交主题选择：{error}"))?;
    read_theme_snapshot(connection)
}

fn delete_theme_in_database(
    connection: &mut Connection,
    theme_id: &str,
    expected_revision: i64,
) -> Result<ThemeSnapshot, String> {
    validate_text_field(theme_id, "主题 ID", 64)?;
    if builtin_theme_ids().contains(&theme_id) {
        return Err("ReadRay 内置主题不能删除。".to_string());
    }
    let transaction = connection
        .transaction()
        .map_err(|error| format!("无法开始删除主题：{error}"))?;
    ensure_revision(&transaction, expected_revision)?;
    let current_theme_id: String = transaction
        .query_row(
            "SELECT theme_id FROM theme_preferences WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("无法读取当前主题：{error}"))?;
    let deleted = transaction
        .execute("DELETE FROM custom_themes WHERE id = ?1", params![theme_id])
        .map_err(|error| format!("无法删除自定义主题：{error}"))?;
    if deleted != 1 {
        return Err("要删除的自定义主题不存在，当前主题未改变。".to_string());
    }
    let (next_theme_id, next_mode) = if current_theme_id == theme_id {
        (DEFAULT_THEME_ID, ThemeMode::Light)
    } else {
        (current_theme_id.as_str(), read_preference(&transaction)?.2)
    };
    let changed = transaction
        .execute(
            "UPDATE theme_preferences \
             SET revision = revision + 1, theme_id = ?1, mode = ?2 \
             WHERE id = 1 AND revision = ?3",
            params![next_theme_id, next_mode.storage_value(), expected_revision],
        )
        .map_err(|error| format!("无法更新删除后的主题偏好：{error}"))?;
    if changed != 1 {
        return Err("主题已在另一个窗口更新，请重新读取后重试。".to_string());
    }
    transaction
        .commit()
        .map_err(|error| format!("无法提交主题删除：{error}"))?;
    read_theme_snapshot(connection)
}

fn emit_theme_updated(app: &AppHandle) {
    if let Err(error) = app.emit("readray://theme-updated", ()) {
        eprintln!("ReadRay 主题更新事件发送失败：{error}");
    }
}

#[tauri::command]
pub fn get_theme_snapshot(app: AppHandle) -> Result<ThemeSnapshot, String> {
    read_theme_snapshot(&learning_records::open_database_for_app(&app)?)
}

#[tauri::command]
pub fn inspect_theme_package(directory_path: String) -> Result<ReadRayThemeV1, String> {
    parse_theme_package(&directory_path)
}

#[tauri::command]
pub fn import_theme_package(
    app: AppHandle,
    directory_path: String,
    expected_theme_id: String,
    expected_revision: i64,
) -> Result<ThemeSnapshot, String> {
    let theme = parse_theme_package(&directory_path)?;
    if theme.manifest.id != expected_theme_id {
        return Err("主题包在安全预检后发生变化，请重新选择目录后重试。".to_string());
    }
    let snapshot = insert_theme(
        &mut learning_records::open_database_for_app(&app)?,
        &theme,
        expected_revision,
    )?;
    emit_theme_updated(&app);
    Ok(snapshot)
}

#[tauri::command]
pub fn select_theme(
    app: AppHandle,
    theme_id: String,
    mode: ThemeMode,
    expected_revision: i64,
) -> Result<ThemeSnapshot, String> {
    let snapshot = select_theme_in_database(
        &mut learning_records::open_database_for_app(&app)?,
        &theme_id,
        mode,
        expected_revision,
    )?;
    emit_theme_updated(&app);
    Ok(snapshot)
}

#[tauri::command]
pub fn delete_custom_theme(
    app: AppHandle,
    theme_id: String,
    expected_revision: i64,
) -> Result<ThemeSnapshot, String> {
    let snapshot = delete_theme_in_database(
        &mut learning_records::open_database_for_app(&app)?,
        &theme_id,
        expected_revision,
    )?;
    emit_theme_updated(&app);
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn manifest(id: &str, modes: &str) -> String {
        format!(
            r#"{{"formatVersion":1,"id":"{id}","name":"Test Theme","version":"1.0.0","author":"Tester","modes":{modes}}}"#
        )
    }

    fn required_css(extra: &str) -> String {
        format!(
            r#"
            /* harmless comment */
            :root {{
              --rr-theme-canvas: #f2f1ed;
              --rr-theme-sidebar: #ebeae5;
              --rr-theme-surface: #e6e5e0;
              --rr-theme-text-primary: #26251e;
              --rr-theme-text-secondary: rgba(38, 37, 30, 0.9);
              --rr-theme-border: rgba(38, 37, 30, 0.1);
              --rr-theme-accent: #f54e00;
            }}
            {extra}
            "#
        )
    }

    fn test_database(label: &str) -> (PathBuf, Connection) {
        let root = std::env::temp_dir().join(format!(
            "readray-themes-{label}-{}-{}",
            std::process::id(),
            TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let connection = learning_records::open_database(&root.join("readray.sqlite3")).unwrap();
        (root, connection)
    }

    #[test]
    fn parses_and_normalizes_comments_whitespace_and_light_dark_sections() {
        let theme = parse_theme_package_files(
            &manifest("two-modes", r#"["light","dark"]"#),
            &required_css(
                r#"
                .theme-light { --rr-theme-accent: #F54E00; }
                .theme-dark {
                  --rr-theme-canvas: #171512;
                  --rr-theme-sidebar: #1f1b18;
                  --rr-theme-surface: #27211d;
                  --rr-theme-text-primary: #f6f0e8;
                  --rr-theme-text-secondary: #d5c9bb;
                  --rr-theme-border: rgba(246, 240, 232, 0.12);
                  --rr-theme-accent: #ff6a32;
                }
                "#,
            ),
        )
        .unwrap();
        assert_eq!(theme.light.unwrap().accent, "#f54e00");
        assert_eq!(theme.dark.unwrap().canvas, "#171512");
        assert!(theme.warnings.is_empty());
    }

    #[test]
    fn normalizes_color_components_to_the_frontend_canonical_grammar() {
        assert_eq!(normalize_color("#FFFFFF").unwrap().0, "#fff");
        assert_eq!(normalize_color("#ffffffff").unwrap().0, "#fff");
        assert_eq!(
            normalize_color("rgb(001, 02, 3)").unwrap().0,
            "rgb(1, 2, 3)"
        );
        assert_eq!(
            normalize_color("rgba(1, 2, 3, 00.5000)").unwrap().0,
            "rgba(1, 2, 3, 0.5)"
        );
        assert_eq!(
            normalize_color("rgba(1, 2, 3, .0500)").unwrap().0,
            "rgba(1, 2, 3, 0.05)"
        );
        assert_eq!(
            normalize_color("rgba(1, 2, 3, 01.000)").unwrap().0,
            "rgba(1, 2, 3, 1)"
        );
    }

    #[test]
    fn ignores_unknown_selector_variable_and_properties_without_runtime_data() {
        let theme = parse_theme_package_files(
            &manifest("warning-theme", r#"["light"]"#),
            &required_css(
                r#"
                .theme-light { --unknown-color: #ffffff; color: #ffffff; }
                .workspace { display: grid; --rr-theme-danger: #000000; }
                "#,
            ),
        )
        .unwrap();
        let colors = theme.light.unwrap();
        assert_eq!(
            colors.danger, colors.accent,
            "unknown selector cannot set danger"
        );
        assert_eq!(theme.warnings.len(), 3);
    }

    #[test]
    fn rejects_import_url_nested_rules_duplicate_invalid_color_and_oversize() {
        for css in [
            required_css("@import url('https://evil.invalid/theme.css');"),
            required_css(":root { --rr-theme-danger: url(https://evil.invalid/a); }"),
            required_css(":root { color: red; .nested { color: blue; } }"),
            required_css(":root { --rr-theme-accent: #ffffff; }"),
            required_css(":root { --rr-theme-danger: not-a-color; }"),
        ] {
            assert!(
                parse_theme_package_files(&manifest("bad-theme", r#"["light"]"#), &css).is_err()
            );
        }
        let oversized = " ".repeat(MAX_CSS_BYTES as usize + 1);
        assert!(
            parse_theme_package_files(&manifest("large-theme", r#"["light"]"#), &oversized)
                .is_err()
        );
    }

    #[test]
    fn rejects_missing_fields_and_low_readability() {
        assert!(parse_theme_package_files(
            r#"{"formatVersion":1,"id":"missing","name":"Missing","version":"1"}"#,
            &required_css("")
        )
        .is_err());
        let low_contrast = required_css(
            ".theme-light { --rr-theme-text-primary: #eeeeee; --rr-theme-canvas: #ffffff; }",
        );
        assert!(parse_theme_package_files(
            &manifest("low-contrast", r#"["light"]"#),
            &low_contrast
        )
        .is_err());
        let missing_required =
            required_css("").replace("--rr-theme-text-secondary: rgba(38, 37, 30, 0.9);", "");
        assert!(parse_theme_package_files(
            &manifest("missing-token", r#"["light"]"#),
            &missing_required
        )
        .is_err());
    }

    #[test]
    fn flexoki_builtin_theme_provides_light_and_dark_modes_and_is_protected() {
        let theme = flexoki_theme();
        assert!(theme.builtin);
        assert_eq!(theme.manifest.id, FLEXOKI_THEME_ID);
        assert_eq!(theme.manifest.name, "Flexoki");
        assert_eq!(theme.manifest.author, "Steph Ango");
        assert_eq!(theme.manifest.license.as_deref(), Some("MIT"));
        assert_eq!(
            theme.manifest.source_url.as_deref(),
            Some("https://stephango.com/flexoki")
        );
        assert!(theme.light.is_some());
        assert!(theme.dark.is_some());
        let light = theme.light.unwrap();
        assert_eq!(light.canvas, "#fffcf0");
        assert_eq!(light.accent, "#24837b");
        let dark = theme.dark.unwrap();
        assert_eq!(dark.canvas, "#100f0f");
        assert_eq!(dark.accent, "#3aa99f");

        // 随包主题不能被自定义主题 ID 冲突覆盖。
        assert!(validate_manifest(&flexoki_theme().manifest).is_err());
        // 随包主题不可删除。
        assert!(delete_theme_in_database(
            &mut test_database("flexoki-delete").1,
            FLEXOKI_THEME_ID,
            0,
        )
        .is_err());
        // 随包主题支持的模式选择。
        let (_, mut connection) = test_database("flexoki-select");
        let selected_dark =
            select_theme_in_database(&mut connection, FLEXOKI_THEME_ID, ThemeMode::Dark, 0)
                .unwrap();
        assert_eq!(selected_dark.current_theme_id, FLEXOKI_THEME_ID);
        assert_eq!(selected_dark.current_mode, ThemeMode::Dark);
        let selected_light =
            select_theme_in_database(&mut connection, FLEXOKI_THEME_ID, ThemeMode::Light, 1)
                .unwrap();
        assert_eq!(selected_light.current_theme_id, FLEXOKI_THEME_ID);
        assert_eq!(selected_light.current_mode, ThemeMode::Light);
    }

    #[test]
    fn codex_builtin_themes_are_unique_complete_mode_aware_and_protected() {
        let themes = codex_builtin_themes();
        assert_eq!(themes.len(), 15);

        // 主题 ID 唯一，且不与既有内置主题冲突。
        let mut ids = std::collections::HashSet::new();
        for theme in &themes {
            assert!(theme.builtin);
            assert!(
                ids.insert(theme.manifest.id.as_str()),
                "重复 ID：{}",
                theme.manifest.id
            );
            assert_ne!(theme.manifest.id, DEFAULT_THEME_ID);
            assert_ne!(theme.manifest.id, FLEXOKI_THEME_ID);
            // 随包 Codex 主题必须同时支持浅色和深色模式。
            assert!(theme.light.is_some() && theme.dark.is_some());
        }

        // 每个声明模式都有完整 28 token 配色，且通过规范化与可读性校验。
        for theme in &themes {
            // 内置 manifest 不被 validate_manifest 校验（那是自定义导入校验器），
            // 直接核对必填文本字段非空。
            assert!(!theme.manifest.name.trim().is_empty());
            assert!(!theme.manifest.author.trim().is_empty());
            if let Some(light) = &theme.light {
                validate_normalized_colors(light).unwrap();
                validate_readability(light, ThemeMode::Light).unwrap();
            }
            if let Some(dark) = &theme.dark {
                validate_normalized_colors(dark).unwrap();
                validate_readability(dark, ThemeMode::Dark).unwrap();
            }
            assert_eq!(
                theme.manifest.modes.contains(&ThemeMode::Light),
                theme.light.is_some()
            );
            assert_eq!(
                theme.manifest.modes.contains(&ThemeMode::Dark),
                theme.dark.is_some()
            );
        }

        // 内置主题不可删除、不可被自定义 ID 冲突覆盖。
        assert!(
            delete_theme_in_database(&mut test_database("codex-delete").1, "catppuccin", 0,)
                .is_err()
        );
        // validate_manifest 对内置 ID 返回冲突错误（自定义主题不能用内置 ID）。
        let catppuccin_manifest = themes
            .iter()
            .find(|theme| theme.manifest.id == "catppuccin")
            .unwrap()
            .manifest
            .clone();
        assert!(validate_manifest(&catppuccin_manifest).is_err());

        // 模式选择：保留的 Codex 主题两种模式都可用。
        let (_, mut connection) = test_database("codex-select");
        let selected_dark =
            select_theme_in_database(&mut connection, "catppuccin", ThemeMode::Dark, 0).unwrap();
        assert_eq!(selected_dark.current_theme_id, "catppuccin");
        assert_eq!(selected_dark.current_mode, ThemeMode::Dark);
        let cat_selected =
            select_theme_in_database(&mut connection, "catppuccin", ThemeMode::Light, 1).unwrap();
        assert_eq!(cat_selected.current_mode, ThemeMode::Light);

        // 重启恢复：Codex 主题随快照恢复且仍是内置。
        let (root, mut connection) = test_database("codex-restart");
        let _ = select_theme_in_database(&mut connection, "solarized", ThemeMode::Dark, 0).unwrap();
        drop(connection);
        let reopened = learning_records::open_database(&root.join("readray.sqlite3")).unwrap();
        let restored = read_theme_snapshot(&reopened).unwrap();
        assert_eq!(restored.current_theme_id, "solarized");
        assert_eq!(restored.current_mode, ThemeMode::Dark);
        let restored_theme = restored
            .themes
            .iter()
            .find(|theme| theme.manifest.id == "solarized")
            .unwrap();
        assert!(restored_theme.builtin);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removed_builtin_theme_selection_falls_back_to_default() {
        let (_, connection) = test_database("retired-theme-fallback");
        connection
            .execute(
                "UPDATE theme_preferences SET revision = 4, theme_id = 'ayu', mode = 'dark' WHERE id = 1",
                [],
            )
            .unwrap();

        let snapshot = read_theme_snapshot(&connection).unwrap();
        assert_eq!(snapshot.revision, 5);
        assert_eq!(snapshot.current_theme_id, DEFAULT_THEME_ID);
        assert_eq!(snapshot.current_mode, ThemeMode::Light);
        assert!(!snapshot
            .themes
            .iter()
            .any(|theme| theme.manifest.id == "ayu"));

        let persisted = read_theme_snapshot(&connection).unwrap();
        assert_eq!(persisted.revision, 5);
        assert_eq!(persisted.current_theme_id, DEFAULT_THEME_ID);
    }

    #[test]
    fn import_select_delete_revision_and_restart_recovery_are_authoritative() {
        let (root, mut connection) = test_database("lifecycle");
        let theme =
            parse_theme_package_files(&manifest("stored-theme", r#"["light"]"#), &required_css(""))
                .unwrap();
        let imported = insert_theme(&mut connection, &theme, 0).unwrap();
        assert_eq!(imported.revision, 1);
        assert_eq!(imported.current_theme_id, DEFAULT_THEME_ID);
        assert!(insert_theme(&mut connection, &theme, 1)
            .unwrap_err()
            .contains("已存在"));
        assert_eq!(read_theme_snapshot(&connection).unwrap().revision, 1);
        assert_eq!(read_theme_snapshot(&connection).unwrap().themes.len(), 18);

        let selected =
            select_theme_in_database(&mut connection, "stored-theme", ThemeMode::Light, 1).unwrap();
        assert_eq!(selected.revision, 2);
        assert_eq!(selected.current_theme_id, "stored-theme");
        assert!(
            select_theme_in_database(&mut connection, "missing-theme", ThemeMode::Light, 2)
                .is_err()
        );
        assert_eq!(
            read_theme_snapshot(&connection).unwrap().current_theme_id,
            "stored-theme"
        );
        assert!(delete_theme_in_database(&mut connection, "stored-theme", 1).is_err());
        assert_eq!(
            read_theme_snapshot(&connection).unwrap().current_theme_id,
            "stored-theme"
        );

        let deleted = delete_theme_in_database(&mut connection, "stored-theme", 2).unwrap();
        assert_eq!(deleted.revision, 3);
        assert_eq!(deleted.current_theme_id, DEFAULT_THEME_ID);
        assert_eq!(deleted.current_mode, ThemeMode::Light);
        assert_eq!(deleted.themes.len(), 17);
        drop(connection);

        let reopened = learning_records::open_database(&root.join("readray.sqlite3")).unwrap();
        let restored = read_theme_snapshot(&reopened).unwrap();
        assert_eq!(restored.revision, 3);
        assert_eq!(restored.current_theme_id, DEFAULT_THEME_ID);
        assert_eq!(restored.themes.len(), 17);
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn text_length_counts_unicode_code_points_like_the_frontend() {
        let within = "😀".repeat(80);
        let over = "😀".repeat(81);
        assert_eq!(within.chars().count(), 80);
        assert_eq!("😀".chars().count(), 1);
        assert!(validate_text_field(&within, "主题名称", 80).is_ok());
        assert!(validate_text_field(&over, "主题名称", 80).is_err());
        assert!(validate_text_field(&"😀".repeat(2_048), "主题来源 URL", 2_048).is_ok());
        assert!(validate_text_field(&"😀".repeat(2_049), "主题来源 URL", 2_048).is_err());
    }

    #[test]
    fn default_theme_keeps_current_runtime_color_values() {
        let theme = default_theme();
        assert_eq!(
            theme.manifest.modes,
            vec![ThemeMode::Light, ThemeMode::Dark]
        );
        let colors = theme.light.unwrap();
        assert_eq!(colors.canvas, "#f2f1ed");
        assert_eq!(colors.sidebar, "#ebeae5");
        assert_eq!(colors.surface, "#e6e5e0");
        assert_eq!(colors.surface_elevated, "#ebeae5");
        assert_eq!(colors.text_primary, "#26251e");
        assert_eq!(colors.text_secondary, "rgba(38, 37, 30, 0.9)");
        assert_eq!(colors.text_muted, "rgba(38, 37, 30, 0.55)");
        assert_eq!(colors.text_subtle, "rgba(38, 37, 30, 0.4)");
        assert_eq!(colors.border, "rgba(38, 37, 30, 0.1)");
        assert_eq!(colors.border_soft, "rgba(38, 37, 30, 0.06)");
        assert_eq!(colors.accent, "#f54e00");
        assert_eq!(colors.danger, "#cf2d56");

        let dark = theme.dark.unwrap();
        assert_eq!(dark.canvas, "#0d0d0b");
        assert_eq!(dark.sidebar, "#171512");
        assert_eq!(dark.surface, "#1f1b18");
        assert_eq!(dark.text_primary, "#f6f0e8");
        assert_eq!(dark.accent, "#ff6a32");
        assert_eq!(dark.accent_text, "#0d0d0b");
        assert_eq!(dark.shadow, "rgba(0, 0, 0, 0.32)");
    }
}
