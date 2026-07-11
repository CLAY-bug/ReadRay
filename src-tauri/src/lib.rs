use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{
    LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent,
};

pub mod deepseek_explanation;
pub mod explanation;
pub mod learning_records;
#[cfg(target_os = "windows")]
pub mod windows_uia;

const READRAY_SHORTCUT_LABEL: &str = "Ctrl+Alt+R";
pub(crate) const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
pub(crate) const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-flash";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowState {
    visible: bool,
    always_on_top: bool,
}

#[derive(Clone, Copy)]
enum OverlayWindowStage {
    Input,
    Result,
    Error,
}

impl OverlayWindowStage {
    fn size(self) -> LogicalSize<f64> {
        match self {
            Self::Input => LogicalSize::new(720.0, 104.0),
            Self::Result => LogicalSize::new(800.0, 560.0),
            Self::Error => LogicalSize::new(720.0, 132.0),
        }
    }
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

static SAVED_OVERLAY_POSITION: OnceLock<Mutex<Option<SavedOverlayPosition>>> = OnceLock::new();
static ACTIVE_OVERLAY_DRAG: OnceLock<Mutex<Option<ActiveOverlayDrag>>> = OnceLock::new();

fn tauri_err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn saved_overlay_position() -> &'static Mutex<Option<SavedOverlayPosition>> {
    SAVED_OVERLAY_POSITION.get_or_init(|| Mutex::new(None))
}

fn active_overlay_drag() -> &'static Mutex<Option<ActiveOverlayDrag>> {
    ACTIVE_OVERLAY_DRAG.get_or_init(|| Mutex::new(None))
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
    window.set_size(stage.size()).map_err(tauri_err)?;
    if let Some(position) = get_saved_overlay_position()? {
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
    window.show().map_err(tauri_err)?;
    window.set_focus().map_err(tauri_err)
}

fn toggle_window_visibility(window: &WebviewWindow) -> Result<bool, String> {
    let visible = window.is_visible().map_err(tauri_err)?;

    if visible {
        let _ = remember_overlay_position(window);
        window.hide().map_err(tauri_err)?;
        Ok(false)
    } else {
        resize_overlay_window(window, OverlayWindowStage::Input)?;
        show_and_focus(window)?;
        Ok(true)
    }
}

#[tauri::command]
fn stage1_status(window: tauri::WebviewWindow) -> Result<WindowState, String> {
    Ok(WindowState {
        visible: window.is_visible().map_err(tauri_err)?,
        always_on_top: false,
    })
}

#[tauri::command]
fn shortcut_label() -> &'static str {
    READRAY_SHORTCUT_LABEL
}

#[tauri::command]
fn toggle_main_window(window: tauri::WebviewWindow) -> Result<bool, String> {
    toggle_window_visibility(&window)
}

#[tauri::command]
fn set_main_window_always_on_top(
    window: WebviewWindow,
    enabled: bool,
) -> Result<WindowState, String> {
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
    resize_overlay_window(&window, OverlayWindowStage::Input)?;
    window.set_always_on_top(true).map_err(tauri_err)?;
    show_and_focus(&window)
}

#[tauri::command]
fn set_overlay_window_stage(window: WebviewWindow, stage: &str) -> Result<(), String> {
    let overlay_stage = match stage {
        "input" | "loading" => OverlayWindowStage::Input,
        "result" => OverlayWindowStage::Result,
        "error" => OverlayWindowStage::Error,
        other => return Err(format!("未知 overlay stage：{other}")),
    };

    resize_overlay_window(&window, overlay_stage)
}

#[tauri::command]
fn hide_overlay_window(window: WebviewWindow) -> Result<(), String> {
    let _ = remember_overlay_position(&window);
    window.hide().map_err(tauri_err)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn present_anchored_overlay_window(
    window: WebviewWindow,
    stage: &str,
    anchor_rect: windows_uia::ScreenRect,
) -> Result<(), String> {
    let anchored_stage = match stage {
        "loading" => AnchoredOverlayStage::Loading,
        "result" => AnchoredOverlayStage::Result,
        "error" => AnchoredOverlayStage::Error,
        other => return Err(format!("未知 anchored overlay stage：{other}")),
    };

    place_anchored_overlay_window(&window, anchored_stage.logical_size(), &anchor_rect)?;
    window.set_always_on_top(true).map_err(tauri_err)?;
    show_and_focus(&window)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn resize_anchored_overlay_window(
    window: WebviewWindow,
    width: f64,
    height: f64,
    anchor_rect: windows_uia::ScreenRect,
) -> Result<(), String> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err("anchored overlay 尺寸必须是有限的正数。".to_string());
    }

    place_anchored_overlay_window(&window, LogicalSize::new(width, height), &anchor_rect)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn hide_anchored_overlay_window(window: WebviewWindow) -> Result<(), String> {
    window.hide().map_err(tauri_err)
}

#[tauri::command]
fn begin_overlay_window_drag(
    window: WebviewWindow,
    pointer_x: f64,
    pointer_y: f64,
) -> Result<(), String> {
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
    *active_overlay_drag().lock().map_err(tauri_err)? = None;
    remember_overlay_position(&window)
}

#[tauri::command]
async fn deepseek_smoke_test(prompt: Option<String>) -> Result<DeepSeekSmokeResult, String> {
    let model =
        std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| DEFAULT_DEEPSEEK_MODEL.to_string());
    let api_key = match std::env::var("DEEPSEEK_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            return Ok(DeepSeekSmokeResult {
                configured: false,
                ok: false,
                model,
                status: None,
                message: "未设置 DEEPSEEK_API_KEY，已跳过真实 API 调用。".to_string(),
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

    let response = reqwest::Client::new()
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
    use tauri_plugin_global_shortcut::{
        Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
    };

    #[cfg(desktop)]
    let readray_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyR);
    #[cfg(desktop)]
    let registered_shortcut = readray_shortcut.clone();
    #[cfg(all(desktop, target_os = "windows"))]
    let uia_capture_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyU);
    #[cfg(all(desktop, target_os = "windows"))]
    let registered_uia_capture_shortcut = uia_capture_shortcut.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            #[cfg(desktop)]
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    #[cfg(target_os = "windows")]
                    if shortcut == &uia_capture_shortcut
                        && matches!(event.state(), ShortcutState::Pressed)
                    {
                        let capture = windows_uia::capture_foreground();
                        match serde_json::to_string(&capture) {
                            Ok(json) => eprintln!("READRAY_UIA_CAPTURE={json}"),
                            Err(error) => {
                                eprintln!("READRAY_UIA_CAPTURE_SERIALIZE_ERROR={error}")
                            }
                        }
                        let _ = app.emit("readray://uia-capture", capture);
                        return;
                    }

                    if shortcut == &readray_shortcut
                        && matches!(event.state(), ShortcutState::Pressed)
                    {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = resize_overlay_window(&window, OverlayWindowStage::Input);
                            let _ = show_and_focus(&window);
                            let _ = app.emit("readray://show-input", ());
                        }
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            learning_records::initialize_for_app(app.handle()).map_err(std::io::Error::other)?;

            #[cfg(desktop)]
            {
                app.global_shortcut().register(registered_shortcut)?;
                #[cfg(target_os = "windows")]
                app.global_shortcut()
                    .register(registered_uia_capture_shortcut)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Focused(false) = event {
                let _ = window.hide();
                let _ = window.emit("readray://hidden", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            stage1_status,
            shortcut_label,
            toggle_main_window,
            set_main_window_always_on_top,
            deepseek_smoke_test,
            prepare_overlay_input_window,
            set_overlay_window_stage,
            hide_overlay_window,
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
            learning_records::list_learning_records,
            learning_records::search_learning_records,
            learning_records::get_learning_record,
            learning_records::delete_learning_record
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
