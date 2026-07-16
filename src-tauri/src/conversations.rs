#[cfg(test)]
use crate::learning_records::open_database;
use crate::learning_records::{open_database_for_app, unix_time_ms};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::Path;
use tauri::AppHandle;

const MAX_MODEL_LEN: usize = 160;
const MAX_TITLE_LEN: usize = 80;
const MAX_MESSAGE_LEN: usize = 32_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub role: ConversationRole,
    pub content: String,
    pub sequence: i64,
    pub created_at_unix_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSnapshot {
    pub id: i64,
    pub title: Option<String>,
    pub model: String,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
    pub messages: Vec<ConversationMessage>,
}

struct StoredConversation {
    id: i64,
    title: Option<String>,
    model: String,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

struct StoredMessage {
    id: i64,
    conversation_id: i64,
    role: String,
    content: String,
    sequence: i64,
    created_at_unix_ms: i64,
}

pub(crate) struct ConversationStore {
    connection: Connection,
}

impl ConversationStore {
    pub(crate) fn open_for_app(app: &AppHandle) -> Result<Self, String> {
        Ok(Self {
            connection: open_database_for_app(app)?,
        })
    }

    #[cfg(test)]
    pub(crate) fn open_path(path: &Path) -> Result<Self, String> {
        Ok(Self {
            connection: open_database(path)?,
        })
    }

    pub(crate) fn create(&mut self, model: &str) -> Result<ConversationSnapshot, String> {
        validate_model(model)?;
        let timestamp = unix_time_ms()?;
        self.connection
            .execute(
                "INSERT INTO quick_ai_conversations (
                    title, model, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (NULL, ?1, ?2, ?2)",
                params![model, timestamp],
            )
            .map_err(|error| format!("Quick AI 对话创建失败：{error}"))?;

        self.get_required(self.connection.last_insert_rowid())
    }

    pub(crate) fn create_with_exchange(
        &mut self,
        model: &str,
        user_content: &str,
        assistant_content: &str,
    ) -> Result<ConversationSnapshot, String> {
        validate_model(model)?;
        validate_message("用户消息", user_content)?;
        validate_message("助手消息", assistant_content)?;
        let timestamp = unix_time_ms()?;
        let title = title_from_first_message(user_content);
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("Quick AI 对话事务无法开始：{error}"))?;
        transaction
            .execute(
                "INSERT INTO quick_ai_conversations (
                    title, model, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?3)",
                params![title, model, timestamp],
            )
            .map_err(|error| format!("Quick AI 对话创建失败：{error}"))?;
        let conversation_id = transaction.last_insert_rowid();
        insert_message(
            &transaction,
            conversation_id,
            ConversationRole::User,
            user_content,
            1,
            timestamp,
        )?;
        insert_message(
            &transaction,
            conversation_id,
            ConversationRole::Assistant,
            assistant_content,
            2,
            timestamp,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("Quick AI 对话事务无法提交：{error}"))?;

        self.get_required(conversation_id)
    }

    pub(crate) fn append_exchange(
        &mut self,
        conversation_id: i64,
        user_content: &str,
        assistant_content: &str,
    ) -> Result<ConversationSnapshot, String> {
        validate_message("用户消息", user_content)?;
        validate_message("助手消息", assistant_content)?;
        let timestamp = unix_time_ms()?;
        let title = title_from_first_message(user_content);
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("Quick AI 对话事务无法开始：{error}"))?;
        let next_sequence: Option<i64> = transaction
            .query_row(
                "SELECT MAX(sequence) + 1 FROM quick_ai_messages WHERE conversation_id = ?1",
                [conversation_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("Quick AI 消息序号读取失败：{error}"))?;
        let next_sequence = next_sequence.unwrap_or(1);
        let updated = transaction
            .execute(
                "UPDATE quick_ai_conversations
                 SET title = COALESCE(title, ?1), updated_at_unix_ms = ?2
                 WHERE id = ?3",
                params![title, timestamp, conversation_id],
            )
            .map_err(|error| format!("Quick AI 对话更新时间失败：{error}"))?;
        if updated == 0 {
            return Err(format!("Quick AI 对话不存在：id={conversation_id}"));
        }
        insert_message(
            &transaction,
            conversation_id,
            ConversationRole::User,
            user_content,
            next_sequence,
            timestamp,
        )?;
        insert_message(
            &transaction,
            conversation_id,
            ConversationRole::Assistant,
            assistant_content,
            next_sequence + 1,
            timestamp,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("Quick AI 对话事务无法提交：{error}"))?;

        self.get_required(conversation_id)
    }

    pub(crate) fn get(&self, id: i64) -> Result<Option<ConversationSnapshot>, String> {
        let stored = self
            .connection
            .query_row(
                "SELECT id, title, model, created_at_unix_ms, updated_at_unix_ms
                 FROM quick_ai_conversations WHERE id = ?1",
                [id],
                |row| {
                    Ok(StoredConversation {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        model: row.get(2)?,
                        created_at_unix_ms: row.get(3)?,
                        updated_at_unix_ms: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("Quick AI 对话读取失败：{error}"))?;
        let Some(stored) = stored else {
            return Ok(None);
        };

        let mut statement = self
            .connection
            .prepare(
                "SELECT id, conversation_id, role, content, sequence, created_at_unix_ms
                 FROM quick_ai_messages
                 WHERE conversation_id = ?1
                 ORDER BY sequence ASC",
            )
            .map_err(|error| format!("Quick AI 消息读取语句无法准备：{error}"))?;
        let rows = statement
            .query_map([id], |row| {
                Ok(StoredMessage {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    sequence: row.get(4)?,
                    created_at_unix_ms: row.get(5)?,
                })
            })
            .map_err(|error| format!("Quick AI 消息读取失败：{error}"))?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(decode_message(
                row.map_err(|error| format!("Quick AI 消息行读取失败：{error}"))?,
            )?);
        }

        Ok(Some(ConversationSnapshot {
            id: stored.id,
            title: stored.title,
            model: stored.model,
            created_at_unix_ms: stored.created_at_unix_ms,
            updated_at_unix_ms: stored.updated_at_unix_ms,
            messages,
        }))
    }

    pub(crate) fn get_required(&self, id: i64) -> Result<ConversationSnapshot, String> {
        self.get(id)?
            .ok_or_else(|| format!("Quick AI 对话不存在：id={id}"))
    }
}

fn insert_message(
    connection: &Connection,
    conversation_id: i64,
    role: ConversationRole,
    content: &str,
    sequence: i64,
    created_at_unix_ms: i64,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO quick_ai_messages (
                conversation_id, role, content, sequence, created_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                conversation_id,
                role_to_storage(role),
                content.trim(),
                sequence,
                created_at_unix_ms,
            ],
        )
        .map_err(|error| format!("Quick AI 消息写入失败：{error}"))?;

    Ok(())
}

fn decode_message(stored: StoredMessage) -> Result<ConversationMessage, String> {
    Ok(ConversationMessage {
        id: stored.id,
        conversation_id: stored.conversation_id,
        role: role_from_storage(&stored.role)?,
        content: stored.content,
        sequence: stored.sequence,
        created_at_unix_ms: stored.created_at_unix_ms,
    })
}

fn validate_model(model: &str) -> Result<(), String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("Quick AI model 不能为空。".to_string());
    }
    if model.chars().count() > MAX_MODEL_LEN {
        return Err(format!(
            "Quick AI model 长度不能超过 {MAX_MODEL_LEN} 个字符。"
        ));
    }
    Ok(())
}

fn validate_message(label: &str, content: &str) -> Result<(), String> {
    let content = content.trim();
    if content.is_empty() {
        return Err(format!("Quick AI {label}不能为空。"));
    }
    let length = content.chars().count();
    if length > MAX_MESSAGE_LEN {
        return Err(format!(
            "Quick AI {label}长度不能超过 {MAX_MESSAGE_LEN} 个字符，当前为 {length}。"
        ));
    }
    Ok(())
}

fn title_from_first_message(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE_LEN)
        .collect()
}

fn role_to_storage(role: ConversationRole) -> &'static str {
    match role {
        ConversationRole::User => "user",
        ConversationRole::Assistant => "assistant",
    }
}

fn role_from_storage(value: &str) -> Result<ConversationRole, String> {
    match value {
        "user" => Ok(ConversationRole::User),
        "assistant" => Ok(ConversationRole::Assistant),
        _ => Err(format!("Quick AI 消息包含未知 role：{value}")),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_DATABASE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub(crate) fn test_database_path() -> (PathBuf, PathBuf) {
        let suffix = format!(
            "readray-quick-ai-{}-{}",
            std::process::id(),
            TEST_DATABASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(suffix);
        (root.clone(), root.join("readray.sqlite3"))
    }

    #[test]
    fn migration_v2_creates_conversation_tables() {
        let (root, path) = test_database_path();
        let store = ConversationStore::open_path(&path).unwrap();
        let version: i64 = store
            .connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let table_count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('quick_ai_conversations', 'quick_ai_messages')",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version, 2);
        assert_eq!(table_count, 2);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn conversation_and_multiple_exchanges_round_trip() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store
            .create_with_exchange(
                "deepseek-v4-flash",
                "Remember the word amber.",
                "I will remember amber in this conversation.",
            )
            .unwrap();
        let conversation = store
            .append_exchange(
                conversation.id,
                "What word did I ask you to remember?",
                "You asked me to remember amber.",
            )
            .unwrap();
        let loaded = store.get_required(conversation.id).unwrap();

        assert_eq!(loaded.messages.len(), 4);
        assert_eq!(loaded.messages[0].role, ConversationRole::User);
        assert_eq!(loaded.messages[3].role, ConversationRole::Assistant);
        assert_eq!(loaded.messages[3].sequence, 4);
        assert_eq!(loaded.model, "deepseek-v4-flash");
        assert!(loaded.title.as_deref().unwrap().starts_with("Remember"));
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_conversation_can_be_created_and_read() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let created = store.create("deepseek-v4-flash").unwrap();
        let loaded = store.get_required(created.id).unwrap();

        assert!(loaded.messages.is_empty());
        assert!(loaded.title.is_none());
        drop(store);
        let _ = fs::remove_dir_all(root);
    }
}
