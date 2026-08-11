use crate::deepseek_client::{
    configured_model, post_tracked_chat_completion_with_policy_and_checkpoint,
    ChatCompletionRequestPolicy, TrackedChatCompletionError,
};
use crate::explanation::{
    classify_query_type, normalize_source_sentence_translation, validate_explanation_card,
    CaptureInput, ExplanationCard, QueryType, SourceType, MAX_CONTEXT_TEXT_LEN,
};
use crate::learning_records;
use crate::model_usage::ModelUsageCategory;
use futures_util::future::{AbortHandle, AbortRegistration, Abortable};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const EXPLANATION_CARD_TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const EXPLANATION_CARD_MAX_TRANSIENT_RETRIES: u8 = 1;
const WORD_MAX_TOKENS: u16 = 1_536;
const PHRASE_MAX_TOKENS: u16 = 1_536;
const SENTENCE_MAX_TOKENS: u16 = 1_536;
const PARAGRAPH_MAX_TOKENS: u16 = 4_096;
const EXPLANATION_CARD_TEMPERATURE: f32 = 0.2;
const EXPLANATION_REQUEST_CANCELLED: &str = "READRAY_EXPLANATION_REQUEST_CANCELLED";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExplanationRequestScope {
    Manual,
    Anchored,
}

struct ActiveExplanationRequest {
    request_key: String,
    generation: u64,
    abort_handle: AbortHandle,
}

#[derive(Default)]
struct ExplanationRequestAuthorityState {
    next_generation: u64,
    active: HashMap<ExplanationRequestScope, ActiveExplanationRequest>,
}

#[derive(Default)]
struct ExplanationRequestAuthority {
    state: Mutex<ExplanationRequestAuthorityState>,
}

impl ExplanationRequestAuthority {
    fn register(
        self: &Arc<Self>,
        scope: ExplanationRequestScope,
        request_key: String,
    ) -> Result<(ExplanationRequestGuard, AbortRegistration), String> {
        validate_request_key(&request_key)?;
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let mut state = self
            .state
            .lock()
            .map_err(|_| "ExplanationCard 请求权威不可用。".to_string())?;
        let generation = state
            .next_generation
            .checked_add(1)
            .ok_or_else(|| "ExplanationCard 请求 generation 已耗尽。".to_string())?;
        state.next_generation = generation;
        let previous = state.active.insert(
            scope,
            ActiveExplanationRequest {
                request_key: request_key.clone(),
                generation,
                abort_handle,
            },
        );
        drop(state);
        if let Some(previous) = previous {
            previous.abort_handle.abort();
        }
        Ok((
            ExplanationRequestGuard {
                authority: Arc::clone(self),
                scope,
                request_key,
                generation,
            },
            abort_registration,
        ))
    }

    fn is_current(
        &self,
        scope: ExplanationRequestScope,
        request_key: &str,
        generation: u64,
    ) -> bool {
        self.state
            .lock()
            .ok()
            .and_then(|state| {
                state.active.get(&scope).map(|request| {
                    request.request_key == request_key && request.generation == generation
                })
            })
            .unwrap_or(false)
    }

    fn cancel(&self, scope: ExplanationRequestScope, request_key: Option<&str>) {
        let request = self.state.lock().ok().and_then(|mut state| {
            let matches = state
                .active
                .get(&scope)
                .is_some_and(|request| request_key.is_none_or(|key| request.request_key == key));
            matches.then(|| state.active.remove(&scope)).flatten()
        });
        if let Some(request) = request {
            request.abort_handle.abort();
        }
    }

    fn finish_if_current(
        &self,
        scope: ExplanationRequestScope,
        request_key: &str,
        generation: u64,
    ) {
        if let Ok(mut state) = self.state.lock() {
            let matches = state.active.get(&scope).is_some_and(|request| {
                request.request_key == request_key && request.generation == generation
            });
            if matches {
                state.active.remove(&scope);
            }
        }
    }

    fn commit_if_current<T>(
        &self,
        scope: ExplanationRequestScope,
        request_key: &str,
        generation: u64,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "ExplanationCard 请求权威不可用。".to_string())?;
        let is_current = state.active.get(&scope).is_some_and(|request| {
            request.request_key == request_key && request.generation == generation
        });
        if !is_current {
            return Err(cancelled_request_error());
        }
        operation()
    }
}

struct ExplanationRequestGuard {
    authority: Arc<ExplanationRequestAuthority>,
    scope: ExplanationRequestScope,
    request_key: String,
    generation: u64,
}

impl ExplanationRequestGuard {
    fn is_current(&self) -> bool {
        self.authority
            .is_current(self.scope, &self.request_key, self.generation)
    }

    fn commit_if_current<T>(
        &self,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        self.authority
            .commit_if_current(self.scope, &self.request_key, self.generation, operation)
    }
}

impl Drop for ExplanationRequestGuard {
    fn drop(&mut self) {
        self.authority
            .finish_if_current(self.scope, &self.request_key, self.generation);
    }
}

static EXPLANATION_REQUEST_AUTHORITY: OnceLock<Arc<ExplanationRequestAuthority>> = OnceLock::new();

fn explanation_request_authority() -> Arc<ExplanationRequestAuthority> {
    Arc::clone(EXPLANATION_REQUEST_AUTHORITY.get_or_init(|| Arc::new(Default::default())))
}

fn validate_request_key(request_key: &str) -> Result<(), String> {
    let valid = !request_key.is_empty()
        && request_key.len() <= 128
        && request_key
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_' | b':'));
    if valid {
        Ok(())
    } else {
        Err("ExplanationCard requestKey 无效。".to_string())
    }
}

fn cancelled_request_error() -> String {
    EXPLANATION_REQUEST_CANCELLED.to_string()
}

pub(crate) fn cancel_explanation_scope(scope: ExplanationRequestScope) {
    explanation_request_authority().cancel(scope, None);
}

pub(crate) fn cancel_all_explanation_requests() {
    let authority = explanation_request_authority();
    authority.cancel(ExplanationRequestScope::Manual, None);
    authority.cancel(ExplanationRequestScope::Anchored, None);
}

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
    request_key: String,
    request_scope: ExplanationRequestScope,
    minimal_context_text: Option<String>,
) -> Result<ExplanationCard, String> {
    let authority = explanation_request_authority();
    let (request, abort_registration) = authority.register(request_scope, request_key)?;
    let provider_request =
        create_explanation_card_for_input(&app, &input, minimal_context_text.as_deref(), || {
            request.is_current()
        });
    let card = Abortable::new(provider_request, abort_registration)
        .await
        .map_err(|_| cancelled_request_error())??;
    request.commit_if_current(|| {
        learning_records::save_for_app(&app, &input, &card)
            .map(|_| ())
            .map_err(|error| format!("ExplanationCard 保存失败：{error}"))
    })?;

    Ok(card)
}

#[tauri::command]
pub fn cancel_explanation_request(
    request_key: String,
    request_scope: ExplanationRequestScope,
) -> Result<(), String> {
    validate_request_key(&request_key)?;
    explanation_request_authority().cancel(request_scope, Some(&request_key));
    Ok(())
}

pub(crate) async fn create_explanation_card_for_input(
    app: &tauri::AppHandle,
    input: &CaptureInput,
    minimal_context_text: Option<&str>,
    is_current: impl Fn() -> bool,
) -> Result<ExplanationCard, String> {
    let request_body = build_request_body(input, minimal_context_text)?;
    let response: DeepSeekChatResponse = post_tracked_chat_completion_with_policy_and_checkpoint(
        app,
        ModelUsageCategory::ExplanationQuery,
        "DeepSeek ExplanationCard",
        &request_body,
        ChatCompletionRequestPolicy::new(
            EXPLANATION_CARD_TOTAL_TIMEOUT,
            EXPLANATION_CARD_MAX_TRANSIENT_RETRIES,
        ),
        is_current,
    )
    .await
    .map_err(|error| match error {
        TrackedChatCompletionError::Cancelled => cancelled_request_error(),
        TrackedChatCompletionError::Failed(error) => error,
    })?;
    finish_explanation_response(input, response)
}

fn build_request_body(
    input: &CaptureInput,
    minimal_context_text: Option<&str>,
) -> Result<serde_json::Value, String> {
    let minimal_context_text = normalize_minimal_context_text(minimal_context_text)?;
    let query_type = classify_query_type(&input.query_text)?;
    let model = configured_model();

    let user_prompt = build_user_prompt(input, minimal_context_text)?;
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
        "thinking": {
            "type": "disabled"
        },
        "stream": false,
        "max_tokens": max_tokens_for_query_type(query_type),
        "temperature": EXPLANATION_CARD_TEMPERATURE
    }))
}

fn normalize_minimal_context_text(value: Option<&str>) -> Result<Option<&str>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Ok(None);
    }

    let len = value.chars().count();
    if len > MAX_CONTEXT_TEXT_LEN {
        return Err(format!(
            "minimalContextText 长度不能超过 {MAX_CONTEXT_TEXT_LEN} 个字符，当前为 {len}。"
        ));
    }
    Ok(Some(value))
}

fn max_tokens_for_query_type(query_type: QueryType) -> u16 {
    match query_type {
        QueryType::Word => WORD_MAX_TOKENS,
        QueryType::Phrase => PHRASE_MAX_TOKENS,
        QueryType::Sentence => SENTENCE_MAX_TOKENS,
        QueryType::Paragraph => PARAGRAPH_MAX_TOKENS,
    }
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
        return Err(
            "ExplanationCard 模型输出错误：DeepSeek 返回空内容，无法解析 JSON。".to_string(),
        );
    }

    let mut value: serde_json::Value = serde_json::from_str(content)
        .map_err(|error| format!("ExplanationCard 模型输出错误：返回内容不是合法 JSON：{error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "ExplanationCard 模型输出错误：JSON 顶层必须是对象。".to_string())?;
    object.insert(
        "sourceText".to_string(),
        serde_json::Value::String(input.query_text.trim().to_string()),
    );
    let mut card: ExplanationCard = serde_json::from_value(value)
        .map_err(|error| format!("ExplanationCard 模型输出错误：JSON 结构无效：{error}"))?;
    normalize_source_sentence_translation(&mut card);
    validate_explanation_card(input, &card).map_err(|errors| {
        format!(
            "ExplanationCard schema 错误：{}",
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
        .ok_or_else(|| "ExplanationCard 模型输出错误：响应缺少 choices[0]。".to_string())?;

    if let Some(finish_reason) = choice.finish_reason.as_deref() {
        if finish_reason != "stop" {
            return Err(format!(
                "ExplanationCard 模型输出错误：生成未正常结束（finish_reason={finish_reason}）。"
            ));
        }
    }

    choice.message.content.ok_or_else(|| {
        "ExplanationCard 模型输出错误：响应缺少 choices[0].message.content。".to_string()
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelCaptureInput<'a> {
    query_text: &'a str,
    context_text: Option<&'a str>,
    source_type: &'a SourceType,
    source_app: Option<&'a str>,
}

fn build_user_prompt(
    input: &CaptureInput,
    minimal_context_text: Option<&str>,
) -> Result<String, String> {
    let model_input = ModelCaptureInput {
        query_text: &input.query_text,
        context_text: minimal_context_text,
        source_type: &input.source_type,
        source_app: input.source_app.as_deref(),
    };
    let input_json = serde_json::to_string_pretty(&model_input)
        .map_err(|error| format!("CaptureInput 无法序列化为 JSON：{error}"))?;

    Ok(format!("CaptureInput JSON:\n{input_json}"))
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
- sourceText is the selected text; headword is the normalized lookup form.
- Prefer the contextual sense. contextMeaning is allowed only with nonblank contextText (max 800 chars).
- Copy a useful contextual sentence when present. sourceSentence may appear without sourceSentenceZh.
- Provide sourceSentenceZh (max 2400) only when sourceSentence (max 1200) is primarily English. When sourceSentence is primarily Chinese, including Chinese-dominant mixed text with terms such as Rust/generation or Memory/Review, sourceSentenceZh must be null. sourceSentenceZh is never allowed without sourceSentence.
- partOfSpeech and phonetic may be null. Code identifiers such as anchorRect should normally use null phonetic.
- basicMeanings requires 1-4 concise Chinese meanings (max 400 chars each).
- phrases and nearMeanings have at most 3 useful items each; examples have at most 2 bilingual items and may be empty.
- Omit optional content rather than fabricate it."#
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
- sourceText is the selected text. Explain it as one semantic unit; basicMeaning is required (max 400 chars).
- contextMeaning is allowed only with nonblank contextText (max 800 chars).
- composition is optional (max 800 chars) and only explains useful structure or usage.
- Copy a useful contextual sentence when present. sourceSentence may appear without sourceSentenceZh.
- Provide sourceSentenceZh (max 2400) only when sourceSentence (max 1200) is primarily English. When sourceSentence is primarily Chinese, including Chinese-dominant mixed text with terms such as Rust/generation or Memory/Review, sourceSentenceZh must be null. sourceSentenceZh is never allowed without sourceSentence.
- examples has at most 2 bilingual items and may be empty. Do not fabricate content or split the phrase into word-card fields."#
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
- sourceText is the selected text. translation is required (max 12000 chars); translate the complete sentence naturally and accurately.
- keyPoints has at most 3 useful expressions (expression max 160 chars; meaning max 600).
- explanation (max 1600) and reviewHint (max 400) are optional; omit or use null when they add no value.
- Do not fabricate or generate word-card fields."#
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
- sourceText is the selected text. translation is required (max 12000 chars); translate the complete paragraph without omitting sentences.
- Preserve paragraph breaks when useful.
- keyPoints has at most 5 useful expressions (expression max 160 chars; meaning max 600).
- summary is optional (max 1000 chars) and only adds meaning beyond the translation.
- Do not fabricate or generate word-card fields."#
        }
    };

    format!(
        r#"Create one ReadRay ExplanationCard JSON object matching the schema below: no Markdown, code fences, commentary, or extra object.
queryType is fixed by the local classifier; keep the exact camelCase property names shown.
Use concise Chinese explanations and natural complete Chinese translations. Never claim dictionary authority or invent unsupported facts.

{schema}"#
    )
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
        let request_body = build_request_body(input, input.context_text.as_deref())?;
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
    fn request_body_uses_non_thinking_fast_path_and_schema_specific_budgets() {
        let cases = [
            (input("market", None), QueryType::Word, WORD_MAX_TOKENS),
            (
                input("in progress", None),
                QueryType::Phrase,
                PHRASE_MAX_TOKENS,
            ),
            (
                input("The work is still in progress.", None),
                QueryType::Sentence,
                SENTENCE_MAX_TOKENS,
            ),
            (
                input(
                    "The first sentence explains the state. The second describes the next action.",
                    None,
                ),
                QueryType::Paragraph,
                PARAGRAPH_MAX_TOKENS,
            ),
        ];

        for (input, query_type, expected_max_tokens) in cases {
            let body = build_request_body(&input, input.context_text.as_deref()).unwrap();
            assert_eq!(body["thinking"]["type"], "disabled");
            assert_eq!(body["response_format"]["type"], "json_object");
            assert_eq!(body["stream"], false);
            assert_eq!(body["max_tokens"], expected_max_tokens);
            assert_eq!(max_tokens_for_query_type(query_type), expected_max_tokens);
        }

        assert_eq!(EXPLANATION_CARD_TOTAL_TIMEOUT, Duration::from_secs(10));
        assert_eq!(EXPLANATION_CARD_MAX_TRANSIENT_RETRIES, 1);
        assert!(WORD_MAX_TOKENS < PARAGRAPH_MAX_TOKENS);
        assert!(PHRASE_MAX_TOKENS < PARAGRAPH_MAX_TOKENS);
        assert!(SENTENCE_MAX_TOKENS < PARAGRAPH_MAX_TOKENS);
    }

    #[test]
    fn request_prompt_uses_separate_minimal_context_with_stable_field_order() {
        let input = input(
            "market",
            Some("Original full paragraph with unrelated surrounding content."),
        );
        let body = build_request_body(&input, Some("The market remained open.")).unwrap();
        let prompt = body["messages"][1]["content"].as_str().unwrap();

        assert!(prompt.contains("The market remained open."));
        assert!(!prompt.contains("Original full paragraph"));
        let query_position = prompt.find("\"queryText\"").unwrap();
        let context_position = prompt.find("\"contextText\"").unwrap();
        let source_type_position = prompt.find("\"sourceType\"").unwrap();
        let source_app_position = prompt.find("\"sourceApp\"").unwrap();
        assert!(query_position < context_position);
        assert!(context_position < source_type_position);
        assert!(source_type_position < source_app_position);
    }

    #[test]
    fn word_and_phrase_prompts_define_asymmetric_source_sentence_translation() {
        for query_type in [QueryType::Word, QueryType::Phrase] {
            let prompt = explanation_card_system_prompt(query_type);
            assert!(prompt.contains("sourceSentence may appear without sourceSentenceZh"));
            assert!(prompt.contains("only when sourceSentence (max 1200) is primarily English"));
            assert!(prompt.contains("sourceSentenceZh must be null"));
            assert!(prompt.contains("sourceSentenceZh is never allowed without sourceSentence"));
        }
    }

    #[test]
    fn chinese_dominant_model_source_sentence_drops_translation_before_validation() {
        let context = "ReadRay 会在请求结束前检查 Rust generation，避免旧结果覆盖新结果。";
        let content = json!({
            "queryType": "word",
            "sourceText": "generation",
            "headword": "generation",
            "basicMeanings": ["代次；生成"],
            "contextMeaning": "这里指请求的内部代次。",
            "sourceSentence": context,
            "sourceSentenceZh": "ReadRay 会检查请求代次，避免旧结果覆盖新结果。",
            "phrases": [],
            "nearMeanings": [],
            "examples": []
        })
        .to_string();

        let card =
            parse_explanation_card_content(&input("generation", Some(context)), &content).unwrap();

        assert!(matches!(
            card,
            ExplanationCard::Word {
                source_sentence: Some(_),
                source_sentence_zh: None,
                ..
            }
        ));
    }

    #[test]
    fn english_model_source_sentence_keeps_chinese_translation() {
        let context = "The request generation prevents an older result from replacing a newer one.";
        let content = json!({
            "queryType": "phrase",
            "sourceText": "request generation",
            "basicMeaning": "请求代次",
            "contextMeaning": "用于区分新旧异步请求的代次。",
            "composition": null,
            "sourceSentence": context,
            "sourceSentenceZh": "请求代次可以防止旧结果覆盖新结果。",
            "examples": []
        })
        .to_string();

        let card =
            parse_explanation_card_content(&input("request generation", Some(context)), &content)
                .unwrap();

        assert!(matches!(
            card,
            ExplanationCard::Phrase {
                source_sentence: Some(_),
                source_sentence_zh: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn minimal_context_is_normalized_and_bounded_before_provider_request() {
        let input = input(
            "market",
            Some("Original context remains the learning fact."),
        );
        let blank_body = build_request_body(&input, Some(" \r\n\t ")).unwrap();
        let blank_prompt = blank_body["messages"][1]["content"].as_str().unwrap();
        assert!(blank_prompt.contains("\"contextText\": null"));
        assert_eq!(
            input.context_text.as_deref(),
            Some("Original context remains the learning fact.")
        );

        let maximum = "界".repeat(MAX_CONTEXT_TEXT_LEN);
        assert!(build_request_body(&input, Some(&maximum)).is_ok());

        let over_limit = "界".repeat(MAX_CONTEXT_TEXT_LEN + 1);
        let error = build_request_body(&input, Some(&over_limit)).err().unwrap();
        assert_eq!(
            error,
            format!(
                "minimalContextText 长度不能超过 {MAX_CONTEXT_TEXT_LEN} 个字符，当前为 {}。",
                MAX_CONTEXT_TEXT_LEN + 1
            )
        );
    }

    #[test]
    fn request_authority_isolates_scopes_and_aborts_replaced_requests() {
        let authority = Arc::new(ExplanationRequestAuthority::default());
        let (manual_a, manual_a_abort) = authority
            .register(ExplanationRequestScope::Manual, "manual:1".to_string())
            .unwrap();
        let (anchored_a, _) = authority
            .register(ExplanationRequestScope::Anchored, "anchored:1".to_string())
            .unwrap();
        let (manual_b, _) = authority
            .register(ExplanationRequestScope::Manual, "manual:2".to_string())
            .unwrap();

        assert!(!manual_a.is_current());
        assert!(manual_b.is_current());
        assert!(anchored_a.is_current());
        let aborted = tauri::async_runtime::block_on(Abortable::new(
            std::future::pending::<()>(),
            manual_a_abort,
        ));
        assert!(aborted.is_err());
    }

    #[test]
    fn repeated_client_key_cannot_reuse_an_old_request_generation() {
        let authority = Arc::new(ExplanationRequestAuthority::default());
        let (old_request, old_abort) = authority
            .register(ExplanationRequestScope::Manual, "manual:1".to_string())
            .unwrap();
        let (new_request, _) = authority
            .register(ExplanationRequestScope::Manual, "manual:1".to_string())
            .unwrap();

        assert_ne!(old_request.generation, new_request.generation);
        assert!(!old_request.is_current());
        assert!(new_request.is_current());
        let aborted =
            tauri::async_runtime::block_on(Abortable::new(std::future::pending::<()>(), old_abort));
        assert!(aborted.is_err());

        let old_commit_count = std::cell::Cell::new(0_u8);
        let error = old_request
            .commit_if_current(|| {
                old_commit_count.set(old_commit_count.get() + 1);
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error, EXPLANATION_REQUEST_CANCELLED);
        assert_eq!(old_commit_count.get(), 0);

        drop(old_request);
        assert!(new_request.is_current());

        let new_commit_count = std::cell::Cell::new(0_u8);
        new_request
            .commit_if_current(|| {
                new_commit_count.set(new_commit_count.get() + 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(new_commit_count.get(), 1);
    }

    #[test]
    fn cancelling_an_old_distinct_client_key_keeps_the_current_request() {
        let authority = Arc::new(ExplanationRequestAuthority::default());
        let (old_request, _) = authority
            .register(ExplanationRequestScope::Manual, "manual:old:1".to_string())
            .unwrap();
        let (new_request, _) = authority
            .register(ExplanationRequestScope::Manual, "manual:new:1".to_string())
            .unwrap();

        authority.cancel(ExplanationRequestScope::Manual, Some("manual:old:1"));

        assert!(!old_request.is_current());
        assert!(new_request.is_current());
        let commit_count = std::cell::Cell::new(0_u8);
        new_request
            .commit_if_current(|| {
                commit_count.set(commit_count.get() + 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(commit_count.get(), 1);
    }

    #[test]
    fn request_generation_exhaustion_fails_without_replacing_the_active_request() {
        let authority = Arc::new(ExplanationRequestAuthority::default());
        let (active_request, _) = authority
            .register(
                ExplanationRequestScope::Anchored,
                "anchored:active".to_string(),
            )
            .unwrap();
        authority.state.lock().unwrap().next_generation = u64::MAX;

        let error = authority
            .register(
                ExplanationRequestScope::Anchored,
                "anchored:new".to_string(),
            )
            .err()
            .unwrap();

        assert_eq!(error, "ExplanationCard 请求 generation 已耗尽。");
        assert!(active_request.is_current());
    }

    #[test]
    fn cancellation_and_late_completion_cannot_commit_a_learning_record() {
        let authority = Arc::new(ExplanationRequestAuthority::default());
        let (request, _) = authority
            .register(
                ExplanationRequestScope::Anchored,
                "anchored:late".to_string(),
            )
            .unwrap();
        authority.cancel(ExplanationRequestScope::Anchored, Some("anchored:late"));

        let save_count = std::cell::Cell::new(0_u8);
        let error = request
            .commit_if_current(|| {
                save_count.set(save_count.get() + 1);
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error, EXPLANATION_REQUEST_CANCELLED);
        assert_eq!(save_count.get(), 0);
    }

    #[test]
    fn provider_returned_then_cancelled_before_save_is_rejected() {
        let authority = Arc::new(ExplanationRequestAuthority::default());
        let (request, _) = authority
            .register(
                ExplanationRequestScope::Manual,
                "manual:returned".to_string(),
            )
            .unwrap();
        let provider_result = "validated card";

        authority.cancel(ExplanationRequestScope::Manual, Some("manual:returned"));
        let saved = std::cell::Cell::new(false);
        let result = request.commit_if_current(|| {
            let _ = provider_result;
            saved.set(true);
            Ok(())
        });

        assert_eq!(result.unwrap_err(), EXPLANATION_REQUEST_CANCELLED);
        assert!(!saved.get());
    }

    #[test]
    fn failed_request_can_be_retried_with_a_new_key() {
        let authority = Arc::new(ExplanationRequestAuthority::default());
        let (failed, _) = authority
            .register(ExplanationRequestScope::Manual, "manual:failed".to_string())
            .unwrap();
        drop(failed);
        let (retry, _) = authority
            .register(ExplanationRequestScope::Manual, "manual:retry".to_string())
            .unwrap();

        assert!(retry.is_current());
    }

    #[test]
    fn paragraph_budget_keeps_normal_long_translation_schema_capacity() {
        let input = input(
            "The first sentence introduces the subject. The second sentence develops it.",
            None,
        );
        let content = json!({
            "queryType": "paragraph",
            "sourceText": input.query_text.clone(),
            "translation": "译".repeat(3_000),
            "keyPoints": [],
            "summary": null
        })
        .to_string();

        let card = parse_explanation_card_content(&input, &content).unwrap();
        assert_eq!(card.query_type(), QueryType::Paragraph);
        assert_eq!(max_tokens_for_query_type(QueryType::Paragraph), 4_096);
    }

    #[test]
    fn invalid_json_fails() {
        let error =
            parse_explanation_card_content(&input("market", None), "{not-json").unwrap_err();

        assert!(error.starts_with("ExplanationCard 模型输出错误："));
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

        assert!(error.starts_with("ExplanationCard schema 错误："));
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
