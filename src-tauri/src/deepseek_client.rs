use crate::model_usage::{record_for_app, ModelTokenUsage, ModelUsageCategory};
use crate::{secret_store, DEEPSEEK_BASE_URL, DEFAULT_DEEPSEEK_MODEL};
use reqwest::Response;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::AppHandle;

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
    let response = reqwest::Client::new()
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

    let response = reqwest::Client::new()
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

fn parse_model_token_usage(value: &Value) -> Result<ModelTokenUsage, String> {
    let usage_value = value
        .get("usage")
        .cloned()
        .ok_or_else(|| "DeepSeek 模型响应缺少 usage。".to_string())?;
    let usage: DeepSeekUsageFields = serde_json::from_value(usage_value)
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
}
