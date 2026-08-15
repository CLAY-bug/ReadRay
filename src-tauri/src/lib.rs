use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{
    Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewWindow,
    WindowEvent,
};

pub mod conversations;
pub mod deepseek_client;
pub mod deepseek_explanation;
pub mod desktop_lifecycle;
pub mod explanation;
mod explanation_cache;
pub mod learning_records;
pub mod model_usage;
pub mod quick_ai;
pub mod quick_ai_prompt;
pub mod review;
pub mod secret_store;
pub mod settings;
pub mod themes;
#[cfg(target_os = "windows")]
pub mod windows_uia;
pub mod writing;

const MAIN_WINDOW_LABEL: &str = "main";
const OVERLAY_WINDOW_LABEL: &str = "overlay";
pub(crate) const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
pub(crate) const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-flash";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowState {
    visible: bool,
    always_on_top: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OverlayWindowStage {
    Input,
    Result,
    Error,
    QuickAi,
}

impl OverlayWindowStage {
    fn size(self) -> LogicalSize<f64> {
        match self {
            Self::Input => LogicalSize::new(750.0, 58.0),
            Self::Result => LogicalSize::new(800.0, 560.0),
            Self::Error => LogicalSize::new(720.0, 132.0),
            Self::QuickAi => LogicalSize::new(750.0, 500.0),
        }
    }

    fn uses_drawer_anchor_with(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Input, Self::QuickAi) | (Self::QuickAi, Self::Input)
        )
    }
}

fn current_overlay_window_stage() -> &'static Mutex<OverlayWindowStage> {
    static STAGE: OnceLock<Mutex<OverlayWindowStage>> = OnceLock::new();
    STAGE.get_or_init(|| Mutex::new(OverlayWindowStage::Input))
}

fn get_current_overlay_window_stage() -> Result<OverlayWindowStage, String> {
    current_overlay_window_stage()
        .lock()
        .map(|stage| *stage)
        .map_err(tauri_err)
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
enum AnchoredOverlayStage {
    Loading,
    Result,
    Error,
}

#[cfg(target_os = "windows")]
impl AnchoredOverlayStage {
    fn logical_size(self) -> LogicalSize<f64> {
        match self {
            Self::Loading => LogicalSize::new(430.0, 92.0),
            Self::Result => LogicalSize::new(520.0, 380.0),
            Self::Error => LogicalSize::new(430.0, 132.0),
        }
    }
}

const DEFAULT_OVERLAY_CENTER_Y_RATIO: f64 = 0.36;
#[cfg(target_os = "windows")]
const ANCHORED_OVERLAY_GAP: f64 = 10.0;
#[cfg(target_os = "windows")]
const ANCHORED_OVERLAY_MARGIN: f64 = 8.0;
#[cfg(target_os = "windows")]
const ANCHORED_OVERLAY_MAX_HEIGHT_RATIO: f64 = 0.7;
#[cfg(target_os = "windows")]
const ANCHORED_OVERLAY_MIN_WIDTH: f64 = 360.0;
#[cfg(target_os = "windows")]
const ANCHORED_OVERLAY_MAX_WIDTH: f64 = 720.0;
#[cfg(target_os = "windows")]
const ANCHORED_OVERLAY_MIN_HEIGHT: f64 = 80.0;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeepSeekSmokeResult {
    configured: bool,
    ok: bool,
    model: String,
    status: Option<u16>,
    message: String,
    content_preview: Option<String>,
}

#[derive(Clone, Copy)]
struct SavedOverlayPosition {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy)]
struct ActiveOverlayDrag {
    pointer_x: f64,
    pointer_y: f64,
    window_x: f64,
    window_y: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlayIntent {
    kind: &'static str,
    #[cfg(target_os = "windows")]
    #[serde(skip_serializing_if = "Option::is_none")]
    capture: Option<windows_uia::WindowsUiaCapture>,
}

impl OverlayIntent {
    fn show_input() -> Self {
        Self {
            kind: "showInput",
            #[cfg(target_os = "windows")]
            capture: None,
        }
    }

    #[cfg(target_os = "windows")]
    fn uia_capture(capture: windows_uia::WindowsUiaCapture) -> Self {
        Self {
            kind: "uiaCapture",
            capture: Some(capture),
        }
    }
}

static SAVED_OVERLAY_POSITION: OnceLock<Mutex<Option<SavedOverlayPosition>>> = OnceLock::new();
static ACTIVE_OVERLAY_DRAG: OnceLock<Mutex<Option<ActiveOverlayDrag>>> = OnceLock::new();
static PENDING_OVERLAY_INTENT: OnceLock<Mutex<Option<OverlayIntent>>> = OnceLock::new();
static OVERLAY_FOCUS_GRACE_UNTIL: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
#[cfg(target_os = "windows")]
static PENDING_UIA_CAPTURE: OnceLock<Mutex<Option<windows_uia::WindowsUiaCapture>>> =
    OnceLock::new();

fn tauri_err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn ensure_window_label(window: &WebviewWindow, expected: &str) -> Result<(), String> {
    if window.label() == expected {
        Ok(())
    } else {
        Err(format!(
            "窗口命令要求 label={expected}，当前为 {}。",
            window.label()
        ))
    }
}

fn ensure_overlay_window(window: &WebviewWindow) -> Result<(), String> {
    ensure_window_label(window, OVERLAY_WINDOW_LABEL)
}

fn ensure_main_window(window: &WebviewWindow) -> Result<(), String> {
    ensure_window_label(window, MAIN_WINDOW_LABEL)
}

fn saved_overlay_position() -> &'static Mutex<Option<SavedOverlayPosition>> {
    SAVED_OVERLAY_POSITION.get_or_init(|| Mutex::new(None))
}

fn active_overlay_drag() -> &'static Mutex<Option<ActiveOverlayDrag>> {
    ACTIVE_OVERLAY_DRAG.get_or_init(|| Mutex::new(None))
}

fn pending_overlay_intent() -> &'static Mutex<Option<OverlayIntent>> {
    PENDING_OVERLAY_INTENT.get_or_init(|| Mutex::new(None))
}

fn set_pending_overlay_intent(intent: OverlayIntent) -> Result<(), String> {
    let mut pending = pending_overlay_intent().lock().map_err(tauri_err)?;
    *pending = Some(intent);
    Ok(())
}

fn overlay_focus_grace_until() -> &'static Mutex<Option<Instant>> {
    OVERLAY_FOCUS_GRACE_UNTIL.get_or_init(|| Mutex::new(None))
}

fn start_overlay_focus_grace() {
    if let Ok(mut grace_until) = overlay_focus_grace_until().lock() {
        *grace_until = Some(Instant::now() + Duration::from_millis(350));
    }
}

fn overlay_focus_grace_active() -> bool {
    overlay_focus_grace_until()
        .lock()
        .ok()
        .and_then(|grace_until| *grace_until)
        .is_some_and(|grace_until| Instant::now() < grace_until)
}

#[cfg(target_os = "windows")]
fn pending_uia_capture() -> &'static Mutex<Option<windows_uia::WindowsUiaCapture>> {
    PENDING_UIA_CAPTURE.get_or_init(|| Mutex::new(None))
}

fn save_overlay_position(x: i32, y: i32, scale_factor: f64) -> Result<(), String> {
    save_overlay_position_from_logical(f64::from(x) / scale_factor, f64::from(y) / scale_factor)
}

fn save_overlay_position_from_logical(x: f64, y: f64) -> Result<(), String> {
    let mut saved = saved_overlay_position().lock().map_err(tauri_err)?;

    *saved = Some(SavedOverlayPosition { x, y });

    Ok(())
}

fn remember_overlay_position(window: &WebviewWindow) -> Result<(), String> {
    let position = window.outer_position().map_err(tauri_err)?;
    save_overlay_position(
        position.x,
        position.y,
        window.scale_factor().map_err(tauri_err)?,
    )
}

fn get_saved_overlay_position() -> Result<Option<SavedOverlayPosition>, String> {
    saved_overlay_position()
        .lock()
        .map(|saved| *saved)
        .map_err(tauri_err)
}

fn place_default_overlay_position(
    window: &WebviewWindow,
    stage: OverlayWindowStage,
) -> Result<(), String> {
    let Some(monitor) = window.current_monitor().map_err(tauri_err)? else {
        return window.center().map_err(tauri_err);
    };

    let scale_factor = window.scale_factor().map_err(tauri_err)?;
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let stage_size = stage.size();

    let monitor_x = f64::from(monitor_position.x) / scale_factor;
    let monitor_y = f64::from(monitor_position.y) / scale_factor;
    let monitor_width = f64::from(monitor_size.width) / scale_factor;
    let monitor_height = f64::from(monitor_size.height) / scale_factor;

    let x = monitor_x + (monitor_width - stage_size.width) / 2.0;
    let y = monitor_y + monitor_height * DEFAULT_OVERLAY_CENTER_Y_RATIO - stage_size.height / 2.0;

    window
        .set_position(LogicalPosition::new(x.max(monitor_x), y.max(monitor_y)))
        .map_err(tauri_err)
}

fn load_project_env() {
    let env_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|project_root| project_root.join(".env"));

    if let Some(env_path) = env_path {
        if env_path.exists() {
            if let Err(error) = dotenvy::from_path(&env_path) {
                eprintln!(
                    "ReadRay failed to load .env from {}: {error}",
                    env_path.display()
                );
            }
        }
    }
}

fn resize_overlay_window(window: &WebviewWindow, stage: OverlayWindowStage) -> Result<(), String> {
    let previous_stage = get_current_overlay_window_stage()?;
    let drawer_position = if previous_stage.uses_drawer_anchor_with(stage) {
        let position = window.outer_position().map_err(tauri_err)?;
        let scale_factor = window.scale_factor().map_err(tauri_err)?;
        let previous_size = previous_stage.size();
        let next_size = stage.size();
        Some(SavedOverlayPosition {
            x: f64::from(position.x) / scale_factor + (previous_size.width - next_size.width) / 2.0,
            y: f64::from(position.y) / scale_factor,
        })
    } else {
        None
    };

    window.set_size(stage.size()).map_err(tauri_err)?;
    *current_overlay_window_stage().lock().map_err(tauri_err)? = stage;
    if let Some(position) = drawer_position {
        window
            .set_position(LogicalPosition::new(position.x, position.y))
            .map_err(tauri_err)?;
        save_overlay_position_from_logical(position.x, position.y)?;
    } else if let Some(position) = get_saved_overlay_position()? {
        window
            .set_position(LogicalPosition::new(position.x, position.y))
            .map_err(tauri_err)?;
    } else {
        place_default_overlay_position(window, stage)?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn place_anchored_overlay_window(
    window: &WebviewWindow,
    requested_size: LogicalSize<f64>,
    anchor_rect: &windows_uia::ScreenRect,
) -> Result<(), String> {
    let anchor_center_x = anchor_rect.x + anchor_rect.width / 2.0;
    let anchor_center_y = anchor_rect.y + anchor_rect.height / 2.0;
    let monitor = window
        .available_monitors()
        .map_err(tauri_err)?
        .into_iter()
        .find(|monitor| {
            let work_area = monitor.work_area();
            let left = f64::from(work_area.position.x);
            let top = f64::from(work_area.position.y);
            let right = left + f64::from(work_area.size.width);
            let bottom = top + f64::from(work_area.size.height);

            anchor_center_x >= left
                && anchor_center_x < right
                && anchor_center_y >= top
                && anchor_center_y < bottom
        })
        .or_else(|| window.current_monitor().ok().flatten())
        .ok_or_else(|| "无法确定选区所在显示器。".to_string())?;

    let work_area = monitor.work_area();
    let scale_factor = monitor.scale_factor();
    let gap = ANCHORED_OVERLAY_GAP * scale_factor;
    let margin = ANCHORED_OVERLAY_MARGIN * scale_factor;
    let requested_width = requested_size
        .width
        .clamp(ANCHORED_OVERLAY_MIN_WIDTH, ANCHORED_OVERLAY_MAX_WIDTH)
        * scale_factor;
    let requested_height = requested_size.height.max(ANCHORED_OVERLAY_MIN_HEIGHT) * scale_factor;
    let max_width = (f64::from(work_area.size.width) - margin * 2.0).max(1.0);
    let max_height =
        (f64::from(work_area.size.height) * ANCHORED_OVERLAY_MAX_HEIGHT_RATIO).max(1.0);
    let width = requested_width.min(max_width).round().max(1.0) as u32;
    let height = requested_height.min(max_height).round().max(1.0) as u32;

    let work_left = f64::from(work_area.position.x);
    let work_top = f64::from(work_area.position.y);
    let work_right = work_left + f64::from(work_area.size.width);
    let work_bottom = work_top + f64::from(work_area.size.height);
    let max_x = (work_right - f64::from(width) - margin).max(work_left + margin);
    let x = (anchor_center_x - f64::from(width) / 2.0).clamp(work_left + margin, max_x);

    let preferred_bottom = anchor_rect.y + anchor_rect.height + gap;
    let preferred_top = anchor_rect.y - gap - f64::from(height);
    let y = if preferred_bottom + f64::from(height) + margin <= work_bottom {
        preferred_bottom
    } else if preferred_top >= work_top + margin {
        preferred_top
    } else {
        preferred_bottom.clamp(
            work_top + margin,
            (work_bottom - f64::from(height) - margin).max(work_top + margin),
        )
    };

    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(tauri_err)?;
    window
        .set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
        .map_err(tauri_err)
}

fn show_and_focus(window: &WebviewWindow) -> Result<(), String> {
    start_overlay_focus_grace();
    window.show().map_err(tauri_err)?;
    window.set_focus().map_err(tauri_err)?;

    let retry_window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(90));
        if matches!(retry_window.is_visible(), Ok(true))
            && matches!(retry_window.is_focused(), Ok(false))
        {
            let _ = retry_window.set_focus();
        }
    });

    Ok(())
}

pub(crate) fn wake_quick_query(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(OVERLAY_WINDOW_LABEL)
        .ok_or_else(|| "ReadRay overlay 窗口不存在。".to_string())?;
    set_pending_overlay_intent(OverlayIntent::show_input())?;
    resize_overlay_window(&window, get_current_overlay_window_stage()?)?;
    show_and_focus(&window)?;
    app.emit_to(OVERLAY_WINDOW_LABEL, "readray://overlay-intent", ())
        .map_err(tauri_err)
}

fn toggle_window_visibility(window: &WebviewWindow) -> Result<bool, String> {
    let visible = window.is_visible().map_err(tauri_err)?;

    if visible {
        let _ = remember_overlay_position(window);
        window.hide().map_err(tauri_err)?;
        Ok(false)
    } else {
        resize_overlay_window(window, get_current_overlay_window_stage()?)?;
        show_and_focus(window)?;
        Ok(true)
    }
}

#[tauri::command]
fn stage1_status(window: tauri::WebviewWindow) -> Result<WindowState, String> {
    ensure_overlay_window(&window)?;
    Ok(WindowState {
        visible: window.is_visible().map_err(tauri_err)?,
        always_on_top: false,
    })
}

#[tauri::command]
fn shortcut_label() -> &'static str {
    desktop_lifecycle::DEFAULT_QUICK_QUERY_SHORTCUT
}

#[tauri::command]
fn toggle_overlay_window(window: tauri::WebviewWindow) -> Result<bool, String> {
    ensure_overlay_window(&window)?;
    toggle_window_visibility(&window)
}

#[tauri::command]
fn set_overlay_window_always_on_top(
    window: WebviewWindow,
    enabled: bool,
) -> Result<WindowState, String> {
    ensure_overlay_window(&window)?;
    window.set_always_on_top(enabled).map_err(tauri_err)?;
    if enabled {
        show_and_focus(&window)?;
    }

    Ok(WindowState {
        visible: window.is_visible().map_err(tauri_err)?,
        always_on_top: enabled,
    })
}

#[tauri::command]
fn prepare_overlay_input_window(window: WebviewWindow) -> Result<(), String> {
    ensure_overlay_window(&window)?;
    deepseek_explanation::cancel_all_explanation_requests();
    resize_overlay_window(&window, OverlayWindowStage::Input)?;
    window.set_always_on_top(true).map_err(tauri_err)?;
    show_and_focus(&window)
}

#[tauri::command]
fn set_overlay_window_stage(window: WebviewWindow, stage: &str) -> Result<(), String> {
    ensure_overlay_window(&window)?;
    let overlay_stage = match stage {
        "input" | "loading" => OverlayWindowStage::Input,
        "result" => OverlayWindowStage::Result,
        "error" => OverlayWindowStage::Error,
        "quick-ai" => OverlayWindowStage::QuickAi,
        other => return Err(format!("未知 overlay stage：{other}")),
    };

    resize_overlay_window(&window, overlay_stage)
}

#[tauri::command]
fn hide_overlay_window(window: WebviewWindow) -> Result<(), String> {
    ensure_overlay_window(&window)?;
    deepseek_explanation::cancel_explanation_scope(
        deepseek_explanation::ExplanationRequestScope::Manual,
    );
    let _ = remember_overlay_position(&window);
    window.hide().map_err(tauri_err)
}

#[tauri::command]
fn take_overlay_intent(window: WebviewWindow) -> Result<Option<OverlayIntent>, String> {
    ensure_overlay_window(&window)?;
    let intent = pending_overlay_intent().lock().map_err(tauri_err)?.take();

    if let Some(intent) = intent.as_ref() {
        eprintln!("READRAY_OVERLAY_INTENT_TAKEN={}", intent.kind);
    }

    Ok(intent)
}

#[tauri::command]
fn main_window_is_maximized(window: WebviewWindow) -> Result<bool, String> {
    ensure_main_window(&window)?;
    window.is_maximized().map_err(tauri_err)
}

#[tauri::command]
fn minimize_main_window(window: WebviewWindow) -> Result<(), String> {
    ensure_main_window(&window)?;
    window.minimize().map_err(tauri_err)
}

#[tauri::command]
fn toggle_main_window_maximized(window: WebviewWindow) -> Result<bool, String> {
    ensure_main_window(&window)?;
    if window.is_maximized().map_err(tauri_err)? {
        window.unmaximize().map_err(tauri_err)?;
        Ok(false)
    } else {
        window.maximize().map_err(tauri_err)?;
        Ok(true)
    }
}

#[tauri::command]
fn start_main_window_drag(window: WebviewWindow) -> Result<(), String> {
    ensure_main_window(&window)?;
    window.start_dragging().map_err(tauri_err)
}

#[tauri::command]
fn hide_main_window(window: WebviewWindow) -> Result<(), String> {
    ensure_main_window(&window)?;
    window.hide().map_err(tauri_err)
}

#[cfg(target_os = "windows")]
fn show_anchored_overlay_window(
    window: &WebviewWindow,
    stage: AnchoredOverlayStage,
    anchor_rect: &windows_uia::ScreenRect,
) -> Result<(), String> {
    place_anchored_overlay_window(window, stage.logical_size(), anchor_rect)?;
    window.set_always_on_top(true).map_err(tauri_err)?;
    show_and_focus(window)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn present_anchored_overlay_window(
    window: WebviewWindow,
    stage: &str,
    anchor_rect: windows_uia::ScreenRect,
) -> Result<(), String> {
    ensure_overlay_window(&window)?;
    let anchored_stage = match stage {
        "loading" => {
            deepseek_explanation::cancel_explanation_scope(
                deepseek_explanation::ExplanationRequestScope::Manual,
            );
            AnchoredOverlayStage::Loading
        }
        "result" => AnchoredOverlayStage::Result,
        "error" => AnchoredOverlayStage::Error,
        other => return Err(format!("未知 anchored overlay stage：{other}")),
    };

    show_anchored_overlay_window(&window, anchored_stage, &anchor_rect)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn resize_anchored_overlay_window(
    window: WebviewWindow,
    width: f64,
    height: f64,
    anchor_rect: windows_uia::ScreenRect,
) -> Result<(), String> {
    ensure_overlay_window(&window)?;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err("anchored overlay 尺寸必须是有限的正数。".to_string());
    }

    place_anchored_overlay_window(&window, LogicalSize::new(width, height), &anchor_rect)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn hide_anchored_overlay_window(window: WebviewWindow) -> Result<(), String> {
    ensure_overlay_window(&window)?;
    deepseek_explanation::cancel_explanation_scope(
        deepseek_explanation::ExplanationRequestScope::Anchored,
    );
    window.hide().map_err(tauri_err)
}

#[tauri::command]
fn begin_overlay_window_drag(
    window: WebviewWindow,
    pointer_x: f64,
    pointer_y: f64,
) -> Result<(), String> {
    ensure_overlay_window(&window)?;
    let position = window.outer_position().map_err(tauri_err)?;
    let scale_factor = window.scale_factor().map_err(tauri_err)?;
    let mut active_drag = active_overlay_drag().lock().map_err(tauri_err)?;

    *active_drag = Some(ActiveOverlayDrag {
        pointer_x,
        pointer_y,
        window_x: f64::from(position.x) / scale_factor,
        window_y: f64::from(position.y) / scale_factor,
    });

    Ok(())
}

#[tauri::command]
fn drag_overlay_window(
    window: WebviewWindow,
    pointer_x: f64,
    pointer_y: f64,
) -> Result<(), String> {
    ensure_overlay_window(&window)?;
    let Some(active_drag) = *active_overlay_drag().lock().map_err(tauri_err)? else {
        return Ok(());
    };

    let x = active_drag.window_x + pointer_x - active_drag.pointer_x;
    let y = active_drag.window_y + pointer_y - active_drag.pointer_y;

    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(tauri_err)?;
    save_overlay_position_from_logical(x, y)
}

#[tauri::command]
fn finish_overlay_window_drag(window: WebviewWindow) -> Result<(), String> {
    ensure_overlay_window(&window)?;
    *active_overlay_drag().lock().map_err(tauri_err)? = None;
    remember_overlay_position(&window)
}

#[tauri::command]
async fn deepseek_smoke_test(prompt: Option<String>) -> Result<DeepSeekSmokeResult, String> {
    let model =
        std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| DEFAULT_DEEPSEEK_MODEL.to_string());
    let api_key = match secret_store::deepseek_api_key_state()?.into_key() {
        Some(value) => value,
        None => {
            return Ok(DeepSeekSmokeResult {
                configured: false,
                ok: false,
                model,
                status: None,
                message: "未配置 DeepSeek API Key，已跳过真实 API 调用。".to_string(),
                content_preview: None,
            });
        }
    };

    let user_prompt =
        prompt.unwrap_or_else(|| "Reply with one short English sentence for ReadRay.".to_string());
    let request_body = json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": "You are a concise assistant for a desktop app smoke test."
            },
            {
                "role": "user",
                "content": user_prompt
            }
        ],
        "stream": false,
        "max_tokens": 64
    });

    let response = crate::deepseek_client::shared_http_client()?
        .post(format!("{DEEPSEEK_BASE_URL}/chat/completions"))
        .bearer_auth(api_key)
        .json(&request_body)
        .send()
        .await
        .map_err(|error| format!("DeepSeek 请求失败：{error}"))?;

    let status = response.status();
    let status_code = status.as_u16();
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("DeepSeek 响应不是有效 JSON：{error}"))?;

    if !status.is_success() {
        return Ok(DeepSeekSmokeResult {
            configured: true,
            ok: false,
            model,
            status: Some(status_code),
            message: value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(|message| message.as_str())
                .unwrap_or("DeepSeek API 返回非成功状态。")
                .to_string(),
            content_preview: None,
        });
    }

    let content = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    Ok(DeepSeekSmokeResult {
        configured: true,
        ok: !content.is_empty(),
        model,
        status: Some(status_code),
        message: if content.is_empty() {
            "DeepSeek API 返回成功，但没有解析到 message.content。".to_string()
        } else {
            "DeepSeek API 调用成功。".to_string()
        },
        content_preview: if content.is_empty() {
            None
        } else {
            Some(content.chars().take(160).collect())
        },
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_project_env();

    #[cfg(desktop)]
    use tauri::Emitter;
    #[cfg(desktop)]
    use tauri::Manager;
    #[cfg(desktop)]
    use tauri_plugin_global_shortcut::ShortcutState;

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if !argv
                .iter()
                .any(|argument| argument == desktop_lifecycle::AUTOSTART_ARGUMENT)
            {
                if let Err(error) = desktop_lifecycle::show_main_window(app) {
                    eprintln!("READRAY_SINGLE_INSTANCE_RESTORE_ERROR={error}");
                }
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            #[cfg(desktop)]
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    #[cfg(target_os = "windows")]
                    if desktop_lifecycle::shortcut_action(shortcut)
                        == Some(desktop_lifecycle::ShortcutAction::SelectionExplanation)
                    {
                        match event.state() {
                            ShortcutState::Pressed => {
                                let capture = windows_uia::capture_foreground();
                                eprintln!(
                                    "READRAY_UIA_CAPTURE ok={} selected_chars={} has_context={} has_minimal_context={} has_anchor={} text_pattern={}",
                                    capture.ok,
                                    capture
                                        .selected_text
                                        .as_deref()
                                        .map(|text| text.chars().count())
                                        .unwrap_or(0),
                                    capture.context_text.is_some(),
                                    capture.minimal_context.is_some(),
                                    capture.anchor_rect.is_some(),
                                    capture.text_pattern.unwrap_or("none")
                                );

                                match pending_uia_capture().lock() {
                                    Ok(mut pending) => *pending = Some(capture),
                                    Err(error) => {
                                        eprintln!("READRAY_UIA_PENDING_CAPTURE_ERROR={error}")
                                    }
                                }
                            }
                            ShortcutState::Released => {
                                let capture = match pending_uia_capture().lock() {
                                    Ok(mut pending) => pending.take(),
                                    Err(error) => {
                                        eprintln!("READRAY_UIA_PENDING_CAPTURE_ERROR={error}");
                                        None
                                    }
                                };

                                if let Some(capture) = capture {
                                    let loading_anchor = capture.anchor_rect.clone().filter(|_| {
                                        capture
                                            .selected_text
                                            .as_deref()
                                            .is_some_and(|text| !text.trim().is_empty())
                                    });

                                    if let Err(error) = set_pending_overlay_intent(
                                        OverlayIntent::uia_capture(capture),
                                    ) {
                                        eprintln!("READRAY_OVERLAY_INTENT_ERROR={error}");
                                        return;
                                    }

                                    if let Some(anchor_rect) = loading_anchor {
                                        if let Some(window) =
                                            app.get_webview_window(OVERLAY_WINDOW_LABEL)
                                        {
                                            if let Err(error) = show_anchored_overlay_window(
                                                &window,
                                                AnchoredOverlayStage::Loading,
                                                &anchor_rect,
                                            ) {
                                                eprintln!(
                                                    "READRAY_ANCHORED_OVERLAY_WAKE_ERROR={error}"
                                                );
                                            } else {
                                                eprintln!("READRAY_ANCHORED_OVERLAY_WAKE=ok");
                                            }
                                        }
                                    }

                                    if let Err(error) = app.emit_to(
                                        OVERLAY_WINDOW_LABEL,
                                        "readray://overlay-intent",
                                        (),
                                    ) {
                                        eprintln!("READRAY_OVERLAY_INTENT_EMIT_ERROR={error}");
                                    }
                                }
                            }
                        }
                        return;
                    }

                    if desktop_lifecycle::shortcut_action(shortcut)
                        == Some(desktop_lifecycle::ShortcutAction::QuickQuery)
                        && matches!(event.state(), ShortcutState::Released)
                    {
                        eprintln!("READRAY_OVERLAY_SHORTCUT=released");
                        if let Err(error) = wake_quick_query(app) {
                            eprintln!("READRAY_OVERLAY_SHOW_ERROR={error}");
                        } else {
                            eprintln!("READRAY_OVERLAY_SHOW=ok");
                        }
                    }
                })
                .build(),
        )
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg(desktop_lifecycle::AUTOSTART_ARGUMENT)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            learning_records::initialize_for_app(app.handle()).map_err(std::io::Error::other)?;

            #[cfg(desktop)]
            {
                desktop_lifecycle::setup_main_window_state(app.handle())
                    .map_err(std::io::Error::other)?;
                settings::initialize_desktop_preferences(app.handle())
                    .map_err(std::io::Error::other)?;
                desktop_lifecycle::setup_tray(app).map_err(std::io::Error::other)?;
                if desktop_lifecycle::launched_from_autostart() {
                    if let Some(main) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                        let _ = main.hide();
                    }
                    if let Some(overlay) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
                        let _ = overlay.hide();
                    }
                } else {
                    desktop_lifecycle::show_main_window(app.handle())
                        .map_err(std::io::Error::other)?;
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) if window.label() == OVERLAY_WINDOW_LABEL => {
                if overlay_focus_grace_active() {
                    eprintln!("READRAY_OVERLAY_FOCUS=lost_ignored");
                } else {
                    eprintln!("READRAY_OVERLAY_FOCUS=lost");
                    deepseek_explanation::cancel_all_explanation_requests();
                    let _ = window.hide();
                    let _ = window.emit("readray://hidden", ());
                }
            }
            WindowEvent::CloseRequested { api, .. } if window.label() == MAIN_WINDOW_LABEL => {
                api.prevent_close();
                desktop_lifecycle::handle_main_close(window.app_handle(), window);
            }
            WindowEvent::CloseRequested { api, .. } if window.label() == OVERLAY_WINDOW_LABEL => {
                api.prevent_close();
                deepseek_explanation::cancel_all_explanation_requests();
                let _ = window.hide();
                let _ = window.emit("readray://hidden", ());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            stage1_status,
            shortcut_label,
            toggle_overlay_window,
            set_overlay_window_always_on_top,
            deepseek_smoke_test,
            prepare_overlay_input_window,
            set_overlay_window_stage,
            hide_overlay_window,
            take_overlay_intent,
            main_window_is_maximized,
            minimize_main_window,
            toggle_main_window_maximized,
            start_main_window_drag,
            hide_main_window,
            #[cfg(target_os = "windows")]
            present_anchored_overlay_window,
            #[cfg(target_os = "windows")]
            resize_anchored_overlay_window,
            #[cfg(target_os = "windows")]
            hide_anchored_overlay_window,
            begin_overlay_window_drag,
            drag_overlay_window,
            finish_overlay_window_drag,
            deepseek_explanation::create_explanation_card,
            deepseek_explanation::cancel_explanation_request,
            learning_records::list_learning_records,
            learning_records::search_learning_records,
            learning_records::get_learning_record,
            learning_records::list_learning_targets,
            learning_records::search_learning_targets,
            learning_records::get_learning_target,
            learning_records::delete_learning_record,
            learning_records::get_today_learning_summary,
            review::get_review_feed_page,
            review::get_review_feed_item_state,
            review::prepare_review_feed_card,
            review::submit_review_outcome,
            review::undo_review_outcome,
            review::save_review_quality_feedback,
            review::undo_review_quality_feedback,
            quick_ai::create_quick_ai_conversation,
            quick_ai::get_quick_ai_conversation,
            quick_ai::list_recent_quick_ai_conversations,
            quick_ai::list_all_quick_ai_conversations,
            quick_ai::rename_quick_ai_conversation,
            quick_ai::delete_quick_ai_conversation,
            quick_ai::export_quick_ai_conversation,
            quick_ai::send_quick_ai_message,
            quick_ai::send_quick_ai_message_streaming,
            quick_ai::abort_quick_ai_streaming,
            settings::get_settings_snapshot,
            settings::get_app_preferences,
            settings::update_app_preferences,
            settings::get_autostart_enabled,
            settings::set_autostart_enabled,
            settings::validate_and_save_deepseek_api_key,
            settings::clear_deepseek_api_key,
            settings::get_deepseek_balance,
            settings::open_readray_data_directory,
            settings::backup_readray_database,
            themes::get_theme_snapshot,
            themes::inspect_theme_package,
            themes::import_theme_package,
            themes::select_theme,
            themes::delete_custom_theme,
            model_usage::get_model_usage_summary,
            writing::create_writing_document,
            writing::list_writing_documents,
            writing::get_writing_document,
            writing::save_writing_draft,
            writing::delete_writing_document,
            writing::complete_writing_document,
            writing::continue_writing_document,
            writing::analyze_writing_document,
            writing::ask_writing_question,
            desktop_lifecycle::request_app_exit,
            desktop_lifecycle::get_pending_app_exit_request,
            desktop_lifecycle::restore_main_window,
            desktop_lifecycle::cancel_app_exit,
            desktop_lifecycle::complete_app_exit,
            desktop_lifecycle::force_app_exit,
            desktop_lifecycle::apply_main_window_close_behavior
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
