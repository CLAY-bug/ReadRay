use crate::model_usage::{record_for_app, ModelTokenUsage, ModelUsageCategory};
use crate::{secret_store, DEEPSEEK_BASE_URL, DEFAULT_DEEPSEEK_MODEL};
use futures_util::StreamExt;
use reqwest::Response;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::AppHandle;

/// DeepSeek 请求整体超时：从请求发出到响应读取完成。
/// 流式长回答（8K tokens 上限）实测一般 30-90s，180s 留足余量，
/// 同时兜底"连接建立后永不返回"的挂起场景。
const REQUEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(180);

/// 流式响应空闲超时：读取到每个 chunk 后重置；连续 60s 无数据则中断。
/// 防止 DeepSeek 流不关闭或网络半开导致 UI 永久停在"正在生成"。
const STREAM_READ_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) fn shared_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(REQUEST_TOTAL_TIMEOUT)
        .read_timeout(STREAM_READ_TIMEOUT)
        .build()
        .map_err(|error| format!("ReadRay HTTP 客户端创建失败：{error}"))
}

#[derive(Debug)]
pub(crate) struct StreamChunk {
    pub delta: String,
    pub finish_reason: Option<String>,
    pub usage: Option<Value>,
    /// 推理模型（deepseek-v4-flash）的 `delta.reasoning_content`，仅捕获供
    /// 调用方验证丢弃，绝不转发给 UI。
    pub reasoning: Option<String>,
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
    let finish_reason = choice.and_then(|choice| choice.finish_reason);
    let usage = chunk.usage.filter(|usage| !usage.is_null());
    if delta.is_empty() && reasoning.is_none() && finish_reason.is_none() && usage.is_none() {
        return Ok(None);
    }
    Ok(Some(StreamChunk {
        delta,
        finish_reason,
        usage,
        reasoning,
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

pub(crate) async fn get_deepseek_json<T>(operation: &str, path: &str) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let api_key = secret_store::deepseek_api_key_state()?
        .into_key()
        .ok_or_else(|| format!("未配置 DeepSeek API Key，无法执行 {operation}。"))?;

    let response = shared_http_client()?
        .get(format!("{DEEPSEEK_BASE_URL}{path}"))
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
    fn shared_client_creates_successfully() {
        // 流式链路必须有整体超时 + 流空闲超时（builder 设置，见 shared_http_client），
        // 防止"一直生成中"。reqwest Client 不暴露超时 getter，
        // 此处验证共享客户端可创建（超时配置在构建时由 builder 生效）。
        assert!(shared_http_client().is_ok());
        assert_eq!(REQUEST_TOTAL_TIMEOUT.as_secs(), 180);
        assert_eq!(STREAM_READ_TIMEOUT.as_secs(), 60);
    }
}
