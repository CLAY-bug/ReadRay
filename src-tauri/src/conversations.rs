#[cfg(test)]
use crate::learning_records::open_database;
use crate::learning_records::{open_database_for_app, unix_time_ms};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tauri::AppHandle;

const MAX_MODEL_LEN: usize = 160;
const MAX_TITLE_LEN: usize = 80;
const AUTO_TITLE_LEN: usize = 18;
const MAX_MESSAGE_LEN: usize = 32_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationRole {
    User,
    Assistant,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationOrigin {
    Overlay,
    Main,
    Legacy,
}

impl ConversationOrigin {
    fn as_storage(self) -> &'static str {
        match self {
            Self::Overlay => "overlay",
            Self::Main => "main",
            Self::Legacy => "legacy",
        }
    }

    fn from_storage(value: &str) -> Result<Self, String> {
        match value {
            "overlay" => Ok(Self::Overlay),
            "main" => Ok(Self::Main),
            "legacy" => Ok(Self::Legacy),
            _ => Err(format!("Quick AI 对话包含未知创建来源：{value}")),
        }
    }
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
    pub origin: ConversationOrigin,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
    pub messages: Vec<ConversationMessage>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentConversationSummary {
    pub id: i64,
    pub title: String,
    pub origin: ConversationOrigin,
    pub updated_at_unix_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationExportSummary {
    pub conversation_id: i64,
    pub file_name: String,
    pub file_path: String,
    pub message_count: usize,
}

struct StoredConversation {
    id: i64,
    title: Option<String>,
    model: String,
    origin: String,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PreparedTurn {
    Pending {
        snapshot: ConversationSnapshot,
        user_message_id: i64,
    },
    Completed {
        snapshot: ConversationSnapshot,
    },
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

    pub(crate) fn create_with_origin(
        &mut self,
        model: &str,
        origin: ConversationOrigin,
    ) -> Result<ConversationSnapshot, String> {
        validate_model(model)?;
        if origin == ConversationOrigin::Legacy {
            return Err("新建 Quick AI 对话必须标明 Overlay 或主窗口来源。".to_string());
        }
        let timestamp = unix_time_ms()?;
        self.connection
            .execute(
                "INSERT INTO quick_ai_conversations (
                    title, model, origin, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (NULL, ?1, ?2, ?3, ?3)",
                params![model, origin.as_storage(), timestamp],
            )
            .map_err(|error| format!("Quick AI 对话创建失败：{error}"))?;

        self.get_required(self.connection.last_insert_rowid())
    }

    #[cfg(test)]
    pub(crate) fn create(&mut self, model: &str) -> Result<ConversationSnapshot, String> {
        self.create_with_origin(model, ConversationOrigin::Main)
    }

    #[cfg(test)]
    pub(crate) fn create_with_exchange(
        &mut self,
        model: &str,
        user_content: &str,
        assistant_content: &str,
    ) -> Result<ConversationSnapshot, String> {
        let conversation = self.create(model)?;
        let user_message_id = match self.prepare_turn(conversation.id, 1, user_content)? {
            PreparedTurn::Pending {
                user_message_id, ..
            } => user_message_id,
            PreparedTurn::Completed { snapshot } => return Ok(snapshot),
        };
        self.complete_turn(conversation.id, 1, user_message_id, assistant_content)
    }

    #[cfg(test)]
    pub(crate) fn append_exchange(
        &mut self,
        conversation_id: i64,
        user_content: &str,
        assistant_content: &str,
    ) -> Result<ConversationSnapshot, String> {
        let conversation = self.get_required(conversation_id)?;
        let expected_user_sequence = conversation
            .messages
            .last()
            .map(|message| message.sequence + 1)
            .unwrap_or(1);
        let user_message_id =
            match self.prepare_turn(conversation_id, expected_user_sequence, user_content)? {
                PreparedTurn::Pending {
                    user_message_id, ..
                } => user_message_id,
                PreparedTurn::Completed { snapshot } => return Ok(snapshot),
            };
        self.complete_turn(
            conversation_id,
            expected_user_sequence,
            user_message_id,
            assistant_content,
        )
    }

    pub(crate) fn prepare_turn(
        &mut self,
        conversation_id: i64,
        expected_user_sequence: i64,
        user_content: &str,
    ) -> Result<PreparedTurn, String> {
        validate_user_sequence(expected_user_sequence)?;
        validate_message("用户消息", user_content)?;
        let normalized_content = user_content.trim();
        let timestamp = unix_time_ms()?;
        let title = title_from_first_message(normalized_content);
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("Quick AI 待回答消息事务无法开始：{error}"))?;
        let current_max = max_sequence(&transaction, conversation_id)?.unwrap_or(0);

        if let Some(stored_user) =
            stored_message_at(&transaction, conversation_id, expected_user_sequence)?
        {
            if role_from_storage(&stored_user.role)? != ConversationRole::User {
                return Err(format!(
                    "Quick AI 轮次冲突：sequence={expected_user_sequence} 不是用户消息。"
                ));
            }
            if stored_user.content != normalized_content {
                return Err(format!(
                    "Quick AI 轮次冲突：sequence={expected_user_sequence} 已属于另一条用户消息。"
                ));
            }

            let next_message =
                stored_message_at(&transaction, conversation_id, expected_user_sequence + 1)?;
            let user_message_id = stored_user.id;
            if let Some(next_message) = next_message {
                if role_from_storage(&next_message.role)? != ConversationRole::Assistant {
                    return Err(format!(
                        "Quick AI 会话顺序无效：sequence={} 不是助手消息。",
                        expected_user_sequence + 1
                    ));
                }
                drop(transaction);
                return Ok(PreparedTurn::Completed {
                    snapshot: self.get_required(conversation_id)?,
                });
            }
            if current_max != expected_user_sequence {
                return Err(format!(
                    "Quick AI 待回答轮次已过期：期望尾序号为 {expected_user_sequence}，实际为 {current_max}。"
                ));
            }

            drop(transaction);
            return Ok(PreparedTurn::Pending {
                snapshot: self.get_required(conversation_id)?,
                user_message_id,
            });
        }

        let expected_next_sequence = current_max + 1;
        if expected_user_sequence != expected_next_sequence {
            return Err(format!(
                "Quick AI 会话版本冲突：期望写入 user sequence={expected_user_sequence}，当前可写入 sequence={expected_next_sequence}。"
            ));
        }
        if current_max > 0 {
            let last_message = stored_message_at(&transaction, conversation_id, current_max)?
                .ok_or_else(|| "Quick AI 会话尾消息无法读取。".to_string())?;
            if role_from_storage(&last_message.role)? != ConversationRole::Assistant {
                return Err(format!(
                    "Quick AI 会话仍有待回答消息：user sequence={current_max}。"
                ));
            }
        }

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
            normalized_content,
            expected_user_sequence,
            timestamp,
        )?;
        let user_message_id = transaction.last_insert_rowid();
        transaction
            .commit()
            .map_err(|error| format!("Quick AI 待回答消息事务无法提交：{error}"))?;

        Ok(PreparedTurn::Pending {
            snapshot: self.get_required(conversation_id)?,
            user_message_id,
        })
    }

    pub(crate) fn complete_turn(
        &mut self,
        conversation_id: i64,
        expected_user_sequence: i64,
        user_message_id: i64,
        assistant_content: &str,
    ) -> Result<ConversationSnapshot, String> {
        validate_user_sequence(expected_user_sequence)?;
        validate_message("助手消息", assistant_content)?;
        let timestamp = unix_time_ms()?;
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("Quick AI 回答事务无法开始：{error}"))?;
        let stored_user = stored_message_at(
            &transaction,
            conversation_id,
            expected_user_sequence,
        )?
        .ok_or_else(|| {
            format!(
                "Quick AI 待回答消息不存在：conversation_id={conversation_id}, sequence={expected_user_sequence}。"
            )
        })?;
        if stored_user.id != user_message_id
            || role_from_storage(&stored_user.role)? != ConversationRole::User
        {
            return Err("Quick AI 待回答消息身份不匹配。".to_string());
        }

        if let Some(stored_assistant) =
            stored_message_at(&transaction, conversation_id, expected_user_sequence + 1)?
        {
            if role_from_storage(&stored_assistant.role)? != ConversationRole::Assistant {
                return Err("Quick AI 已完成轮次包含无效角色。".to_string());
            }
            drop(transaction);
            return self.get_required(conversation_id);
        }

        let current_max = max_sequence(&transaction, conversation_id)?.unwrap_or(0);
        if current_max != expected_user_sequence {
            return Err(format!(
                "Quick AI 回答版本冲突：待回答 user sequence={expected_user_sequence}，当前尾序号为 {current_max}。"
            ));
        }
        insert_message(
            &transaction,
            conversation_id,
            ConversationRole::Assistant,
            assistant_content,
            expected_user_sequence + 1,
            timestamp,
        )?;
        transaction
            .execute(
                "UPDATE quick_ai_conversations
                 SET updated_at_unix_ms = ?1
                 WHERE id = ?2",
                params![timestamp, conversation_id],
            )
            .map_err(|error| format!("Quick AI 对话更新时间失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("Quick AI 回答事务无法提交：{error}"))?;

        self.get_required(conversation_id)
    }

    pub(crate) fn get(&self, id: i64) -> Result<Option<ConversationSnapshot>, String> {
        let stored = self
            .connection
            .query_row(
                "SELECT id, title, model, origin, created_at_unix_ms, updated_at_unix_ms
                 FROM quick_ai_conversations WHERE id = ?1",
                [id],
                |row| {
                    Ok(StoredConversation {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        model: row.get(2)?,
                        origin: row.get(3)?,
                        created_at_unix_ms: row.get(4)?,
                        updated_at_unix_ms: row.get(5)?,
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
            origin: ConversationOrigin::from_storage(&stored.origin)?,
            created_at_unix_ms: stored.created_at_unix_ms,
            updated_at_unix_ms: stored.updated_at_unix_ms,
            messages,
        }))
    }

    pub(crate) fn get_required(&self, id: i64) -> Result<ConversationSnapshot, String> {
        self.get(id)?
            .ok_or_else(|| format!("Quick AI 对话不存在：id={id}"))
    }

    pub(crate) fn list_recent(
        &self,
        limit: u32,
        origin: Option<ConversationOrigin>,
    ) -> Result<Vec<RecentConversationSummary>, String> {
        let origin_filter = origin.map(ConversationOrigin::as_storage);
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, title, origin, updated_at_unix_ms
                 FROM quick_ai_conversations
                 WHERE title IS NOT NULL AND length(trim(title)) > 0
                   AND (?1 IS NULL OR origin = ?1)
                 ORDER BY updated_at_unix_ms DESC, id DESC
                 LIMIT ?2",
            )
            .map_err(|error| format!("最近 Quick AI 对话语句无法准备：{error}"))?;
        let rows = statement
            .query_map(params![origin_filter, i64::from(limit)], |row| {
                let origin: String = row.get(2)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    origin,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|error| format!("最近 Quick AI 对话读取失败：{error}"))?;
        let mut conversations = Vec::new();
        for row in rows {
            let (id, title, origin, updated_at_unix_ms) =
                row.map_err(|error| format!("最近 Quick AI 对话行读取失败：{error}"))?;
            conversations.push(RecentConversationSummary {
                id,
                title,
                origin: ConversationOrigin::from_storage(&origin)?,
                updated_at_unix_ms,
            });
        }

        Ok(conversations)
    }

    pub(crate) fn list_all(
        &self,
        origin: Option<ConversationOrigin>,
    ) -> Result<Vec<RecentConversationSummary>, String> {
        let origin_filter = origin.map(ConversationOrigin::as_storage);
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, title, origin, updated_at_unix_ms
                 FROM quick_ai_conversations
                 WHERE title IS NOT NULL AND length(trim(title)) > 0
                   AND (?1 IS NULL OR origin = ?1)
                 ORDER BY updated_at_unix_ms DESC, id DESC",
            )
            .map_err(|error| format!("全部 Quick AI 对话语句无法准备：{error}"))?;
        let rows = statement
            .query_map([origin_filter], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|error| format!("全部 Quick AI 对话读取失败：{error}"))?;
        let mut conversations = Vec::new();
        for row in rows {
            let (id, title, origin, updated_at_unix_ms) =
                row.map_err(|error| format!("全部 Quick AI 对话行读取失败：{error}"))?;
            conversations.push(RecentConversationSummary {
                id,
                title,
                origin: ConversationOrigin::from_storage(&origin)?,
                updated_at_unix_ms,
            });
        }
        Ok(conversations)
    }

    pub(crate) fn rename(
        &mut self,
        conversation_id: i64,
        title: &str,
    ) -> Result<ConversationSnapshot, String> {
        let title = validate_title(title)?;
        let timestamp = unix_time_ms()?;
        let message_count: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM quick_ai_messages WHERE conversation_id = ?1",
                [conversation_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("Quick AI 会话消息数量读取失败：{error}"))?;
        if message_count == 0 {
            return Err("空白 Quick AI 会话无需重命名。".to_string());
        }
        let updated = self
            .connection
            .execute(
                "UPDATE quick_ai_conversations
                 SET title = ?1, updated_at_unix_ms = ?2
                 WHERE id = ?3",
                params![title, timestamp, conversation_id],
            )
            .map_err(|error| format!("Quick AI 会话重命名失败：{error}"))?;
        if updated == 0 {
            return Err(format!("Quick AI 对话不存在：id={conversation_id}"));
        }
        self.get_required(conversation_id)
    }

    pub(crate) fn delete(&mut self, conversation_id: i64) -> Result<bool, String> {
        self.connection
            .execute(
                "DELETE FROM quick_ai_conversations WHERE id = ?1",
                [conversation_id],
            )
            .map(|deleted| deleted > 0)
            .map_err(|error| format!("Quick AI 会话删除失败：{error}"))
    }
}

pub(crate) fn export_snapshot_to_path(
    snapshot: &ConversationSnapshot,
    path: &Path,
) -> Result<ConversationExportSummary, String> {
    if snapshot.messages.is_empty() {
        return Err("空白 Quick AI 会话没有可导出的消息。".to_string());
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Quick AI 导出路径缺少有效文件名。".to_string())?;
    let content = render_export_markdown(snapshot);
    fs::write(path, content.as_bytes())
        .map_err(|error| format!("Quick AI 对话文件写入失败：{error}"))?;
    Ok(ConversationExportSummary {
        conversation_id: snapshot.id,
        file_name: file_name.to_string(),
        file_path: path.to_string_lossy().into_owned(),
        message_count: snapshot.messages.len(),
    })
}

fn render_export_markdown(snapshot: &ConversationSnapshot) -> String {
    let title = snapshot.title.as_deref().unwrap_or("ReadRay 对话").trim();
    let mut output = format!(
        "# {}\n\n",
        if title.is_empty() {
            "ReadRay 对话"
        } else {
            title
        }
    );
    for message in &snapshot.messages {
        let role = match message.role {
            ConversationRole::User => "用户",
            ConversationRole::Assistant => "ReadRay",
        };
        output.push_str("## ");
        output.push_str(role);
        output.push_str("\n\n");
        output.push_str(&message.content);
        output.push_str("\n\n");
    }
    output
}

fn max_sequence(connection: &Connection, conversation_id: i64) -> Result<Option<i64>, String> {
    connection
        .query_row(
            "SELECT MAX(sequence) FROM quick_ai_messages WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("Quick AI 消息尾序号读取失败：{error}"))
}

fn stored_message_at(
    connection: &Connection,
    conversation_id: i64,
    sequence: i64,
) -> Result<Option<StoredMessage>, String> {
    connection
        .query_row(
            "SELECT id, conversation_id, role, content, sequence, created_at_unix_ms
             FROM quick_ai_messages
             WHERE conversation_id = ?1 AND sequence = ?2",
            params![conversation_id, sequence],
            |row| {
                Ok(StoredMessage {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    sequence: row.get(4)?,
                    created_at_unix_ms: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("Quick AI 指定序号消息读取失败：{error}"))
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

fn validate_title(title: &str) -> Result<&str, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Quick AI 会话名称不能为空。".to_string());
    }
    let length = title.chars().count();
    if length > MAX_TITLE_LEN {
        return Err(format!(
            "Quick AI 会话名称长度不能超过 {MAX_TITLE_LEN} 个字符，当前为 {length}。"
        ));
    }
    Ok(title)
}

fn validate_user_sequence(sequence: i64) -> Result<(), String> {
    if sequence <= 0 || sequence % 2 == 0 {
        return Err(format!(
            "Quick AI user sequence 必须是正奇数，当前为 {sequence}。"
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
        .take(AUTO_TITLE_LEN)
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

        assert!(version >= 2);
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
    fn automatic_title_uses_a_fixed_prefix_without_ellipsis() {
        let title = title_from_first_message(&format!("{} 后续内容", "前".repeat(30)));

        assert_eq!(title, "前".repeat(AUTO_TITLE_LEN));
        assert!(!title.contains('…'));
    }

    #[test]
    fn saved_conversation_survives_store_reopen() {
        let (root, path) = test_database_path();
        let conversation_id = {
            let mut store = ConversationStore::open_path(&path).unwrap();
            store
                .create_with_exchange(
                    "deepseek-v4-flash",
                    "Persist this question.",
                    "This answer is persisted.",
                )
                .unwrap()
                .id
        };

        let reopened = ConversationStore::open_path(&path).unwrap();
        let loaded = reopened.get_required(conversation_id).unwrap();

        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].content, "Persist this question.");
        assert_eq!(loaded.messages[1].content, "This answer is persisted.");
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_failure_pending_user_survives_store_reopen() {
        let (root, path) = test_database_path();
        let (conversation_id, user_message_id) = {
            let mut store = ConversationStore::open_path(&path).unwrap();
            let conversation = store.create("deepseek-v4-flash").unwrap();
            let prepared = store
                .prepare_turn(conversation.id, 1, "Persist before model request")
                .unwrap();
            let PreparedTurn::Pending {
                snapshot,
                user_message_id,
            } = prepared
            else {
                panic!("first prepare must create a pending user");
            };
            assert_eq!(snapshot.messages.len(), 1);
            (conversation.id, user_message_id)
        };

        let reopened = ConversationStore::open_path(&path).unwrap();
        let loaded = reopened.get_required(conversation_id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].id, user_message_id);
        assert_eq!(loaded.messages[0].role, ConversationRole::User);
        assert_eq!(loaded.messages[0].content, "Persist before model request");
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_retry_reuses_pending_user_and_adds_exactly_one_assistant() {
        let (root, path) = test_database_path();
        let (conversation_id, original_user_id) = {
            let mut store = ConversationStore::open_path(&path).unwrap();
            let conversation = store.create("deepseek-v4-flash").unwrap();
            let PreparedTurn::Pending {
                user_message_id, ..
            } = store
                .prepare_turn(conversation.id, 1, "Retry after restart")
                .unwrap()
            else {
                panic!("first prepare must be pending");
            };
            (conversation.id, user_message_id)
        };

        let mut reopened = ConversationStore::open_path(&path).unwrap();
        let PreparedTurn::Pending {
            user_message_id, ..
        } = reopened
            .prepare_turn(conversation_id, 1, "Retry after restart")
            .unwrap()
        else {
            panic!("retry must reuse the pending user");
        };
        assert_eq!(user_message_id, original_user_id);
        let completed = reopened
            .complete_turn(
                conversation_id,
                1,
                user_message_id,
                "Completed after restart",
            )
            .unwrap();

        assert_eq!(completed.messages.len(), 2);
        assert_eq!(
            completed
                .messages
                .iter()
                .filter(|message| message.role == ConversationRole::User)
                .count(),
            1
        );
        assert_eq!(
            completed
                .messages
                .iter()
                .filter(|message| message.role == ConversationRole::Assistant)
                .count(),
            1
        );
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completed_turn_retry_returns_authoritative_snapshot_without_duplicates() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store.create("deepseek-v4-flash").unwrap();
        let PreparedTurn::Pending {
            user_message_id, ..
        } = store
            .prepare_turn(conversation.id, 1, "Caller may miss the response")
            .unwrap()
        else {
            panic!("first prepare must be pending");
        };
        let completed = store
            .complete_turn(conversation.id, 1, user_message_id, "Committed answer")
            .unwrap();
        assert_eq!(completed.messages.len(), 2);

        let PreparedTurn::Completed { snapshot } = store
            .prepare_turn(conversation.id, 1, "Caller may miss the response")
            .unwrap()
        else {
            panic!("retry must detect the completed sequence slot");
        };
        let completed_again = store
            .complete_turn(
                conversation.id,
                1,
                user_message_id,
                "Ignored duplicate answer",
            )
            .unwrap();

        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(completed_again.messages.len(), 2);
        assert_eq!(completed_again.messages[1].content, "Committed answer");
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_or_concurrent_turn_cannot_write_against_old_history() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store
            .create_with_exchange("deepseek-v4-flash", "First turn", "First answer")
            .unwrap();
        let PreparedTurn::Pending {
            user_message_id, ..
        } = store
            .prepare_turn(conversation.id, 3, "Concurrent request A")
            .unwrap()
        else {
            panic!("first concurrent request must reserve sequence 3");
        };

        let content_conflict = store
            .prepare_turn(conversation.id, 3, "Concurrent request B")
            .unwrap_err();
        assert!(content_conflict.contains("轮次冲突"));
        let stale_version = store
            .prepare_turn(conversation.id, 5, "Skip pending answer")
            .unwrap_err();
        assert!(stale_version.contains("版本冲突") || stale_version.contains("待回答消息"));
        let wrong_identity = store
            .complete_turn(
                conversation.id,
                3,
                user_message_id + 1000,
                "Must not be saved",
            )
            .unwrap_err();
        assert!(wrong_identity.contains("身份不匹配"));
        let loaded = store.get_required(conversation.id).unwrap();
        assert_eq!(loaded.messages.len(), 3);
        assert_eq!(loaded.messages[2].content, "Concurrent request A");
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn assistant_save_failure_leaves_one_retriable_pending_user() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store
            .create_with_exchange("deepseek-v4-flash", "First turn", "First answer")
            .unwrap();
        let PreparedTurn::Pending {
            user_message_id, ..
        } = store
            .prepare_turn(conversation.id, 3, "Retry this exact input")
            .unwrap()
        else {
            panic!("turn must be pending before the assistant save");
        };
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_quick_ai_assistant_insert
                 BEFORE INSERT ON quick_ai_messages
                 WHEN NEW.role = 'assistant'
                 BEGIN
                   SELECT RAISE(ABORT, 'simulated assistant save failure');
                 END;",
            )
            .unwrap();

        let error = store
            .complete_turn(conversation.id, 3, user_message_id, "This save should fail")
            .unwrap_err();
        assert!(error.contains("simulated assistant save failure"));
        let after_failure = store.get_required(conversation.id).unwrap();
        assert_eq!(after_failure.messages.len(), 3);
        assert_eq!(after_failure.messages[2].id, user_message_id);

        store
            .connection
            .execute_batch("DROP TRIGGER fail_quick_ai_assistant_insert;")
            .unwrap();
        let PreparedTurn::Pending {
            user_message_id: retried_user_id,
            ..
        } = store
            .prepare_turn(conversation.id, 3, "Retry this exact input")
            .unwrap()
        else {
            panic!("save failure must remain pending");
        };
        assert_eq!(retried_user_id, user_message_id);
        let after_retry = store
            .complete_turn(conversation.id, 3, retried_user_id, "This save succeeds")
            .unwrap();
        assert_eq!(after_retry.messages.len(), 4);
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

    #[test]
    fn recent_conversations_have_real_titles_and_exclude_empty_conversations() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        store.create("deepseek-v4-flash").unwrap();
        let first = store
            .create_with_exchange("deepseek-v4-flash", "First topic", "First answer")
            .unwrap();
        let second = store
            .create_with_exchange("deepseek-v4-flash", "Second topic", "Second answer")
            .unwrap();

        let recent = store.list_recent(1, None).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, second.id);
        assert_eq!(recent[0].title, "Second topic");
        let all = store.list_recent(10, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].id, first.id);

        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn conversation_origin_is_immutable_and_filters_overlay_history() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let main = store
            .create_with_exchange("deepseek-v4-flash", "Main topic", "Main answer")
            .unwrap();
        let overlay = store
            .create_with_origin("deepseek-v4-flash", ConversationOrigin::Overlay)
            .unwrap();
        let PreparedTurn::Pending {
            user_message_id, ..
        } = store.prepare_turn(overlay.id, 1, "Overlay topic").unwrap()
        else {
            panic!("overlay first turn must be pending");
        };
        let overlay = store
            .complete_turn(overlay.id, 1, user_message_id, "Overlay answer")
            .unwrap();

        let overlay_recent = store
            .list_recent(8, Some(ConversationOrigin::Overlay))
            .unwrap();
        assert_eq!(overlay_recent.len(), 1);
        assert_eq!(overlay_recent[0].id, overlay.id);
        assert_eq!(overlay_recent[0].origin, ConversationOrigin::Overlay);
        assert_eq!(
            store.get_required(main.id).unwrap().origin,
            ConversationOrigin::Main
        );
        assert_eq!(store.list_all(None).unwrap().len(), 2);
        assert!(store
            .create_with_origin("deepseek-v4-flash", ConversationOrigin::Legacy)
            .unwrap_err()
            .contains("必须标明"));

        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn all_conversations_rename_and_delete_use_database_identity() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let first = store
            .create_with_exchange("deepseek-v4-flash", "First topic", "First answer")
            .unwrap();
        let second = store
            .create_with_exchange("deepseek-v4-flash", "Second topic", "Second answer")
            .unwrap();
        store.create("deepseek-v4-flash").unwrap();

        let all = store.list_all(None).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|item| item.id == first.id));
        assert!(all.iter().any(|item| item.id == second.id));

        let renamed = store.rename(first.id, "  Renamed topic  ").unwrap();
        assert_eq!(renamed.id, first.id);
        assert_eq!(renamed.title.as_deref(), Some("Renamed topic"));
        assert_eq!(renamed.messages, first.messages);

        let rename_error = store.rename(second.id, "   ").unwrap_err();
        assert!(rename_error.contains("不能为空"));
        assert_eq!(
            store.get_required(second.id).unwrap().title,
            second.title,
            "failed rename must not alter the target conversation"
        );

        assert!(store.delete(first.id).unwrap());
        assert!(store.get(first.id).unwrap().is_none());
        let remaining_messages: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM quick_ai_messages WHERE conversation_id = ?1",
                [first.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining_messages, 0);
        assert!(!store.delete(first.id).unwrap());
        assert_eq!(store.get_required(second.id).unwrap(), second);

        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn export_writes_every_message_in_database_sequence_order() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store
            .create_with_exchange("deepseek-v4-flash", "First user", "First assistant")
            .unwrap();
        let conversation = store
            .append_exchange(conversation.id, "Second user", "Second assistant")
            .unwrap();
        let export_path = root.join("ordered-conversation.md");

        let exported = export_snapshot_to_path(&conversation, &export_path).unwrap();
        let content = fs::read_to_string(&export_path).unwrap();
        let first_user = content.find("First user").unwrap();
        let first_assistant = content.find("First assistant").unwrap();
        let second_user = content.find("Second user").unwrap();
        let second_assistant = content.find("Second assistant").unwrap();

        assert_eq!(exported.conversation_id, conversation.id);
        assert_eq!(exported.file_name, "ordered-conversation.md");
        assert_eq!(exported.message_count, 4);
        assert!(content.starts_with("# First user\n\n"));
        assert!(first_user < first_assistant);
        assert!(first_assistant < second_user);
        assert!(second_user < second_assistant);
        assert_eq!(content.matches("## 用户").count(), 2);
        assert_eq!(content.matches("## ReadRay").count(), 2);

        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_or_unwritable_export_does_not_report_success() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let empty = store.create("deepseek-v4-flash").unwrap();
        let empty_path = root.join("empty.md");
        let empty_error = export_snapshot_to_path(&empty, &empty_path).unwrap_err();
        assert!(empty_error.contains("没有可导出"));
        assert!(!empty_path.exists());

        let conversation = store
            .create_with_exchange("deepseek-v4-flash", "Export me", "Stored answer")
            .unwrap();
        let blocked_path = root.join("missing-parent").join("export.md");
        let write_error = export_snapshot_to_path(&conversation, &blocked_path).unwrap_err();
        assert!(write_error.contains("写入失败"));
        assert!(store.get_required(conversation.id).is_ok());

        drop(store);
        let _ = fs::remove_dir_all(root);
    }
}
