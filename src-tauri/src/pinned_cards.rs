use crate::explanation::{validate_explanation_card, CaptureInput, ExplanationCard, SourceType};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{LogicalPosition, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow, SetForegroundWindow},
};

const INITIAL_OVERLAY_WINDOW_LABEL: &str = "overlay";
const QUERY_OVERLAY_LABEL_PREFIX: &str = "overlay-query-";
const PINNED_CARD_LIMIT: usize = 8;
const OVERLAY_INPUT_WIDTH: f64 = 750.0;
const OVERLAY_INPUT_HEIGHT: f64 = 58.0;
const PINNED_CARD_CLOSE_DELAY_MS: u64 = 60;

static QUERY_OVERLAY_COUNTER: AtomicU64 = AtomicU64::new(1);
static ACTIVE_OVERLAY_LABEL: OnceLock<Mutex<String>> = OnceLock::new();
static PINNED_CARDS: OnceLock<Mutex<HashMap<String, PinnedCardEntry>>> = OnceLock::new();
static ACTIVE_PINNED_CARD_DRAG: OnceLock<Mutex<Option<ActivePinnedCardDrag>>> = OnceLock::new();

struct PinnedCardEntry {
    source_window_hwnd: Option<usize>,
}

#[derive(Clone)]
struct ActivePinnedCardDrag {
    label: String,
    pointer_x: f64,
    pointer_y: f64,
    window_x: f64,
    window_y: f64,
}

fn tauri_err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn active_overlay_label_state() -> &'static Mutex<String> {
    ACTIVE_OVERLAY_LABEL.get_or_init(|| Mutex::new(INITIAL_OVERLAY_WINDOW_LABEL.to_string()))
}

fn pinned_cards() -> &'static Mutex<HashMap<String, PinnedCardEntry>> {
    PINNED_CARDS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn active_pinned_card_drag() -> &'static Mutex<Option<ActivePinnedCardDrag>> {
    ACTIVE_PINNED_CARD_DRAG.get_or_init(|| Mutex::new(None))
}

pub(crate) fn active_overlay_label() -> Result<String, String> {
    active_overlay_label_state()
        .lock()
        .map(|label| label.clone())
        .map_err(tauri_err)
}

pub(crate) fn active_overlay_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    let label = active_overlay_label()?;
    if is_pinned_card_window(&label) {
        return Err("ReadRay 正在准备下一查询窗口，请稍后重试。".to_string());
    }
    app.get_webview_window(&label)
        .ok_or_else(|| format!("ReadRay 活动查询窗口不存在：{label}"))
}

pub(crate) fn is_active_overlay_label(label: &str) -> bool {
    active_overlay_label_state()
        .lock()
        .is_ok_and(|active| active.as_str() == label)
}

pub(crate) fn ensure_active_overlay_window(window: &WebviewWindow) -> Result<(), String> {
    if is_active_overlay_label(window.label()) && !is_pinned_card_window(window.label()) {
        Ok(())
    } else {
        Err(format!("窗口 {} 不是当前活动查询窗口。", window.label()))
    }
}

pub(crate) fn is_pinned_card_window(label: &str) -> bool {
    pinned_cards()
        .lock()
        .is_ok_and(|cards| cards.contains_key(label))
}

#[cfg(test)]
fn is_query_overlay_label(label: &str) -> bool {
    label
        .strip_prefix(QUERY_OVERLAY_LABEL_PREFIX)
        .is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
        })
}

fn next_query_overlay_label() -> String {
    let id = QUERY_OVERLAY_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{QUERY_OVERLAY_LABEL_PREFIX}{id}")
}

#[cfg(target_os = "windows")]
fn valid_source_window_hwnd(raw_hwnd: Option<usize>) -> Option<usize> {
    raw_hwnd.filter(|raw| {
        *raw != 0 && unsafe { IsWindow(Some(HWND(*raw as *mut std::ffi::c_void))).as_bool() }
    })
}

#[cfg(not(target_os = "windows"))]
fn valid_source_window_hwnd(_raw_hwnd: Option<usize>) -> Option<usize> {
    None
}

#[cfg(target_os = "windows")]
fn restore_source_window_focus(raw_hwnd: Option<usize>) -> bool {
    if let Some(raw_hwnd) = valid_source_window_hwnd(raw_hwnd) {
        return unsafe { SetForegroundWindow(HWND(raw_hwnd as *mut std::ffi::c_void)).as_bool() };
    }
    false
}

#[cfg(not(target_os = "windows"))]
fn restore_source_window_focus(_raw_hwnd: Option<usize>) -> bool {
    false
}

#[cfg(target_os = "windows")]
pub(crate) fn restore_source_before_selection_capture(app: &tauri::AppHandle) -> bool {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        return false;
    }
    let candidates = match pinned_cards().lock() {
        Ok(cards) => cards
            .iter()
            .map(|(label, entry)| (label.clone(), entry.source_window_hwnd))
            .collect::<Vec<_>>(),
        Err(_) => return false,
    };
    for (label, source_window_hwnd) in candidates {
        let Some(window) = app.get_webview_window(&label) else {
            continue;
        };
        if window.hwnd().is_ok_and(|hwnd| hwnd == foreground) {
            let restored = restore_source_window_focus(source_window_hwnd);
            eprintln!("READRAY_PINNED_CARD_SOURCE_RESTORE label={label} restored={restored}");
            return restored;
        }
    }
    false
}

fn validate_card(card: &ExplanationCard) -> Result<(), String> {
    // ExplanationCard 已在模型响应进入应用时完成过一次权威校验。这里再次校验
    // 前端回传的数据，但用原文作为非空上下文占位，避免合法的语境释义因为
    // pin 命令没有携带原始 CaptureInput 而被误判为无上下文卡片。
    let input = CaptureInput {
        query_text: card.source_text().to_string(),
        context_text: Some(card.source_text().to_string()),
        source_type: SourceType::WindowsUia,
        source_app: None,
    };
    validate_explanation_card(&input, card)
        .map_err(|errors| format!("无法固定无效的解释卡片：{}", errors.join("；")))
}

#[tauri::command]
pub(crate) async fn promote_overlay_to_pinned_card(
    window: WebviewWindow,
    card: ExplanationCard,
    source_window_hwnd: Option<usize>,
) -> Result<String, String> {
    ensure_active_overlay_window(&window)?;
    validate_card(&card)?;

    let pinned_label = window.label().to_string();
    let next_overlay_label = next_query_overlay_label();
    {
        let mut cards = pinned_cards().lock().map_err(tauri_err)?;
        if cards.len() >= PINNED_CARD_LIMIT {
            return Err(format!(
                "最多同时固定 {PINNED_CARD_LIMIT} 张解释卡片，请先关闭一张后再试。"
            ));
        }
        if cards.contains_key(&pinned_label) {
            return Err("当前卡片正在固定，请稍候。".to_string());
        }
        cards.insert(
            pinned_label.clone(),
            PinnedCardEntry {
                source_window_hwnd: valid_source_window_hwnd(source_window_hwnd),
            },
        );
    }

    // Windows/WebView2 不允许从同步 command 创建 WebviewWindow；本 command
    // 必须保持 async。当前可见窗口始终原地保留，只在后台补建下一查询窗口。
    let next_overlay = match WebviewWindowBuilder::new(
        window.app_handle(),
        &next_overlay_label,
        WebviewUrl::App("index.html?view=overlay".into()),
    )
    .title("ReadRay Overlay")
    .inner_size(OVERLAY_INPUT_WIDTH, OVERLAY_INPUT_HEIGHT)
    .min_inner_size(680.0, OVERLAY_INPUT_HEIGHT)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .resizable(false)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .build()
    {
        Ok(window) => window,
        Err(error) => {
            forget_pinned_card(&pinned_label);
            return Err(format!("无法准备下一查询窗口：{error}"));
        }
    };

    {
        let mut active_label = active_overlay_label_state().lock().map_err(tauri_err)?;
        if active_label.as_str() != pinned_label {
            forget_pinned_card(&pinned_label);
            let _ = next_overlay.close();
            return Err("活动查询窗口已变化，本次固定已取消。".to_string());
        }
        *active_label = next_overlay_label.clone();
    }

    let _ = window.set_title("ReadRay 固定解释");
    let source_window_hwnd = pinned_cards().lock().ok().and_then(|cards| {
        cards
            .get(&pinned_label)
            .and_then(|entry| entry.source_window_hwnd)
    });
    let focus_restored = restore_source_window_focus(source_window_hwnd);
    eprintln!(
        "READRAY_OVERLAY_PROMOTED pinned_label={pinned_label} next_overlay={next_overlay_label} source_focus_restored={focus_restored}"
    );
    Ok(next_overlay_label)
}

fn ensure_pinned_card_window(window: &WebviewWindow) -> Result<(), String> {
    if is_pinned_card_window(window.label()) {
        Ok(())
    } else {
        Err("当前窗口不是 ReadRay 固定卡片。".to_string())
    }
}

#[tauri::command]
pub(crate) fn close_pinned_card(window: WebviewWindow) -> Result<(), String> {
    ensure_pinned_card_window(&window)?;
    window.hide().map_err(tauri_err)?;
    forget_pinned_card(window.label());
    let closing_window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(PINNED_CARD_CLOSE_DELAY_MS));
        if let Err(error) = closing_window.close() {
            eprintln!("READRAY_PINNED_CARD_CLOSE_ERROR={error}");
        }
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn begin_pinned_card_drag(
    window: WebviewWindow,
    pointer_x: f64,
    pointer_y: f64,
) -> Result<(), String> {
    ensure_pinned_card_window(&window)?;
    let position = window.outer_position().map_err(tauri_err)?;
    let scale_factor = window.scale_factor().map_err(tauri_err)?;
    *active_pinned_card_drag().lock().map_err(tauri_err)? = Some(ActivePinnedCardDrag {
        label: window.label().to_string(),
        pointer_x,
        pointer_y,
        window_x: f64::from(position.x) / scale_factor,
        window_y: f64::from(position.y) / scale_factor,
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn drag_pinned_card(
    window: WebviewWindow,
    pointer_x: f64,
    pointer_y: f64,
) -> Result<(), String> {
    ensure_pinned_card_window(&window)?;
    let active = active_pinned_card_drag().lock().map_err(tauri_err)?.clone();
    let Some(active) = active.filter(|active| active.label == window.label()) else {
        return Ok(());
    };
    window
        .set_position(LogicalPosition::new(
            active.window_x + pointer_x - active.pointer_x,
            active.window_y + pointer_y - active.pointer_y,
        ))
        .map_err(tauri_err)
}

#[tauri::command]
pub(crate) fn finish_pinned_card_drag(window: WebviewWindow) -> Result<(), String> {
    ensure_pinned_card_window(&window)?;
    let mut active = active_pinned_card_drag().lock().map_err(tauri_err)?;
    if active
        .as_ref()
        .is_some_and(|active| active.label == window.label())
    {
        *active = None;
    }
    Ok(())
}

pub(crate) fn forget_pinned_card(label: &str) {
    if let Ok(mut cards) = pinned_cards().lock() {
        cards.remove(label);
    }
    if let Ok(mut active) = active_pinned_card_drag().lock() {
        if active.as_ref().is_some_and(|active| active.label == label) {
            *active = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_overlay_labels_only_accept_generated_numeric_suffixes() {
        assert!(is_query_overlay_label("overlay-query-1"));
        assert!(is_query_overlay_label("overlay-query-42"));
        assert!(!is_query_overlay_label("overlay-query-"));
        assert!(!is_query_overlay_label("overlay-query-user"));
        assert!(!is_query_overlay_label("overlay"));
    }
}
