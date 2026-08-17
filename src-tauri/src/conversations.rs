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
    /// 回答引用的外部来源（任务 4）：随 assistant 消息落库的 JSON 数组，重启与
    /// 历史回看直接来自本行，不从 run/step 审计表重建。
    pub sources: Option<Vec<crate::agent_runtime::protocol::SourceMetadata>>,
    /// finish_reason=length 的诚实截断标志（任务 4）：回答照常持久化，UI 只给
    /// "回答可能不完整"的轻微提示，不做"继续生成"。
    pub truncated: bool,
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
    sources_json: Option<String>,
    truncated: i64,
    superseded_by_id: Option<i64>,
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

/// 重新生成准备结果（任务 4）：Ready 可执行新 run；AlreadyCurrent 表示目标回答
/// 已被更新的重新生成替代（幂等，不新建 run，直接返回当前权威快照）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RegenerationTurn {
    Ready {
        snapshot: ConversationSnapshot,
        user_message_id: i64,
        target_message_id: i64,
    },
    AlreadyCurrent {
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
        self.complete_turn(
            conversation.id,
            1,
            user_message_id,
            assistant_content,
            None,
            false,
        )
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
            None,
            false,
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
        // 尾消息是待回答 user（pending，上一轮失败/中断）时，允许以
        // current_max+2 开启新轮次：旧 pending 保留在库中可重试，新消息
        // 进入独立的新轮次（前端失败态直接输入新消息）。重试仍走上方
        // stored_user 分支复用同一 sequence，幂等语义不变。
        let tail_is_pending_user = current_max > 0
            && stored_message_at(&transaction, conversation_id, current_max)?
                .map(|message| role_from_storage(&message.role) == Ok(ConversationRole::User))
                .unwrap_or(false);
        let allowed_successor = expected_user_sequence == expected_next_sequence
            || (tail_is_pending_user && expected_user_sequence == current_max + 2);
        if !allowed_successor {
            return Err(format!(
                "Quick AI 会话版本冲突：期望写入 user sequence={expected_user_sequence}，当前可写入 sequence={expected_next_sequence}。"
            ));
        }
        if current_max > 0 {
            let last_message = stored_message_at(&transaction, conversation_id, current_max)?
                .ok_or_else(|| "Quick AI 会话尾消息无法读取。".to_string())?;
            if role_from_storage(&last_message.role)? != ConversationRole::Assistant
                && expected_user_sequence != current_max + 2
            {
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
            None,
            false,
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
        sources_json: Option<String>,
        truncated: bool,
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
            sources_json,
            truncated,
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

    /// 重新生成准备（任务 4）：校验该轮 user 与目标 assistant 后返回 Ready；
    /// 目标已被更新的重新生成替代时返回 AlreadyCurrent（幂等，直接返回权威快照）。
    /// 只允许重新生成会话中最后一条（未被替代的）assistant。
    pub(crate) fn prepare_regeneration(
        &mut self,
        conversation_id: i64,
        expected_user_sequence: i64,
        user_content: &str,
        target_message_id: i64,
    ) -> Result<RegenerationTurn, String> {
        validate_user_sequence(expected_user_sequence)?;
        let normalized_content = user_content.trim();
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("Quick AI 重新生成事务无法开始：{error}"))?;
        let stored_user = stored_message_at(&transaction, conversation_id, expected_user_sequence)?
            .ok_or_else(|| {
                format!(
                    "Quick AI 待重新生成的用户消息不存在：conversation_id={conversation_id}, sequence={expected_user_sequence}。"
                )
            })?;
        if role_from_storage(&stored_user.role)? != ConversationRole::User {
            return Err("Quick AI 重新生成目标不是用户消息。".to_string());
        }
        if stored_user.content != normalized_content {
            return Err("Quick AI 重新生成的用户消息内容不匹配。".to_string());
        }

        let target = stored_message_by_id(&transaction, conversation_id, target_message_id)?
            .ok_or_else(|| format!("Quick AI 重新生成目标回答不存在：id={target_message_id}。"))?;
        if role_from_storage(&target.role)? != ConversationRole::Assistant {
            return Err("Quick AI 重新生成目标不是助手消息。".to_string());
        }
        if target.sequence <= expected_user_sequence {
            return Err("Quick AI 重新生成目标不属于该轮回答。".to_string());
        }
        // 已被替代的目标优先幂等返回（重复请求/迟到重试不报"非尾"错误）；
        // 未替代但非尾的目标才是真正的非法请求。
        if target.superseded_by_id.is_some() {
            drop(transaction);
            return Ok(RegenerationTurn::AlreadyCurrent {
                snapshot: self.get_required(conversation_id)?,
            });
        }
        let current_max = max_sequence(&transaction, conversation_id)?.unwrap_or(0);
        if target.sequence != current_max {
            return Err("Quick AI 只能重新生成会话中最后一条回答。".to_string());
        }
        drop(transaction);
        Ok(RegenerationTurn::Ready {
            snapshot: self.get_required(conversation_id)?,
            user_message_id: stored_user.id,
            target_message_id,
        })
    }

    /// 重新生成完成（任务 4）：插入新 assistant（sequence 取当前最大序号的下一个
    /// 偶数，保持 user 奇 / assistant 偶交替），再把旧 assistant 标记为被替代；
    /// 旧行不物理删除（可审计），可见快照与导出只取未被替代的当前回答。
    pub(crate) fn complete_regeneration(
        &mut self,
        conversation_id: i64,
        expected_user_sequence: i64,
        user_message_id: i64,
        target_message_id: i64,
        assistant_content: &str,
        sources_json: Option<String>,
        truncated: bool,
    ) -> Result<ConversationSnapshot, String> {
        validate_user_sequence(expected_user_sequence)?;
        validate_message("助手消息", assistant_content)?;
        let timestamp = unix_time_ms()?;
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("Quick AI 重新生成回答事务无法开始：{error}"))?;
        let stored_user = stored_message_at(
            &transaction,
            conversation_id,
            expected_user_sequence,
        )?
        .ok_or_else(|| {
            format!(
                "Quick AI 待重新生成的用户消息不存在：conversation_id={conversation_id}, sequence={expected_user_sequence}。"
            )
        })?;
        if stored_user.id != user_message_id
            || role_from_storage(&stored_user.role)? != ConversationRole::User
        {
            return Err("Quick AI 待重新生成用户消息身份不匹配。".to_string());
        }
        let target = stored_message_by_id(&transaction, conversation_id, target_message_id)?
            .ok_or_else(|| format!("Quick AI 重新生成目标回答不存在：id={target_message_id}。"))?;
        if role_from_storage(&target.role)? != ConversationRole::Assistant
            || target.sequence <= expected_user_sequence
        {
            return Err("Quick AI 重新生成目标不是该轮助手消息。".to_string());
        }
        if target.superseded_by_id.is_some() {
            return Err("Quick AI 重新生成目标已被更新的回答替代。".to_string());
        }
        let current_max = max_sequence(&transaction, conversation_id)?.unwrap_or(0);
        if target.sequence != current_max {
            return Err("Quick AI 只能重新生成会话中最后一条回答。".to_string());
        }
        let new_sequence = current_max + 1 + i64::from(current_max % 2 == 0);
        insert_message(
            &transaction,
            conversation_id,
            ConversationRole::Assistant,
            assistant_content,
            new_sequence,
            timestamp,
            sources_json,
            truncated,
        )?;
        let new_message_id = transaction.last_insert_rowid();
        let superseded = transaction
            .execute(
                "UPDATE quick_ai_messages
                 SET superseded_by_id = ?1
                 WHERE id = ?2 AND superseded_by_id IS NULL",
                params![new_message_id, target_message_id],
            )
            .map_err(|error| format!("Quick AI 重新生成替代标记失败：{error}"))?;
        if superseded != 1 {
            return Err("Quick AI 重新生成目标已被更新的回答替代。".to_string());
        }
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
            .map_err(|error| format!("Quick AI 重新生成回答事务无法提交：{error}"))?;

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
                "SELECT id, conversation_id, role, content, sequence, created_at_unix_ms,
                        sources_json, truncated
                 FROM quick_ai_messages
                 WHERE conversation_id = ?1 AND superseded_by_id IS NULL
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
                    sources_json: row.get(6)?,
                    truncated: row.get(7)?,
                    superseded_by_id: None,
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
            "SELECT id, conversation_id, role, content, sequence, created_at_unix_ms,
                    sources_json, truncated, superseded_by_id
             FROM quick_ai_messages
             WHERE conversation_id = ?1 AND sequence = ?2",
            params![conversation_id, sequence],
            read_stored_message,
        )
        .optional()
        .map_err(|error| format!("Quick AI 指定序号消息读取失败：{error}"))
}

fn stored_message_by_id(
    connection: &Connection,
    conversation_id: i64,
    message_id: i64,
) -> Result<Option<StoredMessage>, String> {
    connection
        .query_row(
            "SELECT id, conversation_id, role, content, sequence, created_at_unix_ms,
                    sources_json, truncated, superseded_by_id
             FROM quick_ai_messages
             WHERE conversation_id = ?1 AND id = ?2",
            params![conversation_id, message_id],
            read_stored_message,
        )
        .optional()
        .map_err(|error| format!("Quick AI 指定消息读取失败：{error}"))
}

fn read_stored_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredMessage> {
    Ok(StoredMessage {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        sequence: row.get(4)?,
        created_at_unix_ms: row.get(5)?,
        sources_json: row.get(6)?,
        truncated: row.get(7)?,
        superseded_by_id: row.get(8)?,
    })
}

fn insert_message(
    connection: &Connection,
    conversation_id: i64,
    role: ConversationRole,
    content: &str,
    sequence: i64,
    created_at_unix_ms: i64,
    sources_json: Option<String>,
    truncated: bool,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO quick_ai_messages (
                conversation_id, role, content, sequence, created_at_unix_ms,
                sources_json, truncated
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                conversation_id,
                role_to_storage(role),
                content.trim(),
                sequence,
                created_at_unix_ms,
                sources_json,
                i64::from(truncated),
            ],
        )
        .map_err(|error| format!("Quick AI 消息写入失败：{error}"))?;

    Ok(())
}

fn decode_message(stored: StoredMessage) -> Result<ConversationMessage, String> {
    let sources = match stored.sources_json.as_deref() {
        None | Some("") => None,
        Some(raw) => {
            match serde_json::from_str::<Vec<crate::agent_runtime::protocol::SourceMetadata>>(raw) {
                Ok(sources) => Some(sources),
                Err(error) => {
                    // 来源是展示元数据：解析失败只记录日志并降级为无来源，
                    // 不因单个损坏 blob 阻断整个会话加载。
                    eprintln!("READRAY_AGENT_SOURCES_PARSE_FAILED={error}");
                    None
                }
            }
        }
    };
    Ok(ConversationMessage {
        id: stored.id,
        conversation_id: stored.conversation_id,
        role: role_from_storage(&stored.role)?,
        content: stored.content,
        sequence: stored.sequence,
        created_at_unix_ms: stored.created_at_unix_ms,
        sources,
        truncated: stored.truncated != 0,
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
                None,
                false,
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
    fn failed_pending_user_can_be_retried_then_superseded_by_a_new_turn() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store.create("deepseek-v4-flash").unwrap();

        // 第一轮失败：pending user（seq 1）落库，无 assistant。
        let PreparedTurn::Pending {
            user_message_id: first_id,
            ..
        } = store
            .prepare_turn(conversation.id, 1, "First failed question")
            .unwrap()
        else {
            panic!("first prepare must be pending");
        };

        // 失败态直接重试（同一轮次）：复用同一 user_message_id，语义不变。
        let PreparedTurn::Pending {
            user_message_id: retried_id,
            ..
        } = store
            .prepare_turn(conversation.id, 1, "First failed question")
            .unwrap()
        else {
            panic!("retry must reuse the pending user");
        };
        assert_eq!(retried_id, first_id);

        // 失败态直接输入新消息：新轮次使用 seq 3（跳过待完成的 assistant 位置 seq 2），
        // 旧 pending 保留在库中（审计可追溯），不删除。
        let PreparedTurn::Pending {
            user_message_id: second_id,
            ..
        } = store
            .prepare_turn(conversation.id, 3, "Second new question")
            .unwrap()
        else {
            panic!("new turn must be pending");
        };
        assert_ne!(second_id, first_id);

        // 新轮次的 pending 也可重试（seq 3 复用同一 user_message_id）。
        let PreparedTurn::Pending {
            user_message_id: second_retried,
            ..
        } = store
            .prepare_turn(conversation.id, 3, "Second new question")
            .unwrap()
        else {
            panic!("new turn retry must reuse pending");
        };
        assert_eq!(second_retried, second_id);

        // 冲突序号仍被拒绝：偶数与跳过尾部的任意序号都不允许。
        assert!(store
            .prepare_turn(conversation.id, 2, "Even sequence")
            .is_err());
        assert!(store.prepare_turn(conversation.id, 4, "Skip two").is_err());

        // 旧 pending 与新 pending 都保留（两条 user 消息）。
        let snapshot = store.get_required(conversation.id).unwrap();
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[0].content, "First failed question");
        assert_eq!(snapshot.messages[1].content, "Second new question");

        // 完成新轮次后恰好补一条 assistant，旧失败轮次不被改动。
        let completed = store
            .complete_turn(conversation.id, 3, second_id, "Second answer", None, false)
            .unwrap();
        assert_eq!(completed.messages.len(), 3);
        assert_eq!(completed.messages[1].content, "Second new question");
        assert_eq!(completed.messages[2].content, "Second answer");
        drop(store);
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
            .complete_turn(
                conversation.id,
                1,
                user_message_id,
                "Committed answer",
                None,
                false,
            )
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
                None,
                false,
            )
            .unwrap();

        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(completed_again.messages.len(), 2);
        assert_eq!(completed_again.messages[1].content, "Committed answer");
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_or_concurrent_turn_cannot_overwrite_existing_history() {
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
        // pending 尾之后以 current_max+2 开启新轮次（失败/中断后用户直接输入
        // 新消息的语义）：旧 pending 保留、新轮次取代推进。
        let PreparedTurn::Pending { .. } = store
            .prepare_turn(conversation.id, 5, "Superseding new turn")
            .unwrap()
        else {
            panic!("pending 尾后的新轮次必须成功");
        };
        // 推进后，向旧 sequence 写入不同内容仍被拒绝（stale 保护）。
        let stale = store
            .prepare_turn(conversation.id, 3, "Old request after advance")
            .unwrap_err();
        assert!(stale.contains("轮次冲突"));
        let wrong_identity = store
            .complete_turn(
                conversation.id,
                3,
                user_message_id + 1000,
                "Must not be saved",
                None,
                false,
            )
            .unwrap_err();
        assert!(wrong_identity.contains("身份不匹配"));
        let loaded = store.get_required(conversation.id).unwrap();
        assert_eq!(loaded.messages.len(), 4);
        assert_eq!(loaded.messages[2].content, "Concurrent request A");
        assert_eq!(loaded.messages[3].content, "Superseding new turn");
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
            .complete_turn(
                conversation.id,
                3,
                user_message_id,
                "This save should fail",
                None,
                false,
            )
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
            .complete_turn(
                conversation.id,
                3,
                retried_user_id,
                "This save succeeds",
                None,
                false,
            )
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
            .complete_turn(
                overlay.id,
                1,
                user_message_id,
                "Overlay answer",
                None,
                false,
            )
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

    fn sample_sources_json() -> String {
        serde_json::json!([{
            "sourceId": "source-1",
            "title": "Example",
            "url": "https://example.com/article",
            "siteName": "Example",
            "publishedAt": null,
            "retrievedAtUnixMs": 100,
            "contentType": "text/html"
        }])
        .to_string()
    }

    #[test]
    fn complete_turn_persists_sources_and_truncated_flag_with_the_message() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store.create("deepseek-v4-flash").unwrap();
        let PreparedTurn::Pending {
            user_message_id, ..
        } = store
            .prepare_turn(conversation.id, 1, "带来源的问题")
            .unwrap()
        else {
            panic!("first prepare must be pending");
        };
        let completed = store
            .complete_turn(
                conversation.id,
                1,
                user_message_id,
                "带来源的回答",
                Some(sample_sources_json()),
                true,
            )
            .unwrap();
        let assistant = completed.messages.last().unwrap();
        assert_eq!(assistant.role, ConversationRole::Assistant);
        assert_eq!(assistant.truncated, true);
        let sources = assistant.sources.as_ref().expect("来源必须随消息返回");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source_id, "source-1");
        assert_eq!(sources[0].url, "https://example.com/article");
        assert_eq!(sources[0].site_name.as_deref(), Some("Example"));

        // 重启（重新打开数据库）后来源与截断标志仍可回看。
        drop(store);
        let reopened = ConversationStore::open_path(&path).unwrap();
        let loaded = reopened.get_required(conversation.id).unwrap();
        let assistant = loaded.messages.last().unwrap();
        assert_eq!(assistant.truncated, true);
        assert_eq!(assistant.sources.as_ref().unwrap()[0].source_id, "source-1");
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn regeneration_replaces_visible_answer_and_keeps_old_row_for_audit() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store
            .create_with_exchange("deepseek-v4-flash", "原问题", "旧回答")
            .unwrap();
        let old_assistant = conversation.messages[1].id;

        let RegenerationTurn::Ready {
            user_message_id,
            target_message_id,
            ..
        } = store
            .prepare_regeneration(conversation.id, 1, "原问题", old_assistant)
            .unwrap()
        else {
            panic!("首次重新生成必须 Ready");
        };
        assert_eq!(target_message_id, old_assistant);

        let regenerated = store
            .complete_regeneration(
                conversation.id,
                1,
                user_message_id,
                old_assistant,
                "新回答",
                Some(sample_sources_json()),
                false,
            )
            .unwrap();
        // 可见快照只保留当前回答：旧回答被过滤，新回答在新 sequence（下一个偶数）。
        assert_eq!(regenerated.messages.len(), 2);
        assert_eq!(regenerated.messages[0].content, "原问题");
        assert_eq!(regenerated.messages[1].content, "新回答");
        assert_eq!(regenerated.messages[1].sequence, 4);
        assert_eq!(
            regenerated.messages[1].sources.as_ref().unwrap()[0].source_id,
            "source-1"
        );

        // 旧行物理保留、标记被替代（可审计），内容不被覆盖。
        let connection = open_database(&path).unwrap();
        let (old_content, superseded_by): (String, Option<i64>) = connection
            .query_row(
                "SELECT content, superseded_by_id FROM quick_ai_messages WHERE id = ?1",
                [old_assistant],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(old_content, "旧回答", "旧回答不得被物理覆盖");
        let superseded_by = superseded_by.expect("旧回答必须标记被替代");
        assert_ne!(superseded_by, old_assistant);
        let new_content: String = connection
            .query_row(
                "SELECT content FROM quick_ai_messages WHERE id = ?1",
                [superseded_by],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(new_content, "新回答");
        drop(connection);

        // 再次对同一旧目标重新生成：AlreadyCurrent 幂等返回当前权威快照。
        let RegenerationTurn::AlreadyCurrent { snapshot } = store
            .prepare_regeneration(conversation.id, 1, "原问题", old_assistant)
            .unwrap()
        else {
            panic!("已被替代的目标必须返回 AlreadyCurrent");
        };
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[1].content, "新回答");
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn regeneration_export_shows_current_answer_only() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store
            .create_with_exchange("deepseek-v4-flash", "导出问题", "旧回答")
            .unwrap();
        let old_assistant = conversation.messages[1].id;
        let RegenerationTurn::Ready {
            user_message_id, ..
        } = store
            .prepare_regeneration(conversation.id, 1, "导出问题", old_assistant)
            .unwrap()
        else {
            panic!("首次重新生成必须 Ready");
        };
        let regenerated = store
            .complete_regeneration(
                conversation.id,
                1,
                user_message_id,
                old_assistant,
                "当前回答",
                None,
                false,
            )
            .unwrap();
        let export_path = root.join("regenerated.md");
        export_snapshot_to_path(&regenerated, &export_path).unwrap();
        let content = fs::read_to_string(&export_path).unwrap();
        assert!(content.contains("导出问题"));
        assert!(content.contains("当前回答"));
        assert!(!content.contains("旧回答"), "导出必须只显示当前答案");
        assert_eq!(content.matches("## ReadRay").count(), 1);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn regeneration_keeps_parity_for_the_next_new_turn() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store
            .create_with_exchange("deepseek-v4-flash", "第一问", "第一答")
            .unwrap();
        let old_assistant = conversation.messages[1].id;
        let RegenerationTurn::Ready {
            user_message_id, ..
        } = store
            .prepare_regeneration(conversation.id, 1, "第一问", old_assistant)
            .unwrap()
        else {
            panic!("首次重新生成必须 Ready");
        };
        let regenerated = store
            .complete_regeneration(
                conversation.id,
                1,
                user_message_id,
                old_assistant,
                "重新回答",
                None,
                false,
            )
            .unwrap();
        assert_eq!(regenerated.messages.last().unwrap().sequence, 4);

        // 重新生成后追加新轮次：user sequence 仍为奇数、assistant 为下一个偶数。
        let PreparedTurn::Pending {
            user_message_id: next_user,
            ..
        } = store.prepare_turn(conversation.id, 5, "第二问").unwrap()
        else {
            panic!("重新生成后的新轮次必须 pending");
        };
        let completed = store
            .complete_turn(conversation.id, 5, next_user, "第二答", None, false)
            .unwrap();
        assert_eq!(
            completed
                .messages
                .iter()
                .map(|message| message.sequence)
                .collect::<Vec<_>>(),
            vec![1, 4, 5, 6],
            "可见序列：[user(1), 重新回答(4), 第二问(5), 第二答(6)]"
        );
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn regeneration_rejects_invalid_targets_and_untail_turns() {
        let (root, path) = test_database_path();
        let mut store = ConversationStore::open_path(&path).unwrap();
        let conversation = store
            .create_with_exchange("deepseek-v4-flash", "第一问", "第一答")
            .unwrap();
        let first_assistant = conversation.messages[1].id;
        let PreparedTurn::Pending {
            user_message_id, ..
        } = store.prepare_turn(conversation.id, 3, "第二问").unwrap()
        else {
            panic!("第二问必须 pending");
        };
        store
            .complete_turn(conversation.id, 3, user_message_id, "第二答", None, false)
            .unwrap();

        // 只允许重新生成会话中最后一条回答：第一轮的回答不是尾。
        let not_tail = store
            .prepare_regeneration(conversation.id, 1, "第一问", first_assistant)
            .unwrap_err();
        assert!(not_tail.contains("最后一条"));

        // 用户消息内容不匹配、不存在的目标、非 assistant 目标都被拒绝。
        let mismatch = store
            .prepare_regeneration(conversation.id, 1, "改过的问题", first_assistant)
            .unwrap_err();
        assert!(mismatch.contains("内容不匹配"));
        let missing = store
            .prepare_regeneration(conversation.id, 3, "第二问", 999_999)
            .unwrap_err();
        assert!(missing.contains("不存在"));
        let not_assistant = store
            .prepare_regeneration(conversation.id, 3, "第二问", {
                let loaded = store.get_required(conversation.id).unwrap();
                loaded.messages[2].id
            })
            .unwrap_err();
        assert!(not_assistant.contains("不是助手消息"));

        // 完成时目标已被替代：拒绝（避免两个"当前回答"并存）。
        let last_assistant = {
            let loaded = store.get_required(conversation.id).unwrap();
            loaded.messages.last().unwrap().id
        };
        let RegenerationTurn::Ready {
            user_message_id: regen_user,
            ..
        } = store
            .prepare_regeneration(conversation.id, 3, "第二问", last_assistant)
            .unwrap()
        else {
            panic!("尾回答必须 Ready");
        };
        let raced = store
            .complete_regeneration(
                conversation.id,
                3,
                regen_user,
                last_assistant,
                "并发替代",
                None,
                false,
            )
            .unwrap();
        // 第一次完成成功。
        let raced_assistant = raced.messages.last().unwrap().id;
        let second_attempt = store
            .complete_regeneration(
                conversation.id,
                3,
                regen_user,
                last_assistant,
                "不应保存",
                None,
                false,
            )
            .unwrap_err();
        assert!(second_attempt.contains("已被更新的回答替代"));
        let loaded = store.get_required(conversation.id).unwrap();
        assert_eq!(loaded.messages.last().unwrap().id, raced_assistant);
        assert_eq!(loaded.messages.last().unwrap().content, "并发替代");
        drop(store);
        let _ = fs::remove_dir_all(root);
    }
}
