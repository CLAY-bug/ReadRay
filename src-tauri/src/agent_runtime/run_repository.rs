//! Agent run/step/source 的 SQLite 仓库与状态迁移校验。
//!
//! 对应方案 §16.2 概念表与 §16.3 状态机：run 状态迁移必须由本仓库校验、不能
//! 任意倒退；completed 只能对应已持久化的最终 assistant（由调用方在业务写入
//! 成功后触发）；同一个 tool call 的步骤写入使用 run_id + step_sequence 幂等
//! 身份。本模块不承载 API Key、prompt 或私有推理。

use crate::agent_runtime::coordinator::AgentEventSink;
use crate::agent_runtime::protocol::{
    AgentError, AgentErrorKind, AgentEvent, AgentEventPayload, AgentSurface,
};
use crate::learning_records::{open_database, unix_time_ms};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentRunStatus {
    Prepared,
    ModelStreaming,
    ToolRunning,
    Synthesizing,
    Completed,
    Stopped,
    Failed,
    Truncated,
}

impl AgentRunStatus {
    fn as_storage(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::ModelStreaming => "model_streaming",
            Self::ToolRunning => "tool_running",
            Self::Synthesizing => "synthesizing",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Truncated => "truncated",
        }
    }

    fn from_storage(value: &str) -> Result<Self, String> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "model_streaming" => Ok(Self::ModelStreaming),
            "tool_running" => Ok(Self::ToolRunning),
            "synthesizing" => Ok(Self::Synthesizing),
            "completed" => Ok(Self::Completed),
            "stopped" => Ok(Self::Stopped),
            "failed" => Ok(Self::Failed),
            "truncated" => Ok(Self::Truncated),
            _ => Err(format!("agent run 包含未知状态：{value}")),
        }
    }

    pub(crate) fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Stopped | Self::Failed | Self::Truncated
        )
    }
}

/// 状态迁移表（§16.3）：终态不可迁移，prepared 之后的推进必须按序经过
/// model_streaming / tool_running / synthesizing，任何状态都可进入终态。
fn allowed_transitions(from: AgentRunStatus) -> &'static [AgentRunStatus] {
    match from {
        AgentRunStatus::Prepared => &[
            AgentRunStatus::ModelStreaming,
            AgentRunStatus::Stopped,
            AgentRunStatus::Failed,
            AgentRunStatus::Truncated,
        ],
        AgentRunStatus::ModelStreaming => &[
            AgentRunStatus::ToolRunning,
            AgentRunStatus::Synthesizing,
            AgentRunStatus::ModelStreaming,
            AgentRunStatus::Stopped,
            AgentRunStatus::Failed,
            AgentRunStatus::Truncated,
        ],
        AgentRunStatus::ToolRunning => &[
            AgentRunStatus::ModelStreaming,
            AgentRunStatus::Stopped,
            AgentRunStatus::Failed,
            AgentRunStatus::Truncated,
        ],
        AgentRunStatus::Synthesizing => &[
            AgentRunStatus::Completed,
            AgentRunStatus::Stopped,
            AgentRunStatus::Failed,
            AgentRunStatus::Truncated,
        ],
        AgentRunStatus::Completed
        | AgentRunStatus::Stopped
        | AgentRunStatus::Failed
        | AgentRunStatus::Truncated => &[],
    }
}

/// 恢复查询的完整载荷；当前生产只消费 run_id/status，其余字段供后续任务
/// （恢复策略、审计）读取。
#[allow(dead_code)]
pub(crate) struct StoredRun {
    pub run_id: String,
    pub surface: AgentSurface,
    pub authority_kind: String,
    pub conversation_id: Option<i64>,
    pub expected_user_sequence: Option<i64>,
    pub user_message_id: Option<i64>,
    pub retry_of_run_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub status: AgentRunStatus,
    pub termination_reason: Option<String>,
    pub started_at_unix_ms: i64,
    pub completed_at_unix_ms: Option<i64>,
}

pub(crate) struct NewRun {
    pub run_id: String,
    pub surface: AgentSurface,
    pub conversation_id: i64,
    pub expected_user_sequence: i64,
    pub user_message_id: i64,
    pub retry_of_run_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub started_at_unix_ms: i64,
}

pub(crate) struct NewStep {
    pub run_id: String,
    pub step_sequence: i64,
    pub turn_index: Option<u32>,
    pub kind: String,
    pub status: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub input_json: Option<String>,
    pub result_json: Option<String>,
    pub error_code: Option<String>,
    pub started_at_unix_ms: i64,
    pub completed_at_unix_ms: Option<i64>,
}

pub(crate) struct NewSource {
    pub source_id: String,
    pub run_id: String,
    pub tool_call_id: String,
    pub title: String,
    pub url: String,
    pub site_name: Option<String>,
    pub published_at: Option<String>,
    pub retrieved_at_unix_ms: i64,
    pub content_type: Option<String>,
    pub metadata_json: Option<String>,
}

pub(crate) struct AgentRunRepository {
    connection: Connection,
}

impl AgentRunRepository {
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        Ok(Self {
            connection: open_database(path)?,
        })
    }

    pub(crate) fn create_run(&mut self, run: &NewRun) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO agent_runs (
                   run_id, surface_kind, authority_kind, conversation_id,
                   expected_user_sequence, user_message_id, retry_of_run_id,
                   provider, model, status, started_at_unix_ms, updated_at_unix_ms
                 ) VALUES (?1, ?2, 'conversation', ?3, ?4, ?5, ?6, ?7, ?8,
                           'prepared', ?9, ?9)",
                params![
                    run.run_id,
                    surface_to_storage(run.surface),
                    run.conversation_id,
                    run.expected_user_sequence,
                    run.user_message_id,
                    run.retry_of_run_id,
                    run.provider,
                    run.model,
                    run.started_at_unix_ms,
                ],
            )
            .map_err(|error| format!("agent run 创建失败：{error}"))?;
        Ok(())
    }

    /// 状态迁移：读取当前状态、校验迁移表、乐观更新。终态后任何迁移都被拒绝。
    pub(crate) fn transition(
        &mut self,
        run_id: &str,
        to: AgentRunStatus,
        termination_reason: Option<&str>,
        now_unix_ms: i64,
    ) -> Result<(), String> {
        let current: Option<String> = self
            .connection
            .query_row(
                "SELECT status FROM agent_runs WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("agent run 状态读取失败：{error}"))?;
        let current = current.ok_or_else(|| format!("agent run 不存在：{run_id}"))?;
        let current = AgentRunStatus::from_storage(&current)?;
        if !allowed_transitions(current).contains(&to) {
            return Err(format!(
                "agent run 状态迁移非法：{current:?} -> {to:?}（run_id={run_id}）。"
            ));
        }
        let completed_at = (to == AgentRunStatus::Completed).then_some(now_unix_ms);
        let updated = self
            .connection
            .execute(
                "UPDATE agent_runs
                 SET status = ?1, termination_reason = ?2,
                     completed_at_unix_ms = ?3, updated_at_unix_ms = ?4
                 WHERE run_id = ?5 AND status = ?6",
                params![
                    to.as_storage(),
                    termination_reason,
                    completed_at,
                    now_unix_ms,
                    run_id,
                    current.as_storage(),
                ],
            )
            .map_err(|error| format!("agent run 状态更新失败：{error}"))?;
        if updated == 0 {
            return Err(format!(
                "agent run 状态并发冲突：run_id={run_id} 已不在 {current:?} 状态。"
            ));
        }
        Ok(())
    }

    /// 步骤写入使用 (run_id, step_sequence) 幂等身份；重复写入被忽略。
    pub(crate) fn append_step(&mut self, step: &NewStep) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO agent_steps (
                   run_id, step_sequence, turn_index, kind, status,
                   tool_call_id, tool_name, input_json, result_json, error_code,
                   started_at_unix_ms, completed_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    step.run_id,
                    step.step_sequence,
                    step.turn_index,
                    step.kind,
                    step.status,
                    step.tool_call_id,
                    step.tool_name,
                    step.input_json,
                    step.result_json,
                    step.error_code,
                    step.started_at_unix_ms,
                    step.completed_at_unix_ms,
                ],
            )
            .map_err(|error| format!("agent step 写入失败：{error}"))?;
        Ok(())
    }

    /// 来源写入使用 source_id 幂等身份；重复写入被忽略。
    pub(crate) fn insert_source(&mut self, source: &NewSource) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO agent_sources (
                   source_id, run_id, tool_call_id, title, url, site_name,
                   published_at, retrieved_at_unix_ms, content_type, metadata_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    source.source_id,
                    source.run_id,
                    source.tool_call_id,
                    source.title,
                    source.url,
                    source.site_name,
                    source.published_at,
                    source.retrieved_at_unix_ms,
                    source.content_type,
                    source.metadata_json,
                ],
            )
            .map_err(|error| format!("agent source 写入失败：{error}"))?;
        Ok(())
    }

    /// 恢复查询：该 conversation 轮次最近一次 run（按开始时间倒序）。
    pub(crate) fn latest_run_for_turn(
        &self,
        conversation_id: i64,
        expected_user_sequence: i64,
    ) -> Result<Option<StoredRun>, String> {
        self.connection
            .query_row(
                "SELECT run_id, surface_kind, authority_kind, conversation_id,
                        expected_user_sequence, user_message_id, retry_of_run_id,
                        provider, model, status, termination_reason,
                        started_at_unix_ms, completed_at_unix_ms
                 FROM agent_runs
                 WHERE authority_kind = 'conversation'
                   AND conversation_id = ?1 AND expected_user_sequence = ?2
                 ORDER BY started_at_unix_ms DESC, run_id DESC
                 LIMIT 1",
                params![conversation_id, expected_user_sequence],
                read_stored_run,
            )
            .optional()
            .map_err(|error| format!("agent run 恢复查询失败：{error}"))
    }

    #[cfg(test)]
    pub(crate) fn get_run(&self, run_id: &str) -> Result<Option<StoredRun>, String> {
        self.connection
            .query_row(
                "SELECT run_id, surface_kind, authority_kind, conversation_id,
                        expected_user_sequence, user_message_id, retry_of_run_id,
                        provider, model, status, termination_reason,
                        started_at_unix_ms, completed_at_unix_ms
                 FROM agent_runs WHERE run_id = ?1",
                [run_id],
                read_stored_run,
            )
            .optional()
            .map_err(|error| format!("agent run 读取失败：{error}"))
    }
}

/// 把内核事件流持久化为 run 状态与 step/source 行的 sink。
///
/// 分工：本 sink 只推进中间状态（prepared → model_streaming → tool_running →
/// synthesizing）；终态（completed/stopped/failed/truncated）由调用方在业务
/// 写入（complete_turn）成功后统一写入，保证 completed 只对应已持久化的最终
/// assistant。持久化失败返回 `PersistenceFailed`，由调用方按"不伪装成功"
/// 处理（保留 pending user，允许重试）。
pub(crate) struct PersistingSink<'a> {
    run_id: String,
    repository: &'a mut AgentRunRepository,
}

impl<'a> PersistingSink<'a> {
    pub(crate) fn new(run_id: impl Into<String>, repository: &'a mut AgentRunRepository) -> Self {
        Self {
            run_id: run_id.into(),
            repository,
        }
    }
}

impl AgentEventSink for PersistingSink<'_> {
    fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError> {
        let now = unix_time_ms().unwrap_or(0);
        match &event.payload {
            AgentEventPayload::TurnStarted { .. } => {
                self.repository
                    .transition(&self.run_id, AgentRunStatus::ModelStreaming, None, now)
            }
            AgentEventPayload::ToolCallStarted { .. } => {
                self.repository
                    .transition(&self.run_id, AgentRunStatus::ToolRunning, None, now)
            }
            AgentEventPayload::RunCompleted { .. } => {
                self.repository
                    .transition(&self.run_id, AgentRunStatus::Synthesizing, None, now)
            }
            _ => Ok(()),
        }
        .map_err(persistence_error)?;

        if !matches!(event.payload, AgentEventPayload::RunStarted { .. }) {
            let (kind, status, tool_call_id, tool_name, input_json, result_json, error_code) =
                describe_step(&event.payload);
            self.repository
                .append_step(&NewStep {
                    run_id: self.run_id.clone(),
                    step_sequence: i64::try_from(event.step_sequence).unwrap_or(i64::MAX),
                    turn_index: event.turn_id,
                    kind,
                    status,
                    tool_call_id,
                    tool_name,
                    input_json,
                    result_json,
                    error_code,
                    started_at_unix_ms: now,
                    completed_at_unix_ms: None,
                })
                .map_err(persistence_error)?;
        }

        if let AgentEventPayload::ToolCallCompleted { result }
        | AgentEventPayload::ToolCallFailed { result } = &event.payload
        {
            // 任务 3：来源与工具调用关联落库（工具结果的 details.sources）。
            for source in crate::agent_runtime::network::sources_from_details(&result.details) {
                self.repository
                    .insert_source(&NewSource {
                        source_id: source.source_id.clone(),
                        run_id: self.run_id.clone(),
                        tool_call_id: result.tool_call_id.clone(),
                        title: source.title.clone(),
                        url: source.url.clone(),
                        site_name: source.site_name.clone(),
                        published_at: source.published_at.clone(),
                        retrieved_at_unix_ms: i64::try_from(source.retrieved_at_unix_ms)
                            .unwrap_or(0),
                        content_type: source.content_type.clone(),
                        metadata_json: None,
                    })
                    .map_err(persistence_error)?;
            }
        }
        Ok(())
    }
}

fn persistence_error(error: String) -> AgentError {
    agent_error(AgentErrorKind::PersistenceFailed, error)
}

fn agent_error(kind: AgentErrorKind, message: impl Into<String>) -> AgentError {
    AgentError::new(kind, message).expect("persisting sink 的固定错误消息必须有效")
}

/// 事件 → step 行的列投影（审计用途，字段名不属于公开协议）。
fn describe_step(
    payload: &AgentEventPayload,
) -> (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    match payload {
        AgentEventPayload::TurnStarted { .. } => {
            step("turn_started", "ok", None, None, None, None, None)
        }
        AgentEventPayload::AssistantTextDelta { .. } => {
            step("assistant_text_delta", "ok", None, None, None, None, None)
        }
        AgentEventPayload::AssistantTextCompleted { .. } => step(
            "assistant_text_completed",
            "ok",
            None,
            None,
            None,
            None,
            None,
        ),
        AgentEventPayload::ToolCallStarted { call } => step(
            "tool_call_started",
            "ok",
            Some(call.id.clone()),
            Some(call.name.clone()),
            Some(call.arguments.to_string()),
            None,
            None,
        ),
        AgentEventPayload::ToolCallProgress { tool_call_id, .. } => step(
            "tool_call_progress",
            "ok",
            Some(tool_call_id.clone()),
            None,
            None,
            None,
            None,
        ),
        AgentEventPayload::ToolCallCompleted { result } => step(
            "tool_call_completed",
            "ok",
            Some(result.tool_call_id.clone()),
            Some(result.tool_name.clone()),
            None,
            Some(result.content.clone()),
            None,
        ),
        AgentEventPayload::ToolCallFailed { result } => step(
            "tool_call_failed",
            "failed",
            Some(result.tool_call_id.clone()),
            Some(result.tool_name.clone()),
            None,
            Some(result.content.clone()),
            result
                .error
                .as_ref()
                .map(|error| format!("{:?}", error.kind)),
        ),
        AgentEventPayload::SourcesUpdated { .. } => {
            step("sources_updated", "ok", None, None, None, None, None)
        }
        AgentEventPayload::RunStopped { reason } => step(
            "run_stopped",
            "ok",
            None,
            None,
            None,
            None,
            Some(format!("{reason:?}")),
        ),
        AgentEventPayload::RunTruncated { reason } => step(
            "run_truncated",
            "ok",
            None,
            None,
            None,
            None,
            Some(format!("{reason:?}")),
        ),
        AgentEventPayload::RunFailed { error } => step(
            "run_failed",
            "failed",
            None,
            None,
            None,
            None,
            Some(format!("{:?}", error.kind)),
        ),
        AgentEventPayload::RunCompleted { .. } => {
            step("run_completed", "ok", None, None, None, None, None)
        }
        AgentEventPayload::RunStarted { .. } => {
            step("run_started", "ok", None, None, None, None, None)
        }
    }
}

fn step(
    kind: &str,
    status: &str,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    input_json: Option<String>,
    result_json: Option<String>,
    error_code: Option<String>,
) -> (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    (
        kind.to_string(),
        status.to_string(),
        tool_call_id,
        tool_name,
        input_json,
        result_json,
        error_code,
    )
}

#[derive(Debug)]
struct ConversionError(String);

impl std::fmt::Display for ConversionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ConversionError {}

fn conversion_error(index: usize, message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(ConversionError(message)),
    )
}

fn read_stored_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRun> {
    Ok(StoredRun {
        run_id: row.get(0)?,
        surface: surface_from_storage(&row.get::<_, String>(1)?)
            .map_err(|error| conversion_error(1, error))?,
        authority_kind: row.get(2)?,
        conversation_id: row.get(3)?,
        expected_user_sequence: row.get(4)?,
        user_message_id: row.get(5)?,
        retry_of_run_id: row.get(6)?,
        provider: row.get(7)?,
        model: row.get(8)?,
        status: AgentRunStatus::from_storage(&row.get::<_, String>(9)?)
            .map_err(|error| conversion_error(9, error))?,
        termination_reason: row.get(10)?,
        started_at_unix_ms: row.get(11)?,
        completed_at_unix_ms: row.get(12)?,
    })
}

fn surface_to_storage(surface: AgentSurface) -> &'static str {
    match surface {
        AgentSurface::MainConversation => "main_conversation",
        AgentSurface::QuickAiOverlay => "quick_ai_overlay",
        AgentSurface::WritingCoach => "writing_coach",
    }
}

fn surface_from_storage(value: &str) -> Result<AgentSurface, String> {
    match value {
        "main_conversation" => Ok(AgentSurface::MainConversation),
        "quick_ai_overlay" => Ok(AgentSurface::QuickAiOverlay),
        "writing_coach" => Ok(AgentSurface::WritingCoach),
        _ => Err(format!("agent run 包含未知 surface：{value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_DATABASE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_database_path() -> (std::path::PathBuf, std::path::PathBuf) {
        let suffix = format!(
            "readray-agent-runs-{}-{}",
            std::process::id(),
            TEST_DATABASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(suffix);
        (root.clone(), root.join("readray.sqlite3"))
    }

    fn new_run(run_id: &str, started_at: i64) -> NewRun {
        NewRun {
            run_id: run_id.to_string(),
            surface: AgentSurface::MainConversation,
            conversation_id: 1,
            expected_user_sequence: 3,
            user_message_id: 9,
            retry_of_run_id: None,
            provider: "deepseek".to_string(),
            model: "deepseek-v4-flash".to_string(),
            started_at_unix_ms: started_at,
        }
    }

    #[test]
    fn run_round_trip_and_latest_run_for_turn() {
        let (root, path) = test_database_path();
        let mut repository = AgentRunRepository::open(&path).unwrap();
        repository.create_run(&new_run("run-a", 100)).unwrap();
        repository.create_run(&new_run("run-b", 200)).unwrap();

        let latest = repository
            .latest_run_for_turn(1, 3)
            .unwrap()
            .expect("必须能找到最近 run");
        assert_eq!(latest.run_id, "run-b");
        assert_eq!(latest.status, AgentRunStatus::Prepared);
        assert_eq!(latest.conversation_id, Some(1));
        assert_eq!(latest.expected_user_sequence, Some(3));
        assert_eq!(latest.user_message_id, Some(9));
        assert_eq!(latest.surface, AgentSurface::MainConversation);

        assert!(repository.get_run("run-a").unwrap().is_some());
        assert!(repository.get_run("missing").unwrap().is_none());
        assert!(repository.latest_run_for_turn(2, 3).unwrap().is_none());
        drop(repository);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_transitions_are_validated_and_terminal_states_are_frozen() {
        let (root, path) = test_database_path();
        let mut repository = AgentRunRepository::open(&path).unwrap();
        repository.create_run(&new_run("run-1", 100)).unwrap();

        // 合法推进：prepared -> model_streaming -> tool_running -> model_streaming -> synthesizing -> completed
        repository
            .transition("run-1", AgentRunStatus::ModelStreaming, None, 110)
            .unwrap();
        repository
            .transition("run-1", AgentRunStatus::ToolRunning, None, 120)
            .unwrap();
        repository
            .transition("run-1", AgentRunStatus::ModelStreaming, None, 130)
            .unwrap();
        repository
            .transition("run-1", AgentRunStatus::Synthesizing, None, 140)
            .unwrap();
        repository
            .transition("run-1", AgentRunStatus::Completed, None, 150)
            .unwrap();
        let completed = repository.get_run("run-1").unwrap().unwrap();
        assert_eq!(completed.status, AgentRunStatus::Completed);
        assert_eq!(completed.completed_at_unix_ms, Some(150));

        // 终态后任何迁移都被拒绝。
        let frozen = repository
            .transition("run-1", AgentRunStatus::ModelStreaming, None, 160)
            .unwrap_err();
        assert!(frozen.contains("非法"));

        // 非法倒退：completed 之外也不允许回退。
        repository.create_run(&new_run("run-2", 100)).unwrap();
        repository
            .transition("run-2", AgentRunStatus::ModelStreaming, None, 110)
            .unwrap();
        let rewind = repository
            .transition("run-2", AgentRunStatus::Prepared, None, 120)
            .unwrap_err();
        assert!(rewind.contains("非法"));

        // 终态带 termination_reason：stopped。
        repository.create_run(&new_run("run-3", 100)).unwrap();
        repository
            .transition("run-3", AgentRunStatus::Stopped, Some("user_aborted"), 120)
            .unwrap();
        let stopped = repository.get_run("run-3").unwrap().unwrap();
        assert_eq!(stopped.termination_reason.as_deref(), Some("user_aborted"));
        assert_eq!(stopped.completed_at_unix_ms, None);
        drop(repository);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn step_and_source_writes_are_idempotent_by_identity() {
        let (root, path) = test_database_path();
        let mut repository = AgentRunRepository::open(&path).unwrap();
        repository.create_run(&new_run("run-1", 100)).unwrap();

        let step = NewStep {
            run_id: "run-1".to_string(),
            step_sequence: 2,
            turn_index: Some(1),
            kind: "tool_call".to_string(),
            status: "ok".to_string(),
            tool_call_id: Some("call-1".to_string()),
            tool_name: Some("get_date".to_string()),
            input_json: Some("{}".to_string()),
            result_json: Some("{\"content\":\"2026-08-17\"}".to_string()),
            error_code: None,
            started_at_unix_ms: 100,
            completed_at_unix_ms: Some(101),
        };
        repository.append_step(&step).unwrap();
        repository.append_step(&step).unwrap();
        let count: i64 = repository
            .connection
            .query_row(
                "SELECT COUNT(*) FROM agent_steps WHERE run_id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let source = NewSource {
            source_id: "source-1".to_string(),
            run_id: "run-1".to_string(),
            tool_call_id: "call-1".to_string(),
            title: "Example".to_string(),
            url: "https://example.com/article".to_string(),
            site_name: None,
            published_at: None,
            retrieved_at_unix_ms: 100,
            content_type: None,
            metadata_json: None,
        };
        repository.insert_source(&source).unwrap();
        repository.insert_source(&source).unwrap();
        let source_count: i64 = repository
            .connection
            .query_row(
                "SELECT COUNT(*) FROM agent_sources WHERE run_id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_count, 1);

        // 未知 run 的 step 写入被外键拒绝。
        let orphan = NewStep {
            run_id: "missing".to_string(),
            step_sequence: 1,
            turn_index: None,
            kind: "turn".to_string(),
            status: "ok".to_string(),
            tool_call_id: None,
            tool_name: None,
            input_json: None,
            result_json: None,
            error_code: None,
            started_at_unix_ms: 100,
            completed_at_unix_ms: None,
        };
        assert!(repository.append_step(&orphan).is_err());
        drop(repository);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persisting_sink_links_sources_to_tool_call_id() {
        let (root, path) = test_database_path();
        let mut repository = AgentRunRepository::open(&path).unwrap();
        repository.create_run(&new_run("run-1", 100)).unwrap();
        let mut sink = PersistingSink::new("run-1", &mut repository);
        let source = crate::agent_runtime::protocol::SourceMetadata {
            source_id: "source-1".to_string(),
            title: "Example".to_string(),
            url: "https://example.com/article".to_string(),
            site_name: Some("Example".to_string()),
            published_at: None,
            retrieved_at_unix_ms: 100,
            content_type: None,
        };
        let call = crate::agent_runtime::protocol::ToolCall {
            id: "call-search-1".to_string(),
            name: "web_search".to_string(),
            arguments: serde_json::json!({"query": "Rust"}),
        };
        let result = crate::agent_runtime::protocol::ToolResult {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            is_error: false,
            is_truncated: false,
            content: "results".to_string(),
            provenance: crate::agent_runtime::protocol::ToolProvenance::ExternalSearch,
            started_at_unix_ms: 100,
            finished_at_unix_ms: 101,
            details: Some(serde_json::json!({ "sources": [source] })),
            error: None,
        };
        let event = crate::agent_runtime::protocol::AgentEvent::new(
            "run-1",
            Some(1),
            2,
            crate::agent_runtime::protocol::AgentEventPayload::ToolCallCompleted { result },
        )
        .unwrap();
        sink.emit(event).unwrap();
        drop(sink);

        let (tool_call_id, url, title): (String, String, String) = repository
            .connection
            .query_row(
                "SELECT tool_call_id, url, title FROM agent_sources WHERE source_id = 'source-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(tool_call_id, "call-search-1", "来源必须关联 tool_call_id");
        assert_eq!(url, "https://example.com/article");
        assert_eq!(title, "Example");
        drop(repository);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persisting_sink_does_not_write_empty_tool_call_sources() {
        let (root, path) = test_database_path();
        let mut repository = AgentRunRepository::open(&path).unwrap();
        repository.create_run(&new_run("run-1", 100)).unwrap();
        let mut sink = PersistingSink::new("run-1", &mut repository);
        // SourcesUpdated 事件本身不再落库（来源由工具完成事件携带 tool_call_id 落库）。
        let source = crate::agent_runtime::protocol::SourceMetadata {
            source_id: "source-orphan".to_string(),
            title: "Orphan".to_string(),
            url: "https://example.com/orphan".to_string(),
            site_name: None,
            published_at: None,
            retrieved_at_unix_ms: 100,
            content_type: None,
        };
        let event = crate::agent_runtime::protocol::AgentEvent::new(
            "run-1",
            Some(1),
            2,
            crate::agent_runtime::protocol::AgentEventPayload::SourcesUpdated {
                sources: vec![source],
            },
        )
        .unwrap();
        sink.emit(event).unwrap();
        drop(sink);
        let count: i64 = repository
            .connection
            .query_row(
                "SELECT COUNT(*) FROM agent_sources WHERE run_id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "没有 tool_call_id 的来源不落库");
        drop(repository);
        let _ = fs::remove_dir_all(root);
    }
}
