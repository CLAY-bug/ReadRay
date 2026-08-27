use crate::deepseek_client::{
    configured_model, post_tracked_chat_completion_with_policy_and_checkpoint,
    ChatCompletionRequestPolicy, TrackedChatCompletionError,
};
use crate::explanation::{
    classify_query_type, clear_source_sentence_fields, determine_query_direction,
    is_context_sensitive_word, normalize_english_learning_target,
    normalize_model_english_learning_target, normalize_source_sentence_translation,
    validate_explanation_card, CaptureInput, ExplanationCard, QueryDirection, QueryType,
    MAX_CONTEXT_TEXT_LEN,
};
use crate::explanation_cache::{self, ExplanationCacheSpec};
use crate::learning_records;
use crate::model_usage::ModelUsageCategory;
use futures_util::future::{
    AbortHandle, AbortRegistration, Abortable, BoxFuture, FutureExt, Shared,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
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
const EXPLANATION_MODEL_REVISION: &str = "deepseek-chat-completions-thinking-disabled-v1";
const EXPLANATION_PROMPT_VERSION: &str = "explanation-card-directional-v7";

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

    #[cfg(test)]
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
    #[cfg(test)]
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

fn cache_authority_for_request(request: Arc<ExplanationRequestGuard>) -> WaiterCacheAuthority {
    Arc::new(move |operation| {
        request
            .commit_if_current(|| {
                operation();
                Ok(())
            })
            .is_ok()
    })
}

static EXPLANATION_REQUEST_AUTHORITY: OnceLock<Arc<ExplanationRequestAuthority>> = OnceLock::new();

fn explanation_request_authority() -> Arc<ExplanationRequestAuthority> {
    Arc::clone(EXPLANATION_REQUEST_AUTHORITY.get_or_init(|| Arc::new(Default::default())))
}

type SharedProviderResult = Result<Arc<ExplanationCard>, Arc<str>>;
type SharedProviderFuture = Shared<BoxFuture<'static, SharedProviderResult>>;
type WaiterCacheAuthority = Arc<dyn Fn(&mut dyn FnMut()) -> bool + Send + Sync>;

struct InFlightEntry {
    flight_id: u64,
    waiters: HashMap<u64, WaiterCacheAuthority>,
    abort_handle: AbortHandle,
    future: SharedProviderFuture,
}

#[derive(Default)]
struct ExplanationSingleFlightState {
    next_flight_id: u64,
    next_waiter_id: u64,
    entries: HashMap<String, InFlightEntry>,
}

#[derive(Default)]
struct ExplanationSingleFlight {
    state: Mutex<ExplanationSingleFlightState>,
}

impl ExplanationSingleFlight {
    #[cfg(test)]
    fn acquire<Factory, ProviderFuture>(
        self: &Arc<Self>,
        cache_key: String,
        provider: Factory,
    ) -> Result<ExplanationFlightWaiter, String>
    where
        Factory: FnOnce(Arc<Self>, String, u64) -> ProviderFuture + Send + 'static,
        ProviderFuture: Future<Output = Result<ExplanationCard, String>> + Send + 'static,
    {
        self.acquire_with_authority(
            cache_key,
            Arc::new(|operation| {
                operation();
                true
            }),
            provider,
        )
    }

    fn acquire_with_authority<Factory, ProviderFuture>(
        self: &Arc<Self>,
        cache_key: String,
        waiter_authority: WaiterCacheAuthority,
        provider: Factory,
    ) -> Result<ExplanationFlightWaiter, String>
    where
        Factory: FnOnce(Arc<Self>, String, u64) -> ProviderFuture + Send + 'static,
        ProviderFuture: Future<Output = Result<ExplanationCard, String>> + Send + 'static,
    {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "ExplanationCard single-flight 状态不可用。".to_string())?;
        let waiter_id = state.next_waiter_id.checked_add(1).ok_or_else(|| {
            "ExplanationCard single-flight waiter generation 已耗尽。".to_string()
        })?;
        state.next_waiter_id = waiter_id;
        if let Some(entry) = state.entries.get_mut(&cache_key) {
            entry.waiters.insert(waiter_id, waiter_authority);
            return Ok(ExplanationFlightWaiter {
                manager: Arc::clone(self),
                cache_key,
                flight_id: entry.flight_id,
                waiter_id,
                future: entry.future.clone(),
                released: false,
            });
        }

        let flight_id = state
            .next_flight_id
            .checked_add(1)
            .ok_or_else(|| "ExplanationCard single-flight generation 已耗尽。".to_string())?;
        state.next_flight_id = flight_id;
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let manager_for_provider = Arc::clone(self);
        let key_for_provider = cache_key.clone();
        let future = async move {
            match Abortable::new(
                provider(manager_for_provider, key_for_provider, flight_id),
                abort_registration,
            )
            .await
            {
                Ok(Ok(card)) => Ok(Arc::new(card)),
                Ok(Err(error)) => Err(Arc::<str>::from(error)),
                Err(_) => Err(Arc::<str>::from(cancelled_request_error())),
            }
        }
        .boxed()
        .shared();
        state.entries.insert(
            cache_key.clone(),
            InFlightEntry {
                flight_id,
                waiters: HashMap::from([(waiter_id, waiter_authority)]),
                abort_handle,
                future: future.clone(),
            },
        );
        drop(state);

        let manager_for_cleanup = Arc::clone(self);
        let key_for_cleanup = cache_key.clone();
        let cleanup_future = future.clone();
        tauri::async_runtime::spawn(async move {
            let _ = cleanup_future.await;
            manager_for_cleanup.finish(&key_for_cleanup, flight_id);
        });

        Ok(ExplanationFlightWaiter {
            manager: Arc::clone(self),
            cache_key,
            flight_id,
            waiter_id,
            future,
            released: false,
        })
    }

    fn has_waiters(&self, cache_key: &str, flight_id: u64) -> bool {
        self.state
            .lock()
            .ok()
            .and_then(|state| {
                state
                    .entries
                    .get(cache_key)
                    .map(|entry| entry.flight_id == flight_id && !entry.waiters.is_empty())
            })
            .unwrap_or(false)
    }

    fn commit_if_current_waiter(
        &self,
        cache_key: &str,
        flight_id: u64,
        mut operation: impl FnMut(),
    ) -> bool {
        let state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return false,
        };
        let Some(entry) = state
            .entries
            .get(cache_key)
            .filter(|entry| entry.flight_id == flight_id)
        else {
            return false;
        };
        for authority in entry.waiters.values() {
            if authority(&mut operation) {
                return true;
            }
        }
        false
    }

    fn release(&self, cache_key: &str, flight_id: u64, waiter_id: u64) {
        let abort_handle = self.state.lock().ok().and_then(|mut state| {
            let entry = state.entries.get_mut(cache_key)?;
            if entry.flight_id != flight_id {
                return None;
            }
            entry.waiters.remove(&waiter_id);
            entry
                .waiters
                .is_empty()
                .then(|| state.entries.remove(cache_key))
                .flatten()
                .map(|entry| entry.abort_handle)
        });
        if let Some(abort_handle) = abort_handle {
            abort_handle.abort();
        }
    }

    fn finish(&self, cache_key: &str, flight_id: u64) {
        if let Ok(mut state) = self.state.lock() {
            let matches = state
                .entries
                .get(cache_key)
                .is_some_and(|entry| entry.flight_id == flight_id);
            if matches {
                state.entries.remove(cache_key);
            }
        }
    }
}

struct ExplanationFlightWaiter {
    manager: Arc<ExplanationSingleFlight>,
    cache_key: String,
    flight_id: u64,
    waiter_id: u64,
    future: SharedProviderFuture,
    released: bool,
}

impl ExplanationFlightWaiter {
    async fn wait(mut self) -> Result<ExplanationCard, String> {
        let result = self
            .future
            .clone()
            .await
            .map(|card| (*card).clone())
            .map_err(|error| error.to_string());
        self.release();
        result
    }

    fn release(&mut self) {
        if !self.released {
            self.manager
                .release(&self.cache_key, self.flight_id, self.waiter_id);
            self.released = true;
        }
    }
}

impl Drop for ExplanationFlightWaiter {
    fn drop(&mut self) {
        self.release();
    }
}

static EXPLANATION_SINGLE_FLIGHT: OnceLock<Arc<ExplanationSingleFlight>> = OnceLock::new();

fn explanation_single_flight() -> Arc<ExplanationSingleFlight> {
    Arc::clone(EXPLANATION_SINGLE_FLIGHT.get_or_init(|| Arc::new(Default::default())))
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

fn friendly_explanation_error(error: &str) -> String {
    if error == EXPLANATION_REQUEST_CANCELLED {
        return error.to_string();
    }
    eprintln!("READRAY_EXPLANATION_FAILED={error}");
    classify_explanation_error(error).to_string()
}

fn classify_explanation_error(error: &str) -> &'static str {
    if error.contains("未配置 DeepSeek API Key") {
        "未配置 DeepSeek API Key，请在“设置 → AI 服务”中完成配置。"
    } else if error.contains("无法读取 DeepSeek API Key") {
        "无法读取已保存的 DeepSeek API Key，请在“设置 → AI 服务”中重新保存。"
    } else if error.contains("HTTP 401") || error.contains("HTTP 403") {
        "DeepSeek API Key 无效或已过期，请在“设置 → AI 服务”中更新。"
    } else if error.contains("HTTP 402") {
        "DeepSeek 账户余额不足，请充值后重试。"
    } else if error.contains("HTTP 429") {
        "请求过于频繁，请稍等片刻再试。"
    } else if error.contains("超时") {
        "解释生成超时，请重试。"
    } else if error.contains("网络错误") || error.contains("请求失败") {
        "网络连接出现问题，请检查网络后重试。"
    } else if error.contains("保存失败") {
        "解释已生成，但保存学习记录失败，请重试。"
    } else if error.contains("模型输出错误")
        || error.contains("schema 错误")
        || error.contains("usage")
    {
        "这次没能生成有效的解释，请重试。"
    } else {
        "暂时无法生成解释，请重试。"
    }
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
    let (request, abort_registration) = authority
        .register(request_scope, request_key)
        .map_err(|error| friendly_explanation_error(&error))?;
    let request = Arc::new(request);
    let provider_request = resolve_explanation_card(
        app.clone(),
        input.clone(),
        minimal_context_text,
        cache_authority_for_request(Arc::clone(&request)),
    );
    let card = Abortable::new(provider_request, abort_registration)
        .await
        .map_err(|_| cancelled_request_error())?
        .map_err(|error| friendly_explanation_error(&error))?;
    request
        .commit_if_current(|| {
            learning_records::save_for_app(&app, &input, &card)
                .map(|_| ())
                .map_err(|error| format!("ExplanationCard 保存失败：{error}"))
        })
        .map_err(|error| friendly_explanation_error(&error))?;

    Ok(card)
}

async fn resolve_explanation_card(
    app: tauri::AppHandle,
    input: CaptureInput,
    minimal_context_text: Option<String>,
    waiter_authority: WaiterCacheAuthority,
) -> Result<ExplanationCard, String> {
    let model_context_text = select_model_context(&input, minimal_context_text.as_deref());
    let spec = ExplanationCacheSpec::new(
        &input,
        model_context_text.as_deref(),
        configured_model(),
        EXPLANATION_MODEL_REVISION,
        EXPLANATION_PROMPT_VERSION,
    )?;
    let single_flight = explanation_single_flight();
    let cache_key = spec.cache_key.clone();
    let provider_app = app.clone();
    let provider_spec = spec.clone();
    let provider_input = CaptureInput {
        query_text: input.query_text.clone(),
        context_text: spec.minimal_context_text.clone(),
        source_type: input.source_type.clone(),
        source_app: None,
    };
    let waiter = single_flight.acquire_with_authority(
        cache_key,
        waiter_authority,
        move |manager, flight_cache_key, flight_id| async move {
            if let Some(card) =
                explanation_cache::lookup_for_app(&provider_app, &provider_spec, &provider_input)
                    .await
            {
                return Ok(card);
            }
            let card = create_explanation_card_for_input(
                &provider_app,
                &provider_input,
                provider_spec.minimal_context_text.as_deref(),
                || manager.has_waiters(&flight_cache_key, flight_id),
            )
            .await?;
            let cache_written =
                manager.commit_if_current_waiter(&flight_cache_key, flight_id, || {
                    explanation_cache::upsert_for_app_fail_open(
                        &provider_app,
                        &provider_spec,
                        &provider_input,
                        &card,
                    );
                });
            if !cache_written {
                return Err(cancelled_request_error());
            }
            explanation_cache::schedule_maintenance_for_app(&provider_app);
            Ok(card)
        },
    )?;
    let shared_card = waiter.wait().await?;
    explanation_cache::rebind_and_validate_card(&input, &spec, shared_card)
}

fn select_model_context(
    input: &CaptureInput,
    minimal_context_text: Option<&str>,
) -> Option<String> {
    let is_context_sensitive_query =
        classify_query_type(&input.query_text)
            .ok()
            .is_some_and(|query_type| {
                query_type == QueryType::Word && is_context_sensitive_word(&input.query_text)
            });
    if !is_context_sensitive_query {
        return minimal_context_text.map(str::to_string);
    }

    let full_context = input
        .context_text
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .filter(|value| value.chars().count() <= MAX_CONTEXT_TEXT_LEN);
    let minimal_context = minimal_context_text.filter(|value| !value.trim().is_empty());
    let query_text = input.query_text.trim();

    match (full_context, minimal_context) {
        (Some(full), Some(minimal))
            if full.chars().count() > minimal.chars().count() && full.contains(query_text) =>
        {
            Some(full.to_string())
        }
        (Some(_), Some(minimal)) => Some(minimal.to_string()),
        (Some(full), None) if full.contains(query_text) => Some(full.to_string()),
        (Some(_), None) => None,
        (None, None) => None,
        (None, Some(minimal)) => Some(minimal.to_string()),
    }
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
    let query_direction = determine_query_direction(&input.query_text)?;
    let model = configured_model();

    let user_prompt = build_user_prompt(input, minimal_context_text)?;
    Ok(json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": explanation_card_system_prompt(query_type, query_direction)
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
    let expected_query_type = classify_query_type(&input.query_text)?;
    let query_direction = determine_query_direction(&input.query_text)?;
    if query_direction == QueryDirection::EnToZh {
        object.insert(
            "learningTargetText".to_string(),
            serde_json::Value::String(normalize_english_learning_target(&input.query_text)?),
        );
    }
    normalize_model_query_type(object, expected_query_type);
    let mut card: ExplanationCard = serde_json::from_value(value)
        .map_err(|error| format!("ExplanationCard 模型输出错误：JSON 结构无效：{error}"))?;
    if query_direction == QueryDirection::ZhToEn {
        card.set_learning_target_text(normalize_model_english_learning_target(
            card.learning_target_text(),
        ));
        card.align_primary_result_with_learning_target();
    }
    normalize_source_sentence_translation(&mut card);
    if let Err(errors) = validate_explanation_card(input, &card) {
        let source_sentence_only =
            !errors.is_empty() && errors.iter().all(|error| error.contains("sourceSentence"));
        if source_sentence_only {
            let mut repaired = card.clone();
            if clear_source_sentence_fields(&mut repaired)
                && validate_explanation_card(input, &repaired).is_ok()
            {
                let query_digest: String = input.query_text.chars().take(80).collect();
                eprintln!(
                    "READRAY_EXPLANATION_SOURCE_SENTENCE_DROPPED=query={query_digest} original_errors={}",
                    summarize_validation_errors(&errors)
                );
                return Ok(repaired);
            }
        }
        return Err(format!(
            "ExplanationCard schema 错误：{}",
            summarize_validation_errors(&errors)
        ));
    }

    Ok(card)
}

fn normalize_model_query_type(
    object: &mut serde_json::Map<String, serde_json::Value>,
    expected_query_type: QueryType,
) {
    if expected_query_type != QueryType::Word {
        return;
    }

    let is_word_alias = matches!(
        object.get("queryType").and_then(|value| value.as_str()),
        Some("abbreviation" | "acronym" | "initialism")
    );
    if is_word_alias {
        object.insert(
            "queryType".to_string(),
            serde_json::Value::String("word".to_string()),
        );
    }
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
}

fn build_user_prompt(
    input: &CaptureInput,
    minimal_context_text: Option<&str>,
) -> Result<String, String> {
    let model_input = ModelCaptureInput {
        query_text: &input.query_text,
        context_text: minimal_context_text,
    };
    let input_json = serde_json::to_string_pretty(&model_input)
        .map_err(|error| format!("CaptureInput 无法序列化为 JSON：{error}"))?;

    Ok(format!("CaptureInput JSON:\n{input_json}"))
}

fn explanation_card_system_prompt(
    query_type: QueryType,
    query_direction: QueryDirection,
) -> String {
    let schema = match query_type {
        QueryType::Word => {
            r#"{
  "queryType": "word",
  "sourceText": "market",
  "learningTargetText": "market",
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
- Standalone uppercase abbreviations, acronyms, initialisms, and code/product identifiers are word-like entries. Always use queryType "word" for them; never invent "abbreviation", "acronym", or "initialism" as a queryType.
- For a context-sensitive abbreviation or identifier, inspect all of contextText, including later sentences and other occurrences of the token. Use a later, more specific role, product, organization, or domain clue to resolve the selected token when one is present; do not default to the most common expansion or report that the meaning is unknown when the context provides such a clue.
- Prefer the contextual sense. contextMeaning is allowed only with nonblank contextText (max 800 chars).
- Copy the contextual sentence that best supports the resolved sense, even when it is later than the selected occurrence. sourceSentence may appear without sourceSentenceZh.
- Provide sourceSentenceZh (max 2400) only when sourceSentence (max 1200) contains no Chinese characters at all (a pure English/Latin sentence). If sourceSentence contains any Chinese characters, including mixed text with terms such as Rust/generation or GLM Coding, sourceSentenceZh must be null. sourceSentenceZh is never allowed without sourceSentence.
- partOfSpeech and phonetic may be null. Code identifiers such as anchorRect should normally use null phonetic.
- basicMeanings requires 1-4 concise Chinese meanings (max 400 chars each).
- phrases and nearMeanings have at most 3 useful items each; examples have at most 2 bilingual items and may be empty.
- Omit optional content rather than fabricate it."#
        }
        QueryType::Phrase => {
            r#"{
  "queryType": "phrase",
  "sourceText": "in progress",
  "learningTargetText": "in progress",
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
- Provide sourceSentenceZh (max 2400) only when sourceSentence (max 1200) contains no Chinese characters at all (a pure English/Latin sentence). If sourceSentence contains any Chinese characters, including mixed text with terms such as Rust/generation or GLM Coding, sourceSentenceZh must be null. sourceSentenceZh is never allowed without sourceSentence.
- examples has at most 2 bilingual items and may be empty. Do not fabricate content or split the phrase into word-card fields."#
        }
        QueryType::Sentence => {
            r#"{
  "queryType": "sentence",
  "sourceText": "The window remains beside the selected text.",
  "learningTargetText": "The window remains beside the selected text.",
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
  "learningTargetText": "The first sentence explains the state. The second describes the next action.",
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

    let direction_rules = match query_direction {
        QueryDirection::EnToZh => {
            r#"Direction is enToZh. Explain or translate the selected English into Chinese.
learningTargetText is required but Rust will replace it with the deterministic normalized English query; do not use Chinese context to rewrite or translate the English target."#
        }
        QueryDirection::ZhToEn => {
            r#"Direction is zhToEn. Translate the complete selected Chinese into natural, idiomatic English suitable as a learning target.
learningTargetText is required, must contain useful Latin English, and must contain no Chinese. sourceText remains the original Chinese selection.
For word, learningTargetText and headword are the natural English result. For phrase, learningTargetText and basicMeaning are the natural English result. For sentence or paragraph, learningTargetText and translation are the complete natural English result.
Do not answer with a language-direction choice, commentary, field name, placeholder, or transliteration when a natural English translation is available."#
        }
    };

    format!(
        r#"Create one ReadRay ExplanationCard JSON object matching the schema below: no Markdown, code fences, commentary, or extra object.
The only allowed queryType values are "word", "phrase", "sentence", and "paragraph". queryType is fixed by the local classifier; keep the exact camelCase property names shown.
{direction_rules}
Use concise supporting explanations and natural complete translations in the requested direction. Never claim dictionary authority or invent unsupported facts.

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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::task::{Poll, Waker};

    #[derive(Default)]
    struct AsyncGate {
        open: AtomicBool,
        waker: Mutex<Option<Waker>>,
    }

    impl AsyncGate {
        async fn wait(&self) {
            futures_util::future::poll_fn(|context| {
                if self.open.load(Ordering::Acquire) {
                    Poll::Ready(())
                } else {
                    *self.waker.lock().unwrap() = Some(context.waker().clone());
                    Poll::Pending
                }
            })
            .await;
        }

        fn open(&self) {
            self.open.store(true, Ordering::Release);
            if let Some(waker) = self.waker.lock().unwrap().take() {
                waker.wake();
            }
        }
    }

    fn input(query_text: &str, context_text: Option<&str>) -> CaptureInput {
        CaptureInput {
            query_text: query_text.to_string(),
            context_text: context_text.map(str::to_string),
            source_type: SourceType::Manual,
            source_app: None,
        }
    }

    #[test]
    fn friendly_error_classifier_tracks_producer_messages() {
        // 这些字符串是 deepseek_client::ChatCompletionRequestError::product_message
        // 的真实输出格式；classifier 依赖它们做子串匹配，改动 producer 文案时
        // 此测试会失败，提醒同步更新分类。
        let cases = [
            ("DeepSeek ExplanationCard 超时：请重试。", "解释生成超时，请重试。"),
            (
                "DeepSeek ExplanationCard 网络错误：连接或响应读取失败。",
                "网络连接出现问题，请检查网络后重试。",
            ),
            (
                "DeepSeek ExplanationCard 网络错误：服务返回 HTTP 401。",
                "DeepSeek API Key 无效或已过期，请在“设置 → AI 服务”中更新。",
            ),
            (
                "DeepSeek ExplanationCard 网络错误：服务返回 HTTP 402。",
                "DeepSeek 账户余额不足，请充值后重试。",
            ),
            (
                "DeepSeek ExplanationCard 网络错误：服务返回 HTTP 429。",
                "请求过于频繁，请稍等片刻再试。",
            ),
            (
                "未配置 DeepSeek API Key，无法执行 DeepSeek ExplanationCard。",
                "未配置 DeepSeek API Key，请在“设置 → AI 服务”中完成配置。",
            ),
            (
                "ExplanationCard schema 错误：explanationCard.sourceSentenceZh 只有在 sourceSentence 存在时才允许提供。",
                "这次没能生成有效的解释，请重试。",
            ),
            ("其他未知错误。", "暂时无法生成解释，请重试。"),
        ];
        for (producer, expected) in cases {
            assert_eq!(
                classify_explanation_error(producer),
                expected,
                "for: {producer}"
            );
        }
    }

    fn shared_test_card() -> ExplanationCard {
        parse_explanation_card_content(
            &input("market", None),
            r#"{
              "queryType": "word",
              "sourceText": "market",
              "learningTargetText": "market",
              "headword": "market",
              "partOfSpeech": null,
              "phonetic": null,
              "basicMeanings": ["市场"],
              "contextMeaning": null,
              "sourceSentence": null,
              "sourceSentenceZh": null,
              "phrases": [],
              "nearMeanings": [],
              "examples": [],
              "reviewHint": null
            }"#,
        )
        .unwrap()
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
    fn chinese_response_keeps_source_and_validates_model_english_target() {
        let chinese = input("界面", Some("这个界面支持本地学习记录。"));
        let content = json!({
            "queryType": "word",
            "sourceText": "model must not replace authority",
            "learningTargetText": "interface",
            "headword": "interface",
            "basicMeanings": ["界面"],
            "phrases": [],
            "nearMeanings": [],
            "examples": []
        })
        .to_string();
        let card = parse_explanation_card_content(&chinese, &content).unwrap();
        assert_eq!(card.source_text(), "界面");
        assert_eq!(card.learning_target_text(), "interface");

        let invalid = content.replace("interface", "中文目标");
        let error = parse_explanation_card_content(&chinese, &invalid).unwrap_err();
        assert!(error.contains("有效拉丁英文"));
    }

    #[test]
    fn chinese_response_normalizes_target_before_validation_and_alignment() {
        let chinese = input("用户界面", None);
        let content = json!({
            "queryType": "word",
            "sourceText": "用户界面",
            "learningTargetText": "  user   interface  ",
            "headword": "model value before alignment",
            "basicMeanings": ["用户界面"],
            "phrases": [],
            "nearMeanings": [],
            "examples": []
        })
        .to_string();

        let card = parse_explanation_card_content(&chinese, &content).unwrap();
        assert_eq!(card.learning_target_text(), "user interface");
        assert!(matches!(
            card,
            ExplanationCard::Word { ref headword, .. } if headword == "user interface"
        ));
    }

    #[test]
    fn english_target_is_overwritten_locally_and_chinese_prompt_is_directional() {
        let english = input("fine-tuning", Some("中文上下文不能改变英文目标。"));
        let content = json!({
            "queryType": "word",
            "sourceText": "wrong",
            "learningTargetText": "模型改写",
            "headword": "fine-tuning",
            "basicMeanings": ["微调"],
            "phrases": [],
            "nearMeanings": [],
            "examples": []
        })
        .to_string();
        let card = parse_explanation_card_content(&english, &content).unwrap();
        assert_eq!(card.learning_target_text(), "fine-tuning");

        let prompt = explanation_card_system_prompt(QueryType::Word, QueryDirection::ZhToEn);
        assert!(prompt.contains("Direction is zhToEn"));
        assert!(prompt.contains("natural, idiomatic English"));
        assert!(prompt.contains("sourceText remains the original Chinese selection"));
    }

    #[test]
    fn abbreviation_alias_is_normalized_to_word_for_word_queries() {
        let content = json!({
            "queryType": "abbreviation",
            "sourceText": "wrong",
            "learningTargetText": "wrong",
            "headword": "FDE",
            "partOfSpeech": "initialism",
            "phonetic": null,
            "basicMeanings": ["前线部署工程师"],
            "contextMeaning": null,
            "sourceSentence": null,
            "sourceSentenceZh": null,
            "phrases": [],
            "nearMeanings": [],
            "examples": [],
            "reviewHint": null
        })
        .to_string();

        let card = parse_explanation_card_content(&input("FDE", None), &content).unwrap();
        assert!(matches!(
            card,
            ExplanationCard::Word { ref headword, .. } if headword == "FDE"
        ));
    }

    #[test]
    fn word_prompt_forbids_separate_abbreviation_query_types() {
        let prompt = explanation_card_system_prompt(QueryType::Word, QueryDirection::EnToZh);
        assert!(prompt.contains("Always use queryType \"word\""));
        assert!(prompt.contains("never invent \"abbreviation\", \"acronym\", or \"initialism\""));
        assert!(prompt.contains("including later sentences and other occurrences"));
        assert!(prompt.contains("best supports the resolved sense"));
        assert!(prompt.contains("The only allowed queryType values are"));
    }

    #[test]
    fn context_sensitive_queries_prefer_the_more_complete_captured_context() {
        let full_context =
            "A note mentions XYZ first. In this context, XYZ refers to the deployment role.";
        let xyz_input = input("XYZ", Some(full_context));
        let selected =
            select_model_context(&xyz_input, Some("A note mentions XYZ first.")).unwrap();
        assert_eq!(selected, full_context);

        let ordinary_input = input("market", Some(full_context));
        let selected = select_model_context(&ordinary_input, Some("The market remained open."));
        assert_eq!(selected.as_deref(), Some("The market remained open."));
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
        assert!(query_position < context_position);
        assert!(!prompt.contains("\"sourceType\""));
        assert!(!prompt.contains("\"sourceApp\""));
    }

    #[test]
    fn word_and_phrase_prompts_define_asymmetric_source_sentence_translation() {
        for query_type in [QueryType::Word, QueryType::Phrase] {
            let prompt = explanation_card_system_prompt(query_type, QueryDirection::EnToZh);
            assert!(prompt.contains("sourceSentence may appear without sourceSentenceZh"));
            assert!(prompt.contains(
                "only when sourceSentence (max 1200) contains no Chinese characters at all"
            ));
            assert!(prompt.contains("sourceSentenceZh must be null"));
            assert!(prompt.contains("sourceSentenceZh is never allowed without sourceSentence"));
        }
    }

    #[test]
    fn english_source_sentence_without_translation_degrades_to_valid_card() {
        let context = "The request generation prevents an older result from replacing a newer one.";
        let content = json!({
            "queryType": "word",
            "sourceText": "generation",
            "headword": "generation",
            "basicMeanings": ["代次；生成"],
            "contextMeaning": "这里指请求的内部代次。",
            "sourceSentence": context,
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
                source_sentence: None,
                source_sentence_zh: None,
                ..
            }
        ));
    }

    #[test]
    fn non_source_sentence_schema_errors_still_fail_the_card() {
        let content = json!({
            "queryType": "word",
            "sourceText": "generation",
            "headword": "generation",
            "basicMeanings": [],
            "sourceSentence": "The request generation prevents an older result.",
            "phrases": [],
            "nearMeanings": [],
            "examples": []
        })
        .to_string();

        let error =
            parse_explanation_card_content(&input("generation", None), &content).unwrap_err();

        assert!(error.contains("schema 错误"));
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
    fn single_flight_calls_provider_once_and_leader_cancel_keeps_waiter_alive() {
        tauri::async_runtime::block_on(async {
            let manager = Arc::new(ExplanationSingleFlight::default());
            let gate = Arc::new(AsyncGate::default());
            let provider_calls = Arc::new(AtomicUsize::new(0));
            let usage_writes = Arc::new(AtomicUsize::new(0));

            let leader_gate = Arc::clone(&gate);
            let leader_calls = Arc::clone(&provider_calls);
            let leader_usage = Arc::clone(&usage_writes);
            let leader = manager
                .acquire("same-key".to_string(), move |_, _, _| async move {
                    leader_calls.fetch_add(1, Ordering::SeqCst);
                    leader_gate.wait().await;
                    leader_usage.fetch_add(1, Ordering::SeqCst);
                    Ok(shared_test_card())
                })
                .unwrap();
            let waiter = manager
                .acquire("same-key".to_string(), |_, _, _| async move {
                    panic!("same key must not start a second provider")
                })
                .unwrap();

            drop(leader);
            assert!(manager.has_waiters("same-key", waiter.flight_id));
            gate.open();
            let card = waiter.wait().await.unwrap();
            assert_eq!(card.source_text(), "market");
            assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
            assert_eq!(usage_writes.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn same_scope_fast_replacement_joins_before_cancelled_waiter_drops() {
        tauri::async_runtime::block_on(async {
            let manager = Arc::new(ExplanationSingleFlight::default());
            let authority = Arc::new(ExplanationRequestAuthority::default());
            let lookup_gate = Arc::new(AsyncGate::default());
            let provider_calls = Arc::new(AtomicUsize::new(0));
            let cache_writes = Arc::new(AtomicUsize::new(0));
            let (request_a, abort_a) = authority
                .register(
                    ExplanationRequestScope::Anchored,
                    "anchored:same:1".to_string(),
                )
                .unwrap();
            let request_a = Arc::new(request_a);
            let shared_lookup_gate = Arc::clone(&lookup_gate);
            let shared_calls = Arc::clone(&provider_calls);
            let shared_writes = Arc::clone(&cache_writes);
            let waiter_a = manager
                .acquire_with_authority(
                    "same-cache-key".to_string(),
                    cache_authority_for_request(Arc::clone(&request_a)),
                    move |manager, key, flight_id| async move {
                        shared_lookup_gate.wait().await;
                        shared_calls.fetch_add(1, Ordering::SeqCst);
                        let committed = manager.commit_if_current_waiter(&key, flight_id, || {
                            shared_writes.fetch_add(1, Ordering::SeqCst);
                        });
                        if !committed {
                            return Err(cancelled_request_error());
                        }
                        Ok(shared_test_card())
                    },
                )
                .unwrap();

            let (request_b, _) = authority
                .register(
                    ExplanationRequestScope::Anchored,
                    "anchored:same:2".to_string(),
                )
                .unwrap();
            let request_b = Arc::new(request_b);
            // resolve 的关键不变量：B 在第一次 await 之前同步 acquire，
            // 因而 A 的 abort wake 尚未处理时 flight 已有两个 waiter。
            let waiter_b = manager
                .acquire_with_authority(
                    "same-cache-key".to_string(),
                    cache_authority_for_request(Arc::clone(&request_b)),
                    |_, _, _| async move {
                        panic!("same-scope replacement must join the existing flight")
                    },
                )
                .unwrap();
            assert!(manager.has_waiters("same-cache-key", waiter_b.flight_id));

            let aborted_a = Abortable::new(waiter_a.wait(), abort_a).await;
            assert!(aborted_a.is_err());
            assert!(manager.has_waiters("same-cache-key", waiter_b.flight_id));
            lookup_gate.open();
            let card = waiter_b.wait().await.unwrap();
            let saves = std::cell::Cell::new(0_u8);
            request_b
                .commit_if_current(|| {
                    let _ = card;
                    saves.set(saves.get() + 1);
                    Ok(())
                })
                .unwrap();
            assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
            assert_eq!(cache_writes.load(Ordering::SeqCst), 1);
            assert_eq!(saves.get(), 1);
        });
    }

    #[test]
    fn authority_cancel_before_waiter_drop_prevents_cache_commit() {
        tauri::async_runtime::block_on(async {
            let manager = Arc::new(ExplanationSingleFlight::default());
            let authority = Arc::new(ExplanationRequestAuthority::default());
            let provider_gate = Arc::new(AsyncGate::default());
            let cache_writes = Arc::new(AtomicUsize::new(0));
            let (request, _) = authority
                .register(
                    ExplanationRequestScope::Anchored,
                    "anchored:linearized-cancel".to_string(),
                )
                .unwrap();
            let request = Arc::new(request);
            let gate_for_provider = Arc::clone(&provider_gate);
            let writes_for_provider = Arc::clone(&cache_writes);
            let waiter = manager
                .acquire_with_authority(
                    "linearized-cache-key".to_string(),
                    cache_authority_for_request(Arc::clone(&request)),
                    move |manager, key, flight_id| async move {
                        gate_for_provider.wait().await;
                        let committed = manager.commit_if_current_waiter(&key, flight_id, || {
                            writes_for_provider.fetch_add(1, Ordering::SeqCst);
                        });
                        if !committed {
                            return Err(cancelled_request_error());
                        }
                        Ok(shared_test_card())
                    },
                )
                .unwrap();

            authority.cancel(
                ExplanationRequestScope::Anchored,
                Some("anchored:linearized-cancel"),
            );
            assert!(!request.is_current());
            // 故意保留 waiter，不依赖外层 Abortable 先调度到 Drop。
            assert!(manager.has_waiters("linearized-cache-key", waiter.flight_id));
            provider_gate.open();
            assert_eq!(
                waiter.wait().await.unwrap_err(),
                EXPLANATION_REQUEST_CANCELLED
            );
            assert_eq!(cache_writes.load(Ordering::SeqCst), 0);
        });
    }

    #[test]
    fn cancelled_waiter_cannot_save_while_valid_waiter_saves_shared_result() {
        tauri::async_runtime::block_on(async {
            let manager = Arc::new(ExplanationSingleFlight::default());
            let authority = Arc::new(ExplanationRequestAuthority::default());
            let gate = Arc::new(AsyncGate::default());
            let provider_calls = Arc::new(AtomicUsize::new(0));
            let (cancelled_request, _) = authority
                .register(
                    ExplanationRequestScope::Anchored,
                    "anchored:cancelled".to_string(),
                )
                .unwrap();
            let (valid_request, _) = authority
                .register(ExplanationRequestScope::Manual, "manual:valid".to_string())
                .unwrap();

            let provider_gate = Arc::clone(&gate);
            let calls = Arc::clone(&provider_calls);
            let cancelled_waiter = manager
                .acquire("shared-key".to_string(), move |_, _, _| async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    provider_gate.wait().await;
                    Ok(shared_test_card())
                })
                .unwrap();
            let valid_waiter = manager
                .acquire("shared-key".to_string(), |_, _, _| async move {
                    panic!("shared waiter must not start another provider")
                })
                .unwrap();

            authority.cancel(
                ExplanationRequestScope::Anchored,
                Some("anchored:cancelled"),
            );
            drop(cancelled_waiter);
            gate.open();
            let shared_card = valid_waiter.wait().await.unwrap();
            let save_count = std::cell::Cell::new(0_u8);
            assert_eq!(
                cancelled_request
                    .commit_if_current(|| {
                        save_count.set(save_count.get() + 1);
                        Ok(())
                    })
                    .unwrap_err(),
                EXPLANATION_REQUEST_CANCELLED
            );
            valid_request
                .commit_if_current(|| {
                    let _ = shared_card;
                    save_count.set(save_count.get() + 1);
                    Ok(())
                })
                .unwrap();
            assert_eq!(save_count.get(), 1);
            assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn failed_or_fully_cancelled_flight_does_not_leave_a_permanent_entry() {
        tauri::async_runtime::block_on(async {
            let manager = Arc::new(ExplanationSingleFlight::default());
            let failed = manager
                .acquire("retry-key".to_string(), |_, _, _| async move {
                    Err("provider failed".to_string())
                })
                .unwrap();
            assert_eq!(failed.wait().await.unwrap_err(), "provider failed");

            let retry = manager
                .acquire("retry-key".to_string(), |_, _, _| async move {
                    Ok(shared_test_card())
                })
                .unwrap();
            assert_eq!(retry.wait().await.unwrap().source_text(), "market");

            let provider_started = Arc::new(AsyncGate::default());
            let provider_finish = Arc::new(AsyncGate::default());
            let cache_writes = Arc::new(AtomicUsize::new(0));
            let started_for_provider = Arc::clone(&provider_started);
            let finish_for_provider = Arc::clone(&provider_finish);
            let writes_for_provider = Arc::clone(&cache_writes);
            let cancelled = manager
                .acquire("cancel-key".to_string(), move |_, _, _| async move {
                    started_for_provider.open();
                    finish_for_provider.wait().await;
                    writes_for_provider.fetch_add(1, Ordering::SeqCst);
                    Ok(shared_test_card())
                })
                .unwrap();
            let flight_id = cancelled.flight_id;
            provider_started.wait().await;
            drop(cancelled);
            provider_finish.open();
            assert!(!manager.has_waiters("cancel-key", flight_id));
            assert_eq!(cache_writes.load(Ordering::SeqCst), 0);
            let replacement = manager
                .acquire("cancel-key".to_string(), |_, _, _| async move {
                    Ok(shared_test_card())
                })
                .unwrap();
            assert_ne!(replacement.flight_id, flight_id);
            assert_eq!(replacement.wait().await.unwrap().source_text(), "market");
        });
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
