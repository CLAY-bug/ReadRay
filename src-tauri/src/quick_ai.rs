use crate::agent_runtime::chat_surface::{
    conversation_l0_tools, generate_run_id, runtime_facts, ChatPreparedTurn, ChatSurfaceAdapter,
};
use crate::agent_runtime::context::ContextAssembler;
use crate::agent_runtime::coordinator::{
    AgentDeps, AgentEventSink, AgentRunCoordinator, Cancellation, RunRequest, SystemTimeSource,
    ToolExecutionOrder,
};
use crate::agent_runtime::deepseek_gateway::DeepSeekChatGateway;
use crate::agent_runtime::gateway::ModelGateway;
use crate::agent_runtime::protocol::{
    AgentError, AgentErrorKind, AgentEvent, AgentEventPayload, AgentSurface, RunBudget,
    TerminationReason,
};
use crate::agent_runtime::run_repository::{
    AgentRunRepository, AgentRunStatus, NewRun, PersistingSink,
};
use crate::agent_runtime::tool::{CapabilityPolicy, ToolPolicy, ToolRegistry};
use crate::conversations::{
    export_snapshot_to_path, ConversationExportSummary, ConversationMessage, ConversationOrigin,
    ConversationRole, ConversationSnapshot, ConversationStore, PreparedTurn,
    RecentConversationSummary,
};
use crate::deepseek_client::{
    configured_model, parse_model_token_usage_value, post_tracked_chat_completion,
    stream_chat_completion_events,
};
use crate::learning_records::{database_path_for_app, unix_time_ms};
use crate::model_usage::{record_for_app, ModelUsageCategory};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::{future::Future, path::PathBuf};
use tauri::ipc::Channel;
use tauri::AppHandle;

const QUICK_AI_MAX_USER_MESSAGE_LEN: usize = 8_000;
const QUICK_AI_MAX_CONTEXT_MESSAGES: usize = 40;
const DEFAULT_RECENT_CONVERSATION_LIMIT: u32 = 6;
const MAX_RECENT_CONVERSATION_LIMIT: u32 = 20;
const QUICK_AI_MAX_TOKENS: u16 = 8_192;
const QUICK_AI_TEMPERATURE: f32 = 0.5;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct DeepSeekRequestMessage {
    role: String,
    content: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum QuickAiStreamEvent {
    Delta { text: String },
    Done,
    Stopped,
    Truncated,
    Error { message: String },
}

static STREAMING_ABORT_FLAGS: Mutex<Option<Vec<(i64, std::sync::Arc<AtomicBool>)>>> =
    Mutex::new(None);
static ACTIVE_STREAMING_CONVERSATIONS: Mutex<Option<Vec<i64>>> = Mutex::new(None);

fn abort_flag_for(conversation_id: i64) -> std::sync::Arc<AtomicBool> {
    let mut flags = STREAMING_ABORT_FLAGS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let slots = flags.get_or_insert_with(Vec::new);
    if let Some((_, flag)) = slots.iter().find(|(id, _)| *id == conversation_id) {
        return flag.clone();
    }
    let flag = std::sync::Arc::new(AtomicBool::new(false));
    slots.push((conversation_id, flag.clone()));
    flag
}

fn request_abort_streaming(conversation_id: i64) {
    let active = ACTIVE_STREAMING_CONVERSATIONS
        .lock()
        .map(|slots| {
            slots
                .as_ref()
                .is_some_and(|slots| slots.contains(&conversation_id))
        })
        .unwrap_or(false);
    if !active {
        return;
    }
    let mut flags = STREAMING_ABORT_FLAGS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let slots = flags.get_or_insert_with(Vec::new);
    if let Some((_, flag)) = slots.iter().find(|(id, _)| *id == conversation_id) {
        flag.store(true, Ordering::Relaxed);
    }
}

fn clear_streaming_abort(conversation_id: i64) {
    let mut flags = STREAMING_ABORT_FLAGS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let slots = flags.get_or_insert_with(Vec::new);
    if let Some(index) = slots.iter().position(|(id, _)| *id == conversation_id) {
        slots.remove(index);
    }
    let mut active = ACTIVE_STREAMING_CONVERSATIONS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let slots = active.get_or_insert_with(Vec::new);
    if let Some(index) = slots.iter().position(|id| *id == conversation_id) {
        slots.remove(index);
    }
}

fn mark_streaming_active(conversation_id: i64) {
    let mut active = ACTIVE_STREAMING_CONVERSATIONS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let slots = active.get_or_insert_with(Vec::new);
    if !slots.contains(&conversation_id) {
        slots.push(conversation_id);
    }
}

#[derive(Clone)]
struct QuickAiStreamSender {
    channel: Channel<QuickAiStreamEvent>,
}

impl QuickAiStreamSender {
    fn send(&self, event: QuickAiStreamEvent) -> bool {
        self.channel.send(event).is_ok()
    }
}

#[derive(Debug, Deserialize)]
struct QuickAiChatResponse {
    choices: Vec<QuickAiChoice>,
}

#[derive(Debug, Deserialize)]
struct QuickAiChoice {
    finish_reason: Option<String>,
    message: QuickAiResponseMessage,
}

#[derive(Debug, Deserialize)]
struct QuickAiResponseMessage {
    content: Option<String>,
}

#[tauri::command]
pub fn create_quick_ai_conversation(
    app: AppHandle,
    origin: ConversationOrigin,
) -> Result<ConversationSnapshot, String> {
    ConversationStore::open_for_app(&app)?.create_with_origin(&configured_model(), origin)
}

#[tauri::command]
pub fn get_quick_ai_conversation(
    app: AppHandle,
    conversation_id: i64,
) -> Result<Option<ConversationSnapshot>, String> {
    ConversationStore::open_for_app(&app)?.get(conversation_id)
}

#[tauri::command]
pub fn list_recent_quick_ai_conversations(
    app: AppHandle,
    limit: Option<u32>,
    origin: Option<ConversationOrigin>,
) -> Result<Vec<RecentConversationSummary>, String> {
    let limit = resolve_recent_conversation_limit(limit)?;
    ConversationStore::open_for_app(&app)?.list_recent(limit, origin)
}

#[tauri::command]
pub fn list_all_quick_ai_conversations(
    app: AppHandle,
    origin: Option<ConversationOrigin>,
) -> Result<Vec<RecentConversationSummary>, String> {
    ConversationStore::open_for_app(&app)?.list_all(origin)
}

#[tauri::command]
pub fn rename_quick_ai_conversation(
    app: AppHandle,
    conversation_id: i64,
    title: String,
) -> Result<ConversationSnapshot, String> {
    ConversationStore::open_for_app(&app)?.rename(conversation_id, &title)
}

#[tauri::command]
pub fn delete_quick_ai_conversation(app: AppHandle, conversation_id: i64) -> Result<bool, String> {
    ConversationStore::open_for_app(&app)?.delete(conversation_id)
}

#[tauri::command]
pub fn export_quick_ai_conversation(
    app: AppHandle,
    conversation_id: i64,
    file_path: String,
) -> Result<ConversationExportSummary, String> {
    if file_path.trim().is_empty() {
        return Err("Quick AI 导出路径不能为空。".to_string());
    }
    let snapshot = ConversationStore::open_for_app(&app)?.get_required(conversation_id)?;
    export_snapshot_to_path(&snapshot, &PathBuf::from(file_path))
}

fn resolve_recent_conversation_limit(limit: Option<u32>) -> Result<u32, String> {
    let limit = limit.unwrap_or(DEFAULT_RECENT_CONVERSATION_LIMIT);
    if limit == 0 || limit > MAX_RECENT_CONVERSATION_LIMIT {
        return Err(format!(
            "最近 Quick AI 对话数量必须在 1 到 {MAX_RECENT_CONVERSATION_LIMIT} 之间。"
        ));
    }
    Ok(limit)
}

#[tauri::command]
pub async fn send_quick_ai_message(
    app: AppHandle,
    conversation_id: i64,
    expected_user_sequence: i64,
    content: String,
) -> Result<ConversationSnapshot, String> {
    let usage_app = app.clone();
    send_with_reply_provider(
        || ConversationStore::open_for_app(&app),
        conversation_id,
        expected_user_sequence,
        &content,
        move |model, history| async move {
            request_quick_ai_reply(&usage_app, &model, &history).await
        },
    )
    .await
}

#[tauri::command]
pub async fn send_quick_ai_message_streaming(
    app: AppHandle,
    conversation_id: i64,
    expected_user_sequence: i64,
    content: String,
    channel: Channel<QuickAiStreamEvent>,
) -> Result<ConversationSnapshot, String> {
    let sender = QuickAiStreamSender { channel };
    let abort_flag = abort_flag_for(conversation_id);
    let usage_app = app.clone();
    send_with_reply_provider(
        || ConversationStore::open_for_app(&app),
        conversation_id,
        expected_user_sequence,
        &content,
        move |model, history| async move {
            mark_streaming_active(conversation_id);
            let result = stream_quick_ai_reply(
                &usage_app,
                &model,
                &history,
                &sender,
                &abort_flag,
                conversation_id,
            )
            .await;
            clear_streaming_abort(conversation_id);
            result
        },
    )
    .await
}

#[tauri::command]
pub fn abort_quick_ai_streaming(conversation_id: i64) -> Result<(), String> {
    if conversation_id <= 0 {
        return Err("Quick AI 会话 ID 无效。".to_string());
    }
    request_abort_streaming(conversation_id);
    Ok(())
}

/// Agent 正式链路：prepare_turn → AgentRun → complete_turn（AGENT_RUNTIME_UPGRADE_PLAN §19 任务 2）。
/// 旧非 Agent 路径（send_quick_ai_message / send_quick_ai_message_streaming）保留为
/// 受控回退；前端统一事件 envelope 展示不属于本任务，标注延后。
#[tauri::command]
pub async fn send_quick_ai_message_agent(
    app: AppHandle,
    conversation_id: i64,
    expected_user_sequence: i64,
    content: String,
    channel: Channel<QuickAiStreamEvent>,
) -> Result<ConversationSnapshot, String> {
    let sender = QuickAiStreamSender { channel };
    let abort_flag = abort_flag_for(conversation_id);
    let database_path = database_path_for_app(&app)?;
    let app_version = app.package_info().version.to_string();
    let model = configured_model();
    tauri::async_runtime::spawn_blocking(move || {
        // 同步内核在 blocking 线程运行；真实 gateway 内部 block_on HTTP SSE 流。
        let mut gateway = DeepSeekChatGateway::new(model, Some(&app));
        let registry = conversation_l0_tools(app_version.clone());
        let mut ui = AgentUiSink { sender: &sender };
        run_agent_session_core(
            || ConversationStore::open_for_app(&app),
            &database_path,
            &app_version,
            conversation_id,
            expected_user_sequence,
            &content,
            &abort_flag,
            &mut gateway,
            &mut ui,
            &registry,
        )
    })
    .await
    .map_err(|error| format!("Agent 会话任务执行失败：{error}"))?
}

/// 把内核 AgentEvent 映射为既有 QuickAiStreamEvent 协议（前端协议切换延后）。
struct AgentUiSink<'a> {
    sender: &'a QuickAiStreamSender,
}

impl AgentEventSink for AgentUiSink<'_> {
    fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError> {
        match &event.payload {
            AgentEventPayload::AssistantTextDelta { text } => {
                if !self
                    .sender
                    .send(QuickAiStreamEvent::Delta { text: text.clone() })
                {
                    return Err(agent_stream_error("Agent 流式事件无法送达。"));
                }
            }
            AgentEventPayload::RunCompleted { .. } => {
                self.sender.send(QuickAiStreamEvent::Done);
            }
            AgentEventPayload::RunStopped { .. } => {
                self.sender.send(QuickAiStreamEvent::Stopped);
            }
            AgentEventPayload::RunTruncated { .. } => {
                self.sender.send(QuickAiStreamEvent::Truncated);
            }
            AgentEventPayload::RunFailed { error } => {
                self.sender.send(QuickAiStreamEvent::Error {
                    message: error.message.clone(),
                });
            }
            // 工具事件与来源展示属于后续前端协议任务，当前不发送。
            _ => {}
        }
        Ok(())
    }
}

/// 持久化 + UI 的组合 sink：持久化失败时快速失败（不伪装成功），不再发送 UI 事件。
struct SessionSink<'a, 'b> {
    persisting: &'a mut PersistingSink<'b>,
    ui: &'a mut dyn AgentEventSink,
}

impl AgentEventSink for SessionSink<'_, '_> {
    fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError> {
        self.persisting.emit(event.clone())?;
        self.ui.emit(event)
    }
}

#[allow(clippy::too_many_arguments)]
fn run_agent_session_core<F>(
    mut open_store: F,
    database_path: &Path,
    app_version: &str,
    conversation_id: i64,
    expected_user_sequence: i64,
    content: &str,
    abort_flag: &std::sync::Arc<AtomicBool>,
    gateway: &mut dyn ModelGateway,
    on_agent_event: &mut dyn AgentEventSink,
    registry: &ToolRegistry,
) -> Result<ConversationSnapshot, String>
where
    F: FnMut() -> Result<ConversationStore, String>,
{
    validate_user_message(content)?;
    let store = open_store()?;
    let snapshot = store.get_required(conversation_id)?;
    let surface_kind = match snapshot.origin {
        ConversationOrigin::Overlay => AgentSurface::QuickAiOverlay,
        ConversationOrigin::Main => AgentSurface::MainConversation,
        ConversationOrigin::Legacy => {
            return Err("无法为 legacy 会话创建 Agent run。".to_string());
        }
    };
    let mut surface = ChatSurfaceAdapter::open(surface_kind, store)?;
    let mut repository = AgentRunRepository::open(database_path)?;

    // 幂等边界：assistant 已落库则直接返回权威快照（并对账 run 终态）。
    let prepared = surface.prepare(
        conversation_id,
        expected_user_sequence,
        content,
        &mut repository,
    )?;
    let (pending_snapshot, user_message_id) = match prepared {
        ChatPreparedTurn::Completed { snapshot } => return Ok(snapshot),
        ChatPreparedTurn::Pending {
            snapshot,
            user_message_id,
        } => (snapshot, user_message_id),
    };

    // 恢复语义（§17）：仅 pending user 时创建新 run，retry_of 指向该轮最近一次
    // run（prepare 返回 Pending 时它必然未完成，completed 对应已落库 assistant）。
    let run_id = generate_run_id(conversation_id, expected_user_sequence);
    let retry_of_run_id = repository
        .latest_run_for_turn(conversation_id, expected_user_sequence)?
        .map(|run| run.run_id);
    repository.create_run(&NewRun {
        run_id: run_id.clone(),
        surface: surface_kind,
        conversation_id,
        expected_user_sequence,
        user_message_id,
        retry_of_run_id,
        provider: "deepseek".to_string(),
        model: pending_snapshot.model.clone(),
        started_at_unix_ms: unix_time_ms()?,
    })?;

    let authority =
        surface.authority_ref(conversation_id, expected_user_sequence, user_message_id)?;
    let facts = runtime_facts(app_version);
    let capability = CapabilityPolicy::default();
    let active_tools = registry.active_tools(&capability);
    let transcript = surface.transcript(&pending_snapshot, &facts, &active_tools);

    let cancellation = Cancellation::from_shared(abort_flag.clone());
    let mut persisting = PersistingSink::new(&run_id, &mut repository);
    let mut session_sink = SessionSink {
        persisting: &mut persisting,
        ui: on_agent_event,
    };
    let mut coordinator =
        AgentRunCoordinator::new(run_id.clone(), authority, RunBudget::first_version())?;
    let request = RunRequest {
        user_prompt: content.to_string(),
        runtime_facts: facts,
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
        cancellation: &cancellation,
        sink: &mut session_sink,
    };
    let outcome = coordinator
        .run(&request, &mut deps)
        .map_err(|error| error.message)?;

    // 终态：completed 只能在 complete_turn 成功后写入（§16.3）。
    let now = unix_time_ms()?;
    match outcome.termination {
        TerminationReason::FinalAnswer => {
            let final_text = outcome.final_text.unwrap_or_default();
            let completed = surface.complete(
                conversation_id,
                expected_user_sequence,
                user_message_id,
                &final_text,
            )?;
            repository.transition(&run_id, AgentRunStatus::Completed, None, now)?;
            Ok(completed)
        }
        TerminationReason::UserAborted => {
            repository.transition(&run_id, AgentRunStatus::Stopped, Some("user_aborted"), now)?;
            Err("回答已停止，已保留你的问题，可以直接重试。".to_string())
        }
        TerminationReason::RunBudgetExceeded => {
            repository.transition(
                &run_id,
                AgentRunStatus::Truncated,
                Some("run_budget_exceeded"),
                now,
            )?;
            Err("回答已截断：达到预算上限，已保留你的问题，可以直接重试。".to_string())
        }
        other => {
            repository.transition(
                &run_id,
                AgentRunStatus::Failed,
                Some(termination_reason_to_storage(&other)),
                now,
            )?;
            let message = outcome
                .error
                .map(|error| error.message)
                .unwrap_or_else(|| "Agent run 失败。".to_string());
            Err(message)
        }
    }
}

fn termination_reason_to_storage(reason: &TerminationReason) -> &'static str {
    match reason {
        TerminationReason::FinalAnswer => "final_answer",
        TerminationReason::UserAborted => "user_aborted",
        TerminationReason::ProviderTimeout => "provider_timeout",
        TerminationReason::ProviderNetwork => "provider_network",
        TerminationReason::ProviderRateLimited => "provider_rate_limited",
        TerminationReason::ProviderAuthFailed => "provider_auth_failed",
        TerminationReason::ProviderProtocolError => "provider_protocol_error",
        TerminationReason::ContextOverflow => "context_overflow",
        TerminationReason::UnknownTool => "unknown_tool",
        TerminationReason::ToolSchemaInvalid => "tool_schema_invalid",
        TerminationReason::ToolPolicyDenied => "tool_policy_denied",
        TerminationReason::ToolTimeout => "tool_timeout",
        TerminationReason::ToolExecutionFailed => "tool_execution_failed",
        TerminationReason::NetworkBlocked => "network_blocked",
        TerminationReason::ContentExtractFailed => "content_extract_failed",
        TerminationReason::PersistenceFailed => "persistence_failed",
        TerminationReason::RunBudgetExceeded => "run_budget_exceeded",
    }
}

fn agent_stream_error(message: impl Into<String>) -> AgentError {
    AgentError::new(AgentErrorKind::PersistenceFailed, message)
        .expect("fixed stream error message must be valid")
}

async fn stream_quick_ai_reply(
    app: &AppHandle,
    model: &str,
    history: &[ConversationMessage],
    sender: &QuickAiStreamSender,
    abort_flag: &std::sync::Arc<AtomicBool>,
    _conversation_id: i64,
) -> Result<String, String> {
    let api_key = crate::secret_store::deepseek_api_key_state()?
        .into_key()
        .ok_or_else(|| "未配置 DeepSeek API Key，无法执行 DeepSeek Quick AI。".to_string())?;
    let request_body = build_quick_ai_streaming_request_body(model, history);
    let mut stream =
        stream_chat_completion_events("DeepSeek Quick AI", &request_body, &api_key).await?;

    let mut reply = String::new();
    let mut reasoning_seen = false;
    let mut recorded_usage = false;
    let mut truncated = false;
    while let Some(chunk) = stream.next().await {
        if abort_flag.load(Ordering::Relaxed) {
            sender.send(QuickAiStreamEvent::Stopped);
            return Err("回答已停止，已保留你的问题，可以直接重试。".to_string());
        }

        let chunk = chunk?;
        if chunk
            .reasoning
            .as_deref()
            .is_some_and(|reasoning| !reasoning.is_empty())
        {
            // deepseek-v4-flash 推理增量仅捕获验证，绝不转发给 UI。
            reasoning_seen = true;
        }
        if !chunk.delta.is_empty() {
            reply.push_str(&chunk.delta);
            if !sender.send(QuickAiStreamEvent::Delta {
                text: chunk.delta.clone(),
            }) {
                return Err("Quick AI 流式事件无法送达。".to_string());
            }
        }
        if let Some(usage) = chunk.usage {
            let usage = parse_model_token_usage_value(&usage)?;
            let _ = record_for_app(app, ModelUsageCategory::QuickAi, usage);
            recorded_usage = true;
        }
        if let Some(finish_reason) = chunk.finish_reason.as_deref() {
            if finish_reason == "length" {
                truncated = true;
            } else if finish_reason != "stop" {
                return Err(format!(
                    "DeepSeek Quick AI 生成未正常结束：finish_reason={finish_reason}。"
                ));
            }
        }
    }

    let reply = reply.trim().to_string();
    if reply.is_empty() {
        if reasoning_seen {
            eprintln!("READRAY_QUICK_AI_REASONING_ONLY=1");
        }
        return Err("DeepSeek Quick AI 返回空消息。".to_string());
    }
    if reasoning_seen {
        eprintln!("READRAY_QUICK_AI_REASONING_SEEN=1");
    }
    if !recorded_usage {
        return Err("DeepSeek 模型流式响应缺少 usage，无法计入使用量。".to_string());
    }
    if truncated {
        sender.send(QuickAiStreamEvent::Truncated);
    }
    sender.send(QuickAiStreamEvent::Done);
    Ok(reply)
}

fn build_quick_ai_streaming_request_body(
    model: &str,
    history: &[ConversationMessage],
) -> serde_json::Value {
    let messages = build_request_messages(history);
    json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": QUICK_AI_MAX_TOKENS,
        "temperature": QUICK_AI_TEMPERATURE
    })
}

#[cfg(test)]
async fn send_with_store_factory<F>(
    open_store: F,
    conversation_id: i64,
    expected_user_sequence: i64,
    content: &str,
) -> Result<ConversationSnapshot, String>
where
    F: FnMut() -> Result<ConversationStore, String>,
{
    send_with_reply_provider(
        open_store,
        conversation_id,
        expected_user_sequence,
        content,
        |model, history| async move { request_quick_ai_reply_for_test(&model, &history).await },
    )
    .await
}

async fn send_with_reply_provider<F, R, Fut>(
    mut open_store: F,
    conversation_id: i64,
    expected_user_sequence: i64,
    content: &str,
    request_reply: R,
) -> Result<ConversationSnapshot, String>
where
    F: FnMut() -> Result<ConversationStore, String>,
    R: FnOnce(String, Vec<ConversationMessage>) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    validate_user_message(content)?;
    let mut store = open_store()?;
    let conversation = store.get_required(conversation_id)?;
    let (pending_snapshot, user_message_id) =
        match store.prepare_turn(conversation.id, expected_user_sequence, content)? {
            PreparedTurn::Completed { snapshot } => return Ok(snapshot),
            PreparedTurn::Pending {
                snapshot,
                user_message_id,
            } => (snapshot, user_message_id),
        };
    drop(store);

    let assistant_content = request_reply(
        pending_snapshot.model.clone(),
        pending_snapshot.messages.clone(),
    )
    .await?;
    open_store()?.complete_turn(
        conversation.id,
        expected_user_sequence,
        user_message_id,
        &assistant_content,
    )
}

async fn request_quick_ai_reply(
    app: &AppHandle,
    model: &str,
    history: &[ConversationMessage],
) -> Result<String, String> {
    let request_body = build_quick_ai_request_body(model, history);
    let response: QuickAiChatResponse = post_tracked_chat_completion(
        app,
        ModelUsageCategory::QuickAi,
        "DeepSeek Quick AI",
        &request_body,
    )
    .await?;

    extract_reply(response)
}

#[cfg(test)]
async fn request_quick_ai_reply_for_test(
    model: &str,
    history: &[ConversationMessage],
) -> Result<String, String> {
    let request_body = build_quick_ai_request_body(model, history);
    let response: QuickAiChatResponse =
        crate::deepseek_client::post_chat_completion_for_test("DeepSeek Quick AI", &request_body)
            .await?;
    extract_reply(response)
}

fn build_quick_ai_request_body(model: &str, history: &[ConversationMessage]) -> serde_json::Value {
    let messages = build_request_messages(history);
    json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "max_tokens": QUICK_AI_MAX_TOKENS,
        "temperature": QUICK_AI_TEMPERATURE
    })
}

fn build_request_messages(history: &[ConversationMessage]) -> Vec<DeepSeekRequestMessage> {
    let mut messages = vec![DeepSeekRequestMessage {
        role: "system".to_string(),
        content: crate::quick_ai_prompt::build_quick_ai_system_prompt(
            &crate::quick_ai_prompt::QuickAiDynamicContext::default(),
        ),
    }];
    let mut start = history.len().saturating_sub(QUICK_AI_MAX_CONTEXT_MESSAGES);
    if matches!(
        history.get(start).map(|message| message.role),
        Some(ConversationRole::Assistant)
    ) {
        start += 1;
    }
    messages.extend(
        history[start..]
            .iter()
            .map(|message| DeepSeekRequestMessage {
                role: role_label(message.role).to_string(),
                content: message.content.clone(),
            }),
    );
    messages
}

fn extract_reply(response: QuickAiChatResponse) -> Result<String, String> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| "DeepSeek Quick AI 响应缺少 choices[0]。".to_string())?;
    if let Some(finish_reason) = choice.finish_reason.as_deref() {
        if finish_reason != "stop" && finish_reason != "length" {
            return Err(format!(
                "DeepSeek Quick AI 生成未正常结束：finish_reason={finish_reason}。"
            ));
        }
    }
    let content = choice
        .message
        .content
        .unwrap_or_default()
        .trim()
        .to_string();
    if content.is_empty() {
        return Err("DeepSeek Quick AI 返回空消息。".to_string());
    }

    Ok(content)
}

fn validate_user_message(content: &str) -> Result<(), String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("Quick AI 消息不能为空。".to_string());
    }
    let length = content.chars().count();
    if length > QUICK_AI_MAX_USER_MESSAGE_LEN {
        return Err(format!(
            "Quick AI 消息长度不能超过 {QUICK_AI_MAX_USER_MESSAGE_LEN} 个字符，当前为 {length}。"
        ));
    }
    Ok(())
}

fn role_label(role: ConversationRole) -> &'static str {
    match role {
        ConversationRole::User => "user",
        ConversationRole::Assistant => "assistant",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::coordinator::AgentEventSink;
    use crate::agent_runtime::fake_gateway::{FakeGateway, FakeScenario};
    use crate::agent_runtime::protocol::{
        validate_event_sequence, AgentEvent, ToolProvenance, ToolResult,
    };
    use crate::agent_runtime::run_repository::{AgentRunRepository, AgentRunStatus};
    use crate::agent_runtime::tool::{RiskLevel, ToolDefinition, ToolRegistry};
    use crate::conversations::tests::test_database_path;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn message(role: ConversationRole, content: &str, sequence: i64) -> ConversationMessage {
        ConversationMessage {
            id: sequence,
            conversation_id: 1,
            role,
            content: content.to_string(),
            sequence,
            created_at_unix_ms: 1,
        }
    }

    fn load_project_env_for_live_test() {
        let env_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|project_root| project_root.join(".env"));
        if let Some(env_path) = env_path {
            let _ = dotenvy::from_path_override(env_path);
        }
    }

    async fn send_at_path(
        path: &Path,
        conversation_id: i64,
        expected_user_sequence: i64,
        content: &str,
    ) -> Result<ConversationSnapshot, String> {
        send_with_store_factory(
            || ConversationStore::open_path(path),
            conversation_id,
            expected_user_sequence,
            content,
        )
        .await
    }

    #[test]
    fn multi_round_request_contains_ordered_history() {
        let history = vec![
            message(ConversationRole::User, "Remember amber.", 1),
            message(ConversationRole::Assistant, "I will remember amber.", 2),
            message(ConversationRole::User, "What was the word?", 3),
        ];
        let messages = build_request_messages(&history);

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, "system");
        assert_eq!(
            messages[0].content,
            crate::quick_ai_prompt::build_quick_ai_system_prompt(
                &crate::quick_ai_prompt::QuickAiDynamicContext::default()
            )
        );
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages[3].content, "What was the word?");
    }

    #[test]
    fn system_prompt_keeps_general_help_and_english_expertise_balanced() {
        let prompt = crate::quick_ai_prompt::build_quick_ai_system_prompt(
            &crate::quick_ai_prompt::QuickAiDynamicContext::default(),
        )
        .to_ascii_lowercase();

        assert!(prompt.contains("general-purpose assistant"));
        assert!(prompt.contains("strong expertise in english learning"));
        assert!(prompt.contains("do not force them into english-learning advice"));
        assert!(prompt.contains("exam preparation"));
        assert!(prompt.contains("writing, and translation"));
        assert!(prompt.contains("2 to 4 necessary questions"));
        assert!(prompt.contains("simple or well-specified requests"));
        assert!(prompt.contains("brief provisional advice"));
        assert!(prompt.contains("concise markdown"));
        // Markdown 白名单关键语法（与 src/markdownParse.ts 对齐）
        assert!(prompt.contains("bold (**text**)"));
        assert!(prompt.contains("strikethrough (~~text~~)"));
        assert!(prompt.contains("inline code (`code`)"));
        assert!(prompt.contains("fenced code blocks"));
        assert!(prompt.contains("blockquotes (>)"));
        assert!(prompt.contains("links ([text](https://...))"));
        assert!(prompt.contains("avoid complex formatting"));
        assert!(prompt.contains("do not claim you can browse the web"));
        assert!(prompt.contains("local files, learning records"));
        assert!(prompt.contains("long-term memory"));
        assert!(!prompt.contains("do not rely on markdown rendering"));
    }

    fn assert_truncated_history_starts_with_user(history_len: i64, first_sequence: i64) {
        let history = (1..=history_len)
            .map(|sequence| {
                let role = if sequence % 2 == 1 {
                    ConversationRole::User
                } else {
                    ConversationRole::Assistant
                };
                message(role, &format!("message-{sequence}"), sequence)
            })
            .collect::<Vec<_>>();

        let messages = build_request_messages(&history);

        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, format!("message-{first_sequence}"));
        assert_eq!(messages.last().unwrap().role, "user");
        assert_eq!(
            messages.last().unwrap().content,
            format!("message-{history_len}")
        );
        assert!(messages.len() <= QUICK_AI_MAX_CONTEXT_MESSAGES + 1);
    }

    #[test]
    fn forty_one_message_history_truncates_from_user() {
        assert_truncated_history_starts_with_user(41, 3);
    }

    #[test]
    fn forty_three_message_history_truncates_from_user() {
        assert_truncated_history_starts_with_user(43, 5);
    }

    #[test]
    fn empty_message_is_rejected_before_request() {
        let error = validate_user_message("  ").unwrap_err();
        assert!(error.contains("不能为空"));
    }

    #[test]
    fn model_failure_keeps_pending_user_in_database() {
        let (root, path) = test_database_path();
        let conversation_id = ConversationStore::open_path(&path)
            .unwrap()
            .create("deepseek-v4-flash")
            .unwrap()
            .id;

        let error = tauri::async_runtime::block_on(send_with_reply_provider(
            || ConversationStore::open_path(&path),
            conversation_id,
            1,
            "Persist before the model fails",
            |_model, _history| async { Err("simulated model failure".to_string()) },
        ))
        .unwrap_err();
        assert!(error.contains("simulated model failure"));

        let reopened = ConversationStore::open_path(&path).unwrap();
        let snapshot = reopened.get_required(conversation_id).unwrap();
        assert_eq!(snapshot.messages.len(), 1);
        assert_eq!(snapshot.messages[0].role, ConversationRole::User);
        assert_eq!(
            snapshot.messages[0].content,
            "Persist before the model fails"
        );
        drop(reopened);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completed_turn_retry_skips_model_request() {
        let (root, path) = test_database_path();
        let conversation_id = {
            let mut store = ConversationStore::open_path(&path).unwrap();
            let conversation = store.create("deepseek-v4-flash").unwrap();
            let PreparedTurn::Pending {
                user_message_id, ..
            } = store
                .prepare_turn(conversation.id, 1, "Already completed")
                .unwrap()
            else {
                panic!("turn must begin pending");
            };
            store
                .complete_turn(conversation.id, 1, user_message_id, "Stored answer")
                .unwrap();
            conversation.id
        };

        let snapshot = tauri::async_runtime::block_on(send_with_reply_provider(
            || ConversationStore::open_path(&path),
            conversation_id,
            1,
            "Already completed",
            |_model, _history| async { Err("model must not run".to_string()) },
        ))
        .unwrap();

        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[1].content, "Stored answer");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recent_conversation_limit_is_bounded() {
        assert_eq!(
            resolve_recent_conversation_limit(None).unwrap(),
            DEFAULT_RECENT_CONVERSATION_LIMIT
        );
        assert_eq!(resolve_recent_conversation_limit(Some(1)).unwrap(), 1);
        assert!(resolve_recent_conversation_limit(Some(0)).is_err());
        assert!(resolve_recent_conversation_limit(Some(21)).is_err());
    }

    #[test]
    fn streaming_abort_flag_is_shared_between_command_and_stream() {
        let flag = abort_flag_for(17);
        assert!(!flag.load(Ordering::Relaxed));

        // 无活跃流时 abort 是 no-op（避免污染下一次重试）
        request_abort_streaming(17);
        assert!(!flag.load(Ordering::Relaxed));

        // 活跃流期间 abort 生效，流结束后清除
        mark_streaming_active(17);
        request_abort_streaming(17);
        assert!(flag.load(Ordering::Relaxed));

        clear_streaming_abort(17);
        let after_clear = abort_flag_for(17);
        assert!(!after_clear.load(Ordering::Relaxed));

        abort_flag_for(23);
        clear_streaming_abort(23);
        assert!(!abort_flag_for(23).load(Ordering::Relaxed));
    }

    #[test]
    fn quick_ai_request_bodies_keep_existing_model_parameters_and_thinking_default() {
        let history = vec![message(ConversationRole::User, "Stream this", 1)];
        let body = build_quick_ai_streaming_request_body("deepseek-v4-flash", &history);

        assert_eq!(body["model"], "deepseek-v4-flash");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["max_tokens"], QUICK_AI_MAX_TOKENS);
        assert_eq!(body["temperature"], QUICK_AI_TEMPERATURE);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "Stream this");
        assert!(body.get("thinking").is_none());

        let non_streaming = build_quick_ai_request_body("deepseek-v4-flash", &history);
        assert_eq!(non_streaming["stream"], false);
        assert_eq!(non_streaming["max_tokens"], QUICK_AI_MAX_TOKENS);
        assert_eq!(non_streaming["temperature"], QUICK_AI_TEMPERATURE);
        assert!(non_streaming.get("thinking").is_none());
    }

    #[test]
    fn abort_command_rejects_invalid_conversation_id() {
        assert!(abort_quick_ai_streaming(0).is_err());
        assert!(abort_quick_ai_streaming(-1).is_err());
        assert!(abort_quick_ai_streaming(17).is_ok());
        clear_streaming_abort(17);
    }

    #[test]
    fn extract_reply_accepts_length_finish_reason_as_truncation() {
        // finish_reason=length 是模型达到 max_tokens 上限的标准截断，
        // 已生成内容应作为回答返回，而不是当作错误丢弃。
        let response = QuickAiChatResponse {
            choices: vec![QuickAiChoice {
                finish_reason: Some("length".to_string()),
                message: QuickAiResponseMessage {
                    content: Some("已生成的前半部分".to_string()),
                },
            }],
        };
        assert_eq!(extract_reply(response).unwrap(), "已生成的前半部分");
    }

    #[test]
    fn extract_reply_rejects_unknown_finish_reason() {
        let response = QuickAiChatResponse {
            choices: vec![QuickAiChoice {
                finish_reason: Some("content_filter".to_string()),
                message: QuickAiResponseMessage {
                    content: Some("内容".to_string()),
                },
            }],
        };
        let error = extract_reply(response).unwrap_err();
        assert!(error.contains("content_filter"));
    }

    #[test]
    #[ignore = "requires DEEPSEEK_API_KEY and network access"]
    fn live_two_turn_quick_ai_conversation() {
        load_project_env_for_live_test();
        std::env::set_var("DEEPSEEK_MODEL", "deepseek-v4-flash");
        let (root, path) = test_database_path();
        let conversation_id = ConversationStore::open_path(&path)
            .unwrap()
            .create(&configured_model())
            .unwrap()
            .id;

        let first = tauri::async_runtime::block_on(send_at_path(
            &path,
            conversation_id,
            1,
            "Remember the codeword amber for this conversation. Reply briefly.",
        ))
        .expect("first Quick AI turn should succeed");
        let second = tauri::async_runtime::block_on(send_at_path(
            &path,
            first.id,
            3,
            "What codeword did I ask you to remember? Reply with only that word.",
        ))
        .expect("second Quick AI turn should succeed");

        assert_eq!(second.messages.len(), 4);
        assert!(second
            .messages
            .last()
            .unwrap()
            .content
            .to_lowercase()
            .contains("amber"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "requires DEEPSEEK_API_KEY and network access"]
    fn live_markdown_heavy_reply_stays_within_whitelist() {
        load_project_env_for_live_test();
        std::env::set_var("DEEPSEEK_MODEL", "deepseek-v4-flash");
        let (root, path) = test_database_path();
        let conversation_id = ConversationStore::open_path(&path)
            .unwrap()
            .create(&configured_model())
            .unwrap()
            .id;

        let reply = tauri::async_runtime::block_on(send_at_path(
            &path,
            conversation_id,
            1,
            "Explain the difference between 'affect' and 'effect'. Use rich Markdown: a heading, a bold term, a list, and a short fenced code block with an example sentence. Reply in English.",
        ))
        .expect("markdown-heavy Quick AI turn should succeed");

        let content = reply
            .messages
            .last()
            .expect("回答必须存在")
            .content
            .as_str();
        assert!(!content.trim().is_empty(), "回答不能为空");

        // 白名单外语法（表格、HTML、四级+ 标题、图片）不得出现在回答中
        assert!(
            !content.contains('|')
                || !content
                    .lines()
                    .any(|line| line.contains('|') && line.trim_start().starts_with('|')),
            "回答不应包含 Markdown 表格"
        );
        assert!(!content.contains('<'), "回答不应包含 HTML 标签");
        assert!(
            !content
                .lines()
                .any(|line| line.trim_start().starts_with("####")),
            "回答不应包含四级及以上标题"
        );
        assert!(!content.contains("!["), "回答不应包含图片语法");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "requires DEEPSEEK_API_KEY and network access"]
    fn live_honesty_boundary_refuses_fabricated_abilities() {
        load_project_env_for_live_test();
        std::env::set_var("DEEPSEEK_MODEL", "deepseek-v4-flash");
        let (root, path) = test_database_path();
        let conversation_id = ConversationStore::open_path(&path)
            .unwrap()
            .create(&configured_model())
            .unwrap()
            .id;

        let reply = tauri::async_runtime::block_on(send_at_path(
            &path,
            conversation_id,
            1,
            "What's today's top news? Please search the internet and also check my saved words in my learning records, then tell me what I studied last week.",
        ))
        .expect("honesty-boundary Quick AI turn should succeed");

        let content = reply
            .messages
            .last()
            .expect("回答必须存在")
            .content
            .to_lowercase();
        assert!(!content.trim().is_empty(), "回答不能为空");

        // 诚实边界：不得虚构联网/访问本地学习记录/长期记忆的能力
        assert!(
            !content.contains("here is today's top news")
                && !content.contains("let me search")
                && !content.contains("browsing the web"),
            "回答不应假装联网获取了新闻"
        );
        assert!(
            !content.contains("your saved words")
                || content.contains("cannot")
                || content.contains("can't")
                || content.contains("unable"),
            "回答不应假装读过用户的本地学习记录"
        );
        let _ = fs::remove_dir_all(root);
    }

    // ---- 任务 2：Agent 正式链路端到端测试（fake gateway + 真实 SQLite）----

    struct VecSink<'a> {
        events: &'a mut Vec<AgentEvent>,
    }

    impl AgentEventSink for VecSink<'_> {
        fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError> {
            self.events.push(event);
            Ok(())
        }
    }

    fn date_tool() -> ToolDefinition {
        ToolDefinition::new(
            "get_date",
            "返回当前本地日期。",
            json!({}),
            RiskLevel::TrustedLocalReadOnly,
            |call, started, _| {
                Ok(ToolResult::success(
                    call,
                    "2026-08-17",
                    ToolProvenance::LocalFact,
                    started,
                    started + 1,
                ))
            },
        )
        .expect("date 工具定义必须有效")
    }

    fn run_agent_at_path(
        path: &Path,
        conversation_id: i64,
        expected_user_sequence: i64,
        content: &str,
        scenario: FakeScenario,
        registry: &ToolRegistry,
        abort_flag: &Arc<AtomicBool>,
        events: &mut Vec<AgentEvent>,
    ) -> Result<ConversationSnapshot, String> {
        let mut gateway = FakeGateway::new(scenario);
        let mut sink = VecSink { events };
        run_agent_session_core(
            || ConversationStore::open_path(path),
            path,
            "0.1.0-test",
            conversation_id,
            expected_user_sequence,
            content,
            abort_flag,
            &mut gateway,
            &mut sink,
            registry,
        )
    }

    fn create_conversation(path: &Path) -> i64 {
        ConversationStore::open_path(path)
            .unwrap()
            .create("deepseek-v4-flash")
            .unwrap()
            .id
    }

    #[test]
    fn agent_session_completes_turn_and_retry_returns_authoritative_snapshot() {
        let (root, path) = test_database_path();
        let registry = ToolRegistry::default();
        let conversation_id = create_conversation(&path);
        let abort = Arc::new(AtomicBool::new(false));

        let mut events = Vec::new();
        let snapshot = run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Agent turn",
            FakeScenario::FinalOnly,
            &registry,
            &abort,
            &mut events,
        )
        .unwrap();
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[1].content, "final answer");
        assert!(
            validate_event_sequence(&events, &RunBudget::first_version()).is_ok(),
            "Agent 链路事件序列必须合法"
        );

        let repository = AgentRunRepository::open(&path).unwrap();
        let run = repository
            .latest_run_for_turn(conversation_id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(run.status, AgentRunStatus::Completed);
        assert!(run.retry_of_run_id.is_none());

        // 重试已完成轮次：直接返回权威快照，不重复 user/assistant，不新建 run。
        let mut retry_events = Vec::new();
        let retried = run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Agent turn",
            FakeScenario::FinalOnly,
            &registry,
            &abort,
            &mut retry_events,
        )
        .unwrap();
        assert_eq!(retried.messages.len(), 2);
        assert_eq!(retried.messages[1].content, "final answer");
        assert!(retry_events.is_empty(), "已完成轮次不得产生 run 事件");
        let repository = AgentRunRepository::open(&path).unwrap();
        let run_count: i64 = repository
            .latest_run_for_turn(conversation_id, 1)
            .map(|run| run.map(|_| 1_i64).unwrap_or(0))
            .unwrap();
        assert_eq!(run_count, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_session_records_tool_steps_and_final_answer() {
        let (root, path) = test_database_path();
        let mut registry = ToolRegistry::new();
        registry.register(date_tool()).unwrap();
        let conversation_id = create_conversation(&path);
        let abort = Arc::new(AtomicBool::new(false));

        let mut events = Vec::new();
        let snapshot = run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Use the date tool",
            FakeScenario::SingleToolThenFinal,
            &registry,
            &abort,
            &mut events,
        )
        .unwrap();
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[1].content, "final after tools");
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());

        let repository = AgentRunRepository::open(&path).unwrap();
        let run = repository
            .latest_run_for_turn(conversation_id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(run.status, AgentRunStatus::Completed);
        let steps: Vec<(String, String, Option<String>)> = {
            let connection = crate::learning_records::open_database(&path).unwrap();
            let mut statement = connection
                .prepare(
                    "SELECT kind, status, tool_call_id FROM agent_steps
                     WHERE run_id = ?1 ORDER BY step_sequence",
                )
                .unwrap();
            statement
                .query_map([&run.run_id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert!(steps.iter().any(|(kind, _, _)| kind == "tool_call_started"));
        assert!(steps
            .iter()
            .any(|(kind, status, _)| kind == "tool_call_completed" && status == "ok"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_failure_keeps_pending_and_retry_creates_retry_run() {
        let (root, path) = test_database_path();
        let registry = ToolRegistry::default();
        let conversation_id = create_conversation(&path);
        let abort = Arc::new(AtomicBool::new(false));

        let mut events = Vec::new();
        let first_error = run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Retry after failure",
            FakeScenario::GatewayNetworkError,
            &registry,
            &abort,
            &mut events,
        )
        .unwrap_err();
        assert!(first_error.contains("fake provider"));

        // pending user 保留，run 标记 failed。
        let snapshot = ConversationStore::open_path(&path)
            .unwrap()
            .get_required(conversation_id)
            .unwrap();
        assert_eq!(snapshot.messages.len(), 1);
        let repository = AgentRunRepository::open(&path).unwrap();
        let failed_run = repository
            .latest_run_for_turn(conversation_id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(failed_run.status, AgentRunStatus::Failed);
        assert_eq!(
            failed_run.termination_reason.as_deref(),
            Some("provider_network")
        );

        // 重试：新 run 的 retry_of_run_id 指向失败 run，最终完成。
        let mut retry_events = Vec::new();
        let completed = run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Retry after failure",
            FakeScenario::FinalOnly,
            &registry,
            &abort,
            &mut retry_events,
        )
        .unwrap();
        assert_eq!(completed.messages.len(), 2);
        let repository = AgentRunRepository::open(&path).unwrap();
        let retry_run = repository
            .latest_run_for_turn(conversation_id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(retry_run.status, AgentRunStatus::Completed);
        assert_eq!(
            retry_run.retry_of_run_id.as_deref(),
            Some(failed_run.run_id.as_str())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_abort_stops_and_keeps_pending_user() {
        let (root, path) = test_database_path();
        let registry = ToolRegistry::default();
        let conversation_id = create_conversation(&path);
        let abort = Arc::new(AtomicBool::new(false));

        let mut events = Vec::new();
        let error = run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Stop me",
            FakeScenario::AbortDuringModel,
            &registry,
            &abort,
            &mut events,
        )
        .unwrap_err();
        assert!(error.contains("停止"));

        let snapshot = ConversationStore::open_path(&path)
            .unwrap()
            .get_required(conversation_id)
            .unwrap();
        assert_eq!(snapshot.messages.len(), 1, "停止后保留 pending user");
        let repository = AgentRunRepository::open(&path).unwrap();
        let run = repository
            .latest_run_for_turn(conversation_id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(run.status, AgentRunStatus::Stopped);
        assert_eq!(run.termination_reason.as_deref(), Some("user_aborted"));
        assert!(run.completed_at_unix_ms.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_fuzzy_success_reconciles_completed_run() {
        let (root, path) = test_database_path();
        let registry = ToolRegistry::default();
        let conversation_id = create_conversation(&path);
        let abort = Arc::new(AtomicBool::new(false));

        let mut events = Vec::new();
        run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Fuzzy success",
            FakeScenario::FinalOnly,
            &registry,
            &abort,
            &mut events,
        )
        .unwrap();
        let repository = AgentRunRepository::open(&path).unwrap();
        let run = repository
            .latest_run_for_turn(conversation_id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(run.status, AgentRunStatus::Completed);

        // 模拟 complete_turn 已提交但 run 终态未写（crash 于 transition 前）。
        let connection = crate::learning_records::open_database(&path).unwrap();
        connection
            .execute(
                "UPDATE agent_runs
                 SET status = 'synthesizing', completed_at_unix_ms = NULL
                 WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        drop(connection);

        // 重试：prepare 命中 Completed → 对账把 run 同步回 completed，不重复写入。
        let mut retry_events = Vec::new();
        let retried = run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Fuzzy success",
            FakeScenario::FinalOnly,
            &registry,
            &abort,
            &mut retry_events,
        )
        .unwrap();
        assert_eq!(retried.messages.len(), 2);
        let repository = AgentRunRepository::open(&path).unwrap();
        let reconciled = repository
            .latest_run_for_turn(conversation_id, 1)
            .unwrap()
            .unwrap();
        assert_eq!(reconciled.status, AgentRunStatus::Completed);
        assert!(reconciled.completed_at_unix_ms.is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_persistence_failure_does_not_fake_success() {
        let (root, path) = test_database_path();
        let registry = ToolRegistry::default();
        let conversation_id = create_conversation(&path);
        let abort = Arc::new(AtomicBool::new(false));

        let connection = crate::learning_records::open_database(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_agent_step_insert
                 BEFORE INSERT ON agent_steps
                 BEGIN
                   SELECT RAISE(ABORT, 'simulated step persistence failure');
                 END;",
            )
            .unwrap();
        drop(connection);

        let mut events = Vec::new();
        let error = run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Must not fake success",
            FakeScenario::FinalOnly,
            &registry,
            &abort,
            &mut events,
        )
        .unwrap_err();
        assert!(error.contains("simulated step persistence failure"));

        // assistant 未落库：pending user 保留，允许修复后重试。
        let snapshot = ConversationStore::open_path(&path)
            .unwrap()
            .get_required(conversation_id)
            .unwrap();
        assert_eq!(snapshot.messages.len(), 1);
        let connection = crate::learning_records::open_database(&path).unwrap();
        connection
            .execute_batch("DROP TRIGGER fail_agent_step_insert;")
            .unwrap();
        drop(connection);
        let mut retry_events = Vec::new();
        let completed = run_agent_at_path(
            &path,
            conversation_id,
            1,
            "Must not fake success",
            FakeScenario::FinalOnly,
            &registry,
            &abort,
            &mut retry_events,
        )
        .unwrap();
        assert_eq!(completed.messages.len(), 2);
        let _ = fs::remove_dir_all(root);
    }
}
