//! 任务 0 的 deterministic fake-provider replay harness。
//!
//! 该文件只在测试构建中编译。它不读取凭据、不访问网络、不打开 SQLite，也不
//! 连接任何 Tauri command；目的只是把协议分支固定成可重复的离线证据。

use super::protocol::*;
use serde_json::json;
use std::collections::HashMap;

const REPLAY_RUN_ID: &str = "run-replay-1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplayScenario {
    FinalOnly,
    SingleTool,
    /// Controlled completion-order fixture: execution is deliberately sequential;
    /// only the completion order differs from the model call order.
    MultipleTools,
    UnknownTool,
    InvalidArguments,
    ToolFailure,
    ToolTimeout,
    AbortDuringModel,
    AbortDuringTool,
    BudgetExceeded,
}

#[derive(Debug)]
pub(crate) struct ReplayReport {
    pub events: Vec<AgentEvent>,
    pub final_text: Option<String>,
    pub termination: TerminationReason,
    pub error: Option<AgentError>,
    pub model_turns: u32,
    pub tool_calls: u32,
    pub completion_order: Vec<String>,
    pub ordered_results_for_model: Vec<String>,
    pub private_reasoning_seen: bool,
}

impl ReplayReport {
    fn new(events: Vec<AgentEvent>) -> Self {
        Self {
            events,
            final_text: None,
            termination: TerminationReason::ProviderProtocolError,
            error: None,
            model_turns: 0,
            tool_calls: 0,
            completion_order: Vec::new(),
            ordered_results_for_model: Vec::new(),
            private_reasoning_seen: false,
        }
    }
}

struct FakeProvider {
    scenario: ReplayScenario,
    model_turns: u32,
}

impl FakeProvider {
    fn new(scenario: ReplayScenario) -> Self {
        Self {
            scenario,
            model_turns: 0,
        }
    }

    fn model_events(&mut self) -> Vec<ModelEvent> {
        self.model_turns += 1;
        let turn = self.model_turns;
        match self.scenario {
            ReplayScenario::FinalOnly => vec![
                ModelEvent::TextDelta {
                    text: "final answer".to_string(),
                },
                ModelEvent::Usage {
                    usage: ModelUsage {
                        prompt_tokens: 10,
                        completion_tokens: 2,
                        total_tokens: 12,
                    },
                },
                ModelEvent::Completed {
                    reason: ModelFinishReason::Stop,
                },
            ],
            ReplayScenario::SingleTool | ReplayScenario::MultipleTools if turn == 1 => {
                tool_events(self.scenario)
            }
            ReplayScenario::UnknownTool | ReplayScenario::InvalidArguments if turn == 1 => {
                tool_events(self.scenario)
            }
            ReplayScenario::ToolFailure | ReplayScenario::ToolTimeout if turn == 1 => vec![
                ModelEvent::ToolCall {
                    call: date_call("call-failure", "get_date", json!({})),
                },
                ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                },
            ],
            ReplayScenario::AbortDuringTool if turn == 1 => vec![
                ModelEvent::ToolCall {
                    call: date_call("call-abort", "get_date", json!({})),
                },
                ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                },
            ],
            ReplayScenario::BudgetExceeded => vec![
                ModelEvent::ReasoningDelta {
                    text: "private continuation".to_string(),
                },
                ModelEvent::ToolCall {
                    call: date_call(&format!("call-budget-{turn}"), "get_date", json!({})),
                },
                ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                },
            ],
            ReplayScenario::SingleTool | ReplayScenario::MultipleTools => final_events(),
            ReplayScenario::ToolFailure
            | ReplayScenario::ToolTimeout
            | ReplayScenario::AbortDuringTool
            | ReplayScenario::UnknownTool
            | ReplayScenario::InvalidArguments
            | ReplayScenario::AbortDuringModel => final_events(),
        }
    }

    fn execute_tool(
        &self,
        call: &ToolCall,
        started_at_unix_ms: u64,
    ) -> Result<ToolResult, AgentError> {
        let (kind, message) = match self.scenario {
            ReplayScenario::ToolFailure => (
                AgentErrorKind::ToolExecutionFailed,
                "fake tool returned a deterministic failure",
            ),
            ReplayScenario::ToolTimeout => (
                AgentErrorKind::ToolTimeout,
                "fake tool exceeded its deterministic timeout",
            ),
            _ => {
                return Ok(ToolResult::success(
                    call,
                    "2026-08-16",
                    ToolProvenance::LocalFact,
                    started_at_unix_ms,
                    started_at_unix_ms + 1,
                ))
            }
        };
        let error = AgentError::new(kind, message).expect("fixed fake error is valid");
        Err(error)
    }
}

fn final_events() -> Vec<ModelEvent> {
    vec![
        ModelEvent::TextDelta {
            text: "final after tools".to_string(),
        },
        ModelEvent::Usage {
            usage: ModelUsage {
                prompt_tokens: 20,
                completion_tokens: 4,
                total_tokens: 24,
            },
        },
        ModelEvent::Completed {
            reason: ModelFinishReason::Stop,
        },
    ]
}

fn tool_events(scenario: ReplayScenario) -> Vec<ModelEvent> {
    let mut events = vec![ModelEvent::ToolCall {
        call: match scenario {
            ReplayScenario::UnknownTool => date_call("call-unknown", "not_registered", json!({})),
            ReplayScenario::InvalidArguments => {
                date_call("call-invalid", "get_date", json!(["not", "an", "object"]))
            }
            _ => date_call("call-1", "get_date", json!({})),
        },
    }];
    if scenario == ReplayScenario::MultipleTools {
        events.push(ModelEvent::ToolCall {
            call: date_call("call-2", "get_date", json!({"timezone": "Asia/Shanghai"})),
        });
    }
    events.push(ModelEvent::Completed {
        reason: ModelFinishReason::ToolCalls,
    });
    events
}

fn date_call(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
    }
}

pub(crate) fn replay(scenario: ReplayScenario, budget: RunBudget) -> ReplayReport {
    // 预算本身是协议输入；测试场景可以显式缩小它，但不能绕过校验。
    budget.validate().expect("replay budget must be valid");
    let authority = AuthorityRef::conversation(AgentSurface::QuickAiOverlay, 1, 1, 1);
    let mut events = Vec::new();
    let mut step_sequence = 0_u64;
    macro_rules! emit {
        ($turn_id:expr, $payload:expr $(,)?) => {{
            step_sequence += 1;
            let event = AgentEvent::new(REPLAY_RUN_ID, $turn_id, step_sequence, $payload)
                .expect("replay emits valid envelope");
            events.push(event);
        }};
    }
    emit!(
        None,
        AgentEventPayload::RunStarted {
            surface: AgentSurface::QuickAiOverlay,
            authority,
        },
    );

    let mut report = ReplayReport::new(Vec::new());
    let mut provider = FakeProvider::new(scenario);
    let mut text = String::new();
    let mut usage = None;
    let mut continuation_state = ProviderContinuationState {
        provider: "fake-provider".to_string(),
        response_id: Some("fake-response-1".to_string()),
        tool_call_state: None,
        private_reasoning: None,
        provider_extensions: None,
    };

    loop {
        if report.model_turns >= budget.max_model_turns {
            let error = error(
                AgentErrorKind::RunBudgetExceeded,
                "model turn budget exceeded",
            );
            emit!(
                Some(report.model_turns),
                AgentEventPayload::RunTruncated {
                    reason: TerminationReason::RunBudgetExceeded,
                },
            );
            report.termination = TerminationReason::RunBudgetExceeded;
            report.error = Some(error);
            report.events = events;
            return report;
        }

        report.model_turns += 1;
        let turn_id = report.model_turns;
        emit!(
            Some(turn_id),
            AgentEventPayload::TurnStarted {
                turn_index: turn_id,
            },
        );
        if scenario == ReplayScenario::AbortDuringModel {
            emit!(
                Some(turn_id),
                AgentEventPayload::RunStopped {
                    reason: TerminationReason::UserAborted,
                },
            );
            report.termination = TerminationReason::UserAborted;
            report.error = Some(error(
                AgentErrorKind::UserAborted,
                "user aborted during model",
            ));
            report.events = events;
            return report;
        }

        let model_events = provider.model_events();
        let mut calls = Vec::new();
        for model_event in model_events {
            match model_event {
                ModelEvent::TextDelta { text: delta } => {
                    text.push_str(&delta);
                    emit!(
                        Some(turn_id),
                        AgentEventPayload::AssistantTextDelta { text: delta },
                    );
                }
                ModelEvent::ReasoningDelta { text: delta } => {
                    continuation_state.private_reasoning = Some(delta);
                    report.private_reasoning_seen = true;
                }
                ModelEvent::ToolCall { call } => calls.push(call),
                ModelEvent::ToolCallArgumentsDelta { .. } => {}
                ModelEvent::Usage { usage: next } => usage = Some(next),
                ModelEvent::SourceMetadata { source } => emit!(
                    Some(turn_id),
                    AgentEventPayload::SourcesUpdated {
                        sources: vec![source],
                    },
                ),
                ModelEvent::Completed { .. } => {}
            }
        }

        if calls.is_empty() {
            if text.trim().is_empty() {
                let failure = error(
                    AgentErrorKind::ProviderProtocolError,
                    "fake provider returned no final text",
                );
                emit!(
                    Some(turn_id),
                    AgentEventPayload::RunFailed {
                        error: failure.clone(),
                    },
                );
                report.termination = failure.termination_reason();
                report.error = Some(failure);
            } else {
                emit!(
                    Some(turn_id),
                    AgentEventPayload::AssistantTextCompleted { text: text.clone() },
                );
                emit!(
                    Some(turn_id),
                    AgentEventPayload::RunCompleted {
                        text: text.clone(),
                        usage,
                    },
                );
                report.final_text = Some(text);
                report.termination = TerminationReason::FinalAnswer;
            }
            report.events = events;
            return report;
        }

        let calls_len = u32::try_from(calls.len()).expect("fake calls fit u32");
        if report.tool_calls.saturating_add(calls_len) > budget.max_tool_calls
            || calls.len() > usize::from(budget.max_parallel_tools)
        {
            let exceeded = error(AgentErrorKind::RunBudgetExceeded, "tool budget exceeded");
            emit!(
                Some(turn_id),
                AgentEventPayload::RunTruncated {
                    reason: TerminationReason::RunBudgetExceeded,
                },
            );
            report.termination = TerminationReason::RunBudgetExceeded;
            report.error = Some(exceeded);
            report.events = events;
            return report;
        }

        let mut results_by_id = HashMap::new();
        let execution_order: Vec<&ToolCall> = if scenario == ReplayScenario::MultipleTools {
            calls.iter().rev().collect()
        } else {
            calls.iter().collect()
        };
        for (index, call) in execution_order.into_iter().enumerate() {
            report.tool_calls += 1;
            if let Err(validation) = call.validate() {
                let failure = error(AgentErrorKind::ToolSchemaInvalid, validation);
                let result = ToolResult::failure(call, failure.clone(), 10, 11);
                report.termination = failure.termination_reason();
                report.error = Some(failure.clone());
                emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                emit!(
                    Some(turn_id),
                    AgentEventPayload::RunFailed { error: failure },
                );
                report.events = events;
                return report;
            }
            if call.name != "get_date" {
                let failure = error(AgentErrorKind::UnknownTool, "tool is not active");
                let result = ToolResult::failure(call, failure.clone(), 10, 11);
                report.termination = failure.termination_reason();
                report.error = Some(failure.clone());
                emit!(
                    Some(turn_id),
                    AgentEventPayload::ToolCallStarted { call: call.clone() },
                );
                emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                emit!(
                    Some(turn_id),
                    AgentEventPayload::RunFailed { error: failure },
                );
                report.events = events;
                return report;
            }
            emit!(
                Some(turn_id),
                AgentEventPayload::ToolCallStarted { call: call.clone() },
            );
            if scenario == ReplayScenario::AbortDuringTool {
                emit!(
                    Some(turn_id),
                    AgentEventPayload::RunStopped {
                        reason: TerminationReason::UserAborted,
                    },
                );
                report.termination = TerminationReason::UserAborted;
                report.error = Some(error(
                    AgentErrorKind::UserAborted,
                    "user aborted during tool",
                ));
                report.events = events;
                return report;
            }
            match provider.execute_tool(call, 10 + index as u64) {
                Ok(result) => {
                    report.completion_order.push(call.id.clone());
                    results_by_id.insert(call.id.clone(), result.clone());
                    emit!(
                        Some(turn_id),
                        AgentEventPayload::ToolCallCompleted { result },
                    );
                }
                Err(failure) => {
                    let result = ToolResult::failure(call, failure.clone(), 10, 11);
                    emit!(Some(turn_id), AgentEventPayload::ToolCallFailed { result },);
                    emit!(
                        Some(turn_id),
                        AgentEventPayload::RunFailed {
                            error: failure.clone(),
                        },
                    );
                    report.termination = failure.termination_reason();
                    report.error = Some(failure);
                    report.events = events;
                    return report;
                }
            }
        }

        report.ordered_results_for_model = calls
            .iter()
            .filter_map(|call| results_by_id.get(&call.id))
            .map(|result| format!("{}={}", result.tool_call_id, result.content))
            .collect();
        continuation_state.tool_call_state = Some(json!({
            "toolCallIds": calls.iter().map(|call| call.id.clone()).collect::<Vec<_>>(),
        }));
        // 私有 continuation 仅用于下一次 fake provider 请求，绝不创建 AgentEvent。
    }
}

fn error(kind: AgentErrorKind, message: impl Into<String>) -> AgentError {
    AgentError::new(kind, message).expect("fixed replay error is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid(report: &ReplayReport, budget: &RunBudget) {
        assert!(
            validate_event_sequence(&report.events, budget).is_ok(),
            "events: {:?}",
            report.events
        );
    }

    #[test]
    fn replay_final_only_completes_without_tools() {
        let report = replay(ReplayScenario::FinalOnly, RunBudget::first_version());
        assert_eq!(report.termination, TerminationReason::FinalAnswer);
        assert_eq!(report.tool_calls, 0);
        assert_eq!(report.final_text.as_deref(), Some("final answer"));
        assert_valid(&report, &RunBudget::first_version());
    }

    #[test]
    fn replay_single_tool_then_final() {
        let report = replay(ReplayScenario::SingleTool, RunBudget::first_version());
        assert_eq!(report.termination, TerminationReason::FinalAnswer);
        assert_eq!(report.tool_calls, 1);
        assert_eq!(report.final_text.as_deref(), Some("final after tools"));
        assert_valid(&report, &RunBudget::first_version());
    }

    #[test]
    fn replay_multiple_tools_preserves_model_result_order() {
        let report = replay(ReplayScenario::MultipleTools, RunBudget::first_version());
        assert_eq!(report.termination, TerminationReason::FinalAnswer);
        assert_eq!(report.completion_order, vec!["call-2", "call-1"]);
        assert_eq!(
            report.ordered_results_for_model,
            vec!["call-1=2026-08-16", "call-2=2026-08-16"]
        );
        assert_valid(&report, &RunBudget::first_version());
    }

    #[test]
    fn replay_unknown_tool_is_classified_and_does_not_continue() {
        let report = replay(ReplayScenario::UnknownTool, RunBudget::first_version());
        assert_eq!(report.termination, TerminationReason::UnknownTool);
        assert_eq!(
            report.error.as_ref().unwrap().kind,
            AgentErrorKind::UnknownTool
        );
        assert!(report.final_text.is_none());
        assert_valid(&report, &RunBudget::first_version());
    }

    #[test]
    fn replay_invalid_arguments_are_rejected_before_execution() {
        let report = replay(ReplayScenario::InvalidArguments, RunBudget::first_version());
        assert_eq!(report.termination, TerminationReason::ToolSchemaInvalid);
        assert_eq!(report.tool_calls, 1);
        assert_valid(&report, &RunBudget::first_version());
    }

    #[test]
    fn replay_tool_failure_and_timeout_are_distinct() {
        let failure = replay(ReplayScenario::ToolFailure, RunBudget::first_version());
        assert_eq!(failure.termination, TerminationReason::ToolExecutionFailed);
        let timeout = replay(ReplayScenario::ToolTimeout, RunBudget::first_version());
        assert_eq!(timeout.termination, TerminationReason::ToolTimeout);
        assert_valid(&failure, &RunBudget::first_version());
        assert_valid(&timeout, &RunBudget::first_version());
    }

    #[test]
    fn replay_abort_covers_model_and_tool_boundaries() {
        let model = replay(ReplayScenario::AbortDuringModel, RunBudget::first_version());
        let tool = replay(ReplayScenario::AbortDuringTool, RunBudget::first_version());
        assert_eq!(model.termination, TerminationReason::UserAborted);
        assert_eq!(tool.termination, TerminationReason::UserAborted);
        assert_valid(&model, &RunBudget::first_version());
        assert_valid(&tool, &RunBudget::first_version());
    }

    #[test]
    fn replay_model_turn_budget_is_independent_and_private_reasoning_is_not_an_agent_event() {
        let mut budget = RunBudget::first_version();
        budget.max_model_turns = 1;
        let report = replay(ReplayScenario::BudgetExceeded, budget);
        assert_eq!(report.termination, TerminationReason::RunBudgetExceeded);
        assert_eq!(report.model_turns, 1);
        assert!(report.private_reasoning_seen);
        for event in &report.events {
            let encoded = serde_json::to_string(&event.payload).unwrap();
            assert!(!encoded.contains("private continuation"));
        }
        assert_valid(&report, &budget);
    }

    #[test]
    fn replay_tool_call_budget_is_independent() {
        let mut budget = RunBudget::first_version();
        budget.max_tool_calls = 1;
        budget.max_parallel_tools = 1;
        let report = replay(ReplayScenario::BudgetExceeded, budget);
        assert_eq!(report.termination, TerminationReason::RunBudgetExceeded);
        assert_eq!(report.tool_calls, 1);
        assert_valid(&report, &budget);
    }

    #[test]
    fn replay_parallel_tool_budget_is_independent() {
        let mut budget = RunBudget::first_version();
        budget.max_parallel_tools = 1;
        let report = replay(ReplayScenario::MultipleTools, budget);
        assert_eq!(report.termination, TerminationReason::RunBudgetExceeded);
        assert_eq!(report.tool_calls, 0);
        assert_valid(&report, &budget);
    }
}
