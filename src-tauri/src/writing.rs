use crate::agent_runtime::chat_surface::generate_run_id;
use crate::agent_runtime::context::{ContextAssembler, RuntimeFacts};
use crate::agent_runtime::coordinator::{
    AgentDeps, AgentEventSink, AgentRunCoordinator, Cancellation, RunRequest, SystemTimeSource,
    ToolExecutionOrder,
};
use crate::agent_runtime::deepseek_gateway::DeepSeekChatGateway;
use crate::agent_runtime::gateway::{ModelGateway, ProviderMessage};
use crate::agent_runtime::protocol::{
    AgentError, AgentErrorKind, AgentEvent, AgentEventPayload, AuthorityRef, TerminationReason,
};
use crate::agent_runtime::tool::{ToolPolicy, ToolRegistry};
use crate::agent_runtime::writing_surface::{
    writing_active_tools, writing_capability, WritingSurfaceAdapter,
};
use crate::deepseek_client::configured_model;
use crate::learning_records::{open_database_for_app, unix_time_ms};
use crate::model_usage::ModelUsageCategory;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::AppHandle;

const WRITING_ANALYSIS_SCHEMA_VERSION: i64 = 1;
const WRITING_ANSWER_SCHEMA_VERSION: i64 = 1;
const WRITING_MAX_TITLE_CHARS: usize = 300;
const WRITING_MAX_PARAGRAPHS: usize = 500;
const WRITING_MAX_PARAGRAPH_CHARS: usize = 20_000;
const WRITING_MAX_TOTAL_CHARS: usize = 100_000;
const WRITING_MAX_ISSUES: usize = 8;
const WRITING_MAX_PATTERNS: usize = 4;
const WRITING_MAX_QUESTION_CHARS: usize = 2_000;
const WRITING_MAX_SELECTION_CHARS: usize = 2_000;
const WRITING_MAX_CONVERSATION_CONTEXT_ANSWERS: usize = 8;
/// 写作生成参数与已验证的旧写作请求一致（2026 年生产路径）：推理模型在更大
/// 输出预算下可能把预算全烧在 reasoning_content 上导致正文为空。
const WRITING_MAX_TOKENS: u16 = 4_096;
const WRITING_TEMPERATURE: f32 = 0.2;

/// 写作流式事件协议：Writing 检查/问答不逐字流式渲染结构 JSON，只向用户展示
/// 友好的进度状态与终态。用户是普通学习者，事件只表达"正在做什么/已完成"，
/// 不暴露 prompt、工具调用内部或错误原文。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum WritingStreamEvent {
    /// 友好进度文案："正在检查语法…" / "正在判断表达方式…" 等。
    Status { label: String },
    /// 结果已通过结构校验并进入正式状态（保存完成）。
    Done,
    /// 用户主动中断，草稿与已产生内容保留；可重新检查。
    Stopped,
    /// 友好失败，不展示技术原文。
    Error { message: String },
}

#[derive(Clone)]
struct WritingStreamSender {
    channel: Channel<WritingStreamEvent>,
}

impl WritingStreamSender {
    fn new(channel: Channel<WritingStreamEvent>) -> Self {
        Self { channel }
    }

    fn send(&self, event: WritingStreamEvent) -> bool {
        self.channel.send(event).is_ok()
    }
}

/// 写作请求的中止标志（按 document_id 键控）：检查/问答过程中用户可主动中断。
static WRITING_ABORT_FLAGS: Mutex<Option<HashMap<i64, Arc<AtomicBool>>>> = Mutex::new(None);

fn writing_abort_flag_for(document_id: i64) -> Arc<AtomicBool> {
    let mut flags = WRITING_ABORT_FLAGS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let slots = flags.get_or_insert_with(HashMap::new);
    if let Some(flag) = slots.get(&document_id) {
        return flag.clone();
    }
    let flag = Arc::new(AtomicBool::new(false));
    slots.insert(document_id, flag.clone());
    flag
}

/// 写作进度阶段文案。Writing 检查/问答共享同一条运行管线，只在文案上有差异。
#[derive(Clone, Copy)]
struct WritingStageLabels {
    start: &'static str,
    analyzing: &'static str,
    finishing: &'static str,
}

/// 把内核 AgentEvent 投影为写作友好进度事件。终端事件（Done/Stopped/Error）
/// 由调用方在 coordinator 结束后依据真实结果发送，这里只处理中间状态。
struct WritingUiSink {
    sender: WritingStreamSender,
    labels: WritingStageLabels,
    analyzing_emitted: bool,
}

impl AgentEventSink for WritingUiSink {
    fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError> {
        let Some(projected) =
            project_writing_ui_event(&event, self.labels, &mut self.analyzing_emitted)
        else {
            return Ok(());
        };
        if !self.sender.send(projected) {
            return Err(agent_stream_error("写作流式状态事件无法送达。"));
        }
        Ok(())
    }
}

/// AgentEvent → 写作友好进度状态的确定性投影（离线可测）。
fn project_writing_ui_event(
    event: &AgentEvent,
    labels: WritingStageLabels,
    analyzing_emitted: &mut bool,
) -> Option<WritingStreamEvent> {
    match &event.payload {
        AgentEventPayload::TurnStarted { .. } => {
            if *analyzing_emitted {
                None
            } else {
                Some(WritingStreamEvent::Status {
                    label: labels.start.to_string(),
                })
            }
        }
        AgentEventPayload::AssistantTextDelta { .. } => {
            if *analyzing_emitted {
                None
            } else {
                *analyzing_emitted = true;
                Some(WritingStreamEvent::Status {
                    label: labels.analyzing.to_string(),
                })
            }
        }
        AgentEventPayload::AssistantTextCompleted { .. } => Some(WritingStreamEvent::Status {
            label: labels.finishing.to_string(),
        }),
        AgentEventPayload::ToolCallStarted { call } => {
            let label = match call.name.as_str() {
                "web_search" => "正在核实资料…",
                "fetch_web_page" => "正在读取资料…",
                _ => return None,
            };
            Some(WritingStreamEvent::Status {
                label: label.to_string(),
            })
        }
        AgentEventPayload::ToolCallCompleted { .. } => Some(WritingStreamEvent::Status {
            label: labels.finishing.to_string(),
        }),
        _ => None,
    }
}

fn agent_stream_error(message: impl Into<String>) -> AgentError {
    AgentError::new(AgentErrorKind::PersistenceFailed, message).expect("固定流事件错误消息必须有效")
}

/// 运行写作 coordinator 并返回 RunOutcome。Writing 不写 agent_runs 业务审计表
/// （写作权威仍归 writing_analyses / writing_assistant_answers），因此 sink 只
/// 转发现进度事件；持久化失败快速失败不伪装成功。
fn run_writing_coordinator(
    gateway: &mut dyn ModelGateway,
    registry: &ToolRegistry,
    transcript: Vec<ProviderMessage>,
    run_id: String,
    authority: AuthorityRef,
    cancellation: &Cancellation,
    sender: WritingStreamSender,
    labels: WritingStageLabels,
) -> Result<crate::agent_runtime::coordinator::RunOutcome, AgentError> {
    let capability = writing_capability();
    let mut ui = WritingUiSink {
        sender,
        labels,
        analyzing_emitted: false,
    };
    let mut coordinator = AgentRunCoordinator::new(
        run_id,
        authority,
        crate::agent_runtime::protocol::RunBudget::first_version(),
    )
    .map_err(agent_stream_error)?;
    let request = RunRequest {
        user_prompt: String::new(),
        runtime_facts: RuntimeFacts {
            local_datetime: String::new(),
            timezone: String::new(),
            app_version: String::new(),
        },
        capability,
        tool_execution_order: ToolExecutionOrder::CallOrder,
        initial_messages: Some(transcript),
    };
    let mut deps = AgentDeps {
        gateway,
        registry,
        policy: &ToolPolicy,
        assembler: &ContextAssembler::default(),
        time: &SystemTimeSource,
        cancellation,
        sink: &mut ui,
    };
    coordinator.run(&request, &mut deps)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WritingSnapshot {
    pub title: String,
    pub paragraphs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WritingDocumentSummary {
    pub id: i64,
    pub revision: i64,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
    pub last_opened_at_unix_ms: Option<i64>,
    pub draft_updated_at_unix_ms: Option<i64>,
    pub completed_at_unix_ms: Option<i64>,
    pub draft_snapshot: Option<WritingSnapshot>,
    pub completed_snapshot: Option<WritingSnapshot>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WritingDocumentRecord {
    #[serde(flatten)]
    pub summary: WritingDocumentSummary,
    pub comparison_baseline: WritingSnapshot,
    pub comparison_baseline_revision: Option<i64>,
    pub versions: Vec<WritingVersion>,
    pub active_analysis: Option<WritingAnalysis>,
    pub baseline_analysis: Option<WritingAnalysis>,
    pub answers: Vec<WritingAgentAnswer>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WritingVersion {
    pub id: i64,
    pub document_id: i64,
    pub ordinal: i64,
    pub source_revision: i64,
    pub analysis_revision: Option<i64>,
    pub comparison_baseline_revision: Option<i64>,
    pub snapshot: WritingSnapshot,
    pub comparison_baseline: WritingSnapshot,
    pub issues: Vec<WritingIssue>,
    pub patterns: Vec<WritingPattern>,
    pub completed_at_unix_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WritingIssue {
    pub id: String,
    pub category: String,
    pub source: String,
    pub target_text: String,
    pub explanation: String,
    pub hint: String,
    pub deeper_hint: String,
    pub reference: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WritingPattern {
    pub id: String,
    pub title: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WritingAnalysisContent {
    issues: Vec<WritingIssue>,
    patterns: Vec<WritingPattern>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WritingAnalysis {
    pub id: i64,
    pub document_id: i64,
    pub document_revision: i64,
    pub round: i64,
    pub issues: Vec<WritingIssue>,
    pub patterns: Vec<WritingPattern>,
    pub created_at_unix_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WritingAnswerMap {
    pub core: String,
    pub questions: Vec<String>,
    pub phrases: Vec<String>,
    pub starters: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WritingAnswerContent {
    title: String,
    copy: String,
    map: Option<WritingAnswerMap>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WritingQuestionScope {
    Document,
    Paragraph,
    Selection,
}

impl WritingQuestionScope {
    fn storage_value(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Paragraph => "paragraph",
            Self::Selection => "selection",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Document => "整篇文章",
            Self::Paragraph => "当前段落",
            Self::Selection => "所选内容",
        }
    }

    fn from_storage(value: &str) -> Result<Self, String> {
        match value {
            "document" => Ok(Self::Document),
            "paragraph" => Ok(Self::Paragraph),
            "selection" => Ok(Self::Selection),
            _ => Err(format!("写作辅助记录包含未知 scope：{value}")),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WritingQuestionRequest {
    pub document_id: i64,
    pub expected_revision: i64,
    pub version_id: Option<i64>,
    pub question: String,
    pub scope: WritingQuestionScope,
    pub selection_text: Option<String>,
    pub parent_answer_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WritingAgentAnswer {
    pub id: i64,
    pub document_id: i64,
    pub document_revision: i64,
    pub version_id: Option<i64>,
    pub parent_answer_id: Option<i64>,
    pub question: String,
    pub scope: WritingQuestionScope,
    pub scope_label: String,
    pub selection_text: Option<String>,
    pub title: String,
    pub copy: String,
    pub map: Option<WritingAnswerMap>,
    pub created_at_unix_ms: i64,
}

#[derive(Debug)]
struct StoredDocument {
    id: i64,
    revision: i64,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
    last_opened_at_unix_ms: Option<i64>,
    draft_title: Option<String>,
    draft_paragraphs_json: Option<String>,
    draft_updated_at_unix_ms: Option<i64>,
    completed_title: Option<String>,
    completed_paragraphs_json: Option<String>,
    completed_at_unix_ms: Option<i64>,
    comparison_baseline_title: String,
    comparison_baseline_paragraphs_json: String,
    comparison_baseline_revision: Option<i64>,
}

pub(crate) struct WritingStore {
    connection: Connection,
}

impl WritingStore {
    fn open_for_app(app: &AppHandle) -> Result<Self, String> {
        Ok(Self {
            connection: open_database_for_app(app)?,
        })
    }

    #[cfg(test)]
    fn open_path(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self {
            connection: crate::learning_records::open_database(path)?,
        })
    }

    fn create(&mut self) -> Result<WritingDocumentRecord, String> {
        let timestamp = unix_time_ms()?;
        let empty = WritingSnapshot {
            title: String::new(),
            paragraphs: vec![String::new()],
        };
        let paragraphs_json = encode_paragraphs(&empty.paragraphs)?;
        self.connection
            .execute(
                "INSERT INTO writing_documents (
                    revision, created_at_unix_ms, updated_at_unix_ms, last_opened_at_unix_ms,
                    draft_title, draft_paragraphs_json, draft_updated_at_unix_ms,
                    comparison_baseline_title, comparison_baseline_paragraphs_json,
                    comparison_baseline_revision
                 ) VALUES (0, ?1, ?1, ?1, ?2, ?3, ?1, ?2, ?3, 0)",
                params![timestamp, empty.title, paragraphs_json],
            )
            .map_err(|error| format!("写作文章创建失败：{error}"))?;
        self.get_required(self.connection.last_insert_rowid(), false)
    }

    fn list(&self, query: Option<&str>) -> Result<Vec<WritingDocumentSummary>, String> {
        let normalized_query = query.map(str::trim).filter(|value| !value.is_empty());
        if normalized_query.is_some_and(|value| value.chars().count() > WRITING_MAX_QUESTION_CHARS)
        {
            return Err("写作文章搜索关键词过长。".to_string());
        }

        let filter = if normalized_query.is_some() {
            "WHERE instr(
                lower(COALESCE(draft_title, '')),
                lower(?1)
             ) > 0 OR instr(
                lower(COALESCE(draft_paragraphs_json, '')),
                lower(?1)
             ) > 0 OR instr(
                lower(COALESCE(completed_title, '')),
                lower(?1)
             ) > 0 OR instr(
                lower(COALESCE(completed_paragraphs_json, '')),
                lower(?1)
             ) > 0"
        } else {
            ""
        };
        let sql = format!(
            "{} {filter} ORDER BY updated_at_unix_ms DESC, id DESC",
            document_select_sql()
        );
        let mut statement = self
            .connection
            .prepare(&sql)
            .map_err(|error| format!("写作文章列表语句无法准备：{error}"))?;
        let mut summaries = Vec::new();
        if let Some(query) = normalized_query {
            let rows = statement
                .query_map([query], read_stored_document)
                .map_err(|error| format!("写作文章搜索失败：{error}"))?;
            for row in rows {
                summaries.push(decode_document_summary(
                    row.map_err(|error| format!("写作文章搜索行读取失败：{error}"))?,
                )?);
            }
        } else {
            let rows = statement
                .query_map([], read_stored_document)
                .map_err(|error| format!("写作文章列表读取失败：{error}"))?;
            for row in rows {
                summaries.push(decode_document_summary(
                    row.map_err(|error| format!("写作文章列表行读取失败：{error}"))?,
                )?);
            }
        }
        Ok(summaries)
    }

    fn get(
        &mut self,
        document_id: i64,
        touch_last_opened: bool,
    ) -> Result<Option<WritingDocumentRecord>, String> {
        if touch_last_opened {
            self.connection
                .execute(
                    "UPDATE writing_documents SET last_opened_at_unix_ms = ?1 WHERE id = ?2",
                    params![unix_time_ms()?, document_id],
                )
                .map_err(|error| format!("写作文章最近打开时间更新失败：{error}"))?;
        }
        let stored = self
            .connection
            .query_row(
                &format!("{} WHERE id = ?1", document_select_sql()),
                [document_id],
                read_stored_document,
            )
            .optional()
            .map_err(|error| format!("写作文章读取失败：{error}"))?;
        stored
            .map(|stored| self.decode_document(stored))
            .transpose()
    }

    fn get_required(
        &mut self,
        document_id: i64,
        touch_last_opened: bool,
    ) -> Result<WritingDocumentRecord, String> {
        self.get(document_id, touch_last_opened)?
            .ok_or_else(|| format!("写作文章不存在：id={document_id}"))
    }

    fn save_draft(
        &mut self,
        document_id: i64,
        expected_revision: i64,
        snapshot: &WritingSnapshot,
    ) -> Result<WritingDocumentRecord, String> {
        validate_snapshot(snapshot, true)?;
        let timestamp = unix_time_ms()?;
        let paragraphs_json = encode_paragraphs(&snapshot.paragraphs)?;
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("写作草稿保存事务无法开始：{error}"))?;
        let updated = transaction
            .execute(
                "UPDATE writing_documents
                 SET revision = revision + 1,
                     updated_at_unix_ms = ?1,
                     draft_title = ?2,
                     draft_paragraphs_json = ?3,
                     draft_updated_at_unix_ms = ?1
                 WHERE id = ?4 AND revision = ?5 AND draft_title IS NOT NULL",
                params![
                    timestamp,
                    snapshot.title,
                    paragraphs_json,
                    document_id,
                    expected_revision
                ],
            )
            .map_err(|error| format!("写作草稿保存失败：{error}"))?;
        if updated == 0 {
            return Err(write_conflict(
                &transaction,
                document_id,
                expected_revision,
                "保存草稿",
            )?);
        }
        transaction
            .commit()
            .map_err(|error| format!("写作草稿保存事务无法提交：{error}"))?;
        self.get_required(document_id, false)
    }

    fn delete(&mut self, document_id: i64, expected_revision: i64) -> Result<bool, String> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("写作文章删除事务无法开始：{error}"))?;
        let deleted = transaction
            .execute(
                "DELETE FROM writing_documents WHERE id = ?1 AND revision = ?2",
                params![document_id, expected_revision],
            )
            .map_err(|error| format!("写作文章删除失败：{error}"))?;
        if deleted == 0 {
            let current = current_revision(&transaction, document_id)?;
            if let Some(current) = current {
                return Err(format!(
                    "写作文章版本冲突，无法删除：期望 revision={expected_revision}，当前 revision={current}。"
                ));
            }
            return Ok(false);
        }
        transaction
            .commit()
            .map_err(|error| format!("写作文章删除事务无法提交：{error}"))?;
        Ok(true)
    }

    fn complete(
        &mut self,
        document_id: i64,
        expected_revision: i64,
    ) -> Result<WritingDocumentRecord, String> {
        let timestamp = unix_time_ms()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("完成写作事务无法开始：{error}"))?;
        let stored = transaction
            .query_row(
                &format!("{} WHERE id = ?1", document_select_sql()),
                [document_id],
                read_stored_document,
            )
            .optional()
            .map_err(|error| format!("完成写作读取文章失败：{error}"))?
            .ok_or_else(|| format!("写作文章不存在：id={document_id}"))?;
        if stored.revision != expected_revision {
            return Err(format!(
                "写作文章版本冲突，无法完成写作：期望 revision={expected_revision}，当前 revision={}。",
                stored.revision
            ));
        }
        let baseline_title = stored.comparison_baseline_title.clone();
        let baseline_json = stored.comparison_baseline_paragraphs_json.clone();
        let baseline_revision = stored.comparison_baseline_revision;
        let snapshot = decode_document_summary(stored)?
            .draft_snapshot
            .ok_or_else(|| "当前文章没有可完成的草稿。".to_string())?;
        validate_snapshot(&snapshot, false)?;
        let paragraphs_json = encode_paragraphs(&snapshot.paragraphs)?;
        let analysis_lookup_revision = baseline_revision.unwrap_or(expected_revision);
        let analysis_json = transaction
            .query_row(
                "SELECT analysis_json
                 FROM writing_analyses
                 WHERE document_id = ?1 AND document_revision = ?2
                 ORDER BY round DESC LIMIT 1",
                params![document_id, analysis_lookup_revision],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("完成版本分析读取失败：{error}"))?;
        let analysis_revision = analysis_json.as_ref().map(|_| analysis_lookup_revision);
        let updated = transaction
            .execute(
                "UPDATE writing_documents
                 SET revision = revision + 1,
                     updated_at_unix_ms = ?1,
                     draft_title = NULL,
                     draft_paragraphs_json = NULL,
                     draft_updated_at_unix_ms = NULL,
                     completed_title = ?2,
                     completed_paragraphs_json = ?3,
                     completed_at_unix_ms = ?1
                 WHERE id = ?4 AND revision = ?5 AND draft_title IS NOT NULL",
                params![
                    timestamp,
                    snapshot.title,
                    paragraphs_json,
                    document_id,
                    expected_revision
                ],
            )
            .map_err(|error| format!("完成稿保存失败：{error}"))?;
        if updated == 0 {
            return Err(write_conflict(
                &transaction,
                document_id,
                expected_revision,
                "完成写作",
            )?);
        }
        let ordinal: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(ordinal), 0) + 1
                 FROM writing_versions WHERE document_id = ?1",
                [document_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("写作版本序号读取失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO writing_versions (
                    document_id, ordinal, source_revision, title, paragraphs_json,
                    comparison_baseline_title, comparison_baseline_paragraphs_json,
                    analysis_json, completed_at_unix_ms, analysis_revision,
                    comparison_baseline_revision
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    document_id,
                    ordinal,
                    expected_revision,
                    snapshot.title,
                    paragraphs_json,
                    baseline_title,
                    baseline_json,
                    analysis_json,
                    timestamp,
                    analysis_revision,
                    baseline_revision
                ],
            )
            .map_err(|error| format!("不可变写作版本保存失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("完成写作事务无法提交：{error}"))?;
        self.get_required(document_id, false)
    }

    fn continue_editing(
        &mut self,
        document_id: i64,
        expected_revision: i64,
        version_id: Option<i64>,
    ) -> Result<WritingDocumentRecord, String> {
        let record = self.get_required(document_id, false)?;
        require_revision(&record, expected_revision, "继续修改")?;
        if record.summary.draft_snapshot.is_some() {
            return Err(
                "当前文章已有修改中草稿，已拒绝用完成版本覆盖；请返回现有草稿。".to_string(),
            );
        }
        let version = match version_id {
            Some(version_id) => record
                .versions
                .iter()
                .find(|version| version.id == version_id)
                .cloned()
                .ok_or_else(|| format!("写作版本不存在：id={version_id}"))?,
            None => record
                .versions
                .last()
                .cloned()
                .ok_or_else(|| "当前文章没有可继续修改的完成版本。".to_string())?,
        };
        let timestamp = unix_time_ms()?;
        let paragraphs_json = encode_paragraphs(&version.snapshot.paragraphs)?;
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("继续修改事务无法开始：{error}"))?;
        let updated = transaction
            .execute(
                "UPDATE writing_documents
                 SET revision = revision + 1,
                     updated_at_unix_ms = ?1,
                     draft_title = ?2,
                     draft_paragraphs_json = ?3,
                     draft_updated_at_unix_ms = ?1,
                     comparison_baseline_title = ?2,
                     comparison_baseline_paragraphs_json = ?3,
                     comparison_baseline_revision = revision + 1
                 WHERE id = ?4 AND revision = ?5 AND draft_title IS NULL",
                params![
                    timestamp,
                    version.snapshot.title,
                    paragraphs_json,
                    document_id,
                    expected_revision
                ],
            )
            .map_err(|error| format!("继续修改草稿创建失败：{error}"))?;
        if updated == 0 {
            return Err(write_conflict(
                &transaction,
                document_id,
                expected_revision,
                "继续修改",
            )?);
        }
        transaction
            .commit()
            .map_err(|error| format!("继续修改事务无法提交：{error}"))?;
        self.get_required(document_id, false)
    }

    fn save_analysis_if_current(
        &mut self,
        document_id: i64,
        expected_revision: i64,
        snapshot: &WritingSnapshot,
        content: &WritingAnalysisContent,
    ) -> Result<WritingDocumentRecord, String> {
        validate_analysis_content(snapshot, content)?;
        let timestamp = unix_time_ms()?;
        let analysis_json = serde_json::to_string(content)
            .map_err(|error| format!("写作分析结果无法序列化：{error}"))?;
        let baseline_json = encode_paragraphs(&snapshot.paragraphs)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("写作分析保存事务无法开始：{error}"))?;
        let current = current_revision(&transaction, document_id)?
            .ok_or_else(|| format!("写作文章不存在：id={document_id}"))?;
        if current != expected_revision {
            return Err(format!(
                "写作分析结果已过期，未保存：请求 revision={expected_revision}，当前 revision={current}。"
            ));
        }
        let new_revision = expected_revision + 1;
        let updated = transaction
            .execute(
                "UPDATE writing_documents
                 SET revision = revision + 1,
                     comparison_baseline_title = ?1,
                     comparison_baseline_paragraphs_json = ?2,
                     comparison_baseline_revision = revision + 1,
                     updated_at_unix_ms = ?3
                 WHERE id = ?4 AND revision = ?5 AND draft_title IS NOT NULL",
                params![
                    snapshot.title,
                    baseline_json,
                    timestamp,
                    document_id,
                    expected_revision
                ],
            )
            .map_err(|error| format!("写作对比基线保存失败：{error}"))?;
        if updated == 0 {
            return Err(write_conflict(
                &transaction,
                document_id,
                expected_revision,
                "保存写作分析",
            )?);
        }
        let round: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(round), 0) + 1
                 FROM writing_analyses WHERE document_id = ?1",
                [document_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("写作分析轮次读取失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO writing_analyses (
                    document_id, document_revision, round, analysis_json,
                    schema_version, created_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    document_id,
                    new_revision,
                    round,
                    analysis_json,
                    WRITING_ANALYSIS_SCHEMA_VERSION,
                    timestamp
                ],
            )
            .map_err(|error| format!("写作分析结果保存失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("写作分析保存事务无法提交：{error}"))?;
        self.get_required(document_id, false)
    }

    fn save_answer_if_current(
        &mut self,
        request: &WritingQuestionRequest,
        content: &WritingAnswerContent,
    ) -> Result<WritingAgentAnswer, String> {
        validate_answer_content(content)?;
        let timestamp = unix_time_ms()?;
        let answer_json = serde_json::to_string(content)
            .map_err(|error| format!("写作辅助回答无法序列化：{error}"))?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("写作辅助回答保存事务无法开始：{error}"))?;
        let current = current_revision(&transaction, request.document_id)?
            .ok_or_else(|| format!("写作文章不存在：id={}", request.document_id))?;
        if current != request.expected_revision {
            return Err(format!(
                "写作辅助回答已过期，未保存：请求 revision={}，当前 revision={current}。",
                request.expected_revision
            ));
        }
        let target_revision = match request.version_id {
            Some(version_id) => transaction
                .query_row(
                    "SELECT source_revision FROM writing_versions
                     WHERE id = ?1 AND document_id = ?2",
                    params![version_id, request.document_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| format!("写作辅助目标版本读取失败：{error}"))?
                .ok_or_else(|| format!("写作版本不存在：id={version_id}"))?,
            None => request.expected_revision,
        };
        let draft_session_start_revision = transaction
            .query_row(
                "SELECT MAX(source_revision) FROM writing_versions WHERE document_id = ?1",
                [request.document_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|error| format!("写作草稿辅导会话边界读取失败：{error}"))?
            .unwrap_or(-1);
        if let Some(parent_answer_id) = request.parent_answer_id {
            let parent_target: Option<(i64, Option<i64>)> = transaction
                .query_row(
                    "SELECT document_revision, version_id
                     FROM writing_assistant_answers
                     WHERE id = ?1 AND document_id = ?2",
                    params![parent_answer_id, request.document_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|error| format!("写作追问目标读取失败：{error}"))?;
            let parent_matches =
                parent_target.is_some_and(|(parent_revision, parent_version_id)| {
                    answer_matches_visible_target(
                        parent_revision,
                        parent_version_id,
                        request.version_id,
                        target_revision,
                        draft_session_start_revision,
                    )
                });
            if !parent_matches {
                return Err("追问目标不属于当前可见文章版本。".to_string());
            }
        }
        transaction
            .execute(
                "INSERT INTO writing_assistant_answers (
                    document_id, document_revision, version_id, parent_answer_id,
                    question, scope, selection_text, answer_json, schema_version,
                    created_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    request.document_id,
                    target_revision,
                    request.version_id,
                    request.parent_answer_id,
                    request.question.trim(),
                    request.scope.storage_value(),
                    request.selection_text.as_deref(),
                    answer_json,
                    WRITING_ANSWER_SCHEMA_VERSION,
                    timestamp
                ],
            )
            .map_err(|error| format!("写作辅助回答保存失败：{error}"))?;
        let answer_id = transaction.last_insert_rowid();
        transaction
            .commit()
            .map_err(|error| format!("写作辅助回答保存事务无法提交：{error}"))?;
        self.get_answer(answer_id)?
            .ok_or_else(|| "写作辅助回答保存后无法读取。".to_string())
    }

    fn decode_document(&self, stored: StoredDocument) -> Result<WritingDocumentRecord, String> {
        let baseline = decode_required_snapshot(
            stored.comparison_baseline_title.clone(),
            stored.comparison_baseline_paragraphs_json.clone(),
            "写作对比基线",
        )?;
        let comparison_baseline_revision = stored.comparison_baseline_revision;
        let summary = decode_document_summary(stored)?;
        let active_analysis = self.latest_analysis(summary.id, summary.revision)?;
        let baseline_analysis = match comparison_baseline_revision {
            Some(revision) if revision == summary.revision => active_analysis.clone(),
            Some(revision) => self.latest_analysis(summary.id, revision)?,
            None => None,
        };
        Ok(WritingDocumentRecord {
            versions: self.list_versions(summary.id)?,
            active_analysis,
            baseline_analysis,
            answers: self.list_answers(summary.id)?,
            summary,
            comparison_baseline: baseline,
            comparison_baseline_revision,
        })
    }

    fn list_versions(&self, document_id: i64) -> Result<Vec<WritingVersion>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, document_id, ordinal, source_revision, title, paragraphs_json,
                        comparison_baseline_title, comparison_baseline_paragraphs_json,
                        analysis_json, completed_at_unix_ms, analysis_revision,
                        comparison_baseline_revision
                 FROM writing_versions
                 WHERE document_id = ?1
                 ORDER BY ordinal ASC",
            )
            .map_err(|error| format!("写作版本列表语句无法准备：{error}"))?;
        let rows = statement
            .query_map([document_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                ))
            })
            .map_err(|error| format!("写作版本列表读取失败：{error}"))?;
        let mut versions = Vec::new();
        for row in rows {
            let (
                id,
                document_id,
                ordinal,
                source_revision,
                title,
                paragraphs_json,
                baseline_title,
                baseline_paragraphs_json,
                analysis_json,
                completed_at_unix_ms,
                analysis_revision,
                comparison_baseline_revision,
            ) = row.map_err(|error| format!("写作版本行读取失败：{error}"))?;
            let analysis = analysis_json
                .map(|value| {
                    serde_json::from_str::<WritingAnalysisContent>(&value)
                        .map_err(|error| format!("写作版本 {id} 的分析 JSON 无法解析：{error}"))
                })
                .transpose()?;
            let (issues, patterns) = analysis
                .map(|content| (content.issues, content.patterns))
                .unwrap_or_default();
            versions.push(WritingVersion {
                id,
                document_id,
                ordinal,
                source_revision,
                analysis_revision,
                comparison_baseline_revision,
                snapshot: decode_required_snapshot(title, paragraphs_json, "写作完成版本")?,
                comparison_baseline: decode_required_snapshot(
                    baseline_title,
                    baseline_paragraphs_json,
                    "写作版本对比基线",
                )?,
                issues,
                patterns,
                completed_at_unix_ms,
            });
        }
        Ok(versions)
    }

    fn latest_analysis(
        &self,
        document_id: i64,
        document_revision: i64,
    ) -> Result<Option<WritingAnalysis>, String> {
        let stored = self
            .connection
            .query_row(
                "SELECT id, document_id, document_revision, round, analysis_json,
                        created_at_unix_ms
                 FROM writing_analyses
                 WHERE document_id = ?1 AND document_revision = ?2
                 ORDER BY round DESC
                 LIMIT 1",
                params![document_id, document_revision],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("写作分析读取失败：{error}"))?;
        stored
            .map(
                |(id, document_id, document_revision, round, analysis_json, created_at_unix_ms)| {
                    let content: WritingAnalysisContent = serde_json::from_str(&analysis_json)
                        .map_err(|error| format!("写作分析 {id} 的 JSON 无法解析：{error}"))?;
                    Ok(WritingAnalysis {
                        id,
                        document_id,
                        document_revision,
                        round,
                        issues: content.issues,
                        patterns: content.patterns,
                        created_at_unix_ms,
                    })
                },
            )
            .transpose()
    }

    fn list_answers(&self, document_id: i64) -> Result<Vec<WritingAgentAnswer>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, document_id, document_revision, parent_answer_id, question, scope,
                        selection_text, answer_json, created_at_unix_ms, version_id
                 FROM writing_assistant_answers
                 WHERE document_id = ?1
                 ORDER BY created_at_unix_ms ASC, id ASC",
            )
            .map_err(|error| format!("写作辅助历史语句无法准备：{error}"))?;
        let rows = statement
            .query_map([document_id], read_stored_answer)
            .map_err(|error| format!("写作辅助历史读取失败：{error}"))?;
        let mut answers = Vec::new();
        for row in rows {
            answers.push(decode_answer(
                row.map_err(|error| format!("写作辅助历史行读取失败：{error}"))?,
            )?);
        }
        Ok(answers)
    }

    fn get_answer(&self, answer_id: i64) -> Result<Option<WritingAgentAnswer>, String> {
        let stored = self
            .connection
            .query_row(
                "SELECT id, document_id, document_revision, parent_answer_id, question, scope,
                        selection_text, answer_json, created_at_unix_ms, version_id
                 FROM writing_assistant_answers
                 WHERE id = ?1",
                [answer_id],
                read_stored_answer,
            )
            .optional()
            .map_err(|error| format!("写作辅助回答读取失败：{error}"))?;
        stored.map(decode_answer).transpose()
    }
}

type StoredAnswer = (
    i64,
    i64,
    i64,
    Option<i64>,
    String,
    String,
    Option<String>,
    String,
    i64,
    Option<i64>,
);

fn read_stored_answer(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAnswer> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

fn decode_answer(stored: StoredAnswer) -> Result<WritingAgentAnswer, String> {
    let (
        id,
        document_id,
        document_revision,
        parent_answer_id,
        question,
        scope,
        selection_text,
        answer_json,
        created_at_unix_ms,
        version_id,
    ) = stored;
    let scope = WritingQuestionScope::from_storage(&scope)?;
    let content: WritingAnswerContent = serde_json::from_str(&answer_json)
        .map_err(|error| format!("写作辅助回答 {id} 的 JSON 无法解析：{error}"))?;
    Ok(WritingAgentAnswer {
        id,
        document_id,
        document_revision,
        version_id,
        parent_answer_id,
        question,
        scope,
        scope_label: scope.label().to_string(),
        selection_text,
        title: content.title,
        copy: content.copy,
        map: content.map,
        created_at_unix_ms,
    })
}

fn document_select_sql() -> &'static str {
    "SELECT id, revision, created_at_unix_ms, updated_at_unix_ms,
            last_opened_at_unix_ms, draft_title, draft_paragraphs_json,
            draft_updated_at_unix_ms, completed_title, completed_paragraphs_json,
            completed_at_unix_ms, comparison_baseline_title,
            comparison_baseline_paragraphs_json, comparison_baseline_revision
     FROM writing_documents"
}

fn read_stored_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredDocument> {
    Ok(StoredDocument {
        id: row.get(0)?,
        revision: row.get(1)?,
        created_at_unix_ms: row.get(2)?,
        updated_at_unix_ms: row.get(3)?,
        last_opened_at_unix_ms: row.get(4)?,
        draft_title: row.get(5)?,
        draft_paragraphs_json: row.get(6)?,
        draft_updated_at_unix_ms: row.get(7)?,
        completed_title: row.get(8)?,
        completed_paragraphs_json: row.get(9)?,
        completed_at_unix_ms: row.get(10)?,
        comparison_baseline_title: row.get(11)?,
        comparison_baseline_paragraphs_json: row.get(12)?,
        comparison_baseline_revision: row.get(13)?,
    })
}

fn decode_document_summary(stored: StoredDocument) -> Result<WritingDocumentSummary, String> {
    let draft_snapshot =
        decode_optional_snapshot(stored.draft_title, stored.draft_paragraphs_json, "写作草稿")?;
    let completed_snapshot = decode_optional_snapshot(
        stored.completed_title,
        stored.completed_paragraphs_json,
        "写作完成稿",
    )?;
    if draft_snapshot.is_none() && completed_snapshot.is_none() {
        return Err(format!("写作文章 {} 同时缺少草稿和完成稿。", stored.id));
    }
    Ok(WritingDocumentSummary {
        id: stored.id,
        revision: stored.revision,
        created_at_unix_ms: stored.created_at_unix_ms,
        updated_at_unix_ms: stored.updated_at_unix_ms,
        last_opened_at_unix_ms: stored.last_opened_at_unix_ms,
        draft_updated_at_unix_ms: stored.draft_updated_at_unix_ms,
        completed_at_unix_ms: stored.completed_at_unix_ms,
        draft_snapshot,
        completed_snapshot,
    })
}

fn decode_optional_snapshot(
    title: Option<String>,
    paragraphs_json: Option<String>,
    label: &str,
) -> Result<Option<WritingSnapshot>, String> {
    match (title, paragraphs_json) {
        (None, None) => Ok(None),
        (Some(title), Some(paragraphs_json)) => {
            decode_required_snapshot(title, paragraphs_json, label).map(Some)
        }
        _ => Err(format!("{label}的标题与正文保存状态不一致。")),
    }
}

fn decode_required_snapshot(
    title: String,
    paragraphs_json: String,
    label: &str,
) -> Result<WritingSnapshot, String> {
    let paragraphs: Vec<String> = serde_json::from_str(&paragraphs_json)
        .map_err(|error| format!("{label}正文 JSON 无法解析：{error}"))?;
    let snapshot = WritingSnapshot { title, paragraphs };
    validate_snapshot(&snapshot, true)?;
    Ok(snapshot)
}

fn encode_paragraphs(paragraphs: &[String]) -> Result<String, String> {
    serde_json::to_string(paragraphs).map_err(|error| format!("写作正文无法序列化：{error}"))
}

fn validate_snapshot(snapshot: &WritingSnapshot, allow_blank: bool) -> Result<(), String> {
    if snapshot.title.chars().count() > WRITING_MAX_TITLE_CHARS {
        return Err(format!(
            "写作标题不能超过 {WRITING_MAX_TITLE_CHARS} 个字符。"
        ));
    }
    if snapshot.paragraphs.is_empty() || snapshot.paragraphs.len() > WRITING_MAX_PARAGRAPHS {
        return Err(format!(
            "写作正文段落数量必须在 1 到 {WRITING_MAX_PARAGRAPHS} 之间。"
        ));
    }
    let mut total = snapshot.title.chars().count();
    for paragraph in &snapshot.paragraphs {
        let length = paragraph.chars().count();
        if length > WRITING_MAX_PARAGRAPH_CHARS {
            return Err(format!(
                "写作单段正文不能超过 {WRITING_MAX_PARAGRAPH_CHARS} 个字符。"
            ));
        }
        total += length;
    }
    if total > WRITING_MAX_TOTAL_CHARS {
        return Err(format!(
            "写作文章总长度不能超过 {WRITING_MAX_TOTAL_CHARS} 个字符。"
        ));
    }
    if !allow_blank
        && snapshot.title.trim().is_empty()
        && snapshot
            .paragraphs
            .iter()
            .all(|paragraph| paragraph.trim().is_empty())
    {
        return Err("写作正文为空，无法执行当前操作。".to_string());
    }
    Ok(())
}

fn validate_analysis_content(
    snapshot: &WritingSnapshot,
    content: &WritingAnalysisContent,
) -> Result<(), String> {
    if content.issues.len() > WRITING_MAX_ISSUES {
        return Err(format!(
            "写作分析问题数量不能超过 {WRITING_MAX_ISSUES} 个。"
        ));
    }
    if content.patterns.len() > WRITING_MAX_PATTERNS {
        return Err(format!(
            "写作分析模式数量不能超过 {WRITING_MAX_PATTERNS} 个。"
        ));
    }
    let body = snapshot.paragraphs.join("\n");
    let mut issue_ids = HashSet::new();
    for issue in &content.issues {
        validate_text_field("问题 ID", &issue.id, 80)?;
        if !issue_ids.insert(issue.id.as_str()) {
            return Err(format!("写作分析包含重复问题 ID：{}", issue.id));
        }
        validate_text_field("问题类别", &issue.category, 80)?;
        validate_text_field("问题原文", &issue.source, 1_000)?;
        validate_text_field("问题定位文本", &issue.target_text, 500)?;
        validate_text_field("问题说明", &issue.explanation, 1_500)?;
        validate_text_field("问题提示", &issue.hint, 800)?;
        validate_text_field("进一步提示", &issue.deeper_hint, 1_200)?;
        validate_text_field("参考表达", &issue.reference, 1_200)?;
        if !body.contains(&issue.target_text) {
            return Err(format!(
                "写作分析问题 {} 的 targetText 不存在于送检正文中，标题定位不受支持。",
                issue.id
            ));
        }
        if !body.contains(&issue.source) {
            return Err(format!(
                "写作分析问题 {} 的 source 不存在于送检正文中。",
                issue.id
            ));
        }
        if !issue.source.contains(&issue.target_text) && !issue.target_text.contains(&issue.source)
        {
            return Err(format!(
                "写作分析问题 {} 的 source 与 targetText 无法验证为同一处原文。",
                issue.id
            ));
        }
    }
    let mut pattern_ids = HashSet::new();
    for pattern in &content.patterns {
        validate_text_field("模式 ID", &pattern.id, 80)?;
        if !pattern_ids.insert(pattern.id.as_str()) {
            return Err(format!("写作分析包含重复模式 ID：{}", pattern.id));
        }
        validate_text_field("模式标题", &pattern.title, 200)?;
        validate_text_field("模式说明", &pattern.description, 1_000)?;
    }
    Ok(())
}

fn validate_answer_content(content: &WritingAnswerContent) -> Result<(), String> {
    validate_text_field("回答标题", &content.title, 300)?;
    validate_text_field("回答正文", &content.copy, 4_000)?;
    if let Some(map) = &content.map {
        validate_text_field("写作地图核心", &map.core, 1_500)?;
        validate_string_list("写作地图问题", &map.questions, 5, 500)?;
        validate_string_list("写作地图表达", &map.phrases, 10, 200)?;
        validate_string_list("写作地图起笔句式", &map.starters, 5, 500)?;
    }
    Ok(())
}

fn validate_string_list(
    label: &str,
    values: &[String],
    maximum: usize,
    max_chars: usize,
) -> Result<(), String> {
    if values.len() > maximum {
        return Err(format!("{label}数量不能超过 {maximum} 个。"));
    }
    for value in values {
        validate_text_field(label, value, max_chars)?;
    }
    Ok(())
}

fn validate_text_field(label: &str, value: &str, max_chars: usize) -> Result<(), String> {
    let length = value.trim().chars().count();
    if length == 0 {
        return Err(format!("{label}不能为空。"));
    }
    if length > max_chars {
        return Err(format!("{label}不能超过 {max_chars} 个字符。"));
    }
    Ok(())
}

fn validate_question(
    snapshot: &WritingSnapshot,
    request: &WritingQuestionRequest,
    parent: Option<&WritingAgentAnswer>,
    target_revision: i64,
    draft_session_start_revision: i64,
) -> Result<(), String> {
    validate_text_field(
        "写作辅助问题",
        &request.question,
        WRITING_MAX_QUESTION_CHARS,
    )?;
    match (request.scope, request.selection_text.as_deref()) {
        (WritingQuestionScope::Selection, Some(selection)) => {
            validate_text_field("所选内容", selection, WRITING_MAX_SELECTION_CHARS)?;
            let article = format!("{}\n{}", snapshot.title, snapshot.paragraphs.join("\n"));
            if !article.contains(selection.trim()) {
                return Err("所选内容已不在当前文章中，请重新选择后再提问。".to_string());
            }
        }
        (WritingQuestionScope::Selection, None) => {
            return Err("选区问答缺少所选内容。".to_string());
        }
        (_, Some(_)) => return Err("只有选区问答可以携带所选内容。".to_string()),
        (_, None) => {}
    }
    if let Some(parent) = parent {
        if parent.document_id != request.document_id
            || !answer_matches_visible_target(
                parent.document_revision,
                parent.version_id,
                request.version_id,
                target_revision,
                draft_session_start_revision,
            )
        {
            return Err("追问目标不属于当前可见文章版本。".to_string());
        }
    } else if request.parent_answer_id.is_some() {
        return Err("追问目标不存在。".to_string());
    }
    Ok(())
}

fn answer_matches_visible_target(
    answer_revision: i64,
    answer_version_id: Option<i64>,
    request_version_id: Option<i64>,
    target_revision: i64,
    draft_session_start_revision: i64,
) -> bool {
    match request_version_id {
        Some(version_id) => {
            answer_version_id == Some(version_id) && answer_revision == target_revision
        }
        None => {
            answer_version_id.is_none()
                && answer_revision > draft_session_start_revision
                && answer_revision <= target_revision
        }
    }
}

fn collect_visible_answer_context(
    answers: &[WritingAgentAnswer],
    request: &WritingQuestionRequest,
    target_revision: i64,
    draft_session_start_revision: i64,
) -> Result<Vec<WritingAgentAnswer>, String> {
    let mut context = Vec::new();
    let mut next_answer_id = request.parent_answer_id;
    let mut visited = HashSet::new();
    while let Some(answer_id) = next_answer_id {
        if !visited.insert(answer_id) {
            return Err("写作辅导问答链存在循环，已拒绝调用模型。".to_string());
        }
        let answer = answers
            .iter()
            .find(|candidate| candidate.id == answer_id)
            .ok_or_else(|| "追问目标不存在。".to_string())?;
        if answer.document_id != request.document_id
            || !answer_matches_visible_target(
                answer.document_revision,
                answer.version_id,
                request.version_id,
                target_revision,
                draft_session_start_revision,
            )
        {
            return Err("追问目标不属于当前可见文章版本。".to_string());
        }
        context.push(answer.clone());
        if context.len() == WRITING_MAX_CONVERSATION_CONTEXT_ANSWERS {
            break;
        }
        next_answer_id = answer.parent_answer_id;
    }
    context.reverse();
    Ok(context)
}

fn require_revision(
    record: &WritingDocumentRecord,
    expected_revision: i64,
    operation: &str,
) -> Result<(), String> {
    if record.summary.revision != expected_revision {
        return Err(format!(
            "写作文章版本冲突，无法{operation}：期望 revision={expected_revision}，当前 revision={}。",
            record.summary.revision
        ));
    }
    Ok(())
}

fn current_revision(
    transaction: &Transaction<'_>,
    document_id: i64,
) -> Result<Option<i64>, String> {
    transaction
        .query_row(
            "SELECT revision FROM writing_documents WHERE id = ?1",
            [document_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("写作文章当前版本读取失败：{error}"))
}

fn write_conflict(
    transaction: &Transaction<'_>,
    document_id: i64,
    expected_revision: i64,
    operation: &str,
) -> Result<String, String> {
    match current_revision(transaction, document_id)? {
        Some(current) => Ok(format!(
            "写作文章版本冲突，无法{operation}：期望 revision={expected_revision}，当前 revision={current}。"
        )),
        None => Ok(format!("写作文章不存在：id={document_id}")),
    }
}

/// 对可见正文计算稳定短摘要，用于写入 `AuthorityRef::writing`。写作结果不因
/// 正文短变化跨轮伪造身份；这里只作事件承载，真实迟到边界仍由
/// `save_analysis_if_current` / `save_answer_if_current` 的 expectedRevision 在
/// 事务内强制。
fn writing_snapshot_digest(snapshot: &WritingSnapshot) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    snapshot.title.hash(&mut hasher);
    for paragraph in &snapshot.paragraphs {
        paragraph.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// 经共享 Agent Runtime 内核执行写作检查（流式状态 + 可取消）。
///
/// 产出 `[System, User]` 上下文（复用 `WritingSurfaceAdapter` 与写作专项系统
/// 提示词），驱动 `AgentRunCoordinator` + `DeepSeekChatGateway`，最终仍收口为
/// 结构化 JSON，经 `parse_writing_analysis_content` 校验后才返回给调用方
/// （`analyze_writing_document_with` 进一步做 expectedRevision 保存）。终端事件
/// 由本函数依据真实结果发送；中间进度经 `WritingUiSink` 发布。
async fn request_writing_analysis_via_runtime(
    app: AppHandle,
    document_id: i64,
    expected_revision: i64,
    sender: WritingStreamSender,
    abort_flag: Arc<AtomicBool>,
    snapshot: WritingSnapshot,
) -> Result<WritingAnalysisContent, String> {
    let article_json =
        serde_json::to_string(&snapshot).map_err(|error| format!("写作文章无法序列化：{error}"))?;
    let user_content = format!("请检查下面这篇英文文章。文章 JSON：\n{article_json}");
    let adapter = WritingSurfaceAdapter::new(writing_analysis_system_prompt());
    let transcript = adapter.transcript(&user_content);

    let labels = WritingStageLabels {
        start: "正在检查语法…",
        analyzing: "正在判断表达是否地道、更正式…",
        finishing: "正在整理检查结果…",
    };
    let snapshot_digest = writing_snapshot_digest(&snapshot);
    let outcome = run_writing_runtime(
        app,
        document_id,
        expected_revision,
        None,
        snapshot_digest,
        sender.clone(),
        abort_flag,
        transcript,
        labels,
        ModelUsageCategory::Writing,
        "DeepSeek 写作检查",
    )
    .await?;

    match outcome.termination {
        TerminationReason::FinalAnswer => {
            let final_text = outcome.final_text.unwrap_or_default();
            match parse_writing_analysis_content_salvage(&snapshot, &final_text) {
                Ok(content) => {
                    sender.send(WritingStreamEvent::Done);
                    Ok(content)
                }
                Err(error) => {
                    sender.send(WritingStreamEvent::Error {
                        message: "检查结果暂时无法解析，请重试。".to_string(),
                    });
                    Err(error)
                }
            }
        }
        TerminationReason::UserAborted => {
            sender.send(WritingStreamEvent::Stopped);
            Err("检查已停止，可重新检查。".to_string())
        }
        TerminationReason::RunBudgetExceeded => {
            sender.send(WritingStreamEvent::Error {
                message: "检查未完成，可重新检查。".to_string(),
            });
            Err("检查未完成，已保留你的文章，可重新检查。".to_string())
        }
        other => {
            let message = outcome
                .error
                .map(|error| error.message)
                .unwrap_or_else(|| "Agent run 失败。".to_string());
            eprintln!("READRAY_WRITING_RUN_FAILED=termination={other:?} error={message}");
            sender.send(WritingStreamEvent::Error {
                message: "暂时无法完成检查，请重试。".to_string(),
            });
            Err("暂时无法完成检查，请重试。".to_string())
        }
    }
}

/// 共享的写作 Runtime 执行：在 blocking 线程同步驱动 coordinator，返回终态。
async fn run_writing_runtime(
    app: AppHandle,
    document_id: i64,
    expected_revision: i64,
    version_id: Option<i64>,
    digest: String,
    sender: WritingStreamSender,
    abort_flag: Arc<AtomicBool>,
    transcript: Vec<ProviderMessage>,
    labels: WritingStageLabels,
    usage_category: ModelUsageCategory,
    operation: &'static str,
) -> Result<crate::agent_runtime::coordinator::RunOutcome, String> {
    abort_flag.store(false, Ordering::Relaxed);
    let model = configured_model();
    let cancellation = Cancellation::from_shared(abort_flag.clone());
    let run_id = generate_run_id(document_id, expected_revision);
    let authority = AuthorityRef::writing(document_id, expected_revision, digest, 0, version_id, 1);
    let run_sender = sender.clone();
    let app_for_run = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut gateway = DeepSeekChatGateway::for_surface(
            model,
            Some(&app_for_run),
            operation,
            usage_category,
            true,
        )
        .with_generation_params(WRITING_MAX_TOKENS, WRITING_TEMPERATURE)
        .with_thinking_disabled();
        let registry = writing_active_tools();
        run_writing_coordinator(
            &mut gateway,
            &registry,
            transcript,
            run_id,
            authority,
            &cancellation,
            run_sender,
            labels,
        )
    })
    .await
    .map_err(|error| format!("写作 Runtime 运行失败：{error}"))?
    .map_err(|error| error.message)
}

fn writing_analysis_system_prompt() -> &'static str {
    r#"You are ReadRay's English writing coach. Return exactly one JSON object and no Markdown.
Use concise Chinese explanations. Preserve the writer's voice and do not rewrite the full article.
This article was written by an English learner and almost certainly contains genuine issues: report the real problems you find.
Only report high-value issues that materially affect grammar, clarity, naturalness, or reasoning.
First catch clear grammar problems: sentence structure, tense, subject-verb agreement, articles/prepositions, and other objective errors.
For expression issues (naturalness, formality, idiomaticity), infer the writing scene from the article's content and purpose
(academic writing, job application, business email, casual/spoken language, daily essay, etc.) rather than requiring the user to declare it.
For each expression worth adjusting, give ONE scene-based judgment - whether the current wording reads too informal or too formal for that inferred scene -
and ONE suggested rewrite that fits the scene, carried in "reference". Do not list two parallel alternatives side by side
(do not mechanically offer both a "formal version" and a "more natural version" together).
Only suggest an expression change when it genuinely matters for the inferred scene; do not attach a formal/idiomatic comparison to every issue.
CRITICAL - verbatim copying: "source" and "targetText" must be copied character-for-character, word-for-word from the submitted article,
including the original spelling and punctuation. Never paraphrase, rephrase, translate, or invent them: your suggested rewrite goes ONLY into
"reference", never into "source" or "targetText". "targetText" must be a contiguous substring of "source", and "source" a contiguous substring
of the submitted body. If you cannot find an exact verbatim match, drop that issue instead of fabricating a source. The article title is NOT part
of the checked body: never use the title in "source" or "targetText", and never report an issue that targets only the title.
Be thorough: this is feedback for an English learner, so missing a real problem is worse than reporting one extra.
Report every genuine issue you find - typically 4 to 6 for a learner draft. An empty issues array is allowed only when
the article is truly flawless, which is rare for a learner draft: treat empty as exceptional, never as a default.
Keep every field short: "explanation" within about 60 Chinese characters, "hint" within about 40, "deeperHint" within about 80, "reference" within about 60.
Return at most 8 issues. Return 2 to 4 writing takeaways only when they are supported by
real issues or high-value transferable lessons visible in this submitted article; fewer or an
empty array is allowed only when the article is truly free of issues. Do not list every good word, phrase,
or generic strength as a takeaway.
Even when "issues" is empty or very small, still give 1 or 2 genuine takeaways drawn from real content
in this article (a transferable word choice, structure, or usage worth reusing), so the learner always gains something.
Use this exact camelCase schema:
{
  "issues": [
    {
      "id": "issue-1",
      "category": "问题类别",
      "source": "原文逐字片段",
      "targetText": "可在文章中精确定位的连续文本",
      "explanation": "为什么这是问题（表达类需说明推断的写作场景）",
      "hint": "只给一步提示",
      "deeperHint": "进一步但不代写的提示",
      "reference": "针对该场景的一条建议表达（不是并列的多个选项）"
    }
  ],
  "patterns": [
    {
      "id": "01",
      "title": "本次文章中可迁移的写作要点",
      "description": "它对应本文哪个真实问题，以及用户下次可以如何识别和使用"
    }
  ]
}"#
}

/// 严格校验解析（仅测试引用；生产路径使用 `parse_writing_analysis_content_salvage`）。
#[cfg(test)]
fn parse_writing_analysis_content(
    snapshot: &WritingSnapshot,
    content: &str,
) -> Result<WritingAnalysisContent, String> {
    let parsed: WritingAnalysisContent = serde_json::from_str(content.trim())
        .map_err(|error| format!("DeepSeek 写作分析不是合法 JSON：{error}"))?;
    validate_analysis_content(snapshot, &parsed)
        .map_err(|error| format!("DeepSeek 写作分析校验失败：{error}"))?;
    Ok(parsed)
}

/// 校验失败的容错解析（运行时路径使用）：丢弃无法在正文中定位的个别问题，
/// 保留合法问题保存。
///
/// 关闭思考模式后模型偶发把某个 source/targetText 写成非逐字片段（含标题），
/// 严格校验会因"一条坏问题"整次检查作废。这里做诚实的部分成功：只保存能通过
/// 逐字定位校验的问题（前端可高亮），被丢弃的问题 ID 记入诊断日志；没有任何
/// 合法问题时仍按失败处理（不伪装成功）。合法输入走 `parse_writing_analysis_content`
/// 的快路径，行为不变。
fn parse_writing_analysis_content_salvage(
    snapshot: &WritingSnapshot,
    content: &str,
) -> Result<WritingAnalysisContent, String> {
    let mut parsed: WritingAnalysisContent = serde_json::from_str(content.trim())
        .map_err(|error| format!("DeepSeek 写作分析不是合法 JSON：{error}"))?;
    if validate_analysis_content(snapshot, &parsed).is_ok() {
        return Ok(parsed);
    }
    let body = snapshot.paragraphs.join("\n");
    let mut seen_ids = HashSet::new();
    let mut kept_issues = Vec::new();
    for issue in parsed.issues {
        if seen_ids.insert(issue.id.clone()) && issue_is_locatable(&body, &issue) {
            kept_issues.push(issue);
        } else {
            eprintln!(
                "READRAY_WRITING_ANALYSIS_DROPPED=issue={} source={:?} targetText={:?}",
                issue.id, issue.source, issue.target_text
            );
        }
    }
    let kept_patterns: Vec<WritingPattern> = parsed
        .patterns
        .drain(..)
        .filter(|pattern| {
            validate_text_field("模式 ID", &pattern.id, 80).is_ok()
                && validate_text_field("模式标题", &pattern.title, 200).is_ok()
                && validate_text_field("模式说明", &pattern.description, 1_000).is_ok()
        })
        .collect();
    if kept_issues.is_empty() {
        return Err("DeepSeek 写作分析校验失败：没有能在正文中定位的问题。".to_string());
    }
    let salvaged = WritingAnalysisContent {
        issues: kept_issues,
        patterns: kept_patterns,
    };
    validate_analysis_content(snapshot, &salvaged)
        .map_err(|error| format!("DeepSeek 写作分析校验失败：{error}"))?;
    Ok(salvaged)
}

/// 单条问题是否可在送检正文中逐字定位（镜像 `validate_analysis_content` 的
/// 逐字约束与字段约束）。
fn issue_is_locatable(body: &str, issue: &WritingIssue) -> bool {
    validate_text_field("问题 ID", &issue.id, 80).is_ok()
        && validate_text_field("问题类别", &issue.category, 80).is_ok()
        && validate_text_field("问题原文", &issue.source, 1_000).is_ok()
        && validate_text_field("问题定位文本", &issue.target_text, 500).is_ok()
        && validate_text_field("问题说明", &issue.explanation, 1_500).is_ok()
        && validate_text_field("问题提示", &issue.hint, 800).is_ok()
        && validate_text_field("进一步提示", &issue.deeper_hint, 1_200).is_ok()
        && validate_text_field("参考表达", &issue.reference, 1_200).is_ok()
        && body.contains(&issue.target_text)
        && body.contains(&issue.source)
        && (issue.source.contains(&issue.target_text) || issue.target_text.contains(&issue.source))
}

#[derive(Clone)]
struct WritingQuestionModelInput {
    snapshot: WritingSnapshot,
    question: String,
    scope_label: String,
    selection_text: Option<String>,
    previous_answers: Vec<WritingAgentAnswer>,
}

/// 经共享 Agent Runtime 内核执行写作问答（流式状态 + 可取消）。
///
/// 复用同一 `WritingSurfaceAdapter` + coordinator + gateway，产出结构化 JSON 回答，
/// 经 `parse_writing_answer_content` 校验后才返回给调用方（`ask_writing_question_with`
/// 进一步做 expectedRevision 保存）。终端事件按真实结果发送。
async fn request_writing_answer_via_runtime(
    app: AppHandle,
    document_id: i64,
    expected_revision: i64,
    version_id: Option<i64>,
    sender: WritingStreamSender,
    abort_flag: Arc<AtomicBool>,
    input: WritingQuestionModelInput,
) -> Result<WritingAnswerContent, String> {
    let digest = writing_snapshot_digest(&input.snapshot);
    let context = json!({
        "article": input.snapshot,
        "scope": input.scope_label,
        "selectionText": input.selection_text,
        "question": input.question,
        "previousTurns": input.previous_answers.into_iter().map(|answer| json!({
            "question": answer.question,
            "answer": {
                "title": answer.title,
                "copy": answer.copy,
                "map": answer.map,
            }
        })).collect::<Vec<_>>(),
    });
    let user_content = serde_json::to_string(&context)
        .map_err(|error| format!("写作辅助上下文无法序列化：{error}"))?;
    let adapter = WritingSurfaceAdapter::new(writing_answer_system_prompt());
    let transcript = adapter.transcript(&user_content);

    let labels = WritingStageLabels {
        start: "正在理解你的问题…",
        analyzing: "正在根据文章组织回答…",
        finishing: "正在整理回答…",
    };
    let outcome = run_writing_runtime(
        app,
        document_id,
        expected_revision,
        version_id,
        digest,
        sender.clone(),
        abort_flag,
        transcript,
        labels,
        ModelUsageCategory::Writing,
        "DeepSeek 写作辅助",
    )
    .await?;

    match outcome.termination {
        TerminationReason::FinalAnswer => {
            let final_text = outcome.final_text.unwrap_or_default();
            match parse_writing_answer_content(&final_text) {
                Ok(content) => {
                    sender.send(WritingStreamEvent::Done);
                    Ok(content)
                }
                Err(error) => {
                    sender.send(WritingStreamEvent::Error {
                        message: "回答暂时无法解析，请重试。".to_string(),
                    });
                    Err(error)
                }
            }
        }
        TerminationReason::UserAborted => {
            sender.send(WritingStreamEvent::Stopped);
            Err("回答已停止，可重新提问。".to_string())
        }
        TerminationReason::RunBudgetExceeded => {
            sender.send(WritingStreamEvent::Error {
                message: "回答未完成，可重新提问。".to_string(),
            });
            Err("回答未完成，请重试。".to_string())
        }
        other => {
            let message = outcome
                .error
                .map(|error| error.message)
                .unwrap_or_else(|| "Agent run 失败。".to_string());
            eprintln!("READRAY_WRITING_RUN_FAILED=termination={other:?} error={message}");
            sender.send(WritingStreamEvent::Error {
                message: "暂时无法回答，请重试。".to_string(),
            });
            Err("暂时无法回答，请重试。".to_string())
        }
    }
}

fn writing_answer_system_prompt() -> &'static str {
    r#"You are ReadRay's English writing coach. Answer the user's writing question using the supplied real article and optional selection.
previousTurns contains the same visible writing-coach conversation in chronological order. Preserve its conversational continuity while treating the current article as authoritative.
Return exactly one JSON object and no Markdown. Use concise Chinese. Do not write or replace the full article.
For a direct language question, omit map. For planning or "what next" questions, map may be included.
Use this exact camelCase schema:
{
  "title": "回答标题",
  "copy": "可执行的简短回答",
  "map": {
    "core": "用户想表达的核心",
    "questions": ["需要先想清的问题"],
    "phrases": ["可立即使用的表达"],
    "starters": ["可自行补全的起笔句式"]
  }
}"#
}

fn parse_writing_answer_content(content: &str) -> Result<WritingAnswerContent, String> {
    let parsed: WritingAnswerContent = serde_json::from_str(content.trim())
        .map_err(|error| format!("DeepSeek 写作辅助回答不是合法 JSON：{error}"))?;
    validate_answer_content(&parsed)
        .map_err(|error| format!("DeepSeek 写作辅助回答校验失败：{error}"))?;
    Ok(parsed)
}

async fn analyze_writing_document_with<Open, Request, Fut>(
    mut open_store: Open,
    document_id: i64,
    expected_revision: i64,
    request_analysis: Request,
) -> Result<WritingDocumentRecord, String>
where
    Open: FnMut() -> Result<WritingStore, String>,
    Request: FnOnce(WritingSnapshot) -> Fut,
    Fut: Future<Output = Result<WritingAnalysisContent, String>>,
{
    let mut store = open_store()?;
    let record = store.get_required(document_id, false)?;
    require_revision(&record, expected_revision, "检查文章")?;
    let snapshot = record
        .summary
        .draft_snapshot
        .ok_or_else(|| "完成稿需要先进入继续修改，才能再次检查。".to_string())?;
    validate_snapshot(&snapshot, false)?;
    drop(store);

    let content = request_analysis(snapshot.clone()).await?;
    let mut store = open_store()?;
    store.save_analysis_if_current(document_id, expected_revision, &snapshot, &content)
}

async fn ask_writing_question_with<Open, Request, Fut>(
    mut open_store: Open,
    request: WritingQuestionRequest,
    request_answer: Request,
) -> Result<WritingAgentAnswer, String>
where
    Open: FnMut() -> Result<WritingStore, String>,
    Request: FnOnce(WritingQuestionModelInput) -> Fut,
    Fut: Future<Output = Result<WritingAnswerContent, String>>,
{
    let mut store = open_store()?;
    let record = store.get_required(request.document_id, false)?;
    require_revision(&record, request.expected_revision, "请求写作辅助")?;
    let (snapshot, target_revision) = match request.version_id {
        Some(version_id) => {
            let version = record
                .versions
                .iter()
                .find(|version| version.id == version_id)
                .ok_or_else(|| format!("写作版本不存在：id={version_id}"))?;
            (version.snapshot.clone(), version.source_revision)
        }
        None => (
            record
                .summary
                .draft_snapshot
                .or(record.summary.completed_snapshot)
                .ok_or_else(|| "当前文章没有可供写作辅助参考的正文。".to_string())?,
            request.expected_revision,
        ),
    };
    let draft_session_start_revision = record
        .versions
        .iter()
        .map(|version| version.source_revision)
        .max()
        .unwrap_or(-1);
    let parent = request.parent_answer_id.and_then(|parent_id| {
        record
            .answers
            .iter()
            .find(|answer| answer.id == parent_id)
            .cloned()
    });
    validate_question(
        &snapshot,
        &request,
        parent.as_ref(),
        target_revision,
        draft_session_start_revision,
    )?;
    let previous_answers = collect_visible_answer_context(
        &record.answers,
        &request,
        target_revision,
        draft_session_start_revision,
    )?;
    let model_input = WritingQuestionModelInput {
        snapshot,
        question: request.question.trim().to_string(),
        scope_label: request.scope.label().to_string(),
        selection_text: request.selection_text.clone(),
        previous_answers,
    };
    drop(store);

    let content = request_answer(model_input).await?;
    let mut store = open_store()?;
    store.save_answer_if_current(&request, &content)
}

#[tauri::command]
pub fn create_writing_document(app: AppHandle) -> Result<WritingDocumentRecord, String> {
    WritingStore::open_for_app(&app)?.create()
}

#[tauri::command]
pub fn list_writing_documents(
    app: AppHandle,
    query: Option<String>,
) -> Result<Vec<WritingDocumentSummary>, String> {
    WritingStore::open_for_app(&app)?.list(query.as_deref())
}

#[tauri::command]
pub fn get_writing_document(
    app: AppHandle,
    document_id: i64,
) -> Result<Option<WritingDocumentRecord>, String> {
    WritingStore::open_for_app(&app)?.get(document_id, true)
}

#[tauri::command]
pub fn save_writing_draft(
    app: AppHandle,
    document_id: i64,
    expected_revision: i64,
    snapshot: WritingSnapshot,
) -> Result<WritingDocumentRecord, String> {
    WritingStore::open_for_app(&app)?.save_draft(document_id, expected_revision, &snapshot)
}

#[tauri::command]
pub fn delete_writing_document(
    app: AppHandle,
    document_id: i64,
    expected_revision: i64,
) -> Result<bool, String> {
    WritingStore::open_for_app(&app)?.delete(document_id, expected_revision)
}

#[tauri::command]
pub fn complete_writing_document(
    app: AppHandle,
    document_id: i64,
    expected_revision: i64,
) -> Result<WritingDocumentRecord, String> {
    WritingStore::open_for_app(&app)?.complete(document_id, expected_revision)
}

#[tauri::command]
pub fn continue_writing_document(
    app: AppHandle,
    document_id: i64,
    expected_revision: i64,
    version_id: Option<i64>,
) -> Result<WritingDocumentRecord, String> {
    WritingStore::open_for_app(&app)?.continue_editing(document_id, expected_revision, version_id)
}

#[tauri::command]
pub async fn analyze_writing_document(
    app: AppHandle,
    document_id: i64,
    expected_revision: i64,
    channel: Channel<WritingStreamEvent>,
) -> Result<WritingDocumentRecord, String> {
    let usage_app = app.clone();
    let sender = WritingStreamSender::new(channel);
    let abort_flag = writing_abort_flag_for(document_id);
    analyze_writing_document_with(
        || WritingStore::open_for_app(&app),
        document_id,
        expected_revision,
        move |snapshot| {
            request_writing_analysis_via_runtime(
                usage_app,
                document_id,
                expected_revision,
                sender,
                abort_flag,
                snapshot,
            )
        },
    )
    .await
}

#[tauri::command]
pub async fn ask_writing_question(
    app: AppHandle,
    request: WritingQuestionRequest,
    channel: Channel<WritingStreamEvent>,
) -> Result<WritingAgentAnswer, String> {
    let usage_app = app.clone();
    let sender = WritingStreamSender::new(channel);
    let abort_flag = writing_abort_flag_for(request.document_id);
    let document_id = request.document_id;
    let expected_revision = request.expected_revision;
    let version_id = request.version_id;
    ask_writing_question_with(
        || WritingStore::open_for_app(&app),
        request,
        move |input| {
            request_writing_answer_via_runtime(
                usage_app,
                document_id,
                expected_revision,
                version_id,
                sender,
                abort_flag,
                input,
            )
        },
    )
    .await
}

/// 中断一次进行中的写作检查/问答。中断后保留当前草稿与已产生内容，可重试。
#[tauri::command]
pub fn abort_writing_analysis(document_id: i64) -> Result<(), String> {
    writing_abort_flag_for(document_id).store(true, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_DATABASE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_database_path() -> (PathBuf, PathBuf) {
        let suffix = format!(
            "readray-writing-{}-{}",
            std::process::id(),
            TEST_DATABASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(suffix);
        (root.clone(), root.join("readray.sqlite3"))
    }

    fn snapshot(title: &str, body: &str) -> WritingSnapshot {
        WritingSnapshot {
            title: title.to_string(),
            paragraphs: vec![body.to_string()],
        }
    }

    fn valid_analysis(target: &str) -> WritingAnalysisContent {
        WritingAnalysisContent {
            issues: vec![WritingIssue {
                id: "issue-1".to_string(),
                category: "动词结构".to_string(),
                source: target.to_string(),
                target_text: target.to_string(),
                explanation: "这里的结构影响自然度。".to_string(),
                hint: "先检查核心动词。".to_string(),
                deeper_hint: "保留原意，只调整动词后的结构。".to_string(),
                reference: "A concise reference.".to_string(),
            }],
            patterns: vec![WritingPattern {
                id: "01".to_string(),
                title: "检查核心动词".to_string(),
                description: "先定位主语和核心动词，再处理其余成分。".to_string(),
            }],
        }
    }

    fn valid_answer() -> WritingAnswerContent {
        WritingAnswerContent {
            title: "先确认句子重心".to_string(),
            copy: "保留原意，先把最重要的判断放在主句。".to_string(),
            map: None,
        }
    }

    #[test]
    fn article_crud_search_restart_and_delete_use_database_facts() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        assert_eq!(created.summary.revision, 0);
        assert!(created.summary.id > 0);
        let saved = store
            .save_draft(
                created.summary.id,
                created.summary.revision,
                &snapshot("Database authority", "The draft survives a restart."),
            )
            .unwrap();
        assert_eq!(saved.summary.revision, 1);
        assert_eq!(store.list(Some("SURVIVES")).unwrap().len(), 1);
        drop(store);

        let mut reopened = WritingStore::open_path(&path).unwrap();
        let restored = reopened.get_required(created.summary.id, true).unwrap();
        assert_eq!(
            restored.summary.draft_snapshot.unwrap().title,
            "Database authority"
        );
        assert!(reopened
            .delete(created.summary.id, restored.summary.revision)
            .unwrap());
        assert!(reopened.get(created.summary.id, false).unwrap().is_none());
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn autosave_revision_rejects_stale_writes_and_keeps_newer_text() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let first = store
            .save_draft(
                created.summary.id,
                0,
                &snapshot("First", "The first saved body."),
            )
            .unwrap();
        let second = store
            .save_draft(
                created.summary.id,
                first.summary.revision,
                &snapshot("Second", "The newer saved body."),
            )
            .unwrap();
        let error = store
            .save_draft(
                created.summary.id,
                first.summary.revision,
                &snapshot("Stale", "This must not overwrite."),
            )
            .unwrap_err();
        assert!(error.contains("版本冲突"));
        let current = store.get_required(created.summary.id, false).unwrap();
        assert_eq!(current.summary.revision, second.summary.revision);
        assert_eq!(current.summary.draft_snapshot.unwrap().title, "Second");
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completed_versions_are_immutable_and_keep_comparison_baselines() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let first_draft = snapshot("Version one", "The original body.");
        let saved = store
            .save_draft(created.summary.id, 0, &first_draft)
            .unwrap();
        let analyzed = store
            .save_analysis_if_current(
                created.summary.id,
                saved.summary.revision,
                &first_draft,
                &valid_analysis("original body"),
            )
            .unwrap();
        assert_eq!(
            analyzed.active_analysis.as_ref().unwrap().document_revision,
            analyzed.summary.revision
        );
        assert_eq!(
            analyzed
                .baseline_analysis
                .as_ref()
                .unwrap()
                .document_revision,
            analyzed.summary.revision
        );
        let completed = store
            .complete(created.summary.id, analyzed.summary.revision)
            .unwrap();
        assert_eq!(completed.versions.len(), 1);
        assert_eq!(completed.versions[0].comparison_baseline, first_draft);
        assert_eq!(completed.versions[0].issues[0].target_text, "original body");
        let first_version = completed.versions[0].clone();

        let continued = store
            .continue_editing(
                created.summary.id,
                completed.summary.revision,
                Some(first_version.id),
            )
            .unwrap();
        assert!(continued.active_analysis.is_none());
        let second_draft = snapshot("Version two", "The revised body.");
        let saved_again = store
            .save_draft(
                created.summary.id,
                continued.summary.revision,
                &second_draft,
            )
            .unwrap();
        let completed_again = store
            .complete(created.summary.id, saved_again.summary.revision)
            .unwrap();
        assert_eq!(completed_again.versions.len(), 2);
        assert_eq!(completed_again.versions[0], first_version);
        assert_eq!(completed_again.versions[1].snapshot.title, "Version two");
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn structured_analysis_parser_rejects_invalid_json_and_unknown_locations() {
        let article = snapshot("Title", "This sentence has a real target.");
        let valid = serde_json::to_string(&valid_analysis("real target")).unwrap();
        assert_eq!(
            parse_writing_analysis_content(&article, &valid)
                .unwrap()
                .issues
                .len(),
            1
        );
        assert!(parse_writing_analysis_content(&article, "{not-json")
            .unwrap_err()
            .contains("不是合法 JSON"));
        let invalid = serde_json::to_string(&valid_analysis("invented target")).unwrap();
        assert!(parse_writing_analysis_content(&article, &invalid)
            .unwrap_err()
            .contains("不存在于送检正文"));
    }

    #[test]
    fn salvage_keeps_locatable_issues_and_drops_invented_or_title_ones() {
        // 运行时路径的容错：个别问题编造 source/targetText（含标题）时，丢弃
        // 它们、保留能在正文中逐字定位的问题，避免整次检查作废。
        let article = snapshot("Title", "This sentence has a real target.");
        let content = WritingAnalysisContent {
            issues: vec![
                WritingIssue {
                    id: "issue-1".into(),
                    category: "语法".into(),
                    source: "has a real target".into(),
                    target_text: "has a real target".into(),
                    explanation: "说明".into(),
                    hint: "提示".into(),
                    deeper_hint: "进一步".into(),
                    reference: "参考".into(),
                },
                WritingIssue {
                    id: "issue-2".into(),
                    category: "语法".into(),
                    source: "invented fragment".into(),
                    target_text: "invented fragment".into(),
                    explanation: "说明".into(),
                    hint: "提示".into(),
                    deeper_hint: "进一步".into(),
                    reference: "参考".into(),
                },
                WritingIssue {
                    id: "issue-3".into(),
                    category: "标题".into(),
                    source: "Title".into(),
                    target_text: "Title".into(),
                    explanation: "说明".into(),
                    hint: "提示".into(),
                    deeper_hint: "进一步".into(),
                    reference: "参考".into(),
                },
            ],
            patterns: vec![],
        };
        let json = serde_json::to_string(&content).unwrap();
        let salvaged = parse_writing_analysis_content_salvage(&article, &json).unwrap();
        assert_eq!(salvaged.issues.len(), 1);
        assert_eq!(salvaged.issues[0].id, "issue-1");
        // 保存边界还会再跑严格校验：容错结果必须仍然合法。
        validate_analysis_content(&article, &salvaged).unwrap();
    }

    #[test]
    fn salvage_fails_when_no_issue_is_locatable() {
        let article = snapshot("Title", "This sentence has a real target.");
        let content = WritingAnalysisContent {
            issues: vec![WritingIssue {
                id: "issue-1".into(),
                category: "语法".into(),
                source: "totally invented".into(),
                target_text: "totally invented".into(),
                explanation: "说明".into(),
                hint: "提示".into(),
                deeper_hint: "进一步".into(),
                reference: "参考".into(),
            }],
            patterns: vec![],
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(parse_writing_analysis_content_salvage(&article, &json)
            .unwrap_err()
            .contains("没有能在正文中定位的问题"));
    }

    #[test]
    fn salvage_passes_through_valid_analysis_unchanged() {
        let article = snapshot("Title", "This sentence has a real target.");
        let expected = valid_analysis("has a real target");
        let json = serde_json::to_string(&expected).unwrap();
        assert_eq!(
            parse_writing_analysis_content_salvage(&article, &json).unwrap(),
            expected
        );
    }

    #[test]
    fn structured_answer_parser_rejects_empty_or_extra_fields() {
        assert!(parse_writing_answer_content(r#"{"title":"","copy":"answer"}"#).is_err());
        assert!(parse_writing_answer_content(
            r#"{"title":"Title","copy":"Answer","unexpected":true}"#
        )
        .is_err());
        assert_eq!(
            parse_writing_answer_content(r#"{"title":"Title","copy":"Answer","map":null}"#)
                .unwrap()
                .copy,
            "Answer"
        );
    }

    #[test]
    fn save_failure_preserves_previous_draft_and_revision() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let saved = store
            .save_draft(created.summary.id, 0, &snapshot("Safe", "Saved body"))
            .unwrap();
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_writing_save
                 BEFORE UPDATE OF draft_title ON writing_documents
                 BEGIN SELECT RAISE(FAIL, 'forced writing save failure'); END;",
            )
            .unwrap();
        assert!(store
            .save_draft(
                created.summary.id,
                saved.summary.revision,
                &snapshot("Unsafe", "Must not be partially stored"),
            )
            .is_err());
        store
            .connection
            .execute_batch("DROP TRIGGER fail_writing_save;")
            .unwrap();
        let restored = store.get_required(created.summary.id, false).unwrap();
        assert_eq!(restored.summary.revision, saved.summary.revision);
        assert_eq!(restored.summary.draft_snapshot.unwrap().title, "Safe");
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_failure_keeps_saved_body_and_does_not_create_analysis() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let saved = store
            .save_draft(
                created.summary.id,
                0,
                &snapshot("Saved first", "The model can fail safely."),
            )
            .unwrap();
        drop(store);

        let result = tauri::async_runtime::block_on(analyze_writing_document_with(
            || WritingStore::open_path(&path),
            created.summary.id,
            saved.summary.revision,
            |_snapshot| async { Err("forced model failure".to_string()) },
        ));
        assert!(result.unwrap_err().contains("forced model failure"));
        let mut reopened = WritingStore::open_path(&path).unwrap();
        let restored = reopened.get_required(created.summary.id, false).unwrap();
        assert_eq!(restored.summary.revision, saved.summary.revision);
        assert!(restored.active_analysis.is_none());
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn late_analysis_is_rejected_after_a_newer_save() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let first = store
            .save_draft(
                created.summary.id,
                0,
                &snapshot("First", "The first revision target."),
            )
            .unwrap();
        drop(store);
        let path_for_request = path.clone();
        let result = tauri::async_runtime::block_on(analyze_writing_document_with(
            || WritingStore::open_path(&path),
            created.summary.id,
            first.summary.revision,
            move |_snapshot| async move {
                let mut concurrent = WritingStore::open_path(&path_for_request).unwrap();
                concurrent
                    .save_draft(
                        created.summary.id,
                        first.summary.revision,
                        &snapshot("Newer", "A newer revision wins."),
                    )
                    .unwrap();
                Ok(valid_analysis("first revision target"))
            },
        ));
        assert!(result.unwrap_err().contains("已过期"));
        let mut reopened = WritingStore::open_path(&path).unwrap();
        let restored = reopened.get_required(created.summary.id, false).unwrap();
        assert_eq!(restored.summary.draft_snapshot.unwrap().title, "Newer");
        assert!(restored.active_analysis.is_none());
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn selection_follow_up_uses_real_article_and_persists_validated_answers() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let saved = store
            .save_draft(
                created.summary.id,
                0,
                &snapshot("Question", "This exact selection needs help."),
            )
            .unwrap();
        drop(store);
        let first_request = WritingQuestionRequest {
            document_id: created.summary.id,
            expected_revision: saved.summary.revision,
            version_id: None,
            question: "这里的语气自然吗？".to_string(),
            scope: WritingQuestionScope::Selection,
            selection_text: Some("exact selection".to_string()),
            parent_answer_id: None,
        };
        let first = tauri::async_runtime::block_on(ask_writing_question_with(
            || WritingStore::open_path(&path),
            first_request,
            |input| async move {
                assert_eq!(input.selection_text.as_deref(), Some("exact selection"));
                Ok(valid_answer())
            },
        ))
        .unwrap();
        let mut store = WritingStore::open_path(&path).unwrap();
        let auto_saved = store
            .save_draft(
                created.summary.id,
                saved.summary.revision,
                &snapshot(
                    "Question after auto save",
                    "This exact selection still needs help after a local edit.",
                ),
            )
            .unwrap();
        drop(store);
        let follow_up = WritingQuestionRequest {
            document_id: created.summary.id,
            expected_revision: auto_saved.summary.revision,
            version_id: None,
            question: "能再简单一点吗？".to_string(),
            scope: WritingQuestionScope::Selection,
            selection_text: Some("exact selection".to_string()),
            parent_answer_id: Some(first.id),
        };
        let second = tauri::async_runtime::block_on(ask_writing_question_with(
            || WritingStore::open_path(&path),
            follow_up,
            |input| async move {
                assert_eq!(
                    input
                        .previous_answers
                        .iter()
                        .map(|answer| answer.id)
                        .collect::<Vec<_>>(),
                    vec![first.id]
                );
                Ok(valid_answer())
            },
        ))
        .unwrap();
        assert_eq!(second.parent_answer_id, Some(first.id));
        let third = tauri::async_runtime::block_on(ask_writing_question_with(
            || WritingStore::open_path(&path),
            WritingQuestionRequest {
                document_id: created.summary.id,
                expected_revision: auto_saved.summary.revision,
                version_id: None,
                question: "那正式一点该怎么说？".to_string(),
                scope: WritingQuestionScope::Selection,
                selection_text: Some("exact selection".to_string()),
                parent_answer_id: Some(second.id),
            },
            |input| async move {
                assert_eq!(
                    input
                        .previous_answers
                        .iter()
                        .map(|answer| answer.id)
                        .collect::<Vec<_>>(),
                    vec![first.id, second.id]
                );
                Ok(valid_answer())
            },
        ))
        .unwrap();
        assert_eq!(third.parent_answer_id, Some(second.id));
        let mut reopened = WritingStore::open_path(&path).unwrap();
        assert_eq!(
            reopened
                .get_required(created.summary.id, false)
                .unwrap()
                .answers
                .len(),
            3
        );
        let completed = reopened
            .complete(created.summary.id, auto_saved.summary.revision)
            .unwrap();
        let continued = reopened
            .continue_editing(
                created.summary.id,
                completed.summary.revision,
                Some(completed.versions[0].id),
            )
            .unwrap();
        drop(reopened);
        let rejected = tauri::async_runtime::block_on(ask_writing_question_with(
            || WritingStore::open_path(&path),
            WritingQuestionRequest {
                document_id: created.summary.id,
                expected_revision: continued.summary.revision,
                version_id: None,
                question: "旧草稿回答还能追问吗？".to_string(),
                scope: WritingQuestionScope::Selection,
                selection_text: Some("exact selection".to_string()),
                parent_answer_id: Some(third.id),
            },
            |_input| async move { panic!("跨完成版本的旧草稿追问不应调用模型") },
        ));
        assert!(rejected.unwrap_err().contains("不属于当前可见文章版本"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_or_late_question_does_not_persist_an_answer() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let saved = store
            .save_draft(
                created.summary.id,
                0,
                &snapshot("Question safety", "The current paragraph stays safe."),
            )
            .unwrap();
        drop(store);
        let request = WritingQuestionRequest {
            document_id: created.summary.id,
            expected_revision: saved.summary.revision,
            version_id: None,
            question: "这里自然吗？".to_string(),
            scope: WritingQuestionScope::Paragraph,
            selection_text: None,
            parent_answer_id: None,
        };
        let failed = tauri::async_runtime::block_on(ask_writing_question_with(
            || WritingStore::open_path(&path),
            request.clone(),
            |_input| async { Err("forced answer model failure".to_string()) },
        ));
        assert!(failed.unwrap_err().contains("forced answer model failure"));

        let path_for_request = path.clone();
        let late = tauri::async_runtime::block_on(ask_writing_question_with(
            || WritingStore::open_path(&path),
            request,
            move |_input| async move {
                let mut concurrent = WritingStore::open_path(&path_for_request).unwrap();
                concurrent
                    .save_draft(
                        created.summary.id,
                        saved.summary.revision,
                        &snapshot("Newer question", "A newer paragraph wins."),
                    )
                    .unwrap();
                Ok(valid_answer())
            },
        ));
        assert!(late.unwrap_err().contains("已过期"));

        let mut reopened = WritingStore::open_path(&path).unwrap();
        let restored = reopened.get_required(created.summary.id, false).unwrap();
        assert!(restored.answers.is_empty());
        assert_eq!(
            restored.summary.draft_snapshot.unwrap().title,
            "Newer question"
        );
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn continue_editing_rejects_an_existing_draft_and_preserves_it() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let saved_v1 = store
            .save_draft(
                created.summary.id,
                created.summary.revision,
                &snapshot("Version one", "The immutable first version."),
            )
            .unwrap();
        let completed_v1 = store
            .complete(created.summary.id, saved_v1.summary.revision)
            .unwrap();
        let version_id = completed_v1.versions[0].id;
        let continued = store
            .continue_editing(
                created.summary.id,
                completed_v1.summary.revision,
                Some(version_id),
            )
            .unwrap();
        let draft_b = snapshot("Draft B", "These later edits must survive.");
        let saved_b = store
            .save_draft(created.summary.id, continued.summary.revision, &draft_b)
            .unwrap();

        let error = store
            .continue_editing(
                created.summary.id,
                saved_b.summary.revision,
                Some(version_id),
            )
            .unwrap_err();
        assert!(error.contains("已有") && error.contains("草稿"));
        let restored = store.get_required(created.summary.id, false).unwrap();
        assert_eq!(restored.summary.revision, saved_b.summary.revision);
        assert_eq!(restored.summary.draft_snapshot, Some(draft_b));
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn search_matches_completed_content_even_when_a_new_draft_exists() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let saved = store
            .save_draft(
                created.summary.id,
                0,
                &snapshot("First version", "CompletedOnlyNeedle remains searchable."),
            )
            .unwrap();
        let completed = store
            .complete(created.summary.id, saved.summary.revision)
            .unwrap();
        let continued = store
            .continue_editing(
                created.summary.id,
                completed.summary.revision,
                Some(completed.versions[0].id),
            )
            .unwrap();
        store
            .save_draft(
                created.summary.id,
                continued.summary.revision,
                &snapshot("Current draft", "No matching phrase is here."),
            )
            .unwrap();

        let matches = store.list(Some("completedonlyneedle")).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, created.summary.id);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn analysis_validator_rejects_invented_source_and_title_only_targets() {
        let article = snapshot("TitleOnlyTarget", "This body contains a real target.");
        let mut invented_source = valid_analysis("real target");
        invented_source.issues[0].source = "model invented source".to_string();
        assert!(validate_analysis_content(&article, &invented_source)
            .unwrap_err()
            .contains("source"));

        let title_target = valid_analysis("TitleOnlyTarget");
        assert!(validate_analysis_content(&article, &title_target)
            .unwrap_err()
            .contains("正文"));
    }

    #[test]
    fn analysis_advances_revision_and_old_analysis_is_not_active_after_edit() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let checked_snapshot = snapshot("Checked", "The checked body is exact.");
        let saved = store
            .save_draft(created.summary.id, 0, &checked_snapshot)
            .unwrap();
        let analyzed = store
            .save_analysis_if_current(
                created.summary.id,
                saved.summary.revision,
                &checked_snapshot,
                &valid_analysis("checked body"),
            )
            .unwrap();
        assert!(analyzed.summary.revision > saved.summary.revision);
        assert_eq!(
            analyzed.active_analysis.as_ref().unwrap().document_revision,
            analyzed.summary.revision
        );
        assert!(store
            .complete(created.summary.id, saved.summary.revision)
            .unwrap_err()
            .contains("版本冲突"));

        let edited = store
            .save_draft(
                created.summary.id,
                analyzed.summary.revision,
                &snapshot("Edited", "The body changed after analysis."),
            )
            .unwrap();
        assert!(edited.active_analysis.is_none());
        assert_eq!(
            edited.baseline_analysis.as_ref().unwrap().document_revision,
            analyzed.summary.revision
        );
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_completion_uses_exact_current_analysis_without_inventing_baseline_revision() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let checked_snapshot = snapshot("Metadata", "The exact metadata target.");
        let saved = store
            .save_draft(created.summary.id, 0, &checked_snapshot)
            .unwrap();
        let analyzed = store
            .save_analysis_if_current(
                created.summary.id,
                saved.summary.revision,
                &checked_snapshot,
                &valid_analysis("metadata target"),
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE writing_documents
                 SET comparison_baseline_revision = NULL
                 WHERE id = ?1",
                [created.summary.id],
            )
            .unwrap();
        let completed = store
            .complete(created.summary.id, analyzed.summary.revision)
            .unwrap();
        let (source_revision, analysis_revision, baseline_revision): (i64, i64, Option<i64>) =
            store
                .connection
                .query_row(
                    "SELECT source_revision, analysis_revision, comparison_baseline_revision
                 FROM writing_versions WHERE id = ?1",
                    [completed.versions[0].id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
        assert_eq!(analysis_revision, source_revision);
        assert_eq!(baseline_revision, None);
        assert_eq!(
            completed.versions[0].issues[0].target_text,
            "metadata target"
        );
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn historical_question_request_accepts_explicit_version_identity() {
        let request = serde_json::json!({
            "documentId": 17,
            "expectedRevision": 4,
            "versionId": 91,
            "question": "Is this version clear?",
            "scope": "document",
            "selectionText": null,
            "parentAnswerId": null
        });
        assert!(serde_json::from_value::<WritingQuestionRequest>(request).is_ok());
    }

    #[test]
    fn historical_question_uses_visible_immutable_version_not_hidden_draft() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let version_snapshot = snapshot("Version one", "Visible immutable version body.");
        let saved_v1 = store
            .save_draft(created.summary.id, 0, &version_snapshot)
            .unwrap();
        let completed = store
            .complete(created.summary.id, saved_v1.summary.revision)
            .unwrap();
        let version = completed.versions[0].clone();
        let continued = store
            .continue_editing(
                created.summary.id,
                completed.summary.revision,
                Some(version.id),
            )
            .unwrap();
        let hidden_draft = snapshot("Draft B", "Hidden newer draft body.");
        let saved_b = store
            .save_draft(
                created.summary.id,
                continued.summary.revision,
                &hidden_draft,
            )
            .unwrap();
        drop(store);

        let request = WritingQuestionRequest {
            document_id: created.summary.id,
            expected_revision: saved_b.summary.revision,
            version_id: Some(version.id),
            question: "这个完成版本清楚吗？".to_string(),
            scope: WritingQuestionScope::Document,
            selection_text: None,
            parent_answer_id: None,
        };
        let answer = tauri::async_runtime::block_on(ask_writing_question_with(
            || WritingStore::open_path(&path),
            request,
            move |input| async move {
                assert_eq!(input.snapshot, version_snapshot);
                assert_ne!(input.snapshot, hidden_draft);
                Ok(valid_answer())
            },
        ))
        .unwrap();
        assert_eq!(answer.version_id, Some(version.id));
        assert_eq!(answer.document_revision, version.source_revision);

        let mut reopened = WritingStore::open_path(&path).unwrap();
        let restored = reopened.get_required(created.summary.id, false).unwrap();
        assert_eq!(restored.summary.draft_snapshot.unwrap().title, "Draft B");
        assert_eq!(restored.answers[0].version_id, Some(version.id));
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn analysis_and_completion_with_the_same_expected_revision_cannot_both_commit() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let first = store.create().unwrap();
        let first_snapshot = snapshot("Analysis wins", "The first exact target.");
        let first_saved = store
            .save_draft(first.summary.id, 0, &first_snapshot)
            .unwrap();
        let analyzed = store
            .save_analysis_if_current(
                first.summary.id,
                first_saved.summary.revision,
                &first_snapshot,
                &valid_analysis("first exact target"),
            )
            .unwrap();
        assert!(store
            .complete(first.summary.id, first_saved.summary.revision)
            .is_err());
        assert_eq!(
            analyzed.active_analysis.unwrap().document_revision,
            analyzed.summary.revision
        );

        let second = store.create().unwrap();
        let second_snapshot = snapshot("Completion wins", "The second exact target.");
        let second_saved = store
            .save_draft(second.summary.id, 0, &second_snapshot)
            .unwrap();
        store
            .complete(second.summary.id, second_saved.summary.revision)
            .unwrap();
        assert!(store
            .save_analysis_if_current(
                second.summary.id,
                second_saved.summary.revision,
                &second_snapshot,
                &valid_analysis("second exact target"),
            )
            .is_err());
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restart_only_restores_analysis_bound_to_current_revision() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let checked_snapshot = snapshot("Restart", "The restart target is here.");
        let saved = store
            .save_draft(created.summary.id, 0, &checked_snapshot)
            .unwrap();
        let analyzed = store
            .save_analysis_if_current(
                created.summary.id,
                saved.summary.revision,
                &checked_snapshot,
                &valid_analysis("restart target"),
            )
            .unwrap();
        drop(store);

        let mut reopened = WritingStore::open_path(&path).unwrap();
        assert_eq!(
            reopened
                .get_required(created.summary.id, false)
                .unwrap()
                .active_analysis
                .unwrap()
                .document_revision,
            analyzed.summary.revision
        );
        let edited = reopened
            .save_draft(
                created.summary.id,
                analyzed.summary.revision,
                &snapshot("Restart edited", "A newer body after restart."),
            )
            .unwrap();
        assert!(edited.active_analysis.is_none());
        drop(reopened);

        let mut reopened_again = WritingStore::open_path(&path).unwrap();
        assert!(reopened_again
            .get_required(created.summary.id, false)
            .unwrap()
            .active_analysis
            .is_none());
        drop(reopened_again);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn baseline_analysis_survives_edit_restart_and_completion_without_becoming_active() {
        let (root, path) = test_database_path();
        let mut store = WritingStore::open_path(&path).unwrap();
        let created = store.create().unwrap();
        let checked_snapshot = snapshot("Baseline check", "The checked target remains traceable.");
        let saved = store
            .save_draft(created.summary.id, 0, &checked_snapshot)
            .unwrap();
        let analyzed = store
            .save_analysis_if_current(
                created.summary.id,
                saved.summary.revision,
                &checked_snapshot,
                &valid_analysis("checked target"),
            )
            .unwrap();
        let edited_snapshot = snapshot(
            "Edited after check",
            "The checked target remains traceable after a local edit.",
        );
        let edited = store
            .save_draft(
                created.summary.id,
                analyzed.summary.revision,
                &edited_snapshot,
            )
            .unwrap();
        assert!(edited.active_analysis.is_none());
        drop(store);

        let mut reopened = WritingStore::open_path(&path).unwrap();
        let restored = reopened.get_required(created.summary.id, false).unwrap();
        assert!(restored.active_analysis.is_none());
        assert_eq!(
            restored
                .baseline_analysis
                .as_ref()
                .unwrap()
                .document_revision,
            analyzed.summary.revision
        );
        let restored_json = serde_json::to_value(&restored).unwrap();
        assert_eq!(
            restored_json["baselineAnalysis"]["documentRevision"],
            analyzed.summary.revision
        );

        let completed = reopened
            .complete(created.summary.id, edited.summary.revision)
            .unwrap();
        let version = completed.versions[0].clone();
        assert_eq!(version.snapshot, edited_snapshot);
        assert_eq!(version.comparison_baseline, checked_snapshot);
        assert_eq!(version.source_revision, edited.summary.revision);
        assert_eq!(version.analysis_revision, Some(analyzed.summary.revision));
        assert_eq!(
            version.comparison_baseline_revision,
            Some(analyzed.summary.revision)
        );
        assert_eq!(version.issues, valid_analysis("checked target").issues);
        assert_eq!(version.patterns, valid_analysis("checked target").patterns);

        drop(reopened);
        let mut reopened_completed = WritingStore::open_path(&path).unwrap();
        let restored_completed = reopened_completed
            .get_required(created.summary.id, false)
            .unwrap();
        assert_eq!(restored_completed.versions[0], version);
        drop(reopened_completed);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn analysis_prompt_demands_scene_based_single_suggestion_not_parallel_options() {
        // 任务 3 B(1)：检查先抓明显语法问题；表达类问题由模型从内容推断场景，
        // 给"一个针对场景的判断 + 一条建议"，不并列"正式/地道"两个选项。
        let prompt = writing_analysis_system_prompt();
        assert!(prompt.contains("First catch clear grammar problems"));
        assert!(prompt.contains("infer the writing scene"));
        assert!(prompt.contains("ONE scene-based judgment"));
        assert!(prompt.contains("ONE suggested rewrite"));
        assert!(prompt.contains("Do not list two parallel alternatives"));
    }

    #[test]
    fn analysis_prompt_demands_verbatim_source_copying() {
        // 关闭思考模式后模型容易把建议改写混进 source/或编造片段；prompt 必须
        // 强制逐字复制原文、改写只进 reference、找不到精确匹配就放弃该问题。
        let prompt = writing_analysis_system_prompt();
        assert!(prompt.contains("CRITICAL - verbatim copying"));
        assert!(prompt.contains("character-for-character"));
        assert!(prompt.contains("goes ONLY into"));
        assert!(prompt.contains("drop that issue instead of fabricating a source"));
        // 标题不属于检查正文：绝不放进 source/targetText（校验器拒绝标题定位）。
        assert!(prompt.contains("The article title is NOT part"));
        assert!(prompt.contains("never report an issue that targets only the title"));
        // 对抗"空结果"倾向：学习者文章几乎一定有真实问题，要如实报告。
        assert!(prompt.contains("almost certainly contains genuine issues"));
        assert!(prompt.contains("truly free of issues"));
        // 输出精炼但不得漏报：漏报真实问题比多报更糟，学习者草稿通常 4-6 个
        // 真实问题；字段有长度上限（explanation/hint/deeperHint/reference）。
        assert!(prompt.contains("missing a real problem is worse than reporting one extra"));
        assert!(prompt.contains("typically 4 to 6 for a learner draft"));
        assert!(prompt.contains("treat empty as exceptional, never as a default"));
        assert!(prompt.contains("\"explanation\" within about 60"));
        assert!(prompt.contains("\"reference\" within about 60"));
        // 空/极少 issues 时仍给 1-2 条真实可迁移要点：学习者检查后总是有所收获。
        assert!(prompt.contains("Even when \"issues\" is empty or very small"));
        assert!(prompt.contains("the learner always gains something"));
    }

    #[test]
    fn snapshot_digest_is_stable_and_deterministic() {
        let article = snapshot("My essay", "Hello world.");
        let again = snapshot("My essay", "Hello world.");
        assert_eq!(
            writing_snapshot_digest(&article),
            writing_snapshot_digest(&again)
        );
        let changed = snapshot("My essay", "Hello world! Changed.");
        assert_ne!(
            writing_snapshot_digest(&article),
            writing_snapshot_digest(&changed)
        );
        assert!(!writing_snapshot_digest(&article).is_empty());
    }

    #[test]
    fn abort_flag_is_shared_and_abort_set_increments() {
        let flag_a = writing_abort_flag_for(999_001);
        let flag_b = writing_abort_flag_for(999_001);
        assert!(
            std::sync::Arc::ptr_eq(&flag_a, &flag_b),
            "同文档中止标志必须共享"
        );
        assert!(!flag_a.load(Ordering::Relaxed));
        abort_writing_analysis(999_001).unwrap();
        assert!(flag_a.load(Ordering::Relaxed));
        // 重新检查会清零（在 runtime 入口执行 store(false)）。
        flag_a.store(false, Ordering::Relaxed);
        assert!(!flag_a.load(Ordering::Relaxed));
        let other = writing_abort_flag_for(999_002);
        assert!(!std::sync::Arc::ptr_eq(&flag_a, &other));
    }

    #[test]
    fn writing_ui_sink_projects_friendly_status_only_once_per_stage() {
        use crate::agent_runtime::protocol::AgentEventPayload;

        let labels = WritingStageLabels {
            start: "正在检查语法…",
            analyzing: "正在判断表达是否地道、更正式…",
            finishing: "正在整理检查结果…",
        };
        let mut analysis_started = false;

        // TurnStarted → 起始阶段。
        let event = AgentEvent::new(
            "run-1",
            Some(1),
            1,
            AgentEventPayload::TurnStarted { turn_index: 1 },
        )
        .expect("事件必须有效");
        let projected =
            project_writing_ui_event(&event, labels, &mut analysis_started).expect("应有起始状态");
        match projected {
            WritingStreamEvent::Status { label } => assert_eq!(label, "正在检查语法…"),
            other => panic!("意外事件：{other:?}"),
        }

        // 首个文本增量 → 分析阶段，且只发一次。
        let event = AgentEvent::new(
            "run-1",
            Some(1),
            2,
            AgentEventPayload::AssistantTextDelta {
                text: "{\"issues\":".into(),
            },
        )
        .expect("事件必须有效");
        let projected = project_writing_ui_event(&event, labels, &mut analysis_started)
            .expect("应有分析阶段状态");
        assert!(
            matches!(projected, WritingStreamEvent::Status { label } if label.contains("正在判断表达"))
        );
        // 第二个增量不再重复发分析阶段。
        let another_delta = AgentEvent::new(
            "run-1",
            Some(1),
            3,
            AgentEventPayload::AssistantTextDelta { text: "[".into() },
        )
        .expect("事件必须有效");
        assert!(project_writing_ui_event(&another_delta, labels, &mut analysis_started).is_none());

        // 文本完成 → 整理阶段。
        let completed = AgentEvent::new(
            "run-1",
            Some(1),
            4,
            AgentEventPayload::AssistantTextCompleted {
                text: "full json".into(),
            },
        )
        .expect("事件必须有效");
        let projected = project_writing_ui_event(&completed, labels, &mut analysis_started)
            .expect("应有整理阶段状态");
        assert!(
            matches!(projected, WritingStreamEvent::Status { label } if label == "正在整理检查结果…")
        );

        // 其他内部事件（RunStarted）不投影到写作中间状态，终态由调用方处理。
        let run_started = AgentEvent::new(
            "run-1",
            None,
            5,
            AgentEventPayload::RunStarted {
                surface: crate::agent_runtime::protocol::AgentSurface::WritingCoach,
                authority: AuthorityRef::writing(1, 0, "d", 0, None, 1),
            },
        )
        .expect("事件必须有效");
        assert!(project_writing_ui_event(&run_started, labels, &mut analysis_started).is_none());
    }
}
