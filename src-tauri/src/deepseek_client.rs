use crate::{secret_store, DEEPSEEK_BASE_URL, DEFAULT_DEEPSEEK_MODEL};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

pub(crate) fn configured_model() -> String {
    std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| DEFAULT_DEEPSEEK_MODEL.to_string())
}

pub(crate) async fn post_chat_completion<T>(
    operation: &str,
    request_body: &Value,
) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let api_key = secret_store::deepseek_api_key_state()?
        .into_key()
        .ok_or_else(|| format!("未配置 DeepSeek API Key，无法执行 {operation}。"))?;

    post_chat_completion_with_api_key(operation, request_body, &api_key).await
}

pub(crate) async fn post_chat_completion_with_api_key<T>(
    operation: &str,
    request_body: &Value,
    api_key: &str,
) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let response = reqwest::Client::new()
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

    response
        .json()
        .await
        .map_err(|error| format!("{operation} 响应结构无法解析：{error}"))
}
