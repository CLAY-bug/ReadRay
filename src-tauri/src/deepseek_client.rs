use crate::model_usage::{record_for_app, ModelTokenUsage, ModelUsageCategory};
use crate::{secret_store, DEEPSEEK_BASE_URL, DEFAULT_DEEPSEEK_MODEL};
use futures_util::StreamExt;
use reqwest::Response;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::future::Future;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// DeepSeek 请求整体超时：从请求发出到响应读取完成。
/// 流式长回答（8K tokens 上限）实测一般 30-90s，180s 留足余量，
/// 同时兜底"连接建立后永不返回"的挂起场景。
const REQUEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(180);

/// 流式响应空闲超时：读取到每个 chunk 后重置；连续 60s 无数据则中断。
/// 防止 DeepSeek 流不关闭或网络半开导致 UI 永久停在"正在生成"。
const STREAM_READ_TIMEOUT: Duration = Duration::from_secs(60);

static SHARED_HTTP_CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();

pub(crate) fn shared_http_client() -> Result<&'static reqwest::Client, String> {
    match SHARED_HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(REQUEST_TOTAL_TIMEOUT)
            .read_timeout(STREAM_READ_TIMEOUT)
            .build()
            .map_err(|error| format!("ReadRay HTTP 客户端创建失败：{error}"))
    }) {
        Ok(client) => Ok(client),
        Err(error) => Err(error.clone()),
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChatCompletionRequestPolicy {
    total_timeout: Duration,
    max_transient_retries: u8,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum TrackedChatCompletionError {
    Cancelled,
    Failed(String),
}

impl ChatCompletionRequestPolicy {
    pub(crate) const fn new(total_timeout: Duration, max_transient_retries: u8) -> Self {
        Self {
            total_timeout,
            max_transient_retries,
        }
    }
}

#[derive(Debug)]
enum ChatCompletionRequestError {
    Timeout,
    Network,
    Http { status_code: u16 },
    InvalidJson,
    ClientConfiguration,
}

impl ChatCompletionRequestError {
    fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::Network
                | Self::Http {
                    status_code: 408 | 429 | 500..=599
                }
        )
    }

    fn product_message(&self, operation: &str) -> String {
        match self {
            Self::Timeout => format!("{operation} 超时：请重试。"),
            Self::Network => format!("{operation} 网络错误：连接或响应读取失败。"),
            Self::Http { status_code } => {
                format!("{operation} 网络错误：服务返回 HTTP {status_code}。")
            }
            Self::InvalidJson => {
                format!("{operation} 模型输出错误：响应不是合法 JSON。")
            }
            Self::ClientConfiguration => {
                format!("{operation} 配置错误：HTTP 客户端无法初始化。")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StreamChunk {
    pub delta: String,
    pub finish_reason: Option<String>,
    pub usage: Option<Value>,
    /// 推理模型（deepseek-v4-flash）的 `delta.reasoning_content`，仅捕获供
    /// 调用方验证丢弃，绝不转发给 UI。
    pub reasoning: Option<String>,
    /// OpenAI 格式的 `delta.tool_calls` 增量（按 index 归属；id/name 只在首个
    /// 分片出现，arguments 为跨分片拼接的 JSON 片段）。
    pub tool_calls: Option<Vec<StreamChunkToolCallDelta>>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct StreamChunkToolCallDelta {
    #[serde(default)]
    pub index: usize,
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<StreamChunkToolCallFunction>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct StreamChunkToolCallFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

pub(crate) async fn stream_chat_completion_events(
    operation: &str,
    request_body: &Value,
    api_key: &str,
) -> Result<futures_util::stream::BoxStream<'static, Result<StreamChunk, String>>, String> {
    let response = shared_http_client()?
        .post(format!("{DEEPSEEK_BASE_URL}/chat/completions"))
        .bearer_auth(api_key)
        .json(request_body)
        .send()
        .await
        .map_err(|error| format!("{operation} 请求失败：{error}"))?;

    let status = response.status();
    if !status.is_success() {
        let status_code = status.as_u16();
        let value: Value = response.json().await.unwrap_or_else(|_| json!({}));
        let message = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("DeepSeek API 返回非成功状态。");
        return Err(format!(
            "{operation} 请求返回 HTTP {status_code}：{message}"
        ));
    }

    Ok(parse_sse_stream(
        operation.to_string().into(),
        Box::pin(response.bytes_stream()),
    ))
}

#[derive(Debug, Deserialize)]
struct StreamChunkLine {
    choices: Option<Vec<StreamChunkChoice>>,
    usage: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct StreamChunkChoice {
    #[serde(default)]
    delta: StreamChunkDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct StreamChunkDelta {
    #[serde(default)]
    content: Option<String>,
    /// deepseek-v4-flash 推理过程增量；解析后只由调用方捕获验证、丢弃。
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamChunkToolCallDelta>>,
}

fn parse_sse_stream(
    operation: std::sync::Arc<str>,
    stream: futures_util::stream::BoxStream<'static, Result<bytes::Bytes, reqwest::Error>>,
) -> futures_util::stream::BoxStream<'static, Result<StreamChunk, String>> {
    struct SseState {
        stream: futures_util::stream::BoxStream<'static, Result<bytes::Bytes, reqwest::Error>>,
        pending: bytes::BytesMut,
        lines: std::collections::VecDeque<String>,
        stream_finished: bool,
    }

    futures_util::stream::unfold(
        SseState {
            stream,
            pending: bytes::BytesMut::new(),
            lines: std::collections::VecDeque::new(),
            stream_finished: false,
        },
        move |mut state| {
            let operation = operation.clone();
            async move {
                loop {
                    if let Some(line) = state.lines.pop_front() {
                        match parse_sse_line(&operation, &line) {
                            Ok(Some(chunk)) => return Some((Ok(chunk), state)),
                            Ok(None) => continue,
                            Err(error) => return Some((Err(error), state)),
                        }
                    }
                    if state.stream_finished {
                        return None;
                    }
                    match state.stream.next().await {
                        Some(Ok(bytes)) => {
                            state.pending.extend_from_slice(&bytes);
                            for line in split_sse_lines(&mut state.pending) {
                                state.lines.push_back(line);
                            }
                        }
                        Some(Err(error)) => {
                            return Some((
                                Err(format!("{operation} 流式响应读取失败：{error}")),
                                state,
                            ));
                        }
                        None => {
                            state.stream_finished = true;
                            if !state.pending.is_empty() {
                                state.pending.clear();
                                return Some((
                                    Err(format!("{operation} 流式响应在 [DONE] 之前结束。")),
                                    state,
                                ));
                            }
                        }
                    }
                }
            }
        },
    )
    .boxed()
}

fn split_sse_lines(pending: &mut bytes::BytesMut) -> Vec<String> {
    let mut lines = Vec::new();
    let bytes = std::mem::take(pending);
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            lines.push(
                std::str::from_utf8(&bytes[start..index])
                    .unwrap_or("")
                    .trim_end_matches('\r')
                    .to_string(),
            );
            start = index + 1;
        }
        index += 1;
    }
    if start < bytes.len() {
        pending.extend_from_slice(&bytes[start..]);
    }
    lines
}

fn parse_sse_line(operation: &str, line: &str) -> Result<Option<StreamChunk>, String> {
    if line.is_empty() {
        return Ok(None);
    }
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(None);
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(None);
    }
    let chunk: StreamChunkLine = serde_json::from_str(data)
        .map_err(|error| format!("{operation} 流式响应块无法解析：{error}"))?;
    let choice = match chunk.choices {
        None => None,
        Some(mut choices) => {
            if choices.len() > 1 {
                return Err(format!("{operation} 流式响应包含多个 choices。"));
            }
            choices.pop()
        }
    };
    let delta = choice
        .as_ref()
        .and_then(|choice| choice.delta.content.clone())
        .unwrap_or_default();
    let reasoning = choice
        .as_ref()
        .and_then(|choice| choice.delta.reasoning_content.clone())
        .unwrap_or_default();
    let reasoning = (!reasoning.is_empty()).then_some(reasoning);
    let tool_calls = choice
        .as_ref()
        .and_then(|choice| choice.delta.tool_calls.clone())
        .filter(|calls| !calls.is_empty());
    let finish_reason = choice.and_then(|choice| choice.finish_reason);
    let usage = chunk.usage.filter(|usage| !usage.is_null());
    if delta.is_empty()
        && reasoning.is_none()
        && tool_calls.is_none()
        && finish_reason.is_none()
        && usage.is_none()
    {
        return Ok(None);
    }
    Ok(Some(StreamChunk {
        delta,
        finish_reason,
        usage,
        reasoning,
        tool_calls,
    }))
}

pub(crate) fn configured_model() -> String {
    std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| DEFAULT_DEEPSEEK_MODEL.to_string())
}

pub(crate) async fn post_tracked_chat_completion<T>(
    app: &AppHandle,
    category: ModelUsageCategory,
    operation: &str,
    request_body: &Value,
) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let api_key = secret_store::deepseek_api_key_state()?
        .into_key()
        .ok_or_else(|| format!("未配置 DeepSeek API Key，无法执行 {operation}。"))?;

    let value = send_chat_completion_value(operation, request_body, &api_key).await?;
    decode_tracked_chat_completion_value(operation, value, |usage| {
        record_for_app(app, category, usage)
    })
}

pub(crate) async fn post_tracked_chat_completion_with_policy_and_checkpoint<T, Check>(
    app: &AppHandle,
    category: ModelUsageCategory,
    operation: &str,
    request_body: &Value,
    policy: ChatCompletionRequestPolicy,
    is_current: Check,
) -> Result<T, TrackedChatCompletionError>
where
    T: DeserializeOwned,
    Check: Fn() -> bool,
{
    if !is_current() {
        return Err(TrackedChatCompletionError::Cancelled);
    }
    let api_key = secret_store::deepseek_api_key_state()
        .map_err(|_| {
            TrackedChatCompletionError::Failed(format!(
                "{operation} 配置错误：无法读取 DeepSeek API Key。"
            ))
        })?
        .into_key()
        .ok_or_else(|| {
            TrackedChatCompletionError::Failed(format!(
                "{operation} 配置错误：未配置 DeepSeek API Key。"
            ))
        })?;

    let value = send_chat_completion_value_with_policy(request_body, &api_key, policy)
        .await
        .map_err(|error| TrackedChatCompletionError::Failed(error.product_message(operation)))?;
    decode_tracked_chat_completion_value_with_checkpoint(
        operation,
        value,
        |usage| record_for_app(app, category, usage),
        is_current,
    )
}

#[cfg(test)]
pub(crate) async fn post_chat_completion_for_test<T>(
    operation: &str,
    request_body: &Value,
) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let api_key = secret_store::deepseek_api_key_state()?
        .into_key()
        .ok_or_else(|| format!("未配置 DeepSeek API Key，无法执行 {operation}。"))?;
    let value = send_chat_completion_value(operation, request_body, &api_key).await?;
    deserialize_deepseek_response(operation, value)
}

pub(crate) async fn post_chat_completion_with_api_key<T>(
    operation: &str,
    request_body: &Value,
    api_key: &str,
) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let value = send_chat_completion_value(operation, request_body, api_key).await?;
    deserialize_deepseek_response(operation, value)
}

async fn send_chat_completion_value(
    operation: &str,
    request_body: &Value,
    api_key: &str,
) -> Result<Value, String> {
    let response = shared_http_client()?
        .post(format!("{DEEPSEEK_BASE_URL}/chat/completions"))
        .bearer_auth(api_key)
        .json(request_body)
        .send()
        .await
        .map_err(|error| format!("{operation} 请求失败：{error}"))?;

    decode_deepseek_response_value(operation, response).await
}

async fn send_chat_completion_value_with_policy(
    request_body: &Value,
    api_key: &str,
    policy: ChatCompletionRequestPolicy,
) -> Result<Value, ChatCompletionRequestError> {
    run_with_request_policy(policy, |remaining_timeout| {
        send_chat_completion_value_once(request_body, api_key, remaining_timeout)
    })
    .await
}

async fn run_with_request_policy<T, Operation, OperationFuture>(
    policy: ChatCompletionRequestPolicy,
    mut operation: Operation,
) -> Result<T, ChatCompletionRequestError>
where
    Operation: FnMut(Duration) -> OperationFuture,
    OperationFuture: Future<Output = Result<T, ChatCompletionRequestError>>,
{
    let started_at = Instant::now();
    let mut retries = 0_u8;

    loop {
        let remaining_timeout = policy
            .total_timeout
            .checked_sub(started_at.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(ChatCompletionRequestError::Timeout)?;
        match operation(remaining_timeout).await {
            Ok(value) => return Ok(value),
            Err(error) if error.is_transient() && retries < policy.max_transient_retries => {
                retries += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn send_chat_completion_value_once(
    request_body: &Value,
    api_key: &str,
    timeout: Duration,
) -> Result<Value, ChatCompletionRequestError> {
    let client =
        shared_http_client().map_err(|_| ChatCompletionRequestError::ClientConfiguration)?;
    let response = client
        .post(format!("{DEEPSEEK_BASE_URL}/chat/completions"))
        .bearer_auth(api_key)
        .json(request_body)
        .timeout(timeout)
        .send()
        .await
        .map_err(classify_reqwest_error)?;

    decode_deepseek_response_value_for_policy(response).await
}

fn classify_reqwest_error(error: reqwest::Error) -> ChatCompletionRequestError {
    if error.is_timeout() {
        ChatCompletionRequestError::Timeout
    } else if error.is_decode() {
        ChatCompletionRequestError::InvalidJson
    } else {
        ChatCompletionRequestError::Network
    }
}

async fn decode_deepseek_response_value_for_policy(
    response: Response,
) -> Result<Value, ChatCompletionRequestError> {
    let status = response.status();
    if !status.is_success() {
        return Err(ChatCompletionRequestError::Http {
            status_code: status.as_u16(),
        });
    }

    response.json().await.map_err(classify_reqwest_error)
}

pub(crate) async fn get_deepseek_json<T>(operation: &str, path: &str) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let api_key = secret_store::deepseek_api_key_state()?
        .into_key()
        .ok_or_else(|| format!("未配置 DeepSeek API Key，无法执行 {operation}。"))?;

    let response = shared_http_client()?
        .get(format!("{DEEPSEEK_BASE_URL}{path}"))
        .header("Accept", "application/json")
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|error| format!("{operation} 请求失败：{error}"))?;

    let value = decode_deepseek_response_value(operation, response).await?;
    deserialize_deepseek_response(operation, value)
}

async fn decode_deepseek_response_value(
    operation: &str,
    response: Response,
) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        let status_code = status.as_u16();
        let value: Value = response.json().await.unwrap_or_else(|_| json!({}));
        let message = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .or_else(|| value.get("error").and_then(Value::as_str))
            .or_else(|| value.get("message").and_then(Value::as_str))
            .unwrap_or("DeepSeek API 返回非成功状态。");
        return Err(format!(
            "{operation} 请求返回 HTTP {status_code}：{message}"
        ));
    }

    response
        .json()
        .await
        .map_err(|error| format!("{operation} 响应结构无法解析：{error}"))
}

fn deserialize_deepseek_response<T>(operation: &str, value: Value) -> Result<T, String>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|error| format!("{operation} 响应结构无法解析：{error}"))
}

#[derive(Debug, Deserialize)]
struct DeepSeekUsageFields {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

pub(crate) fn parse_model_token_usage(value: &Value) -> Result<ModelTokenUsage, String> {
    let usage_value = value
        .get("usage")
        .cloned()
        .ok_or_else(|| "DeepSeek 模型响应缺少 usage。".to_string())?;
    parse_model_token_usage_value(&usage_value)
}

pub(crate) fn parse_model_token_usage_value(value: &Value) -> Result<ModelTokenUsage, String> {
    let usage: DeepSeekUsageFields = serde_json::from_value(value.clone())
        .map_err(|error| format!("DeepSeek 模型响应 usage 结构无效：{error}"))?;
    let expected_total = usage
        .prompt_tokens
        .checked_add(usage.completion_tokens)
        .ok_or_else(|| "DeepSeek 模型响应 usage Token 数溢出。".to_string())?;
    if usage.total_tokens != expected_total {
        return Err("DeepSeek 模型响应 usage 总数与输入、输出 Token 不一致。".to_string());
    }
    Ok(ModelTokenUsage {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
    })
}

fn decode_tracked_chat_completion_value<T, Record>(
    operation: &str,
    value: Value,
    record_usage: Record,
) -> Result<T, String>
where
    T: DeserializeOwned,
    Record: FnOnce(ModelTokenUsage) -> Result<(), String>,
{
    let usage = parse_model_token_usage(&value)?;
    let _ = record_usage(usage);
    deserialize_deepseek_response(operation, value)
}

fn decode_tracked_chat_completion_value_with_checkpoint<T, Record, Check>(
    operation: &str,
    value: Value,
    record_usage: Record,
    is_current: Check,
) -> Result<T, TrackedChatCompletionError>
where
    T: DeserializeOwned,
    Record: FnOnce(ModelTokenUsage) -> Result<(), String>,
    Check: Fn() -> bool,
{
    if !is_current() {
        return Err(TrackedChatCompletionError::Cancelled);
    }
    let response =
        decode_tracked_chat_completion_value(operation, value, record_usage).map_err(|_| {
            TrackedChatCompletionError::Failed(format!(
                "{operation} 模型输出错误：响应结构不符合协议。"
            ))
        })?;
    if !is_current() {
        return Err(TrackedChatCompletionError::Cancelled);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct TestResponse {
        answer: String,
    }

    fn response(answer: Value, usage: Value) -> Value {
        json!({ "answer": answer, "usage": usage })
    }

    #[test]
    fn records_valid_usage_before_business_response_validation() {
        let recorded = Cell::new(None);
        let result = decode_tracked_chat_completion_value::<TestResponse, _>(
            "测试模型调用",
            response(
                json!(42),
                json!({
                    "prompt_tokens": 12,
                    "completion_tokens": 8,
                    "total_tokens": 20,
                    "prompt_cache_hit_tokens": 4
                }),
            ),
            |usage| {
                recorded.set(Some(usage));
                Ok(())
            },
        );

        assert!(result.is_err());
        assert_eq!(
            recorded.get(),
            Some(ModelTokenUsage {
                prompt_tokens: 12,
                completion_tokens: 8,
                total_tokens: 20,
            })
        );
    }

    #[test]
    fn usage_write_failure_does_not_fail_valid_model_response() {
        let result = decode_tracked_chat_completion_value::<TestResponse, _>(
            "测试模型调用",
            response(
                json!("ok"),
                json!({
                    "prompt_tokens": 3,
                    "completion_tokens": 2,
                    "total_tokens": 5
                }),
            ),
            |_| Err("database locked".to_string()),
        )
        .unwrap();

        assert_eq!(result.answer, "ok");
    }

    #[test]
    fn rejects_missing_negative_or_inconsistent_usage() {
        assert!(parse_model_token_usage(&json!({ "answer": "ok" })).is_err());
        assert!(parse_model_token_usage(&json!({
            "usage": {
                "prompt_tokens": -1,
                "completion_tokens": 2,
                "total_tokens": 1
            }
        }))
        .is_err());
        assert!(parse_model_token_usage(&json!({
            "usage": {
                "prompt_tokens": 3,
                "completion_tokens": 2,
                "total_tokens": 6
            }
        }))
        .is_err());
    }

    #[test]
    fn parses_usage_value_from_streaming_final_chunk_shape() {
        // 流式最终 chunk 的 usage 是 usage 对象本身（choices 为空数组，usage 非 null），
        // 与完整响应体（外层带 "usage" 键）形状不同，必须走 *_value 入口。
        let usage = json!({
            "prompt_tokens": 6,
            "completion_tokens": 12,
            "total_tokens": 18,
            "prompt_tokens_details": { "cached_tokens": 0 },
            "prompt_cache_hit_tokens": 0,
            "prompt_cache_miss_tokens": 6
        });
        let parsed = parse_model_token_usage_value(&usage).unwrap();
        assert_eq!(parsed.prompt_tokens, 6);
        assert_eq!(parsed.completion_tokens, 12);
        assert_eq!(parsed.total_tokens, 18);
    }

    #[test]
    fn rejects_inconsistent_or_malformed_streaming_usage_value() {
        let inconsistent = json!({
            "prompt_tokens": 6,
            "completion_tokens": 12,
            "total_tokens": 99
        });
        let error = parse_model_token_usage_value(&inconsistent).unwrap_err();
        assert!(error.contains("不一致"));

        let missing_fields = json!({ "prompt_tokens": 6 });
        assert!(parse_model_token_usage_value(&missing_fields).is_err());
        assert!(parse_model_token_usage_value(&json!(null)).is_err());
    }

    #[test]
    fn splits_sse_lines_across_partial_buffers() {
        let mut pending = bytes::BytesMut::new();
        pending.extend_from_slice(b"data: {\"a\":1}\r\ndata: {\"b\"");
        let mut first = split_sse_lines(&mut pending);
        assert_eq!(first.pop(), Some("data: {\"a\":1}".to_string()));
        assert_eq!(pending.as_ref(), b"data: {\"b\"");

        pending.extend_from_slice(b":2}\r\ndata: [DONE]\r\n");
        let mut second = split_sse_lines(&mut pending);
        assert_eq!(second.pop(), Some("data: [DONE]".to_string()));
        assert_eq!(second.pop(), Some("data: {\"b\":2}".to_string()));
        assert!(pending.is_empty());
    }

    #[test]
    fn parses_delta_finish_reason_and_usage_from_sse_lines() {
        let chunk = parse_sse_line(
            "测试模型调用",
            r#"data: {"choices":[{"delta":{"content":"Hello","role":"assistant"},"finish_reason":null}]}"#,
        )
        .unwrap()
        .expect("data line must yield a chunk");
        assert_eq!(chunk.delta, "Hello");
        assert_eq!(chunk.finish_reason, None);
        assert!(chunk.usage.is_none());
        assert_eq!(chunk.reasoning, None);

        let usage = json!({"prompt_tokens": 17, "completion_tokens": 9, "total_tokens": 26});
        let final_chunk = parse_sse_line(
            "测试模型调用",
            &format!(
                r#"data: {{"choices":[{{"delta":{{"content":"","role":null}},"finish_reason":"stop"}}],"usage":{usage}}}"#
            ),
        )
        .unwrap()
        .expect("final data line must yield a chunk");
        assert_eq!(final_chunk.delta, "");
        assert_eq!(final_chunk.finish_reason.as_deref(), Some("stop"));
        assert_eq!(final_chunk.usage, Some(usage));
        assert_eq!(final_chunk.reasoning, None);
    }

    #[test]
    fn captures_reasoning_content_without_folding_it_into_delta() {
        // deepseek-v4-flash 推理增量在 delta.reasoning_content，与 content 分离；
        // 捕获后由调用方验证丢弃，不得混入回答文本。
        let chunk = parse_sse_line(
            "测试模型调用",
            r#"data: {"choices":[{"delta":{"content":"Answer","role":"assistant","reasoning_content":"thinking"}}]}"#,
        )
        .unwrap()
        .expect("data line must yield a chunk");
        assert_eq!(chunk.delta, "Answer");
        assert_eq!(chunk.reasoning.as_deref(), Some("thinking"));

        // 只含推理、没有正文的 chunk 也必须产出，让调用方能统计"纯推理零内容"。
        let reasoning_only = parse_sse_line(
            "测试模型调用",
            r#"data: {"choices":[{"delta":{"reasoning_content":"thinking only"}}]}"#,
        )
        .unwrap()
        .expect("reasoning-only line must yield a chunk");
        assert_eq!(reasoning_only.delta, "");
        assert_eq!(reasoning_only.reasoning.as_deref(), Some("thinking only"));
    }

    #[test]
    fn parses_tool_call_argument_deltas_across_chunks() {
        // 首个分片带 id/name，后续分片只带 arguments 增量。
        let first = parse_sse_line(
            "测试模型调用",
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_date","arguments":""}}]},"finish_reason":null}]}"#,
        )
        .unwrap()
        .expect("tool call 首分片必须产出 chunk");
        let tool_calls = first.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].index, 0);
        assert_eq!(tool_calls[0].id.as_deref(), Some("call_1"));
        let function = tool_calls[0].function.as_ref().unwrap();
        assert_eq!(function.name.as_deref(), Some("get_date"));
        assert_eq!(function.arguments.as_deref(), Some(""));

        let second = parse_sse_line(
            "测试模型调用",
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"timezone\":\"Asia"}}]},"finish_reason":null}]}"#,
        )
        .unwrap()
        .expect("tool call 参数增量必须产出 chunk");
        let second_calls = second.tool_calls.as_ref().unwrap();
        assert_eq!(second_calls[0].id, None);
        let second_function = second_calls[0].function.as_ref().unwrap();
        assert_eq!(second_function.name, None);
        assert_eq!(
            second_function.arguments.as_deref(),
            Some("{\"timezone\":\"Asia")
        );

        // 空 arguments 分片也会产出（tool_calls 存在即产出，不能丢弃）。
        let empty_arguments = parse_sse_line(
            "测试模型调用",
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":""}}]},"finish_reason":null}]}"#,
        )
        .unwrap()
        .expect("空 arguments 增量也必须产出 chunk");
        assert!(empty_arguments.tool_calls.is_some());
        assert!(empty_arguments.delta.is_empty());

        // 纯文本 chunk 不含 tool_calls。
        let text_chunk = parse_sse_line(
            "测试模型调用",
            r#"data: {"choices":[{"delta":{"content":"ok"},"finish_reason":null}]}"#,
        )
        .unwrap()
        .unwrap();
        assert!(text_chunk.tool_calls.is_none());
    }

    #[test]
    fn sse_line_handling_is_lenient_toward_events_and_whitespace() {
        assert!(parse_sse_line("测试模型调用", "").unwrap().is_none());
        assert!(parse_sse_line("测试模型调用", "event: message")
            .unwrap()
            .is_none());
        assert!(parse_sse_line("测试模型调用", "data: [DONE]")
            .unwrap()
            .is_none());
        assert!(parse_sse_line("测试模型调用", "data: [DONE] ")
            .unwrap()
            .is_none());

        let chunk = parse_sse_line(
            "测试模型调用",
            r#"data: {"choices":[{"delta":{"content":"X"},"finish_reason":null}],"usage":null}"#,
        )
        .unwrap()
        .expect("null usage must be ignored");
        assert_eq!(chunk.delta, "X");
        assert!(chunk.usage.is_none());
    }

    #[test]
    fn sse_line_with_multiple_choices_or_bad_json_is_rejected() {
        let error = parse_sse_line(
            "测试模型调用",
            r#"data: {"choices":[{"delta":{"content":"A"}},{"delta":{"content":"B"}}]}"#,
        )
        .unwrap_err();
        assert!(error.contains("多个 choices"));
        let error = parse_sse_line("测试模型调用", "data: {not-json").unwrap_err();
        assert!(error.contains("无法解析"));
    }

    #[test]
    fn sse_usage_only_chunk_yields_chunk_without_choice() {
        let usage = json!({"prompt_tokens": 17, "completion_tokens": 9, "total_tokens": 26});
        let chunk = parse_sse_line(
            "测试模型调用",
            &format!(r#"data: {{"choices":[],"usage":{usage}}}"#),
        )
        .unwrap()
        .expect("usage-only data line must yield a chunk");
        assert_eq!(chunk.delta, "");
        assert_eq!(chunk.finish_reason, None);
        assert_eq!(chunk.usage, Some(usage));
    }

    #[test]
    fn shared_client_is_a_process_singleton_with_existing_timeouts() {
        // 流式链路必须有整体超时 + 流空闲超时（builder 设置，见 shared_http_client），
        // 防止"一直生成中"。reqwest Client 不暴露超时 getter，
        // 此处同时验证所有调用取得同一个进程级实例，不会重建连接池。
        let first = shared_http_client().unwrap();
        let second = shared_http_client().unwrap();
        assert!(std::ptr::eq(first, second));
        assert_eq!(REQUEST_TOTAL_TIMEOUT.as_secs(), 180);
        assert_eq!(STREAM_READ_TIMEOUT.as_secs(), 60);
    }

    #[test]
    fn request_policy_retries_one_transient_failure_then_stops() {
        let attempts = Cell::new(0_u8);
        let largest_timeout = Cell::new(Duration::ZERO);
        let result = tauri::async_runtime::block_on(run_with_request_policy(
            ChatCompletionRequestPolicy::new(Duration::from_secs(10), 1),
            |remaining_timeout| {
                attempts.set(attempts.get() + 1);
                largest_timeout.set(largest_timeout.get().max(remaining_timeout));
                std::future::ready(if attempts.get() == 1 {
                    Err(ChatCompletionRequestError::Http { status_code: 429 })
                } else {
                    Ok("ok")
                })
            },
        ))
        .unwrap();

        assert_eq!(result, "ok");
        assert_eq!(attempts.get(), 2);
        assert!(largest_timeout.get() <= Duration::from_secs(10));

        let failed_attempts = Cell::new(0_u8);
        let error = tauri::async_runtime::block_on(run_with_request_policy::<(), _, _>(
            ChatCompletionRequestPolicy::new(Duration::from_secs(10), 1),
            |_| {
                failed_attempts.set(failed_attempts.get() + 1);
                std::future::ready(Err(ChatCompletionRequestError::Network))
            },
        ))
        .unwrap_err();
        assert!(matches!(error, ChatCompletionRequestError::Network));
        assert_eq!(failed_attempts.get(), 2);
    }

    #[test]
    fn request_policy_does_not_retry_4xx_or_invalid_json() {
        let client_error_attempts = Cell::new(0_u8);
        let client_error = tauri::async_runtime::block_on(run_with_request_policy::<(), _, _>(
            ChatCompletionRequestPolicy::new(Duration::from_secs(10), 1),
            |_| {
                client_error_attempts.set(client_error_attempts.get() + 1);
                std::future::ready(Err(ChatCompletionRequestError::Http { status_code: 400 }))
            },
        ))
        .unwrap_err();
        assert!(matches!(
            client_error,
            ChatCompletionRequestError::Http { status_code: 400 }
        ));
        assert_eq!(client_error_attempts.get(), 1);

        let invalid_json_attempts = Cell::new(0_u8);
        let invalid_json = tauri::async_runtime::block_on(run_with_request_policy::<(), _, _>(
            ChatCompletionRequestPolicy::new(Duration::from_secs(10), 1),
            |_| {
                invalid_json_attempts.set(invalid_json_attempts.get() + 1);
                std::future::ready(Err(ChatCompletionRequestError::InvalidJson))
            },
        ))
        .unwrap_err();
        assert!(matches!(
            invalid_json,
            ChatCompletionRequestError::InvalidJson
        ));
        assert_eq!(invalid_json_attempts.get(), 1);
    }

    #[test]
    fn retry_records_usage_only_after_the_single_successful_response() {
        let attempts = Cell::new(0_u8);
        let value = tauri::async_runtime::block_on(run_with_request_policy(
            ChatCompletionRequestPolicy::new(Duration::from_secs(10), 1),
            |_| {
                attempts.set(attempts.get() + 1);
                std::future::ready(if attempts.get() == 1 {
                    Err(ChatCompletionRequestError::Http { status_code: 500 })
                } else {
                    Ok(response(
                        json!("ok"),
                        json!({
                            "prompt_tokens": 3,
                            "completion_tokens": 2,
                            "total_tokens": 5
                        }),
                    ))
                })
            },
        ))
        .unwrap();
        let usage_writes = Cell::new(0_u8);
        let decoded = decode_tracked_chat_completion_value::<TestResponse, _>(
            "测试模型调用",
            value,
            |_| {
                usage_writes.set(usage_writes.get() + 1);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(decoded.answer, "ok");
        assert_eq!(attempts.get(), 2);
        assert_eq!(usage_writes.get(), 1);
    }

    #[test]
    fn checkpoint_rejects_a_response_before_usage_processing() {
        let current = Cell::new(false);
        let usage_writes = Cell::new(0_u8);
        let value = response(
            json!("ok"),
            json!({
                "prompt_tokens": 3,
                "completion_tokens": 2,
                "total_tokens": 5
            }),
        );

        let result = decode_tracked_chat_completion_value_with_checkpoint::<TestResponse, _, _>(
            "测试模型调用",
            value,
            |_| {
                usage_writes.set(usage_writes.get() + 1);
                Ok(())
            },
            || current.get(),
        );

        assert_eq!(result.unwrap_err(), TrackedChatCompletionError::Cancelled);
        assert_eq!(usage_writes.get(), 0);
    }

    #[test]
    fn checkpoint_rejects_a_response_cancelled_during_usage_processing() {
        let current = Cell::new(true);
        let usage_writes = Cell::new(0_u8);
        let result = decode_tracked_chat_completion_value_with_checkpoint::<TestResponse, _, _>(
            "测试模型调用",
            response(
                json!("ok"),
                json!({
                    "prompt_tokens": 3,
                    "completion_tokens": 2,
                    "total_tokens": 5
                }),
            ),
            |_| {
                usage_writes.set(usage_writes.get() + 1);
                current.set(false);
                Ok(())
            },
            || current.get(),
        );

        assert_eq!(result.unwrap_err(), TrackedChatCompletionError::Cancelled);
        assert_eq!(usage_writes.get(), 1);
    }
}
