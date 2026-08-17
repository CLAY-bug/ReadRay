//! DeepSeekChatGateway：基于既有 chat/completions 传输的真实 ModelGateway。
//!
//! 复用 `deepseek_client::shared_http_client` 与 SSE 流解析，不引入第二个 HTTP
//! 客户端。HTTP 流通过 `ChatCompletionStreamer` 注入：生产使用真实传输，测试
//! 使用确定性 fake 流，因此本模块可以完全离线验证。流式 tool_calls 按 OpenAI
//! 格式跨分片累积 id/name/arguments，流结束后输出完整 `ToolCall`。Responses
//! API 因 spike 返回 400 未经确认，不作为本任务依据。

use crate::agent_runtime::gateway::{
    ModelGateway, ModelRequest, ModelTurnOutcome, ProviderMessage,
};
use crate::agent_runtime::protocol::{
    AgentError, AgentErrorKind, ModelEvent, ModelFinishReason, ModelUsage,
    ProviderContinuationState, ToolCall,
};
use crate::agent_runtime::tool::ToolSchema;
use crate::deepseek_client::{
    parse_model_token_usage_value, stream_chat_completion_events, StreamChunk,
};
use crate::model_usage::{record_for_app, ModelTokenUsage, ModelUsageCategory};
use futures_util::future::BoxFuture;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

/// 与既有 Quick AI 请求保持一致的生成参数（quick_ai.rs 私有常量）。
const AGENT_MAX_TOKENS: u16 = 8_192;
const AGENT_TEMPERATURE: f32 = 0.5;

pub(crate) type StreamChunkStream =
    futures_util::stream::BoxStream<'static, Result<StreamChunk, String>>;

/// 可注入的 chat/completions SSE 传输边界。
pub(crate) trait ChatCompletionStreamer: Send + Sync {
    fn request_stream(
        &self,
        operation: &str,
        request_body: &Value,
        api_key: &str,
    ) -> BoxFuture<'static, Result<StreamChunkStream, String>>;
}

pub(crate) struct DeepSeekTransport;

impl ChatCompletionStreamer for DeepSeekTransport {
    fn request_stream(
        &self,
        operation: &str,
        request_body: &Value,
        api_key: &str,
    ) -> BoxFuture<'static, Result<StreamChunkStream, String>> {
        let operation = operation.to_string();
        let request_body = request_body.clone();
        let api_key = api_key.to_string();
        Box::pin(
            async move { stream_chat_completion_events(&operation, &request_body, &api_key).await },
        )
    }
}

#[derive(Default)]
struct ToolCallState {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub(crate) struct DeepSeekChatGateway {
    model: String,
    operation: String,
    streamer: Box<dyn ChatCompletionStreamer>,
    record_usage: Box<dyn Fn(&ModelUsage) -> Result<(), String> + Send + Sync>,
    resolve_api_key: Box<dyn Fn() -> Result<String, AgentError> + Send + Sync>,
    continuation: ProviderContinuationState,
}

impl DeepSeekChatGateway {
    pub(crate) fn new(model: impl Into<String>, app: Option<&AppHandle>) -> Self {
        let app = app.cloned();
        Self {
            model: model.into(),
            operation: "DeepSeek Quick AI Agent".to_string(),
            streamer: Box::new(DeepSeekTransport),
            record_usage: Box::new(move |usage| {
                let Some(app) = &app else {
                    return Ok(());
                };
                record_for_app(
                    app,
                    ModelUsageCategory::QuickAi,
                    usage_to_token_usage(usage),
                )
            }),
            resolve_api_key: Box::new(|| {
                crate::secret_store::deepseek_api_key_state()
                    .map_err(|error| agent_error(AgentErrorKind::ProviderAuthFailed, error))?
                    .into_key()
                    .ok_or_else(|| {
                        agent_error(
                            AgentErrorKind::ProviderAuthFailed,
                            "未配置 DeepSeek API Key，无法执行 Agent 请求。",
                        )
                    })
            }),
            continuation: ProviderContinuationState {
                provider: "deepseek-chat-completions".to_string(),
                response_id: None,
                tool_call_state: None,
                private_reasoning: None,
                provider_extensions: None,
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn with_streamer(
        model: impl Into<String>,
        streamer: Box<dyn ChatCompletionStreamer>,
        record_usage: Box<dyn Fn(&ModelUsage) -> Result<(), String> + Send + Sync>,
    ) -> Self {
        Self {
            model: model.into(),
            operation: "fake-deepseek-gateway".to_string(),
            streamer,
            record_usage,
            resolve_api_key: Box::new(|| Ok("test-api-key".to_string())),
            continuation: ProviderContinuationState {
                provider: "deepseek-chat-completions".to_string(),
                response_id: None,
                tool_call_state: None,
                private_reasoning: None,
                provider_extensions: None,
            },
        }
    }
}

impl ModelGateway for DeepSeekChatGateway {
    fn stream_model(
        &mut self,
        request: ModelRequest,
        on_event: &mut dyn FnMut(ModelEvent) -> Result<(), AgentError>,
    ) -> Result<ModelTurnOutcome, AgentError> {
        let api_key = (self.resolve_api_key)()?;
        let request_body = build_chat_request_body(&self.model, &request.messages, &request.tools);
        let operation = self.operation.clone();
        let deadline_unix_ms = request.deadline_unix_ms;
        let cancellation = request.cancellation.clone();

        tauri::async_runtime::block_on(async move {
            let mut stream = Box::pin(
                self.streamer
                    .request_stream(&operation, &request_body, &api_key)
                    .await
                    .map_err(|error| {
                        agent_error(
                            AgentErrorKind::ProviderNetwork,
                            format!("{operation} 请求失败：{error}"),
                        )
                    })?,
            );
            let mut tool_states: BTreeMap<usize, ToolCallState> = BTreeMap::new();
            let mut finish_reason = ModelFinishReason::Stop;
            let mut aborted = false;

            while let Some(chunk) = stream.next().await {
                if cancellation.is_requested() {
                    aborted = true;
                    break;
                }
                if now_unix_ms() >= deadline_unix_ms {
                    return Err(agent_error(
                        AgentErrorKind::ProviderTimeout,
                        "模型请求超过 run 总超时。",
                    ));
                }
                let chunk = chunk.map_err(|error| {
                    agent_error(
                        AgentErrorKind::ProviderNetwork,
                        format!("{operation} 流式响应读取失败：{error}"),
                    )
                })?;
                if !chunk.delta.is_empty() {
                    on_event(ModelEvent::TextDelta {
                        text: chunk.delta.clone(),
                    })
                    .map_err(propagate_sink_error)?;
                }
                if let Some(reasoning) = chunk.reasoning {
                    // 私有推理只留在内存 continuation，绝不投影为 AgentEvent。
                    self.continuation.private_reasoning = Some(reasoning.clone());
                    on_event(ModelEvent::ReasoningDelta { text: reasoning })
                        .map_err(propagate_sink_error)?;
                }
                if let Some(calls) = chunk.tool_calls {
                    for delta in calls {
                        let state = tool_states.entry(delta.index).or_default();
                        if let Some(id) = delta.id {
                            state.id = Some(id);
                        }
                        if let Some(function) = delta.function {
                            if let Some(name) = function.name {
                                state.name = Some(name);
                            }
                            if let Some(arguments) = function.arguments {
                                state.arguments.push_str(&arguments);
                            }
                        }
                    }
                }
                if let Some(raw_usage) = chunk.usage {
                    // "存在但格式非法"的 usage 只记日志并跳过，不杀死 run：
                    // 统计失败不改变业务结果（与既有链路策略一致）。
                    let Ok(parsed) = parse_model_token_usage_value(&raw_usage) else {
                        eprintln!("READRAY_AGENT_USAGE_PARSE_FAILED={raw_usage}");
                        continue;
                    };
                    let usage = ModelUsage {
                        prompt_tokens: parsed.prompt_tokens,
                        completion_tokens: parsed.completion_tokens,
                        total_tokens: parsed.total_tokens,
                    };
                    // 使用量尽力记录，统计写入失败不改变模型结果。
                    if let Err(error) = (self.record_usage)(&usage) {
                        eprintln!("READRAY_AGENT_USAGE_RECORD_FAILED={error}");
                    }
                    on_event(ModelEvent::Usage { usage }).map_err(propagate_sink_error)?;
                }
                if let Some(reason) = chunk.finish_reason.as_deref() {
                    finish_reason = match reason {
                        "stop" => ModelFinishReason::Stop,
                        "tool_calls" => ModelFinishReason::ToolCalls,
                        "length" => {
                            // 边界：Length 已作为 Completed{Length} 输出，但任务 2
                            // 协调器会把截断文本按完整回答持久化；截断的继续/UI
                            // 语义属任务 4，此处只标注不实现。
                            ModelFinishReason::Length
                        }
                        other => ModelFinishReason::Provider(other.to_string()),
                    };
                }
            }

            if aborted {
                // 取消是内核的 RunStopped 边界：不输出已累积的 tool calls 与 Completed。
                return Ok(ModelTurnOutcome { aborted });
            }

            // 流结束后输出累积的完整 tool calls；解析失败属于 provider 协议错误。
            for (_, state) in tool_states {
                let id = state.id.ok_or_else(|| {
                    agent_error(
                        AgentErrorKind::ProviderProtocolError,
                        "tool call 增量缺少 id。",
                    )
                })?;
                let name = state.name.ok_or_else(|| {
                    agent_error(
                        AgentErrorKind::ProviderProtocolError,
                        "tool call 增量缺少名称。",
                    )
                })?;
                let arguments: Value = serde_json::from_str(&state.arguments).map_err(|_| {
                    agent_error(
                        AgentErrorKind::ProviderProtocolError,
                        "tool call 参数增量不是合法 JSON。",
                    )
                })?;
                if !arguments.is_object() {
                    return Err(agent_error(
                        AgentErrorKind::ProviderProtocolError,
                        "tool call 参数不是 JSON object。",
                    ));
                }
                on_event(ModelEvent::ToolCall {
                    call: ToolCall {
                        id,
                        name,
                        arguments,
                    },
                })
                .map_err(propagate_sink_error)?;
            }
            on_event(ModelEvent::Completed {
                reason: finish_reason,
            })
            .map_err(propagate_sink_error)?;
            Ok(ModelTurnOutcome { aborted })
        })
    }

    fn continuation(&self) -> &ProviderContinuationState {
        &self.continuation
    }
}

fn propagate_sink_error(error: AgentError) -> AgentError {
    error
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn usage_to_token_usage(usage: &ModelUsage) -> ModelTokenUsage {
    ModelTokenUsage {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
    }
}

pub(crate) fn build_chat_request_body(
    model: &str,
    messages: &[ProviderMessage],
    tools: &[ToolSchema],
) -> Value {
    let mut body = json!({
        "model": model,
        "messages": messages.iter().map(project_message).collect::<Vec<_>>(),
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": AGENT_MAX_TOKENS,
        "temperature": AGENT_TEMPERATURE,
    });
    if !tools.is_empty() {
        body["tools"] = json!(tools.iter().map(project_tool).collect::<Vec<_>>());
    }
    body
}

fn project_message(message: &ProviderMessage) -> Value {
    match message {
        ProviderMessage::System { content } => {
            json!({ "role": "system", "content": content })
        }
        ProviderMessage::User { content } => json!({ "role": "user", "content": content }),
        ProviderMessage::Assistant {
            content,
            tool_calls,
        } => {
            let projected_calls = tool_calls
                .iter()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            "arguments": serde_json::to_string(&call.arguments)
                                .unwrap_or_default(),
                        }
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "role": "assistant",
                "content": content,
                "tool_calls": projected_calls,
            })
        }
        ProviderMessage::Tool { result } => json!({
            "role": "tool",
            "tool_call_id": result.tool_call_id,
            "content": result.content,
        }),
    }
}

fn project_tool(tool: &ToolSchema) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}

fn agent_error(kind: AgentErrorKind, message: impl Into<String>) -> AgentError {
    AgentError::new(kind, message).expect("gateway 的固定错误消息必须有效")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::coordinator::Cancellation;
    use crate::agent_runtime::protocol::{ModelUsage, RunBudget, ToolResult};
    use crate::deepseek_client::{StreamChunkToolCallDelta, StreamChunkToolCallFunction};
    use std::sync::Mutex;

    fn tool_schema(name: &str) -> ToolSchema {
        ToolSchema {
            name: name.to_string(),
            description: format!("{name} 描述"),
            input_schema: json!({ "type": "object" }),
        }
    }

    #[test]
    fn request_body_projects_messages_and_tools() {
        let call = ToolCall {
            id: "call-1".into(),
            name: "get_date".into(),
            arguments: json!({}),
        };
        let result = ToolResult::success(
            &call,
            "2026-08-17",
            crate::agent_runtime::protocol::ToolProvenance::LocalFact,
            1,
            2,
        );
        let messages = vec![
            ProviderMessage::System {
                content: "system".into(),
            },
            ProviderMessage::User {
                content: "hello".into(),
            },
            ProviderMessage::Assistant {
                content: "checking".into(),
                tool_calls: vec![call.clone()],
            },
            ProviderMessage::Tool { result },
        ];
        let body =
            build_chat_request_body("deepseek-v4-flash", &messages, &[tool_schema("get_date")]);

        assert_eq!(body["model"], "deepseek-v4-flash");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["max_tokens"], AGENT_MAX_TOKENS);
        assert_eq!(body["temperature"], AGENT_TEMPERATURE);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        let assistant = &body["messages"][2];
        assert_eq!(assistant["role"], "assistant");
        assert_eq!(assistant["tool_calls"][0]["id"], "call-1");
        assert_eq!(assistant["tool_calls"][0]["function"]["name"], "get_date");
        assert_eq!(assistant["tool_calls"][0]["function"]["arguments"], "{}");
        assert_eq!(body["messages"][3]["role"], "tool");
        assert_eq!(body["messages"][3]["tool_call_id"], "call-1");
        assert_eq!(body["messages"][3]["content"], "2026-08-17");
        assert_eq!(body["tools"][0]["function"]["name"], "get_date");
        assert_eq!(body["tools"][0]["function"]["description"], "get_date 描述");
        assert_eq!(
            body["tools"][0]["function"]["parameters"],
            json!({"type": "object"})
        );
    }

    struct FakeStreamer {
        chunks: Vec<Result<StreamChunk, String>>,
        requests: Mutex<Vec<(String, Value)>>,
    }

    impl FakeStreamer {
        fn new(chunks: Vec<Result<StreamChunk, String>>) -> Self {
            Self {
                chunks,
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl ChatCompletionStreamer for FakeStreamer {
        fn request_stream(
            &self,
            operation: &str,
            request_body: &Value,
            _api_key: &str,
        ) -> BoxFuture<'static, Result<StreamChunkStream, String>> {
            self.requests
                .lock()
                .unwrap()
                .push((operation.to_string(), request_body.clone()));
            let chunks = self.chunks.clone();
            Box::pin(async move {
                Ok(Box::pin(futures_util::stream::iter(chunks)) as StreamChunkStream)
            })
        }
    }

    fn text_chunk(delta: &str) -> StreamChunk {
        StreamChunk {
            delta: delta.to_string(),
            finish_reason: None,
            usage: None,
            reasoning: None,
            tool_calls: None,
        }
    }

    fn finish_chunk(reason: &str, usage: Option<Value>) -> StreamChunk {
        StreamChunk {
            delta: String::new(),
            finish_reason: Some(reason.to_string()),
            usage,
            reasoning: None,
            tool_calls: None,
        }
    }

    fn run_gateway(
        streamer: FakeStreamer,
        cancel: Option<&Cancellation>,
    ) -> (
        Result<ModelTurnOutcome, AgentError>,
        Vec<ModelEvent>,
        std::sync::Arc<std::sync::Mutex<Option<ModelUsage>>>,
    ) {
        let recorded_usage = std::sync::Arc::new(std::sync::Mutex::new(None));
        let recorded_for_closure = recorded_usage.clone();
        let mut gateway = DeepSeekChatGateway::with_streamer(
            "deepseek-v4-flash",
            Box::new(streamer),
            Box::new(move |usage| {
                *recorded_for_closure.lock().unwrap() = Some(usage.clone());
                Ok(())
            }),
        );
        let request = ModelRequest {
            messages: vec![ProviderMessage::User {
                content: "hi".into(),
            }],
            tools: vec![tool_schema("get_date")],
            budget: RunBudget::first_version(),
            deadline_unix_ms: now_unix_ms() + 180_000,
            cancellation: cancel.cloned().unwrap_or_default(),
        };
        let mut events = Vec::new();
        let outcome = gateway.stream_model(request, &mut |event| {
            events.push(event.clone());
            Ok(())
        });
        (outcome, events, recorded_usage)
    }

    #[test]
    fn final_text_stream_produces_text_usage_and_completed() {
        let streamer = FakeStreamer::new(vec![
            Ok(text_chunk("final ")),
            Ok(text_chunk("answer")),
            Ok(finish_chunk(
                "stop",
                Some(json!({
                    "prompt_tokens": 10,
                    "completion_tokens": 2,
                    "total_tokens": 12
                })),
            )),
        ]);
        let (outcome, events, recorded_usage) = run_gateway(streamer, None);

        let outcome = outcome.expect("gateway 必须成功返回");
        assert!(!outcome.aborted);
        assert!(matches!(
            events.as_slice(),
            [
                ModelEvent::TextDelta { .. },
                ModelEvent::TextDelta { .. },
                ModelEvent::Usage { .. },
                ModelEvent::Completed { .. }
            ]
        ));
        assert_eq!(
            recorded_usage
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .total_tokens,
            12
        );
    }

    #[test]
    fn tool_call_stream_folds_argument_deltas_into_one_call() {
        let streamer = FakeStreamer::new(vec![
            Ok(StreamChunk {
                delta: String::new(),
                finish_reason: None,
                usage: None,
                reasoning: None,
                tool_calls: Some(vec![StreamChunkToolCallDelta {
                    index: 0,
                    id: Some("call_1".into()),
                    function: Some(StreamChunkToolCallFunction {
                        name: Some("get_date".into()),
                        arguments: Some("{\"timezone\":\"Asia".into()),
                    }),
                }]),
            }),
            Ok(StreamChunk {
                delta: String::new(),
                finish_reason: None,
                usage: None,
                reasoning: None,
                tool_calls: Some(vec![StreamChunkToolCallDelta {
                    index: 0,
                    id: None,
                    function: Some(StreamChunkToolCallFunction {
                        name: None,
                        arguments: Some("/Shanghai\"}".into()),
                    }),
                }]),
            }),
            Ok(finish_chunk("tool_calls", None)),
        ]);
        let (outcome, events, _) = run_gateway(streamer, None);

        let outcome = outcome.expect("gateway 必须成功返回");
        assert!(!outcome.aborted);
        let mut calls = Vec::new();
        for event in &events {
            if let ModelEvent::ToolCall { call } = event {
                calls.push(call.clone());
            }
        }
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "get_date");
        assert_eq!(calls[0].arguments, json!({"timezone": "Asia/Shanghai"}));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelEvent::Completed {
                reason: ModelFinishReason::ToolCalls
            }
        )));
    }

    #[test]
    fn cancellation_stops_streaming_with_aborted_outcome() {
        let cancellation = Cancellation::new();
        let cancel_for_stream = cancellation.clone();
        let streamer = FakeStreamer::new(vec![
            Ok(text_chunk("partial")),
            Ok(text_chunk("after cancel")),
        ]);
        let recorded_usage = std::sync::Arc::new(std::sync::Mutex::new(None));
        let recorded_for_closure = recorded_usage.clone();
        let mut gateway = DeepSeekChatGateway::with_streamer(
            "deepseek-v4-flash",
            Box::new(streamer),
            Box::new(move |usage| {
                *recorded_for_closure.lock().unwrap() = Some(usage.clone());
                Ok(())
            }),
        );
        let request = ModelRequest {
            messages: vec![ProviderMessage::User {
                content: "hi".into(),
            }],
            tools: vec![],
            budget: RunBudget::first_version(),
            deadline_unix_ms: now_unix_ms() + 180_000,
            cancellation: cancellation.clone(),
        };
        let mut events = Vec::new();
        let outcome = gateway.stream_model(request, &mut |event| {
            events.push(event.clone());
            if matches!(event, ModelEvent::TextDelta { text } if text == "partial") {
                cancel_for_stream.request();
            }
            Ok(())
        });
        let outcome = outcome.expect("取消是正常收尾");
        assert!(outcome.aborted);
        assert_eq!(events.len(), 1, "取消后不再继续输出事件");
    }

    #[test]
    fn malformed_usage_does_not_kill_the_run() {
        let streamer = FakeStreamer::new(vec![
            Ok(text_chunk("answer")),
            Ok(finish_chunk(
                "stop",
                Some(json!({"prompt_tokens": 1, "completion_tokens": 2})),
            )),
        ]);
        let (outcome, events, recorded_usage) = run_gateway(streamer, None);

        // "存在但格式非法"的 usage（缺 total_tokens）只记日志，不终止 run。
        let outcome = outcome.expect("usage 解析失败不得终止 run");
        assert!(!outcome.aborted);
        assert!(events
            .iter()
            .any(|event| matches!(event, ModelEvent::Completed { .. })));
        assert!(!events
            .iter()
            .any(|event| matches!(event, ModelEvent::Usage { .. })));
        assert!(recorded_usage.lock().unwrap().is_none());
    }

    #[test]
    fn stream_errors_map_to_provider_network() {
        let streamer = FakeStreamer::new(vec![Err("connection reset".to_string())]);
        let (outcome, _, _) = run_gateway(streamer, None);
        let error = outcome.expect_err("流错误必须映射为 AgentError");
        assert_eq!(error.kind, AgentErrorKind::ProviderNetwork);
    }

    #[test]
    fn malformed_tool_call_arguments_is_provider_protocol_error() {
        let streamer = FakeStreamer::new(vec![
            Ok(StreamChunk {
                delta: String::new(),
                finish_reason: None,
                usage: None,
                reasoning: None,
                tool_calls: Some(vec![StreamChunkToolCallDelta {
                    index: 0,
                    id: Some("call_1".into()),
                    function: Some(StreamChunkToolCallFunction {
                        name: Some("get_date".into()),
                        arguments: Some("{broken".into()),
                    }),
                }]),
            }),
            Ok(finish_chunk("tool_calls", None)),
        ]);
        let (outcome, _, _) = run_gateway(streamer, None);
        let error = outcome.expect_err("坏参数增量必须报协议错误");
        assert_eq!(error.kind, AgentErrorKind::ProviderProtocolError);
    }
}
