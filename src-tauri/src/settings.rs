use crate::deepseek_client::{
    configured_model, get_deepseek_json, post_chat_completion_with_api_key,
};
use crate::{learning_records, secret_store};
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

static BACKUP_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeepSeekBalanceResponse {
    is_available: bool,
    balance_infos: Vec<DeepSeekBalanceInfoResponse>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeepSeekBalanceInfoResponse {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekBalance {
    is_available: bool,
    balances: Vec<DeepSeekCurrencyBalance>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekCurrencyBalance {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseBackupResult {
    file_name: String,
    file_path: String,
    byte_size: u64,
    created_at_unix_ms: i64,
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

fn is_valid_currency_code(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn is_valid_non_negative_decimal(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 {
        return false;
    }
    let mut parts = value.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    fraction.is_none_or(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn parse_deepseek_balance(value: Value) -> Result<DeepSeekBalance, String> {
    let response: DeepSeekBalanceResponse = serde_json::from_value(value)
        .map_err(|error| format!("DeepSeek 余额响应结构无效：{error}"))?;
    let mut currencies = HashSet::new();
    let mut balances = Vec::with_capacity(response.balance_infos.len());

    for balance in response.balance_infos {
        if !is_valid_currency_code(&balance.currency) {
            return Err("DeepSeek 余额响应包含无效币种代码。".to_string());
        }
        if !currencies.insert(balance.currency.clone()) {
            return Err(format!(
                "DeepSeek 余额响应重复返回币种 {}。",
                balance.currency
            ));
        }
        if !is_valid_non_negative_decimal(&balance.total_balance)
            || !is_valid_non_negative_decimal(&balance.granted_balance)
            || !is_valid_non_negative_decimal(&balance.topped_up_balance)
        {
            return Err(format!(
                "DeepSeek 余额响应中的 {} 金额格式无效。",
                balance.currency
            ));
        }
        balances.push(DeepSeekCurrencyBalance {
            currency: balance.currency,
            total_balance: balance.total_balance,
            granted_balance: balance.granted_balance,
            topped_up_balance: balance.topped_up_balance,
        });
    }

    Ok(DeepSeekBalance {
        is_available: response.is_available,
        balances,
    })
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
    let app_data_directory = app_data_directory(app)?;

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

fn app_data_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("无法确定 ReadRay 应用数据目录：{error}"))
}

fn open_data_directory_with(
    directory: &Path,
    open: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    if !directory.is_dir() {
        return Err(format!(
            "ReadRay 数据目录不存在或不是文件夹：{}",
            directory.display()
        ));
    }
    open(directory).map_err(|error| format!("无法打开 ReadRay 数据目录：{error}"))
}

fn now_unix_ms() -> Result<i64, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间早于 Unix epoch：{error}"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| "当前时间超出 ReadRay 支持范围。".to_string())
}

fn paths_refer_to_same_file(source: &Path, destination: &Path) -> bool {
    if let (Ok(source), Ok(destination)) = (source.canonicalize(), destination.canonicalize()) {
        return source == destination;
    }
    source == destination
}

fn temporary_backup_path(destination: &Path) -> Result<PathBuf, String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "备份路径缺少父目录。".to_string())?;
    let file_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "备份文件名无效。".to_string())?;

    for _ in 0..100 {
        let counter = BACKUP_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{file_name}.readray-backup-{}-{counter}.tmp",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("无法为 ReadRay 备份分配临时文件。".to_string())
}

fn validate_backup_snapshot(path: &Path) -> Result<(), String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("无法校验 ReadRay 备份：{error}"))?;
    let quick_check: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|error| format!("无法校验 ReadRay 备份完整性：{error}"))?;
    if quick_check != "ok" {
        return Err(format!("ReadRay 备份完整性校验失败：{quick_check}"));
    }
    Ok(())
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn cleanup_temporary_backup(path: &Path) -> Result<(), String> {
    let artifacts = [
        path.to_path_buf(),
        sqlite_sidecar_path(path, "-journal"),
        sqlite_sidecar_path(path, "-wal"),
        sqlite_sidecar_path(path, "-shm"),
    ];
    for artifact in artifacts {
        if artifact.exists() {
            fs::remove_file(&artifact)
                .map_err(|error| format!("无法清理临时备份 {}：{error}", artifact.display()))?;
        }
    }
    Ok(())
}

fn create_sqlite_backup(
    connection: &Connection,
    source_path: &Path,
    destination: &Path,
) -> Result<DatabaseBackupResult, String> {
    if paths_refer_to_same_file(source_path, destination) {
        return Err("备份目标不能覆盖 ReadRay 正在使用的数据库。".to_string());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "备份路径缺少父目录。".to_string())?;
    if !parent.is_dir() {
        return Err(format!("备份目录不存在：{}", parent.display()));
    }

    let temporary_path = temporary_backup_path(destination)?;
    let temporary_path_text = temporary_path
        .to_str()
        .ok_or_else(|| "备份路径包含 SQLite 无法处理的字符。".to_string())?;

    let outcome = (|| {
        connection
            .execute("VACUUM INTO ?1", params![temporary_path_text])
            .map_err(|error| format!("创建 ReadRay SQLite 一致性快照失败：{error}"))?;
        validate_backup_snapshot(&temporary_path)?;

        let byte_size = fs::metadata(&temporary_path)
            .map_err(|error| format!("无法读取 ReadRay 备份大小：{error}"))?
            .len();
        if byte_size == 0 {
            return Err("ReadRay 备份为空，未写入目标文件。".to_string());
        }
        let file_name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "备份文件名无效。".to_string())?
            .to_string();
        let created_at_unix_ms = now_unix_ms()?;

        fs::rename(&temporary_path, destination)
            .map_err(|error| format!("无法完成 ReadRay 备份文件：{error}"))?;

        Ok(DatabaseBackupResult {
            file_name,
            file_path: destination.to_string_lossy().into_owned(),
            byte_size,
            created_at_unix_ms,
        })
    })();

    if outcome.is_err() {
        if let Err(cleanup_error) = cleanup_temporary_backup(&temporary_path) {
            return Err(format!(
                "{}；临时备份清理失败：{cleanup_error}",
                outcome.unwrap_err()
            ));
        }
    }
    outcome
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

#[tauri::command]
pub async fn get_deepseek_balance() -> Result<DeepSeekBalance, String> {
    let value = get_deepseek_json("DeepSeek 余额查询", "/user/balance").await?;
    parse_deepseek_balance(value)
}

#[tauri::command]
pub fn open_readray_data_directory(app: AppHandle) -> Result<(), String> {
    let directory = app_data_directory(&app)?;
    open_data_directory_with(&directory, |path| {
        app.opener()
            .open_path(path.to_string_lossy().into_owned(), None::<String>)
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub async fn backup_readray_database(
    app: AppHandle,
    file_path: String,
) -> Result<DatabaseBackupResult, String> {
    let destination = PathBuf::from(file_path.trim());
    if file_path.trim().is_empty() {
        return Err("请选择 ReadRay 备份保存位置。".to_string());
    }
    let source_path = learning_records::database_path_for_app(&app)?;

    tauri::async_runtime::spawn_blocking(move || {
        let connection = learning_records::open_database(&source_path)?;
        create_sqlite_backup(&connection, &source_path, &destination)
    })
    .await
    .map_err(|error| format!("ReadRay 备份任务异常结束：{error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "readray-settings-{label}-{}-{}",
            std::process::id(),
            TEST_PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn backup_source(path: &Path) -> Connection {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE learning_records (id INTEGER PRIMARY KEY);
                 CREATE TABLE quick_ai_conversations (id INTEGER PRIMARY KEY);
                 CREATE TABLE writing_documents (id INTEGER PRIMARY KEY);
                 CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO learning_records DEFAULT VALUES;
                 INSERT INTO learning_records DEFAULT VALUES;
                 INSERT INTO quick_ai_conversations DEFAULT VALUES;
                 INSERT INTO writing_documents DEFAULT VALUES;
                 INSERT INTO app_settings (key, value) VALUES ('non_sensitive', 'saved');",
            )
            .unwrap();
        connection
    }

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
    fn balance_mapping_supports_multiple_currencies() {
        let balance = parse_deepseek_balance(json!({
            "is_available": true,
            "balance_infos": [
                {
                    "currency": "CNY",
                    "total_balance": "110.00",
                    "granted_balance": "10.00",
                    "topped_up_balance": "100.00"
                },
                {
                    "currency": "USD",
                    "total_balance": "3.25",
                    "granted_balance": "0.25",
                    "topped_up_balance": "3.00"
                }
            ]
        }))
        .unwrap();

        assert!(balance.is_available);
        assert_eq!(balance.balances.len(), 2);
        assert_eq!(balance.balances[0].currency, "CNY");
        assert_eq!(balance.balances[0].total_balance, "110.00");
        assert_eq!(balance.balances[1].currency, "USD");
        assert_eq!(balance.balances[1].granted_balance, "0.25");
    }

    #[test]
    fn balance_mapping_rejects_invalid_or_ambiguous_payloads() {
        let duplicate_currency = json!({
            "is_available": true,
            "balance_infos": [
                {
                    "currency": "CNY",
                    "total_balance": "1.00",
                    "granted_balance": "0.00",
                    "topped_up_balance": "1.00"
                },
                {
                    "currency": "CNY",
                    "total_balance": "2.00",
                    "granted_balance": "0.00",
                    "topped_up_balance": "2.00"
                }
            ]
        });
        let invalid_amount = json!({
            "is_available": false,
            "balance_infos": [{
                "currency": "USD",
                "total_balance": "-1.00",
                "granted_balance": "0.00",
                "topped_up_balance": "0.00"
            }]
        });
        let unknown_field = json!({
            "is_available": true,
            "balance_infos": [],
            "api_key": "must-not-be-accepted"
        });

        assert!(parse_deepseek_balance(duplicate_currency).is_err());
        assert!(parse_deepseek_balance(invalid_amount).is_err());
        assert!(parse_deepseek_balance(unknown_field).is_err());
    }

    #[test]
    fn directory_open_failure_is_reported_without_fallback() {
        let directory = test_directory("open-failure");
        let error =
            open_data_directory_with(&directory, |_| Err("Windows Explorer 拒绝打开".to_string()))
                .unwrap_err();

        assert!(error.contains("无法打开 ReadRay 数据目录"));
        assert!(error.contains("Windows Explorer 拒绝打开"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn sqlite_backup_contains_all_tables_and_is_a_point_in_time_snapshot() {
        let root = test_directory("backup-success");
        let source_path = root.join("readray.sqlite3");
        let destination = root.join("readray-backup.sqlite3");
        let source = backup_source(&source_path);
        fs::write(&destination, b"old backup placeholder").unwrap();

        let result = create_sqlite_backup(&source, &source_path, &destination).unwrap();
        assert_eq!(result.file_name, "readray-backup.sqlite3");
        assert_eq!(result.file_path, destination.to_string_lossy());
        assert!(result.byte_size > 0);
        assert!(result.created_at_unix_ms > 0);

        source
            .execute("INSERT INTO learning_records DEFAULT VALUES", [])
            .unwrap();
        let backup = Connection::open(&destination).unwrap();
        assert_eq!(count_table(&backup, "learning_records").unwrap(), 2);
        assert_eq!(count_table(&backup, "quick_ai_conversations").unwrap(), 1);
        assert_eq!(count_table(&backup, "writing_documents").unwrap(), 1);
        let setting: String = backup
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'non_sensitive'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(setting, "saved");
        assert_eq!(count_table(&source, "learning_records").unwrap(), 3);

        drop(backup);
        drop(source);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_backup_failure_preserves_source_and_removes_partial_file() {
        let root = test_directory("backup-failure");
        let source_path = root.join("readray.sqlite3");
        let destination = root.join("failed-backup.sqlite3");
        let source = backup_source(&source_path);
        source.execute_batch("BEGIN IMMEDIATE").unwrap();

        let error = create_sqlite_backup(&source, &source_path, &destination).unwrap_err();
        assert!(error.contains("一致性快照失败"));
        assert!(!destination.exists());
        let partial_files = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with('.') && name.contains(".tmp")
            })
            .count();
        assert_eq!(partial_files, 0);

        source.execute_batch("ROLLBACK").unwrap();
        assert_eq!(count_table(&source, "learning_records").unwrap(), 2);
        let same_file_error =
            create_sqlite_backup(&source, &source_path, &source_path).unwrap_err();
        assert!(same_file_error.contains("不能覆盖"));
        assert_eq!(count_table(&source, "learning_records").unwrap(), 2);

        drop(source);
        let _ = fs::remove_dir_all(root);
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
