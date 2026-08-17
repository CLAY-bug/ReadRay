//! DeepSeek Responses API 的独立 live spike 与响应夹具。
//!
//! 当前 DeepSeek Responses API 的公开形状仍可能变化，因此请求只在显式环境开关、
//! \`#[ignore]\` 测试中执行；离线 parser fixture 始终可运行。该文件不创建 AppHandle、
//! 不打开 SQLite，也不记录或打印 API key。

use super::protocol::{AgentError, AgentErrorKind, ModelUsage, SourceMetadata};
use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepseek_client::{configured_model, shared_http_client};
    use crate::secret_store;
    use crate::DEEPSEEK_BASE_URL;
    use futures_util::StreamExt;
    use serde_json::json;
    use std::path::PathBuf;

    const LIVE_SPIKE_FLAG: &str = "READRAY_RUN_DEEPSEEK_RESPONSES_SPIKE";

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ResponseTerminal {
        Completed,
        Incomplete,
        Failed,
    }

    #[derive(Debug, Default, PartialEq)]
    struct ResponsesObservation {
        event_types: Vec<String>,
        response_ids: Vec<String>,
        text: String,
        function_calls: Vec<String>,
        web_search_actions: Vec<String>,
        usages: Vec<ModelUsage>,
        sources: Vec<SourceMetadata>,
        terminal: Option<ResponseTerminal>,
        invalid_response_id: bool,
    }

    fn parse_sse_observation(input: &str) -> Result<ResponsesObservation, String> {
        let mut observation = ResponsesObservation::default();
        let mut previous_sequence = None;
        let mut event_count = 0_u64;
        let mut terminal_index = None;
        let mut pending_event_name = None;
        for (line_index, line) in input.lines().enumerate() {
            if let Some(event_name) = line.strip_prefix("event:") {
                if pending_event_name.is_some() {
                    return Err(format!(
                        "SSE event field was not followed by data at line {}.",
                        line_index + 1
                    ));
                }
                let event_name = event_name.trim();
                if event_name.is_empty() {
                    return Err(format!(
                        "SSE event field is empty at line {}.",
                        line_index + 1
                    ));
                }
                pending_event_name = Some(event_name.to_string());
                continue;
            }

            let Some(data) = line.strip_prefix("data:") else {
                if line.trim().is_empty() || line.starts_with(':') {
                    if pending_event_name.is_some() && line.trim().is_empty() {
                        return Err(format!(
                            "SSE event field was not followed by data at line {}.",
                            line_index + 1
                        ));
                    }
                    continue;
                }
                return Err(format!(
                    "SSE line must contain event: or data: at line {}.",
                    line_index + 1
                ));
            };
            let data = data.trim();
            if data.is_empty() {
                return Err(format!(
                    "SSE data field is empty at line {}.",
                    line_index + 1
                ));
            }
            if data == "[DONE]" {
                if pending_event_name.is_some() {
                    return Err(format!(
                        "SSE [DONE] must not have an event: field at line {}.",
                        line_index + 1
                    ));
                }
                if observation.terminal.is_none() {
                    return Err(format!(
                        "SSE [DONE] appeared before terminal event at line {}.",
                        line_index + 1
                    ));
                }
                continue;
            }
            let event_name = pending_event_name.take().ok_or_else(|| {
                format!(
                    "SSE data event is missing a preceding event: field at line {}.",
                    line_index + 1
                )
            })?;
            let value = serde_json::from_str::<Value>(data)
                .map_err(|_| format!("SSE event JSON is invalid at line {}.", line_index + 1))?;
            let object = value.as_object().ok_or_else(|| {
                format!(
                    "SSE event must be a JSON object at line {}.",
                    line_index + 1
                )
            })?;
            let sequence_number = object
                .get("sequence_number")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    format!(
                        "SSE event sequence_number is missing at line {}.",
                        line_index + 1
                    )
                })?;
            if previous_sequence.is_some_and(|previous| sequence_number <= previous) {
                return Err(format!(
                    "SSE sequence_number is not increasing at line {}.",
                    line_index + 1
                ));
            }
            previous_sequence = Some(sequence_number);
            let event_type = object
                .get("type")
                .and_then(Value::as_str)
                .filter(|event_type| !event_type.trim().is_empty())
                .ok_or_else(|| format!("SSE event type is missing at line {}.", line_index + 1))?;
            if event_name != event_type {
                return Err(format!(
                    "SSE event/data type mismatch at line {}.",
                    line_index + 1
                ));
            }
            if let Some(terminal) = response_terminal(event_type) {
                if terminal_index.is_some() {
                    return Err("SSE response has more than one terminal event.".to_string());
                }
                terminal_index = Some(event_count);
                observation.terminal = Some(terminal);
            } else if terminal_index.is_some() {
                return Err("SSE event appeared after terminal response event.".to_string());
            }
            event_count += 1;
            observe_value(&value, &mut observation);
        }
        if pending_event_name.is_some() {
            return Err("SSE stream ended with an event: field without data.".to_string());
        }
        if observation.invalid_response_id {
            return Err(
                "SSE response id must be a non-empty string of reasonable length.".to_string(),
            );
        }
        if event_count == 0 {
            return Err("SSE stream contained no JSON events.".to_string());
        }
        if observation.terminal.is_none() || terminal_index != Some(event_count - 1) {
            return Err(
                "SSE stream ended without a final response.completed/incomplete/failed event."
                    .to_string(),
            );
        }
        Ok(observation)
    }

    fn response_terminal(event_type: &str) -> Option<ResponseTerminal> {
        match event_type {
            "response.completed" => Some(ResponseTerminal::Completed),
            "response.incomplete" => Some(ResponseTerminal::Incomplete),
            "response.failed" => Some(ResponseTerminal::Failed),
            _ => None,
        }
    }

    fn terminal_error(terminal: ResponseTerminal) -> Option<AgentError> {
        match terminal {
            ResponseTerminal::Completed => None,
            ResponseTerminal::Incomplete => Some(
                AgentError::new(
                    AgentErrorKind::ProviderProtocolError,
                    "DeepSeek Responses returned response.incomplete; no final answer is publishable.",
                )
                .expect("fixed terminal classification is valid"),
            ),
            ResponseTerminal::Failed => Some(
                AgentError::new(
                    AgentErrorKind::ProviderProtocolError,
                    "DeepSeek Responses returned response.failed.",
                )
                .expect("fixed terminal classification is valid"),
            ),
        }
    }

    fn require_completed(
        label: &str,
        observation: &ResponsesObservation,
    ) -> Result<(), AgentError> {
        match observation.terminal {
            Some(ResponseTerminal::Completed) => Ok(()),
            Some(terminal) => {
                Err(terminal_error(terminal).expect("non-completed terminal has error"))
            }
            None => Err(AgentError::new(
                AgentErrorKind::ProviderProtocolError,
                format!("{label} Responses stream has no terminal event."),
            )
            .expect("fixed terminal classification is valid")),
        }
    }

    fn observe_value(value: &Value, observation: &mut ResponsesObservation) {
        let Some(object) = value.as_object() else {
            if let Some(values) = value.as_array() {
                for value in values {
                    observe_value(value, observation);
                }
            }
            return;
        };

        let event_type = object.get("type").and_then(Value::as_str);
        if let Some(event_type) = event_type {
            observation.event_types.push(event_type.to_string());
        }
        if let Some(id) = object.get("response_id").and_then(Value::as_str) {
            record_response_id(id, observation);
        }
        if let Some(id) = object
            .get("response")
            .and_then(Value::as_object)
            .and_then(|response| response.get("id"))
            .and_then(Value::as_str)
        {
            record_response_id(id, observation);
        }
        if event_type == Some("response.created") {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                record_response_id(id, observation);
            }
        }
        if event_type.is_some_and(|event| event.contains("output_text")) {
            if let Some(delta) = object.get("delta").and_then(Value::as_str) {
                observation.text.push_str(delta);
            }
        }
        if event_type.is_some_and(|event| event.contains("function_call")) {
            let call_id = object
                .get("call_id")
                .or_else(|| object.get("id"))
                .and_then(Value::as_str);
            let name = object.get("name").and_then(Value::as_str);
            if let (Some(call_id), Some(name)) = (call_id, name) {
                let call = format!("{call_id}:{name}");
                if !observation.function_calls.contains(&call) {
                    observation.function_calls.push(call);
                }
            }
        }
        if event_type.is_some_and(|event| event.contains("web_search")) {
            let action_id = object
                .get("id")
                .or_else(|| object.get("call_id"))
                .and_then(Value::as_str)
                .unwrap_or(event_type.unwrap_or("web_search"));
            if !observation
                .web_search_actions
                .iter()
                .any(|known| known == action_id)
            {
                observation.web_search_actions.push(action_id.to_string());
            }
        }
        if let Some(usage) = object.get("usage").and_then(parse_usage) {
            if !observation.usages.contains(&usage) {
                observation.usages.push(usage);
            }
        }
        if let Some(annotation) = object.get("annotation") {
            record_annotation(annotation, observation);
        }
        if let Some(annotations) = object.get("annotations").and_then(Value::as_array) {
            for annotation in annotations {
                record_annotation(annotation, observation);
            }
        }

        for (key, child) in object {
            // Event type 已在上面消费；其余递归仍保留，兼容 response.output/source 嵌套。
            if key != "type" {
                observe_value(child, observation);
            }
        }
    }

    fn record_response_id(id: &str, observation: &mut ResponsesObservation) {
        if !is_valid_response_id(id) {
            observation.invalid_response_id = true;
        } else if !observation.response_ids.iter().any(|known| known == id) {
            observation.response_ids.push(id.to_string());
        }
    }

    fn is_valid_response_id(id: &str) -> bool {
        !id.trim().is_empty()
            && id.len() <= 256
            && !id
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
    }

    fn record_annotation(value: &Value, observation: &mut ResponsesObservation) {
        let Some(object) = value.as_object() else {
            return;
        };
        let annotation_type = object.get("type").and_then(Value::as_str).unwrap_or("");
        if !matches!(annotation_type, "url_citation" | "citation") {
            return;
        }
        if let Some(source) = source_metadata(object) {
            if !observation
                .sources
                .iter()
                .any(|known| known.url == source.url)
            {
                observation.sources.push(source);
            }
        }
    }

    fn parse_usage(value: &Value) -> Option<ModelUsage> {
        let prompt_tokens = value
            .get("prompt_tokens")
            .or_else(|| value.get("input_tokens"))
            .and_then(Value::as_u64)?;
        let completion_tokens = value
            .get("completion_tokens")
            .or_else(|| value.get("output_tokens"))
            .and_then(Value::as_u64)?;
        let total_tokens = value.get("total_tokens").and_then(Value::as_u64)?;
        let usage = ModelUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        };
        usage.validate().ok().map(|_| usage)
    }

    fn source_metadata(object: &serde_json::Map<String, Value>) -> Option<SourceMetadata> {
        let url = object.get("url").and_then(Value::as_str)?;
        let title = object
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("DeepSeek source")
            .to_string();
        let source = SourceMetadata {
            source_id: stable_source_id(url),
            title,
            url: url.to_string(),
            site_name: object
                .get("site_name")
                .or_else(|| object.get("site"))
                .and_then(Value::as_str)
                .map(str::to_string),
            published_at: object
                .get("published_at")
                .and_then(Value::as_str)
                .map(str::to_string),
            retrieved_at_unix_ms: 0,
            content_type: object
                .get("content_type")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        source.validate().ok().map(|_| source)
    }

    fn stable_source_id(url: &str) -> String {
        // 四路独立 FNV-1a 变体避免把 URL 本身或其前缀暴露到 source_id；不新增 hash 依赖。
        const OFFSET_BASIS: [u64; 4] = [
            0xcbf29ce484222325,
            0x84222325cbf29ce4,
            0x9e3779b185ebca87,
            0xd6e8feb86659fd93,
        ];
        const PRIME: u64 = 0x00000100000001b3;
        let mut lanes = OFFSET_BASIS;
        for (index, byte) in url.bytes().enumerate() {
            for (lane_index, lane) in lanes.iter_mut().enumerate() {
                let salt = (lane_index as u64 + 1).wrapping_mul(0x9e3779b9);
                *lane ^= u64::from(byte)
                    .wrapping_add((index as u64).rotate_left(lane_index as u32))
                    ^ salt;
                *lane = lane.wrapping_mul(PRIME);
            }
        }
        format!(
            "deepseek-source-{0:016x}{1:016x}{2:016x}{3:016x}",
            lanes[0], lanes[1], lanes[2], lanes[3]
        )
    }

    fn function_request_body(model: &str) -> Value {
        json!({
            "model": model,
            "input": "Call the protocol probe function exactly once with echo=responses-spike.",
            "tools": [{
                "type": "function",
                "name": "readray_protocol_probe",
                "description": "Return the fixed string supplied in echo. This is a harmless live spike.",
                "parameters": {
                    "type": "object",
                    "properties": { "echo": { "type": "string" } },
                    "required": ["echo"],
                    "additionalProperties": false
                }
            }],
            "tool_choice": { "type": "function", "name": "readray_protocol_probe" },
            "stream": true
        })
    }

    fn web_search_request_body(model: &str) -> Value {
        json!({
            "model": model,
            "input": "Search the public web for the current Rust language release. Return a short answer with source URLs.",
            "tools": [{ "type": "web_search" }],
            "stream": true
        })
    }

    /// chat/completions 端点的 server-side web search 请求形状（OpenAI 兼容）。
    /// 任务 3 provider 决策：Responses 端点 400 后，唯一能 live 确认内置
    /// web_search 来源能力的途径；与 Responses spike 共用同一显式开关。
    fn web_search_chat_request_body(model: &str) -> Value {
        json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": "Search the public web for the current Rust language release and report the version together with its source URLs."
            }],
            "tools": [{ "type": "web_search" }],
            "stream": true,
            "stream_options": { "include_usage": true }
        })
    }

    #[derive(Debug, Default, PartialEq)]
    struct ChatObservation {
        text: String,
        usages: Vec<ModelUsage>,
        citations: Vec<SourceMetadata>,
        finish_reason: Option<String>,
    }

    /// OpenAI 兼容 SSE（`data:` 单行 JSON + 末尾 `[DONE]`）的宽松观察器。
    /// 只收集决策所需事实：文本、usage、finish_reason 与顶层 citations。
    fn parse_chat_sse_observation(input: &str) -> Result<ChatObservation, String> {
        let mut observation = ChatObservation::default();
        let mut done_seen = false;
        for (line_index, line) in input.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let Some(data) = line.strip_prefix("data:") else {
                return Err(format!(
                    "chat SSE line must start with data: at line {}.",
                    line_index + 1
                ));
            };
            let data = data.trim();
            if data == "[DONE]" {
                done_seen = true;
                break;
            }
            let value = serde_json::from_str::<Value>(data).map_err(|_| {
                format!("chat SSE event JSON is invalid at line {}.", line_index + 1)
            })?;
            let Some(object) = value.as_object() else {
                return Err(format!(
                    "chat SSE event must be a JSON object at line {}.",
                    line_index + 1
                ));
            };
            if let Some(choice) = object
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(Value::as_object)
            {
                if let Some(delta) = choice
                    .get("delta")
                    .and_then(Value::as_object)
                    .and_then(|delta| delta.get("content"))
                    .and_then(Value::as_str)
                {
                    observation.text.push_str(delta);
                }
                if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                    observation.finish_reason = Some(reason.to_string());
                }
            }
            if let Some(usage) = object.get("usage").and_then(parse_usage) {
                if !observation.usages.contains(&usage) {
                    observation.usages.push(usage);
                }
            }
            if let Some(citations) = object.get("citations").and_then(Value::as_array) {
                for citation in citations {
                    if let Some(source) = source_metadata_from_citation(citation) {
                        if !observation
                            .citations
                            .iter()
                            .any(|known| known.url == source.url)
                        {
                            observation.citations.push(source);
                        }
                    }
                }
            }
        }
        if !done_seen {
            return Err("chat SSE stream ended without [DONE].".to_string());
        }
        Ok(observation)
    }

    /// 把 OpenAI 兼容 citation 元素投影为 SourceMetadata；字段缺失时回退，
    /// 协议级校验失败（含敏感查询参数/凭据）的条目整体跳过。
    fn source_metadata_from_citation(value: &Value) -> Option<SourceMetadata> {
        let object = value.as_object()?;
        let url = object.get("url").and_then(Value::as_str)?;
        let title = object
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or("Web source")
            .to_string();
        let source = SourceMetadata {
            source_id: stable_source_id(url),
            title,
            url: url.to_string(),
            site_name: object
                .get("site_name")
                .or_else(|| object.get("site"))
                .and_then(Value::as_str)
                .map(str::to_string),
            published_at: object
                .get("published_at")
                .and_then(Value::as_str)
                .map(str::to_string),
            retrieved_at_unix_ms: 0,
            content_type: None,
        };
        source.validate().ok().map(|_| source)
    }

    async fn send_streaming_spike_request(
        endpoint: &str,
        api_key: &str,
        request_body: &Value,
    ) -> Result<String, String> {
        let response = shared_http_client()?
            .post(endpoint)
            .bearer_auth(api_key)
            .json(request_body)
            .send()
            .await
            .map_err(|_| "Responses spike request failed before HTTP response.".to_string())?;
        let status = response.status();
        if !status.is_success() {
            // 只提取错误分类与截断后的 message（请求体不含凭据，message 最多
            // 400 字符，用于区分端点/请求形状/账户作用域原因），不打印完整 body。
            let classification = response
                .bytes()
                .await
                .ok()
                .and_then(|bytes| {
                    serde_json::from_str::<Value>(&String::from_utf8_lossy(&bytes)).ok()
                })
                .and_then(|value| value.get("error").cloned())
                .map(|error| {
                    let code = error.get("code").and_then(Value::as_str).unwrap_or("?");
                    let kind = error.get("type").and_then(Value::as_str).unwrap_or("?");
                    let message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .chars()
                        .take(400)
                        .collect::<String>();
                    format!("code={code} type={kind} message={message}")
                })
                .unwrap_or_default();
            return Err(format!(
                "Responses spike returned HTTP {} {classification}",
                status.as_u16()
            ));
        }
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "Responses spike stream failed.".to_string())?;
            if body.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
                return Err("Responses spike stream exceeded 4 MiB observation cap".to_string());
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).map_err(|_| "Responses spike stream was not UTF-8.".to_string())
    }

    #[test]
    fn responses_fixture_exposes_function_web_search_action_and_usage_without_source_claim() {
        let fixture = r#"
event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"provider-response-001"}}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":1,"item":{"type":"function_call","call_id":"call_fixture_1","name":"readray_protocol_probe","arguments":"{\"echo\":\"responses-spike\"}"}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","sequence_number":2,"delta":"Rust release: "}

event: response.output_text.delta
data: {"type":"response.output_text.delta","sequence_number":3,"delta":"source unknown"}

event: response.output_item.added
data: {"type":"response.output_item.added","sequence_number":4,"item":{"type":"web_search_call","id":"web_search_fixture_1","status":"in_progress"}}

event: response.completed
data: {"type":"response.completed","sequence_number":5,"response":{"usage":{"input_tokens":12,"output_tokens":4,"total_tokens":16}}}
"#;
        let observation = parse_sse_observation(fixture).unwrap();
        assert!(observation
            .event_types
            .iter()
            .any(|event| event == "response.output_item.added"));
        assert_eq!(observation.response_ids, vec!["provider-response-001"]);
        assert_eq!(
            observation.function_calls,
            vec!["call_fixture_1:readray_protocol_probe"]
        );
        assert_eq!(observation.web_search_actions, vec!["web_search_fixture_1"]);
        assert_eq!(observation.text, "Rust release: source unknown");
        assert_eq!(observation.usages[0].total_tokens, 16);
        assert!(observation.sources.is_empty());
        assert_eq!(observation.terminal, Some(ResponseTerminal::Completed));
    }

    #[test]
    fn responses_fixture_records_explicit_url_citation_annotation_if_present() {
        let fixture = r#"
event: response.output_text.annotation.added
data: {"type":"response.output_text.annotation.added","sequence_number":0,"annotation":{"type":"url_citation","url":"https://www.rust-lang.org/learn/get-started","title":"Rust Releases"}}
event: response.completed
data: {"type":"response.completed","sequence_number":1,"response":{"usage":{"input_tokens":2,"output_tokens":1,"total_tokens":3}}}
"#;
        let observation = parse_sse_observation(fixture).unwrap();
        assert_eq!(observation.sources.len(), 1);
        assert_eq!(
            observation.sources[0].url,
            "https://www.rust-lang.org/learn/get-started"
        );
    }

    #[test]
    fn responses_sse_parser_rejects_bad_json_sequence_and_incomplete_streams() {
        let invalid_json = "event: response.created\ndata: {not-json}\n";
        assert!(parse_sse_observation(invalid_json).is_err());
        let non_monotonic = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"sequence_number\":1}\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"sequence_number\":1}\n",
        );
        assert!(parse_sse_observation(non_monotonic).is_err());
        let missing_terminal =
            "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0}\n";
        assert!(parse_sse_observation(missing_terminal).is_err());
        let terminal_then_event = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"sequence_number\":0}\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"delta\":\"late\"}\n",
        );
        assert!(parse_sse_observation(terminal_then_event).is_err());
    }

    #[test]
    fn responses_sse_parser_rejects_missing_or_mismatched_event_field() {
        let missing_event = "data: {\"type\":\"response.completed\",\"sequence_number\":0}\n";
        assert!(parse_sse_observation(missing_event).is_err());
        let mismatched_event = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.completed\",\"sequence_number\":0}\n",
        );
        assert!(parse_sse_observation(mismatched_event).is_err());
        let missing_type = concat!(
            "event: response.completed\n",
            "data: {\"sequence_number\":0}\n",
        );
        assert!(parse_sse_observation(missing_type).is_err());
        let invalid_response_id = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\" \"}}\n",
        );
        assert!(parse_sse_observation(invalid_response_id).is_err());
    }

    #[test]
    fn responses_sse_parser_accepts_each_provider_terminal_shape() {
        for terminal in [
            ("response.completed", ResponseTerminal::Completed),
            ("response.incomplete", ResponseTerminal::Incomplete),
            ("response.failed", ResponseTerminal::Failed),
        ] {
            let fixture = format!(
                "event: response.created\ndata: {{\"type\":\"response.created\",\"sequence_number\":0}}\n\
event: {}\ndata: {{\"type\":\"{}\",\"sequence_number\":1}}\n",
                terminal.0,
                terminal.0
            );
            let observation = parse_sse_observation(&fixture).unwrap();
            assert_eq!(observation.terminal, Some(terminal.1));
        }
    }

    #[test]
    fn responses_failed_is_rejected_and_incomplete_is_provider_protocol_error() {
        assert!(terminal_error(ResponseTerminal::Completed).is_none());
        assert_eq!(
            terminal_error(ResponseTerminal::Incomplete).unwrap().kind,
            AgentErrorKind::ProviderProtocolError
        );
        assert_eq!(
            terminal_error(ResponseTerminal::Failed).unwrap().kind,
            AgentErrorKind::ProviderProtocolError
        );
    }

    #[test]
    fn response_source_ids_are_stable_and_parser_never_keeps_bearer_data() {
        let first_value = json!({
            "title": "Example",
            "url": "https://example.com/a"
        });
        let second_value = json!({
            "title": "Example renamed",
            "url": "https://example.com/a"
        });
        let first = source_metadata(first_value.as_object().unwrap()).unwrap();
        let second = source_metadata(second_value.as_object().unwrap()).unwrap();
        assert_eq!(first.source_id, second.source_id);
        assert!(!first.source_id.contains("Bearer"));
        assert!(!first.source_id.contains("sk-"));
        assert!(!first.source_id.contains("example"));

        let long_prefix = "https://example.com/".to_string() + &"a".repeat(1_900);
        let first_long = source_metadata(
            json!({
                "title": "Long first",
                "url": format!("{long_prefix}1")
            })
            .as_object()
            .unwrap(),
        )
        .unwrap();
        let second_long = source_metadata(
            json!({
                "title": "Long second",
                "url": format!("{long_prefix}2")
            })
            .as_object()
            .unwrap(),
        )
        .unwrap();
        assert_ne!(first_long.source_id, second_long.source_id);
        assert!(first_long.source_id.starts_with("deepseek-source-"));
    }

    fn load_project_env_for_live_test() {
        let env_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|project_root| project_root.join(".env"));
        if let Some(env_path) = env_path {
            let _ = dotenvy::from_path_override(env_path);
        }
    }

    #[test]
    #[ignore = "requires READRAY_RUN_DEEPSEEK_RESPONSES_SPIKE=1, DEEPSEEK_API_KEY and network access"]
    fn live_deepseek_responses_function_and_web_search_observability() {
        if std::env::var(LIVE_SPIKE_FLAG).ok().as_deref() != Some("1") {
            eprintln!("Responses live spike skipped; set {LIVE_SPIKE_FLAG}=1 explicitly.");
            return;
        }
        load_project_env_for_live_test();
        // 2026-08-17 实测：.env 修复后请求已真实发出，但 POST {base}/responses 返回 HTTP 400，
        // 错误 body 按设计不读取，端点/请求体/作用域原因与 web_search 来源可观测性仍未确认。
        let api_key = secret_store::deepseek_api_key_state()
            .expect("DeepSeek key state should be readable")
            .into_key()
            .expect("DeepSeek API key is required for explicit live spike");
        let endpoint = std::env::var("DEEPSEEK_RESPONSES_ENDPOINT")
            .unwrap_or_else(|_| format!("{DEEPSEEK_BASE_URL}/responses"));
        let model = configured_model();
        let function_body = function_request_body(&model);
        let web_search_body = web_search_request_body(&model);
        let (function_stream, search_stream) = tauri::async_runtime::block_on(async {
            let function_stream =
                send_streaming_spike_request(&endpoint, &api_key, &function_body).await?;
            let search_stream =
                send_streaming_spike_request(&endpoint, &api_key, &web_search_body).await?;
            Ok::<_, String>((function_stream, search_stream))
        })
        .expect("DeepSeek Responses live spike should return two streams");

        let function_observation = parse_sse_observation(&function_stream)
            .expect("function Responses stream must satisfy the spike protocol");
        let search_observation = parse_sse_observation(&search_stream)
            .expect("web_search Responses stream must satisfy the spike protocol");
        require_completed("function", &function_observation)
            .unwrap_or_else(|error| panic!("function Responses terminal rejected: {error:?}"));
        require_completed("web_search", &search_observation)
            .unwrap_or_else(|error| panic!("web_search Responses terminal rejected: {error:?}"));
        assert!(
            !function_observation.function_calls.is_empty(),
            "function tool event was not observable; event types: {:?}",
            function_observation.event_types
        );
        assert!(
            !function_observation.usages.is_empty(),
            "function usage was not observable; event types: {:?}",
            function_observation.event_types
        );
        assert!(
            !search_observation.web_search_actions.is_empty(),
            "web_search action was not observable; event types: {:?}",
            search_observation.event_types
        );
        assert!(
            !search_observation.usages.is_empty(),
            "web_search usage was not observable; event types: {:?}",
            search_observation.event_types
        );
        // DeepSeek 官方是否返回可映射的 citation/annotation 来源由 live 结果决定；
        // 离线 fixture 不把自造 sources 字段当作来源证据。
    }

    #[test]
    fn chat_web_search_fixture_exposes_text_usage_and_citations() {
        let fixture = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Rust release: \"},\"finish_reason\":null}]}\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"1.88.0\"},\"finish_reason\":null}]}\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":4,\"total_tokens\":16},\"citations\":[{\"url\":\"https://blog.rust-lang.org/releases\",\"title\":\"Rust Releases\",\"index\":0},{\"url\":\"https://example.com/a\"}]}\n",
            "data: [DONE]\n",
        );
        let observation = parse_chat_sse_observation(fixture).unwrap();
        assert_eq!(observation.text, "Rust release: 1.88.0");
        assert_eq!(observation.finish_reason.as_deref(), Some("stop"));
        assert_eq!(observation.usages[0].total_tokens, 16);
        assert_eq!(observation.citations.len(), 2);
        assert_eq!(
            observation.citations[0].url,
            "https://blog.rust-lang.org/releases"
        );
        assert_eq!(observation.citations[0].title, "Rust Releases");
        assert_eq!(observation.citations[1].title, "Web source");
        assert!(!observation.citations[0]
            .source_id
            .contains("blog.rust-lang.org"));
    }

    #[test]
    fn chat_web_search_parser_rejects_bad_lines_and_missing_done() {
        let bad_json = "data: {not-json}\n";
        assert!(parse_chat_sse_observation(bad_json).is_err());
        let missing_done = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n";
        assert!(parse_chat_sse_observation(missing_done).is_err());
        let missing_data_prefix = "choices:[{\"delta\":{\"content\":\"x\"}}]\n";
        assert!(parse_chat_sse_observation(missing_data_prefix).is_err());
        // 含凭据查询参数的 citation 被整体拒绝，不会进入来源列表。
        let sensitive = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}],\"citations\":[{\"url\":\"https://example.com/page?api_key=redacted\",\"title\":\"Leak\"}]}\n",
            "data: [DONE]\n",
        );
        let observation = parse_chat_sse_observation(sensitive).unwrap();
        assert!(observation.citations.is_empty());
    }

    /// 非流式 chat/completions 响应（web_search 变体探测用）。
    fn parse_chat_completion_response(input: &str) -> Result<ChatObservation, String> {
        let value: Value = serde_json::from_str(input)
            .map_err(|_| "chat response JSON is invalid.".to_string())?;
        let mut observation = ChatObservation::default();
        if let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(Value::as_object)
        {
            if let Some(message) = choice.get("message").and_then(Value::as_object) {
                if let Some(content) = message.get("content").and_then(Value::as_str) {
                    observation.text.push_str(content);
                }
                if let Some(citations) = message.get("citations").and_then(Value::as_array) {
                    for citation in citations {
                        if let Some(source) = source_metadata_from_citation(citation) {
                            if !observation
                                .citations
                                .iter()
                                .any(|known| known.url == source.url)
                            {
                                observation.citations.push(source);
                            }
                        }
                    }
                }
            }
        }
        if let Some(usage) = value.get("usage").and_then(parse_usage) {
            observation.usages.push(usage);
        }
        Ok(observation)
    }

    /// 非流式 web_search 请求变体（探测响应形状；正式实现形状由 live 结果决定）。
    fn non_streaming_web_search_chat_request_body(model: &str) -> Value {
        json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": "Search the public web for the current Rust language release and report the version together with its source URLs."
            }],
            "tools": [{ "type": "web_search" }],
            "stream": false
        })
    }

    #[test]
    #[ignore = "requires READRAY_RUN_DEEPSEEK_RESPONSES_SPIKE=1, DEEPSEEK_API_KEY and network access"]
    fn live_deepseek_chat_web_search_observability() {
        if std::env::var(LIVE_SPIKE_FLAG).ok().as_deref() != Some("1") {
            eprintln!("Responses live spike skipped; set {LIVE_SPIKE_FLAG}=1 explicitly.");
            return;
        }
        load_project_env_for_live_test();
        let api_key = secret_store::deepseek_api_key_state()
            .expect("DeepSeek key state should be readable")
            .into_key()
            .expect("DeepSeek API key is required for explicit live spike");
        let endpoint = format!("{DEEPSEEK_BASE_URL}/chat/completions");
        let model = configured_model();
        // provider 决策证据：内置 web_search 必须返回真实 URL+标题的 citation，
        // 才能映射到 UI 来源卡片；探测失败时判定不满足来源要求。
        let variants: Vec<(&str, Value, bool)> = vec![
            ("stream", web_search_chat_request_body(&model), true),
            (
                "non-stream",
                non_streaming_web_search_chat_request_body(&model),
                false,
            ),
        ];
        for (label, request_body, streaming) in variants {
            let result = tauri::async_runtime::block_on(send_streaming_spike_request(
                &endpoint,
                &api_key,
                &request_body,
            ));
            let outcome = match result {
                Ok(raw) => {
                    let parsed = if streaming {
                        parse_chat_sse_observation(&raw)
                    } else {
                        parse_chat_completion_response(&raw)
                    };
                    match parsed {
                        Ok(observation) => format!(
                            "text={} citations={} urls={:?}",
                            observation.text.trim().len(),
                            observation.citations.len(),
                            observation
                                .citations
                                .iter()
                                .map(|source| source.url.as_str())
                                .collect::<Vec<_>>()
                        ),
                        Err(error) => format!("parse_error={error}"),
                    }
                }
                Err(error) => format!("http_error={error}"),
            };
            eprintln!("READRAY_WEB_SEARCH_VARIANT={label} {outcome}");
        }
    }
}
