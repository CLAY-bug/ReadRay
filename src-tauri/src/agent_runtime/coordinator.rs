//! AgentRunCoordinator：无持久化副作用的最小 Agent 循环。
//!
//! 本模块是任务 1 的循环内核入口。它只通过 `AgentEventSink` 发布事件，不写
//! SQLite、不注册 Tauri command、不接入正式会话/写作。工具、上下文投影和
//! provider 差异都通过依赖注入进入，因此可以用 fake ModelGateway 完全离线验证。
//! 状态机与事件序列约束由 `protocol::validate_event_sequence` 保证。
//!
//! 工具失败语义（方案 §8.2/§8.3/§20）：运行期工具失败（ToolExecutionFailed /
//! ToolTimeout）可恢复——失败结果按原调用顺序回传模型，由模型决定降级，预算与
//! run 超时是硬兜底；授权/schema 失败（未知工具、策略拒绝、schema 无效、
//! call.validate 失败）保持 fail-fast，直接 RunFailed 终止。

use crate::agent_runtime::context::{ContextAssembler, RuntimeFacts};
use crate::agent_runtime::gateway::{ModelGateway, ModelRequest};
use crate::agent_runtime::protocol::{
    AgentError, AgentErrorKind, AgentEvent, AgentEventPayload, AuthorityRef, ModelEvent,
    ModelUsage, RunBudget, TerminationReason, ToolCall, ToolResult,
};
use crate::agent_runtime::tool::{CapabilityPolicy, ToolDefinition, ToolPolicy, ToolRegistry};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// 用户请求停止的共享信号。Coordinator 在模型流式输出与工具批次边界检查它。
#[derive(Clone, Debug, Default)]
pub(crate) struct Cancellation {
    flag: Arc<AtomicBool>,
}

impl Cancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_requested(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

pub(crate) trait TimeSource {
    fn now_unix_ms(&self) -> u64;
}

/// 生产环境的系统时钟。
pub(crate) struct SystemTimeSource;

impl TimeSource for SystemTimeSource {
    fn now_unix_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

pub(crate) trait AgentEventSink {
    fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError>;
}

/// 防御性 run 身份守卫：拒绝携带其他 run_id 的迟到事件，保证“页面卸载、切换会话
/// 或请求身份变化后，迟到事件不得更新当前页面”的内核侧约定。接线留给任务 2 的
/// surface adapter；本任务由测试直接构造。
pub(crate) struct RunScopedSink<'a> {
    run_id: String,
    inner: &'a mut dyn AgentEventSink,
}

impl<'a> RunScopedSink<'a> {
    pub fn new(run_id: impl Into<String>, inner: &'a mut dyn AgentEventSink) -> Self {
        Self {
            run_id: run_id.into(),
            inner,
        }
    }
}

impl AgentEventSink for RunScopedSink<'_> {
    fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError> {
        if event.run_id != self.run_id {
            return Err(agent_error(
                AgentErrorKind::ProviderProtocolError,
                format!(
                    "迟到事件 run_id {} 不属于当前 run {}。",
                    event.run_id, self.run_id
                ),
            ));
        }
        self.inner.emit(event)
    }
}

/// 第一版同步内核的工具完成顺序提示。真正的并行 executor 可以忽略它对完成顺序
/// 的编排，但无论完成顺序如何，回传给模型的结果始终按原始调用顺序组装。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolExecutionOrder {
    CallOrder,
    ReverseCallOrder,
}

impl Default for ToolExecutionOrder {
    fn default() -> Self {
        Self::CallOrder
    }
}

pub(crate) struct RunRequest {
    pub user_prompt: String,
    pub runtime_facts: RuntimeFacts,
    pub capability: CapabilityPolicy,
    pub tool_execution_order: ToolExecutionOrder,
}

/// run 的最终摘要。事件本身由 `AgentEventSink` 承载，不在这里重复。
pub(crate) struct RunOutcome {
    pub final_text: Option<String>,
    pub termination: TerminationReason,
    pub error: Option<AgentError>,
    pub model_turns: u32,
    pub tool_calls: u32,
    pub usage: Option<ModelUsage>,
}

pub(crate) struct AgentDeps<'a> {
    pub gateway: &'a mut dyn ModelGateway,
    pub registry: &'a ToolRegistry,
    pub policy: &'a ToolPolicy,
    pub assembler: &'a ContextAssembler,
    pub time: &'a dyn TimeSource,
    pub cancellation: &'a Cancellation,
    pub sink: &'a mut dyn AgentEventSink,
}

pub(crate) struct AgentRunCoordinator {
    run_id: String,
    authority: AuthorityRef,
    budget: RunBudget,
}

impl AgentRunCoordinator {
    pub fn new(
        run_id: impl Into<String>,
        authority: AuthorityRef,
        budget: RunBudget,
    ) -> Result<Self, String> {
        let run_id = run_id.into();
        if run_id.trim().is_empty() {
            return Err("run_id 不能为空。".to_string());
        }
        authority.validate()?;
        budget.validate()?;
        Ok(Self {
            run_id,
            authority,
            budget,
        })
    }

    /// 驱动一次 Agent run 到明确终止。
    ///
    /// 契约：所有声明性失败（provider 错误、未知工具、schema/策略拒绝、参数
    /// 增量解析失败、工具执行失败、取消、预算/超时）都以 `Ok(outcome)` 加唯一
    /// 终态 AgentEvent 收尾，事件序列满足 `protocol::validate_event_sequence`；
    /// executor 返回不属于本次调用的结果或结果未通过协议级校验时，同样以
    /// `RunFailed` 终态收尾并返回 `Ok(outcome)`；只有 sink 拒绝事件才返回 `Err`。
    pub fn run(
        &mut self,
        request: &RunRequest,
        deps: &mut AgentDeps,
    ) -> Result<RunOutcome, AgentError> {
        let AgentDeps {
            gateway,
            registry,
            policy,
            assembler,
            time,
            cancellation,
            sink,
        } = deps;

        let run_id = self.run_id.clone();
        let authority = self.authority.clone();
        let budget = self.budget;
        let deadline = time.now_unix_ms().saturating_add(budget.run_timeout_ms);

        let mut outcome = RunOutcome {
            final_text: None,
            termination: TerminationReason::ProviderProtocolError,
            error: None,
            model_turns: 0,
            tool_calls: 0,
            usage: None,
        };
        let mut step_sequence = 0_u64;
        macro_rules! emit {
            ($turn_id:expr, $payload:expr $(,)?) => {{
                step_sequence += 1;
                let event = AgentEvent::new(run_id.clone(), $turn_id, step_sequence, $payload)
                    .expect("coordinator 生成的事件必须通过 envelope 校验");
                sink.emit(event)?;
            }};
        }

        emit!(
            None,
            AgentEventPayload::RunStarted {
                surface: authority.surface,
                authority: authority.clone(),
            }
        );

        let active_tools = registry.active_tools(&request.capability);
        // 边界：max_context_bytes 未在本轮强制（上下文预算与 compaction 属后续任务）。
        let mut transcript =
            assembler.initial_messages(&request.user_prompt, &request.runtime_facts, &active_tools);

        loop {
            // ---- 终止条件：取消、run 超时、模型轮数预算 ----
            if cancellation.is_requested() {
                outcome.termination = TerminationReason::UserAborted;
                outcome.error = Some(agent_error(AgentErrorKind::UserAborted, "用户请求停止。"));
                emit!(
                    None,
                    AgentEventPayload::RunStopped {
                        reason: TerminationReason::UserAborted,
                    }
                );
                return Ok(outcome);
            }
            if time.now_unix_ms() >= deadline {
                outcome.termination = TerminationReason::RunBudgetExceeded;
                outcome.error = Some(agent_error(
                    AgentErrorKind::RunBudgetExceeded,
                    "run 总超时已到。",
                ));
                emit!(
                    None,
                    AgentEventPayload::RunTruncated {
                        reason: TerminationReason::RunBudgetExceeded,
                    }
                );
                return Ok(outcome);
            }
            if outcome.model_turns >= budget.max_model_turns {
                outcome.termination = TerminationReason::RunBudgetExceeded;
                outcome.error = Some(agent_error(
                    AgentErrorKind::RunBudgetExceeded,
                    "模型轮数达到预算上限。",
                ));
                emit!(
                    None,
                    AgentEventPayload::RunTruncated {
                        reason: TerminationReason::RunBudgetExceeded,
                    }
                );
                return Ok(outcome);
            }

            outcome.model_turns += 1;
            let turn_id = outcome.model_turns;
            emit!(
                Some(turn_id),
                AgentEventPayload::TurnStarted {
                    turn_index: turn_id,
                }
            );

            // ---- 请求模型并流式接收事件 ----
            let mut text = String::new();
            let mut calls: Vec<ToolCall> = Vec::new();
            let mut pending_arguments: BTreeMap<String, String> = BTreeMap::new();
            let mut usage: Option<ModelUsage> = None;

            let model_request = ModelRequest {
                messages: transcript.clone(),
                tools: active_tools.clone(),
                budget,
                deadline_unix_ms: deadline,
                cancellation: cancellation.clone(),
            };
            let stream_result = gateway.stream_model(model_request, &mut |event| {
                match event {
                    ModelEvent::TextDelta { text: delta } => {
                        text.push_str(&delta);
                        emit!(
                            Some(turn_id),
                            AgentEventPayload::AssistantTextDelta { text: delta },
                        );
                        Ok(())
                    }
                    ModelEvent::ReasoningDelta { .. } => {
                        // 私有推理只留在 provider continuation 内存边界，不投影为 AgentEvent。
                        Ok(())
                    }
                    ModelEvent::ToolCall { call } => {
                        calls.push(call);
                        Ok(())
                    }
                    ModelEvent::ToolCallArgumentsDelta {
                        tool_call_id,
                        delta,
                    } => {
                        pending_arguments
                            .entry(tool_call_id)
                            .or_default()
                            .push_str(&delta);
                        Ok(())
                    }
                    ModelEvent::Usage { usage: next } => {
                        usage = Some(next);
                        Ok(())
                    }
                    ModelEvent::SourceMetadata { source } => {
                        emit!(
                            Some(turn_id),
                            AgentEventPayload::SourcesUpdated {
                                sources: vec![source],
                            },
                        );
                        Ok(())
                    }
                    ModelEvent::Completed { .. } => Ok(()),
                }
            });

            let turn = match stream_result {
                Ok(turn) => turn,
                Err(provider_error) => {
                    // 边界：瞬态 provider 错误的重试与退避（is_retryable_without_side_effect
                    // 与 max_transient_retries）留待真实 provider 任务实施，本任务不重试。
                    outcome.termination = provider_error.termination_reason();
                    outcome.error = Some(provider_error.clone());
                    emit!(
                        Some(turn_id),
                        AgentEventPayload::RunFailed {
                            error: provider_error,
                        }
                    );
                    return Ok(outcome);
                }
            };

            if turn.aborted || cancellation.is_requested() {
                outcome.termination = TerminationReason::UserAborted;
                outcome.error = Some(agent_error(
                    AgentErrorKind::UserAborted,
                    "用户在模型流式输出期间请求停止。",
                ));
                emit!(
                    Some(turn_id),
                    AgentEventPayload::RunStopped {
                        reason: TerminationReason::UserAborted,
                    }
                );
                return Ok(outcome);
            }

            // 合并 provider 可能增量下发的 tool call 参数。
            if !pending_arguments.is_empty() {
                for call in &mut calls {
                    let Some(raw) = pending_arguments.get(&call.id) else {
                        continue;
                    };
                    let parsed: Value = match serde_json::from_str(raw) {
                        Ok(parsed) => parsed,
                        Err(_) => {
                            let failure = agent_error(
                                AgentErrorKind::ProviderProtocolError,
                                "tool call 参数增量无法解析为 JSON。",
                            );
                            outcome.termination = failure.termination_reason();
                            outcome.error = Some(failure.clone());
                            emit!(
                                Some(turn_id),
                                AgentEventPayload::RunFailed { error: failure },
                            );
                            return Ok(outcome);
                        }
                    };
                    if !parsed.is_object() {
                        let failure = agent_error(
                            AgentErrorKind::ProviderProtocolError,
                            "tool call 参数增量不是 JSON object。",
                        );
                        outcome.termination = failure.termination_reason();
                        outcome.error = Some(failure.clone());
                        emit!(
                            Some(turn_id),
                            AgentEventPayload::RunFailed { error: failure },
                        );
                        return Ok(outcome);
                    }
                    call.arguments = parsed;
                }
            }

            if calls.is_empty() {
                if text.trim().is_empty() {
                    let failure = agent_error(
                        AgentErrorKind::ProviderProtocolError,
                        "provider 既没有返回文本也没有返回工具调用。",
                    );
                    outcome.termination = failure.termination_reason();
                    outcome.error = Some(failure.clone());
                    emit!(
                        Some(turn_id),
                        AgentEventPayload::RunFailed { error: failure }
                    );
                    return Ok(outcome);
                }
                emit!(
                    Some(turn_id),
                    AgentEventPayload::AssistantTextCompleted { text: text.clone() },
                );
                outcome.final_text = Some(text.clone());
                outcome.termination = TerminationReason::FinalAnswer;
                outcome.usage = usage.clone();
                emit!(
                    Some(turn_id),
                    AgentEventPayload::RunCompleted { text, usage },
                );
                return Ok(outcome);
            }

            // ---- 工具循环：先预检，再按完成顺序执行，最后按调用顺序回传 ----
            let mut executable: Vec<(&ToolCall, &ToolDefinition)> = Vec::new();
            for call in &calls {
                if let Err(validation) = call.validate() {
                    if outcome.tool_calls >= budget.max_tool_calls {
                        outcome.termination = TerminationReason::RunBudgetExceeded;
                        outcome.error = Some(agent_error(
                            AgentErrorKind::RunBudgetExceeded,
                            "工具调用数达到预算上限。",
                        ));
                        emit!(
                            Some(turn_id),
                            AgentEventPayload::RunTruncated {
                                reason: TerminationReason::RunBudgetExceeded,
                            }
                        );
                        return Ok(outcome);
                    }
                    outcome.tool_calls += 1;
                    let failure = agent_error(AgentErrorKind::ToolSchemaInvalid, validation);
                    outcome.termination = failure.termination_reason();
                    outcome.error = Some(failure.clone());
                    let now = time.now_unix_ms();
                    let result = ToolResult::failure(call, failure.clone(), now, now);
                    emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                    emit!(
                        Some(turn_id),
                        AgentEventPayload::RunFailed { error: failure },
                    );
                    return Ok(outcome);
                }
                match policy.authorize(call, *registry, &request.capability) {
                    Err(failure) => {
                        if outcome.tool_calls >= budget.max_tool_calls {
                            outcome.termination = TerminationReason::RunBudgetExceeded;
                            outcome.error = Some(agent_error(
                                AgentErrorKind::RunBudgetExceeded,
                                "工具调用数达到预算上限。",
                            ));
                            emit!(
                                Some(turn_id),
                                AgentEventPayload::RunTruncated {
                                    reason: TerminationReason::RunBudgetExceeded,
                                }
                            );
                            return Ok(outcome);
                        }
                        outcome.tool_calls += 1;
                        outcome.termination = failure.termination_reason();
                        outcome.error = Some(failure.clone());
                        let now = time.now_unix_ms();
                        let result = ToolResult::failure(call, failure.clone(), now, now);
                        if failure.kind == AgentErrorKind::ToolSchemaInvalid {
                            // 参数 schema 在真正启动工具前失败：preflight rejection，无 active call。
                            emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                        } else {
                            emit!(
                                Some(turn_id),
                                AgentEventPayload::ToolCallStarted { call: call.clone() },
                            );
                            emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                        }
                        emit!(
                            Some(turn_id),
                            AgentEventPayload::RunFailed { error: failure },
                        );
                        return Ok(outcome);
                    }
                    Ok(definition) => executable.push((call, definition)),
                }
            }

            // 批次预算：总调用数与单批并行数。
            let batch_calls = u32::try_from(executable.len()).expect("批次大小不超过 u32");
            if outcome.tool_calls.saturating_add(batch_calls) > budget.max_tool_calls
                || executable.len() > usize::from(budget.max_parallel_tools)
            {
                outcome.termination = TerminationReason::RunBudgetExceeded;
                outcome.error = Some(agent_error(
                    AgentErrorKind::RunBudgetExceeded,
                    "工具调用数或单批并行数超过预算。",
                ));
                emit!(
                    Some(turn_id),
                    AgentEventPayload::RunTruncated {
                        reason: TerminationReason::RunBudgetExceeded,
                    }
                );
                return Ok(outcome);
            }

            let completion_order: Vec<usize> = match request.tool_execution_order {
                ToolExecutionOrder::CallOrder => (0..executable.len()).collect(),
                ToolExecutionOrder::ReverseCallOrder => (0..executable.len()).rev().collect(),
            };
            let mut results_by_id: BTreeMap<String, ToolResult> = BTreeMap::new();
            for batch_position in completion_order {
                let (call, definition) = executable[batch_position];
                if cancellation.is_requested() {
                    outcome.termination = TerminationReason::UserAborted;
                    outcome.error = Some(agent_error(
                        AgentErrorKind::UserAborted,
                        "用户在工具执行期间请求停止。",
                    ));
                    emit!(
                        Some(turn_id),
                        AgentEventPayload::RunStopped {
                            reason: TerminationReason::UserAborted,
                        }
                    );
                    return Ok(outcome);
                }
                if time.now_unix_ms() >= deadline {
                    outcome.termination = TerminationReason::RunBudgetExceeded;
                    outcome.error = Some(agent_error(
                        AgentErrorKind::RunBudgetExceeded,
                        "run 总超时已到。",
                    ));
                    emit!(
                        Some(turn_id),
                        AgentEventPayload::RunTruncated {
                            reason: TerminationReason::RunBudgetExceeded,
                        }
                    );
                    return Ok(outcome);
                }
                emit!(
                    Some(turn_id),
                    AgentEventPayload::ToolCallStarted { call: call.clone() },
                );
                outcome.tool_calls += 1;
                let started = time.now_unix_ms();
                // 边界：tool_timeout_ms 由执行器以 ToolTimeout 错误表达，内核暂不实施
                // 墙钟强制（留给异步 executor 落地）；run 总超时是硬兜底。
                match definition.execute(call, started, &budget) {
                    Err(failure) => {
                        let now = time.now_unix_ms();
                        let result = ToolResult::failure(call, failure.clone(), started, now);
                        if matches!(
                            failure.kind,
                            AgentErrorKind::ToolExecutionFailed | AgentErrorKind::ToolTimeout
                        ) {
                            // 运行期工具失败可恢复：失败结果按原调用顺序回传模型，
                            // 由模型决定诚实降级（方案 §8.2/§20 tool_error_then_recover）。
                            results_by_id.insert(call.id.clone(), result.clone());
                            emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                            continue;
                        }
                        outcome.termination = failure.termination_reason();
                        outcome.error = Some(failure.clone());
                        emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                        emit!(
                            Some(turn_id),
                            AgentEventPayload::RunFailed { error: failure },
                        );
                        return Ok(outcome);
                    }
                    Ok(result) => {
                        // 最终执行边界核对：结果必须属于本次调用，且通过协议级字节预算校验。
                        if result.tool_call_id != call.id || result.tool_name != call.name {
                            let failure = agent_error(
                                AgentErrorKind::ProviderProtocolError,
                                "executor 返回的结果不属于本次 tool call。",
                            );
                            outcome.termination = failure.termination_reason();
                            outcome.error = Some(failure.clone());
                            let now = time.now_unix_ms();
                            let result = ToolResult::failure(call, failure.clone(), started, now);
                            emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                            emit!(
                                Some(turn_id),
                                AgentEventPayload::RunFailed { error: failure },
                            );
                            return Ok(outcome);
                        }
                        if let Err(validation) = result.validate(&budget) {
                            let failure =
                                agent_error(AgentErrorKind::ProviderProtocolError, validation);
                            outcome.termination = failure.termination_reason();
                            outcome.error = Some(failure.clone());
                            let now = time.now_unix_ms();
                            let result = ToolResult::failure(call, failure.clone(), started, now);
                            emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                            emit!(
                                Some(turn_id),
                                AgentEventPayload::RunFailed { error: failure },
                            );
                            return Ok(outcome);
                        }
                        results_by_id.insert(call.id.clone(), result.clone());
                        emit!(
                            Some(turn_id),
                            AgentEventPayload::ToolCallCompleted { result },
                        );
                    }
                }
            }

            // 无论完成顺序如何，回传给模型的结果始终按原始调用顺序组装。
            let ordered_results: Vec<ToolResult> = calls
                .iter()
                .filter_map(|call| results_by_id.get(&call.id))
                .cloned()
                .collect();
            assembler.append_turn(&mut transcript, &text, &calls, &ordered_results);
        }
    }
}

fn agent_error(kind: AgentErrorKind, message: impl Into<String>) -> AgentError {
    AgentError::new(kind, message).expect("coordinator 的固定错误消息必须有效")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::fake_gateway::{FakeGateway, FakeScenario};
    use crate::agent_runtime::gateway::ProviderMessage;
    use crate::agent_runtime::protocol::{validate_event_sequence, AgentSurface, ToolProvenance};
    use crate::agent_runtime::tool::RiskLevel;
    use serde_json::json;
    use std::cell::Cell;
    use std::collections::BTreeSet;

    struct ScriptedTime {
        values: Vec<u64>,
        index: Cell<usize>,
    }

    impl ScriptedTime {
        fn new(values: Vec<u64>) -> Self {
            assert!(!values.is_empty(), "脚本时钟必须至少有一个值");
            Self {
                values,
                index: Cell::new(0),
            }
        }
    }

    impl TimeSource for ScriptedTime {
        fn now_unix_ms(&self) -> u64 {
            let index = self.index.get();
            let value = self
                .values
                .get(index)
                .copied()
                .unwrap_or_else(|| *self.values.last().expect("脚本时钟非空"));
            self.index.set(index + 1);
            value
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        events: Vec<AgentEvent>,
    }

    impl AgentEventSink for RecordingSink {
        fn emit(&mut self, event: AgentEvent) -> Result<(), AgentError> {
            self.events.push(event);
            Ok(())
        }
    }

    fn steady_clock() -> ScriptedTime {
        ScriptedTime::new(vec![1_000_000])
    }

    fn authority() -> AuthorityRef {
        AuthorityRef::conversation(AgentSurface::QuickAiOverlay, 1, 1, 1)
    }

    fn request(order: ToolExecutionOrder) -> RunRequest {
        RunRequest {
            user_prompt: "今天的日期是什么？".to_string(),
            runtime_facts: RuntimeFacts {
                local_datetime: "2026-08-17 10:00".to_string(),
                timezone: "Asia/Shanghai (UTC+8)".to_string(),
                app_version: "0.1.0-test".to_string(),
            },
            capability: CapabilityPolicy::default(),
            tool_execution_order: order,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        run_id: &str,
        scenario: FakeScenario,
        budget: RunBudget,
        tools: Vec<crate::agent_runtime::tool::ToolDefinition>,
        order: ToolExecutionOrder,
        capability: CapabilityPolicy,
        cancellation: &Cancellation,
        clock: ScriptedTime,
    ) -> (RunOutcome, Vec<AgentEvent>, FakeGateway) {
        let mut registry = ToolRegistry::new();
        for tool in tools {
            registry.register(tool).expect("测试注册的工具必须有效");
        }
        let policy = ToolPolicy;
        let assembler = ContextAssembler::default();
        let mut fake = FakeGateway::new(scenario);
        let mut sink = RecordingSink::default();
        let mut coordinator =
            AgentRunCoordinator::new(run_id, authority(), budget).expect("run 身份必须有效");
        let req = request(order);
        let mut req_capability = req;
        req_capability.capability = capability;
        let mut deps = AgentDeps {
            gateway: &mut fake,
            registry: &registry,
            policy: &policy,
            assembler: &assembler,
            time: &clock,
            cancellation,
            sink: &mut sink,
        };
        let outcome = coordinator
            .run(&req_capability, &mut deps)
            .expect("coordinator 不返回 Err");
        drop(deps);
        (outcome, sink.events, fake)
    }

    fn date_tool() -> crate::agent_runtime::tool::ToolDefinition {
        crate::agent_runtime::tool::ToolDefinition::new(
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

    fn version_tool() -> crate::agent_runtime::tool::ToolDefinition {
        crate::agent_runtime::tool::ToolDefinition::new(
            "get_version",
            "返回应用版本。",
            json!({}),
            RiskLevel::TrustedLocalReadOnly,
            |call, started, _| {
                Ok(ToolResult::success(
                    call,
                    "0.1.0-test",
                    ToolProvenance::LocalFact,
                    started,
                    started + 1,
                ))
            },
        )
        .expect("version 工具定义必须有效")
    }

    fn echo_tool() -> crate::agent_runtime::tool::ToolDefinition {
        crate::agent_runtime::tool::ToolDefinition::new(
            "echo",
            "原样返回传入的文本。",
            json!({
                "type": "object",
                "properties": { "text": { "type": "string", "minLength": 1 } },
                "required": ["text"]
            }),
            RiskLevel::TrustedLocalReadOnly,
            |call, started, _| {
                let text = call
                    .arguments
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Ok(ToolResult::success(
                    call,
                    format!("echo:{text}"),
                    ToolProvenance::LocalFact,
                    started,
                    started + 1,
                ))
            },
        )
        .expect("echo 工具定义必须有效")
    }

    #[test]
    fn system_time_source_reads_clock() {
        let now = SystemTimeSource.now_unix_ms();
        assert!(now > 0, "系统时钟必须产生正的时间戳");
    }

    #[test]
    fn model_request_carries_budget_and_deadline() {
        let (_, _, fake) = run(
            "run-test-1",
            FakeScenario::FinalOnly,
            RunBudget::first_version(),
            vec![],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            ScriptedTime::new(vec![1_000_000]),
        );
        let request = &fake.requests[0];
        assert_eq!(request.budget, RunBudget::first_version());
        assert!(
            request.deadline_unix_ms > 1_000_000,
            "deadline 必须基于 run 起点与超时计算"
        );
    }

    #[test]
    fn no_tool_run_completes_in_one_turn() {
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::FinalOnly,
            RunBudget::first_version(),
            vec![],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::FinalAnswer);
        assert_eq!(outcome.final_text.as_deref(), Some("final answer"));
        assert_eq!(outcome.model_turns, 1);
        assert_eq!(outcome.tool_calls, 0);
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn single_tool_result_is_fed_back_then_final_answer() {
        let (outcome, events, fake) = run(
            "run-test-1",
            FakeScenario::SingleToolThenFinal,
            RunBudget::first_version(),
            vec![date_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::FinalAnswer);
        assert_eq!(outcome.final_text.as_deref(), Some("final after tools"));
        assert_eq!(outcome.model_turns, 2);
        assert_eq!(outcome.tool_calls, 1);
        let tool_messages: Vec<String> = fake.requests[1]
            .messages
            .iter()
            .filter_map(|message| match message {
                ProviderMessage::Tool { result } => {
                    Some(format!("{}={}", result.tool_call_id, result.content))
                }
                _ => None,
            })
            .collect();
        assert_eq!(tool_messages, vec!["call-1=2026-08-17"]);
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn multiple_tools_result_order_does_not_depend_on_completion_order() {
        for order in [
            ToolExecutionOrder::CallOrder,
            ToolExecutionOrder::ReverseCallOrder,
        ] {
            let (_, events, fake) = run(
                "run-test-1",
                FakeScenario::MultipleToolsThenFinal,
                RunBudget::first_version(),
                vec![date_tool(), version_tool()],
                order,
                CapabilityPolicy::default(),
                &Cancellation::new(),
                steady_clock(),
            );
            assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());

            let completion_order: Vec<&str> = events
                .iter()
                .filter_map(|event| match &event.payload {
                    AgentEventPayload::ToolCallCompleted { result } => {
                        Some(result.tool_call_id.as_str())
                    }
                    _ => None,
                })
                .collect();
            let expected = if order == ToolExecutionOrder::CallOrder {
                vec!["call-1", "call-2"]
            } else {
                vec!["call-2", "call-1"]
            };
            assert_eq!(completion_order, expected, "order={order:?}");

            let tool_results: Vec<String> = fake.requests[1]
                .messages
                .iter()
                .filter_map(|message| match message {
                    ProviderMessage::Tool { result } => Some(result.tool_call_id.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(tool_results, vec!["call-1", "call-2"], "order={order:?}");
        }
    }

    #[test]
    fn text_before_tools_is_streamed_within_the_same_turn() {
        let (_, events, _) = run(
            "run-test-1",
            FakeScenario::TextThenToolsThenFinal,
            RunBudget::first_version(),
            vec![date_tool(), version_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn unknown_tool_has_clear_termination() {
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::UnknownTool,
            RunBudget::first_version(),
            vec![date_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::UnknownTool);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::UnknownTool
        );
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn invalid_arguments_format_is_rejected_before_execution() {
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::InvalidArgumentsFormat,
            RunBudget::first_version(),
            vec![date_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::ToolSchemaInvalid);
        // preflight rejection 不产生 ToolCallStarted。
        assert!(!events
            .iter()
            .any(|event| matches!(event.payload, AgentEventPayload::ToolCallStarted { .. })));
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn tool_schema_invalid_arguments_terminates() {
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::InvalidSchemaArguments,
            RunBudget::first_version(),
            vec![echo_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::ToolSchemaInvalid);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::ToolSchemaInvalid
        );
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn policy_denied_tool_is_checked_at_final_boundary() {
        let denied = crate::agent_runtime::tool::ToolDefinition::new(
            "read_web",
            "读取外部网页。",
            json!({
                "type": "object",
                "properties": { "url": { "type": "string" } },
                "required": ["url"]
            }),
            RiskLevel::ExternalReadOnly,
            |call, started, _| {
                Ok(ToolResult::success(
                    call,
                    "external",
                    ToolProvenance::ExternalPage,
                    started,
                    started + 1,
                ))
            },
        )
        .expect("read_web 工具定义必须有效");
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::PolicyDeniedTool,
            RunBudget::first_version(),
            vec![denied],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::ToolPolicyDenied);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::ToolPolicyDenied
        );
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn tool_error_then_recover_reaches_final_answer() {
        for kind in [
            AgentErrorKind::ToolExecutionFailed,
            AgentErrorKind::ToolTimeout,
        ] {
            let kind_for_tool = kind.clone();
            let failing = crate::agent_runtime::tool::ToolDefinition::new(
                "get_date",
                "返回当前本地日期。",
                json!({}),
                RiskLevel::TrustedLocalReadOnly,
                move |_, _, _| Err(agent_error(kind_for_tool.clone(), "fake 工具确定性失败")),
            )
            .expect("failing 工具定义必须有效");
            let (outcome, events, fake) = run(
                "run-test-1",
                FakeScenario::ToolErrorThenRecover,
                RunBudget::first_version(),
                vec![failing],
                ToolExecutionOrder::CallOrder,
                CapabilityPolicy::default(),
                &Cancellation::new(),
                steady_clock(),
            );
            assert_eq!(
                outcome.termination,
                TerminationReason::FinalAnswer,
                "kind={kind:?}"
            );
            assert_eq!(
                outcome.final_text.as_deref(),
                Some("recovered final answer"),
                "kind={kind:?}"
            );
            assert_eq!(outcome.tool_calls, 1, "kind={kind:?}");
            // 模型在下一轮看到了失败结果（Tool 消息 is_error=true，错误分类一致）。
            let tool_messages: Vec<&ToolResult> = fake.requests[1]
                .messages
                .iter()
                .filter_map(|message| match message {
                    ProviderMessage::Tool { result } => Some(result),
                    _ => None,
                })
                .collect();
            assert_eq!(tool_messages.len(), 1, "kind={kind:?}");
            assert!(tool_messages[0].is_error, "kind={kind:?}");
            assert_eq!(tool_messages[0].tool_call_id, "call-1");
            assert_eq!(
                tool_messages[0].error.as_ref().unwrap().kind,
                kind,
                "kind={kind:?}"
            );
            assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
        }
    }

    #[test]
    fn invalid_arguments_delta_terminates_with_run_failed() {
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::InvalidArgumentsDelta,
            RunBudget::first_version(),
            vec![date_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(
            outcome.termination,
            TerminationReason::ProviderProtocolError
        );
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::ProviderProtocolError
        );
        assert!(matches!(
            events.last().map(|event| &event.payload),
            Some(AgentEventPayload::RunFailed { .. })
        ));
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn call_validate_failure_after_budget_exhaustion_truncates() {
        let mut budget = RunBudget::first_version();
        budget.max_tool_calls = 1;
        budget.max_parallel_tools = 1;
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::ValidThenInvalidArguments,
            budget,
            vec![date_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::RunBudgetExceeded);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::RunBudgetExceeded
        );
        assert!(matches!(
            events.last().map(|event| &event.payload),
            Some(AgentEventPayload::RunTruncated { .. })
        ));
        assert!(validate_event_sequence(&events, &budget).is_ok());
    }

    #[test]
    fn policy_denied_after_budget_exhaustion_truncates() {
        let denied = crate::agent_runtime::tool::ToolDefinition::new(
            "read_web",
            "读取外部网页。",
            json!({
                "type": "object",
                "properties": { "url": { "type": "string" } },
                "required": ["url"]
            }),
            RiskLevel::ExternalReadOnly,
            |call, started, _| {
                Ok(ToolResult::success(
                    call,
                    "external",
                    ToolProvenance::ExternalPage,
                    started,
                    started + 1,
                ))
            },
        )
        .expect("read_web 工具定义必须有效");
        let mut budget = RunBudget::first_version();
        budget.max_tool_calls = 1;
        budget.max_parallel_tools = 1;
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::ValidThenPolicyDenied,
            budget,
            vec![date_tool(), denied],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::RunBudgetExceeded);
        assert!(matches!(
            events.last().map(|event| &event.payload),
            Some(AgentEventPayload::RunTruncated { .. })
        ));
        assert!(validate_event_sequence(&events, &budget).is_ok());
    }

    #[test]
    fn user_abort_during_model_terminates() {
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::AbortDuringModel,
            RunBudget::first_version(),
            vec![],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::UserAborted);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::UserAborted
        );
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn user_abort_during_tool_terminates() {
        let cancellation = Cancellation::new();
        let cancel_for_tool = cancellation.clone();
        let cancel_tool = crate::agent_runtime::tool::ToolDefinition::new(
            "get_date",
            "返回当前本地日期。",
            json!({}),
            RiskLevel::TrustedLocalReadOnly,
            move |call, started, _| {
                cancel_for_tool.request();
                Ok(ToolResult::success(
                    call,
                    "2026-08-17",
                    ToolProvenance::LocalFact,
                    started,
                    started + 1,
                ))
            },
        )
        .expect("cancel 工具定义必须有效");
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::MultipleToolsThenFinal,
            RunBudget::first_version(),
            vec![cancel_tool, version_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &cancellation,
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::UserAborted);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::UserAborted
        );
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn gateway_error_terminates_with_standard_classification() {
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::GatewayNetworkError,
            RunBudget::first_version(),
            vec![],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::ProviderNetwork);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::ProviderNetwork
        );
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn model_turn_budget_exceeded_terminates() {
        let mut budget = RunBudget::first_version();
        budget.max_model_turns = 1;
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::LoopCalls,
            budget,
            vec![date_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::RunBudgetExceeded);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::RunBudgetExceeded
        );
        assert_eq!(outcome.model_turns, 1);
        assert!(validate_event_sequence(&events, &budget).is_ok());
    }

    #[test]
    fn tool_call_budget_exceeded_terminates() {
        let mut budget = RunBudget::first_version();
        budget.max_tool_calls = 1;
        budget.max_parallel_tools = 1;
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::LoopCalls,
            budget,
            vec![date_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::RunBudgetExceeded);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::RunBudgetExceeded
        );
        assert_eq!(outcome.tool_calls, 1);
        assert!(validate_event_sequence(&events, &budget).is_ok());
    }

    #[test]
    fn parallel_tool_budget_exceeded_terminates() {
        let mut budget = RunBudget::first_version();
        budget.max_parallel_tools = 1;
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::MultipleToolsThenFinal,
            budget,
            vec![date_tool(), version_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(outcome.termination, TerminationReason::RunBudgetExceeded);
        assert_eq!(outcome.tool_calls, 0);
        assert!(validate_event_sequence(&events, &budget).is_ok());
    }

    #[test]
    fn run_timeout_truncates_after_progress() {
        // 读取顺序：deadline、turn1 loop-top、批前超时检查、tool started、turn2 loop-top。
        let clock = ScriptedTime::new(vec![1_000_000, 1_000_000, 1_000_000, 1_000_000, 1_181_000]);
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::SingleToolThenFinal,
            RunBudget::first_version(),
            vec![date_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            clock,
        );
        assert_eq!(outcome.termination, TerminationReason::RunBudgetExceeded);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::RunBudgetExceeded
        );
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn late_tool_result_from_previous_run_is_rejected() {
        let bad = crate::agent_runtime::tool::ToolDefinition::new(
            "get_date",
            "返回当前本地日期。",
            json!({}),
            RiskLevel::TrustedLocalReadOnly,
            |_, started, _| {
                let wrong = ToolCall {
                    id: "call-old".to_string(),
                    name: "get_date".to_string(),
                    arguments: json!({}),
                };
                Ok(ToolResult::success(
                    &wrong,
                    "2026-08-17",
                    ToolProvenance::LocalFact,
                    started,
                    started + 1,
                ))
            },
        )
        .expect("bad 工具定义必须有效");
        let (outcome, events, _) = run(
            "run-test-1",
            FakeScenario::SingleToolThenFinal,
            RunBudget::first_version(),
            vec![bad],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert_eq!(
            outcome.termination,
            TerminationReason::ProviderProtocolError
        );
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AgentErrorKind::ProviderProtocolError
        );
        assert!(validate_event_sequence(&events, &RunBudget::first_version()).is_ok());
    }

    #[test]
    fn late_event_after_new_run_is_rejected_by_run_scope() {
        let (_, events_a, _) = run(
            "run-a",
            FakeScenario::FinalOnly,
            RunBudget::first_version(),
            vec![],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        assert!(events_a.iter().all(|event| event.run_id == "run-a"));

        // run B 通过 RunScopedSink 发布事件；run A 的迟到事件在 B 的作用域被拒绝。
        let mut recording = RecordingSink::default();
        let stale = events_a.last().cloned().expect("run A 有终态事件");
        {
            let mut scoped = RunScopedSink::new("run-b", &mut recording);
            assert!(scoped.emit(stale).is_err());
        }
        assert!(recording.events.is_empty());

        // 若把 run A 的终态事件混入 run B 的事件序列，validate_event_sequence 拒绝。
        let (_, events_b, _) = run(
            "run-b",
            FakeScenario::FinalOnly,
            RunBudget::first_version(),
            vec![],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        let mut mixed = events_b.clone();
        mixed.push(events_a.last().cloned().unwrap());
        assert!(validate_event_sequence(&mixed, &RunBudget::first_version()).is_err());
    }

    #[test]
    fn model_only_sees_active_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(date_tool()).unwrap();
        registry.register(version_tool()).unwrap();
        let mut capability = CapabilityPolicy::default();
        capability.enabled_tools = Some(BTreeSet::from(["get_date".to_string()]));

        let policy = ToolPolicy;
        let assembler = ContextAssembler::default();
        let mut fake = FakeGateway::new(FakeScenario::FinalOnly);
        let mut sink = RecordingSink::default();
        let mut coordinator =
            AgentRunCoordinator::new("run-test-1", authority(), RunBudget::first_version())
                .expect("run 身份必须有效");
        let mut req = request(ToolExecutionOrder::CallOrder);
        req.capability = capability;
        let mut deps = AgentDeps {
            gateway: &mut fake,
            registry: &registry,
            policy: &policy,
            assembler: &assembler,
            time: &steady_clock(),
            cancellation: &Cancellation::new(),
            sink: &mut sink,
        };
        coordinator
            .run(&req, &mut deps)
            .expect("coordinator 不返回 Err");
        drop(deps);
        assert!(outcome_is_final(&sink.events));
        let request_tools: Vec<String> = fake.requests[0]
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        assert_eq!(request_tools, vec!["get_date"]);
    }

    fn outcome_is_final(events: &[AgentEvent]) -> bool {
        matches!(
            events.last().map(|event| &event.payload),
            Some(AgentEventPayload::RunCompleted { .. })
        )
    }

    #[test]
    fn reasoning_is_kept_out_of_agent_events() {
        let (_, events, fake) = run(
            "run-test-1",
            FakeScenario::SingleToolThenFinal,
            RunBudget::first_version(),
            vec![date_tool()],
            ToolExecutionOrder::CallOrder,
            CapabilityPolicy::default(),
            &Cancellation::new(),
            steady_clock(),
        );
        for event in &events {
            let encoded = serde_json::to_string(&event.payload).expect("payload 可序列化");
            assert!(
                !encoded.contains("private chain"),
                "reasoning 不得进入 AgentEvent：{encoded}"
            );
        }
        assert_eq!(
            fake.continuation().private_reasoning.as_deref(),
            Some("private chain")
        );
    }
}
