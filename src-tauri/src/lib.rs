use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;

const READRAY_SHORTCUT_LABEL: &str = "Ctrl+Alt+R";
const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_DEEPSEEK_MODEL: &str = "deepseek-v4-flash";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowState {
    visible: bool,
    always_on_top: bool,
}

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

fn tauri_err(error: impl std::fmt::Display) -> String {
    error.to_string()
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

fn show_and_focus(window: &tauri::WebviewWindow) -> Result<(), String> {
    window.show().map_err(tauri_err)?;
    window.set_focus().map_err(tauri_err)
}

fn toggle_window_visibility(window: &tauri::WebviewWindow) -> Result<bool, String> {
    let visible = window.is_visible().map_err(tauri_err)?;

    if visible {
        window.hide().map_err(tauri_err)?;
        Ok(false)
    } else {
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
    window: tauri::WebviewWindow,
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
                            let _ = toggle_window_visibility(&window);
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
        .invoke_handler(tauri::generate_handler![
            stage1_status,
            shortcut_label,
            toggle_main_window,
            set_main_window_always_on_top,
            deepseek_smoke_test
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
