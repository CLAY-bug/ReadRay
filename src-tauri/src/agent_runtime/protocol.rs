//! 与业务 surface 无关的 Agent Runtime 协议草案。
//!
//! 任务 0 只定义数据边界和确定性校验。这里不驱动模型、工具或数据库；正式
//! Agent loop 必须等协议评审通过后再在独立任务中实现。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const FIRST_VERSION_MAX_MODEL_TURNS: u32 = 8;
pub const FIRST_VERSION_MAX_TOOL_CALLS: u32 = 16;
pub const FIRST_VERSION_MAX_PARALLEL_TOOLS: u16 = 4;
pub const FIRST_VERSION_TOOL_TIMEOUT_MS: u64 = 30_000;
pub const FIRST_VERSION_RUN_TIMEOUT_MS: u64 = 180_000;
pub const FIRST_VERSION_MAX_TOOL_RESULT_BYTES: usize = 64 * 1024;
pub const FIRST_VERSION_MAX_CONTEXT_BYTES: usize = 128 * 1024;
pub const FIRST_VERSION_MAX_TRANSIENT_RETRIES: u8 = 1;
pub const FIRST_VERSION_MAX_RETRY_BACKOFF_MS: u64 = 2_000;

/// 用户可见入口。Runtime 不根据入口推断权限，权限由 surface adapter 提供。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSurface {
    MainConversation,
    QuickAiOverlay,
    WritingCoach,
}

/// 通用对话和 Writing Coach 的权威身份。
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SurfaceAuthority {
    Conversation {
        conversation_id: i64,
        expected_user_sequence: i64,
        user_message_id: i64,
    },
    Writing {
        document_id: i64,
        expected_revision: i64,
        /// 由 surface adapter 对可见正文计算的稳定摘要；不是正文副本。
        visible_snapshot_digest: String,
        local_generation: u64,
        version_id: Option<i64>,
        request_sequence: u64,
    },
}

/// Surface-neutral 的 run authority。它只标识“结果还能否发布”，不授予工具权限。
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorityRef {
    pub surface: AgentSurface,
    pub authority: SurfaceAuthority,
}

impl AuthorityRef {
    pub fn conversation(
        surface: AgentSurface,
        conversation_id: i64,
        expected_user_sequence: i64,
        user_message_id: i64,
    ) -> Self {
        Self {
            surface,
            authority: SurfaceAuthority::Conversation {
                conversation_id,
                expected_user_sequence,
                user_message_id,
            },
        }
    }

    pub fn writing(
        document_id: i64,
        expected_revision: i64,
        visible_snapshot_digest: impl Into<String>,
        local_generation: u64,
        version_id: Option<i64>,
        request_sequence: u64,
    ) -> Self {
        Self {
            surface: AgentSurface::WritingCoach,
            authority: SurfaceAuthority::Writing {
                document_id,
                expected_revision,
                visible_snapshot_digest: visible_snapshot_digest.into(),
                local_generation,
                version_id,
                request_sequence,
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match (&self.surface, &self.authority) {
            (
                AgentSurface::MainConversation | AgentSurface::QuickAiOverlay,
                SurfaceAuthority::Conversation {
                    conversation_id,
                    expected_user_sequence,
                    user_message_id,
                },
            ) => {
                if *conversation_id <= 0 || *expected_user_sequence <= 0 || *user_message_id <= 0 {
                    return Err("conversation authority 的 ID 和 sequence 必须为正数。".to_string());
                }
            }
            (
                AgentSurface::WritingCoach,
                SurfaceAuthority::Writing {
                    document_id,
                    expected_revision,
                    visible_snapshot_digest,
                    version_id,
                    request_sequence,
                    ..
                },
            ) => {
                if *document_id <= 0 || *expected_revision < 0 || *request_sequence == 0 {
                    return Err(
                        "writing authority 的 document、revision 或 request sequence 无效。"
                            .to_string(),
                    );
                }
                if visible_snapshot_digest.trim().is_empty() || visible_snapshot_digest.len() > 256
                {
                    return Err("writing authority 必须包含短的可见正文摘要。".to_string());
                }
                if version_id.is_some_and(|id| id <= 0) {
                    return Err("writing authority 的 version ID 必须为正数。".to_string());
                }
            }
            _ => return Err("surface 与 authority kind 不匹配。".to_string()),
        }
        Ok(())
    }
}

/// 业务面适配器的最小协议边界。任务 0 只规定身份与 surface，不规定如何读取或
/// 提交 Conversation/Writing 数据；正式实现必须在后续任务中复用现有 repository。
pub trait AgentSurfaceAdapter {
    fn surface(&self) -> AgentSurface;
    fn authority_ref(&self) -> &AuthorityRef;

    fn validate_authority(&self) -> Result<(), String> {
        let authority = self.authority_ref();
        if authority.surface != self.surface() {
            return Err("surface adapter 与 authority surface 不匹配。".to_string());
        }
        authority.validate()
    }
}

/// Provider 输出的 token/tool/source 事件。reasoning 只存在于 provider 内存边界，
/// 不应直接转换成 AgentEvent 发给 UI。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ModelEvent {
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCall { call: ToolCall },
    ToolCallArgumentsDelta { tool_call_id: String, delta: String },
    Usage { usage: ModelUsage },
    SourceMetadata { source: SourceMetadata },
    Completed { reason: ModelFinishReason },
}

impl ModelEvent {
    pub(crate) fn validate(&self) -> Result<(), String> {
        match self {
            Self::TextDelta { text } | Self::ReasoningDelta { text } => {
                if text.is_empty() {
                    return Err("ModelEvent 文本增量不能为空。".to_string());
                }
            }
            Self::ToolCall { call } => call.validate()?,
            Self::ToolCallArgumentsDelta {
                tool_call_id,
                delta,
            } => {
                validate_id("tool_call_id", tool_call_id)?;
                if delta.is_empty() {
                    return Err("tool call arguments 增量不能为空。".to_string());
                }
            }
            Self::Usage { usage } => usage.validate()?,
            Self::SourceMetadata { source } => source.validate()?,
            Self::Completed { .. } => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ModelFinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Provider(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

impl ModelUsage {
    pub fn validate(&self) -> Result<(), String> {
        let expected = self
            .prompt_tokens
            .checked_add(self.completion_tokens)
            .ok_or_else(|| "ModelUsage Token 数溢出。".to_string())?;
        if self.total_tokens != expected {
            return Err("ModelUsage total_tokens 与输入、输出 Token 不一致。".to_string());
        }
        Ok(())
    }
}

/// Provider function tool 的一次完整调用。arguments 必须在最终执行边界再次校验。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

impl ToolCall {
    pub fn validate(&self) -> Result<(), String> {
        validate_id("tool call id", &self.id)?;
        validate_tool_name(&self.name)?;
        if !self.arguments.is_object() {
            return Err(format!(
                "工具 {} 的 arguments 必须是 JSON object。",
                self.name
            ));
        }
        if serde_json::to_vec(&self.arguments)
            .map_err(|error| format!("工具参数无法序列化：{error}"))?
            .len()
            > 16 * 1024
        {
            return Err("工具 arguments 超过 16 KiB 协议上限。".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProvenance {
    LocalFact,
    ExternalSearch,
    ExternalPage,
    Provider,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub is_error: bool,
    pub is_truncated: bool,
    pub content: String,
    pub provenance: ToolProvenance,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: u64,
    pub details: Option<Value>,
    pub error: Option<AgentError>,
}

impl ToolResult {
    pub fn success(
        call: &ToolCall,
        content: impl Into<String>,
        provenance: ToolProvenance,
        started_at_unix_ms: u64,
        finished_at_unix_ms: u64,
    ) -> Self {
        Self {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            is_error: false,
            is_truncated: false,
            content: content.into(),
            provenance,
            started_at_unix_ms,
            finished_at_unix_ms,
            details: None,
            error: None,
        }
    }

    pub fn failure(
        call: &ToolCall,
        error: AgentError,
        started_at_unix_ms: u64,
        finished_at_unix_ms: u64,
    ) -> Self {
        Self {
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            is_error: true,
            is_truncated: false,
            content: error.message.clone(),
            provenance: ToolProvenance::Provider,
            started_at_unix_ms,
            finished_at_unix_ms,
            details: None,
            error: Some(error),
        }
    }

    pub fn validate(&self, budget: &RunBudget) -> Result<(), String> {
        validate_id("tool_call_id", &self.tool_call_id)?;
        validate_tool_name(&self.tool_name)?;
        if self.finished_at_unix_ms < self.started_at_unix_ms {
            return Err("ToolResult finished_at 不能早于 started_at。".to_string());
        }
        if self.content.len() > budget.max_tool_result_bytes {
            return Err("ToolResult content 超过当前 run 的字节预算。".to_string());
        }
        if self.is_error != self.error.is_some() {
            return Err("ToolResult 的 is_error 与 error 字段不一致。".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMetadata {
    pub source_id: String,
    pub title: String,
    pub url: String,
    pub site_name: Option<String>,
    pub published_at: Option<String>,
    pub retrieved_at_unix_ms: u64,
    pub content_type: Option<String>,
}

impl SourceMetadata {
    pub fn validate(&self) -> Result<(), String> {
        validate_id("source_id", &self.source_id)?;
        if self.title.trim().is_empty() || self.title.len() > 512 {
            return Err("source title 不能为空且不能超过 512 字节。".to_string());
        }
        validate_source_url(&self.url)
    }
}

/// 统一错误分类，来源于方案第 17 节；它不承载 API key、prompt 或私有推理。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentErrorKind {
    UserAborted,
    ProviderTimeout,
    ProviderNetwork,
    ProviderRateLimited,
    ProviderAuthFailed,
    ProviderProtocolError,
    ContextOverflow,
    UnknownTool,
    ToolSchemaInvalid,
    ToolPolicyDenied,
    ToolTimeout,
    ToolExecutionFailed,
    NetworkBlocked,
    ContentExtractFailed,
    PersistenceFailed,
    RunBudgetExceeded,
}

impl AgentErrorKind {
    pub fn is_retryable_without_side_effect(&self) -> bool {
        matches!(
            self,
            Self::ProviderTimeout | Self::ProviderNetwork | Self::ProviderRateLimited
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentError {
    pub kind: AgentErrorKind,
    pub message: String,
}

impl AgentError {
    pub fn new(kind: AgentErrorKind, message: impl Into<String>) -> Result<Self, String> {
        let message = message.into();
        if message.trim().is_empty() {
            return Err("AgentError message 不能为空。".to_string());
        }
        if message.len() > 2_000 {
            return Err("AgentError message 不能超过 2,000 字节。".to_string());
        }
        Ok(Self { kind, message })
    }

    pub fn termination_reason(&self) -> TerminationReason {
        TerminationReason::from_error_kind(&self.kind)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    FinalAnswer,
    UserAborted,
    ProviderTimeout,
    ProviderNetwork,
    ProviderRateLimited,
    ProviderAuthFailed,
    ProviderProtocolError,
    ContextOverflow,
    UnknownTool,
    ToolSchemaInvalid,
    ToolPolicyDenied,
    ToolTimeout,
    ToolExecutionFailed,
    NetworkBlocked,
    ContentExtractFailed,
    PersistenceFailed,
    RunBudgetExceeded,
}

impl TerminationReason {
    pub fn from_error_kind(kind: &AgentErrorKind) -> Self {
        match kind {
            AgentErrorKind::UserAborted => Self::UserAborted,
            AgentErrorKind::ProviderTimeout => Self::ProviderTimeout,
            AgentErrorKind::ProviderNetwork => Self::ProviderNetwork,
            AgentErrorKind::ProviderRateLimited => Self::ProviderRateLimited,
            AgentErrorKind::ProviderAuthFailed => Self::ProviderAuthFailed,
            AgentErrorKind::ProviderProtocolError => Self::ProviderProtocolError,
            AgentErrorKind::ContextOverflow => Self::ContextOverflow,
            AgentErrorKind::UnknownTool => Self::UnknownTool,
            AgentErrorKind::ToolSchemaInvalid => Self::ToolSchemaInvalid,
            AgentErrorKind::ToolPolicyDenied => Self::ToolPolicyDenied,
            AgentErrorKind::ToolTimeout => Self::ToolTimeout,
            AgentErrorKind::ToolExecutionFailed => Self::ToolExecutionFailed,
            AgentErrorKind::NetworkBlocked => Self::NetworkBlocked,
            AgentErrorKind::ContentExtractFailed => Self::ContentExtractFailed,
            AgentErrorKind::PersistenceFailed => Self::PersistenceFailed,
            AgentErrorKind::RunBudgetExceeded => Self::RunBudgetExceeded,
        }
    }
}

/// 单个 run 的硬预算。默认值只作为第一版协议建议，不代表生产调优结论。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunBudget {
    pub max_model_turns: u32,
    pub max_tool_calls: u32,
    pub max_parallel_tools: u16,
    pub tool_timeout_ms: u64,
    pub run_timeout_ms: u64,
    pub max_tool_result_bytes: usize,
    pub max_context_bytes: usize,
    pub max_transient_retries: u8,
    pub max_retry_backoff_ms: u64,
}

impl RunBudget {
    pub const fn first_version() -> Self {
        Self {
            max_model_turns: FIRST_VERSION_MAX_MODEL_TURNS,
            max_tool_calls: FIRST_VERSION_MAX_TOOL_CALLS,
            max_parallel_tools: FIRST_VERSION_MAX_PARALLEL_TOOLS,
            tool_timeout_ms: FIRST_VERSION_TOOL_TIMEOUT_MS,
            run_timeout_ms: FIRST_VERSION_RUN_TIMEOUT_MS,
            max_tool_result_bytes: FIRST_VERSION_MAX_TOOL_RESULT_BYTES,
            max_context_bytes: FIRST_VERSION_MAX_CONTEXT_BYTES,
            max_transient_retries: FIRST_VERSION_MAX_TRANSIENT_RETRIES,
            max_retry_backoff_ms: FIRST_VERSION_MAX_RETRY_BACKOFF_MS,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.max_model_turns == 0
            || self.max_tool_calls == 0
            || self.max_parallel_tools == 0
            || self.tool_timeout_ms == 0
            || self.run_timeout_ms == 0
            || self.max_tool_result_bytes == 0
            || self.max_context_bytes == 0
        {
            return Err("RunBudget 的所有硬上限都必须大于 0。".to_string());
        }
        if u32::from(self.max_parallel_tools) > self.max_tool_calls {
            return Err("max_parallel_tools 不能超过 max_tool_calls。".to_string());
        }
        if self.tool_timeout_ms > self.run_timeout_ms {
            return Err("tool_timeout_ms 不能超过 run_timeout_ms。".to_string());
        }
        Ok(())
    }
}

/// Provider 状态只允许留在内存中的 provider adapter；不得持久化或发送给 UI。
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderContinuationState {
    pub provider: String,
    /// Provider 返回的 response id；DeepSeek Responses 只观察并保存这一标识。
    pub response_id: Option<String>,
    /// Responses output item/tool-call 的结构化状态，不能直接当作工具结果。
    pub tool_call_state: Option<Value>,
    /// 推理模型工具链可能要求回传的私有 reasoning；永不进入 AgentEvent/UI/SQLite。
    pub private_reasoning: Option<String>,
    /// 仅供明确支持续接语义的其他 provider 使用；DeepSeek Responses 是 stateless，
    /// 不发送或推断 `previous_response_id`。
    pub provider_extensions: Option<Value>,
}

impl ProviderContinuationState {
    pub fn validate(&self) -> Result<(), String> {
        if self.provider.trim().is_empty() || self.provider.len() > 80 {
            return Err("provider continuation state 的 provider 无效。".to_string());
        }
        if self
            .response_id
            .as_deref()
            .is_some_and(is_invalid_provider_id)
        {
            return Err("provider continuation id 无效。".to_string());
        }
        if self
            .tool_call_state
            .as_ref()
            .is_some_and(|state| !state.is_object())
        {
            return Err("provider tool_call_state 必须是 JSON object。".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventType {
    RunStarted,
    TurnStarted,
    AssistantTextDelta,
    AssistantTextCompleted,
    ToolCallStarted,
    ToolCallProgress,
    ToolCallCompleted,
    ToolCallFailed,
    SourcesUpdated,
    RunStopped,
    RunTruncated,
    RunFailed,
    RunCompleted,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AgentEventPayload {
    RunStarted {
        surface: AgentSurface,
        authority: AuthorityRef,
    },
    TurnStarted {
        turn_index: u32,
    },
    AssistantTextDelta {
        text: String,
    },
    AssistantTextCompleted {
        text: String,
    },
    ToolCallStarted {
        call: ToolCall,
    },
    ToolCallProgress {
        tool_call_id: String,
        message: String,
    },
    ToolCallCompleted {
        result: ToolResult,
    },
    ToolCallFailed {
        result: ToolResult,
    },
    SourcesUpdated {
        sources: Vec<SourceMetadata>,
    },
    RunStopped {
        reason: TerminationReason,
    },
    RunTruncated {
        reason: TerminationReason,
    },
    RunFailed {
        error: AgentError,
    },
    RunCompleted {
        text: String,
        usage: Option<ModelUsage>,
    },
}

impl AgentEventPayload {
    fn event_type(&self) -> AgentEventType {
        match self {
            Self::RunStarted { .. } => AgentEventType::RunStarted,
            Self::TurnStarted { .. } => AgentEventType::TurnStarted,
            Self::AssistantTextDelta { .. } => AgentEventType::AssistantTextDelta,
            Self::AssistantTextCompleted { .. } => AgentEventType::AssistantTextCompleted,
            Self::ToolCallStarted { .. } => AgentEventType::ToolCallStarted,
            Self::ToolCallProgress { .. } => AgentEventType::ToolCallProgress,
            Self::ToolCallCompleted { .. } => AgentEventType::ToolCallCompleted,
            Self::ToolCallFailed { .. } => AgentEventType::ToolCallFailed,
            Self::SourcesUpdated { .. } => AgentEventType::SourcesUpdated,
            Self::RunStopped { .. } => AgentEventType::RunStopped,
            Self::RunTruncated { .. } => AgentEventType::RunTruncated,
            Self::RunFailed { .. } => AgentEventType::RunFailed,
            Self::RunCompleted { .. } => AgentEventType::RunCompleted,
        }
    }

    fn validate(&self, budget: &RunBudget) -> Result<(), String> {
        match self {
            Self::RunStarted { authority, .. } => authority.validate()?,
            Self::TurnStarted { turn_index } if *turn_index == 0 => {
                return Err("turn_index 必须从 1 开始。".to_string())
            }
            Self::AssistantTextDelta { text } | Self::AssistantTextCompleted { text } => {
                if text.is_empty() {
                    return Err("assistant 文本事件不能为空。".to_string());
                }
            }
            Self::ToolCallStarted { call } => call.validate()?,
            Self::ToolCallProgress {
                tool_call_id,
                message,
            } => {
                validate_id("tool_call_id", tool_call_id)?;
                if message.is_empty() {
                    return Err("tool progress message 不能为空。".to_string());
                }
            }
            Self::ToolCallCompleted { result } | Self::ToolCallFailed { result } => {
                result.validate(budget)?
            }
            Self::SourcesUpdated { sources } => {
                for source in sources {
                    source.validate()?;
                }
            }
            Self::RunFailed { error } => {
                if error.message.trim().is_empty() {
                    return Err("run failed 必须包含错误说明。".to_string());
                }
                validate_run_failed_kind(&error.kind)?;
            }
            Self::RunCompleted { text, usage } => {
                if text.trim().is_empty() {
                    return Err("run completed 必须包含非空最终回答。".to_string());
                }
                if let Some(usage) = usage {
                    usage.validate()?
                }
            }
            Self::RunStopped { reason } => validate_run_stopped_reason(reason)?,
            Self::RunTruncated { reason } => validate_run_truncated_reason(reason)?,
            Self::TurnStarted { .. } => {}
        }
        Ok(())
    }
}

/// 对前端和审计可见的统一 envelope。provider continuation/reasoning 不在此协议中。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub run_id: String,
    pub turn_id: Option<u32>,
    pub step_sequence: u64,
    pub event_type: AgentEventType,
    pub payload: AgentEventPayload,
}

impl AgentEvent {
    pub fn new(
        run_id: impl Into<String>,
        turn_id: Option<u32>,
        step_sequence: u64,
        payload: AgentEventPayload,
    ) -> Result<Self, String> {
        let run_id = run_id.into();
        validate_id("run_id", &run_id)?;
        if step_sequence == 0 {
            return Err("AgentEvent step_sequence 必须从 1 开始。".to_string());
        }
        if turn_id.is_some_and(|turn| turn == 0) {
            return Err("AgentEvent turn_id 必须从 1 开始。".to_string());
        }
        Ok(Self {
            run_id,
            turn_id,
            step_sequence,
            event_type: payload.event_type(),
            payload,
        })
    }

    pub fn validate(&self, budget: &RunBudget) -> Result<(), String> {
        validate_id("run_id", &self.run_id)?;
        if self.step_sequence == 0 {
            return Err("AgentEvent step_sequence 必须大于 0。".to_string());
        }
        if self.event_type != self.payload.event_type() {
            return Err("AgentEvent event_type 与 payload 不匹配。".to_string());
        }
        self.payload.validate(budget)
    }
}

pub fn validate_event_sequence(events: &[AgentEvent], budget: &RunBudget) -> Result<(), String> {
    budget.validate()?;
    let Some(first) = events.first() else {
        return Err("AgentEvent 序列不能为空。".to_string());
    };
    let run_id = &first.run_id;
    let mut previous = 0;
    let mut run_started = false;
    let mut current_turn = None;
    let mut assistant_text_completed = false;
    let mut turn_had_tool_calls = false;
    let mut active_tool_calls = BTreeMap::new();
    let mut seen_tool_calls = BTreeSet::new();
    let mut model_turns = 0_u32;
    let mut tool_calls = 0_u32;
    let mut terminal_index = None;

    for (index, event) in events.iter().enumerate() {
        event.validate(budget)?;
        if event.run_id != *run_id {
            return Err("同一 replay 序列不能混用多个 run_id。".to_string());
        }
        if event.step_sequence <= previous {
            return Err("AgentEvent step_sequence 必须严格递增。".to_string());
        }
        previous = event.step_sequence;

        if terminal_index.is_some() {
            return Err("terminal AgentEvent 必须唯一且位于序列末尾。".to_string());
        }

        match &event.payload {
            AgentEventPayload::RunStarted { surface, authority } => {
                if index != 0 || run_started {
                    return Err("RunStarted 必须只出现一次且位于序列开头。".to_string());
                }
                if event.turn_id.is_some() {
                    return Err("RunStarted 不得携带 turn_id。".to_string());
                }
                if surface != &authority.surface {
                    return Err("RunStarted surface 必须与 authority.surface 一致。".to_string());
                }
                run_started = true;
            }
            AgentEventPayload::TurnStarted { turn_index } => {
                if !run_started || !active_tool_calls.is_empty() {
                    return Err(
                        "TurnStarted 必须在上一个 turn/tool 生命周期结束后出现。".to_string()
                    );
                }
                if current_turn.is_some() && !turn_had_tool_calls {
                    return Err(
                        "没有 tool 结果时不能重复开始同一个 model turn 生命周期。".to_string()
                    );
                }
                if event.turn_id != Some(*turn_index) || *turn_index != model_turns + 1 {
                    return Err("TurnStarted turn_id 必须按 1 开始并连续递增。".to_string());
                }
                model_turns += 1;
                if model_turns > budget.max_model_turns {
                    return Err("模型 turn 数超过 RunBudget 上限。".to_string());
                }
                current_turn = Some(*turn_index);
                assistant_text_completed = false;
                turn_had_tool_calls = false;
            }
            AgentEventPayload::AssistantTextDelta { .. } => {
                require_current_turn(event, current_turn)?;
                if assistant_text_completed {
                    return Err(
                        "AssistantTextDelta 不能出现在 assistant_text_completed 之后。".to_string(),
                    );
                }
            }
            AgentEventPayload::AssistantTextCompleted { .. } => {
                require_current_turn(event, current_turn)?;
                if assistant_text_completed || !active_tool_calls.is_empty() {
                    return Err("AssistantTextCompleted 的 turn 生命周期无效。".to_string());
                }
                assistant_text_completed = true;
            }
            AgentEventPayload::ToolCallStarted { call } => {
                require_current_turn(event, current_turn)?;
                if assistant_text_completed {
                    return Err("tool call 不能出现在 assistant_text_completed 之后。".to_string());
                }
                call.validate()?;
                if !seen_tool_calls.insert(call.id.clone()) {
                    return Err("同一 tool_call_id 不能重复开始。".to_string());
                }
                active_tool_calls.insert(call.id.clone(), call.name.clone());
                turn_had_tool_calls = true;
                tool_calls += 1;
                if tool_calls > budget.max_tool_calls {
                    return Err("tool call 数超过 RunBudget 上限。".to_string());
                }
                if active_tool_calls.len() > usize::from(budget.max_parallel_tools) {
                    return Err("同时运行的 tool 数超过 max_parallel_tools。".to_string());
                }
            }
            AgentEventPayload::ToolCallProgress { tool_call_id, .. } => {
                require_current_turn(event, current_turn)?;
                if !active_tool_calls.contains_key(tool_call_id) {
                    return Err("tool progress 必须对应正在运行的 tool call。".to_string());
                }
            }
            AgentEventPayload::ToolCallCompleted { result } => {
                require_current_turn(event, current_turn)?;
                if result.is_error {
                    return Err("ToolCallCompleted 的 result 必须是成功结果。".to_string());
                }
                result.validate(budget)?;
                let Some(expected_tool_name) = active_tool_calls.remove(&result.tool_call_id)
                else {
                    return Err("tool completed 必须对应 active tool call。".to_string());
                };
                if result.tool_name != expected_tool_name {
                    return Err(
                        "tool result 的 tool_name 必须与 ToolCallStarted 一致。".to_string()
                    );
                }
            }
            AgentEventPayload::ToolCallFailed { result } => {
                require_current_turn(event, current_turn)?;
                if !result.is_error {
                    return Err("ToolCallFailed 的 result 必须是错误结果。".to_string());
                }
                result.validate(budget)?;
                if let Some(expected_tool_name) = active_tool_calls.remove(&result.tool_call_id) {
                    if result.tool_name != expected_tool_name {
                        return Err(
                            "tool result 的 tool_name 必须与 ToolCallStarted 一致。".to_string()
                        );
                    }
                } else {
                    // 参数 schema 在真正启动工具前就可能失败；这种显式 preflight
                    // rejection 没有 active call，但仍计入本 run 的 tool-call 上限。
                    if !result
                        .error
                        .as_ref()
                        .is_some_and(|error| error.kind == AgentErrorKind::ToolSchemaInvalid)
                    {
                        return Err("tool failed 必须对应 active tool call。".to_string());
                    }
                    if !seen_tool_calls.insert(result.tool_call_id.clone()) {
                        return Err("同一 preflight tool_call_id 不能重复失败。".to_string());
                    }
                    tool_calls += 1;
                    if tool_calls > budget.max_tool_calls {
                        return Err("tool call 数超过 RunBudget 上限。".to_string());
                    }
                }
            }
            AgentEventPayload::SourcesUpdated { .. } => {
                require_current_turn(event, current_turn)?;
            }
            AgentEventPayload::RunStopped { reason } => {
                require_run_started(run_started)?;
                if !active_tool_calls.is_empty() && *reason != TerminationReason::UserAborted {
                    return Err("非用户停止不能在 active tool call 未结束时终止。".to_string());
                }
                terminal_index = Some(index);
                current_turn = None;
                active_tool_calls.clear();
            }
            AgentEventPayload::RunTruncated { .. } => {
                require_run_started(run_started)?;
                if !active_tool_calls.is_empty() {
                    return Err("RunTruncated 不能留下未结束的 tool call。".to_string());
                }
                terminal_index = Some(index);
                current_turn = None;
            }
            AgentEventPayload::RunFailed { .. } => {
                require_run_started(run_started)?;
                if !active_tool_calls.is_empty() {
                    return Err("RunFailed 不能留下未结束的 tool call。".to_string());
                }
                terminal_index = Some(index);
                current_turn = None;
            }
            AgentEventPayload::RunCompleted { .. } => {
                require_run_started(run_started)?;
                require_current_turn(event, current_turn)?;
                if !assistant_text_completed || !active_tool_calls.is_empty() {
                    return Err(
                        "RunCompleted 必须在最终文本完成且无 active tool call 后出现。".to_string(),
                    );
                }
                terminal_index = Some(index);
                current_turn = None;
            }
        }
    }

    if !run_started {
        return Err("AgentEvent 序列缺少 RunStarted。".to_string());
    }
    if terminal_index != Some(events.len() - 1) {
        return Err("AgentEvent 序列必须以唯一 terminal 事件结束。".to_string());
    }
    Ok(())
}

fn require_run_started(run_started: bool) -> Result<(), String> {
    if run_started {
        Ok(())
    } else {
        Err("run terminal 事件必须位于 RunStarted 之后。".to_string())
    }
}

fn require_current_turn(event: &AgentEvent, current_turn: Option<u32>) -> Result<(), String> {
    if current_turn.is_some() && event.turn_id == current_turn {
        Ok(())
    } else {
        Err("AgentEvent 的 turn_id 不匹配当前 turn 生命周期。".to_string())
    }
}

fn validate_run_stopped_reason(reason: &TerminationReason) -> Result<(), String> {
    if *reason == TerminationReason::UserAborted {
        Ok(())
    } else {
        Err("RunStopped 只允许 UserAborted；其他终止必须使用 RunTruncated/RunFailed。".to_string())
    }
}

fn validate_run_truncated_reason(reason: &TerminationReason) -> Result<(), String> {
    if matches!(
        reason,
        TerminationReason::ContextOverflow | TerminationReason::RunBudgetExceeded
    ) {
        Ok(())
    } else {
        Err("RunTruncated 只允许 ContextOverflow 或 RunBudgetExceeded。".to_string())
    }
}

fn validate_run_failed_kind(kind: &AgentErrorKind) -> Result<(), String> {
    if matches!(
        kind,
        AgentErrorKind::UserAborted
            | AgentErrorKind::ContextOverflow
            | AgentErrorKind::RunBudgetExceeded
    ) {
        Err("RunFailed 的 error kind 必须与 terminal event 矩阵一致。".to_string())
    } else {
        Ok(())
    }
}

fn validate_id(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(format!("{label} 为空、过长或包含控制字符。"));
    }
    Ok(())
}

fn is_invalid_provider_id(value: &str) -> bool {
    value.trim().is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

fn validate_tool_name(value: &str) -> Result<(), String> {
    validate_id("tool name", value)?;
    if value.len() > 64
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err("tool name 只能包含 ASCII 字母、数字、下划线或连字符。".to_string());
    }
    Ok(())
}

fn validate_source_url(url: &str) -> Result<(), String> {
    if url.is_empty()
        || url.len() > 2_048
        || url
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("source URL 为空、过长或包含空白/控制字符。".to_string());
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| "source URL 只能使用 HTTP(S)。".to_string())?;
    let authority_end = rest
        .find(|character| matches!(character, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return Err("source URL 不得包含 userinfo。".to_string());
    }
    if let Some(query) = rest.split_once('?').map(|(_, query)| query) {
        let query = query.split('#').next().unwrap_or(query);
        for pair in query.split('&') {
            let key = pair.split('=').next().unwrap_or("").to_ascii_lowercase();
            if [
                "key",
                "api_key",
                "apikey",
                "token",
                "access_token",
                "auth",
                "authorization",
                "password",
                "passwd",
                "secret",
                "cookie",
                "session",
                "credential",
            ]
            .iter()
            .any(|sensitive| key == *sensitive || key.contains(sensitive))
            {
                return Err("source URL 查询参数疑似包含凭据，已拒绝。".to_string());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authority_ref_is_surface_neutral_but_validates_business_identity() {
        let conversation = AuthorityRef::conversation(AgentSurface::QuickAiOverlay, 1, 3, 9);
        assert!(conversation.validate().is_ok());
        let writing = AuthorityRef::writing(4, 2, "sha256:abc", 7, Some(11), 1);
        assert!(writing.validate().is_ok());
        let invalid = AuthorityRef {
            surface: AgentSurface::WritingCoach,
            authority: SurfaceAuthority::Conversation {
                conversation_id: 1,
                expected_user_sequence: 1,
                user_message_id: 1,
            },
        };
        assert!(invalid.validate().is_err());

        struct FakeSurfaceAdapter(AuthorityRef);
        impl AgentSurfaceAdapter for FakeSurfaceAdapter {
            fn surface(&self) -> AgentSurface {
                AgentSurface::QuickAiOverlay
            }

            fn authority_ref(&self) -> &AuthorityRef {
                &self.0
            }
        }
        assert!(FakeSurfaceAdapter(conversation)
            .validate_authority()
            .is_ok());
    }

    #[test]
    fn first_version_budget_is_finite_and_tool_result_is_bounded() {
        let budget = RunBudget::first_version();
        assert_eq!(budget.max_model_turns, 8);
        assert_eq!(budget.max_tool_calls, 16);
        assert!(budget.validate().is_ok());
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "web_search".to_string(),
            arguments: json!({"query": "Rust"}),
        };
        let result = ToolResult::success(&call, "ok", ToolProvenance::ExternalSearch, 10, 11);
        assert!(result.validate(&budget).is_ok());
    }

    #[test]
    fn agent_event_envelope_rejects_mismatched_type_and_non_monotonic_steps() {
        let authority = AuthorityRef::conversation(AgentSurface::MainConversation, 1, 1, 1);
        let first = AgentEvent::new(
            "run-1",
            None,
            1,
            AgentEventPayload::RunStarted {
                surface: AgentSurface::MainConversation,
                authority,
            },
        )
        .unwrap();
        let turn = AgentEvent::new(
            "run-1",
            Some(1),
            2,
            AgentEventPayload::TurnStarted { turn_index: 1 },
        )
        .unwrap();
        let mut second = AgentEvent::new(
            "run-1",
            Some(1),
            3,
            AgentEventPayload::AssistantTextDelta {
                text: "done".to_string(),
            },
        )
        .unwrap();
        assert!(validate_event_sequence(
            &[
                first.clone(),
                turn,
                second.clone(),
                AgentEvent::new(
                    "run-1",
                    Some(1),
                    4,
                    AgentEventPayload::AssistantTextCompleted {
                        text: "done".to_string(),
                    },
                )
                .unwrap(),
                AgentEvent::new(
                    "run-1",
                    Some(1),
                    5,
                    AgentEventPayload::RunCompleted {
                        text: "done".to_string(),
                        usage: None,
                    },
                )
                .unwrap(),
            ],
            &RunBudget::first_version()
        )
        .is_ok());
        second.step_sequence = 1;
        assert!(validate_event_sequence(&[first, second], &RunBudget::first_version()).is_err());
    }

    #[test]
    fn event_sequence_rejects_surface_terminal_and_lifecycle_violations() {
        let mismatched_surface = AgentEvent::new(
            "run-1",
            None,
            1,
            AgentEventPayload::RunStarted {
                surface: AgentSurface::WritingCoach,
                authority: AuthorityRef::conversation(AgentSurface::QuickAiOverlay, 1, 1, 1),
            },
        )
        .unwrap();
        assert!(
            validate_event_sequence(&[mismatched_surface], &RunBudget::first_version()).is_err()
        );

        let authority = AuthorityRef::conversation(AgentSurface::QuickAiOverlay, 1, 1, 1);
        let events = vec![
            AgentEvent::new(
                "run-1",
                None,
                1,
                AgentEventPayload::RunStarted {
                    surface: AgentSurface::QuickAiOverlay,
                    authority,
                },
            )
            .unwrap(),
            AgentEvent::new(
                "run-1",
                Some(1),
                2,
                AgentEventPayload::TurnStarted { turn_index: 1 },
            )
            .unwrap(),
            AgentEvent::new(
                "run-1",
                Some(1),
                3,
                AgentEventPayload::ToolCallCompleted {
                    result: ToolResult {
                        tool_call_id: "call-missing".to_string(),
                        tool_name: "get_date".to_string(),
                        is_error: false,
                        is_truncated: false,
                        content: "2026-08-16".to_string(),
                        provenance: ToolProvenance::LocalFact,
                        started_at_unix_ms: 1,
                        finished_at_unix_ms: 2,
                        details: None,
                        error: None,
                    },
                },
            )
            .unwrap(),
        ];
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_err());

        let repeated_turn_without_tool = vec![
            AgentEvent::new(
                "run-1",
                None,
                1,
                AgentEventPayload::RunStarted {
                    surface: AgentSurface::QuickAiOverlay,
                    authority: AuthorityRef::conversation(AgentSurface::QuickAiOverlay, 1, 1, 1),
                },
            )
            .unwrap(),
            AgentEvent::new(
                "run-1",
                Some(1),
                2,
                AgentEventPayload::TurnStarted { turn_index: 1 },
            )
            .unwrap(),
            AgentEvent::new(
                "run-1",
                Some(2),
                3,
                AgentEventPayload::TurnStarted { turn_index: 2 },
            )
            .unwrap(),
        ];
        assert!(
            validate_event_sequence(&repeated_turn_without_tool, &RunBudget::first_version())
                .is_err()
        );

        let mut terminal_then_late = vec![
            AgentEvent::new(
                "run-1",
                None,
                1,
                AgentEventPayload::RunStarted {
                    surface: AgentSurface::QuickAiOverlay,
                    authority: AuthorityRef::conversation(AgentSurface::QuickAiOverlay, 1, 1, 1),
                },
            )
            .unwrap(),
            AgentEvent::new(
                "run-1",
                None,
                2,
                AgentEventPayload::RunFailed {
                    error: AgentError::new(AgentErrorKind::ProviderProtocolError, "failed")
                        .unwrap(),
                },
            )
            .unwrap(),
        ];
        terminal_then_late.push(
            AgentEvent::new(
                "run-1",
                None,
                3,
                AgentEventPayload::RunStopped {
                    reason: TerminationReason::UserAborted,
                },
            )
            .unwrap(),
        );
        assert!(validate_event_sequence(&terminal_then_late, &RunBudget::first_version()).is_err());
    }

    #[test]
    fn tool_result_direction_and_name_must_match_the_started_call() {
        let budget = RunBudget::first_version();
        let authority = AuthorityRef::conversation(AgentSurface::QuickAiOverlay, 1, 1, 1);
        let start = || {
            vec![
                AgentEvent::new(
                    "run-1",
                    None,
                    1,
                    AgentEventPayload::RunStarted {
                        surface: AgentSurface::QuickAiOverlay,
                        authority: authority.clone(),
                    },
                )
                .unwrap(),
                AgentEvent::new(
                    "run-1",
                    Some(1),
                    2,
                    AgentEventPayload::TurnStarted { turn_index: 1 },
                )
                .unwrap(),
                AgentEvent::new(
                    "run-1",
                    Some(1),
                    3,
                    AgentEventPayload::ToolCallStarted {
                        call: ToolCall {
                            id: "call-1".to_string(),
                            name: "get_date".to_string(),
                            arguments: json!({}),
                        },
                    },
                )
                .unwrap(),
            ]
        };

        let failure = AgentError::new(AgentErrorKind::ToolExecutionFailed, "failed").unwrap();
        let mut completed_error = ToolResult::failure(
            &ToolCall {
                id: "call-1".to_string(),
                name: "get_date".to_string(),
                arguments: json!({}),
            },
            failure.clone(),
            1,
            2,
        );
        completed_error.tool_name = "get_date".to_string();
        let mut events = start();
        events.push(
            AgentEvent::new(
                "run-1",
                Some(1),
                4,
                AgentEventPayload::ToolCallCompleted {
                    result: completed_error,
                },
            )
            .unwrap(),
        );
        assert!(validate_event_sequence(&events, &budget).is_err());

        let mut failed_success = ToolResult::success(
            &ToolCall {
                id: "call-1".to_string(),
                name: "get_date".to_string(),
                arguments: json!({}),
            },
            "ok",
            ToolProvenance::LocalFact,
            1,
            2,
        );
        failed_success.tool_name = "get_date".to_string();
        let mut events = start();
        events.push(
            AgentEvent::new(
                "run-1",
                Some(1),
                4,
                AgentEventPayload::ToolCallFailed {
                    result: failed_success,
                },
            )
            .unwrap(),
        );
        assert!(validate_event_sequence(&events, &budget).is_err());

        let mut wrong_name = ToolResult::success(
            &ToolCall {
                id: "call-1".to_string(),
                name: "get_date".to_string(),
                arguments: json!({}),
            },
            "ok",
            ToolProvenance::LocalFact,
            1,
            2,
        );
        wrong_name.tool_name = "other_tool".to_string();
        let mut events = start();
        events.push(
            AgentEvent::new(
                "run-1",
                Some(1),
                4,
                AgentEventPayload::ToolCallCompleted { result: wrong_name },
            )
            .unwrap(),
        );
        assert!(validate_event_sequence(&events, &budget).is_err());

        let mut failed_wrong_name = ToolResult::failure(
            &ToolCall {
                id: "call-1".to_string(),
                name: "get_date".to_string(),
                arguments: json!({}),
            },
            failure,
            1,
            2,
        );
        failed_wrong_name.tool_name = "other_tool".to_string();
        let mut events = start();
        events.push(
            AgentEvent::new(
                "run-1",
                Some(1),
                4,
                AgentEventPayload::ToolCallFailed {
                    result: failed_wrong_name,
                },
            )
            .unwrap(),
        );
        assert!(validate_event_sequence(&events, &budget).is_err());
    }

    #[test]
    fn terminal_reason_and_error_kind_matrix_is_frozen() {
        let budget = RunBudget::first_version();
        assert!(AgentEventPayload::RunStopped {
            reason: TerminationReason::UserAborted,
        }
        .validate(&budget)
        .is_ok());
        assert!(AgentEventPayload::RunStopped {
            reason: TerminationReason::FinalAnswer,
        }
        .validate(&budget)
        .is_err());
        assert!(AgentEventPayload::RunTruncated {
            reason: TerminationReason::RunBudgetExceeded,
        }
        .validate(&budget)
        .is_ok());
        assert!(AgentEventPayload::RunTruncated {
            reason: TerminationReason::ProviderNetwork,
        }
        .validate(&budget)
        .is_err());

        let user_aborted = AgentError::new(AgentErrorKind::UserAborted, "aborted").unwrap();
        assert!(AgentEventPayload::RunFailed {
            error: user_aborted,
        }
        .validate(&budget)
        .is_err());
        let context_overflow =
            AgentError::new(AgentErrorKind::ContextOverflow, "overflow").unwrap();
        assert!(AgentEventPayload::RunFailed {
            error: context_overflow,
        }
        .validate(&budget)
        .is_err());
        let network = AgentError::new(AgentErrorKind::ProviderNetwork, "network").unwrap();
        assert!(AgentEventPayload::RunFailed { error: network }
            .validate(&budget)
            .is_ok());
    }

    #[test]
    fn reasoning_model_event_has_no_serialization_or_agent_log_projection() {
        let reasoning = ModelEvent::ReasoningDelta {
            text: "private chain".to_string(),
        };
        assert!(reasoning.validate().is_ok());
        let payload = AgentEventPayload::RunCompleted {
            text: "final".to_string(),
            usage: None,
        };
        let encoded = serde_json::to_string(&payload).unwrap();
        assert!(!encoded.contains("private chain"));
        // ModelEvent intentionally has no Serialize implementation. The only
        // protocol representation tested here is the public AgentEvent payload.
        assert!(!format!("{encoded}").contains("reasoning"));
    }

    #[test]
    fn continuation_state_keeps_private_reasoning_outside_agent_event() {
        let state = ProviderContinuationState {
            provider: "deepseek-responses".to_string(),
            response_id: Some("resp_1".to_string()),
            tool_call_state: Some(json!({"call_id": "call-1"})),
            private_reasoning: Some("must not be projected".to_string()),
            provider_extensions: None,
        };
        assert!(state.validate().is_ok());
        let event = serde_json::to_value(AgentEventPayload::RunCompleted {
            text: "answer".to_string(),
            usage: None,
        })
        .unwrap();
        assert!(!event.to_string().contains("must not be projected"));
    }

    #[test]
    fn source_url_validation_rejects_userinfo_sensitive_query_and_oversized_url() {
        let source = |url: String| SourceMetadata {
            source_id: "source-1".to_string(),
            title: "Example".to_string(),
            url,
            site_name: None,
            published_at: None,
            retrieved_at_unix_ms: 0,
            content_type: None,
        };

        assert!(source("https://example.com/article?q=rust".to_string())
            .validate()
            .is_ok());
        assert!(source("https://user:pass@example.com/article".to_string())
            .validate()
            .is_err());
        assert!(
            source("https://example.com/article?api_key=redacted".to_string())
                .validate()
                .is_err()
        );
        assert!(source(format!("https://example.com/{}", "a".repeat(2_049)))
            .validate()
            .is_err());
    }
}
