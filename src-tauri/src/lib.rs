use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{LogicalPosition, LogicalSize, WebviewWindow, WindowEvent};

pub mod deepseek_explanation;
pub mod explanation;

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

const DEFAULT_OVERLAY_CENTER_Y_RATIO: f64 = 0.36;

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

    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            #[cfg(desktop)]
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
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
            #[cfg(desktop)]
            {
                app.global_shortcut().register(registered_shortcut)?;
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
            begin_overlay_window_drag,
            drag_overlay_window,
            finish_overlay_window_drag,
            deepseek_explanation::create_explanation_card
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
