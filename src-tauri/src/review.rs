use crate::deepseek_client::{configured_model, post_tracked_chat_completion};
use crate::learning_records::{
    database_path_for_app, get_learning_record_from_connection, open_database, unix_time_ms,
    LearningRecord,
};
use crate::model_usage::ModelUsageCategory;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;
use tauri::AppHandle;

const REVIEW_FEED_PAGE_SIZE: u32 = 12;
const MAX_REVIEW_FEED_PAGE_SIZE: u32 = 30;
const DAY_UNIX_MS: i64 = 24 * 60 * 60 * 1_000;
const MAX_REQUEST_KEY_CHARS: usize = 128;
const MAX_FEEDBACK_DETAIL_CHARS: usize = 1_000;
const MAX_GENERATED_CONTEXT_CHARS: usize = 600;
const MAX_GENERATED_TRANSLATION_CHARS: usize = 1_000;
const MAX_GENERATED_HINT_CHARS: usize = 300;
const REVIEW_CARD_MAX_TOKENS: u16 = 900;
const REVIEW_CARD_TEMPERATURE: f32 = 0.45;
const GENERATED_CARD_TTL_UNIX_MS: i64 = 30 * DAY_UNIX_MS;
const GENERATED_CARD_POOL_CAPACITY: i64 = 256;
const GENERATED_CARD_PER_RECORD_CAPACITY: i64 = 3;
const GENERATED_CARD_FAILURE_BACKOFF_BASE_UNIX_MS: i64 = 5 * 60 * 1_000;
const GENERATED_CARD_FAILURE_BACKOFF_MAX_UNIX_MS: i64 = DAY_UNIX_MS;
const MAX_GENERATION_FAILURE_ERROR_CHARS: usize = 1_000;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewReasonCode {
    ScheduledToday,
    NewRecord,
    ContinuedPractice,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewOutcome {
    Remembered,
    Forgotten,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewQualityPolarity {
    Up,
    Down,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewTarget {
    pub learning_target_id: i64,
    pub revision: i64,
    pub next_review_at_unix_ms: i64,
    pub attempt_count: i64,
    pub remembered_count: i64,
    pub forgotten_count: i64,
    pub success_streak: i64,
    pub last_reviewed_at_unix_ms: Option<i64>,
    pub last_outcome: Option<ReviewOutcome>,
    pub last_used_hint: Option<bool>,
    pub last_attempt_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewAttempt {
    pub id: i64,
    pub feed_item_id: i64,
    pub learning_record_id: i64,
    pub learning_target_id: i64,
    pub request_key: String,
    pub expected_revision: i64,
    pub target_revision: i64,
    pub outcome: ReviewOutcome,
    pub used_hint: bool,
    pub next_review_at_unix_ms: i64,
    pub created_at_unix_ms: i64,
    pub undone_at_unix_ms: Option<i64>,
    pub undo_request_key: Option<String>,
    pub undo_target_revision: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQualityFeedback {
    pub id: i64,
    pub feed_item_id: i64,
    pub learning_record_id: i64,
    pub generated_card_id: Option<i64>,
    pub revision: i64,
    pub active: bool,
    pub polarity: ReviewQualityPolarity,
    pub reason_codes: Vec<String>,
    pub detail: Option<String>,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedReviewCard {
    pub id: i64,
    pub learning_record_id: i64,
    pub learning_target_id: i64,
    pub variant_index: i64,
    pub english_context: String,
    pub english_context_zh: String,
    pub hint: String,
    pub model: String,
    pub created_at_unix_ms: i64,
    pub expires_at_unix_ms: i64,
    pub last_used_at_unix_ms: i64,
    pub use_count: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCardGenerationFailure {
    pub request_key: String,
    pub feed_item_id: i64,
    pub learning_record_id: i64,
    pub failure_count: i64,
    pub retry_after_unix_ms: i64,
    pub last_error: String,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFeedItem {
    pub id: i64,
    pub ordinal: i64,
    pub cycle_index: i64,
    pub reason_code: ReviewReasonCode,
    pub learning_record: LearningRecord,
    pub target: ReviewTarget,
    pub attempt: Option<ReviewAttempt>,
    pub quality_feedback: Option<ReviewQualityFeedback>,
    pub generated_card: Option<GeneratedReviewCard>,
    pub generation_failure: Option<ReviewCardGenerationFailure>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFeedPage {
    pub day_start_unix_ms: i64,
    pub day_end_unix_ms: i64,
    pub page_size: u32,
    pub items: Vec<ReviewFeedItem>,
    pub next_cursor: Option<i64>,
    pub can_continue: bool,
    pub completed_count: u32,
    pub remembered_count: u32,
    pub forgotten_count: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReviewOutcomeInput {
    pub feed_item_id: i64,
    pub learning_record_id: i64,
    pub learning_target_id: i64,
    pub expected_revision: i64,
    pub outcome: ReviewOutcome,
    pub used_hint: bool,
    pub request_key: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewOutcomeWriteResult {
    pub target: ReviewTarget,
    pub attempt: ReviewAttempt,
    pub can_continue: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFeedItemState {
    pub day_start_unix_ms: i64,
    pub day_end_unix_ms: i64,
    pub item: ReviewFeedItem,
    pub completed_count: u32,
    pub remembered_count: u32,
    pub forgotten_count: u32,
    pub can_continue: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReviewOutcomeInput {
    pub attempt_id: i64,
    pub feed_item_id: i64,
    pub learning_record_id: i64,
    pub learning_target_id: i64,
    pub expected_revision: i64,
    pub request_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReviewQualityFeedbackInput {
    pub feed_item_id: i64,
    pub learning_record_id: i64,
    pub card_context_key: String,
    pub expected_revision: Option<i64>,
    pub polarity: ReviewQualityPolarity,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    pub detail: Option<String>,
    pub request_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoReviewQualityFeedbackInput {
    pub feedback_id: i64,
    pub feed_item_id: i64,
    pub learning_record_id: i64,
    pub expected_revision: i64,
    pub request_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareReviewFeedCardInput {
    pub feed_item_id: i64,
    pub learning_record_id: i64,
    pub learning_target_id: i64,
    pub request_key: String,
    #[serde(default)]
    pub explicit_retry: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedReviewCardPayload {
    english_context: String,
    english_context_zh: String,
    hint: String,
}

#[derive(Debug, Deserialize)]
struct ReviewCardChatResponse {
    choices: Vec<ReviewCardChoice>,
}

#[derive(Debug, Deserialize)]
struct ReviewCardChoice {
    finish_reason: Option<String>,
    message: ReviewCardMessage,
}

#[derive(Debug, Deserialize)]
struct ReviewCardMessage {
    content: Option<String>,
}

#[derive(Clone, Debug)]
struct StoredTarget {
    target: ReviewTarget,
    previous_last_used_hint: Option<i64>,
}

struct ReviewStore {
    connection: Connection,
}

impl ReviewStore {
    fn open(path: &Path) -> Result<Self, String> {
        Ok(Self {
            connection: open_database(path)?,
        })
    }

    fn load_feed_page(
        &mut self,
        day_start_unix_ms: i64,
        day_end_unix_ms: i64,
        cursor: Option<i64>,
        page_size: u32,
        now_unix_ms: i64,
    ) -> Result<ReviewFeedPage, String> {
        validate_feed_request(day_start_unix_ms, day_end_unix_ms, cursor, page_size)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("复习 Feed 事务无法开始：{error}"))?;

        let mismatched_range: Option<i64> = transaction
            .query_row(
                "SELECT day_end_unix_ms FROM review_feed_items
                 WHERE day_start_unix_ms = ?1 AND day_end_unix_ms <> ?2 LIMIT 1",
                params![day_start_unix_ms, day_end_unix_ms],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("复习 Feed 日期范围读取失败：{error}"))?;
        if mismatched_range.is_some() {
            return Err("复习 Feed 的本地日期边界与已保存数据不一致。".to_string());
        }
        maintain_generated_card_pool(&transaction, now_unix_ms)?;
        ensure_feed_page(
            &transaction,
            day_start_unix_ms,
            day_end_unix_ms,
            cursor.unwrap_or(-1),
            page_size,
            now_unix_ms,
        )?;

        transaction
            .commit()
            .map_err(|error| format!("复习 Feed 事务提交失败：{error}"))?;
        read_feed_page(
            &self.connection,
            day_start_unix_ms,
            day_end_unix_ms,
            cursor.unwrap_or(-1),
            page_size,
            now_unix_ms,
        )
    }

    fn load_feed_item_state(
        &mut self,
        feed_item_id: i64,
        now_unix_ms: i64,
    ) -> Result<ReviewFeedItemState, String> {
        if feed_item_id <= 0 {
            return Err("复习 Feed 条目状态请求包含无效 ID。".to_string());
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("复习 Feed 条目状态事务无法开始：{error}"))?;
        maintain_generated_card_pool(&transaction, now_unix_ms)?;
        transaction
            .commit()
            .map_err(|error| format!("复习 Feed 条目状态事务提交失败：{error}"))?;
        read_feed_item_state(&self.connection, feed_item_id, now_unix_ms)
    }

    fn submit_outcome(
        &mut self,
        input: &SubmitReviewOutcomeInput,
        now_unix_ms: i64,
    ) -> Result<ReviewOutcomeWriteResult, String> {
        validate_request_key(&input.request_key)?;
        if input.feed_item_id <= 0
            || input.learning_record_id <= 0
            || input.learning_target_id <= 0
            || input.expected_revision < 0
        {
            return Err("复习结果请求包含无效的条目、记录或 revision。".to_string());
        }

        if let Some(attempt) = load_attempt_by_request_key(&self.connection, &input.request_key)? {
            ensure_attempt_matches_submission(&attempt, input)?;
            let target = load_target(&self.connection, input.learning_target_id)?
                .ok_or_else(|| "幂等复习结果对应的学习目标不存在。".to_string())?
                .target;
            let can_continue =
                feed_item_can_continue(&self.connection, input.feed_item_id, now_unix_ms)?;
            return Ok(ReviewOutcomeWriteResult {
                target,
                attempt,
                can_continue,
            });
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("复习结果事务无法开始：{error}"))?;
        let (item_record_id, item_target_id) =
            feed_item_identity(&transaction, input.feed_item_id)?;
        if item_record_id != input.learning_record_id || item_target_id != input.learning_target_id
        {
            return Err("复习结果的页面条目与学习记录/目标身份不一致。".to_string());
        }
        if active_attempt_for_feed_item(&transaction, input.feed_item_id)?.is_some() {
            return Err("这个复习 Feed 条目已经完成，请先撤销已有结果。".to_string());
        }
        let stored_target = load_target(&transaction, input.learning_target_id)?
            .ok_or_else(|| "复习结果对应的学习目标不存在。".to_string())?;
        if stored_target.target.revision != input.expected_revision {
            return Err(format!(
                "复习结果 revision 冲突：期望 {}，实际 {}。",
                input.expected_revision, stored_target.target.revision
            ));
        }

        let (next_review_at_unix_ms, success_streak) = schedule_next_review(
            input.outcome,
            input.used_hint,
            stored_target.target.success_streak,
            now_unix_ms,
        )?;
        let target_revision = stored_target.target.revision + 1;
        transaction
            .execute(
                "INSERT INTO review_feed_attempts (
                   feed_item_id, learning_record_id, learning_target_id, request_key, expected_revision,
                   target_revision, outcome, used_hint, next_review_at_unix_ms,
                   previous_next_review_at_unix_ms, previous_attempt_count,
                   previous_remembered_count, previous_forgotten_count,
                   previous_success_streak, previous_last_reviewed_at_unix_ms,
                   previous_last_outcome, previous_last_used_hint, previous_last_attempt_id,
                   created_at_unix_ms
                 ) VALUES (
                   ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                   ?13, ?14, ?15, ?16, ?17, ?18, ?19
                 )",
                params![
                    input.feed_item_id,
                    input.learning_record_id,
                    input.learning_target_id,
                    input.request_key,
                    input.expected_revision,
                    target_revision,
                    outcome_to_storage(input.outcome),
                    bool_to_integer(input.used_hint),
                    next_review_at_unix_ms,
                    stored_target.target.next_review_at_unix_ms,
                    stored_target.target.attempt_count,
                    stored_target.target.remembered_count,
                    stored_target.target.forgotten_count,
                    stored_target.target.success_streak,
                    stored_target.target.last_reviewed_at_unix_ms,
                    stored_target.target.last_outcome.map(outcome_to_storage),
                    stored_target.previous_last_used_hint,
                    stored_target.target.last_attempt_id,
                    now_unix_ms,
                ],
            )
            .map_err(|error| format!("复习 attempt 写入失败：{error}"))?;
        let attempt_id = transaction.last_insert_rowid();
        let remembered_increment = i64::from(input.outcome == ReviewOutcome::Remembered);
        let forgotten_increment = i64::from(input.outcome == ReviewOutcome::Forgotten);
        let affected = transaction
            .execute(
                "UPDATE learning_target_review_states
                 SET revision = ?1,
                     next_review_at_unix_ms = ?2,
                     attempt_count = attempt_count + 1,
                     remembered_count = remembered_count + ?3,
                     forgotten_count = forgotten_count + ?4,
                     success_streak = ?5,
                     last_reviewed_at_unix_ms = ?6,
                     last_outcome = ?7,
                     last_used_hint = ?8,
                     last_attempt_id = ?9,
                     updated_at_unix_ms = ?6
                 WHERE learning_target_id = ?10 AND revision = ?11",
                params![
                    target_revision,
                    next_review_at_unix_ms,
                    remembered_increment,
                    forgotten_increment,
                    success_streak,
                    now_unix_ms,
                    outcome_to_storage(input.outcome),
                    bool_to_integer(input.used_hint),
                    attempt_id,
                    input.learning_target_id,
                    input.expected_revision,
                ],
            )
            .map_err(|error| format!("复习目标写回失败：{error}"))?;
        if affected != 1 {
            return Err("复习结果写回时目标 revision 已变化。".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("复习结果事务提交失败：{error}"))?;

        Ok(ReviewOutcomeWriteResult {
            target: load_target(&self.connection, input.learning_target_id)?
                .ok_or_else(|| "复习结果提交后无法读取学习目标。".to_string())?
                .target,
            attempt: load_attempt(&self.connection, attempt_id)?
                .ok_or_else(|| "复习结果提交后无法读取 attempt。".to_string())?,
            can_continue: feed_item_can_continue(
                &self.connection,
                input.feed_item_id,
                now_unix_ms,
            )?,
        })
    }

    fn undo_outcome(
        &mut self,
        input: &UndoReviewOutcomeInput,
        now_unix_ms: i64,
    ) -> Result<ReviewOutcomeWriteResult, String> {
        validate_request_key(&input.request_key)?;
        if input.attempt_id <= 0
            || input.feed_item_id <= 0
            || input.learning_record_id <= 0
            || input.learning_target_id <= 0
            || input.expected_revision < 0
        {
            return Err("撤销复习结果请求包含无效身份或 revision。".to_string());
        }

        if let Some(attempt) =
            load_attempt_by_undo_request_key(&self.connection, &input.request_key)?
        {
            ensure_attempt_matches_undo(&attempt, input)?;
            let target = load_target(&self.connection, input.learning_target_id)?
                .ok_or_else(|| "幂等撤销对应的学习目标不存在。".to_string())?
                .target;
            let can_continue =
                feed_item_can_continue(&self.connection, input.feed_item_id, now_unix_ms)?;
            return Ok(ReviewOutcomeWriteResult {
                target,
                attempt,
                can_continue,
            });
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("撤销复习结果事务无法开始：{error}"))?;
        let stored_attempt = load_attempt(&transaction, input.attempt_id)?
            .ok_or_else(|| "要撤销的复习 attempt 不存在。".to_string())?;
        ensure_attempt_matches_undo(&stored_attempt, input)?;
        if stored_attempt.undone_at_unix_ms.is_some() {
            return Err("这次复习结果已经撤销。".to_string());
        }
        let stored_target = load_target(&transaction, input.learning_target_id)?
            .ok_or_else(|| "要撤销的学习目标不存在。".to_string())?;
        if stored_target.target.revision != input.expected_revision {
            return Err(format!(
                "撤销复习结果 revision 冲突：期望 {}，实际 {}。",
                input.expected_revision, stored_target.target.revision
            ));
        }
        let undo_target_revision = stored_target.target.revision + 1;
        let attempt_affected = transaction
            .execute(
                "UPDATE review_feed_attempts
                 SET undone_at_unix_ms = ?1,
                     undo_request_key = ?2,
                     undo_expected_revision = ?3,
                     undo_target_revision = ?4
                 WHERE id = ?5 AND undone_at_unix_ms IS NULL",
                params![
                    now_unix_ms,
                    input.request_key,
                    input.expected_revision,
                    undo_target_revision,
                    input.attempt_id,
                ],
            )
            .map_err(|error| format!("复习 attempt 撤销标记失败：{error}"))?;
        if attempt_affected != 1 {
            return Err("复习 attempt 已被其他请求撤销。".to_string());
        }
        recompute_target_after_undo(
            &transaction,
            input.learning_target_id,
            input.expected_revision,
            undo_target_revision,
            now_unix_ms,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("撤销复习结果事务提交失败：{error}"))?;

        Ok(ReviewOutcomeWriteResult {
            target: load_target(&self.connection, input.learning_target_id)?
                .ok_or_else(|| "撤销后无法读取学习目标。".to_string())?
                .target,
            attempt: load_attempt(&self.connection, input.attempt_id)?
                .ok_or_else(|| "撤销后无法读取 attempt。".to_string())?,
            can_continue: feed_item_can_continue(
                &self.connection,
                input.feed_item_id,
                now_unix_ms,
            )?,
        })
    }

    fn save_quality_feedback(
        &mut self,
        input: &SaveReviewQualityFeedbackInput,
        now_unix_ms: i64,
    ) -> Result<ReviewQualityFeedback, String> {
        let normalized = normalize_feedback_input(input)?;
        let input_json = serde_json::to_string(&normalized)
            .map_err(|error| format!("卡片反馈请求无法序列化：{error}"))?;
        if let Some(result) = load_quality_mutation_result(
            &self.connection,
            &normalized.request_key,
            "save",
            &input_json,
        )? {
            return Ok(result);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("卡片反馈事务无法开始：{error}"))?;
        let (item_learning_record_id, generated_card_id) =
            feed_item_feedback_identity(&transaction, normalized.feed_item_id)?;
        if item_learning_record_id != normalized.learning_record_id {
            return Err("卡片反馈的 Feed 条目与学习记录身份不一致。".to_string());
        }
        if feedback_context_key(generated_card_id) != normalized.card_context_key {
            return Err("卡片反馈对应的具体卡片语境已经变化。".to_string());
        }
        let current = load_quality_feedback(&transaction, normalized.feed_item_id)?;
        let result = match current {
            Some(current) => {
                if normalized.expected_revision != Some(current.revision) {
                    return Err(format!(
                        "卡片反馈 revision 冲突：期望 {:?}，实际 {}。",
                        normalized.expected_revision, current.revision
                    ));
                }
                let next_revision = current.revision + 1;
                transaction
                    .execute(
                        "UPDATE review_quality_feedback
                         SET revision = ?1, active = 1, polarity = ?2,
                             reason_codes_json = ?3, detail = ?4, updated_at_unix_ms = ?5
                         WHERE id = ?6 AND revision = ?7",
                        params![
                            next_revision,
                            polarity_to_storage(normalized.polarity),
                            serde_json::to_string(&normalized.reason_codes)
                                .map_err(|error| format!("卡片反馈原因无法序列化：{error}"))?,
                            normalized.detail,
                            now_unix_ms,
                            current.id,
                            current.revision,
                        ],
                    )
                    .map_err(|error| format!("卡片反馈更新失败：{error}"))?;
                load_quality_feedback(&transaction, normalized.feed_item_id)?
                    .ok_or_else(|| "卡片反馈更新后无法读取。".to_string())?
            }
            None => {
                if normalized.expected_revision.is_some() {
                    return Err("卡片反馈不存在，expectedRevision 必须为空。".to_string());
                }
                transaction
                    .execute(
                        "INSERT INTO review_quality_feedback (
                           feed_item_id, learning_record_id, generated_card_id, card_context_key,
                           revision, active, polarity, reason_codes_json,
                           detail, created_at_unix_ms, updated_at_unix_ms
                         ) VALUES (?1, ?2, ?3, ?4, 0, 1, ?5, ?6, ?7, ?8, ?8)",
                        params![
                            normalized.feed_item_id,
                            normalized.learning_record_id,
                            generated_card_id,
                            feedback_context_key(generated_card_id),
                            polarity_to_storage(normalized.polarity),
                            serde_json::to_string(&normalized.reason_codes)
                                .map_err(|error| format!("卡片反馈原因无法序列化：{error}"))?,
                            normalized.detail,
                            now_unix_ms,
                        ],
                    )
                    .map_err(|error| format!("卡片反馈写入失败：{error}"))?;
                load_quality_feedback(&transaction, normalized.feed_item_id)?
                    .ok_or_else(|| "卡片反馈写入后无法读取。".to_string())?
            }
        };
        save_quality_mutation(
            &transaction,
            &normalized.request_key,
            normalized.feed_item_id,
            normalized.learning_record_id,
            "save",
            &input_json,
            &result,
            now_unix_ms,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("卡片反馈事务提交失败：{error}"))?;
        Ok(result)
    }

    fn undo_quality_feedback(
        &mut self,
        input: &UndoReviewQualityFeedbackInput,
        now_unix_ms: i64,
    ) -> Result<ReviewQualityFeedback, String> {
        validate_request_key(&input.request_key)?;
        if input.feedback_id <= 0
            || input.feed_item_id <= 0
            || input.learning_record_id <= 0
            || input.expected_revision < 0
        {
            return Err("撤销卡片反馈请求包含无效身份或 revision。".to_string());
        }
        let input_json = serde_json::to_string(input)
            .map_err(|error| format!("撤销卡片反馈请求无法序列化：{error}"))?;
        if let Some(result) =
            load_quality_mutation_result(&self.connection, &input.request_key, "undo", &input_json)?
        {
            return Ok(result);
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("撤销卡片反馈事务无法开始：{error}"))?;
        let current = load_quality_feedback(&transaction, input.feed_item_id)?
            .ok_or_else(|| "要撤销的卡片反馈不存在。".to_string())?;
        if current.id != input.feedback_id || current.learning_record_id != input.learning_record_id
        {
            return Err("卡片反馈 ID 与 Feed 条目身份不匹配。".to_string());
        }
        if !current.active {
            return Err("这次卡片反馈已经撤销。".to_string());
        }
        if current.revision != input.expected_revision {
            return Err(format!(
                "撤销卡片反馈 revision 冲突：期望 {}，实际 {}。",
                input.expected_revision, current.revision
            ));
        }
        let next_revision = current.revision + 1;
        let affected = transaction
            .execute(
                "UPDATE review_quality_feedback
                 SET revision = ?1, active = 0, updated_at_unix_ms = ?2
                 WHERE id = ?3 AND revision = ?4 AND active = 1",
                params![next_revision, now_unix_ms, current.id, current.revision],
            )
            .map_err(|error| format!("卡片反馈撤销失败：{error}"))?;
        if affected != 1 {
            return Err("卡片反馈撤销时 revision 已变化。".to_string());
        }
        let result = load_quality_feedback(&transaction, input.feed_item_id)?
            .ok_or_else(|| "卡片反馈撤销后无法读取。".to_string())?;
        save_quality_mutation(
            &transaction,
            &input.request_key,
            input.feed_item_id,
            input.learning_record_id,
            "undo",
            &input_json,
            &result,
            now_unix_ms,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("撤销卡片反馈事务提交失败：{error}"))?;
        Ok(result)
    }
}

#[tauri::command]
pub fn get_review_feed_page(
    app: AppHandle,
    day_start_unix_ms: i64,
    day_end_unix_ms: i64,
    cursor: Option<i64>,
    page_size: Option<u32>,
) -> Result<ReviewFeedPage, String> {
    ReviewStore::open(&database_path_for_app(&app)?)?.load_feed_page(
        day_start_unix_ms,
        day_end_unix_ms,
        cursor,
        page_size.unwrap_or(REVIEW_FEED_PAGE_SIZE),
        unix_time_ms()?,
    )
}

#[tauri::command]
pub fn get_review_feed_item_state(
    app: AppHandle,
    feed_item_id: i64,
) -> Result<ReviewFeedItemState, String> {
    ReviewStore::open(&database_path_for_app(&app)?)?
        .load_feed_item_state(feed_item_id, unix_time_ms()?)
}

#[tauri::command]
pub async fn prepare_review_feed_card(
    app: AppHandle,
    input: PrepareReviewFeedCardInput,
) -> Result<GeneratedReviewCard, String> {
    prepare_generated_review_card(&app, &input).await
}

#[tauri::command]
pub fn submit_review_outcome(
    app: AppHandle,
    input: SubmitReviewOutcomeInput,
) -> Result<ReviewOutcomeWriteResult, String> {
    ReviewStore::open(&database_path_for_app(&app)?)?.submit_outcome(&input, unix_time_ms()?)
}

#[tauri::command]
pub fn undo_review_outcome(
    app: AppHandle,
    input: UndoReviewOutcomeInput,
) -> Result<ReviewOutcomeWriteResult, String> {
    ReviewStore::open(&database_path_for_app(&app)?)?.undo_outcome(&input, unix_time_ms()?)
}

#[tauri::command]
pub fn save_review_quality_feedback(
    app: AppHandle,
    input: SaveReviewQualityFeedbackInput,
) -> Result<ReviewQualityFeedback, String> {
    ReviewStore::open(&database_path_for_app(&app)?)?.save_quality_feedback(&input, unix_time_ms()?)
}

#[tauri::command]
pub fn undo_review_quality_feedback(
    app: AppHandle,
    input: UndoReviewQualityFeedbackInput,
) -> Result<ReviewQualityFeedback, String> {
    ReviewStore::open(&database_path_for_app(&app)?)?.undo_quality_feedback(&input, unix_time_ms()?)
}

fn validate_feed_request(
    day_start_unix_ms: i64,
    day_end_unix_ms: i64,
    cursor: Option<i64>,
    page_size: u32,
) -> Result<(), String> {
    if day_start_unix_ms < 0 || day_end_unix_ms <= day_start_unix_ms {
        return Err("复习 Feed 的本地日期范围无效。".to_string());
    }
    if cursor.is_some_and(|value| value < 0) {
        return Err("复习 Feed cursor 无效。".to_string());
    }
    if page_size == 0 || page_size > MAX_REVIEW_FEED_PAGE_SIZE {
        return Err(format!(
            "复习 Feed 每页数量必须在 1 到 {MAX_REVIEW_FEED_PAGE_SIZE} 之间。"
        ));
    }
    Ok(())
}

fn ensure_feed_page(
    transaction: &Transaction<'_>,
    day_start_unix_ms: i64,
    day_end_unix_ms: i64,
    cursor: i64,
    page_size: u32,
    now_unix_ms: i64,
) -> Result<(), String> {
    let total_targets: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM learning_targets target
             WHERE target.target_kind = 'learnable'
               AND EXISTS (
               SELECT 1 FROM learning_target_occurrences occurrence
               JOIN learning_records record ON record.id = occurrence.learning_record_id
               WHERE occurrence.learning_target_id = target.id
                 AND record.created_at_unix_ms <= ?1
             )",
            [now_unix_ms],
            |row| row.get(0),
        )
        .map_err(|error| format!("复习 Feed 学习目标数量读取失败：{error}"))?;
    if total_targets == 0 {
        return Ok(());
    }

    let mut cycle_index: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(cycle_index), 0)
             FROM review_feed_items WHERE day_start_unix_ms = ?1",
            [day_start_unix_ms],
            |row| row.get(0),
        )
        .map_err(|error| format!("复习 Feed 轮次读取失败：{error}"))?;

    loop {
        let available: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM review_feed_items fi
                 WHERE fi.day_start_unix_ms = ?1 AND fi.ordinal > ?2
                   AND fi.target_slot_active = 1",
                params![day_start_unix_ms, cursor],
                |row| row.get(0),
            )
            .map_err(|error| format!("复习 Feed 已准备数量读取失败：{error}"))?;
        let remaining = i64::from(page_size).saturating_sub(available);
        if remaining <= 0 {
            return Ok(());
        }

        let candidate_limit = remaining.saturating_add(1);
        let mut candidates = {
            let mut statement = transaction
                .prepare(
                    "SELECT target.id,
                            MIN(record.created_at_unix_ms) AS first_seen_at_unix_ms,
                            CASE
                              WHEN state.learning_target_id IS NULL OR state.attempt_count = 0 THEN 'new_record'
                              WHEN state.next_review_at_unix_ms < ?1 THEN 'scheduled_today'
                              ELSE 'continued_practice'
                            END AS reason_code,
                            CASE
                              WHEN state.attempt_count > 0 AND state.next_review_at_unix_ms < ?1 THEN 0
                              WHEN state.learning_target_id IS NULL OR state.attempt_count = 0 THEN 1
                              ELSE 2
                            END AS priority
                     FROM learning_targets target
                     JOIN learning_target_occurrences occurrence
                       ON occurrence.learning_target_id = target.id
                     JOIN learning_records record ON record.id = occurrence.learning_record_id
                     LEFT JOIN learning_target_review_states state
                       ON state.learning_target_id = target.id
                     WHERE target.target_kind = 'learnable'
                       AND record.created_at_unix_ms <= ?2
                       AND NOT EXISTS (
                         SELECT 1 FROM review_feed_items fi
                         WHERE fi.day_start_unix_ms = ?3
                           AND fi.cycle_index = ?4
                           AND fi.learning_target_id = target.id
                           AND fi.target_slot_active = 1
                       )
                     GROUP BY target.id
                     ORDER BY priority ASC,
                              COALESCE(state.next_review_at_unix_ms, MIN(record.created_at_unix_ms)) ASC,
                              MAX(record.created_at_unix_ms) DESC,
                              target.id DESC
                     LIMIT ?5",
                )
                .map_err(|error| format!("复习 Feed 候选语句无法准备：{error}"))?;
            let rows = statement
                .query_map(
                    params![
                        day_end_unix_ms,
                        now_unix_ms,
                        day_start_unix_ms,
                        cycle_index,
                        candidate_limit
                    ],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .map_err(|error| format!("复习 Feed 候选读取失败：{error}"))?;
            let mut values = Vec::new();
            for row in rows {
                values.push(row.map_err(|error| format!("复习 Feed 候选行读取失败：{error}"))?);
            }
            values
        };

        if candidates.is_empty() {
            let (cycle_item_count, completed_cycle_item_count): (i64, i64) = transaction
                .query_row(
                    "SELECT COUNT(*),
                            COALESCE(SUM(CASE WHEN EXISTS (
                              SELECT 1 FROM review_feed_attempts a
                              WHERE a.feed_item_id = fi.id AND a.undone_at_unix_ms IS NULL
                            ) THEN 1 ELSE 0 END), 0)
                     FROM review_feed_items fi
                     WHERE fi.day_start_unix_ms = ?1 AND fi.cycle_index = ?2
                       AND fi.target_slot_active = 1",
                    params![day_start_unix_ms, cycle_index],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|error| format!("复习 Feed 轮次完成状态读取失败：{error}"))?;
            if cycle_item_count == 0 || completed_cycle_item_count < cycle_item_count {
                return Ok(());
            }
            cycle_index = cycle_index
                .checked_add(1)
                .ok_or_else(|| "复习 Feed 轮次超出可保存范围。".to_string())?;
            continue;
        }

        let last_target_id: Option<i64> = transaction
            .query_row(
                "SELECT learning_target_id FROM review_feed_items
                 WHERE day_start_unix_ms = ?1 AND target_slot_active = 1
                 ORDER BY ordinal DESC, id DESC LIMIT 1",
                [day_start_unix_ms],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("复习 Feed 上一条身份读取失败：{error}"))?;
        if candidates.len() > 1 && last_target_id == Some(candidates[0].0) {
            candidates.rotate_left(1);
        }
        candidates.truncate(usize::try_from(remaining).unwrap_or(usize::MAX));

        let mut next_ordinal: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(ordinal), -1) + 1
                 FROM review_feed_items WHERE day_start_unix_ms = ?1",
                [day_start_unix_ms],
                |row| row.get(0),
            )
            .map_err(|error| format!("复习 Feed 序号读取失败：{error}"))?;

        for (learning_target_id, first_seen_at_unix_ms, reason_code) in candidates {
            let learning_record_id = select_occurrence_for_review_cycle(
                transaction,
                learning_target_id,
                cycle_index,
                now_unix_ms,
            )?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO learning_target_review_states (
                       learning_target_id, revision, next_review_at_unix_ms,
                       attempt_count, remembered_count, forgotten_count, success_streak,
                       created_at_unix_ms, updated_at_unix_ms
                     ) VALUES (?1, 0, ?2, 0, 0, 0, 0, ?2, ?3)",
                    params![learning_target_id, first_seen_at_unix_ms, now_unix_ms],
                )
                .map_err(|error| format!("复习目标初始化失败：{error}"))?;
            transaction
                .execute(
                    "INSERT INTO review_feed_items (
                       day_start_unix_ms, day_end_unix_ms, learning_record_id,
                       learning_target_id, cycle_index, ordinal, reason_code,
                       target_slot_active, created_at_unix_ms
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)",
                    params![
                        day_start_unix_ms,
                        day_end_unix_ms,
                        learning_record_id,
                        learning_target_id,
                        cycle_index,
                        next_ordinal,
                        reason_code,
                        now_unix_ms
                    ],
                )
                .map_err(|error| format!("复习 Feed 条目保存失败：{error}"))?;
            next_ordinal += 1;
        }
    }
}

fn select_occurrence_for_review_cycle(
    connection: &Connection,
    learning_target_id: i64,
    cycle_index: i64,
    now_unix_ms: i64,
) -> Result<i64, String> {
    let occurrence_count: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM learning_target_occurrences occurrence
             JOIN learning_records record ON record.id = occurrence.learning_record_id
             WHERE occurrence.learning_target_id = ?1
               AND record.created_at_unix_ms <= ?2",
            params![learning_target_id, now_unix_ms],
            |row| row.get(0),
        )
        .map_err(|error| format!("复习目标 occurrence 数量读取失败：{error}"))?;
    if occurrence_count <= 0 {
        return Err("复习目标没有可追溯的真实 occurrence。".to_string());
    }
    let offset = cycle_index.rem_euclid(occurrence_count);
    connection
        .query_row(
            "SELECT occurrence.learning_record_id
             FROM learning_target_occurrences occurrence
             JOIN learning_records record ON record.id = occurrence.learning_record_id
             WHERE occurrence.learning_target_id = ?1
               AND record.created_at_unix_ms <= ?2
             ORDER BY record.created_at_unix_ms DESC, record.id DESC
             LIMIT 1 OFFSET ?3",
            params![learning_target_id, now_unix_ms, offset],
            |row| row.get(0),
        )
        .map_err(|error| format!("复习目标 occurrence 轮换读取失败：{error}"))
}

fn read_feed_page(
    connection: &Connection,
    day_start_unix_ms: i64,
    day_end_unix_ms: i64,
    cursor: i64,
    page_size: u32,
    now_unix_ms: i64,
) -> Result<ReviewFeedPage, String> {
    let feed_rows = {
        let mut statement = connection
            .prepare(
                "SELECT fi.id, fi.ordinal, fi.cycle_index, fi.reason_code,
                        fi.learning_record_id, fi.learning_target_id, fi.generated_card_id
                 FROM review_feed_items fi
                 WHERE fi.day_start_unix_ms = ?1 AND fi.ordinal > ?2
                   AND fi.target_slot_active = 1
                 ORDER BY fi.ordinal ASC, fi.id ASC
                 LIMIT ?3",
            )
            .map_err(|error| format!("复习 Feed 读取语句无法准备：{error}"))?;
        let rows = statement
            .query_map(
                params![day_start_unix_ms, cursor, i64::from(page_size)],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                    ))
                },
            )
            .map_err(|error| format!("复习 Feed 读取失败：{error}"))?;
        let mut feed_rows = Vec::new();
        for row in rows {
            feed_rows.push(row.map_err(|error| format!("复习 Feed 行读取失败：{error}"))?);
        }
        feed_rows
    };

    let mut items = Vec::with_capacity(feed_rows.len());
    for (
        id,
        ordinal,
        cycle_index,
        reason_code,
        learning_record_id,
        learning_target_id,
        generated_card_id,
    ) in feed_rows
    {
        let learning_record = get_learning_record_from_connection(connection, learning_record_id)?
            .ok_or_else(|| format!("复习 Feed 条目 {id} 对应的学习记录不存在。"))?;
        if learning_record.learning_target_id != learning_target_id {
            return Err(format!(
                "复习 Feed 条目 {id} 的 occurrence 与目标身份不一致。"
            ));
        }
        let target = load_target(connection, learning_target_id)?
            .ok_or_else(|| format!("复习 Feed 条目 {id} 对应的学习目标不存在。"))?
            .target;
        items.push(ReviewFeedItem {
            id,
            ordinal,
            cycle_index,
            reason_code: reason_from_storage(&reason_code)?,
            learning_record,
            target,
            attempt: active_attempt_for_feed_item(connection, id)?,
            quality_feedback: load_quality_feedback(connection, id)?,
            generated_card: generated_card_id
                .map(|card_id| load_generated_card(connection, card_id))
                .transpose()?
                .flatten(),
            generation_failure: load_generation_failure_by_feed_item(connection, id)?,
        });
    }

    let (completed_count, remembered_count, forgotten_count): (i64, i64, i64) = connection
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN a.outcome = 'remembered' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN a.outcome = 'forgotten' THEN 1 ELSE 0 END), 0)
             FROM review_feed_attempts a
             JOIN review_feed_items fi ON fi.id = a.feed_item_id
             WHERE fi.day_start_unix_ms = ?1 AND a.undone_at_unix_ms IS NULL
               AND fi.target_slot_active = 1",
            [day_start_unix_ms],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| format!("复习 Feed 完成统计读取失败：{error}"))?;
    let next_cursor = items.last().map(|item| item.ordinal);
    let can_continue = feed_can_continue(
        connection,
        day_start_unix_ms,
        next_cursor.unwrap_or(cursor),
        now_unix_ms,
    )?;
    Ok(ReviewFeedPage {
        day_start_unix_ms,
        day_end_unix_ms,
        page_size,
        next_cursor,
        can_continue,
        completed_count: u32::try_from(completed_count)
            .map_err(|_| "复习 Feed 完成数量无效。".to_string())?,
        remembered_count: u32::try_from(remembered_count)
            .map_err(|_| "复习 Feed 想起数量无效。".to_string())?,
        forgotten_count: u32::try_from(forgotten_count)
            .map_err(|_| "复习 Feed 未想起数量无效。".to_string())?,
        items,
    })
}

fn read_feed_item_state(
    connection: &Connection,
    feed_item_id: i64,
    now_unix_ms: i64,
) -> Result<ReviewFeedItemState, String> {
    let (
        day_start_unix_ms,
        day_end_unix_ms,
        ordinal,
        cycle_index,
        reason_code,
        learning_record_id,
        learning_target_id,
        generated_card_id,
    ): (i64, i64, i64, i64, String, i64, i64, Option<i64>) = connection
        .query_row(
            "SELECT fi.day_start_unix_ms, fi.day_end_unix_ms, fi.ordinal, fi.cycle_index, fi.reason_code,
                    fi.learning_record_id, fi.learning_target_id, fi.generated_card_id
             FROM review_feed_items fi
             WHERE fi.id = ?1",
            [feed_item_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("复习 Feed 条目权威状态读取失败：{error}"))?
        .ok_or_else(|| "复习 Feed 条目不存在。".to_string())?;
    let learning_record = get_learning_record_from_connection(connection, learning_record_id)?
        .ok_or_else(|| "复习 Feed 条目的学习记录不存在。".to_string())?;
    if learning_record.learning_target_id != learning_target_id {
        return Err("复习 Feed 条目的 occurrence 与目标身份不一致。".to_string());
    }
    let target = load_target(connection, learning_target_id)?
        .ok_or_else(|| "复习 Feed 条目的学习目标不存在。".to_string())?
        .target;
    let item = ReviewFeedItem {
        id: feed_item_id,
        ordinal,
        cycle_index,
        reason_code: reason_from_storage(&reason_code)?,
        learning_record,
        target,
        attempt: active_attempt_for_feed_item(connection, feed_item_id)?,
        quality_feedback: load_quality_feedback(connection, feed_item_id)?,
        generated_card: generated_card_id
            .map(|card_id| load_generated_card(connection, card_id))
            .transpose()?
            .flatten(),
        generation_failure: load_generation_failure_by_feed_item(connection, feed_item_id)?,
    };
    let (completed_count, remembered_count, forgotten_count): (i64, i64, i64) = connection
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN a.outcome = 'remembered' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN a.outcome = 'forgotten' THEN 1 ELSE 0 END), 0)
             FROM review_feed_attempts a
             JOIN review_feed_items fi ON fi.id = a.feed_item_id
             WHERE fi.day_start_unix_ms = ?1 AND a.undone_at_unix_ms IS NULL
               AND fi.target_slot_active = 1",
            [day_start_unix_ms],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| format!("复习 Feed 条目完成统计读取失败：{error}"))?;
    Ok(ReviewFeedItemState {
        day_start_unix_ms,
        day_end_unix_ms,
        item,
        completed_count: u32::try_from(completed_count)
            .map_err(|_| "复习 Feed 完成数量无效。".to_string())?,
        remembered_count: u32::try_from(remembered_count)
            .map_err(|_| "复习 Feed 想起数量无效。".to_string())?,
        forgotten_count: u32::try_from(forgotten_count)
            .map_err(|_| "复习 Feed 未想起数量无效。".to_string())?,
        can_continue: feed_can_continue(connection, day_start_unix_ms, ordinal, now_unix_ms)?,
    })
}

fn feed_can_continue(
    connection: &Connection,
    day_start_unix_ms: i64,
    after_ordinal: i64,
    now_unix_ms: i64,
) -> Result<bool, String> {
    let has_saved_rows_after: bool = connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM review_feed_items fi
               WHERE fi.day_start_unix_ms = ?1 AND fi.ordinal > ?2
                 AND fi.target_slot_active = 1
             )",
            params![day_start_unix_ms, after_ordinal],
            |row| row.get(0),
        )
        .map_err(|error| format!("复习 Feed 后续条目状态读取失败：{error}"))?;
    if has_saved_rows_after {
        return Ok(true);
    }

    let cycle_index: Option<i64> = connection
        .query_row(
            "SELECT MAX(cycle_index) FROM review_feed_items WHERE day_start_unix_ms = ?1",
            [day_start_unix_ms],
            |row| row.get(0),
        )
        .map_err(|error| format!("复习 Feed 当前轮次读取失败：{error}"))?;
    let Some(cycle_index) = cycle_index else {
        return connection
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM learning_targets target
                   JOIN learning_target_occurrences occurrence ON occurrence.learning_target_id = target.id
                   JOIN learning_records record ON record.id = occurrence.learning_record_id
                   WHERE target.target_kind = 'learnable'
                     AND record.created_at_unix_ms <= ?1
                 )",
                [now_unix_ms],
                |row| row.get(0),
            )
            .map_err(|error| format!("复习 Feed 初始候选状态读取失败：{error}"));
    };

    let has_unqueued_record: bool = connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM learning_targets target
               WHERE target.target_kind = 'learnable'
                 AND EXISTS (
                 SELECT 1 FROM learning_target_occurrences occurrence
                 JOIN learning_records record ON record.id = occurrence.learning_record_id
                 WHERE occurrence.learning_target_id = target.id
                   AND record.created_at_unix_ms <= ?1
               )
                 AND NOT EXISTS (
                   SELECT 1 FROM review_feed_items fi
                   WHERE fi.day_start_unix_ms = ?2
                     AND fi.cycle_index = ?3
                     AND fi.learning_target_id = target.id
                     AND fi.target_slot_active = 1
                 )
             )",
            params![now_unix_ms, day_start_unix_ms, cycle_index],
            |row| row.get(0),
        )
        .map_err(|error| format!("复习 Feed 当前轮次剩余候选读取失败：{error}"))?;
    if has_unqueued_record {
        return Ok(true);
    }

    let (cycle_item_count, completed_cycle_item_count): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN EXISTS (
                      SELECT 1 FROM review_feed_attempts a
                      WHERE a.feed_item_id = fi.id AND a.undone_at_unix_ms IS NULL
                    ) THEN 1 ELSE 0 END), 0)
             FROM review_feed_items fi
             WHERE fi.day_start_unix_ms = ?1 AND fi.cycle_index = ?2
               AND fi.target_slot_active = 1",
            params![day_start_unix_ms, cycle_index],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| format!("复习 Feed 当前轮次完成状态读取失败：{error}"))?;
    Ok(cycle_item_count > 0 && completed_cycle_item_count == cycle_item_count)
}

fn feed_item_can_continue(
    connection: &Connection,
    feed_item_id: i64,
    now_unix_ms: i64,
) -> Result<bool, String> {
    let (day_start_unix_ms, ordinal): (i64, i64) = connection
        .query_row(
            "SELECT day_start_unix_ms, ordinal FROM review_feed_items WHERE id = ?1",
            [feed_item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| format!("复习 Feed 条目继续状态读取失败：{error}"))?
        .ok_or_else(|| "复习 Feed 条目不存在，无法读取继续状态。".to_string())?;
    feed_can_continue(connection, day_start_unix_ms, ordinal, now_unix_ms)
}

enum GeneratedCardPreflight {
    Ready(GeneratedReviewCard),
    Deferred {
        retry_after_unix_ms: i64,
        last_error: String,
    },
    Generate {
        learning_record: LearningRecord,
        variant_index: i64,
        day_start_unix_ms: i64,
    },
}

fn prepare_generated_card_preflight(
    path: &Path,
    input: &PrepareReviewFeedCardInput,
    now_unix_ms: i64,
) -> Result<GeneratedCardPreflight, String> {
    let mut connection = open_database(path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("AI 复习卡池事务无法开始：{error}"))?;
    maintain_generated_card_pool(&transaction, now_unix_ms)?;
    let (learning_record_id, learning_target_id, day_start_unix_ms, generated_card_id): (
        i64,
        i64,
        i64,
        Option<i64>,
    ) = transaction
        .query_row(
            "SELECT learning_record_id, learning_target_id, day_start_unix_ms, generated_card_id
                 FROM review_feed_items WHERE id = ?1",
            [input.feed_item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| format!("AI 复习卡 Feed 条目读取失败：{error}"))?
        .ok_or_else(|| "AI 复习卡对应的 Feed 条目不存在。".to_string())?;
    if learning_record_id != input.learning_record_id
        || learning_target_id != input.learning_target_id
    {
        return Err("AI 复习卡的 Feed 条目与学习记录/目标身份不一致。".to_string());
    }

    let mut ready_card_id = None;
    if let Some(card_id) = generated_card_id {
        if let Some(card) = load_generated_card(&transaction, card_id)? {
            if card.expires_at_unix_ms > now_unix_ms {
                ensure_generated_card_matches(
                    &card,
                    learning_record_id,
                    learning_target_id,
                    now_unix_ms,
                )?;
                touch_generated_card(&transaction, card.id, now_unix_ms)?;
                ready_card_id = Some(card.id);
            }
        }
        if ready_card_id.is_none() {
            transaction
                .execute(
                    "UPDATE review_feed_items SET generated_card_id = NULL WHERE id = ?1",
                    [input.feed_item_id],
                )
                .map_err(|error| format!("过期 AI 复习卡解绑失败：{error}"))?;
        }
    }
    if ready_card_id.is_none() {
        if let Some(card) = load_generated_card_by_request_key(&transaction, &input.request_key)? {
            ensure_generated_card_matches(
                &card,
                learning_record_id,
                learning_target_id,
                now_unix_ms,
            )?;
            attach_generated_card(&transaction, input.feed_item_id, card.id)?;
            touch_generated_card(&transaction, card.id, now_unix_ms)?;
            ready_card_id = Some(card.id);
        }
    }
    if ready_card_id.is_none() {
        if let Some(card) = load_reusable_generated_card(
            &transaction,
            learning_record_id,
            learning_target_id,
            day_start_unix_ms,
            input.feed_item_id,
            now_unix_ms,
        )? {
            attach_generated_card(&transaction, input.feed_item_id, card.id)?;
            touch_generated_card(&transaction, card.id, now_unix_ms)?;
            ready_card_id = Some(card.id);
        }
    }
    if ready_card_id.is_none() {
        if let Some(card) = load_generated_card_at_capacity(
            &transaction,
            learning_record_id,
            learning_target_id,
            now_unix_ms,
        )? {
            attach_generated_card(&transaction, input.feed_item_id, card.id)?;
            touch_generated_card(&transaction, card.id, now_unix_ms)?;
            ready_card_id = Some(card.id);
        }
    }
    if let Some(card_id) = ready_card_id {
        clear_generation_failure(&transaction, &input.request_key)?;
        transaction
            .commit()
            .map_err(|error| format!("AI 复习卡池事务提交失败：{error}"))?;
        return load_generated_card(&connection, card_id)?
            .map(GeneratedCardPreflight::Ready)
            .ok_or_else(|| "AI 复习卡绑定后无法读取。".to_string());
    }

    if let Some(failure) = load_generation_failure_by_request_key(&transaction, &input.request_key)?
    {
        ensure_generation_failure_matches(&failure, input)?;
        if !input.explicit_retry && failure.retry_after_unix_ms > now_unix_ms {
            transaction
                .commit()
                .map_err(|error| format!("AI 复习卡退避状态事务提交失败：{error}"))?;
            return Ok(GeneratedCardPreflight::Deferred {
                retry_after_unix_ms: failure.retry_after_unix_ms,
                last_error: failure.last_error,
            });
        }
    }

    let variant_index = next_generated_card_variant_index(&transaction, learning_record_id)?;
    let learning_record = get_learning_record_from_connection(&transaction, learning_record_id)?
        .ok_or_else(|| "AI 复习卡对应的学习记录不存在。".to_string())?;
    transaction
        .commit()
        .map_err(|error| format!("AI 复习卡池读取事务提交失败：{error}"))?;
    Ok(GeneratedCardPreflight::Generate {
        learning_record,
        variant_index,
        day_start_unix_ms,
    })
}

async fn prepare_generated_review_card(
    app: &AppHandle,
    input: &PrepareReviewFeedCardInput,
) -> Result<GeneratedReviewCard, String> {
    validate_request_key(&input.request_key)?;
    if input.feed_item_id <= 0 || input.learning_record_id <= 0 || input.learning_target_id <= 0 {
        return Err("AI 复习卡请求包含无效的 Feed 条目或学习记录。".to_string());
    }

    let path = database_path_for_app(app)?;
    let now_unix_ms = unix_time_ms()?;
    let (learning_record, variant_index, learning_record_id, day_start_unix_ms) =
        match prepare_generated_card_preflight(&path, input, now_unix_ms)? {
            GeneratedCardPreflight::Ready(card) => return Ok(card),
            GeneratedCardPreflight::Deferred {
                retry_after_unix_ms,
                last_error,
            } => {
                return Err(format!(
                    "AI 复习卡仍在失败退避期内（可在 {retry_after_unix_ms} 毫秒时间戳后自动重试）：{last_error}"
                ));
            }
            GeneratedCardPreflight::Generate {
                learning_record,
                variant_index,
                day_start_unix_ms,
            } => (
                learning_record,
                variant_index,
                input.learning_record_id,
                day_start_unix_ms,
            ),
        };

    let request_body = match build_generated_card_request(&learning_record, variant_index) {
        Ok(request_body) => request_body,
        Err(error) => return persist_generation_failure_and_return(&path, input, error),
    };
    let response: ReviewCardChatResponse = match post_tracked_chat_completion(
        app,
        ModelUsageCategory::ReviewCard,
        "DeepSeek 复习制卡",
        &request_body,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => return persist_generation_failure_and_return(&path, input, error),
    };
    let payload = match finish_generated_card_response(&learning_record, response) {
        Ok(payload) => payload,
        Err(error) => return persist_generation_failure_and_return(&path, input, error),
    };
    let save_result = (|| -> Result<GeneratedReviewCard, String> {
        let model = configured_model();
        let saved_at_unix_ms = unix_time_ms()?;
        let expires_at_unix_ms = saved_at_unix_ms
            .checked_add(GENERATED_CARD_TTL_UNIX_MS)
            .ok_or_else(|| "AI 复习卡有效期超出可保存范围。".to_string())?;

        let mut connection = open_database(&path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("AI 复习卡保存事务无法开始：{error}"))?;
        maintain_generated_card_pool(&transaction, saved_at_unix_ms)?;
        let (current_record_id, current_target_id, current_day_start, current_card_id): (
            i64,
            i64,
            i64,
            Option<i64>,
        ) =
            transaction
                .query_row(
                    "SELECT learning_record_id, learning_target_id, day_start_unix_ms, generated_card_id
                 FROM review_feed_items WHERE id = ?1",
                    [input.feed_item_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
                .map_err(|error| format!("AI 复习卡保存前 Feed 条目读取失败：{error}"))?
                .ok_or_else(|| "AI 复习卡保存前 Feed 条目已不存在。".to_string())?;
        if current_record_id != learning_record_id
            || current_target_id != input.learning_target_id
            || current_day_start != day_start_unix_ms
        {
            return Err("AI 复习卡保存前 Feed 条目身份已变化。".to_string());
        }
        let card = if let Some(card_id) = current_card_id {
            let card = load_generated_card(&transaction, card_id)?
                .ok_or_else(|| "Feed 条目引用的 AI 复习卡不存在。".to_string())?;
            ensure_generated_card_matches(
                &card,
                learning_record_id,
                input.learning_target_id,
                saved_at_unix_ms,
            )?;
            touch_generated_card(&transaction, card.id, saved_at_unix_ms)?;
            card
        } else if let Some(card) =
            load_generated_card_by_request_key(&transaction, &input.request_key)?
        {
            ensure_generated_card_matches(
                &card,
                learning_record_id,
                input.learning_target_id,
                saved_at_unix_ms,
            )?;
            touch_generated_card(&transaction, card.id, saved_at_unix_ms)?;
            card
        } else if let Some(card) = load_reusable_generated_card(
            &transaction,
            learning_record_id,
            input.learning_target_id,
            day_start_unix_ms,
            input.feed_item_id,
            saved_at_unix_ms,
        )? {
            touch_generated_card(&transaction, card.id, saved_at_unix_ms)?;
            card
        } else if let Some(card) = load_generated_card_at_capacity(
            &transaction,
            learning_record_id,
            input.learning_target_id,
            saved_at_unix_ms,
        )? {
            touch_generated_card(&transaction, card.id, saved_at_unix_ms)?;
            card
        } else {
            let content_json = serde_json::to_string(&payload)
                .map_err(|error| format!("AI 复习卡内容无法序列化：{error}"))?;
            let persisted_variant_index =
                next_generated_card_variant_index(&transaction, learning_record_id)?;
            transaction
                .execute(
                    "INSERT INTO review_generated_cards (
                       learning_record_id, learning_target_id, variant_index, generation_request_key,
                       content_json, model, created_at_unix_ms, expires_at_unix_ms,
                       last_used_at_unix_ms, use_count
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, 1)",
                    params![
                        learning_record_id,
                        input.learning_target_id,
                        persisted_variant_index,
                        input.request_key,
                        content_json,
                        model,
                        saved_at_unix_ms,
                        expires_at_unix_ms,
                    ],
                )
                .map_err(|error| format!("AI 复习卡写入失败：{error}"))?;
            load_generated_card(&transaction, transaction.last_insert_rowid())?
                .ok_or_else(|| "AI 复习卡写入后无法读取。".to_string())?
        };
        transaction
            .execute(
                "UPDATE review_feed_items SET generated_card_id = ?1
                 WHERE id = ?2",
                params![card.id, input.feed_item_id],
            )
            .map_err(|error| format!("AI 复习卡绑定 Feed 条目失败：{error}"))?;
        clear_generation_failure(&transaction, &input.request_key)?;
        maintain_generated_card_pool(&transaction, saved_at_unix_ms)?;
        transaction
            .commit()
            .map_err(|error| format!("AI 复习卡保存事务提交失败：{error}"))?;
        load_generated_card(&connection, card.id)?
            .ok_or_else(|| "AI 复习卡保存后被错误淘汰。".to_string())
    })();
    match save_result {
        Ok(card) => Ok(card),
        Err(error) => persist_generation_failure_and_return(&path, input, error),
    }
}

fn build_generated_card_request(
    record: &LearningRecord,
    variant_index: i64,
) -> Result<serde_json::Value, String> {
    let model = configured_model();
    let record_json = serde_json::to_string_pretty(&json!({
        "learningRecordId": record.id,
        "learningTargetText": record.learning_target_text,
        "sourceText": record.query_text,
        "queryType": record.query_type,
        "savedExplanation": record.explanation_card,
        "variantIndex": variant_index,
    }))
    .map_err(|error| format!("AI 复习卡输入无法序列化：{error}"))?;
    Ok(json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": "You create one concise English review context for ReadRay. Return exactly one JSON object with camelCase fields englishContext, englishContextZh, and hint. englishContext must be natural English, contain learningTargetText exactly (case-insensitive), contain no Chinese, and be one or two sentences. englishContextZh is an accurate Chinese translation. hint is a short Chinese retrieval cue that does not reveal learningTargetText. Use only the saved explanation as semantic authority; do not invent a different meaning, product fact, source, or user history. Return no Markdown or commentary."
            },
            {
                "role": "user",
                "content": format!("Create a distinct review-card variant from this persisted learning record:\n{record_json}")
            }
        ],
        "response_format": { "type": "json_object" },
        "stream": false,
        "max_tokens": REVIEW_CARD_MAX_TOKENS,
        "temperature": REVIEW_CARD_TEMPERATURE
    }))
}

fn finish_generated_card_response(
    record: &LearningRecord,
    response: ReviewCardChatResponse,
) -> Result<GeneratedReviewCardPayload, String> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| "DeepSeek 复习制卡响应缺少 choices[0]。".to_string())?;
    if choice
        .finish_reason
        .as_deref()
        .is_some_and(|reason| reason != "stop")
    {
        return Err(format!(
            "DeepSeek 复习制卡未正常结束：finish_reason={}。",
            choice.finish_reason.unwrap_or_default()
        ));
    }
    let content = choice
        .message
        .content
        .ok_or_else(|| "DeepSeek 复习制卡响应缺少内容。".to_string())?;
    let mut payload: GeneratedReviewCardPayload = serde_json::from_str(content.trim())
        .map_err(|error| format!("DeepSeek 复习制卡内容不是合法 JSON：{error}"))?;
    payload.english_context = payload.english_context.trim().to_string();
    payload.english_context_zh = payload.english_context_zh.trim().to_string();
    payload.hint = payload.hint.trim().to_string();
    validate_generated_card_payload(record, &payload)?;
    Ok(payload)
}

fn validate_generated_card_payload(
    record: &LearningRecord,
    payload: &GeneratedReviewCardPayload,
) -> Result<(), String> {
    validate_generated_text(
        &payload.english_context,
        "AI 复习卡英文语境",
        MAX_GENERATED_CONTEXT_CHARS,
    )?;
    validate_generated_text(
        &payload.english_context_zh,
        "AI 复习卡中文翻译",
        MAX_GENERATED_TRANSLATION_CHARS,
    )?;
    validate_generated_text(&payload.hint, "AI 复习卡提示", MAX_GENERATED_HINT_CHARS)?;
    let query = record.learning_target_text.trim();
    if query.is_empty() || !context_has_complete_query(&payload.english_context, query) {
        return Err("AI 复习卡英文语境没有包含目标表达。".to_string());
    }
    let latin_count = payload
        .english_context
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .count();
    let has_cjk = payload
        .english_context
        .chars()
        .any(|character| matches!(character as u32, 0x3400..=0x9fff));
    if latin_count < 8 || has_cjk {
        return Err("AI 复习卡英文语境必须是可用的纯英文内容。".to_string());
    }
    if payload.hint.to_lowercase().contains(&query.to_lowercase()) {
        return Err("AI 复习卡提示不能直接泄露目标表达。".to_string());
    }
    Ok(())
}

fn context_has_complete_query(context: &str, query: &str) -> bool {
    let context_lower = context.to_lowercase();
    let query_lower = query.to_lowercase();
    if query_lower.is_empty() {
        return false;
    }
    let needs_left_boundary = query_lower
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric());
    let needs_right_boundary = query_lower
        .chars()
        .next_back()
        .is_some_and(|character| character.is_ascii_alphanumeric());
    context_lower.match_indices(&query_lower).any(|(index, _)| {
        let before = context_lower[..index].chars().next_back();
        let after = context_lower[index + query_lower.len()..].chars().next();
        let has_boundaries = (!needs_left_boundary
            || before.is_none_or(|character| !character.is_ascii_alphanumeric()))
            && (!needs_right_boundary
                || after.is_none_or(|character| !character.is_ascii_alphanumeric()));
        let surrounding_latin_count = context_lower[..index]
            .chars()
            .chain(context_lower[index + query_lower.len()..].chars())
            .filter(|character| character.is_ascii_alphabetic())
            .count();
        has_boundaries && surrounding_latin_count >= 4
    })
}

fn validate_generated_text(value: &str, label: &str, max_chars: usize) -> Result<(), String> {
    let count = value.chars().count();
    if value.trim().is_empty() || count > max_chars {
        return Err(format!("{label}必须为 1 到 {max_chars} 个字符。"));
    }
    Ok(())
}

fn load_generated_card(
    connection: &Connection,
    card_id: i64,
) -> Result<Option<GeneratedReviewCard>, String> {
    connection
        .query_row(
            "SELECT id, learning_record_id, learning_target_id, variant_index, content_json,
                    model, created_at_unix_ms, expires_at_unix_ms,
                    last_used_at_unix_ms, use_count
             FROM review_generated_cards WHERE id = ?1",
            [card_id],
            read_generated_card,
        )
        .optional()
        .map_err(|error| format!("AI 复习卡读取失败：{error}"))
}

fn load_reusable_generated_card(
    connection: &Connection,
    learning_record_id: i64,
    learning_target_id: i64,
    day_start_unix_ms: i64,
    feed_item_id: i64,
    now_unix_ms: i64,
) -> Result<Option<GeneratedReviewCard>, String> {
    connection
        .query_row(
            "SELECT id, learning_record_id, learning_target_id, variant_index, content_json,
                    model, created_at_unix_ms, expires_at_unix_ms,
                    last_used_at_unix_ms, use_count
             FROM review_generated_cards card
             WHERE card.learning_record_id = ?1
               AND card.learning_target_id = ?2
               AND card.expires_at_unix_ms > ?3
               AND NOT EXISTS (
                 SELECT 1 FROM review_feed_items used
                 WHERE used.day_start_unix_ms = ?4
                   AND used.generated_card_id = card.id
                   AND used.id <> ?5
               )
             ORDER BY card.last_used_at_unix_ms ASC, card.id ASC
             LIMIT 1",
            params![
                learning_record_id,
                learning_target_id,
                now_unix_ms,
                day_start_unix_ms,
                feed_item_id
            ],
            read_generated_card,
        )
        .optional()
        .map_err(|error| format!("可复用 AI 复习卡读取失败：{error}"))
}

fn load_generated_card_at_capacity(
    connection: &Connection,
    learning_record_id: i64,
    learning_target_id: i64,
    now_unix_ms: i64,
) -> Result<Option<GeneratedReviewCard>, String> {
    let valid_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM review_generated_cards
             WHERE learning_record_id = ?1 AND learning_target_id = ?2
               AND expires_at_unix_ms > ?3",
            params![learning_record_id, learning_target_id, now_unix_ms],
            |row| row.get(0),
        )
        .map_err(|error| format!("AI 复习卡池单记录容量读取失败：{error}"))?;
    if valid_count < GENERATED_CARD_PER_RECORD_CAPACITY {
        return Ok(None);
    }
    connection
        .query_row(
            "SELECT id, learning_record_id, learning_target_id, variant_index, content_json,
                    model, created_at_unix_ms, expires_at_unix_ms,
                    last_used_at_unix_ms, use_count
             FROM review_generated_cards
             WHERE learning_target_id = ?1 AND learning_record_id = ?2
               AND expires_at_unix_ms > ?3
             ORDER BY last_used_at_unix_ms ASC, id ASC
             LIMIT 1",
            params![learning_target_id, learning_record_id, now_unix_ms],
            read_generated_card,
        )
        .optional()
        .map_err(|error| format!("达到容量后的 AI 复习卡复用失败：{error}"))
}

fn load_generation_failure_by_feed_item(
    connection: &Connection,
    feed_item_id: i64,
) -> Result<Option<ReviewCardGenerationFailure>, String> {
    connection
        .query_row(
            "SELECT request_key, feed_item_id, learning_record_id, failure_count,
                    retry_after_unix_ms, last_error, created_at_unix_ms, updated_at_unix_ms
             FROM review_card_generation_failures WHERE feed_item_id = ?1
             ORDER BY updated_at_unix_ms DESC LIMIT 1",
            [feed_item_id],
            read_generation_failure,
        )
        .optional()
        .map_err(|error| format!("AI 复习卡失败状态读取失败：{error}"))
}

fn load_generation_failure_by_request_key(
    connection: &Connection,
    request_key: &str,
) -> Result<Option<ReviewCardGenerationFailure>, String> {
    connection
        .query_row(
            "SELECT request_key, feed_item_id, learning_record_id, failure_count,
                    retry_after_unix_ms, last_error, created_at_unix_ms, updated_at_unix_ms
             FROM review_card_generation_failures WHERE request_key = ?1",
            [request_key],
            read_generation_failure,
        )
        .optional()
        .map_err(|error| format!("AI 复习卡失败请求读取失败：{error}"))
}

fn read_generation_failure(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ReviewCardGenerationFailure> {
    Ok(ReviewCardGenerationFailure {
        request_key: row.get(0)?,
        feed_item_id: row.get(1)?,
        learning_record_id: row.get(2)?,
        failure_count: row.get(3)?,
        retry_after_unix_ms: row.get(4)?,
        last_error: row.get(5)?,
        created_at_unix_ms: row.get(6)?,
        updated_at_unix_ms: row.get(7)?,
    })
}

fn ensure_generation_failure_matches(
    failure: &ReviewCardGenerationFailure,
    input: &PrepareReviewFeedCardInput,
) -> Result<(), String> {
    if failure.feed_item_id != input.feed_item_id
        || failure.learning_record_id != input.learning_record_id
    {
        return Err("AI 复习卡失败 requestKey 已被不同 Feed 条目使用。".to_string());
    }
    Ok(())
}

fn clear_generation_failure(connection: &Connection, request_key: &str) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM review_card_generation_failures WHERE request_key = ?1",
            [request_key],
        )
        .map_err(|error| format!("AI 复习卡失败状态清理失败：{error}"))?;
    Ok(())
}

fn generation_failure_backoff_unix_ms(failure_count: i64) -> i64 {
    let shift = u32::try_from(failure_count.saturating_sub(1).min(16)).unwrap_or(16);
    GENERATED_CARD_FAILURE_BACKOFF_BASE_UNIX_MS
        .saturating_mul(1_i64.checked_shl(shift).unwrap_or(i64::MAX))
        .min(GENERATED_CARD_FAILURE_BACKOFF_MAX_UNIX_MS)
}

fn persist_generation_failure(
    path: &Path,
    input: &PrepareReviewFeedCardInput,
    error: &str,
    now_unix_ms: i64,
) -> Result<(), String> {
    let mut connection = open_database(path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|failure| format!("AI 复习卡失败状态事务无法开始：{failure}"))?;
    let (feed_learning_record_id, feed_learning_target_id): (i64, i64) = transaction
        .query_row(
            "SELECT learning_record_id, learning_target_id FROM review_feed_items WHERE id = ?1",
            [input.feed_item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|failure| format!("AI 复习卡失败状态 Feed 身份读取失败：{failure}"))?
        .ok_or_else(|| "AI 复习卡失败状态对应的 Feed 条目不存在。".to_string())?;
    if feed_learning_record_id != input.learning_record_id
        || feed_learning_target_id != input.learning_target_id
    {
        return Err("AI 复习卡失败状态与 Feed 条目身份不一致。".to_string());
    }
    let existing = load_generation_failure_by_request_key(&transaction, &input.request_key)?;
    if let Some(existing) = existing.as_ref() {
        ensure_generation_failure_matches(existing, input)?;
    }
    let failure_count = existing
        .as_ref()
        .map_or(1, |failure| failure.failure_count.saturating_add(1));
    let retry_after_unix_ms = now_unix_ms
        .checked_add(generation_failure_backoff_unix_ms(failure_count))
        .ok_or_else(|| "AI 复习卡失败退避时间超出可保存范围。".to_string())?;
    let last_error: String = error
        .chars()
        .take(MAX_GENERATION_FAILURE_ERROR_CHARS)
        .collect();
    transaction
        .execute(
            "INSERT INTO review_card_generation_failures (
               request_key, feed_item_id, learning_record_id, failure_count,
               retry_after_unix_ms, last_error, created_at_unix_ms, updated_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT(request_key) DO UPDATE SET
               failure_count = excluded.failure_count,
               retry_after_unix_ms = excluded.retry_after_unix_ms,
               last_error = excluded.last_error,
               updated_at_unix_ms = excluded.updated_at_unix_ms",
            params![
                input.request_key,
                input.feed_item_id,
                input.learning_record_id,
                failure_count,
                retry_after_unix_ms,
                last_error,
                now_unix_ms,
            ],
        )
        .map_err(|failure| format!("AI 复习卡失败状态写入失败：{failure}"))?;
    transaction
        .commit()
        .map_err(|failure| format!("AI 复习卡失败状态事务提交失败：{failure}"))
}

fn persist_generation_failure_and_return<T>(
    path: &Path,
    input: &PrepareReviewFeedCardInput,
    error: String,
) -> Result<T, String> {
    let persistence = unix_time_ms()
        .and_then(|now_unix_ms| persist_generation_failure(path, input, &error, now_unix_ms));
    match persistence {
        Ok(()) => Err(error),
        Err(persistence_error) => Err(format!(
            "{error}；同时无法保存本次制卡失败退避状态：{persistence_error}"
        )),
    }
}

fn load_generated_card_by_request_key(
    connection: &Connection,
    request_key: &str,
) -> Result<Option<GeneratedReviewCard>, String> {
    connection
        .query_row(
            "SELECT id, learning_record_id, learning_target_id, variant_index, content_json,
                    model, created_at_unix_ms, expires_at_unix_ms,
                    last_used_at_unix_ms, use_count
             FROM review_generated_cards WHERE generation_request_key = ?1",
            [request_key],
            read_generated_card,
        )
        .optional()
        .map_err(|error| format!("幂等 AI 复习卡读取失败：{error}"))
}

fn read_generated_card(row: &rusqlite::Row<'_>) -> rusqlite::Result<GeneratedReviewCard> {
    let content_json: String = row.get(4)?;
    let payload: GeneratedReviewCardPayload =
        serde_json::from_str(&content_json).map_err(to_sql_conversion_error)?;
    Ok(GeneratedReviewCard {
        id: row.get(0)?,
        learning_record_id: row.get(1)?,
        learning_target_id: row.get(2)?,
        variant_index: row.get(3)?,
        english_context: payload.english_context,
        english_context_zh: payload.english_context_zh,
        hint: payload.hint,
        model: row.get(5)?,
        created_at_unix_ms: row.get(6)?,
        expires_at_unix_ms: row.get(7)?,
        last_used_at_unix_ms: row.get(8)?,
        use_count: row.get(9)?,
    })
}

fn attach_generated_card(
    connection: &Connection,
    feed_item_id: i64,
    card_id: i64,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE review_feed_items SET generated_card_id = ?1
             WHERE id = ?2 AND generated_card_id IS NULL",
            params![card_id, feed_item_id],
        )
        .map_err(|error| format!("AI 复习卡绑定 Feed 条目失败：{error}"))?;
    Ok(())
}

fn ensure_generated_card_matches(
    card: &GeneratedReviewCard,
    learning_record_id: i64,
    learning_target_id: i64,
    now_unix_ms: i64,
) -> Result<(), String> {
    if card.learning_record_id != learning_record_id
        || card.learning_target_id != learning_target_id
    {
        return Err("AI 复习卡 requestKey 已被不同 Feed 条目使用。".to_string());
    }
    if card.expires_at_unix_ms <= now_unix_ms {
        return Err("AI 复习卡 requestKey 对应的卡片已经过期。".to_string());
    }
    Ok(())
}

fn next_generated_card_variant_index(
    connection: &Connection,
    learning_record_id: i64,
) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(variant_index), -1) + 1
             FROM review_generated_cards WHERE learning_record_id = ?1",
            [learning_record_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("AI 复习卡池序号读取失败：{error}"))
}

fn touch_generated_card(
    connection: &Connection,
    card_id: i64,
    now_unix_ms: i64,
) -> Result<(), String> {
    let affected = connection
        .execute(
            "UPDATE review_generated_cards
             SET last_used_at_unix_ms = ?1, use_count = use_count + 1
             WHERE id = ?2 AND expires_at_unix_ms > ?1",
            params![now_unix_ms, card_id],
        )
        .map_err(|error| format!("AI 复习卡池使用状态更新失败：{error}"))?;
    if affected != 1 {
        return Err("AI 复习卡在复用前已经过期或被淘汰。".to_string());
    }
    Ok(())
}

fn maintain_generated_card_pool(connection: &Connection, now_unix_ms: i64) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM review_generated_cards WHERE expires_at_unix_ms <= ?1",
            [now_unix_ms],
        )
        .map_err(|error| format!("过期 AI 复习卡淘汰失败：{error}"))?;

    let learning_record_identities = {
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT learning_record_id, learning_target_id
                 FROM review_generated_cards",
            )
            .map_err(|error| format!("AI 复习卡池记录语句无法准备：{error}"))?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .map_err(|error| format!("AI 复习卡池记录读取失败：{error}"))?;
        let mut values = Vec::new();
        for row in rows {
            values.push(row.map_err(|error| format!("AI 复习卡池记录行读取失败：{error}"))?);
        }
        values
    };
    for (learning_record_id, learning_target_id) in learning_record_identities {
        let protected_count: i64 = connection
            .query_row(
                "SELECT COUNT(*)
                 FROM review_generated_cards card
                 WHERE card.learning_record_id = ?1
                   AND card.learning_target_id = ?2
                   AND EXISTS (
                     SELECT 1 FROM review_feed_items item
                     WHERE item.generated_card_id = card.id
                   )",
                params![learning_record_id, learning_target_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("单记录 AI 复习卡池受保护数量读取失败：{error}"))?;
        let reusable_capacity = GENERATED_CARD_PER_RECORD_CAPACITY
            .saturating_sub(protected_count.min(GENERATED_CARD_PER_RECORD_CAPACITY));
        connection
            .execute(
                "DELETE FROM review_generated_cards
                 WHERE learning_record_id = ?1
                   AND learning_target_id = ?2
                   AND NOT EXISTS (
                     SELECT 1 FROM review_feed_items protected
                     WHERE protected.generated_card_id = review_generated_cards.id
                   )
                   AND id NOT IN (
                     SELECT candidate.id FROM review_generated_cards candidate
                     WHERE candidate.learning_record_id = ?1
                       AND candidate.learning_target_id = ?2
                       AND NOT EXISTS (
                         SELECT 1 FROM review_feed_items protected
                         WHERE protected.generated_card_id = candidate.id
                       )
                     ORDER BY last_used_at_unix_ms DESC, id DESC
                     LIMIT ?3
                   )",
                params![learning_record_id, learning_target_id, reusable_capacity],
            )
            .map_err(|error| format!("单记录 AI 复习卡池容量淘汰失败：{error}"))?;
    }

    let protected_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM review_generated_cards card
             WHERE EXISTS (
               SELECT 1 FROM review_feed_items item
               WHERE item.generated_card_id = card.id
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("AI 复习卡池全局受保护数量读取失败：{error}"))?;
    let reusable_capacity = GENERATED_CARD_POOL_CAPACITY
        .saturating_sub(protected_count.min(GENERATED_CARD_POOL_CAPACITY));
    let reusable_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM review_generated_cards card
             WHERE NOT EXISTS (
               SELECT 1 FROM review_feed_items item
               WHERE item.generated_card_id = card.id
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("AI 复习卡池可淘汰容量读取失败：{error}"))?;
    let overflow = reusable_count.saturating_sub(reusable_capacity);
    if overflow > 0 {
        connection
            .execute(
                "DELETE FROM review_generated_cards
                 WHERE id IN (
                   SELECT candidate.id FROM review_generated_cards candidate
                   WHERE NOT EXISTS (
                     SELECT 1 FROM review_feed_items protected
                     WHERE protected.generated_card_id = candidate.id
                   )
                   ORDER BY candidate.last_used_at_unix_ms ASC, candidate.id ASC
                   LIMIT ?1
                 )",
                [overflow],
            )
            .map_err(|error| format!("AI 复习卡池总容量淘汰失败：{error}"))?;
    }
    Ok(())
}

fn load_target(
    connection: &Connection,
    learning_target_id: i64,
) -> Result<Option<StoredTarget>, String> {
    connection
        .query_row(
            "SELECT learning_target_id, revision, next_review_at_unix_ms,
                    attempt_count, remembered_count, forgotten_count, success_streak,
                    last_reviewed_at_unix_ms, last_outcome, last_used_hint, last_attempt_id
             FROM learning_target_review_states WHERE learning_target_id = ?1",
            [learning_target_id],
            |row| {
                let last_outcome: Option<String> = row.get(8)?;
                let last_used_hint: Option<i64> = row.get(9)?;
                Ok(StoredTarget {
                    target: ReviewTarget {
                        learning_target_id: row.get(0)?,
                        revision: row.get(1)?,
                        next_review_at_unix_ms: row.get(2)?,
                        attempt_count: row.get(3)?,
                        remembered_count: row.get(4)?,
                        forgotten_count: row.get(5)?,
                        success_streak: row.get(6)?,
                        last_reviewed_at_unix_ms: row.get(7)?,
                        last_outcome: last_outcome
                            .as_deref()
                            .map(outcome_from_storage)
                            .transpose()
                            .map_err(to_sql_conversion_error)?,
                        last_used_hint: last_used_hint.map(|value| value != 0),
                        last_attempt_id: row.get(10)?,
                    },
                    previous_last_used_hint: last_used_hint,
                })
            },
        )
        .optional()
        .map_err(|error| format!("复习目标读取失败：{error}"))
}

fn feed_item_identity(
    transaction: &Transaction<'_>,
    feed_item_id: i64,
) -> Result<(i64, i64), String> {
    transaction
        .query_row(
            "SELECT learning_record_id, learning_target_id FROM review_feed_items WHERE id = ?1",
            [feed_item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| format!("复习 Feed 条目身份读取失败：{error}"))?
        .ok_or_else(|| "复习 Feed 条目不存在。".to_string())
}

fn active_attempt_for_feed_item(
    connection: &Connection,
    feed_item_id: i64,
) -> Result<Option<ReviewAttempt>, String> {
    let mut statement = connection
        .prepare(&format!(
            "{} WHERE feed_item_id = ?1 AND undone_at_unix_ms IS NULL LIMIT 1",
            select_attempt_sql()
        ))
        .map_err(|error| format!("复习 attempt 读取语句无法准备：{error}"))?;
    statement
        .query_row([feed_item_id], read_attempt)
        .optional()
        .map_err(|error| format!("复习 attempt 读取失败：{error}"))
}

fn load_attempt(connection: &Connection, attempt_id: i64) -> Result<Option<ReviewAttempt>, String> {
    let mut statement = connection
        .prepare(&format!("{} WHERE id = ?1", select_attempt_sql()))
        .map_err(|error| format!("复习 attempt 读取语句无法准备：{error}"))?;
    statement
        .query_row([attempt_id], read_attempt)
        .optional()
        .map_err(|error| format!("复习 attempt 读取失败：{error}"))
}

fn load_attempt_by_request_key(
    connection: &Connection,
    request_key: &str,
) -> Result<Option<ReviewAttempt>, String> {
    let mut statement = connection
        .prepare(&format!("{} WHERE request_key = ?1", select_attempt_sql()))
        .map_err(|error| format!("幂等复习 attempt 语句无法准备：{error}"))?;
    statement
        .query_row([request_key], read_attempt)
        .optional()
        .map_err(|error| format!("幂等复习 attempt 读取失败：{error}"))
}

fn load_attempt_by_undo_request_key(
    connection: &Connection,
    request_key: &str,
) -> Result<Option<ReviewAttempt>, String> {
    let mut statement = connection
        .prepare(&format!(
            "{} WHERE undo_request_key = ?1",
            select_attempt_sql()
        ))
        .map_err(|error| format!("幂等撤销 attempt 语句无法准备：{error}"))?;
    statement
        .query_row([request_key], read_attempt)
        .optional()
        .map_err(|error| format!("幂等撤销 attempt 读取失败：{error}"))
}

fn select_attempt_sql() -> &'static str {
    "SELECT id, feed_item_id, learning_record_id, learning_target_id, request_key, expected_revision,
            target_revision, outcome, used_hint, next_review_at_unix_ms,
            created_at_unix_ms, undone_at_unix_ms, undo_request_key, undo_target_revision
     FROM review_feed_attempts"
}

fn read_attempt(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewAttempt> {
    let outcome: String = row.get(7)?;
    Ok(ReviewAttempt {
        id: row.get(0)?,
        feed_item_id: row.get(1)?,
        learning_record_id: row.get(2)?,
        learning_target_id: row.get(3)?,
        request_key: row.get(4)?,
        expected_revision: row.get(5)?,
        target_revision: row.get(6)?,
        outcome: outcome_from_storage(&outcome).map_err(to_sql_conversion_error)?,
        used_hint: row.get::<_, i64>(8)? != 0,
        next_review_at_unix_ms: row.get(9)?,
        created_at_unix_ms: row.get(10)?,
        undone_at_unix_ms: row.get(11)?,
        undo_request_key: row.get(12)?,
        undo_target_revision: row.get(13)?,
    })
}

fn ensure_attempt_matches_submission(
    attempt: &ReviewAttempt,
    input: &SubmitReviewOutcomeInput,
) -> Result<(), String> {
    if attempt.feed_item_id != input.feed_item_id
        || attempt.learning_record_id != input.learning_record_id
        || attempt.learning_target_id != input.learning_target_id
        || attempt.expected_revision != input.expected_revision
        || attempt.outcome != input.outcome
        || attempt.used_hint != input.used_hint
    {
        return Err("复习结果 requestKey 已被不同请求使用。".to_string());
    }
    Ok(())
}

fn ensure_attempt_matches_undo(
    attempt: &ReviewAttempt,
    input: &UndoReviewOutcomeInput,
) -> Result<(), String> {
    if attempt.id != input.attempt_id
        || attempt.feed_item_id != input.feed_item_id
        || attempt.learning_record_id != input.learning_record_id
        || attempt.learning_target_id != input.learning_target_id
        || attempt.undo_target_revision.is_some()
            && attempt.undo_target_revision != Some(input.expected_revision + 1)
    {
        return Err("撤销复习结果 requestKey 或条目身份不一致。".to_string());
    }
    Ok(())
}

fn schedule_next_review(
    outcome: ReviewOutcome,
    used_hint: bool,
    previous_success_streak: i64,
    now_unix_ms: i64,
) -> Result<(i64, i64), String> {
    let (days, success_streak) = match (outcome, used_hint) {
        (ReviewOutcome::Forgotten, _) => (1_i64, 0_i64),
        (ReviewOutcome::Remembered, true) => (2_i64, previous_success_streak),
        (ReviewOutcome::Remembered, false) => {
            let streak = previous_success_streak.saturating_add(1);
            let days = match streak {
                0 | 1 => 3,
                2 => 7,
                3 => 14,
                _ => 30,
            };
            (days, streak)
        }
    };
    let next_review_at_unix_ms = now_unix_ms
        .checked_add(days.saturating_mul(DAY_UNIX_MS))
        .ok_or_else(|| "下次复习时间超出可保存范围。".to_string())?;
    Ok((next_review_at_unix_ms, success_streak))
}

fn recompute_target_after_undo(
    transaction: &Transaction<'_>,
    learning_target_id: i64,
    expected_revision: i64,
    target_revision: i64,
    now_unix_ms: i64,
) -> Result<(), String> {
    let created_at_unix_ms: i64 = transaction
        .query_row(
            "SELECT created_at_unix_ms FROM learning_target_review_states WHERE learning_target_id = ?1",
            [learning_target_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("撤销后学习记录时间读取失败：{error}"))?
        .ok_or_else(|| "撤销后学习记录不存在。".to_string())?;

    let mut statement = transaction
        .prepare(
            "SELECT id, outcome, used_hint, created_at_unix_ms
             FROM review_feed_attempts
             WHERE learning_target_id = ?1 AND undone_at_unix_ms IS NULL
             ORDER BY created_at_unix_ms ASC, id ASC",
        )
        .map_err(|error| format!("撤销后 attempt 重算语句无法准备：{error}"))?;
    let rows = statement
        .query_map([learning_target_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? != 0,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| format!("撤销后 attempt 重算读取失败：{error}"))?;

    let mut attempt_count = 0_i64;
    let mut remembered_count = 0_i64;
    let mut forgotten_count = 0_i64;
    let mut success_streak = 0_i64;
    let mut next_review_at_unix_ms = created_at_unix_ms;
    let mut last_reviewed_at_unix_ms = None;
    let mut last_outcome = None;
    let mut last_used_hint = None;
    let mut last_attempt_id = None;

    for row in rows {
        let (attempt_id, outcome_storage, used_hint, created_at_unix_ms) =
            row.map_err(|error| format!("撤销后 attempt 重算行读取失败：{error}"))?;
        let outcome = outcome_from_storage(&outcome_storage)?;
        let scheduled =
            schedule_next_review(outcome, used_hint, success_streak, created_at_unix_ms)?;
        next_review_at_unix_ms = scheduled.0;
        success_streak = scheduled.1;
        attempt_count += 1;
        remembered_count += i64::from(outcome == ReviewOutcome::Remembered);
        forgotten_count += i64::from(outcome == ReviewOutcome::Forgotten);
        last_reviewed_at_unix_ms = Some(created_at_unix_ms);
        last_outcome = Some(outcome);
        last_used_hint = Some(used_hint);
        last_attempt_id = Some(attempt_id);
    }
    drop(statement);

    let affected = transaction
        .execute(
            "UPDATE learning_target_review_states
             SET revision = ?1,
                 next_review_at_unix_ms = ?2,
                 attempt_count = ?3,
                 remembered_count = ?4,
                 forgotten_count = ?5,
                 success_streak = ?6,
                 last_reviewed_at_unix_ms = ?7,
                 last_outcome = ?8,
                 last_used_hint = ?9,
                 last_attempt_id = ?10,
                 updated_at_unix_ms = ?11
             WHERE learning_target_id = ?12 AND revision = ?13",
            params![
                target_revision,
                next_review_at_unix_ms,
                attempt_count,
                remembered_count,
                forgotten_count,
                success_streak,
                last_reviewed_at_unix_ms,
                last_outcome.map(outcome_to_storage),
                last_used_hint.map(bool_to_integer),
                last_attempt_id,
                now_unix_ms,
                learning_target_id,
                expected_revision,
            ],
        )
        .map_err(|error| format!("撤销后复习目标重算写回失败：{error}"))?;
    if affected != 1 {
        return Err("撤销复习结果时目标 revision 已变化。".to_string());
    }
    Ok(())
}

fn load_quality_feedback(
    connection: &Connection,
    feed_item_id: i64,
) -> Result<Option<ReviewQualityFeedback>, String> {
    connection
        .query_row(
            "SELECT q.id, q.feed_item_id, q.learning_record_id, q.generated_card_id,
                    q.revision, q.active, q.polarity, q.reason_codes_json, q.detail,
                    q.created_at_unix_ms, q.updated_at_unix_ms
             FROM review_quality_feedback q
             JOIN review_feed_items fi ON fi.id = q.feed_item_id
             WHERE q.feed_item_id = ?1
               AND q.generated_card_id IS fi.generated_card_id",
            [feed_item_id],
            |row| {
                let polarity: String = row.get(6)?;
                let reasons_json: String = row.get(7)?;
                Ok(ReviewQualityFeedback {
                    id: row.get(0)?,
                    feed_item_id: row.get(1)?,
                    learning_record_id: row.get(2)?,
                    generated_card_id: row.get(3)?,
                    revision: row.get(4)?,
                    active: row.get::<_, i64>(5)? != 0,
                    polarity: polarity_from_storage(&polarity).map_err(to_sql_conversion_error)?,
                    reason_codes: serde_json::from_str(&reasons_json)
                        .map_err(to_sql_conversion_error)?,
                    detail: row.get(8)?,
                    created_at_unix_ms: row.get(9)?,
                    updated_at_unix_ms: row.get(10)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("卡片反馈读取失败：{error}"))
}

fn normalize_feedback_input(
    input: &SaveReviewQualityFeedbackInput,
) -> Result<SaveReviewQualityFeedbackInput, String> {
    validate_request_key(&input.request_key)?;
    if input.feed_item_id <= 0
        || input.learning_record_id <= 0
        || input.card_context_key.trim() != input.card_context_key
        || input.card_context_key.is_empty()
        || input.expected_revision.is_some_and(|value| value < 0)
    {
        return Err("卡片反馈请求包含无效记录或 revision。".to_string());
    }
    let allowed = match input.polarity {
        ReviewQualityPolarity::Up => [
            "needed",
            "helpful_context",
            "suitable_difficulty",
            "clear_prompt",
            "want_similar",
            "other",
        ]
        .as_slice(),
        ReviewQualityPolarity::Down => [
            "already_known",
            "not_worth_reviewing",
            "incorrect_meaning",
            "unclear_prompt",
            "answer_problem",
            "too_frequent",
            "unwanted_source",
            "other",
        ]
        .as_slice(),
    };
    if input.reason_codes.len() > allowed.len() {
        return Err("卡片反馈原因数量超出允许范围。".to_string());
    }
    let mut seen = HashSet::new();
    for reason in &input.reason_codes {
        if !allowed.contains(&reason.as_str()) || !seen.insert(reason) {
            return Err("卡片反馈包含未知或重复原因。".to_string());
        }
    }
    let detail = input
        .detail
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if detail.is_some_and(|value| value.chars().count() > MAX_FEEDBACK_DETAIL_CHARS) {
        return Err(format!(
            "卡片反馈补充说明不能超过 {MAX_FEEDBACK_DETAIL_CHARS} 个字符。"
        ));
    }
    Ok(SaveReviewQualityFeedbackInput {
        feed_item_id: input.feed_item_id,
        learning_record_id: input.learning_record_id,
        card_context_key: input.card_context_key.clone(),
        expected_revision: input.expected_revision,
        polarity: input.polarity,
        reason_codes: input.reason_codes.clone(),
        detail: detail.map(str::to_string),
        request_key: input.request_key.clone(),
    })
}

fn load_quality_mutation_result(
    connection: &Connection,
    request_key: &str,
    operation: &str,
    input_json: &str,
) -> Result<Option<ReviewQualityFeedback>, String> {
    let stored: Option<(String, String, String)> = connection
        .query_row(
            "SELECT operation, input_json, result_json
             FROM review_quality_mutations WHERE request_key = ?1",
            [request_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|error| format!("幂等卡片反馈读取失败：{error}"))?;
    let Some((stored_operation, stored_input_json, result_json)) = stored else {
        return Ok(None);
    };
    if stored_operation != operation || stored_input_json != input_json {
        return Err("卡片反馈 requestKey 已被不同请求使用。".to_string());
    }
    serde_json::from_str(&result_json)
        .map(Some)
        .map_err(|error| format!("幂等卡片反馈结果无法解析：{error}"))
}

fn save_quality_mutation(
    transaction: &Transaction<'_>,
    request_key: &str,
    feed_item_id: i64,
    learning_record_id: i64,
    operation: &str,
    input_json: &str,
    result: &ReviewQualityFeedback,
    now_unix_ms: i64,
) -> Result<(), String> {
    let result_json = serde_json::to_string(result)
        .map_err(|error| format!("卡片反馈结果无法序列化：{error}"))?;
    transaction
        .execute(
            "INSERT INTO review_quality_mutations (
               request_key, feed_item_id, learning_record_id, operation,
               input_json, result_json, created_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                request_key,
                feed_item_id,
                learning_record_id,
                operation,
                input_json,
                result_json,
                now_unix_ms
            ],
        )
        .map_err(|error| format!("卡片反馈幂等记录写入失败：{error}"))?;
    Ok(())
}

fn feed_item_feedback_identity(
    connection: &Connection,
    feed_item_id: i64,
) -> Result<(i64, Option<i64>), String> {
    connection
        .query_row(
            "SELECT learning_record_id, generated_card_id
             FROM review_feed_items WHERE id = ?1",
            [feed_item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| format!("卡片反馈 Feed 条目身份读取失败：{error}"))?
        .ok_or_else(|| "卡片反馈对应的 Feed 条目不存在。".to_string())
}

fn feedback_context_key(generated_card_id: Option<i64>) -> String {
    generated_card_id
        .map(|card_id| format!("generated:{card_id}"))
        .unwrap_or_else(|| "recorded".to_string())
}

fn validate_request_key(request_key: &str) -> Result<(), String> {
    if request_key.trim() != request_key
        || request_key.is_empty()
        || request_key.chars().count() > MAX_REQUEST_KEY_CHARS
    {
        return Err(format!(
            "requestKey 必须为 1 到 {MAX_REQUEST_KEY_CHARS} 个非首尾空白字符。"
        ));
    }
    Ok(())
}

#[cfg(test)]
fn ensure_learning_record_exists_raw(
    connection: &Connection,
    learning_record_id: i64,
) -> Result<(), String> {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM learning_records WHERE id = ?1)",
            [learning_record_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("学习记录存在性读取失败：{error}"))?;
    if !exists {
        return Err("学习记录不存在。".to_string());
    }
    Ok(())
}

fn bool_to_integer(value: bool) -> i64 {
    i64::from(value)
}

fn outcome_to_storage(outcome: ReviewOutcome) -> &'static str {
    match outcome {
        ReviewOutcome::Remembered => "remembered",
        ReviewOutcome::Forgotten => "forgotten",
    }
}

fn outcome_from_storage(value: &str) -> Result<ReviewOutcome, String> {
    match value {
        "remembered" => Ok(ReviewOutcome::Remembered),
        "forgotten" => Ok(ReviewOutcome::Forgotten),
        _ => Err(format!("复习记录包含未知 outcome：{value}")),
    }
}

fn reason_from_storage(value: &str) -> Result<ReviewReasonCode, String> {
    match value {
        "scheduled_today" => Ok(ReviewReasonCode::ScheduledToday),
        "new_record" => Ok(ReviewReasonCode::NewRecord),
        "continued_practice" => Ok(ReviewReasonCode::ContinuedPractice),
        _ => Err(format!("复习 Feed 条目包含未知 reasonCode：{value}")),
    }
}

fn polarity_to_storage(polarity: ReviewQualityPolarity) -> &'static str {
    match polarity {
        ReviewQualityPolarity::Up => "up",
        ReviewQualityPolarity::Down => "down",
    }
}

fn polarity_from_storage(value: &str) -> Result<ReviewQualityPolarity, String> {
    match value {
        "up" => Ok(ReviewQualityPolarity::Up),
        "down" => Ok(ReviewQualityPolarity::Down),
        _ => Err(format!("卡片反馈包含未知 polarity：{value}")),
    }
}

fn to_sql_conversion_error(error: impl ToString) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const DAY_START: i64 = 1_787_414_400_000;
    const DAY_END: i64 = DAY_START + DAY_UNIX_MS;
    const NOW: i64 = DAY_START + 12 * 60 * 60 * 1_000;

    fn test_database_path() -> (PathBuf, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("readray-review-test-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("readray.sqlite3");
        (directory, path)
    }

    fn insert_record(connection: &Connection, id: i64, created_at: i64, query: &str) {
        let card = json!({
            "queryType": "word",
            "sourceText": query,
            "headword": query,
            "partOfSpeech": "noun",
            "phonetic": "/test/",
            "basicMeanings": ["测试义"],
            "contextMeaning": "当前语境义",
            "sourceSentence": format!("A sentence with {query}."),
            "sourceSentenceZh": "包含目标词的句子。",
            "phrases": [],
            "nearMeanings": [],
            "examples": [],
            "reviewHint": "一个真实保存的提示"
        });
        connection
            .execute(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type, source_app,
                   context_text, explanation_card_json, schema_version, created_at_unix_ms, difficulty
                 ) VALUES (?1, ?2, ?3, 'word', 'manual', 'Obsidian', ?4, ?5, 1, ?6, NULL)",
                params![
                    id,
                    query,
                    query.to_lowercase(),
                    format!("Context for {query}"),
                    card.to_string(),
                    created_at
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO learning_record_targets (
                   learning_record_id, query_direction, learning_target_text,
                   normalized_target_text, created_at_unix_ms
                 ) VALUES (?1, 'en_to_zh', ?2, ?3, ?4)",
                params![id, query, query.to_lowercase(), created_at],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO learning_targets (
                   id, stable_key, canonicalization_version, query_type, display_target_text,
                   normalized_target_text, representative_learning_record_id,
                   created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (?1, ?2, 1, 'word', ?3, ?4, ?1, ?5, ?5)",
                params![
                    id,
                    format!("v1:word:{}", query.to_lowercase()),
                    query,
                    query.to_lowercase(),
                    created_at
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO learning_target_occurrences (
                   learning_record_id, learning_target_id, canonicalization_version,
                   binding_revision, bound_at_unix_ms, updated_at_unix_ms
                 ) VALUES (?1, ?1, 1, 1, ?2, ?2)",
                params![id, created_at],
            )
            .unwrap();
    }

    fn setup(record_count: i64) -> (PathBuf, PathBuf, ReviewStore) {
        let (directory, path) = test_database_path();
        let store = ReviewStore::open(&path).unwrap();
        for id in 1..=record_count {
            insert_record(
                &store.connection,
                id,
                NOW - id * 1_000,
                &format!("same {id}"),
            );
        }
        (directory, path, store)
    }

    fn quality_feedback_unique_indexes(connection: &Connection) -> Vec<Vec<String>> {
        let index_names: Vec<String> = connection
            .prepare(
                "SELECT name
                 FROM pragma_index_list('review_quality_feedback')
                 WHERE \"unique\" = 1
                 ORDER BY seq",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        index_names
            .into_iter()
            .map(|index_name| {
                connection
                    .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                    .unwrap()
                    .query_map([index_name], |row| row.get(0))
                    .unwrap()
                    .map(Result::unwrap)
                    .collect()
            })
            .collect()
    }

    fn submit_input(item: &ReviewFeedItem, request_key: &str) -> SubmitReviewOutcomeInput {
        SubmitReviewOutcomeInput {
            feed_item_id: item.id,
            learning_record_id: item.learning_record.id,
            learning_target_id: item.target.learning_target_id,
            expected_revision: item.target.revision,
            outcome: ReviewOutcome::Remembered,
            used_hint: false,
            request_key: request_key.to_string(),
        }
    }

    fn insert_generated_test_card(
        connection: &Connection,
        learning_record_id: i64,
        variant_index: i64,
        request_key: &str,
        created_at_unix_ms: i64,
        expires_at_unix_ms: i64,
        last_used_at_unix_ms: i64,
    ) -> i64 {
        let learning_target_id = target_id_for_record(connection, learning_record_id);
        let payload = GeneratedReviewCardPayload {
            english_context: format!(
                "I used same {learning_record_id} while comparing two approaches."
            ),
            english_context_zh: format!("我在比较两种方案时使用了 same {learning_record_id}。"),
            hint: "想想表示两个方案相似的表达。".to_string(),
        };
        connection
            .execute(
                "INSERT INTO review_generated_cards (
                   learning_record_id, learning_target_id, variant_index, generation_request_key,
                   content_json, model, created_at_unix_ms, expires_at_unix_ms,
                   last_used_at_unix_ms, use_count
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'test-model', ?6, ?7, ?8, 1)",
                params![
                    learning_record_id,
                    learning_target_id,
                    variant_index,
                    request_key,
                    serde_json::to_string(&payload).unwrap(),
                    created_at_unix_ms,
                    expires_at_unix_ms,
                    last_used_at_unix_ms,
                ],
            )
            .unwrap();
        connection.last_insert_rowid()
    }

    fn target_id_for_record(connection: &Connection, learning_record_id: i64) -> i64 {
        connection
            .query_row(
                "SELECT learning_target_id FROM learning_target_occurrences
                 WHERE learning_record_id = ?1",
                [learning_record_id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn merge_record_into_target(
        connection: &Connection,
        learning_record_id: i64,
        learning_target_id: i64,
    ) {
        let previous_target_id = target_id_for_record(connection, learning_record_id);
        connection
            .execute(
                "DELETE FROM learning_target_occurrences WHERE learning_record_id = ?1",
                [learning_record_id],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM learning_targets WHERE id = ?1",
                [previous_target_id],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE learning_record_targets
                 SET learning_target_text = 'same 1', normalized_target_text = 'same 1'
                 WHERE learning_record_id = ?1",
                [learning_record_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO learning_target_occurrences (
                   learning_record_id, learning_target_id, canonicalization_version,
                   binding_revision, bound_at_unix_ms, updated_at_unix_ms
                 ) VALUES (?1, ?2, 1, 1, ?3, ?3)",
                params![learning_record_id, learning_target_id, NOW],
            )
            .unwrap();
    }

    #[test]
    fn feed_is_persisted_and_next_cycle_waits_for_every_current_item() {
        let (directory, path, mut store) = setup(3);
        let first = store
            .load_feed_page(DAY_START, DAY_END, None, 2, NOW)
            .unwrap();
        assert_eq!(first.items.len(), 2);
        assert!(first.can_continue);
        assert!(first
            .items
            .iter()
            .all(|item| item.cycle_index == 0 && item.reason_code == ReviewReasonCode::NewRecord));

        drop(store);
        let mut reopened = ReviewStore::open(&path).unwrap();
        let persisted = reopened
            .load_feed_page(DAY_START, DAY_END, None, 2, NOW + 5_000)
            .unwrap();
        assert_eq!(
            first.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            persisted
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>()
        );

        let second = reopened
            .load_feed_page(DAY_START, DAY_END, first.next_cursor, 2, NOW + 5_000)
            .unwrap();
        assert_eq!(second.items.len(), 1);
        let first_cycle_ids: HashSet<_> = first
            .items
            .iter()
            .chain(second.items.iter())
            .map(|item| item.learning_record.id)
            .collect();
        assert_eq!(first_cycle_ids.len(), 3);
        assert_eq!(second.items[0].cycle_index, 0);
        assert!(!second.can_continue);

        let blocked = reopened
            .load_feed_page(DAY_START, DAY_END, second.next_cursor, 2, NOW + 6_000)
            .unwrap();
        assert!(blocked.items.is_empty());
        assert!(!blocked.can_continue);

        for (index, item) in first.items.iter().chain(second.items.iter()).enumerate() {
            reopened
                .submit_outcome(
                    &submit_input(item, &format!("complete-cycle-zero-{index}")),
                    NOW + 7_000 + index as i64,
                )
                .unwrap();
        }
        let next_cycle = reopened
            .load_feed_page(DAY_START, DAY_END, second.next_cursor, 2, NOW + 8_000)
            .unwrap();
        assert_eq!(next_cycle.items.len(), 2);
        assert!(next_cycle.items.iter().all(|item| item.cycle_index == 1));
        drop(reopened);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn one_target_appears_once_per_cycle_and_rotates_real_occurrences() {
        let (directory, _path, mut store) = setup(2);
        merge_record_into_target(&store.connection, 2, 1);

        let first = store
            .load_feed_page(DAY_START, DAY_END, None, 2, NOW)
            .unwrap();
        assert_eq!(first.items.len(), 1);
        assert_eq!(first.items[0].target.learning_target_id, 1);
        assert_eq!(first.items[0].learning_record.id, 1);
        let completed = store
            .submit_outcome(&submit_input(&first.items[0], "merged-cycle-0"), NOW + 1)
            .unwrap();
        let second = store
            .load_feed_page(DAY_START, DAY_END, first.next_cursor, 2, NOW + 2)
            .unwrap();
        assert_eq!(second.items.len(), 1);
        assert_eq!(second.items[0].cycle_index, 1);
        assert_eq!(second.items[0].target.learning_target_id, 1);
        assert_eq!(second.items[0].learning_record.id, 2);
        assert_eq!(second.items[0].target.revision, completed.target.revision);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cross_cycle_boundary_avoids_adjacent_same_target_when_alternative_exists() {
        let (directory, _path, mut store) = setup(2);
        let first = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        assert_eq!(first.items[0].target.learning_target_id, 2);
        store
            .submit_outcome(&submit_input(&first.items[0], "boundary-first"), NOW + 1)
            .unwrap();

        let second = store
            .load_feed_page(DAY_START, DAY_END, first.next_cursor, 1, NOW + 2)
            .unwrap();
        assert_eq!(second.items[0].target.learning_target_id, 1);
        let mut forgotten = submit_input(&second.items[0], "boundary-second");
        forgotten.outcome = ReviewOutcome::Forgotten;
        store.submit_outcome(&forgotten, NOW + 3).unwrap();

        let next_cycle = store
            .load_feed_page(DAY_START, DAY_END, second.next_cursor, 1, NOW + 4)
            .unwrap();
        assert_eq!(next_cycle.items[0].cycle_index, 1);
        assert_eq!(next_cycle.items[0].target.learning_target_id, 2);
        assert_ne!(
            next_cycle.items[0].target.learning_target_id,
            second.items[0].target.learning_target_id
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn review_feed_excludes_records_without_reliable_english_target() {
        let (directory, _path, mut store) = setup(1);
        let chinese_card = json!({
            "queryType": "word",
            "sourceText": "旧中文记录",
            "headword": "旧中文记录",
            "basicMeanings": ["历史记录"],
            "phrases": [],
            "nearMeanings": [],
            "examples": []
        });
        store
            .connection
            .execute(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type, source_app,
                   context_text, explanation_card_json, schema_version, created_at_unix_ms, difficulty
                 ) VALUES (99, '旧中文记录', '旧中文记录', 'word', 'manual', NULL,
                           '原始中文来源继续保留', ?1, 1, ?2, NULL)",
                params![chinese_card.to_string(), NOW - 99_000],
            )
            .unwrap();
        store
            .connection
            .execute_batch(&format!(
                "INSERT INTO learning_targets (
                   id, stable_key, target_kind, canonicalization_version, query_type,
                   display_target_text, normalized_target_text,
                   representative_learning_record_id, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (99, 'legacy-compat:record:99', 'legacy_compat', 0, 'word',
                           '旧中文记录', NULL, 99, {created_at}, {created_at});
                 INSERT INTO learning_target_occurrences (
                   learning_record_id, learning_target_id, canonicalization_version,
                   binding_revision, bound_at_unix_ms, updated_at_unix_ms
                 ) VALUES (99, 99, 0, 0, {created_at}, {created_at});",
                created_at = NOW - 99_000
            ))
            .unwrap();

        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 10, NOW)
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].learning_record.learning_target_text, "same 1");
        let raw_count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM learning_records", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(raw_count, 2);
        store
            .submit_outcome(
                &submit_input(&page.items[0], "compatibility-target-excluded"),
                NOW + 1,
            )
            .unwrap();
        let next_cycle = store
            .load_feed_page(DAY_START, DAY_END, page.next_cursor, 10, NOW + 2)
            .unwrap();
        assert_eq!(next_cycle.items.len(), 1);
        assert_eq!(next_cycle.items[0].learning_record.id, 1);
        assert_eq!(next_cycle.items[0].cycle_index, 1);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn feed_has_no_daily_item_cap_but_each_cycle_requires_completion() {
        let (directory, _path, mut store) = setup(2);
        let mut cursor = None;
        let mut items = Vec::new();
        for index in 0..4 {
            let page = store
                .load_feed_page(DAY_START, DAY_END, cursor, 2, NOW + index)
                .unwrap();
            assert_eq!(page.items.len(), 2);
            assert!(page.items.iter().all(|item| item.cycle_index == index));
            for (item_index, item) in page.items.iter().enumerate() {
                store
                    .submit_outcome(
                        &submit_input(item, &format!("cycle-{index}-item-{item_index}")),
                        NOW + 100 + index * 10 + item_index as i64,
                    )
                    .unwrap();
            }
            cursor = page.next_cursor;
            items.extend(page.items);
        }
        assert_eq!(items.len(), 8);
        assert_eq!(items.last().unwrap().cycle_index, 3);
        assert!(items
            .windows(2)
            .all(|pair| { pair[0].learning_record.id != pair[1].learning_record.id }));
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn scheduling_distinguishes_forgotten_hint_and_unassisted_recall() {
        let forgotten = schedule_next_review(ReviewOutcome::Forgotten, false, 0, NOW).unwrap();
        let hinted = schedule_next_review(ReviewOutcome::Remembered, true, 0, NOW).unwrap();
        let recalled = schedule_next_review(ReviewOutcome::Remembered, false, 0, NOW).unwrap();
        assert_eq!(forgotten.0, NOW + DAY_UNIX_MS);
        assert_eq!(hinted.0, NOW + 2 * DAY_UNIX_MS);
        assert_eq!(recalled.0, NOW + 3 * DAY_UNIX_MS);
        assert_eq!(recalled.1, 1);
        assert_ne!(hinted.0, recalled.0);
    }

    #[test]
    fn outcome_is_transactional_idempotent_and_rejects_revision_conflicts() {
        let (directory, _path, mut store) = setup(1);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let input = submit_input(&page.items[0], "attempt-stable-1");
        let first = store.submit_outcome(&input, NOW + 100).unwrap();
        let retry = store.submit_outcome(&input, NOW + 200).unwrap();
        assert_eq!(first.attempt.id, retry.attempt.id);
        assert_eq!(retry.target.attempt_count, 1);

        let mut conflict = input.clone();
        conflict.request_key = "attempt-conflict-2".to_string();
        let error = store.submit_outcome(&conflict, NOW + 300).unwrap_err();
        assert!(error.contains("已经完成") || error.contains("revision 冲突"));
        let attempts: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM review_feed_attempts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(attempts, 1);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn every_result_can_be_undone_and_retry_is_idempotent() {
        let (directory, _path, mut store) = setup(1);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let before = page.items[0].target.clone();
        let written = store
            .submit_outcome(&submit_input(&page.items[0], "attempt-for-undo"), NOW + 100)
            .unwrap();
        let undo = UndoReviewOutcomeInput {
            attempt_id: written.attempt.id,
            feed_item_id: page.items[0].id,
            learning_record_id: page.items[0].learning_record.id,
            learning_target_id: page.items[0].target.learning_target_id,
            expected_revision: written.target.revision,
            request_key: "undo-stable-1".to_string(),
        };
        let undone = store.undo_outcome(&undo, NOW + 200).unwrap();
        let retry = store.undo_outcome(&undo, NOW + 300).unwrap();
        assert_eq!(undone.target.revision, written.target.revision + 1);
        assert_eq!(undone.target.attempt_count, before.attempt_count);
        assert_eq!(
            undone.target.next_review_at_unix_ms,
            before.next_review_at_unix_ms
        );
        assert!(undone.attempt.undone_at_unix_ms.is_some());
        assert_eq!(
            retry.attempt.undo_request_key.as_deref(),
            Some("undo-stable-1")
        );
        let reloaded = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW + 400)
            .unwrap();
        assert_eq!(reloaded.completed_count, 0);
        assert!(reloaded.items[0].attempt.is_none());
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn undoing_an_older_cycle_recomputes_target_from_active_attempts() {
        let (directory, _path, mut store) = setup(1);
        let first_page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let first = store
            .submit_outcome(
                &submit_input(&first_page.items[0], "older-attempt"),
                NOW + 100,
            )
            .unwrap();
        let second_page = store
            .load_feed_page(DAY_START, DAY_END, first_page.next_cursor, 1, NOW + 110)
            .unwrap();
        assert_eq!(second_page.items[0].cycle_index, 1);
        let mut second_input = submit_input(&second_page.items[0], "newer-attempt");
        second_input.outcome = ReviewOutcome::Forgotten;
        let second = store.submit_outcome(&second_input, NOW + 200).unwrap();

        let undone = store
            .undo_outcome(
                &UndoReviewOutcomeInput {
                    attempt_id: first.attempt.id,
                    feed_item_id: first_page.items[0].id,
                    learning_record_id: first_page.items[0].learning_record.id,
                    learning_target_id: first_page.items[0].target.learning_target_id,
                    expected_revision: second.target.revision,
                    request_key: "undo-older-attempt".to_string(),
                },
                NOW + 300,
            )
            .unwrap();
        assert_eq!(undone.target.revision, second.target.revision + 1);
        assert_eq!(undone.target.attempt_count, 1);
        assert_eq!(undone.target.remembered_count, 0);
        assert_eq!(undone.target.forgotten_count, 1);
        assert_eq!(undone.target.last_attempt_id, Some(second.attempt.id));
        assert_eq!(
            undone.target.next_review_at_unix_ms,
            NOW + 200 + DAY_UNIX_MS
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn quality_feedback_is_separate_persistent_and_undoable() {
        let (directory, path, mut store) = setup(1);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let target_before = page.items[0].target.clone();
        let save = SaveReviewQualityFeedbackInput {
            feed_item_id: page.items[0].id,
            learning_record_id: page.items[0].learning_record.id,
            card_context_key: "recorded".to_string(),
            expected_revision: None,
            polarity: ReviewQualityPolarity::Down,
            reason_codes: vec!["unclear_prompt".to_string()],
            detail: Some("  原句上下文还不够完整。  ".to_string()),
            request_key: "quality-save-1".to_string(),
        };
        let feedback = store.save_quality_feedback(&save, NOW + 100).unwrap();
        let retry = store.save_quality_feedback(&save, NOW + 200).unwrap();
        assert_eq!(feedback.id, retry.id);
        assert_eq!(feedback.detail.as_deref(), Some("原句上下文还不够完整。"));
        let target_after = load_target(&store.connection, target_before.learning_target_id)
            .unwrap()
            .unwrap()
            .target;
        assert_eq!(target_after.revision, target_before.revision);
        assert_eq!(
            target_after.next_review_at_unix_ms,
            target_before.next_review_at_unix_ms
        );

        let undo = UndoReviewQualityFeedbackInput {
            feedback_id: feedback.id,
            feed_item_id: feedback.feed_item_id,
            learning_record_id: feedback.learning_record_id,
            expected_revision: feedback.revision,
            request_key: "quality-undo-1".to_string(),
        };
        let undone = store.undo_quality_feedback(&undo, NOW + 300).unwrap();
        assert!(!undone.active);
        drop(store);
        let reopened = ReviewStore::open(&path).unwrap();
        let persisted = load_quality_feedback(&reopened.connection, feedback.feed_item_id)
            .unwrap()
            .unwrap();
        assert!(!persisted.active);
        assert_eq!(persisted.revision, feedback.revision + 1);
        drop(reopened);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn version_sixteen_repairs_registered_v15_intermediate_feedback_and_keeps_writes_usable() {
        let (directory, path, mut store) = setup(2);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 2, NOW)
            .unwrap();
        let recorded_item_id = page.items[0].id;
        let recorded_record_id = page.items[0].learning_record.id;
        let generated_item_id = page.items[1].id;
        let generated_record_id = page.items[1].learning_record.id;
        let generated_card_id = insert_generated_test_card(
            &store.connection,
            generated_record_id,
            0,
            "migration-v16-generated-card",
            NOW + 10,
            NOW + GENERATED_CARD_TTL_UNIX_MS,
            NOW + 10,
        );
        store
            .connection
            .execute(
                "UPDATE review_feed_items SET generated_card_id = ?1 WHERE id = ?2",
                params![generated_card_id, generated_item_id],
            )
            .unwrap();
        store
            .connection
            .execute_batch(
                "DELETE FROM schema_migrations WHERE version IN (16, 17);
                 DROP TABLE learning_record_targets;
                 DROP TABLE review_quality_feedback;
                 CREATE TABLE review_quality_feedback (
                   id INTEGER PRIMARY KEY AUTOINCREMENT,
                   feed_item_id INTEGER NOT NULL REFERENCES review_feed_items(id) ON DELETE CASCADE,
                   learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
                   generated_card_id INTEGER,
                   revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
                   active INTEGER NOT NULL CHECK (active IN (0, 1)),
                   polarity TEXT NOT NULL CHECK (polarity IN ('up', 'down')),
                   reason_codes_json TEXT NOT NULL,
                   detail TEXT,
                   created_at_unix_ms INTEGER NOT NULL,
                   updated_at_unix_ms INTEGER NOT NULL,
                   UNIQUE(feed_item_id)
                 );
                 CREATE INDEX idx_review_quality_feedback_record
                   ON review_quality_feedback(learning_record_id, updated_at_unix_ms DESC, id DESC);",
            )
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO review_quality_feedback (
                   id, feed_item_id, learning_record_id, generated_card_id, revision, active,
                   polarity, reason_codes_json, detail, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (701, ?1, ?2, NULL, 3, 1, 'up', '[\"needed\"]',
                           '保留 recorded 详情', 1111, 1222)",
                params![recorded_item_id, recorded_record_id],
            )
            .unwrap();
        store
            .connection
            .execute(
                "INSERT INTO review_quality_feedback (
                   id, feed_item_id, learning_record_id, generated_card_id, revision, active,
                   polarity, reason_codes_json, detail, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (702, ?1, ?2, ?3, 4, 1, 'down', '[\"unclear_prompt\"]',
                           '保留 generated 详情', 2111, 2222)",
                params![generated_item_id, generated_record_id, generated_card_id],
            )
            .unwrap();
        let registered_versions: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        // v1-21 全量注册后删除 v16/v17，模拟"登记 1-15 + 中间版 feedback"的旧库。
        assert_eq!(registered_versions, 19);
        drop(store);

        let mut upgraded = ReviewStore::open(&path).unwrap();
        let version: i64 = upgraded
            .connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let context_column_count: i64 = upgraded
            .connection
            .query_row(
                "SELECT COUNT(*)
                 FROM pragma_table_info('review_quality_feedback')
                 WHERE name = 'card_context_key'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let recorded: (
            Option<i64>,
            String,
            i64,
            i64,
            String,
            String,
            Option<String>,
            i64,
            i64,
        ) = upgraded
            .connection
            .query_row(
                "SELECT generated_card_id, card_context_key, revision, active, polarity,
                            reason_codes_json, detail, created_at_unix_ms, updated_at_unix_ms
                     FROM review_quality_feedback WHERE id = 701",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .unwrap();
        let generated: (
            Option<i64>,
            String,
            i64,
            i64,
            String,
            String,
            Option<String>,
            i64,
            i64,
        ) = upgraded
            .connection
            .query_row(
                "SELECT generated_card_id, card_context_key, revision, active, polarity,
                            reason_codes_json, detail, created_at_unix_ms, updated_at_unix_ms
                     FROM review_quality_feedback WHERE id = 702",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .unwrap();

        // 修复后数据库推进到当前最新 schema 版本（随 DATABASE_SCHEMA_VERSION 递增）。
        assert_eq!(version, 21);
        assert_eq!(context_column_count, 1);
        assert!(
            quality_feedback_unique_indexes(&upgraded.connection).contains(&vec![
                "feed_item_id".to_string(),
                "card_context_key".to_string(),
            ])
        );
        assert_eq!(
            recorded,
            (
                None,
                "recorded".to_string(),
                3,
                1,
                "up".to_string(),
                "[\"needed\"]".to_string(),
                Some("保留 recorded 详情".to_string()),
                1111,
                1222,
            )
        );
        assert_eq!(
            generated,
            (
                Some(generated_card_id),
                format!("generated:{generated_card_id}"),
                4,
                1,
                "down".to_string(),
                "[\"unclear_prompt\"]".to_string(),
                Some("保留 generated 详情".to_string()),
                2111,
                2222,
            )
        );

        let alternate_card_id = insert_generated_test_card(
            &upgraded.connection,
            recorded_record_id,
            0,
            "migration-v16-alternate-card",
            NOW + 20,
            NOW + GENERATED_CARD_TTL_UNIX_MS,
            NOW + 20,
        );
        let alternate_context = format!("generated:{alternate_card_id}");
        upgraded
            .connection
            .execute(
                "INSERT INTO review_quality_feedback (
                   feed_item_id, learning_record_id, generated_card_id, card_context_key,
                   revision, active, polarity, reason_codes_json, detail,
                   created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, 0, 1, 'up', '[]', NULL, 3000, 3000)",
                params![
                    recorded_item_id,
                    recorded_record_id,
                    alternate_card_id,
                    alternate_context
                ],
            )
            .unwrap();
        let duplicate = upgraded.connection.execute(
            "INSERT INTO review_quality_feedback (
               feed_item_id, learning_record_id, generated_card_id, card_context_key,
               revision, active, polarity, reason_codes_json, detail,
               created_at_unix_ms, updated_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, 0, 1, 'up', '[]', NULL, 3001, 3001)",
            params![
                recorded_item_id,
                recorded_record_id,
                alternate_card_id,
                alternate_context
            ],
        );
        assert!(duplicate.is_err());

        let saved = upgraded
            .save_quality_feedback(
                &SaveReviewQualityFeedbackInput {
                    feed_item_id: generated_item_id,
                    learning_record_id: generated_record_id,
                    card_context_key: format!("generated:{generated_card_id}"),
                    expected_revision: Some(4),
                    polarity: ReviewQualityPolarity::Up,
                    reason_codes: vec!["helpful_context".to_string()],
                    detail: Some("升级后仍可保存".to_string()),
                    request_key: "migration-v16-save".to_string(),
                },
                NOW + 100,
            )
            .unwrap();
        assert_eq!(saved.revision, 5);
        assert!(saved.active);
        let undone = upgraded
            .undo_quality_feedback(
                &UndoReviewQualityFeedbackInput {
                    feedback_id: saved.id,
                    feed_item_id: saved.feed_item_id,
                    learning_record_id: saved.learning_record_id,
                    expected_revision: saved.revision,
                    request_key: "migration-v16-undo".to_string(),
                },
                NOW + 200,
            )
            .unwrap();
        assert_eq!(undone.revision, 6);
        assert!(!undone.active);
        drop(upgraded);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn version_sixteen_keeps_current_feedback_schema_and_data_unchanged() {
        let (directory, path, mut store) = setup(1);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let saved = store
            .save_quality_feedback(
                &SaveReviewQualityFeedbackInput {
                    feed_item_id: page.items[0].id,
                    learning_record_id: page.items[0].learning_record.id,
                    card_context_key: "recorded".to_string(),
                    expected_revision: None,
                    polarity: ReviewQualityPolarity::Down,
                    reason_codes: vec!["unwanted_source".to_string()],
                    detail: Some("正确结构应保持原样".to_string()),
                    request_key: "migration-v16-noop-save".to_string(),
                },
                NOW + 100,
            )
            .unwrap();
        store
            .connection
            .execute("DELETE FROM schema_migrations WHERE version = 16", [])
            .unwrap();
        drop(store);

        let reopened = ReviewStore::open(&path).unwrap();
        let persisted = load_quality_feedback(&reopened.connection, saved.feed_item_id)
            .unwrap()
            .unwrap();
        let mutation_count: i64 = reopened
            .connection
            .query_row(
                "SELECT COUNT(*) FROM review_quality_mutations
                 WHERE request_key = 'migration-v16-noop-save'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted.id, saved.id);
        assert_eq!(persisted.revision, saved.revision);
        assert_eq!(persisted.active, saved.active);
        assert_eq!(persisted.detail, saved.detail);
        assert_eq!(persisted.created_at_unix_ms, saved.created_at_unix_ms);
        assert_eq!(persisted.updated_at_unix_ms, saved.updated_at_unix_ms);
        assert_eq!(mutation_count, 1);
        drop(reopened);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn due_targets_precede_new_records_with_factual_reason_codes() {
        let (directory, _path, mut store) = setup(2);
        store
            .connection
            .execute(
                "INSERT INTO learning_target_review_states (
                   learning_target_id, revision, next_review_at_unix_ms, attempt_count,
                   remembered_count, forgotten_count, success_streak,
                   created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (1, 0, ?1, 1, 1, 0, 1, ?2, ?2)",
                params![DAY_START - DAY_UNIX_MS, NOW],
            )
            .unwrap();
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 2, NOW)
            .unwrap();
        assert_eq!(page.items[0].learning_record.id, 1);
        assert_eq!(page.items[0].reason_code, ReviewReasonCode::ScheduledToday);
        assert_eq!(page.items[1].reason_code, ReviewReasonCode::NewRecord);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generated_card_payload_requires_english_context_and_hides_answer_in_hint() {
        let (directory, _path, store) = setup(1);
        let record = get_learning_record_from_connection(&store.connection, 1)
            .unwrap()
            .unwrap();
        let valid = GeneratedReviewCardPayload {
            english_context: "I used same 1 when comparing the two approaches.".to_string(),
            english_context_zh: "我在比较这两种方法时使用了 same 1。".to_string(),
            hint: "想想表示两个方案相似的表达。".to_string(),
        };
        assert!(validate_generated_card_payload(&record, &valid).is_ok());

        let mut chinese_context = valid;
        chinese_context.english_context = "这是 same 1 的中文语境。".to_string();
        assert!(validate_generated_card_payload(&record, &chinese_context).is_err());

        let leaked_hint = GeneratedReviewCardPayload {
            english_context: "I used same 1 when comparing the two approaches.".to_string(),
            english_context_zh: "我比较了这两种方法。".to_string(),
            hint: "答案是 same 1。".to_string(),
        };
        assert!(validate_generated_card_payload(&record, &leaked_hint).is_err());
        let embedded_word = GeneratedReviewCardPayload {
            english_context: "The same 10 examples belong to another target.".to_string(),
            english_context_zh: "这些例子属于另一个目标。".to_string(),
            hint: "回忆目标表达。".to_string(),
        };
        assert!(validate_generated_card_payload(&record, &embedded_word).is_err());
        let query_only = GeneratedReviewCardPayload {
            english_context: "same 1".to_string(),
            english_context_zh: "相同。".to_string(),
            hint: "回忆目标表达。".to_string(),
        };
        assert!(validate_generated_card_payload(&record, &query_only).is_err());
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generated_card_is_persisted_by_request_and_pool_identity() {
        let (directory, path, mut store) = setup(1);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let payload = GeneratedReviewCardPayload {
            english_context: "I used same 1 while comparing the two approaches.".to_string(),
            english_context_zh: "我在比较这两种方法时使用了 same 1。".to_string(),
            hint: "想想表示两个方案相似的表达。".to_string(),
        };
        store
            .connection
            .execute(
                "INSERT INTO review_generated_cards (
                   learning_record_id, learning_target_id, variant_index, generation_request_key,
                   content_json, model, created_at_unix_ms, expires_at_unix_ms,
                   last_used_at_unix_ms, use_count
                 ) VALUES (1, ?1, 7, 'generated-stable-1', ?2, 'test-model', ?3, ?4, ?3, 1)",
                params![
                    page.items[0].target.learning_target_id,
                    serde_json::to_string(&payload).unwrap(),
                    NOW,
                    NOW + GENERATED_CARD_TTL_UNIX_MS
                ],
            )
            .unwrap();
        let card_id = store.connection.last_insert_rowid();
        attach_generated_card(&store.connection, page.items[0].id, card_id).unwrap();

        let by_request =
            load_generated_card_by_request_key(&store.connection, "generated-stable-1")
                .unwrap()
                .unwrap();
        let reusable = load_reusable_generated_card(
            &store.connection,
            1,
            page.items[0].target.learning_target_id,
            DAY_START + DAY_UNIX_MS,
            page.items[0].id + 1,
            NOW + 1,
        )
        .unwrap()
        .unwrap();
        assert_eq!(by_request.id, reusable.id);
        assert_eq!(by_request.variant_index, 7);
        assert_eq!(by_request.english_context, payload.english_context);

        drop(store);
        let mut reopened = ReviewStore::open(&path).unwrap();
        let restored = reopened
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW + 1)
            .unwrap();
        assert_eq!(
            restored.items[0].generated_card.as_ref().unwrap().id,
            card_id
        );
        drop(reopened);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generated_card_preflight_reuses_valid_pool_card_across_days_not_same_day() {
        let (directory, path, mut store) = setup(1);
        let first_page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let first_item = &first_page.items[0];
        let card_id = insert_generated_test_card(
            &store.connection,
            1,
            0,
            "pool-reuse-source",
            NOW,
            NOW + GENERATED_CARD_TTL_UNIX_MS,
            NOW,
        );
        attach_generated_card(&store.connection, first_item.id, card_id).unwrap();
        store
            .submit_outcome(&submit_input(first_item, "pool-reuse-outcome"), NOW + 1)
            .unwrap();
        let second_page = store
            .load_feed_page(DAY_START, DAY_END, first_page.next_cursor, 1, NOW + 2)
            .unwrap();
        let same_day_input = PrepareReviewFeedCardInput {
            feed_item_id: second_page.items[0].id,
            learning_record_id: 1,
            learning_target_id: second_page.items[0].target.learning_target_id,
            request_key: "review-card-same-day-next-cycle".to_string(),
            explicit_retry: false,
        };
        assert!(matches!(
            prepare_generated_card_preflight(&path, &same_day_input, NOW + 3).unwrap(),
            GeneratedCardPreflight::Generate { .. }
        ));

        let next_day_start = DAY_START + DAY_UNIX_MS;
        let next_day_page = store
            .load_feed_page(
                next_day_start,
                next_day_start + DAY_UNIX_MS,
                None,
                1,
                NOW + DAY_UNIX_MS,
            )
            .unwrap();
        let next_day_input = PrepareReviewFeedCardInput {
            feed_item_id: next_day_page.items[0].id,
            learning_record_id: 1,
            learning_target_id: next_day_page.items[0].target.learning_target_id,
            request_key: "review-card-next-day-reuse".to_string(),
            explicit_retry: false,
        };
        let reused =
            prepare_generated_card_preflight(&path, &next_day_input, NOW + DAY_UNIX_MS + 1)
                .unwrap();
        let GeneratedCardPreflight::Ready(reused) = reused else {
            panic!("有效卡片应在下一天直接复用，不应再次调用模型");
        };
        let attached_card_id: Option<i64> = store
            .connection
            .query_row(
                "SELECT generated_card_id FROM review_feed_items WHERE id = ?1",
                [next_day_page.items[0].id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reused.id, card_id);
        assert_eq!(attached_card_id, Some(card_id));
        assert_eq!(reused.use_count, 2);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fourth_and_later_cycles_reuse_three_card_pool_without_new_generation() {
        let (directory, path, mut store) = setup(1);
        let mut page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let mut card_ids = Vec::new();
        for cycle_index in 0..GENERATED_CARD_PER_RECORD_CAPACITY {
            let item = &page.items[0];
            assert_eq!(item.cycle_index, cycle_index);
            let card_id = insert_generated_test_card(
                &store.connection,
                1,
                cycle_index,
                &format!("three-card-pool-{cycle_index}"),
                NOW + cycle_index,
                NOW + GENERATED_CARD_TTL_UNIX_MS,
                NOW + cycle_index,
            );
            card_ids.push(card_id);
            attach_generated_card(&store.connection, item.id, card_id).unwrap();
            store
                .submit_outcome(
                    &submit_input(item, &format!("three-card-outcome-{cycle_index}")),
                    NOW + 10 + cycle_index,
                )
                .unwrap();
            page = store
                .load_feed_page(
                    DAY_START,
                    DAY_END,
                    page.next_cursor,
                    1,
                    NOW + 20 + cycle_index,
                )
                .unwrap();
        }

        assert_eq!(
            page.items[0].cycle_index,
            GENERATED_CARD_PER_RECORD_CAPACITY
        );
        let fourth_input = PrepareReviewFeedCardInput {
            feed_item_id: page.items[0].id,
            learning_record_id: 1,
            learning_target_id: page.items[0].target.learning_target_id,
            request_key: "fourth-cycle-stable-key".to_string(),
            explicit_retry: false,
        };
        let GeneratedCardPreflight::Ready(fourth_card) =
            prepare_generated_card_preflight(&path, &fourth_input, NOW + 40).unwrap()
        else {
            panic!("第四轮必须复用三卡池，不能再次请求模型");
        };
        assert_eq!(fourth_card.id, card_ids[0]);
        store
            .submit_outcome(
                &submit_input(&page.items[0], "fourth-cycle-outcome"),
                NOW + 41,
            )
            .unwrap();
        let fifth_page = store
            .load_feed_page(DAY_START, DAY_END, page.next_cursor, 1, NOW + 42)
            .unwrap();
        let fifth_input = PrepareReviewFeedCardInput {
            feed_item_id: fifth_page.items[0].id,
            learning_record_id: 1,
            learning_target_id: fifth_page.items[0].target.learning_target_id,
            request_key: "fifth-cycle-stable-key".to_string(),
            explicit_retry: false,
        };
        let GeneratedCardPreflight::Ready(fifth_card) =
            prepare_generated_card_preflight(&path, &fifth_input, NOW + 43).unwrap()
        else {
            panic!("第五轮必须继续复用三卡池，不能再次请求模型");
        };
        let pool_count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM review_generated_cards WHERE learning_record_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fifth_card.id, card_ids[1]);
        assert_eq!(pool_count, GENERATED_CARD_PER_RECORD_CAPACITY);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generation_failure_backoff_survives_restart_and_explicit_retry_keeps_key() {
        let (directory, path, mut store) = setup(1);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let input = PrepareReviewFeedCardInput {
            feed_item_id: page.items[0].id,
            learning_record_id: 1,
            learning_target_id: page.items[0].target.learning_target_id,
            request_key: "persisted-generation-failure".to_string(),
            explicit_retry: false,
        };
        persist_generation_failure(&path, &input, "model unavailable", NOW).unwrap();
        drop(store);

        let mut reopened = ReviewStore::open(&path).unwrap();
        let restored = reopened
            .load_feed_item_state(input.feed_item_id, NOW + 1)
            .unwrap();
        let failure = restored.item.generation_failure.unwrap();
        assert_eq!(failure.request_key, input.request_key);
        assert_eq!(failure.failure_count, 1);
        assert_eq!(
            failure.retry_after_unix_ms,
            NOW + GENERATED_CARD_FAILURE_BACKOFF_BASE_UNIX_MS
        );
        let deferred = prepare_generated_card_preflight(&path, &input, NOW + 1).unwrap();
        assert!(matches!(
            deferred,
            GeneratedCardPreflight::Deferred { retry_after_unix_ms, .. }
                if retry_after_unix_ms == failure.retry_after_unix_ms
        ));

        let explicit_retry = PrepareReviewFeedCardInput {
            explicit_retry: true,
            ..input.clone()
        };
        let generated = prepare_generated_card_preflight(&path, &explicit_retry, NOW + 2).unwrap();
        assert!(matches!(generated, GeneratedCardPreflight::Generate { .. }));
        assert_eq!(explicit_retry.request_key, input.request_key);
        persist_generation_failure(&path, &explicit_retry, "model still unavailable", NOW + 3)
            .unwrap();
        let retried_state = reopened
            .load_feed_item_state(input.feed_item_id, NOW + 4)
            .unwrap();
        let retried_failure = retried_state.item.generation_failure.unwrap();
        assert_eq!(retried_failure.failure_count, 2);
        assert_eq!(
            retried_failure.retry_after_unix_ms,
            NOW + 3 + GENERATED_CARD_FAILURE_BACKOFF_BASE_UNIX_MS * 2
        );
        drop(reopened);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generated_card_pool_expires_cards_and_enforces_per_record_capacity() {
        let (directory, _path, mut store) = setup(1);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let expired_id = insert_generated_test_card(
            &store.connection,
            1,
            0,
            "pool-expired",
            NOW - 10,
            NOW,
            NOW - 10,
        );
        attach_generated_card(&store.connection, page.items[0].id, expired_id).unwrap();
        maintain_generated_card_pool(&store.connection, NOW).unwrap();
        let detached: Option<i64> = store
            .connection
            .query_row(
                "SELECT generated_card_id FROM review_feed_items WHERE id = ?1",
                [page.items[0].id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(detached, None);

        let mut inserted = Vec::new();
        for variant_index in 1..=4 {
            inserted.push(insert_generated_test_card(
                &store.connection,
                1,
                variant_index,
                &format!("pool-per-record-{variant_index}"),
                NOW + variant_index,
                NOW + GENERATED_CARD_TTL_UNIX_MS,
                NOW + variant_index,
            ));
        }
        maintain_generated_card_pool(&store.connection, NOW + 10).unwrap();
        let remaining: Vec<i64> = {
            let mut statement = store
                .connection
                .prepare(
                    "SELECT id FROM review_generated_cards
                     WHERE learning_record_id = 1 ORDER BY last_used_at_unix_ms",
                )
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(remaining.len() as i64, GENERATED_CARD_PER_RECORD_CAPACITY);
        assert!(!remaining.contains(&inserted[0]));
        assert_eq!(remaining, inserted[1..].to_vec());
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generated_card_capacity_isolated_per_occurrence_within_one_target() {
        let (directory, path, mut store) = setup(2);
        merge_record_into_target(&store.connection, 2, 1);
        let first_page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        assert_eq!(first_page.items[0].learning_record.id, 1);
        let learning_target_id = first_page.items[0].target.learning_target_id;
        for variant_index in 0..GENERATED_CARD_PER_RECORD_CAPACITY {
            insert_generated_test_card(
                &store.connection,
                1,
                variant_index,
                &format!("first-occurrence-card-{variant_index}"),
                NOW + variant_index,
                NOW + GENERATED_CARD_TTL_UNIX_MS,
                NOW + variant_index,
            );
        }
        store
            .submit_outcome(
                &submit_input(&first_page.items[0], "rotate-to-second-occurrence"),
                NOW + 10,
            )
            .unwrap();
        let second_page = store
            .load_feed_page(DAY_START, DAY_END, first_page.next_cursor, 1, NOW + 11)
            .unwrap();
        assert_eq!(second_page.items[0].learning_record.id, 2);
        assert_eq!(
            second_page.items[0].target.learning_target_id,
            learning_target_id
        );

        let input = PrepareReviewFeedCardInput {
            feed_item_id: second_page.items[0].id,
            learning_record_id: 2,
            learning_target_id,
            request_key: "second-occurrence-first-card".to_string(),
            explicit_retry: false,
        };
        assert!(matches!(
            prepare_generated_card_preflight(&path, &input, NOW + 12).unwrap(),
            GeneratedCardPreflight::Generate { .. }
        ));
        assert!(
            load_reusable_generated_card(
                &store.connection,
                2,
                learning_target_id,
                DAY_START,
                second_page.items[0].id,
                NOW + 12,
            )
            .unwrap()
            .is_none(),
            "生成卡不能从同 target 的其他 occurrence 跨 record 复用"
        );

        insert_generated_test_card(
            &store.connection,
            2,
            0,
            "second-occurrence-card-0",
            NOW + 20,
            NOW + GENERATED_CARD_TTL_UNIX_MS,
            NOW + 20,
        );
        maintain_generated_card_pool(&store.connection, NOW + 21).unwrap();
        assert!(
            load_generated_card_at_capacity(&store.connection, 2, learning_target_id, NOW + 21)
                .unwrap()
                .is_none(),
            "第二个 record 未达到自身容量时必须继续允许生成"
        );
        let counts_after_first: (i64, i64) = store
            .connection
            .query_row(
                "SELECT SUM(CASE WHEN learning_record_id = 1 THEN 1 ELSE 0 END),
                        SUM(CASE WHEN learning_record_id = 2 THEN 1 ELSE 0 END)
                 FROM review_generated_cards WHERE learning_target_id = ?1",
                [learning_target_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts_after_first, (3, 1));

        for variant_index in 1..GENERATED_CARD_PER_RECORD_CAPACITY {
            insert_generated_test_card(
                &store.connection,
                2,
                variant_index,
                &format!("second-occurrence-card-{variant_index}"),
                NOW + 20 + variant_index,
                NOW + GENERATED_CARD_TTL_UNIX_MS,
                NOW + 20 + variant_index,
            );
        }
        maintain_generated_card_pool(&store.connection, NOW + 30).unwrap();
        let reused =
            load_generated_card_at_capacity(&store.connection, 2, learning_target_id, NOW + 30)
                .unwrap()
                .unwrap();
        assert_eq!(reused.learning_record_id, 2);
        assert_eq!(reused.learning_target_id, learning_target_id);
        let final_counts: (i64, i64) = store
            .connection
            .query_row(
                "SELECT SUM(CASE WHEN learning_record_id = 1 THEN 1 ELSE 0 END),
                        SUM(CASE WHEN learning_record_id = 2 THEN 1 ELSE 0 END)
                 FROM review_generated_cards WHERE learning_target_id = ?1",
                [learning_target_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(final_counts, (3, 3));
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generated_card_pool_enforces_global_capacity_for_unreferenced_cache() {
        let record_count = GENERATED_CARD_POOL_CAPACITY + 1;
        let (directory, _path, store) = setup(record_count);
        for learning_record_id in 1..=record_count {
            insert_generated_test_card(
                &store.connection,
                learning_record_id,
                0,
                &format!("pool-global-{learning_record_id}"),
                NOW + learning_record_id,
                NOW + GENERATED_CARD_TTL_UNIX_MS,
                NOW + learning_record_id,
            );
        }
        maintain_generated_card_pool(&store.connection, NOW).unwrap();
        let remaining: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM review_generated_cards", [], |row| {
                row.get(0)
            })
            .unwrap();
        let oldest_exists: bool = store
            .connection
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM review_generated_cards WHERE generation_request_key = 'pool-global-1'
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, GENERATED_CARD_POOL_CAPACITY);
        assert!(!oldest_exists);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn generated_card_pool_protects_257_feed_bound_cards_without_eviction_oscillation() {
        let record_count = GENERATED_CARD_POOL_CAPACITY + 1;
        let (directory, path, mut store) = setup(record_count);
        let mut cursor = None;
        let mut bound = Vec::new();
        while bound.len() < usize::try_from(record_count).unwrap() {
            let page = store
                .load_feed_page(DAY_START, DAY_END, cursor, MAX_REVIEW_FEED_PAGE_SIZE, NOW)
                .unwrap();
            assert!(!page.items.is_empty());
            for item in page.items {
                let card_id = insert_generated_test_card(
                    &store.connection,
                    item.learning_record.id,
                    0,
                    &format!("pool-bound-{}", item.learning_record.id),
                    NOW + item.learning_record.id,
                    NOW + GENERATED_CARD_TTL_UNIX_MS,
                    NOW + item.learning_record.id,
                );
                attach_generated_card(&store.connection, item.id, card_id).unwrap();
                cursor = Some(item.ordinal);
                bound.push((item.id, item.learning_record.id, card_id));
            }
        }
        assert_eq!(bound.len(), usize::try_from(record_count).unwrap());
        let earliest = bound[0];

        maintain_generated_card_pool(&store.connection, NOW + 10_000).unwrap();
        maintain_generated_card_pool(&store.connection, NOW + 20_000).unwrap();
        let remaining: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM review_generated_cards", [], |row| {
                row.get(0)
            })
            .unwrap();
        let earliest_binding: Option<i64> = store
            .connection
            .query_row(
                "SELECT generated_card_id FROM review_feed_items WHERE id = ?1",
                [earliest.0],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, record_count);
        assert_eq!(earliest_binding, Some(earliest.2));

        let preflight = prepare_generated_card_preflight(
            &path,
            &PrepareReviewFeedCardInput {
                feed_item_id: earliest.0,
                learning_record_id: earliest.1,
                learning_target_id: target_id_for_record(&store.connection, earliest.1),
                request_key: "pool-bound-earliest-reload".to_string(),
                explicit_retry: false,
            },
            NOW + 30_000,
        )
        .unwrap();
        assert!(matches!(
            preflight,
            GeneratedCardPreflight::Ready(GeneratedReviewCard { id, .. }) if id == earliest.2
        ));
        maintain_generated_card_pool(&store.connection, NOW + 40_000).unwrap();
        let stable_binding: Option<i64> = store
            .connection
            .query_row(
                "SELECT generated_card_id FROM review_feed_items WHERE id = ?1",
                [earliest.0],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stable_binding, Some(earliest.2));
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn quality_feedback_is_scoped_to_the_exact_feed_card_context() {
        let (directory, _path, mut store) = setup(1);
        let first_page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let first_item = &first_page.items[0];
        let first_card_id = insert_generated_test_card(
            &store.connection,
            1,
            0,
            "quality-context-first",
            NOW,
            NOW + GENERATED_CARD_TTL_UNIX_MS,
            NOW,
        );
        attach_generated_card(&store.connection, first_item.id, first_card_id).unwrap();
        let first_feedback = store
            .save_quality_feedback(
                &SaveReviewQualityFeedbackInput {
                    feed_item_id: first_item.id,
                    learning_record_id: 1,
                    card_context_key: format!("generated:{first_card_id}"),
                    expected_revision: None,
                    polarity: ReviewQualityPolarity::Down,
                    reason_codes: Vec::new(),
                    detail: None,
                    request_key: "quality-context-first-save".to_string(),
                },
                NOW + 1,
            )
            .unwrap();
        store
            .submit_outcome(
                &submit_input(first_item, "quality-context-outcome"),
                NOW + 2,
            )
            .unwrap();

        let second_page = store
            .load_feed_page(DAY_START, DAY_END, first_page.next_cursor, 1, NOW + 3)
            .unwrap();
        let second_item = &second_page.items[0];
        assert_eq!(second_item.cycle_index, 1);
        let second_card_id = insert_generated_test_card(
            &store.connection,
            1,
            1,
            "quality-context-second",
            NOW + 3,
            NOW + GENERATED_CARD_TTL_UNIX_MS,
            NOW + 3,
        );
        attach_generated_card(&store.connection, second_item.id, second_card_id).unwrap();
        let second_feedback = store
            .save_quality_feedback(
                &SaveReviewQualityFeedbackInput {
                    feed_item_id: second_item.id,
                    learning_record_id: 1,
                    card_context_key: format!("generated:{second_card_id}"),
                    expected_revision: None,
                    polarity: ReviewQualityPolarity::Up,
                    reason_codes: Vec::new(),
                    detail: None,
                    request_key: "quality-context-second-save".to_string(),
                },
                NOW + 4,
            )
            .unwrap();

        assert_ne!(first_feedback.id, second_feedback.id);
        assert_eq!(first_feedback.generated_card_id, Some(first_card_id));
        assert_eq!(second_feedback.generated_card_id, Some(second_card_id));
        assert_eq!(
            load_quality_feedback(&store.connection, first_item.id)
                .unwrap()
                .unwrap()
                .polarity,
            ReviewQualityPolarity::Down
        );
        assert_eq!(
            load_quality_feedback(&store.connection, second_item.id)
                .unwrap()
                .unwrap()
                .polarity,
            ReviewQualityPolarity::Up
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn regenerated_context_keeps_old_feedback_and_accepts_new_feedback() {
        let (directory, _path, mut store) = setup(1);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let item = &page.items[0];
        let first_card_id = insert_generated_test_card(
            &store.connection,
            1,
            0,
            "quality-regenerated-first",
            NOW,
            NOW + GENERATED_CARD_TTL_UNIX_MS,
            NOW,
        );
        attach_generated_card(&store.connection, item.id, first_card_id).unwrap();
        let first_feedback = store
            .save_quality_feedback(
                &SaveReviewQualityFeedbackInput {
                    feed_item_id: item.id,
                    learning_record_id: 1,
                    card_context_key: format!("generated:{first_card_id}"),
                    expected_revision: None,
                    polarity: ReviewQualityPolarity::Down,
                    reason_codes: Vec::new(),
                    detail: None,
                    request_key: "quality-regenerated-first-save".to_string(),
                },
                NOW + 1,
            )
            .unwrap();

        let second_card_id = insert_generated_test_card(
            &store.connection,
            1,
            1,
            "quality-regenerated-second",
            NOW + 2,
            NOW + GENERATED_CARD_TTL_UNIX_MS,
            NOW + 2,
        );
        store
            .connection
            .execute(
                "DELETE FROM review_generated_cards WHERE id = ?1",
                [first_card_id],
            )
            .unwrap();
        attach_generated_card(&store.connection, item.id, second_card_id).unwrap();
        assert!(load_quality_feedback(&store.connection, item.id)
            .unwrap()
            .is_none());
        let stale_context_error = store
            .save_quality_feedback(
                &SaveReviewQualityFeedbackInput {
                    feed_item_id: item.id,
                    learning_record_id: 1,
                    card_context_key: format!("generated:{first_card_id}"),
                    expected_revision: None,
                    polarity: ReviewQualityPolarity::Up,
                    reason_codes: Vec::new(),
                    detail: None,
                    request_key: "quality-regenerated-stale-context".to_string(),
                },
                NOW + 3,
            )
            .unwrap_err();
        assert!(stale_context_error.contains("具体卡片语境"));
        let second_feedback = store
            .save_quality_feedback(
                &SaveReviewQualityFeedbackInput {
                    feed_item_id: item.id,
                    learning_record_id: 1,
                    card_context_key: format!("generated:{second_card_id}"),
                    expected_revision: None,
                    polarity: ReviewQualityPolarity::Up,
                    reason_codes: Vec::new(),
                    detail: None,
                    request_key: "quality-regenerated-second-save".to_string(),
                },
                NOW + 4,
            )
            .unwrap();

        let feedback_count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM review_quality_feedback WHERE feed_item_id = ?1",
                [item.id],
                |row| row.get(0),
            )
            .unwrap();
        let old_polarity: String = store
            .connection
            .query_row(
                "SELECT polarity FROM review_quality_feedback
                 WHERE feed_item_id = ?1 AND generated_card_id = ?2",
                params![item.id, first_card_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(feedback_count, 2);
        assert_eq!(old_polarity, "down");
        assert_eq!(first_feedback.generated_card_id, Some(first_card_id));
        assert_eq!(second_feedback.generated_card_id, Some(second_card_id));
        assert_eq!(second_feedback.polarity, ReviewQualityPolarity::Up);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn feed_item_authority_state_recovers_persisted_outcome_and_feedback() {
        let (directory, _path, mut store) = setup(1);
        let page = store
            .load_feed_page(DAY_START, DAY_END, None, 1, NOW)
            .unwrap();
        let item = &page.items[0];
        let feedback = store
            .save_quality_feedback(
                &SaveReviewQualityFeedbackInput {
                    feed_item_id: item.id,
                    learning_record_id: item.learning_record.id,
                    card_context_key: "recorded".to_string(),
                    expected_revision: None,
                    polarity: ReviewQualityPolarity::Up,
                    reason_codes: Vec::new(),
                    detail: None,
                    request_key: "authority-feedback".to_string(),
                },
                NOW + 1,
            )
            .unwrap();
        let outcome = store
            .submit_outcome(&submit_input(item, "authority-outcome"), NOW + 2)
            .unwrap();

        let state = store.load_feed_item_state(item.id, NOW + 3).unwrap();
        assert_eq!(state.item.attempt.as_ref().unwrap().id, outcome.attempt.id);
        assert_eq!(state.item.target.revision, outcome.target.revision);
        assert_eq!(
            state.item.quality_feedback.as_ref().unwrap().id,
            feedback.id
        );
        assert_eq!(state.completed_count, 1);
        assert_eq!(state.remembered_count, 1);
        assert_eq!(state.forgotten_count, 0);
        assert!(state.can_continue);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn helper_rejects_missing_learning_record() {
        let (directory, _path, store) = setup(0);
        let error = ensure_learning_record_exists_raw(&store.connection, 99).unwrap_err();
        assert_eq!(error, "学习记录不存在。");
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
}
