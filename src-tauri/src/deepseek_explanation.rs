use crate::explanation::{validate_explanation_card, CaptureInput, ExplanationCard};
use crate::{DEEPSEEK_BASE_URL, DEFAULT_DEEPSEEK_MODEL};
use serde::Deserialize;
use serde_json::json;

const EXPLANATION_CARD_MAX_TOKENS: u16 = 900;
const EXPLANATION_CARD_TEMPERATURE: f32 = 0.2;

#[derive(Debug, Deserialize)]
struct DeepSeekChatResponse {
    choices: Vec<DeepSeekChoice>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekChoice {
    finish_reason: Option<String>,
    message: DeepSeekMessage,
}

#[derive(Debug, Deserialize)]
struct DeepSeekMessage {
    content: Option<String>,
}

#[tauri::command]
pub async fn create_explanation_card(input: CaptureInput) -> Result<ExplanationCard, String> {
    let model =
        std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| DEFAULT_DEEPSEEK_MODEL.to_string());
    let api_key = match std::env::var("DEEPSEEK_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return Err("未设置 DEEPSEEK_API_KEY，无法创建 ExplanationCard。".to_string()),
    };

    let user_prompt = build_user_prompt(&input)?;
    let request_body = json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": explanation_card_system_prompt()
            },
            {
                "role": "user",
                "content": user_prompt
            }
        ],
        "response_format": {
            "type": "json_object"
        },
        "stream": false,
        "max_tokens": EXPLANATION_CARD_MAX_TOKENS,
        "temperature": EXPLANATION_CARD_TEMPERATURE
    });

    let response = reqwest::Client::new()
        .post(format!("{DEEPSEEK_BASE_URL}/chat/completions"))
        .bearer_auth(api_key)
        .json(&request_body)
        .send()
        .await
        .map_err(|error| format!("DeepSeek ExplanationCard 请求失败：{error}"))?;

    let status = response.status();
    if !status.is_success() {
        let status_code = status.as_u16();
        let value: serde_json::Value = response.json().await.unwrap_or_else(|_| json!({}));
        let message = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
            .unwrap_or("DeepSeek API 返回非成功状态。");
        return Err(format!(
            "DeepSeek ExplanationCard 请求返回 HTTP {status_code}：{message}"
        ));
    }

    let value: DeepSeekChatResponse = response
        .json()
        .await
        .map_err(|error| format!("DeepSeek ExplanationCard 响应结构无法解析：{error}"))?;
    let content = extract_content(value)?;

    parse_explanation_card_content(&input, &content)
}

pub(crate) fn parse_explanation_card_content(
    input: &CaptureInput,
    content: &str,
) -> Result<ExplanationCard, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("DeepSeek 返回空内容，无法解析 ExplanationCard JSON。".to_string());
    }

    let card: ExplanationCard = serde_json::from_str(content)
        .map_err(|error| format!("DeepSeek 返回内容不是合法 ExplanationCard JSON：{error}"))?;
    validate_explanation_card(input, &card).map_err(|errors| {
        format!(
            "ExplanationCard 校验失败：{}",
            summarize_validation_errors(&errors)
        )
    })?;

    Ok(card)
}

fn extract_content(response: DeepSeekChatResponse) -> Result<String, String> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| "DeepSeek ExplanationCard 响应缺少 choices[0]。".to_string())?;

    if let Some(finish_reason) = choice.finish_reason.as_deref() {
        if finish_reason != "stop" {
            return Err(format!(
                "DeepSeek ExplanationCard 生成未正常结束：finish_reason={finish_reason}。"
            ));
        }
    }

    choice
        .message
        .content
        .ok_or_else(|| "DeepSeek ExplanationCard 响应缺少 choices[0].message.content。".to_string())
}

fn build_user_prompt(input: &CaptureInput) -> Result<String, String> {
    let input_json = serde_json::to_string_pretty(input)
        .map_err(|error| format!("CaptureInput 无法序列化为 JSON：{error}"))?;

    Ok(format!(
        "Create one ExplanationCard JSON object for this CaptureInput.\n\nCaptureInput JSON:\n{input_json}"
    ))
}

fn explanation_card_system_prompt() -> &'static str {
    r#"You create structured JSON for ReadRay, a desktop English learning app.

Return exactly one JSON object. Do not return Markdown. Do not wrap the JSON in code fences. Do not add explanations before or after the JSON.

The JSON object must match this camelCase schema:
{
  "queryType": "word",
  "headword": "market",
  "phonetic": "/ˈmɑːrkɪt/",
  "basicMeaning": "市场；销售；推广",
  "contextMeaning": null,
  "phrases": [
    { "phrase": "market share", "meaning": "市场份额" }
  ],
  "nearMeanings": [
    { "term": "sell", "meaning": "强调卖出商品或服务" }
  ],
  "examples": [
    { "en": "The company entered a new market.", "zh": "这家公司进入了一个新市场。" }
  ],
  "difficulty": null,
  "reviewHint": "注意名词“市场”和动词“推广”的区别。"
}

Rules:
- queryType must be one of: word, phrase, sentence.
- Infer queryType from captureInput.queryText.
- headword and basicMeaning are required and must be non-empty.
- phonetic, contextMeaning, and reviewHint may be null if not useful.
- difficulty must be null for now because ReadRay has not defined its own learner difficulty rubric yet.
- If captureInput.contextText is missing, null, or blank, contextMeaning must be null or omitted.
- If captureInput.contextText is present, contextMeaning may be present but is not required.
- phrases must contain at most 3 items.
- nearMeanings must contain at most 3 items.
- examples must contain at least 1 item and at most 2 items.
- Each example must contain non-empty English en and Chinese zh.
- Use concise Chinese for meanings.
- Do not cite or pretend to quote any commercial dictionary or authoritative dictionary source.
- Keep the output compact enough for a small desktop popover."#
}

fn summarize_validation_errors(errors: &[String]) -> String {
    errors.join("；")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explanation::{QueryType, SourceType};
    use std::path::PathBuf;

    fn input_without_context() -> CaptureInput {
        CaptureInput {
            query_text: "market".to_string(),
            context_text: None,
            source_type: SourceType::Manual,
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

    fn valid_card_json() -> &'static str {
        r#"{
  "queryType": "word",
  "headword": "market",
  "phonetic": "/ˈmɑːrkɪt/",
  "basicMeaning": "市场；销售；推广",
  "phrases": [
    { "phrase": "market share", "meaning": "市场份额" }
  ],
  "nearMeanings": [
    { "term": "sell", "meaning": "强调卖出商品或服务" }
  ],
  "examples": [
    { "en": "The company entered a new market.", "zh": "这家公司进入了一个新市场。" }
  ],
  "difficulty": null,
  "reviewHint": "注意名词和动词用法。"
}"#
    }

    #[test]
    fn valid_json_parses_and_validates() {
        let input = input_without_context();
        let card = parse_explanation_card_content(&input, valid_card_json()).unwrap();

        assert_eq!(card.headword, "market");
        assert_eq!(card.examples.len(), 1);
    }

    #[test]
    fn invalid_json_fails() {
        let input = input_without_context();
        let error = parse_explanation_card_content(&input, "{not-json").unwrap_err();

        assert!(error.contains("不是合法 ExplanationCard JSON"));
    }

    #[test]
    fn context_meaning_without_context_text_fails() {
        let input = input_without_context();
        let content = r#"{
  "queryType": "word",
  "headword": "market",
  "basicMeaning": "市场；销售；推广",
  "contextMeaning": "在上下文中表示推广。",
  "phrases": [],
  "nearMeanings": [],
  "examples": [
    { "en": "They plan to market the product.", "zh": "他们计划推广这个产品。" }
  ]
}"#;

        let error = parse_explanation_card_content(&input, content).unwrap_err();

        assert!(error.contains("contextMeaning"));
    }

    #[test]
    #[ignore = "requires DEEPSEEK_API_KEY and network access"]
    fn live_create_explanation_card_with_flash_model() {
        load_project_env_for_live_test();
        std::env::set_var("DEEPSEEK_MODEL", "deepseek-v4-flash");

        let input = CaptureInput {
            query_text: "market".to_string(),
            context_text: Some("They plan to market the product in Europe.".to_string()),
            source_type: SourceType::Manual,
        };

        let card = tauri::async_runtime::block_on(create_explanation_card(input))
            .expect("live DeepSeek ExplanationCard request should succeed");

        assert_eq!(card.query_type, QueryType::Word);
        assert!(!card.headword.trim().is_empty());
        assert!(!card.basic_meaning.trim().is_empty());
        assert!(!card.examples.is_empty());
        assert!(card.examples.len() <= 2);
        assert!(card
            .examples
            .iter()
            .all(|example| !example.en.trim().is_empty() && !example.zh.trim().is_empty()));
    }
}
