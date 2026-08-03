use crate::deepseek_client::{configured_model, post_chat_completion_with_api_key};
use crate::{learning_records, secret_store};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    api_key_configured: bool,
    api_key_source: &'static str,
    model: String,
    app_data_directory: String,
    learning_record_count: i64,
    conversation_count: i64,
    writing_document_count: i64,
    app_version: String,
}

impl SettingsSnapshot {
    fn with_api_key_state(mut self, configured: bool, source: &'static str) -> Self {
        self.api_key_configured = configured;
        self.api_key_source = source;
        self
    }
}

struct DataCounts {
    learning_records: i64,
    conversations: i64,
    writing_documents: i64,
}

fn count_table(connection: &Connection, table: &str) -> Result<i64, String> {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .map_err(|error| format!("无法读取 ReadRay 本地数据概览：{error}"))
}

fn read_data_counts(connection: &Connection) -> Result<DataCounts, String> {
    Ok(DataCounts {
        learning_records: count_table(connection, "learning_records")?,
        conversations: count_table(connection, "quick_ai_conversations")?,
        writing_documents: count_table(connection, "writing_documents")?,
    })
}

fn validate_api_key_input(api_key: &str) -> Result<&str, String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("请输入 DeepSeek API Key。".to_string());
    }
    if trimmed.len() > 2_048 {
        return Err("API Key 长度异常，请检查后重试。".to_string());
    }
    if trimmed
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err("API Key 中不能包含空格、换行或控制字符。".to_string());
    }
    Ok(trimmed)
}

async fn validate_deepseek_connection(api_key: &str) -> Result<(), String> {
    let request_body = json!({
        "model": configured_model(),
        "messages": [
            {
                "role": "system",
                "content": "You are validating a ReadRay DeepSeek connection."
            },
            {
                "role": "user",
                "content": "Reply OK."
            }
        ],
        "stream": false,
        "max_tokens": 2,
        "temperature": 0
    });
    let _: Value =
        post_chat_completion_with_api_key("DeepSeek API Key 验证", &request_body, api_key).await?;
    Ok(())
}

fn settings_snapshot(app: &AppHandle) -> Result<SettingsSnapshot, String> {
    let api_key_state = secret_store::deepseek_api_key_state()?;
    let counts = read_data_counts(&learning_records::open_database_for_app(app)?)?;
    let app_data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定 ReadRay 应用数据目录：{error}"))?;

    Ok(SettingsSnapshot {
        api_key_configured: api_key_state.configured(),
        api_key_source: api_key_state.source(),
        model: configured_model(),
        app_data_directory: app_data_directory.to_string_lossy().into_owned(),
        learning_record_count: counts.learning_records,
        conversation_count: counts.conversations,
        writing_document_count: counts.writing_documents,
        app_version: app.package_info().version.to_string(),
    })
}

fn run_api_key_state_change<LoadSnapshot, ChangeCredential>(
    load_snapshot: LoadSnapshot,
    change_credential: ChangeCredential,
    configured: bool,
    source: &'static str,
) -> Result<SettingsSnapshot, String>
where
    LoadSnapshot: FnOnce() -> Result<SettingsSnapshot, String>,
    ChangeCredential: FnOnce() -> Result<(), String>,
{
    // 先准备返回前端所需的全部非敏感信息。凭据一旦变更成功，后续只做
    // 不会失败的内存状态更新，避免 SQLite 概览错误反向误报凭据操作失败。
    let snapshot = load_snapshot()?;
    change_credential()?;
    Ok(snapshot.with_api_key_state(configured, source))
}

#[tauri::command]
pub fn get_settings_snapshot(app: AppHandle) -> Result<SettingsSnapshot, String> {
    settings_snapshot(&app)
}

#[tauri::command]
pub async fn validate_and_save_deepseek_api_key(
    app: AppHandle,
    api_key: String,
) -> Result<SettingsSnapshot, String> {
    let api_key = validate_api_key_input(&api_key)?;
    validate_deepseek_connection(api_key).await?;
    run_api_key_state_change(
        || settings_snapshot(&app),
        || secret_store::save_deepseek_api_key(api_key),
        true,
        "credential",
    )
}

#[tauri::command]
pub fn clear_deepseek_api_key(app: AppHandle) -> Result<SettingsSnapshot, String> {
    run_api_key_state_change(
        || settings_snapshot(&app),
        secret_store::clear_deepseek_api_key,
        false,
        "none",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn fixture_snapshot(configured: bool, source: &'static str) -> SettingsSnapshot {
        SettingsSnapshot {
            api_key_configured: configured,
            api_key_source: source,
            model: "deepseek-test".to_string(),
            app_data_directory: "C:\\ReadRay".to_string(),
            learning_record_count: 7,
            conversation_count: 5,
            writing_document_count: 3,
            app_version: "0.1.0".to_string(),
        }
    }

    #[test]
    fn api_key_validation_rejects_empty_whitespace_and_control_characters() {
        assert!(validate_api_key_input("").is_err());
        assert!(validate_api_key_input("only spaces").is_err());
        assert!(validate_api_key_input("key\nvalue").is_err());
        assert_eq!(
            validate_api_key_input("  valid-secret  ").unwrap(),
            "valid-secret"
        );
    }

    #[test]
    fn data_counts_are_read_from_sqlite_tables() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE learning_records (id INTEGER PRIMARY KEY);
                 CREATE TABLE quick_ai_conversations (id INTEGER PRIMARY KEY);
                 CREATE TABLE writing_documents (id INTEGER PRIMARY KEY);
                 INSERT INTO learning_records DEFAULT VALUES;
                 INSERT INTO learning_records DEFAULT VALUES;
                 INSERT INTO quick_ai_conversations DEFAULT VALUES;
                 INSERT INTO writing_documents DEFAULT VALUES;",
            )
            .unwrap();
        let counts = read_data_counts(&connection).unwrap();
        assert_eq!(counts.learning_records, 2);
        assert_eq!(counts.conversations, 1);
        assert_eq!(counts.writing_documents, 1);
    }

    #[test]
    fn save_uses_prepared_snapshot_and_returns_credential_state() {
        let persisted_source = Cell::new("environment");
        let updated = run_api_key_state_change(
            || Ok(fixture_snapshot(true, "environment")),
            || {
                persisted_source.set("credential");
                Ok(())
            },
            true,
            "credential",
        )
        .unwrap();

        assert_eq!(persisted_source.get(), "credential");
        assert!(updated.api_key_configured);
        assert_eq!(updated.api_key_source, "credential");
        assert_eq!(updated.learning_record_count, 7);
        assert_eq!(updated.conversation_count, 5);
        assert_eq!(updated.writing_document_count, 3);
    }

    #[test]
    fn clear_uses_prepared_snapshot_and_returns_missing_state() {
        let persisted_source = Cell::new("credential");
        let updated = run_api_key_state_change(
            || Ok(fixture_snapshot(true, "credential")),
            || {
                persisted_source.set("none");
                Ok(())
            },
            false,
            "none",
        )
        .unwrap();

        assert_eq!(persisted_source.get(), "none");
        assert!(!updated.api_key_configured);
        assert_eq!(updated.api_key_source, "none");
        assert_eq!(updated.learning_record_count, 7);
    }

    #[test]
    fn save_snapshot_failure_prevents_credential_change() {
        let credential_change_called = Cell::new(false);
        let result = run_api_key_state_change(
            || Err("无法读取 ReadRay 本地数据概览".to_string()),
            || {
                credential_change_called.set(true);
                Ok(())
            },
            true,
            "credential",
        );

        assert!(result.is_err());
        assert!(!credential_change_called.get());
    }

    #[test]
    fn clear_snapshot_failure_prevents_credential_change() {
        let credential_change_called = Cell::new(false);
        let result = run_api_key_state_change(
            || Err("无法读取 ReadRay 本地数据概览".to_string()),
            || {
                credential_change_called.set(true);
                Ok(())
            },
            false,
            "none",
        );

        assert!(result.is_err());
        assert!(!credential_change_called.get());
    }

    #[test]
    fn failed_save_keeps_previous_key_state() {
        let persisted_source = Cell::new("credential");
        let result = run_api_key_state_change(
            || Ok(fixture_snapshot(true, "credential")),
            || Err("无法写入 Windows 凭据管理器".to_string()),
            true,
            "credential",
        );

        assert!(result.is_err());
        assert_eq!(persisted_source.get(), "credential");
    }

    #[test]
    fn failed_clear_keeps_previous_key_state() {
        let persisted_source = Cell::new("credential");
        let result = run_api_key_state_change(
            || Ok(fixture_snapshot(true, "credential")),
            || Err("无法从 Windows 凭据管理器清除 API Key".to_string()),
            false,
            "none",
        );

        assert!(result.is_err());
        assert_eq!(persisted_source.get(), "credential");
    }
}
