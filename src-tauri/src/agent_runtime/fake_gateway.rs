//! 仅测试使用的 fake ModelGateway 与确定性场景。
//!
//! 该文件只在测试构建中编译，不访问网络、凭据或 SQLite。它驱动 coordinator 的
//! 无工具、单/多工具、未知工具、坏参数、策略拒绝、provider 错误、循环和停止分支。

use super::gateway::{ModelGateway, ModelRequest, ModelTurnOutcome};
use super::protocol::{
    AgentError, AgentErrorKind, ModelEvent, ModelFinishReason, ModelUsage,
    ProviderContinuationState, ToolCall,
};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FakeScenario {
    FinalOnly,
    /// 最终回答轮 finish_reason=length（任务 4）：文本照常输出，截断只标记。
    FinalTruncated,
    SingleToolThenFinal,
    LearningHistoryThenFinal,
    MultipleToolsThenFinal,
    TextThenToolsThenFinal,
    ToolErrorThenRecover,
    UnknownTool,
    InvalidArgumentsFormat,
    InvalidSchemaArguments,
    PolicyDeniedTool,
    GatewayNetworkError,
    LoopCalls,
    AbortDuringModel,
    InvalidArgumentsDelta,
    ValidThenInvalidArguments,
    ValidThenPolicyDenied,
}

pub(crate) struct FakeGateway {
    scenario: FakeScenario,
    turn: u32,
    pub requests: Vec<ModelRequest>,
    continuation: ProviderContinuationState,
}

impl FakeGateway {
    pub fn new(scenario: FakeScenario) -> Self {
        Self {
            scenario,
            turn: 0,
            requests: Vec::new(),
            continuation: ProviderContinuationState {
                provider: "fake-provider".to_string(),
                response_id: Some("fake-response-1".to_string()),
                tool_call_state: None,
                private_reasoning: None,
                provider_extensions: None,
            },
        }
    }
}

fn call(id: impl Into<String>, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.to_string(),
        arguments,
    }
}

fn usage_event() -> ModelEvent {
    ModelEvent::Usage {
        usage: ModelUsage {
            prompt_tokens: 10,
            completion_tokens: 2,
            total_tokens: 12,
        },
    }
}

fn tool_calls_for(scenario: FakeScenario) -> Vec<ToolCall> {
    match scenario {
        FakeScenario::SingleToolThenFinal | FakeScenario::ToolErrorThenRecover => {
            vec![call("call-1", "get_date", json!({}))]
        }
        FakeScenario::LearningHistoryThenFinal => vec![call(
            "call-learning-history-1",
            "query_learning_history",
            json!({"mode": "recent", "period": "last_7_days", "query_type": "word"}),
        )],
        FakeScenario::MultipleToolsThenFinal | FakeScenario::TextThenToolsThenFinal => vec![
            call("call-1", "get_date", json!({})),
            call("call-2", "get_version", json!({})),
        ],
        _ => Vec::new(),
    }
}

impl ModelGateway for FakeGateway {
    fn stream_model(
        &mut self,
        request: ModelRequest,
        on_event: &mut dyn FnMut(ModelEvent) -> Result<(), AgentError>,
    ) -> Result<ModelTurnOutcome, AgentError> {
        self.turn += 1;
        self.requests.push(request.clone());
        let turn = self.turn;
        let scenario = self.scenario;

        match scenario {
            FakeScenario::FinalOnly => {
                on_event(ModelEvent::TextDelta {
                    text: "final answer".to_string(),
                })?;
                on_event(usage_event())?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::Stop,
                })?;
            }
            FakeScenario::FinalTruncated => {
                on_event(ModelEvent::TextDelta {
                    text: "partial answer".to_string(),
                })?;
                on_event(usage_event())?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::Length,
                })?;
            }
            FakeScenario::SingleToolThenFinal
            | FakeScenario::LearningHistoryThenFinal
            | FakeScenario::MultipleToolsThenFinal
            | FakeScenario::TextThenToolsThenFinal
            | FakeScenario::ToolErrorThenRecover
                if turn == 1 =>
            {
                if scenario == FakeScenario::TextThenToolsThenFinal {
                    on_event(ModelEvent::TextDelta {
                        text: "Let me check that.".to_string(),
                    })?;
                }
                self.continuation.private_reasoning = Some("private chain".to_string());
                on_event(ModelEvent::ReasoningDelta {
                    text: "private chain".to_string(),
                })?;
                for tool_call in tool_calls_for(scenario) {
                    on_event(ModelEvent::ToolCall { call: tool_call })?;
                }
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::SingleToolThenFinal
            | FakeScenario::LearningHistoryThenFinal
            | FakeScenario::MultipleToolsThenFinal
            | FakeScenario::TextThenToolsThenFinal => {
                on_event(ModelEvent::TextDelta {
                    text: "final after tools".to_string(),
                })?;
                on_event(usage_event())?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::Stop,
                })?;
            }
            FakeScenario::ToolErrorThenRecover => {
                on_event(ModelEvent::TextDelta {
                    text: "recovered final answer".to_string(),
                })?;
                on_event(usage_event())?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::Stop,
                })?;
            }
            FakeScenario::UnknownTool => {
                on_event(ModelEvent::ToolCall {
                    call: call("call-unknown", "not_registered", json!({})),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::InvalidArgumentsFormat => {
                on_event(ModelEvent::ToolCall {
                    call: call("call-invalid", "get_date", json!(["not", "an", "object"])),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::InvalidSchemaArguments => {
                on_event(ModelEvent::ToolCall {
                    call: call("call-schema", "echo", json!({"unexpected": true})),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::PolicyDeniedTool => {
                on_event(ModelEvent::ToolCall {
                    call: call(
                        "call-denied",
                        "read_web",
                        json!({"url": "https://example.com"}),
                    ),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::GatewayNetworkError => {
                return Err(AgentError::new(
                    AgentErrorKind::ProviderNetwork,
                    "fake provider 模拟网络失败",
                )
                .expect("固定错误消息必须有效"));
            }
            FakeScenario::LoopCalls => {
                on_event(ModelEvent::ToolCall {
                    call: call(format!("call-loop-{turn}"), "get_date", json!({})),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::AbortDuringModel => {
                on_event(ModelEvent::TextDelta {
                    text: "partial".to_string(),
                })?;
                request.cancellation.request();
                return Ok(ModelTurnOutcome { aborted: true });
            }
            FakeScenario::InvalidArgumentsDelta => {
                on_event(ModelEvent::ToolCall {
                    call: call("call-1", "get_date", json!({})),
                })?;
                on_event(ModelEvent::ToolCallArgumentsDelta {
                    tool_call_id: "call-1".to_string(),
                    delta: "{broken".to_string(),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::ValidThenInvalidArguments if turn == 1 => {
                on_event(ModelEvent::ToolCall {
                    call: call("call-1", "get_date", json!({})),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::ValidThenInvalidArguments => {
                on_event(ModelEvent::ToolCall {
                    call: call("call-invalid-2", "get_date", json!(["not", "an", "object"])),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::ValidThenPolicyDenied if turn == 1 => {
                on_event(ModelEvent::ToolCall {
                    call: call("call-1", "get_date", json!({})),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
            FakeScenario::ValidThenPolicyDenied => {
                on_event(ModelEvent::ToolCall {
                    call: call(
                        "call-denied-2",
                        "read_web",
                        json!({"url": "https://example.com"}),
                    ),
                })?;
                on_event(ModelEvent::Completed {
                    reason: ModelFinishReason::ToolCalls,
                })?;
            }
        }

        Ok(ModelTurnOutcome { aborted: false })
    }

    fn continuation(&self) -> &ProviderContinuationState {
        &self.continuation
    }
}
