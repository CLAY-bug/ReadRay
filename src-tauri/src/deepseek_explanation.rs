use crate::deepseek_client::{configured_model, post_tracked_chat_completion};
use crate::explanation::{
    classify_query_type, validate_explanation_card, CaptureInput, ExplanationCard, QueryType,
};
use crate::learning_records;
use crate::model_usage::ModelUsageCategory;
use serde::Deserialize;
use serde_json::json;

const EXPLANATION_CARD_MAX_TOKENS: u16 = 4_096;
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
pub async fn create_explanation_card(
    app: tauri::AppHandle,
    input: CaptureInput,
) -> Result<ExplanationCard, String> {
    let card = create_explanation_card_for_input(&app, &input).await?;
    learning_records::save_for_app(&app, &input, &card)
        .map_err(|error| format!("ExplanationCard 已生成，但学习记录保存失败：{error}"))?;

    Ok(card)
}

pub(crate) async fn create_explanation_card_for_input(
    app: &tauri::AppHandle,
    input: &CaptureInput,
) -> Result<ExplanationCard, String> {
    let request_body = build_request_body(input)?;
    let response: DeepSeekChatResponse = post_tracked_chat_completion(
        app,
        ModelUsageCategory::ExplanationQuery,
        "DeepSeek ExplanationCard",
        &request_body,
    )
    .await?;
    finish_explanation_response(input, response)
}

fn build_request_body(input: &CaptureInput) -> Result<serde_json::Value, String> {
    let query_type = classify_query_type(&input.query_text)?;
    let model = configured_model();

    let user_prompt = build_user_prompt(&input, query_type)?;
    Ok(json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": explanation_card_system_prompt(query_type)
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
    }))
}

fn finish_explanation_response(
    input: &CaptureInput,
    response: DeepSeekChatResponse,
) -> Result<ExplanationCard, String> {
    let content = extract_content(response)?;

    parse_explanation_card_content(input, &content)
}

pub(crate) fn parse_explanation_card_content(
    input: &CaptureInput,
    content: &str,
) -> Result<ExplanationCard, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("DeepSeek 返回空内容，无法解析 ExplanationCard JSON。".to_string());
    }

    let mut value: serde_json::Value = serde_json::from_str(content)
        .map_err(|error| format!("DeepSeek 返回内容不是合法 ExplanationCard JSON：{error}"))?;
    let object = value.as_object_mut().ok_or_else(|| {
        "DeepSeek 返回内容不是合法 ExplanationCard JSON：顶层必须是对象。".to_string()
    })?;
    object.insert(
        "sourceText".to_string(),
        serde_json::Value::String(input.query_text.trim().to_string()),
    );
    let card: ExplanationCard = serde_json::from_value(value)
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

fn build_user_prompt(input: &CaptureInput, query_type: QueryType) -> Result<String, String> {
    let input_json = serde_json::to_string_pretty(input)
        .map_err(|error| format!("CaptureInput 无法序列化为 JSON：{error}"))?;

    Ok(format!(
        "The local classifier selected queryType={}. Create exactly one matching ExplanationCard JSON object.\n\nCaptureInput JSON:\n{input_json}",
        query_type_label(query_type)
    ))
}

fn explanation_card_system_prompt(query_type: QueryType) -> String {
    let schema = match query_type {
        QueryType::Word => {
            r#"{
  "queryType": "word",
  "sourceText": "market",
  "headword": "market",
  "partOfSpeech": "noun / verb",
  "phonetic": "/ˈmɑːrkɪt/",
  "basicMeanings": ["市场", "推广；销售"],
  "contextMeaning": null,
  "sourceSentence": null,
  "sourceSentenceZh": null,
  "phrases": [{ "phrase": "market share", "meaning": "市场份额" }],
  "nearMeanings": [{ "term": "promote", "meaning": "强调宣传和推广" }],
  "examples": [{ "en": "The company entered a new market.", "zh": "这家公司进入了一个新市场。" }],
  "reviewHint": null
}

Word rules:
- Preserve sourceText exactly. headword is the normalized lookup form.
- Prefer the meaning used in contextText. Put that concise meaning in contextMeaning.
- If contextText contains a useful source sentence, copy it to sourceSentence and translate it to sourceSentenceZh. Otherwise both must be null.
- partOfSpeech and phonetic may be null. Code identifiers such as anchorRect should normally use null phonetic.
- basicMeanings requires 1 to 4 concise Chinese meanings.
- phrases and nearMeanings contain at most 3 useful items each.
- examples contain at most 2 bilingual items and may be empty when an invented example would add little value.
- Do not invent fields merely to fill the card."#
        }
        QueryType::Phrase => {
            r#"{
  "queryType": "phrase",
  "sourceText": "in progress",
  "basicMeaning": "正在进行中",
  "contextMeaning": null,
  "composition": "介词短语，常作表语",
  "sourceSentence": null,
  "sourceSentenceZh": null,
  "examples": [{ "en": "The work is still in progress.", "zh": "这项工作仍在进行中。" }],
  "reviewHint": null
}

Phrase rules:
- Preserve sourceText exactly.
- Explain the phrase as one semantic unit. basicMeaning is required.
- Prefer the meaning used in contextText and put it in contextMeaning.
- composition is optional and should only explain useful structure or usage.
- If contextText contains a useful source sentence, copy it to sourceSentence and translate it to sourceSentenceZh. Otherwise both must be null.
- examples contain at most 2 bilingual items and may be empty.
- Do not split the phrase into fake word-card fields."#
        }
        QueryType::Sentence => {
            r#"{
  "queryType": "sentence",
  "sourceText": "The window remains beside the selected text.",
  "translation": "窗口会保持在所选文本旁边。",
  "keyPoints": [
    { "expression": "remain beside", "meaning": "保持在……旁边" }
  ],
  "explanation": null,
  "reviewHint": null
}

Sentence rules:
- Preserve sourceText exactly.
- translation is required and is the primary output. Translate the complete sentence naturally and accurately.
- keyPoints contains at most 3 expressions that materially help comprehension.
- explanation and reviewHint are optional. Omit or use null when they add no value.
- Do not generate phonetics, dictionary meanings, collocations, or invented examples."#
        }
        QueryType::Paragraph => {
            r#"{
  "queryType": "paragraph",
  "sourceText": "The first sentence explains the state. The second describes the next action.",
  "translation": "第一句解释当前状态。第二句描述下一步操作。",
  "keyPoints": [
    { "expression": "next action", "meaning": "下一步操作" }
  ],
  "summary": null
}

Paragraph rules:
- Preserve sourceText exactly.
- translation is required and is the primary output. Translate the complete selected paragraph without omitting sentences.
- Preserve paragraph breaks when useful.
- keyPoints contains at most 5 expressions that materially help comprehension.
- summary is optional and should only be present when it adds meaning beyond the translation.
- Do not generate phonetics, dictionary meanings, collocations, or invented examples."#
        }
    };

    format!(
        r#"You create structured JSON for ReadRay, a desktop English learning app.

Return exactly one JSON object matching the schema below. Do not return Markdown, code fences, or commentary.
The queryType is already determined locally. Do not change it.
All JSON property names must use the exact camelCase spelling shown.
Use concise Chinese for explanations and natural Chinese for translations.
Do not cite or pretend to quote any commercial or authoritative dictionary.
Only include optional information when it is useful.
If captureInput.contextText is missing, null, or blank, contextMeaning must be null or omitted.
If captureInput.contextText is present, contextMeaning may be present but is not required.

{schema}"#
    )
}

fn query_type_label(query_type: QueryType) -> &'static str {
    match query_type {
        QueryType::Word => "word",
        QueryType::Phrase => "phrase",
        QueryType::Sentence => "sentence",
        QueryType::Paragraph => "paragraph",
    }
}

fn summarize_validation_errors(errors: &[String]) -> String {
    errors.join("；")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepseek_client::post_chat_completion_for_test;
    use crate::explanation::SourceType;
    use std::path::PathBuf;

    fn input(query_text: &str, context_text: Option<&str>) -> CaptureInput {
        CaptureInput {
            query_text: query_text.to_string(),
            context_text: context_text.map(str::to_string),
            source_type: SourceType::Manual,
            source_app: None,
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

    async fn create_explanation_card_for_live_test(
        input: &CaptureInput,
    ) -> Result<ExplanationCard, String> {
        let request_body = build_request_body(input)?;
        let response: DeepSeekChatResponse =
            post_chat_completion_for_test("DeepSeek ExplanationCard", &request_body).await?;
        finish_explanation_response(input, response)
    }

    #[test]
    fn json_for_each_query_type_parses_correctly() {
        let cases = [
            (
                input("market", None),
                r#"{
  "queryType": "word",
  "sourceText": "market",
  "headword": "market",
  "partOfSpeech": "noun / verb",
  "phonetic": "/ˈmɑːrkɪt/",
  "basicMeanings": ["市场", "推广"],
  "phrases": [],
  "nearMeanings": [],
  "examples": []
}"#,
                QueryType::Word,
            ),
            (
                input("in progress", None),
                r#"{
  "queryType": "phrase",
  "sourceText": "in progress",
  "basicMeaning": "正在进行中",
  "examples": []
}"#,
                QueryType::Phrase,
            ),
            (
                input("The work is still in progress.", None),
                r#"{
  "queryType": "sentence",
  "sourceText": "The work is still in progress.",
  "translation": "这项工作仍在进行中。",
  "keyPoints": []
}"#,
                QueryType::Sentence,
            ),
            (
                input(
                    "The first sentence explains the state. The second describes the next action.",
                    None,
                ),
                r#"{
  "queryType": "paragraph",
  "sourceText": "The first sentence explains the state. The second describes the next action.",
  "translation": "第一句解释当前状态。第二句描述下一步操作。",
  "keyPoints": []
}"#,
                QueryType::Paragraph,
            ),
        ];

        for (input, json, expected_type) in cases {
            let card = parse_explanation_card_content(&input, json).unwrap();
            assert_eq!(card.query_type(), expected_type);
        }
    }

    #[test]
    fn invalid_json_fails() {
        let error =
            parse_explanation_card_content(&input("market", None), "{not-json").unwrap_err();

        assert!(error.contains("不是合法 ExplanationCard JSON"));
    }

    #[test]
    fn context_meaning_without_context_text_fails() {
        let content = r#"{
  "queryType": "word",
  "sourceText": "market",
  "headword": "market",
  "basicMeanings": ["市场"],
  "contextMeaning": "在上下文中表示推广。",
  "phrases": [],
  "nearMeanings": [],
  "examples": []
}"#;

        let error = parse_explanation_card_content(&input("market", None), content).unwrap_err();

        assert!(error.contains("contextMeaning"));
    }

    #[test]
    #[ignore = "requires DEEPSEEK_API_KEY and network access"]
    fn live_create_explanation_cards_with_flash_model() {
        load_project_env_for_live_test();
        std::env::set_var("DEEPSEEK_MODEL", "deepseek-v4-flash");

        let long_sentence = "This implementation keeps the original selection available while the asynchronous explanation request is running, so the result can still be placed beside the source sentence.";
        let paragraph = "A reliable desktop reading tool needs to preserve the user's current focus while it gathers context from the foreground application. It should avoid stealing focus before capture, because doing so can destroy the selection that the user intended to explain. After capture succeeds, the tool can call a language model, validate the structured response, and place a compact result beside the original text. The result window should remain small for a single word, but it should grow for a sentence or paragraph and use internal scrolling when the content exceeds the available work area.";
        let cases = [
            ("instruction", QueryType::Word),
            ("anchorRect", QueryType::Word),
            (long_sentence, QueryType::Sentence),
            (paragraph, QueryType::Paragraph),
        ];

        for (query_text, expected_type) in cases {
            let card = tauri::async_runtime::block_on(create_explanation_card_for_live_test(
                &input(query_text, None),
            ))
            .expect("live DeepSeek ExplanationCard request should succeed");

            assert_eq!(card.query_type(), expected_type);
        }
    }
}
