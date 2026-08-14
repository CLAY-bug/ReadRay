use crate::explanation::{
    determine_query_direction, normalize_english_learning_target, validate_explanation_card,
    CaptureInput, ExplanationCard, QueryDirection, QueryType, SourceType,
};
use rusqlite::{
    params, params_from_iter, types::Value, Connection, OptionalExtension, Transaction,
};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const DATABASE_FILE_NAME: &str = "readray.sqlite3";
const DATABASE_SCHEMA_VERSION: i64 = 19;
const LEARNING_TARGET_CANONICALIZATION_VERSION: i64 = 1;
const REVIEW_DAY_UNIX_MS: i64 = 24 * 60 * 60 * 1_000;
pub const EXPLANATION_CARD_SCHEMA_VERSION: i64 = 2;
const DEFAULT_PAGE: u32 = 1;
const DEFAULT_PAGE_SIZE: u32 = 20;
const MAX_PAGE_SIZE: u32 = 100;

const MIGRATION_1: &str = r#"
CREATE TABLE learning_records (
  id INTEGER PRIMARY KEY,
  query_text TEXT NOT NULL,
  normalized_text TEXT NOT NULL,
  query_type TEXT NOT NULL CHECK (query_type IN ('word', 'phrase', 'sentence', 'paragraph')),
  source_type TEXT NOT NULL CHECK (source_type IN ('manual', 'clipboard', 'windows_uia', 'app_adapter', 'ocr')),
  source_app TEXT,
  context_text TEXT,
  explanation_card_json TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  difficulty TEXT
);

CREATE INDEX idx_learning_records_created_at ON learning_records(created_at_unix_ms DESC, id DESC);
CREATE INDEX idx_learning_records_query_type_created_at ON learning_records(query_type, created_at_unix_ms DESC, id DESC);
CREATE INDEX idx_learning_records_normalized_text ON learning_records(normalized_text);
"#;

const MIGRATION_2: &str = r#"
CREATE TABLE quick_ai_conversations (
  id INTEGER PRIMARY KEY,
  title TEXT,
  model TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL
);

CREATE TABLE quick_ai_messages (
  id INTEGER PRIMARY KEY,
  conversation_id INTEGER NOT NULL REFERENCES quick_ai_conversations(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
  content TEXT NOT NULL,
  sequence INTEGER NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  UNIQUE(conversation_id, sequence)
);

CREATE INDEX idx_quick_ai_conversations_updated_at
  ON quick_ai_conversations(updated_at_unix_ms DESC, id DESC);
CREATE INDEX idx_quick_ai_messages_conversation_sequence
  ON quick_ai_messages(conversation_id, sequence);
"#;

const MIGRATION_3: &str = r#"
CREATE TABLE writing_documents (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL,
  last_opened_at_unix_ms INTEGER,
  draft_title TEXT,
  draft_paragraphs_json TEXT,
  draft_updated_at_unix_ms INTEGER,
  completed_title TEXT,
  completed_paragraphs_json TEXT,
  completed_at_unix_ms INTEGER,
  comparison_baseline_title TEXT NOT NULL,
  comparison_baseline_paragraphs_json TEXT NOT NULL,
  CHECK (
    (draft_title IS NULL AND draft_paragraphs_json IS NULL AND draft_updated_at_unix_ms IS NULL)
    OR
    (draft_title IS NOT NULL AND draft_paragraphs_json IS NOT NULL AND draft_updated_at_unix_ms IS NOT NULL)
  ),
  CHECK (
    (completed_title IS NULL AND completed_paragraphs_json IS NULL AND completed_at_unix_ms IS NULL)
    OR
    (completed_title IS NOT NULL AND completed_paragraphs_json IS NOT NULL AND completed_at_unix_ms IS NOT NULL)
  )
);

CREATE TABLE writing_analyses (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id INTEGER NOT NULL REFERENCES writing_documents(id) ON DELETE CASCADE,
  document_revision INTEGER NOT NULL,
  round INTEGER NOT NULL,
  analysis_json TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  UNIQUE(document_id, round)
);

CREATE TABLE writing_versions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id INTEGER NOT NULL REFERENCES writing_documents(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  source_revision INTEGER NOT NULL,
  title TEXT NOT NULL,
  paragraphs_json TEXT NOT NULL,
  comparison_baseline_title TEXT NOT NULL,
  comparison_baseline_paragraphs_json TEXT NOT NULL,
  analysis_json TEXT,
  completed_at_unix_ms INTEGER NOT NULL,
  UNIQUE(document_id, ordinal)
);

CREATE TABLE writing_assistant_answers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id INTEGER NOT NULL REFERENCES writing_documents(id) ON DELETE CASCADE,
  document_revision INTEGER NOT NULL,
  parent_answer_id INTEGER REFERENCES writing_assistant_answers(id) ON DELETE SET NULL,
  question TEXT NOT NULL,
  scope TEXT NOT NULL CHECK (scope IN ('document', 'paragraph', 'selection')),
  selection_text TEXT,
  answer_json TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  created_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_writing_documents_updated_at
  ON writing_documents(updated_at_unix_ms DESC, id DESC);
CREATE INDEX idx_writing_analyses_document_round
  ON writing_analyses(document_id, round DESC);
CREATE INDEX idx_writing_versions_document_ordinal
  ON writing_versions(document_id, ordinal DESC);
CREATE INDEX idx_writing_answers_document_created
  ON writing_assistant_answers(document_id, created_at_unix_ms, id);
"#;

const MIGRATION_4: &str = r#"
ALTER TABLE writing_documents
  ADD COLUMN comparison_baseline_revision INTEGER;

ALTER TABLE writing_versions
  ADD COLUMN analysis_revision INTEGER;

ALTER TABLE writing_versions
  ADD COLUMN comparison_baseline_revision INTEGER;

ALTER TABLE writing_assistant_answers
  ADD COLUMN version_id INTEGER REFERENCES writing_versions(id) ON DELETE CASCADE;

CREATE INDEX idx_writing_answers_version_created
  ON writing_assistant_answers(version_id, created_at_unix_ms, id);
"#;

const MIGRATION_5: &str = r#"
CREATE TABLE model_usage_records (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  category TEXT NOT NULL CHECK (category IN ('explanation_query', 'quick_ai', 'writing')),
  prompt_tokens INTEGER NOT NULL CHECK (prompt_tokens >= 0),
  completion_tokens INTEGER NOT NULL CHECK (completion_tokens >= 0),
  total_tokens INTEGER NOT NULL CHECK (total_tokens >= 0 AND total_tokens = prompt_tokens + completion_tokens),
  created_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_model_usage_records_created_category
  ON model_usage_records(created_at_unix_ms, category);
"#;

const MIGRATION_6: &str = r#"
CREATE TABLE app_preferences (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  ui_font TEXT NOT NULL CHECK (ui_font IN ('geist_source_han_sans', 'source_han_sans')),
  ui_font_size INTEGER NOT NULL CHECK (ui_font_size BETWEEN 12 AND 20),
  learning_font TEXT NOT NULL CHECK (learning_font IN ('newsreader_source_han_serif', 'source_han_serif')),
  learning_font_size INTEGER NOT NULL CHECK (learning_font_size BETWEEN 14 AND 24),
  send_shortcut TEXT NOT NULL CHECK (send_shortcut IN ('enter', 'ctrl_enter'))
);

INSERT INTO app_preferences (
  id,
  revision,
  ui_font,
  ui_font_size,
  learning_font,
  learning_font_size,
  send_shortcut
) VALUES (
  1,
  0,
  'geist_source_han_sans',
  14,
  'newsreader_source_han_serif',
  17,
  'enter'
);
"#;

const MIGRATION_7: &str = r#"
ALTER TABLE app_preferences
  ADD COLUMN close_behavior TEXT NOT NULL DEFAULT 'hide_to_tray'
  CHECK (close_behavior IN ('hide_to_tray', 'exit'));

ALTER TABLE app_preferences
  ADD COLUMN quick_query_shortcut TEXT NOT NULL DEFAULT 'Ctrl+Alt+R';

ALTER TABLE app_preferences
  ADD COLUMN selection_explanation_shortcut TEXT NOT NULL DEFAULT 'Ctrl+Alt+U';
"#;

const MIGRATION_8: &str = r#"
CREATE TABLE custom_themes (
  id TEXT PRIMARY KEY,
  manifest_json TEXT NOT NULL,
  light_colors_json TEXT,
  dark_colors_json TEXT,
  warnings_json TEXT NOT NULL,
  imported_at_unix_ms INTEGER NOT NULL
);

CREATE TABLE theme_preferences (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  theme_id TEXT NOT NULL DEFAULT 'readray-default',
  mode TEXT NOT NULL DEFAULT 'light' CHECK (mode IN ('light', 'dark'))
);

INSERT INTO theme_preferences (id, revision, theme_id, mode)
VALUES (1, 0, 'readray-default', 'light');
"#;

const MIGRATION_9: &str = r#"
ALTER TABLE quick_ai_conversations
  ADD COLUMN origin TEXT NOT NULL DEFAULT 'legacy'
  CHECK (origin IN ('overlay', 'main', 'legacy'));

CREATE INDEX idx_quick_ai_conversations_origin_updated_at
  ON quick_ai_conversations(origin, updated_at_unix_ms DESC, id DESC);
"#;

const MIGRATION_10: &str = r#"
DELETE FROM quick_ai_conversations
WHERE origin = 'legacy';
"#;

const MIGRATION_11: &str = r#"
CREATE TABLE review_targets (
  learning_record_id INTEGER PRIMARY KEY REFERENCES learning_records(id) ON DELETE CASCADE,
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  next_review_at_unix_ms INTEGER NOT NULL,
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  remembered_count INTEGER NOT NULL DEFAULT 0 CHECK (remembered_count >= 0),
  forgotten_count INTEGER NOT NULL DEFAULT 0 CHECK (forgotten_count >= 0),
  success_streak INTEGER NOT NULL DEFAULT 0 CHECK (success_streak >= 0),
  last_reviewed_at_unix_ms INTEGER,
  last_outcome TEXT CHECK (last_outcome IN ('remembered', 'forgotten')),
  last_used_hint INTEGER CHECK (last_used_hint IN (0, 1)),
  last_attempt_id INTEGER,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_review_targets_due
  ON review_targets(next_review_at_unix_ms, learning_record_id);

CREATE TABLE review_daily_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  day_start_unix_ms INTEGER NOT NULL,
  day_end_unix_ms INTEGER NOT NULL CHECK (day_end_unix_ms > day_start_unix_ms),
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  reason_code TEXT NOT NULL CHECK (reason_code IN ('scheduled_today', 'new_record')),
  created_at_unix_ms INTEGER NOT NULL,
  UNIQUE(day_start_unix_ms, learning_record_id),
  UNIQUE(day_start_unix_ms, ordinal)
);

CREATE INDEX idx_review_daily_items_day
  ON review_daily_items(day_start_unix_ms, ordinal);

CREATE TABLE review_attempts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  daily_item_id INTEGER NOT NULL REFERENCES review_daily_items(id) ON DELETE CASCADE,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  request_key TEXT NOT NULL UNIQUE,
  expected_revision INTEGER NOT NULL CHECK (expected_revision >= 0),
  target_revision INTEGER NOT NULL CHECK (target_revision > expected_revision),
  outcome TEXT NOT NULL CHECK (outcome IN ('remembered', 'forgotten')),
  used_hint INTEGER NOT NULL CHECK (used_hint IN (0, 1)),
  next_review_at_unix_ms INTEGER NOT NULL,
  previous_next_review_at_unix_ms INTEGER NOT NULL,
  previous_attempt_count INTEGER NOT NULL CHECK (previous_attempt_count >= 0),
  previous_remembered_count INTEGER NOT NULL CHECK (previous_remembered_count >= 0),
  previous_forgotten_count INTEGER NOT NULL CHECK (previous_forgotten_count >= 0),
  previous_success_streak INTEGER NOT NULL CHECK (previous_success_streak >= 0),
  previous_last_reviewed_at_unix_ms INTEGER,
  previous_last_outcome TEXT CHECK (previous_last_outcome IN ('remembered', 'forgotten')),
  previous_last_used_hint INTEGER CHECK (previous_last_used_hint IN (0, 1)),
  previous_last_attempt_id INTEGER,
  created_at_unix_ms INTEGER NOT NULL,
  undone_at_unix_ms INTEGER,
  undo_request_key TEXT UNIQUE,
  undo_expected_revision INTEGER,
  undo_target_revision INTEGER
);

CREATE UNIQUE INDEX idx_review_attempts_active_daily_item
  ON review_attempts(daily_item_id)
  WHERE undone_at_unix_ms IS NULL;

CREATE INDEX idx_review_attempts_learning_record
  ON review_attempts(learning_record_id, created_at_unix_ms DESC, id DESC);

CREATE TABLE review_quality_feedback (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  learning_record_id INTEGER NOT NULL UNIQUE REFERENCES learning_records(id) ON DELETE CASCADE,
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  active INTEGER NOT NULL CHECK (active IN (0, 1)),
  polarity TEXT NOT NULL CHECK (polarity IN ('up', 'down')),
  reason_codes_json TEXT NOT NULL,
  detail TEXT,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL
);

CREATE TABLE review_quality_mutations (
  request_key TEXT PRIMARY KEY,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  operation TEXT NOT NULL CHECK (operation IN ('save', 'undo')),
  input_json TEXT NOT NULL,
  result_json TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_review_quality_mutations_learning_record
  ON review_quality_mutations(learning_record_id, created_at_unix_ms DESC);
"#;

const MIGRATION_12: &str = r#"
ALTER TABLE model_usage_records RENAME TO model_usage_records_v11;

CREATE TABLE model_usage_records (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  category TEXT NOT NULL CHECK (
    category IN ('explanation_query', 'quick_ai', 'writing', 'review_card')
  ),
  prompt_tokens INTEGER NOT NULL CHECK (prompt_tokens >= 0),
  completion_tokens INTEGER NOT NULL CHECK (completion_tokens >= 0),
  total_tokens INTEGER NOT NULL CHECK (
    total_tokens >= 0 AND total_tokens = prompt_tokens + completion_tokens
  ),
  created_at_unix_ms INTEGER NOT NULL
);

INSERT INTO model_usage_records (
  id, category, prompt_tokens, completion_tokens, total_tokens, created_at_unix_ms
)
SELECT id, category, prompt_tokens, completion_tokens, total_tokens, created_at_unix_ms
FROM model_usage_records_v11;

DROP TABLE model_usage_records_v11;

CREATE INDEX idx_model_usage_records_created_category
  ON model_usage_records(created_at_unix_ms, category);

CREATE TABLE review_generated_cards (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  variant_index INTEGER NOT NULL CHECK (variant_index >= 0),
  generation_request_key TEXT NOT NULL UNIQUE,
  content_json TEXT NOT NULL,
  model TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  UNIQUE(learning_record_id, variant_index)
);

CREATE INDEX idx_review_generated_cards_learning_record
  ON review_generated_cards(learning_record_id, variant_index);

CREATE TABLE review_feed_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  day_start_unix_ms INTEGER NOT NULL,
  day_end_unix_ms INTEGER NOT NULL CHECK (day_end_unix_ms > day_start_unix_ms),
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  cycle_index INTEGER NOT NULL CHECK (cycle_index >= 0),
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  reason_code TEXT NOT NULL CHECK (
    reason_code IN ('scheduled_today', 'new_record', 'continued_practice')
  ),
  generated_card_id INTEGER REFERENCES review_generated_cards(id) ON DELETE SET NULL,
  created_at_unix_ms INTEGER NOT NULL,
  UNIQUE(day_start_unix_ms, cycle_index, learning_record_id),
  UNIQUE(day_start_unix_ms, ordinal)
);

CREATE INDEX idx_review_feed_items_day
  ON review_feed_items(day_start_unix_ms, ordinal);

INSERT INTO review_feed_items (
  id, day_start_unix_ms, day_end_unix_ms, learning_record_id,
  cycle_index, ordinal, reason_code, generated_card_id, created_at_unix_ms
)
SELECT id, day_start_unix_ms, day_end_unix_ms, learning_record_id,
       0, ordinal, reason_code, NULL, created_at_unix_ms
FROM review_daily_items;

CREATE TABLE review_feed_attempts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  feed_item_id INTEGER NOT NULL REFERENCES review_feed_items(id) ON DELETE CASCADE,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  request_key TEXT NOT NULL UNIQUE,
  expected_revision INTEGER NOT NULL CHECK (expected_revision >= 0),
  target_revision INTEGER NOT NULL CHECK (target_revision > expected_revision),
  outcome TEXT NOT NULL CHECK (outcome IN ('remembered', 'forgotten')),
  used_hint INTEGER NOT NULL CHECK (used_hint IN (0, 1)),
  next_review_at_unix_ms INTEGER NOT NULL,
  previous_next_review_at_unix_ms INTEGER NOT NULL,
  previous_attempt_count INTEGER NOT NULL CHECK (previous_attempt_count >= 0),
  previous_remembered_count INTEGER NOT NULL CHECK (previous_remembered_count >= 0),
  previous_forgotten_count INTEGER NOT NULL CHECK (previous_forgotten_count >= 0),
  previous_success_streak INTEGER NOT NULL CHECK (previous_success_streak >= 0),
  previous_last_reviewed_at_unix_ms INTEGER,
  previous_last_outcome TEXT CHECK (previous_last_outcome IN ('remembered', 'forgotten')),
  previous_last_used_hint INTEGER CHECK (previous_last_used_hint IN (0, 1)),
  previous_last_attempt_id INTEGER,
  created_at_unix_ms INTEGER NOT NULL,
  undone_at_unix_ms INTEGER,
  undo_request_key TEXT UNIQUE,
  undo_expected_revision INTEGER,
  undo_target_revision INTEGER
);

CREATE UNIQUE INDEX idx_review_feed_attempts_active_item
  ON review_feed_attempts(feed_item_id)
  WHERE undone_at_unix_ms IS NULL;

CREATE INDEX idx_review_feed_attempts_learning_record
  ON review_feed_attempts(learning_record_id, created_at_unix_ms DESC, id DESC);

INSERT INTO review_feed_attempts (
  id, feed_item_id, learning_record_id, request_key, expected_revision,
  target_revision, outcome, used_hint, next_review_at_unix_ms,
  previous_next_review_at_unix_ms, previous_attempt_count,
  previous_remembered_count, previous_forgotten_count,
  previous_success_streak, previous_last_reviewed_at_unix_ms,
  previous_last_outcome, previous_last_used_hint, previous_last_attempt_id,
  created_at_unix_ms, undone_at_unix_ms, undo_request_key,
  undo_expected_revision, undo_target_revision
)
SELECT id, daily_item_id, learning_record_id, request_key, expected_revision,
       target_revision, outcome, used_hint, next_review_at_unix_ms,
       previous_next_review_at_unix_ms, previous_attempt_count,
       previous_remembered_count, previous_forgotten_count,
       previous_success_streak, previous_last_reviewed_at_unix_ms,
       previous_last_outcome, previous_last_used_hint, previous_last_attempt_id,
       created_at_unix_ms, undone_at_unix_ms, undo_request_key,
       undo_expected_revision, undo_target_revision
FROM review_attempts;
"#;

const MIGRATION_13: &str = r#"
ALTER TABLE review_generated_cards
  ADD COLUMN expires_at_unix_ms INTEGER NOT NULL DEFAULT 0;

ALTER TABLE review_generated_cards
  ADD COLUMN last_used_at_unix_ms INTEGER NOT NULL DEFAULT 0;

ALTER TABLE review_generated_cards
  ADD COLUMN use_count INTEGER NOT NULL DEFAULT 0 CHECK (use_count >= 0);

UPDATE review_generated_cards
SET expires_at_unix_ms = created_at_unix_ms,
    last_used_at_unix_ms = created_at_unix_ms,
    use_count = 1;

CREATE INDEX idx_review_generated_cards_pool
  ON review_generated_cards(expires_at_unix_ms, last_used_at_unix_ms, id);

ALTER TABLE review_quality_mutations RENAME TO review_quality_mutations_v12;
ALTER TABLE review_quality_feedback RENAME TO review_quality_feedback_v12;

CREATE TABLE review_quality_feedback (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  feed_item_id INTEGER NOT NULL REFERENCES review_feed_items(id) ON DELETE CASCADE,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  generated_card_id INTEGER,
  card_context_key TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  active INTEGER NOT NULL CHECK (active IN (0, 1)),
  polarity TEXT NOT NULL CHECK (polarity IN ('up', 'down')),
  reason_codes_json TEXT NOT NULL,
  detail TEXT,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL,
  UNIQUE(feed_item_id, card_context_key)
);

CREATE INDEX idx_review_quality_feedback_record
  ON review_quality_feedback(learning_record_id, updated_at_unix_ms DESC, id DESC);

INSERT INTO review_quality_feedback (
  id, feed_item_id, learning_record_id, generated_card_id, card_context_key, revision, active,
  polarity, reason_codes_json, detail, created_at_unix_ms, updated_at_unix_ms
)
SELECT q.id, fi.id, q.learning_record_id, fi.generated_card_id,
       CASE
         WHEN fi.generated_card_id IS NULL THEN 'recorded'
         ELSE 'generated:' || fi.generated_card_id
       END,
       q.revision, q.active,
       q.polarity, q.reason_codes_json, q.detail, q.created_at_unix_ms, q.updated_at_unix_ms
FROM review_quality_feedback_v12 q
JOIN review_feed_items fi ON fi.id = (
  SELECT candidate.id
  FROM review_feed_items candidate
  WHERE candidate.learning_record_id = q.learning_record_id
  ORDER BY candidate.day_start_unix_ms DESC, candidate.ordinal DESC, candidate.id DESC
  LIMIT 1
);

CREATE TABLE review_quality_mutations (
  request_key TEXT PRIMARY KEY,
  feed_item_id INTEGER NOT NULL REFERENCES review_feed_items(id) ON DELETE CASCADE,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  operation TEXT NOT NULL CHECK (operation IN ('save', 'undo')),
  input_json TEXT NOT NULL,
  result_json TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_review_quality_mutations_item
  ON review_quality_mutations(feed_item_id, created_at_unix_ms DESC);

DROP TABLE review_quality_mutations_v12;
DROP TABLE review_quality_feedback_v12;
"#;

const MIGRATION_14: &str = r#"
DELETE FROM review_feed_items AS later
WHERE EXISTS (
  SELECT 1
  FROM review_feed_items AS earlier
  WHERE earlier.day_start_unix_ms = later.day_start_unix_ms
    AND earlier.cycle_index < later.cycle_index
    AND NOT EXISTS (
      SELECT 1
      FROM review_feed_attempts AS attempt
      WHERE attempt.feed_item_id = earlier.id
        AND attempt.undone_at_unix_ms IS NULL
    )
)
AND NOT EXISTS (
  SELECT 1
  FROM review_feed_attempts AS later_attempt
  WHERE later_attempt.feed_item_id = later.id
)
AND NOT EXISTS (
  SELECT 1
  FROM review_quality_feedback AS later_feedback
  WHERE later_feedback.feed_item_id = later.id
)
AND NOT EXISTS (
  SELECT 1
  FROM review_quality_mutations AS later_mutation
  WHERE later_mutation.feed_item_id = later.id
);

CREATE TABLE review_card_generation_failures (
  request_key TEXT PRIMARY KEY,
  feed_item_id INTEGER NOT NULL REFERENCES review_feed_items(id) ON DELETE CASCADE,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  failure_count INTEGER NOT NULL CHECK (failure_count > 0),
  retry_after_unix_ms INTEGER NOT NULL,
  last_error TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_review_card_generation_failures_retry
  ON review_card_generation_failures(retry_after_unix_ms, feed_item_id);
"#;

const MIGRATION_15: &str = r#"
-- review_targets 的一致性审计与修复需要按 active attempt 的时间顺序
-- 重放调度，由 migrate 中的 repair_review_targets_v15 在同一事务内执行。
"#;

const MIGRATION_16: &str = r#"
-- 已登记 v13 的数据库可能仍保留缺少 card_context_key 的中间版反馈表。
-- 结构检测、数据回填与条件重建由 migrate 中的 repair_review_quality_feedback_v16
-- 在同一迁移事务内执行。
"#;

const MIGRATION_17: &str = r#"
CREATE TABLE learning_record_targets (
  learning_record_id INTEGER PRIMARY KEY REFERENCES learning_records(id) ON DELETE CASCADE,
  query_direction TEXT NOT NULL CHECK (query_direction IN ('en_to_zh', 'zh_to_en')),
  learning_target_text TEXT NOT NULL,
  normalized_target_text TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_learning_record_targets_normalized
  ON learning_record_targets(normalized_target_text, learning_record_id);
"#;

const MIGRATION_18: &str = r#"
CREATE TABLE explanation_card_cache (
  cache_key TEXT PRIMARY KEY,
  normalized_source_text TEXT NOT NULL,
  query_direction TEXT NOT NULL CHECK (query_direction IN ('en_to_zh', 'zh_to_en')),
  query_type TEXT NOT NULL CHECK (query_type IN ('word', 'phrase', 'sentence', 'paragraph')),
  minimal_context_fingerprint TEXT NOT NULL,
  model_id TEXT NOT NULL,
  model_revision TEXT NOT NULL,
  prompt_version TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  explanation_card_json TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  last_accessed_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_explanation_card_cache_maintenance
  ON explanation_card_cache(last_accessed_at_unix_ms, created_at_unix_ms, cache_key);
"#;

const MIGRATION_19: &str = r#"
CREATE TABLE learning_targets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  stable_key TEXT NOT NULL UNIQUE,
  target_kind TEXT NOT NULL DEFAULT 'learnable' CHECK (
    target_kind IN ('learnable', 'legacy_compat')
  ),
  canonicalization_version INTEGER NOT NULL CHECK (canonicalization_version >= 0),
  query_type TEXT NOT NULL CHECK (query_type IN ('word', 'phrase', 'sentence', 'paragraph')),
  display_target_text TEXT NOT NULL,
  normalized_target_text TEXT,
  representative_learning_record_id INTEGER REFERENCES learning_records(id) ON DELETE SET NULL,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL,
  CHECK (
    (target_kind = 'learnable' AND canonicalization_version > 0
      AND normalized_target_text IS NOT NULL AND length(normalized_target_text) > 0)
    OR
    (target_kind = 'legacy_compat' AND canonicalization_version = 0
      AND normalized_target_text IS NULL)
  ),
  UNIQUE(canonicalization_version, query_type, normalized_target_text)
);

CREATE INDEX idx_learning_targets_recent
  ON learning_targets(updated_at_unix_ms DESC, id DESC);
CREATE INDEX idx_learning_targets_query_type_recent
  ON learning_targets(query_type, updated_at_unix_ms DESC, id DESC);

CREATE TABLE learning_target_occurrences (
  learning_record_id INTEGER PRIMARY KEY REFERENCES learning_records(id) ON DELETE CASCADE,
  learning_target_id INTEGER NOT NULL REFERENCES learning_targets(id) ON DELETE RESTRICT,
  canonicalization_version INTEGER NOT NULL CHECK (canonicalization_version >= 0),
  binding_revision INTEGER NOT NULL DEFAULT 0 CHECK (binding_revision >= 0),
  bound_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_learning_target_occurrences_target
  ON learning_target_occurrences(learning_target_id, learning_record_id);

CREATE TABLE learning_target_review_states (
  learning_target_id INTEGER PRIMARY KEY REFERENCES learning_targets(id) ON DELETE CASCADE,
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  next_review_at_unix_ms INTEGER NOT NULL,
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  remembered_count INTEGER NOT NULL DEFAULT 0 CHECK (remembered_count >= 0),
  forgotten_count INTEGER NOT NULL DEFAULT 0 CHECK (forgotten_count >= 0),
  success_streak INTEGER NOT NULL DEFAULT 0 CHECK (success_streak >= 0),
  last_reviewed_at_unix_ms INTEGER,
  last_outcome TEXT CHECK (last_outcome IN ('remembered', 'forgotten')),
  last_used_hint INTEGER CHECK (last_used_hint IN (0, 1)),
  last_attempt_id INTEGER,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL
);

CREATE INDEX idx_learning_target_review_states_due
  ON learning_target_review_states(next_review_at_unix_ms, learning_target_id);
"#;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, MIGRATION_1),
    (2, MIGRATION_2),
    (3, MIGRATION_3),
    (4, MIGRATION_4),
    (5, MIGRATION_5),
    (6, MIGRATION_6),
    (7, MIGRATION_7),
    (8, MIGRATION_8),
    (9, MIGRATION_9),
    (10, MIGRATION_10),
    (11, MIGRATION_11),
    (12, MIGRATION_12),
    (13, MIGRATION_13),
    (14, MIGRATION_14),
    (15, MIGRATION_15),
    (16, MIGRATION_16),
    (17, MIGRATION_17),
    (18, MIGRATION_18),
    (DATABASE_SCHEMA_VERSION, MIGRATION_19),
];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningRecord {
    pub id: i64,
    pub learning_target_id: i64,
    pub query_text: String,
    pub learning_target_text: String,
    pub query_direction: QueryDirection,
    pub normalized_text: String,
    pub query_type: QueryType,
    pub source_type: SourceType,
    pub source_app: Option<String>,
    pub context_text: Option<String>,
    pub explanation_card: ExplanationCard,
    pub schema_version: i64,
    pub created_at_unix_ms: i64,
    pub difficulty: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningRecordPage {
    pub records: Vec<LearningRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayLearningSummary {
    pub record_count: u64,
    pub latest_record: Option<LearningRecord>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningTargetSummary {
    pub id: i64,
    pub stable_key: String,
    pub canonicalization_version: i64,
    pub query_type: QueryType,
    pub learning_target_text: String,
    pub normalized_target_text: String,
    pub query_count: u64,
    pub first_seen_at_unix_ms: i64,
    pub last_seen_at_unix_ms: i64,
    pub representative_record: LearningRecord,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningTargetPage {
    pub targets: Vec<LearningTargetSummary>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningTargetDetail {
    pub target: LearningTargetSummary,
    pub occurrences: Vec<LearningRecord>,
}

struct StoredLearningRecord {
    id: i64,
    learning_target_id: i64,
    query_text: String,
    normalized_text: String,
    query_type: String,
    source_type: String,
    source_app: Option<String>,
    context_text: Option<String>,
    explanation_card_json: String,
    schema_version: i64,
    created_at_unix_ms: i64,
    difficulty: Option<String>,
    learning_target_text: String,
    query_direction: String,
}

struct LearningRecordStore {
    connection: Connection,
}

impl LearningRecordStore {
    fn open(path: &Path) -> Result<Self, String> {
        Ok(Self {
            connection: open_database(path)?,
        })
    }

    fn save(&self, input: &CaptureInput, card: &ExplanationCard) -> Result<LearningRecord, String> {
        validate_explanation_card(input, card).map_err(|errors| {
            format!(
                "学习记录未保存：ExplanationCard 校验失败：{}",
                errors.join("；")
            )
        })?;

        let explanation_card_json = serde_json::to_string(card)
            .map_err(|error| format!("学习记录无法序列化 ExplanationCard：{error}"))?;
        let created_at_unix_ms = unix_time_ms()?;
        let normalized_text = normalize_query_text(&input.query_text);

        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| format!("学习记录写入事务无法开始：{error}"))?;
        transaction
            .execute(
                "INSERT INTO learning_records (
                    query_text, normalized_text, query_type, source_type, source_app, context_text,
                    explanation_card_json, schema_version, created_at_unix_ms, difficulty
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    input.query_text.trim(),
                    normalized_text,
                    query_type_to_storage(card.query_type()),
                    source_type_to_storage(&input.source_type),
                    input.source_app.as_deref(),
                    input.context_text.as_deref(),
                    explanation_card_json,
                    EXPLANATION_CARD_SCHEMA_VERSION,
                    created_at_unix_ms,
                    Option::<String>::None,
                ],
            )
            .map_err(|error| format!("学习记录写入失败：{error}"))?;
        let learning_record_id = transaction.last_insert_rowid();
        let query_direction = determine_query_direction(&input.query_text)?;
        let learning_target_text = card.learning_target_text().trim();
        transaction
            .execute(
                "INSERT INTO learning_record_targets (
                   learning_record_id, query_direction, learning_target_text,
                   normalized_target_text, created_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    learning_record_id,
                    query_direction_to_storage(query_direction),
                    learning_target_text,
                    normalize_query_text(learning_target_text),
                    created_at_unix_ms,
                ],
            )
            .map_err(|error| format!("规范英文学习目标写入失败：{error}"))?;
        bind_learning_record_to_stable_target(
            &transaction,
            learning_record_id,
            card.query_type(),
            learning_target_text,
            created_at_unix_ms,
        )
        .map_err(|error| format!("稳定学习目标写入失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("学习记录写入事务无法提交：{error}"))?;

        self.get(learning_record_id)?
            .ok_or_else(|| "学习记录写入后无法读取新记录。".to_string())
    }

    fn get(&self, id: i64) -> Result<Option<LearningRecord>, String> {
        get_learning_record_from_connection(&self.connection, id)
    }

    fn delete(&mut self, id: i64) -> Result<bool, String> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("学习记录删除事务无法开始：{error}"))?;
        let learning_target_id: Option<i64> = transaction
            .query_row(
                "SELECT learning_target_id FROM learning_target_occurrences
                 WHERE learning_record_id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("学习记录所属目标读取失败：{error}"))?;
        let affected = transaction
            .execute("DELETE FROM learning_records WHERE id = ?1", [id])
            .map_err(|error| format!("学习记录删除失败：{error}"))?;

        if affected > 0 {
            if let Some(learning_target_id) = learning_target_id {
                let representative: Option<(i64, String, i64)> = transaction
                    .query_row(
                        "SELECT record.id, projection.learning_target_text,
                                record.created_at_unix_ms
                         FROM learning_target_occurrences occurrence
                         JOIN learning_records record ON record.id = occurrence.learning_record_id
                         JOIN learning_record_targets projection
                           ON projection.learning_record_id = record.id
                         WHERE occurrence.learning_target_id = ?1
                         ORDER BY record.created_at_unix_ms DESC, record.id DESC
                         LIMIT 1",
                        [learning_target_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .optional()
                    .map_err(|error| format!("学习目标代表 occurrence 重选失败：{error}"))?;
                if let Some((representative_id, display_text, updated_at)) = representative {
                    transaction
                        .execute(
                            "UPDATE learning_targets
                             SET representative_learning_record_id = ?1,
                                 display_target_text = ?2,
                                 updated_at_unix_ms = ?3
                             WHERE id = ?4",
                            params![
                                representative_id,
                                display_text,
                                updated_at,
                                learning_target_id
                            ],
                        )
                        .map_err(|error| format!("学习目标代表 occurrence 更新失败：{error}"))?;
                } else {
                    transaction
                        .execute(
                            "DELETE FROM learning_targets
                             WHERE id = ?1
                               AND NOT EXISTS (
                                 SELECT 1 FROM review_feed_items WHERE learning_target_id = ?1
                               )
                               AND NOT EXISTS (
                                 SELECT 1 FROM review_generated_cards WHERE learning_target_id = ?1
                               )",
                            [learning_target_id],
                        )
                        .map_err(|error| format!("空学习目标清理失败：{error}"))?;
                }
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("学习记录删除事务提交失败：{error}"))?;
        Ok(affected > 0)
    }

    fn list(
        &self,
        page: Option<u32>,
        page_size: Option<u32>,
        keyword: Option<&str>,
        query_type: Option<QueryType>,
    ) -> Result<LearningRecordPage, String> {
        let (page, page_size) = validate_pagination(page, page_size)?;
        let (where_clause, mut values) = build_filter(keyword, query_type)?;
        let count_sql = format!(
            "SELECT COUNT(*) FROM learning_records lr
             JOIN learning_record_targets lrt ON lrt.learning_record_id = lr.id {where_clause}"
        );
        let total: i64 = self
            .connection
            .query_row(&count_sql, params_from_iter(values.iter()), |row| {
                row.get(0)
            })
            .map_err(|error| format!("学习记录总数读取失败：{error}"))?;

        let offset = i64::from(page.saturating_sub(1)) * i64::from(page_size);
        values.push(Value::Integer(i64::from(page_size)));
        values.push(Value::Integer(offset));
        let list_sql = format!(
            "{} {where_clause} ORDER BY lr.created_at_unix_ms DESC, lr.id DESC LIMIT ? OFFSET ?",
            select_learning_record_sql("")
        );
        let mut statement = self
            .connection
            .prepare(&list_sql)
            .map_err(|error| format!("学习记录分页读取语句无法准备：{error}"))?;
        let rows = statement
            .query_map(params_from_iter(values.iter()), read_stored_learning_record)
            .map_err(|error| format!("学习记录分页读取失败：{error}"))?;
        let mut records = Vec::new();
        for row in rows {
            let stored = row.map_err(|error| format!("学习记录行读取失败：{error}"))?;
            records.push(decode_learning_record(stored)?);
        }

        Ok(LearningRecordPage {
            records,
            page,
            page_size,
            total: u64::try_from(total)
                .map_err(|_| "学习记录总数无效，数据库返回了负数。".to_string())?,
        })
    }

    fn summarize_range(
        &self,
        start_unix_ms: i64,
        end_unix_ms: i64,
    ) -> Result<TodayLearningSummary, String> {
        if end_unix_ms <= start_unix_ms {
            return Err("今日学习记录时间范围无效。".to_string());
        }

        let record_count: i64 = self
            .connection
            .query_row(
                "SELECT COUNT(*) FROM learning_records lr
                 JOIN learning_record_targets lrt ON lrt.learning_record_id = lr.id
                 WHERE lr.created_at_unix_ms >= ?1 AND lr.created_at_unix_ms < ?2",
                params![start_unix_ms, end_unix_ms],
                |row| row.get(0),
            )
            .map_err(|error| format!("今日学习记录数量读取失败：{error}"))?;
        let mut statement = self
            .connection
            .prepare(&format!(
                "{} WHERE lr.created_at_unix_ms >= ?1 AND lr.created_at_unix_ms < ?2
                 ORDER BY lr.created_at_unix_ms DESC, lr.id DESC LIMIT 1",
                select_learning_record_sql("")
            ))
            .map_err(|error| format!("今日最近学习记录语句无法准备：{error}"))?;
        let latest_stored = statement
            .query_row(
                params![start_unix_ms, end_unix_ms],
                read_stored_learning_record,
            )
            .optional()
            .map_err(|error| format!("今日最近学习记录读取失败：{error}"))?;

        Ok(TodayLearningSummary {
            record_count: u64::try_from(record_count)
                .map_err(|_| "今日学习记录数量无效，数据库返回了负数。".to_string())?,
            latest_record: latest_stored.map(decode_learning_record).transpose()?,
        })
    }

    fn list_targets(
        &self,
        page: Option<u32>,
        page_size: Option<u32>,
        keyword: Option<&str>,
        query_type: Option<QueryType>,
    ) -> Result<LearningTargetPage, String> {
        let (page, page_size) = validate_pagination(page, page_size)?;
        let (where_clause, mut values) = build_target_filter(keyword, query_type)?;
        let count_sql = format!(
            "SELECT COUNT(*) FROM learning_targets lt
             WHERE lt.target_kind = 'learnable'
               AND EXISTS (
               SELECT 1 FROM learning_target_occurrences lto
               WHERE lto.learning_target_id = lt.id
             ) {where_clause}"
        );
        let total: i64 = self
            .connection
            .query_row(&count_sql, params_from_iter(values.iter()), |row| {
                row.get(0)
            })
            .map_err(|error| format!("学习目标总数读取失败：{error}"))?;

        let offset = i64::from(page.saturating_sub(1)) * i64::from(page_size);
        let order_clause = build_target_order_clause(keyword, &mut values)?;
        values.push(Value::Integer(i64::from(page_size)));
        values.push(Value::Integer(offset));
        let list_sql = format!(
            "SELECT lt.id, lt.stable_key, lt.canonicalization_version, lt.query_type,
                    lt.display_target_text, lt.normalized_target_text,
                    COUNT(lto.learning_record_id), MIN(lr.created_at_unix_ms),
                    MAX(lr.created_at_unix_ms),
                    (
                      SELECT latest.learning_record_id
                      FROM learning_target_occurrences latest
                      JOIN learning_records latest_record ON latest_record.id = latest.learning_record_id
                      WHERE latest.learning_target_id = lt.id
                      ORDER BY latest_record.created_at_unix_ms DESC, latest.learning_record_id DESC
                      LIMIT 1
                    )
             FROM learning_targets lt
             JOIN learning_target_occurrences lto ON lto.learning_target_id = lt.id
             JOIN learning_records lr ON lr.id = lto.learning_record_id
             WHERE lt.target_kind = 'learnable' {where_clause}
             GROUP BY lt.id
             ORDER BY {order_clause}
             LIMIT ? OFFSET ?"
        );
        let mut statement = self
            .connection
            .prepare(&list_sql)
            .map_err(|error| format!("学习目标分页读取语句无法准备：{error}"))?;
        let rows = statement
            .query_map(params_from_iter(values.iter()), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            })
            .map_err(|error| format!("学习目标分页读取失败：{error}"))?;
        let mut targets = Vec::new();
        for row in rows {
            let row = row.map_err(|error| format!("学习目标行读取失败：{error}"))?;
            targets.push(decode_learning_target_summary(&self.connection, row)?);
        }

        Ok(LearningTargetPage {
            targets,
            page,
            page_size,
            total: u64::try_from(total)
                .map_err(|_| "学习目标总数无效，数据库返回了负数。".to_string())?,
        })
    }

    fn get_target(&self, id: i64) -> Result<Option<LearningTargetDetail>, String> {
        if id <= 0 {
            return Err("学习目标 ID 无效。".to_string());
        }
        let row = load_learning_target_summary_row(&self.connection, id)?;
        let Some(row) = row else { return Ok(None) };
        let target = decode_learning_target_summary(&self.connection, row)?;
        let mut statement = self
            .connection
            .prepare(&format!(
                "{} WHERE lto.learning_target_id = ?1
                 ORDER BY lr.created_at_unix_ms DESC, lr.id DESC",
                select_learning_record_sql("")
            ))
            .map_err(|error| format!("学习目标历史出现语句无法准备：{error}"))?;
        let rows = statement
            .query_map([id], read_stored_learning_record)
            .map_err(|error| format!("学习目标历史出现读取失败：{error}"))?;
        let mut occurrences = Vec::new();
        for row in rows {
            occurrences.push(decode_learning_record(
                row.map_err(|error| format!("学习目标历史出现行读取失败：{error}"))?,
            )?);
        }
        Ok(Some(LearningTargetDetail {
            target,
            occurrences,
        }))
    }
}

pub fn initialize_for_app(app: &AppHandle) -> Result<(), String> {
    open_database_for_app(app).map(|_| ())
}

pub(crate) fn open_database_for_app(app: &AppHandle) -> Result<Connection, String> {
    open_database(&database_path_for_app(app)?)
}

pub(crate) fn open_database(path: &Path) -> Result<Connection, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("ReadRay 数据库路径缺少父目录：{}", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("ReadRay 数据库目录无法创建：{error}"))?;

    let mut connection =
        Connection::open(path).map_err(|error| format!("ReadRay 数据库无法打开：{error}"))?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| format!("ReadRay 数据库外键配置失败：{error}"))?;
    migrate(&mut connection)?;

    Ok(connection)
}

#[derive(Debug)]
struct ReviewTargetAuditRow {
    learning_record_id: i64,
    revision: i64,
    next_review_at_unix_ms: i64,
    attempt_count: i64,
    remembered_count: i64,
    forgotten_count: i64,
    success_streak: i64,
    last_reviewed_at_unix_ms: Option<i64>,
    last_outcome: Option<String>,
    last_used_hint: Option<i64>,
    last_attempt_id: Option<i64>,
    learning_record_created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

#[derive(Debug, PartialEq, Eq)]
struct RecomputedReviewTarget {
    next_review_at_unix_ms: i64,
    attempt_count: i64,
    remembered_count: i64,
    forgotten_count: i64,
    success_streak: i64,
    last_reviewed_at_unix_ms: Option<i64>,
    last_outcome: Option<String>,
    last_used_hint: Option<i64>,
    last_attempt_id: Option<i64>,
}

fn review_quality_feedback_columns(transaction: &Transaction<'_>) -> Result<Vec<String>, String> {
    let mut statement = transaction
        .prepare("PRAGMA table_info(review_quality_feedback)")
        .map_err(|error| format!("卡片反馈 v16 结构读取语句无法准备：{error}"))?;
    let rows = statement
        .query_map([], |row| row.get(1))
        .map_err(|error| format!("卡片反馈 v16 结构读取失败：{error}"))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row.map_err(|error| format!("卡片反馈 v16 列名读取失败：{error}"))?);
    }
    Ok(columns)
}

fn review_quality_feedback_has_context_unique(
    transaction: &Transaction<'_>,
) -> Result<bool, String> {
    let index_names = {
        let mut statement = transaction
            .prepare(
                "SELECT name
                 FROM pragma_index_list('review_quality_feedback')
                 WHERE \"unique\" = 1
                 ORDER BY seq",
            )
            .map_err(|error| format!("卡片反馈 v16 唯一约束读取语句无法准备：{error}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("卡片反馈 v16 唯一约束读取失败：{error}"))?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row.map_err(|error| format!("卡片反馈 v16 唯一索引名读取失败：{error}"))?);
        }
        names
    };

    let mut has_context_unique = false;
    let mut has_legacy_feed_unique = false;
    for index_name in index_names {
        let columns = {
            let mut statement = transaction
                .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                .map_err(|error| format!("卡片反馈 v16 唯一索引读取语句无法准备：{error}"))?;
            let rows = statement
                .query_map([index_name], |row| row.get::<_, String>(0))
                .map_err(|error| format!("卡片反馈 v16 唯一索引读取失败：{error}"))?;
            let mut names = Vec::new();
            for row in rows {
                names.push(
                    row.map_err(|error| format!("卡片反馈 v16 唯一索引列读取失败：{error}"))?,
                );
            }
            names
        };
        has_context_unique |= columns == ["feed_item_id", "card_context_key"];
        has_legacy_feed_unique |= columns == ["feed_item_id"];
    }
    Ok(has_context_unique && !has_legacy_feed_unique)
}

fn repair_review_quality_feedback_v16(transaction: &Transaction<'_>) -> Result<(), String> {
    let columns = review_quality_feedback_columns(transaction)?;
    let required_columns = [
        "id",
        "feed_item_id",
        "learning_record_id",
        "generated_card_id",
        "revision",
        "active",
        "polarity",
        "reason_codes_json",
        "detail",
        "created_at_unix_ms",
        "updated_at_unix_ms",
    ];
    if let Some(missing) = required_columns
        .iter()
        .find(|required| !columns.iter().any(|column| column == **required))
    {
        return Err(format!(
            "卡片反馈 v16 无法识别现有表结构：缺少必需列 {missing}。"
        ));
    }

    let has_context_column = columns.iter().any(|column| column == "card_context_key");
    if has_context_column && review_quality_feedback_has_context_unique(transaction)? {
        return Ok(());
    }

    transaction
        .execute_batch(
            "CREATE TABLE review_quality_feedback_v16 (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               feed_item_id INTEGER NOT NULL REFERENCES review_feed_items(id) ON DELETE CASCADE,
               learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
               generated_card_id INTEGER,
               card_context_key TEXT NOT NULL,
               revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
               active INTEGER NOT NULL CHECK (active IN (0, 1)),
               polarity TEXT NOT NULL CHECK (polarity IN ('up', 'down')),
               reason_codes_json TEXT NOT NULL,
               detail TEXT,
               created_at_unix_ms INTEGER NOT NULL,
               updated_at_unix_ms INTEGER NOT NULL,
               UNIQUE(feed_item_id, card_context_key)
             );",
        )
        .map_err(|error| format!("卡片反馈 v16 新表创建失败：{error}"))?;

    let context_expression = if has_context_column {
        "card_context_key"
    } else {
        "CASE
           WHEN generated_card_id IS NULL THEN 'recorded'
           ELSE 'generated:' || generated_card_id
         END"
    };
    transaction
        .execute_batch(&format!(
            "INSERT INTO review_quality_feedback_v16 (
               id, feed_item_id, learning_record_id, generated_card_id, card_context_key,
               revision, active, polarity, reason_codes_json, detail,
               created_at_unix_ms, updated_at_unix_ms
             )
             SELECT id, feed_item_id, learning_record_id, generated_card_id,
                    {context_expression}, revision, active, polarity, reason_codes_json,
                    detail, created_at_unix_ms, updated_at_unix_ms
             FROM review_quality_feedback;

             DROP TABLE review_quality_feedback;
             ALTER TABLE review_quality_feedback_v16 RENAME TO review_quality_feedback;

             CREATE INDEX idx_review_quality_feedback_record
               ON review_quality_feedback(learning_record_id, updated_at_unix_ms DESC, id DESC);"
        ))
        .map_err(|error| format!("卡片反馈 v16 数据重建失败：{error}"))?;

    Ok(())
}

fn repair_review_targets_v15(
    transaction: &Transaction<'_>,
    repaired_at_unix_ms: i64,
) -> Result<(), String> {
    let targets = {
        let mut statement = transaction
            .prepare(
                "SELECT rt.learning_record_id, rt.revision, rt.next_review_at_unix_ms,
                        rt.attempt_count, rt.remembered_count, rt.forgotten_count,
                        rt.success_streak, rt.last_reviewed_at_unix_ms, rt.last_outcome,
                        rt.last_used_hint, rt.last_attempt_id, lr.created_at_unix_ms,
                        rt.updated_at_unix_ms
                 FROM review_targets rt
                 JOIN learning_records lr ON lr.id = rt.learning_record_id
                 ORDER BY rt.learning_record_id",
            )
            .map_err(|error| format!("复习目标 v15 审计语句无法准备：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok(ReviewTargetAuditRow {
                    learning_record_id: row.get(0)?,
                    revision: row.get(1)?,
                    next_review_at_unix_ms: row.get(2)?,
                    attempt_count: row.get(3)?,
                    remembered_count: row.get(4)?,
                    forgotten_count: row.get(5)?,
                    success_streak: row.get(6)?,
                    last_reviewed_at_unix_ms: row.get(7)?,
                    last_outcome: row.get(8)?,
                    last_used_hint: row.get(9)?,
                    last_attempt_id: row.get(10)?,
                    learning_record_created_at_unix_ms: row.get(11)?,
                    updated_at_unix_ms: row.get(12)?,
                })
            })
            .map_err(|error| format!("复习目标 v15 审计读取失败：{error}"))?;
        let mut values = Vec::new();
        for row in rows {
            values.push(row.map_err(|error| format!("复习目标 v15 审计行读取失败：{error}"))?);
        }
        values
    };

    for target in targets {
        let recomputed = recompute_review_target_from_active_attempts(transaction, &target)?;
        let current = RecomputedReviewTarget {
            next_review_at_unix_ms: target.next_review_at_unix_ms,
            attempt_count: target.attempt_count,
            remembered_count: target.remembered_count,
            forgotten_count: target.forgotten_count,
            success_streak: target.success_streak,
            last_reviewed_at_unix_ms: target.last_reviewed_at_unix_ms,
            last_outcome: target.last_outcome.clone(),
            last_used_hint: target.last_used_hint,
            last_attempt_id: target.last_attempt_id,
        };
        if current == recomputed {
            continue;
        }
        let repaired_revision = target
            .revision
            .checked_add(1)
            .ok_or_else(|| "复习目标 v15 修复 revision 超出可保存范围。".to_string())?;
        let affected = transaction
            .execute(
                "UPDATE review_targets
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
                     updated_at_unix_ms = MAX(updated_at_unix_ms, ?11)
                 WHERE learning_record_id = ?12 AND revision = ?13",
                params![
                    repaired_revision,
                    recomputed.next_review_at_unix_ms,
                    recomputed.attempt_count,
                    recomputed.remembered_count,
                    recomputed.forgotten_count,
                    recomputed.success_streak,
                    recomputed.last_reviewed_at_unix_ms,
                    recomputed.last_outcome,
                    recomputed.last_used_hint,
                    recomputed.last_attempt_id,
                    repaired_at_unix_ms.max(target.updated_at_unix_ms),
                    target.learning_record_id,
                    target.revision,
                ],
            )
            .map_err(|error| format!("复习目标 v15 一致性修复失败：{error}"))?;
        if affected != 1 {
            return Err("复习目标 v15 一致性修复时 revision 已变化。".to_string());
        }
    }
    Ok(())
}

fn recompute_review_target_from_active_attempts(
    transaction: &Transaction<'_>,
    target: &ReviewTargetAuditRow,
) -> Result<RecomputedReviewTarget, String> {
    let mut recomputed = RecomputedReviewTarget {
        next_review_at_unix_ms: target.learning_record_created_at_unix_ms,
        attempt_count: 0,
        remembered_count: 0,
        forgotten_count: 0,
        success_streak: 0,
        last_reviewed_at_unix_ms: None,
        last_outcome: None,
        last_used_hint: None,
        last_attempt_id: None,
    };
    let mut statement = transaction
        .prepare(
            "SELECT id, outcome, used_hint, created_at_unix_ms
             FROM review_feed_attempts
             WHERE learning_record_id = ?1 AND undone_at_unix_ms IS NULL
             ORDER BY created_at_unix_ms ASC, id ASC",
        )
        .map_err(|error| format!("复习目标 v15 active attempt 语句无法准备：{error}"))?;
    let rows = statement
        .query_map([target.learning_record_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| format!("复习目标 v15 active attempt 读取失败：{error}"))?;
    for row in rows {
        let (attempt_id, outcome, used_hint, created_at_unix_ms) =
            row.map_err(|error| format!("复习目标 v15 active attempt 行读取失败：{error}"))?;
        let used_hint = used_hint != 0;
        let days = match (outcome.as_str(), used_hint) {
            ("forgotten", _) => {
                recomputed.forgotten_count += 1;
                recomputed.success_streak = 0;
                1_i64
            }
            ("remembered", true) => {
                recomputed.remembered_count += 1;
                2_i64
            }
            ("remembered", false) => {
                recomputed.remembered_count += 1;
                recomputed.success_streak = recomputed.success_streak.saturating_add(1);
                match recomputed.success_streak {
                    0 | 1 => 3_i64,
                    2 => 7_i64,
                    3 => 14_i64,
                    _ => 30_i64,
                }
            }
            _ => return Err("复习目标 v15 发现未知 active attempt 结果。".to_string()),
        };
        recomputed.next_review_at_unix_ms = created_at_unix_ms
            .checked_add(days.saturating_mul(REVIEW_DAY_UNIX_MS))
            .ok_or_else(|| "复习目标 v15 重算时间超出可保存范围。".to_string())?;
        recomputed.attempt_count += 1;
        recomputed.last_reviewed_at_unix_ms = Some(created_at_unix_ms);
        recomputed.last_outcome = Some(outcome);
        recomputed.last_used_hint = Some(i64::from(used_hint));
        recomputed.last_attempt_id = Some(attempt_id);
    }
    Ok(recomputed)
}

pub(crate) fn get_learning_record_from_connection(
    connection: &Connection,
    id: i64,
) -> Result<Option<LearningRecord>, String> {
    let mut statement = connection
        .prepare(&select_learning_record_sql("WHERE lr.id = ?1"))
        .map_err(|error| format!("学习记录读取语句无法准备：{error}"))?;
    let stored = statement
        .query_row([id], read_stored_learning_record)
        .optional()
        .map_err(|error| format!("学习记录读取失败：{error}"))?;

    if let Some(stored) = stored {
        return decode_learning_record(stored).map(Some);
    }

    let mut compatibility_statement = connection
        .prepare(
            "SELECT lr.id, lto.learning_target_id, lr.query_text, lr.normalized_text,
                    lr.query_type, lr.source_type, lr.source_app, lr.context_text,
                    lr.explanation_card_json, lr.schema_version, lr.created_at_unix_ms,
                    lr.difficulty, target.display_target_text, 'zh_to_en'
             FROM learning_records lr
             JOIN learning_target_occurrences lto ON lto.learning_record_id = lr.id
             JOIN learning_targets target ON target.id = lto.learning_target_id
             WHERE lr.id = ?1 AND target.target_kind = 'legacy_compat'",
        )
        .map_err(|error| format!("历史兼容学习记录读取语句无法准备：{error}"))?;
    let compatibility = compatibility_statement
        .query_row([id], read_stored_learning_record)
        .optional()
        .map_err(|error| format!("历史兼容学习记录读取失败：{error}"))?;
    compatibility.map(decode_learning_record).transpose()
}

pub fn save_for_app(
    app: &AppHandle,
    input: &CaptureInput,
    card: &ExplanationCard,
) -> Result<LearningRecord, String> {
    LearningRecordStore::open(&database_path_for_app(app)?)?.save(input, card)
}

#[tauri::command]
pub fn list_learning_records(
    app: AppHandle,
    page: Option<u32>,
    page_size: Option<u32>,
    query_type: Option<QueryType>,
) -> Result<LearningRecordPage, String> {
    LearningRecordStore::open(&database_path_for_app(&app)?)?
        .list(page, page_size, None, query_type)
}

#[tauri::command]
pub fn search_learning_records(
    app: AppHandle,
    keyword: String,
    page: Option<u32>,
    page_size: Option<u32>,
    query_type: Option<QueryType>,
) -> Result<LearningRecordPage, String> {
    LearningRecordStore::open(&database_path_for_app(&app)?)?.list(
        page,
        page_size,
        Some(&keyword),
        query_type,
    )
}

#[tauri::command]
pub fn get_learning_record(app: AppHandle, id: i64) -> Result<Option<LearningRecord>, String> {
    LearningRecordStore::open(&database_path_for_app(&app)?)?.get(id)
}

#[tauri::command]
pub fn list_learning_targets(
    app: AppHandle,
    page: Option<u32>,
    page_size: Option<u32>,
    query_type: Option<QueryType>,
) -> Result<LearningTargetPage, String> {
    LearningRecordStore::open(&database_path_for_app(&app)?)?
        .list_targets(page, page_size, None, query_type)
}

#[tauri::command]
pub fn search_learning_targets(
    app: AppHandle,
    keyword: String,
    page: Option<u32>,
    page_size: Option<u32>,
    query_type: Option<QueryType>,
) -> Result<LearningTargetPage, String> {
    LearningRecordStore::open(&database_path_for_app(&app)?)?.list_targets(
        page,
        page_size,
        Some(&keyword),
        query_type,
    )
}

#[tauri::command]
pub fn get_learning_target(
    app: AppHandle,
    id: i64,
) -> Result<Option<LearningTargetDetail>, String> {
    LearningRecordStore::open(&database_path_for_app(&app)?)?.get_target(id)
}

#[tauri::command]
pub fn delete_learning_record(app: AppHandle, id: i64) -> Result<bool, String> {
    LearningRecordStore::open(&database_path_for_app(&app)?)?.delete(id)
}

#[tauri::command]
pub fn get_today_learning_summary(
    app: AppHandle,
    start_unix_ms: i64,
    end_unix_ms: i64,
) -> Result<TodayLearningSummary, String> {
    LearningRecordStore::open(&database_path_for_app(&app)?)?
        .summarize_range(start_unix_ms, end_unix_ms)
}

pub(crate) fn database_path_for_app(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定 ReadRay 应用数据目录：{error}"))?;

    Ok(app_data_dir.join(DATABASE_FILE_NAME))
}

fn migrate(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at_unix_ms INTEGER NOT NULL
            );",
        )
        .map_err(|error| format!("学习记录迁移表初始化失败：{error}"))?;

    let transaction = connection
        .transaction()
        .map_err(|error| format!("学习记录迁移事务无法开始：{error}"))?;

    for &(version, sql) in MIGRATIONS {
        let applied: Option<i64> = transaction
            .query_row(
                "SELECT version FROM schema_migrations WHERE version = ?1",
                [version],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("学习记录迁移状态读取失败：{error}"))?;
        if applied.is_none() {
            transaction
                .execute_batch(sql)
                .map_err(|error| format!("学习记录迁移 v{version} 执行失败：{error}"))?;
            if version == 15 {
                repair_review_targets_v15(&transaction, unix_time_ms()?)?;
            }
            if version == 16 {
                repair_review_quality_feedback_v16(&transaction)?;
            }
            if version == 17 {
                backfill_learning_record_targets_v17(&transaction)?;
            }
            if version == 19 {
                backfill_learning_targets_v19(&transaction)?;
                backfill_legacy_compatibility_targets_v19(&transaction)?;
                rebuild_review_tables_v19(&transaction)?;
                rebuild_learning_target_review_states_v19(&transaction)?;
                audit_learning_target_aggregation_v19(&transaction)?;
            }
            transaction
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms) VALUES (?1, ?2)",
                    params![version, unix_time_ms()?],
                )
                .map_err(|error| format!("学习记录迁移 v{version} 标记失败：{error}"))?;
        }
    }

    transaction
        .commit()
        .map_err(|error| format!("学习记录迁移事务无法提交：{error}"))?;

    Ok(())
}

fn build_filter(
    keyword: Option<&str>,
    query_type: Option<QueryType>,
) -> Result<(String, Vec<Value>), String> {
    let mut clauses = Vec::new();
    let mut values = Vec::new();

    if let Some(keyword) = keyword {
        let normalized_keyword = normalize_query_text(keyword);
        if normalized_keyword.is_empty() {
            return Err("学习记录搜索关键词不能为空。".to_string());
        }
        clauses.push(
            "(instr(lrt.normalized_target_text, ?) > 0
               OR instr(lr.normalized_text, ?) > 0
               OR instr(lower(COALESCE(lr.context_text, '')), ?) > 0)"
                .to_string(),
        );
        values.push(Value::Text(normalized_keyword.clone()));
        values.push(Value::Text(normalized_keyword.clone()));
        values.push(Value::Text(normalized_keyword));
    }
    if let Some(query_type) = query_type {
        clauses.push("lr.query_type = ?".to_string());
        values.push(Value::Text(query_type_to_storage(query_type).to_string()));
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };

    Ok((where_clause, values))
}

fn build_target_filter(
    keyword: Option<&str>,
    query_type: Option<QueryType>,
) -> Result<(String, Vec<Value>), String> {
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    if let Some(keyword) = keyword {
        let normalized_keyword = canonicalize_learning_target_text(keyword);
        if normalized_keyword.is_empty() {
            return Err("学习目标搜索关键词不能为空。".to_string());
        }
        clauses.push(
            "EXISTS (
               SELECT 1
               FROM learning_target_occurrences search_occurrence
               JOIN learning_records search_record
                 ON search_record.id = search_occurrence.learning_record_id
               JOIN learning_record_targets search_projection
                 ON search_projection.learning_record_id = search_record.id
               WHERE search_occurrence.learning_target_id = lt.id
                 AND (
                   instr(search_projection.normalized_target_text, ?) > 0
                   OR instr(search_record.normalized_text, ?) > 0
                   OR instr(lower(COALESCE(search_record.context_text, '')), ?) > 0
                   OR instr(lower(search_record.explanation_card_json), ?) > 0
                 )
             )"
            .to_string(),
        );
        for _ in 0..4 {
            values.push(Value::Text(normalized_keyword.clone()));
        }
    }
    if let Some(query_type) = query_type {
        clauses.push("lt.query_type = ?".to_string());
        values.push(Value::Text(query_type_to_storage(query_type).to_string()));
    }
    let suffix = if clauses.is_empty() {
        String::new()
    } else {
        format!(" AND {}", clauses.join(" AND "))
    };
    Ok((suffix, values))
}

fn build_target_order_clause(
    keyword: Option<&str>,
    values: &mut Vec<Value>,
) -> Result<String, String> {
    let Some(keyword) = keyword else {
        return Ok("MAX(lr.created_at_unix_ms) DESC, lt.id DESC".to_string());
    };
    let normalized_keyword = canonicalize_learning_target_text(keyword);
    if normalized_keyword.is_empty() {
        return Err("学习目标搜索关键词不能为空。".to_string());
    }
    for _ in 0..4 {
        values.push(Value::Text(normalized_keyword.clone()));
    }
    Ok("CASE
           WHEN lt.normalized_target_text = ? THEN 0
           WHEN instr(lt.normalized_target_text, ?) = 1 THEN 1
           WHEN instr(lt.normalized_target_text, ?) > 0 THEN 2
           WHEN EXISTS (
             SELECT 1
             FROM learning_target_occurrences rank_occurrence
             JOIN learning_records rank_record
               ON rank_record.id = rank_occurrence.learning_record_id
             WHERE rank_occurrence.learning_target_id = lt.id
               AND instr(rank_record.normalized_text, ?) > 0
           ) THEN 3
           ELSE 4
         END ASC,
         MAX(lr.created_at_unix_ms) DESC,
         lt.id DESC"
        .to_string())
}

type LearningTargetSummaryRow = (i64, String, i64, String, String, String, i64, i64, i64, i64);

fn load_learning_target_summary_row(
    connection: &Connection,
    id: i64,
) -> Result<Option<LearningTargetSummaryRow>, String> {
    connection
        .query_row(
            "SELECT lt.id, lt.stable_key, lt.canonicalization_version, lt.query_type,
                    lt.display_target_text, lt.normalized_target_text,
                    COUNT(lto.learning_record_id), MIN(lr.created_at_unix_ms),
                    MAX(lr.created_at_unix_ms), lt.representative_learning_record_id
             FROM learning_targets lt
             JOIN learning_target_occurrences lto ON lto.learning_target_id = lt.id
             JOIN learning_records lr ON lr.id = lto.learning_record_id
             WHERE lt.id = ?1 AND lt.target_kind = 'learnable'
             GROUP BY lt.id",
            [id],
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
                    row.get(9)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("学习目标摘要读取失败：{error}"))
}

fn decode_learning_target_summary(
    connection: &Connection,
    row: LearningTargetSummaryRow,
) -> Result<LearningTargetSummary, String> {
    let representative_record = get_learning_record_from_connection(connection, row.9)?
        .ok_or_else(|| format!("学习目标 {} 的代表学习记录不存在。", row.0))?;
    if representative_record.learning_target_id != row.0 {
        return Err(format!("学习目标 {} 的代表记录身份不一致。", row.0));
    }
    Ok(LearningTargetSummary {
        id: row.0,
        stable_key: row.1,
        canonicalization_version: row.2,
        query_type: query_type_from_storage(&row.3)?,
        learning_target_text: row.4,
        normalized_target_text: row.5,
        query_count: u64::try_from(row.6).map_err(|_| "学习目标查询次数无效。".to_string())?,
        first_seen_at_unix_ms: row.7,
        last_seen_at_unix_ms: row.8,
        representative_record,
    })
}

fn validate_pagination(page: Option<u32>, page_size: Option<u32>) -> Result<(u32, u32), String> {
    let page = page.unwrap_or(DEFAULT_PAGE);
    let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if page == 0 {
        return Err("学习记录页码必须从 1 开始。".to_string());
    }
    if page_size == 0 || page_size > MAX_PAGE_SIZE {
        return Err(format!(
            "学习记录每页数量必须在 1 到 {MAX_PAGE_SIZE} 之间。"
        ));
    }

    Ok((page, page_size))
}

fn select_learning_record_sql(where_clause: &str) -> String {
    format!(
        "SELECT lr.id, lto.learning_target_id, lr.query_text, lr.normalized_text, lr.query_type, lr.source_type,
         lr.source_app, lr.context_text, lr.explanation_card_json, lr.schema_version,
         lr.created_at_unix_ms, lr.difficulty, lrt.learning_target_text, lrt.query_direction
         FROM learning_records lr
         JOIN learning_record_targets lrt ON lrt.learning_record_id = lr.id
         JOIN learning_target_occurrences lto ON lto.learning_record_id = lr.id {where_clause}"
    )
}

fn read_stored_learning_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredLearningRecord> {
    Ok(StoredLearningRecord {
        id: row.get(0)?,
        learning_target_id: row.get(1)?,
        query_text: row.get(2)?,
        normalized_text: row.get(3)?,
        query_type: row.get(4)?,
        source_type: row.get(5)?,
        source_app: row.get(6)?,
        context_text: row.get(7)?,
        explanation_card_json: row.get(8)?,
        schema_version: row.get(9)?,
        created_at_unix_ms: row.get(10)?,
        difficulty: row.get(11)?,
        learning_target_text: row.get(12)?,
        query_direction: row.get(13)?,
    })
}

fn decode_learning_record(stored: StoredLearningRecord) -> Result<LearningRecord, String> {
    let query_type = query_type_from_storage(&stored.query_type)?;
    let source_type = source_type_from_storage(&stored.source_type)?;
    let mut explanation_card: ExplanationCard = serde_json::from_str(&stored.explanation_card_json)
        .map_err(|error| {
            format!(
                "学习记录 {} 的 ExplanationCard JSON 无法解析：{error}",
                stored.id
            )
        })?;

    explanation_card.set_learning_target_text(stored.learning_target_text.clone());
    if explanation_card.query_type() != query_type {
        return Err(format!(
            "学习记录 {} 的 queryType 与 ExplanationCard JSON 不一致。",
            stored.id
        ));
    }

    Ok(LearningRecord {
        id: stored.id,
        learning_target_id: stored.learning_target_id,
        query_text: stored.query_text,
        learning_target_text: stored.learning_target_text,
        query_direction: query_direction_from_storage(&stored.query_direction)?,
        normalized_text: stored.normalized_text,
        query_type,
        source_type,
        source_app: stored.source_app,
        context_text: stored.context_text,
        explanation_card,
        schema_version: stored.schema_version,
        created_at_unix_ms: stored.created_at_unix_ms,
        difficulty: stored.difficulty,
    })
}

fn normalize_query_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn canonicalize_learning_target_text(value: &str) -> String {
    normalize_query_text(value)
}

fn stable_learning_target_key(query_type: QueryType, normalized_target_text: &str) -> String {
    format!(
        "v{}:{}:{}",
        LEARNING_TARGET_CANONICALIZATION_VERSION,
        query_type_to_storage(query_type),
        normalized_target_text
    )
}

fn bind_learning_record_to_stable_target(
    transaction: &Transaction<'_>,
    learning_record_id: i64,
    query_type: QueryType,
    learning_target_text: &str,
    created_at_unix_ms: i64,
) -> Result<i64, String> {
    let display_target_text = learning_target_text.trim();
    let normalized_target_text = canonicalize_learning_target_text(display_target_text);
    if normalized_target_text.is_empty() {
        return Err("规范英文学习目标不能为空。".to_string());
    }
    let query_type_storage = query_type_to_storage(query_type);
    let stable_key = stable_learning_target_key(query_type, &normalized_target_text);
    transaction
        .execute(
            "INSERT OR IGNORE INTO learning_targets (
               stable_key, canonicalization_version, query_type, display_target_text,
               normalized_target_text, representative_learning_record_id,
               created_at_unix_ms, updated_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                stable_key,
                LEARNING_TARGET_CANONICALIZATION_VERSION,
                query_type_storage,
                display_target_text,
                normalized_target_text,
                learning_record_id,
                created_at_unix_ms,
            ],
        )
        .map_err(|error| format!("学习目标创建失败：{error}"))?;
    let learning_target_id: i64 = transaction
        .query_row(
            "SELECT id FROM learning_targets
             WHERE canonicalization_version = ?1 AND query_type = ?2
               AND normalized_target_text = ?3",
            params![
                LEARNING_TARGET_CANONICALIZATION_VERSION,
                query_type_storage,
                normalized_target_text,
            ],
            |row| row.get(0),
        )
        .map_err(|error| format!("学习目标身份读取失败：{error}"))?;
    transaction
        .execute(
            "UPDATE learning_targets
             SET display_target_text = ?1,
                 representative_learning_record_id = ?2,
                 updated_at_unix_ms = MAX(updated_at_unix_ms, ?3)
             WHERE id = ?4
               AND (
                 representative_learning_record_id IS NULL
                 OR EXISTS (
                   SELECT 1
                   FROM learning_records current_record
                   WHERE current_record.id = learning_targets.representative_learning_record_id
                     AND (
                       current_record.created_at_unix_ms < ?3
                       OR (current_record.created_at_unix_ms = ?3 AND current_record.id < ?2)
                     )
                 )
               )",
            params![
                display_target_text,
                learning_record_id,
                created_at_unix_ms,
                learning_target_id,
            ],
        )
        .map_err(|error| format!("学习目标代表记录更新失败：{error}"))?;
    transaction
        .execute(
            "INSERT INTO learning_target_occurrences (
               learning_record_id, learning_target_id, canonicalization_version,
               binding_revision, bound_at_unix_ms, updated_at_unix_ms
             ) VALUES (?1, ?2, ?3, 0, ?4, ?4)",
            params![
                learning_record_id,
                learning_target_id,
                LEARNING_TARGET_CANONICALIZATION_VERSION,
                created_at_unix_ms,
            ],
        )
        .map_err(|error| format!("学习目标 occurrence 写入失败：{error}"))?;
    Ok(learning_target_id)
}

fn query_direction_to_storage(direction: QueryDirection) -> &'static str {
    match direction {
        QueryDirection::EnToZh => "en_to_zh",
        QueryDirection::ZhToEn => "zh_to_en",
    }
}

fn query_direction_from_storage(value: &str) -> Result<QueryDirection, String> {
    match value {
        "en_to_zh" => Ok(QueryDirection::EnToZh),
        "zh_to_en" => Ok(QueryDirection::ZhToEn),
        _ => Err(format!("未知的学习目标查询方向：{value}")),
    }
}

fn backfill_learning_record_targets_v17(transaction: &Transaction<'_>) -> Result<(), String> {
    let records = {
        let mut statement = transaction
            .prepare("SELECT id, query_text, created_at_unix_ms FROM learning_records ORDER BY id")
            .map_err(|error| format!("v17 历史学习目标读取语句无法准备：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|error| format!("v17 历史学习目标读取失败：{error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("v17 历史学习目标行读取失败：{error}"))?
    };

    for (learning_record_id, query_text, created_at_unix_ms) in records {
        if determine_query_direction(&query_text).ok() != Some(QueryDirection::EnToZh) {
            continue;
        }
        let learning_target_text = normalize_english_learning_target(&query_text)?;
        transaction
            .execute(
                "INSERT INTO learning_record_targets (
                   learning_record_id, query_direction, learning_target_text,
                   normalized_target_text, created_at_unix_ms
                 ) VALUES (?1, 'en_to_zh', ?2, ?3, ?4)",
                params![
                    learning_record_id,
                    learning_target_text,
                    normalize_query_text(&learning_target_text),
                    created_at_unix_ms,
                ],
            )
            .map_err(|error| format!("v17 历史英文学习目标回填失败：{error}"))?;
    }
    Ok(())
}

fn backfill_learning_targets_v19(transaction: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = transaction
            .prepare(
                "SELECT lr.id, lr.query_type, lrt.learning_target_text, lr.created_at_unix_ms
                 FROM learning_records lr
                 JOIN learning_record_targets lrt ON lrt.learning_record_id = lr.id
                 ORDER BY lr.created_at_unix_ms ASC, lr.id ASC",
            )
            .map_err(|error| format!("v19 学习目标回填语句无法准备：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|error| format!("v19 学习目标回填读取失败：{error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("v19 学习目标回填行读取失败：{error}"))?
    };
    for (learning_record_id, query_type, learning_target_text, created_at_unix_ms) in rows {
        bind_learning_record_to_stable_target(
            transaction,
            learning_record_id,
            query_type_from_storage(&query_type)?,
            &learning_target_text,
            created_at_unix_ms,
        )?;
    }
    Ok(())
}

fn backfill_legacy_compatibility_targets_v19(transaction: &Transaction<'_>) -> Result<(), String> {
    let records = {
        let mut statement = transaction
            .prepare(
                "SELECT record.id, record.query_type, record.query_text,
                        record.created_at_unix_ms
                 FROM learning_records record
                 LEFT JOIN learning_target_occurrences occurrence
                   ON occurrence.learning_record_id = record.id
                 WHERE occurrence.learning_record_id IS NULL
                 ORDER BY record.id",
            )
            .map_err(|error| format!("v19 历史兼容记录回填语句无法准备：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|error| format!("v19 历史兼容记录回填读取失败：{error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("v19 历史兼容记录回填行读取失败：{error}"))?
    };

    for (learning_record_id, query_type, query_text, created_at_unix_ms) in records {
        let stable_key = format!("legacy-compat:record:{learning_record_id}");
        transaction
            .execute(
                "INSERT INTO learning_targets (
                   stable_key, target_kind, canonicalization_version, query_type,
                   display_target_text, normalized_target_text,
                   representative_learning_record_id, created_at_unix_ms,
                   updated_at_unix_ms
                 ) VALUES (?1, 'legacy_compat', 0, ?2, ?3, NULL, ?4, ?5, ?5)",
                params![
                    stable_key,
                    query_type,
                    query_text,
                    learning_record_id,
                    created_at_unix_ms
                ],
            )
            .map_err(|error| format!("v19 历史兼容目标创建失败：{error}"))?;
        let learning_target_id = transaction.last_insert_rowid();
        transaction
            .execute(
                "INSERT INTO learning_target_occurrences (
                   learning_record_id, learning_target_id, canonicalization_version,
                   binding_revision, bound_at_unix_ms, updated_at_unix_ms
                 ) VALUES (?1, ?2, 0, 0, ?3, ?3)",
                params![learning_record_id, learning_target_id, created_at_unix_ms],
            )
            .map_err(|error| format!("v19 历史兼容 occurrence 创建失败：{error}"))?;
    }
    Ok(())
}

fn rebuild_review_tables_v19(transaction: &Transaction<'_>) -> Result<(), String> {
    let rebuilt_tables = [
        "review_generated_cards",
        "review_feed_items",
        "review_feed_attempts",
        "review_quality_feedback",
        "review_quality_mutations",
        "review_card_generation_failures",
    ];
    let original_counts = rebuilt_tables
        .iter()
        .map(|table| {
            transaction
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(|error| format!("v19 {table} 重建前数量读取失败：{error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    transaction
        .execute_batch(
            r#"
CREATE TABLE review_generated_cards_v19 (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  learning_target_id INTEGER NOT NULL REFERENCES learning_targets(id) ON DELETE RESTRICT,
  variant_index INTEGER NOT NULL CHECK (variant_index >= 0),
  generation_request_key TEXT NOT NULL UNIQUE,
  content_json TEXT NOT NULL,
  model TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  expires_at_unix_ms INTEGER NOT NULL,
  last_used_at_unix_ms INTEGER NOT NULL,
  use_count INTEGER NOT NULL CHECK (use_count >= 0),
  UNIQUE(learning_record_id, variant_index)
);

INSERT INTO review_generated_cards_v19 (
  id, learning_record_id, learning_target_id, variant_index,
  generation_request_key, content_json, model, created_at_unix_ms,
  expires_at_unix_ms, last_used_at_unix_ms, use_count
)
SELECT card.id, card.learning_record_id, occurrence.learning_target_id,
       card.variant_index, card.generation_request_key, card.content_json,
       card.model, card.created_at_unix_ms, card.expires_at_unix_ms,
       card.last_used_at_unix_ms, card.use_count
FROM review_generated_cards card
JOIN learning_target_occurrences occurrence
  ON occurrence.learning_record_id = card.learning_record_id;

CREATE TABLE review_feed_items_v19 (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  day_start_unix_ms INTEGER NOT NULL,
  day_end_unix_ms INTEGER NOT NULL CHECK (day_end_unix_ms > day_start_unix_ms),
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  learning_target_id INTEGER NOT NULL REFERENCES learning_targets(id) ON DELETE RESTRICT,
  cycle_index INTEGER NOT NULL CHECK (cycle_index >= 0),
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  reason_code TEXT NOT NULL CHECK (
    reason_code IN ('scheduled_today', 'new_record', 'continued_practice')
  ),
  generated_card_id INTEGER REFERENCES review_generated_cards_v19(id) ON DELETE SET NULL,
  target_slot_active INTEGER NOT NULL CHECK (target_slot_active IN (0, 1)),
  created_at_unix_ms INTEGER NOT NULL,
  UNIQUE(day_start_unix_ms, cycle_index, learning_record_id),
  UNIQUE(day_start_unix_ms, ordinal)
);

INSERT INTO review_feed_items_v19 (
  id, day_start_unix_ms, day_end_unix_ms, learning_record_id,
  learning_target_id, cycle_index, ordinal, reason_code, generated_card_id,
  target_slot_active, created_at_unix_ms
)
SELECT item.id, item.day_start_unix_ms, item.day_end_unix_ms,
       item.learning_record_id, occurrence.learning_target_id, item.cycle_index,
       item.ordinal, item.reason_code, item.generated_card_id,
       CASE WHEN target.target_kind = 'legacy_compat' THEN 0
       WHEN item.id = (
         SELECT candidate.id
         FROM review_feed_items candidate
         JOIN learning_target_occurrences candidate_occurrence
           ON candidate_occurrence.learning_record_id = candidate.learning_record_id
         LEFT JOIN review_feed_attempts active_attempt
           ON active_attempt.feed_item_id = candidate.id
          AND active_attempt.undone_at_unix_ms IS NULL
         WHERE candidate.day_start_unix_ms = item.day_start_unix_ms
           AND candidate.cycle_index = item.cycle_index
           AND candidate_occurrence.learning_target_id = occurrence.learning_target_id
         ORDER BY CASE WHEN active_attempt.id IS NULL THEN 1 ELSE 0 END ASC,
                  active_attempt.created_at_unix_ms DESC,
                  active_attempt.id DESC,
                  candidate.ordinal ASC,
                  candidate.id ASC
         LIMIT 1
       ) THEN 1 ELSE 0 END,
       item.created_at_unix_ms
FROM review_feed_items item
JOIN learning_target_occurrences occurrence
  ON occurrence.learning_record_id = item.learning_record_id
JOIN learning_targets target ON target.id = occurrence.learning_target_id;

CREATE TABLE review_feed_attempts_v19 (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  feed_item_id INTEGER NOT NULL REFERENCES review_feed_items_v19(id) ON DELETE CASCADE,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  learning_target_id INTEGER NOT NULL REFERENCES learning_targets(id) ON DELETE RESTRICT,
  request_key TEXT NOT NULL UNIQUE,
  expected_revision INTEGER NOT NULL CHECK (expected_revision >= 0),
  target_revision INTEGER NOT NULL CHECK (target_revision > expected_revision),
  outcome TEXT NOT NULL CHECK (outcome IN ('remembered', 'forgotten')),
  used_hint INTEGER NOT NULL CHECK (used_hint IN (0, 1)),
  next_review_at_unix_ms INTEGER NOT NULL,
  previous_next_review_at_unix_ms INTEGER NOT NULL,
  previous_attempt_count INTEGER NOT NULL CHECK (previous_attempt_count >= 0),
  previous_remembered_count INTEGER NOT NULL CHECK (previous_remembered_count >= 0),
  previous_forgotten_count INTEGER NOT NULL CHECK (previous_forgotten_count >= 0),
  previous_success_streak INTEGER NOT NULL CHECK (previous_success_streak >= 0),
  previous_last_reviewed_at_unix_ms INTEGER,
  previous_last_outcome TEXT CHECK (previous_last_outcome IN ('remembered', 'forgotten')),
  previous_last_used_hint INTEGER CHECK (previous_last_used_hint IN (0, 1)),
  previous_last_attempt_id INTEGER,
  created_at_unix_ms INTEGER NOT NULL,
  undone_at_unix_ms INTEGER,
  undo_request_key TEXT UNIQUE,
  undo_expected_revision INTEGER,
  undo_target_revision INTEGER
);

INSERT INTO review_feed_attempts_v19 (
  id, feed_item_id, learning_record_id, learning_target_id, request_key,
  expected_revision, target_revision, outcome, used_hint, next_review_at_unix_ms,
  previous_next_review_at_unix_ms, previous_attempt_count,
  previous_remembered_count, previous_forgotten_count, previous_success_streak,
  previous_last_reviewed_at_unix_ms, previous_last_outcome,
  previous_last_used_hint, previous_last_attempt_id, created_at_unix_ms,
  undone_at_unix_ms, undo_request_key, undo_expected_revision, undo_target_revision
)
SELECT attempt.id, attempt.feed_item_id, attempt.learning_record_id,
       item.learning_target_id, attempt.request_key, attempt.expected_revision,
       attempt.target_revision, attempt.outcome, attempt.used_hint,
       attempt.next_review_at_unix_ms, attempt.previous_next_review_at_unix_ms,
       attempt.previous_attempt_count, attempt.previous_remembered_count,
       attempt.previous_forgotten_count, attempt.previous_success_streak,
       attempt.previous_last_reviewed_at_unix_ms, attempt.previous_last_outcome,
       attempt.previous_last_used_hint, attempt.previous_last_attempt_id,
       attempt.created_at_unix_ms, attempt.undone_at_unix_ms,
       attempt.undo_request_key, attempt.undo_expected_revision,
       attempt.undo_target_revision
FROM review_feed_attempts attempt
JOIN review_feed_items_v19 item ON item.id = attempt.feed_item_id
WHERE item.learning_record_id = attempt.learning_record_id;

CREATE TABLE review_quality_feedback_v19 (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  feed_item_id INTEGER NOT NULL REFERENCES review_feed_items_v19(id) ON DELETE CASCADE,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  generated_card_id INTEGER,
  card_context_key TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  active INTEGER NOT NULL CHECK (active IN (0, 1)),
  polarity TEXT NOT NULL CHECK (polarity IN ('up', 'down')),
  reason_codes_json TEXT NOT NULL,
  detail TEXT,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL,
  UNIQUE(feed_item_id, card_context_key)
);

INSERT INTO review_quality_feedback_v19
SELECT * FROM review_quality_feedback;

CREATE TABLE review_quality_mutations_v19 (
  request_key TEXT PRIMARY KEY,
  feed_item_id INTEGER NOT NULL REFERENCES review_feed_items_v19(id) ON DELETE CASCADE,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  operation TEXT NOT NULL CHECK (operation IN ('save', 'undo')),
  input_json TEXT NOT NULL,
  result_json TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL
);

INSERT INTO review_quality_mutations_v19
SELECT * FROM review_quality_mutations;

CREATE TABLE review_card_generation_failures_v19 (
  request_key TEXT PRIMARY KEY,
  feed_item_id INTEGER NOT NULL REFERENCES review_feed_items_v19(id) ON DELETE CASCADE,
  learning_record_id INTEGER NOT NULL REFERENCES learning_records(id) ON DELETE CASCADE,
  failure_count INTEGER NOT NULL CHECK (failure_count > 0),
  retry_after_unix_ms INTEGER NOT NULL,
  last_error TEXT NOT NULL,
  created_at_unix_ms INTEGER NOT NULL,
  updated_at_unix_ms INTEGER NOT NULL
);

INSERT INTO review_card_generation_failures_v19
SELECT * FROM review_card_generation_failures;

DROP TABLE review_quality_mutations;
DROP TABLE review_quality_feedback;
DROP TABLE review_card_generation_failures;
DROP TABLE review_feed_attempts;
DROP TABLE review_feed_items;
DROP TABLE review_generated_cards;

ALTER TABLE review_generated_cards_v19 RENAME TO review_generated_cards;
ALTER TABLE review_feed_items_v19 RENAME TO review_feed_items;
ALTER TABLE review_feed_attempts_v19 RENAME TO review_feed_attempts;
ALTER TABLE review_quality_feedback_v19 RENAME TO review_quality_feedback;
ALTER TABLE review_quality_mutations_v19 RENAME TO review_quality_mutations;
ALTER TABLE review_card_generation_failures_v19 RENAME TO review_card_generation_failures;

CREATE INDEX idx_review_generated_cards_learning_record
  ON review_generated_cards(learning_record_id, variant_index);
CREATE INDEX idx_review_generated_cards_pool
  ON review_generated_cards(expires_at_unix_ms, last_used_at_unix_ms, id);
CREATE INDEX idx_review_generated_cards_target
  ON review_generated_cards(learning_target_id, last_used_at_unix_ms, id);
CREATE INDEX idx_review_feed_items_day
  ON review_feed_items(day_start_unix_ms, ordinal);
CREATE UNIQUE INDEX idx_review_feed_items_active_target
  ON review_feed_items(day_start_unix_ms, cycle_index, learning_target_id)
  WHERE target_slot_active = 1;
CREATE INDEX idx_review_feed_items_target_cycle
  ON review_feed_items(day_start_unix_ms, cycle_index, learning_target_id, ordinal);
CREATE UNIQUE INDEX idx_review_feed_attempts_active_item
  ON review_feed_attempts(feed_item_id)
  WHERE undone_at_unix_ms IS NULL;
CREATE INDEX idx_review_feed_attempts_learning_record
  ON review_feed_attempts(learning_record_id, created_at_unix_ms DESC, id DESC);
CREATE INDEX idx_review_feed_attempts_target_time
  ON review_feed_attempts(learning_target_id, created_at_unix_ms, id);
CREATE INDEX idx_review_quality_feedback_record
  ON review_quality_feedback(learning_record_id, updated_at_unix_ms DESC, id DESC);
CREATE INDEX idx_review_quality_mutations_item
  ON review_quality_mutations(feed_item_id, created_at_unix_ms DESC);
CREATE INDEX idx_review_card_generation_failures_retry
  ON review_card_generation_failures(retry_after_unix_ms, feed_item_id);
"#,
        )
        .map_err(|error| format!("v19 复习链路强身份重建失败：{error}"))?;
    for (table, original_count) in rebuilt_tables.iter().zip(original_counts) {
        let rebuilt_count: i64 = transaction
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|error| format!("v19 {table} 重建后数量读取失败：{error}"))?;
        if rebuilt_count != original_count {
            return Err(format!(
                "v19 {table} 重建前后数量不一致：{original_count} -> {rebuilt_count}。"
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReplayedLearningTargetReviewState {
    learning_target_id: i64,
    revision: i64,
    next_review_at_unix_ms: i64,
    attempt_count: i64,
    remembered_count: i64,
    forgotten_count: i64,
    success_streak: i64,
    last_reviewed_at_unix_ms: Option<i64>,
    last_outcome: Option<String>,
    last_used_hint: Option<i64>,
    last_attempt_id: Option<i64>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

fn replay_learning_target_review_states_v19(
    transaction: &Transaction<'_>,
) -> Result<HashMap<i64, ReplayedLearningTargetReviewState>, String> {
    let mut states = HashMap::new();
    {
        let mut statement = transaction
            .prepare(
                "SELECT lt.id,
                        CASE WHEN EXISTS (
                          SELECT 1 FROM review_targets old_state
                          JOIN learning_target_occurrences old_occurrence
                            ON old_occurrence.learning_record_id = old_state.learning_record_id
                          WHERE old_occurrence.learning_target_id = lt.id
                        ) THEN COALESCE((
                          SELECT MAX(old_state.revision) + 1
                          FROM review_targets old_state
                          JOIN learning_target_occurrences old_occurrence
                            ON old_occurrence.learning_record_id = old_state.learning_record_id
                          WHERE old_occurrence.learning_target_id = lt.id
                        ), 1) ELSE 0 END,
                        MIN(record.created_at_unix_ms),
                        MAX(lt.updated_at_unix_ms)
                 FROM learning_targets lt
                 JOIN learning_target_occurrences occurrence ON occurrence.learning_target_id = lt.id
                 JOIN learning_records record ON record.id = occurrence.learning_record_id
                 GROUP BY lt.id
                 ORDER BY lt.id",
            )
            .map_err(|error| format!("v19 目标级复习状态初始化语句无法准备：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|error| format!("v19 目标级复习状态初始化读取失败：{error}"))?;
        for row in rows {
            let (learning_target_id, revision, first_seen, updated_at) =
                row.map_err(|error| format!("v19 目标级复习状态初始化行失败：{error}"))?;
            states.insert(
                learning_target_id,
                ReplayedLearningTargetReviewState {
                    learning_target_id,
                    revision,
                    next_review_at_unix_ms: first_seen,
                    attempt_count: 0,
                    remembered_count: 0,
                    forgotten_count: 0,
                    success_streak: 0,
                    last_reviewed_at_unix_ms: None,
                    last_outcome: None,
                    last_used_hint: None,
                    last_attempt_id: None,
                    created_at_unix_ms: first_seen,
                    updated_at_unix_ms: updated_at.max(first_seen),
                },
            );
        }
    }

    let attempts = {
        let mut statement = transaction
            .prepare(
                "SELECT id, learning_target_id, outcome, used_hint, created_at_unix_ms
                 FROM review_feed_attempts
                 WHERE undone_at_unix_ms IS NULL
                 ORDER BY created_at_unix_ms ASC, id ASC",
            )
            .map_err(|error| format!("v19 active attempt 重放语句无法准备：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|error| format!("v19 active attempt 重放读取失败：{error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("v19 active attempt 重放行读取失败：{error}"))?
    };
    for (attempt_id, learning_target_id, outcome, used_hint, created_at_unix_ms) in attempts {
        let state = states
            .get_mut(&learning_target_id)
            .ok_or_else(|| format!("v19 attempt {attempt_id} 缺少稳定学习目标。"))?;
        let (days, next_streak) = match (outcome.as_str(), used_hint != 0) {
            ("forgotten", _) => {
                state.forgotten_count += 1;
                (1_i64, 0_i64)
            }
            ("remembered", true) => {
                state.remembered_count += 1;
                (2_i64, state.success_streak)
            }
            ("remembered", false) => {
                state.remembered_count += 1;
                let streak = state.success_streak.saturating_add(1);
                let days = match streak {
                    0 | 1 => 3_i64,
                    2 => 7_i64,
                    3 => 14_i64,
                    _ => 30_i64,
                };
                (days, streak)
            }
            _ => return Err(format!("v19 attempt {attempt_id} 包含未知结果。")),
        };
        state.success_streak = next_streak;
        state.next_review_at_unix_ms = created_at_unix_ms
            .checked_add(days.saturating_mul(REVIEW_DAY_UNIX_MS))
            .ok_or_else(|| format!("v19 attempt {attempt_id} 调度时间超出范围。"))?;
        state.attempt_count += 1;
        state.last_reviewed_at_unix_ms = Some(created_at_unix_ms);
        state.last_outcome = Some(outcome);
        state.last_used_hint = Some(used_hint);
        state.last_attempt_id = Some(attempt_id);
        state.updated_at_unix_ms = state.updated_at_unix_ms.max(created_at_unix_ms);
    }
    Ok(states)
}

fn rebuild_learning_target_review_states_v19(transaction: &Transaction<'_>) -> Result<(), String> {
    let states = replay_learning_target_review_states_v19(transaction)?;
    for state in states.values() {
        transaction
            .execute(
                "INSERT INTO learning_target_review_states (
                   learning_target_id, revision, next_review_at_unix_ms, attempt_count,
                   remembered_count, forgotten_count, success_streak,
                   last_reviewed_at_unix_ms, last_outcome, last_used_hint,
                   last_attempt_id, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    state.learning_target_id,
                    state.revision,
                    state.next_review_at_unix_ms,
                    state.attempt_count,
                    state.remembered_count,
                    state.forgotten_count,
                    state.success_streak,
                    state.last_reviewed_at_unix_ms,
                    state.last_outcome,
                    state.last_used_hint,
                    state.last_attempt_id,
                    state.created_at_unix_ms,
                    state.updated_at_unix_ms,
                ],
            )
            .map_err(|error| format!("v19 目标级复习状态写入失败：{error}"))?;
    }
    Ok(())
}

fn audit_learning_target_aggregation_v19(transaction: &Transaction<'_>) -> Result<(), String> {
    let invalid_identity_count: i64 = transaction
        .query_row(
            "SELECT COUNT(*)
             FROM learning_target_occurrences occurrence
             JOIN learning_records record ON record.id = occurrence.learning_record_id
             LEFT JOIN learning_record_targets projection ON projection.learning_record_id = record.id
             JOIN learning_targets target ON target.id = occurrence.learning_target_id
             WHERE occurrence.canonicalization_version <> target.canonicalization_version
                OR target.query_type <> record.query_type
                OR (
                  target.target_kind = 'learnable' AND (
                    projection.learning_record_id IS NULL
                    OR target.canonicalization_version <> ?1
                    OR target.normalized_target_text <> projection.normalized_target_text
                  )
                )
                OR (
                  target.target_kind = 'legacy_compat' AND (
                    projection.learning_record_id IS NOT NULL
                    OR target.canonicalization_version <> 0
                    OR target.normalized_target_text IS NOT NULL
                    OR target.stable_key <> 'legacy-compat:record:' || record.id
                    OR (SELECT COUNT(*) FROM learning_target_occurrences same_target
                        WHERE same_target.learning_target_id = target.id) <> 1
                  )
                )",
            [LEARNING_TARGET_CANONICALIZATION_VERSION],
            |row| row.get(0),
        )
        .map_err(|error| format!("v19 occurrence 身份审计失败：{error}"))?;
    let record_count: i64 = transaction
        .query_row("SELECT COUNT(*) FROM learning_records", [], |row| {
            row.get(0)
        })
        .map_err(|error| format!("v19 学习记录数量审计失败：{error}"))?;
    let occurrence_count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM learning_target_occurrences",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("v19 occurrence 数量审计失败：{error}"))?;
    if invalid_identity_count != 0 || record_count != occurrence_count {
        return Err("v19 occurrence 与原始学习目标投影不一致。".to_string());
    }

    let invalid_representatives: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM learning_targets target
             WHERE target.representative_learning_record_id IS NULL
                OR target.representative_learning_record_id <> (
                  SELECT occurrence.learning_record_id
                  FROM learning_target_occurrences occurrence
                  JOIN learning_records record ON record.id = occurrence.learning_record_id
                  WHERE occurrence.learning_target_id = target.id
                  ORDER BY record.created_at_unix_ms DESC, record.id DESC
                  LIMIT 1
                )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("v19 代表记录审计失败：{error}"))?;
    if invalid_representatives != 0 {
        return Err("v19 学习目标代表记录不是确定性的最近 occurrence。".to_string());
    }

    for (table, join_condition) in [
        (
            "review_feed_items",
            "row.learning_target_id <> occurrence.learning_target_id",
        ),
        (
            "review_generated_cards",
            "row.learning_target_id <> occurrence.learning_target_id",
        ),
    ] {
        let sql = format!(
            "SELECT COUNT(*) FROM {table} row
             JOIN learning_target_occurrences occurrence
               ON occurrence.learning_record_id = row.learning_record_id
             WHERE row.learning_target_id IS NULL OR {join_condition}"
        );
        let invalid: i64 = transaction
            .query_row(&sql, [], |row| row.get(0))
            .map_err(|error| format!("v19 {table} target 身份审计失败：{error}"))?;
        if invalid != 0 {
            return Err(format!("v19 {table} 存在不一致的 target 身份。"));
        }
    }
    let invalid_attempts: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM review_feed_attempts attempt
             JOIN review_feed_items item ON item.id = attempt.feed_item_id
             WHERE attempt.learning_target_id IS NULL
                OR attempt.learning_target_id <> item.learning_target_id
                OR attempt.learning_record_id <> item.learning_record_id",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("v19 attempt target 身份审计失败：{error}"))?;
    if invalid_attempts != 0 {
        return Err("v19 attempt 与 Feed target/occurrence 身份不一致。".to_string());
    }
    let invalid_active_slots: i64 = transaction
        .query_row(
            "SELECT COUNT(*)
             FROM review_feed_items item
             JOIN learning_targets target ON target.id = item.learning_target_id
             WHERE item.target_slot_active <> CASE
             WHEN target.target_kind = 'legacy_compat' THEN 0
             WHEN item.id = (
               SELECT candidate.id
               FROM review_feed_items candidate
               LEFT JOIN review_feed_attempts active_attempt
                 ON active_attempt.feed_item_id = candidate.id
                AND active_attempt.undone_at_unix_ms IS NULL
               WHERE candidate.day_start_unix_ms = item.day_start_unix_ms
                 AND candidate.cycle_index = item.cycle_index
                 AND candidate.learning_target_id = item.learning_target_id
               ORDER BY CASE WHEN active_attempt.id IS NULL THEN 1 ELSE 0 END ASC,
                        active_attempt.created_at_unix_ms DESC,
                        active_attempt.id DESC,
                        candidate.ordinal ASC,
                        candidate.id ASC
               LIMIT 1
             ) THEN 1 ELSE 0 END",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("v19 Feed active target 槽位审计失败：{error}"))?;
    if invalid_active_slots != 0 {
        return Err("v19 Feed active target 槽位未保留目标的权威 active attempt。".to_string());
    }
    for table in [
        "review_quality_feedback",
        "review_quality_mutations",
        "review_card_generation_failures",
    ] {
        let invalid: i64 = transaction
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM {table} row
                     JOIN review_feed_items item ON item.id = row.feed_item_id
                     WHERE row.learning_record_id <> item.learning_record_id"
                ),
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("v19 {table} occurrence 追溯审计失败：{error}"))?;
        if invalid != 0 {
            return Err(format!("v19 {table} 与 Feed occurrence 身份不一致。"));
        }
    }
    let invalid_feedback_cards: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM review_quality_feedback feedback
             JOIN review_generated_cards card ON card.id = feedback.generated_card_id
             WHERE feedback.generated_card_id IS NOT NULL
               AND feedback.learning_record_id <> card.learning_record_id",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("v19 feedback 卡片追溯审计失败：{error}"))?;
    if invalid_feedback_cards != 0 {
        return Err("v19 feedback 与 generated-card occurrence 身份不一致。".to_string());
    }

    let expected_states = replay_learning_target_review_states_v19(transaction)?;
    let stored_state_count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM learning_target_review_states",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("v19 目标级复习状态数量审计失败：{error}"))?;
    if usize::try_from(stored_state_count).ok() != Some(expected_states.len()) {
        return Err("v19 目标级复习状态数量不一致。".to_string());
    }
    for expected in expected_states.values() {
        let actual = transaction
            .query_row(
                "SELECT learning_target_id, revision, next_review_at_unix_ms,
                        attempt_count, remembered_count, forgotten_count, success_streak,
                        last_reviewed_at_unix_ms, last_outcome, last_used_hint,
                        last_attempt_id, created_at_unix_ms, updated_at_unix_ms
                 FROM learning_target_review_states WHERE learning_target_id = ?1",
                [expected.learning_target_id],
                |row| {
                    Ok(ReplayedLearningTargetReviewState {
                        learning_target_id: row.get(0)?,
                        revision: row.get(1)?,
                        next_review_at_unix_ms: row.get(2)?,
                        attempt_count: row.get(3)?,
                        remembered_count: row.get(4)?,
                        forgotten_count: row.get(5)?,
                        success_streak: row.get(6)?,
                        last_reviewed_at_unix_ms: row.get(7)?,
                        last_outcome: row.get(8)?,
                        last_used_hint: row.get(9)?,
                        last_attempt_id: row.get(10)?,
                        created_at_unix_ms: row.get(11)?,
                        updated_at_unix_ms: row.get(12)?,
                    })
                },
            )
            .map_err(|error| format!("v19 目标级复习状态审计读取失败：{error}"))?;
        if actual != *expected {
            return Err(format!(
                "v19 学习目标 {} 的复习状态未按 active attempt 顺序重建。",
                expected.learning_target_id
            ));
        }
    }

    let foreign_key_error_count: i64 = transaction
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|error| format!("v19 外键一致性审计失败：{error}"))?;
    if foreign_key_error_count != 0 {
        return Err("v19 迁移后存在外键一致性错误。".to_string());
    }
    Ok(())
}

fn query_type_to_storage(query_type: QueryType) -> &'static str {
    match query_type {
        QueryType::Word => "word",
        QueryType::Phrase => "phrase",
        QueryType::Sentence => "sentence",
        QueryType::Paragraph => "paragraph",
    }
}

fn query_type_from_storage(value: &str) -> Result<QueryType, String> {
    match value {
        "word" => Ok(QueryType::Word),
        "phrase" => Ok(QueryType::Phrase),
        "sentence" => Ok(QueryType::Sentence),
        "paragraph" => Ok(QueryType::Paragraph),
        _ => Err(format!("学习记录包含未知 queryType：{value}")),
    }
}

fn source_type_to_storage(source_type: &SourceType) -> &'static str {
    match source_type {
        SourceType::Manual => "manual",
        SourceType::Clipboard => "clipboard",
        SourceType::WindowsUia => "windows_uia",
        SourceType::AppAdapter => "app_adapter",
        SourceType::Ocr => "ocr",
    }
}

fn source_type_from_storage(value: &str) -> Result<SourceType, String> {
    match value {
        "manual" => Ok(SourceType::Manual),
        "clipboard" => Ok(SourceType::Clipboard),
        "windows_uia" => Ok(SourceType::WindowsUia),
        "app_adapter" => Ok(SourceType::AppAdapter),
        "ocr" => Ok(SourceType::Ocr),
        _ => Err(format!("学习记录包含未知 sourceType：{value}")),
    }
}

pub(crate) fn unix_time_ms() -> Result<i64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间早于 Unix epoch，无法记录学习事件：{error}"))?;
    i64::try_from(duration.as_millis()).map_err(|_| "当前时间超出学习记录可保存范围。".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explanation::{ExampleItem, KeyPointItem, NearMeaningItem, PhraseItem};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_DATABASE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_database_path() -> (PathBuf, PathBuf) {
        let suffix = format!(
            "readray-learning-records-{}-{}",
            std::process::id(),
            TEST_DATABASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(suffix);
        (root.clone(), root.join("readray.sqlite3"))
    }

    fn input(query_text: &str, source_type: SourceType) -> CaptureInput {
        CaptureInput {
            query_text: query_text.to_string(),
            context_text: Some("The company entered a new market.".to_string()),
            source_type,
            source_app: Some("Obsidian.exe".to_string()),
        }
    }

    fn word_card(query_text: &str) -> ExplanationCard {
        ExplanationCard::Word {
            source_text: query_text.to_string(),
            learning_target_text: query_text.to_string(),
            headword: query_text.to_string(),
            part_of_speech: Some("noun".to_string()),
            phonetic: None,
            basic_meanings: vec!["市场".to_string()],
            context_meaning: Some("在语境中表示市场。".to_string()),
            source_sentence: Some("The company entered a new market.".to_string()),
            source_sentence_zh: Some("这家公司进入了一个新市场。".to_string()),
            phrases: vec![PhraseItem {
                phrase: "new market".to_string(),
                meaning: "新市场".to_string(),
            }],
            near_meanings: vec![NearMeaningItem {
                term: "marketplace".to_string(),
                meaning: "交易场所".to_string(),
            }],
            examples: vec![ExampleItem {
                en: "The market is growing.".to_string(),
                zh: "市场正在增长。".to_string(),
            }],
            review_hint: None,
        }
    }

    fn card_for(query_text: &str, query_type: QueryType) -> ExplanationCard {
        match query_type {
            QueryType::Word => word_card(query_text),
            QueryType::Phrase => ExplanationCard::Phrase {
                source_text: query_text.to_string(),
                learning_target_text: query_text.to_string(),
                basic_meaning: "正在进行中".to_string(),
                context_meaning: None,
                composition: None,
                source_sentence: None,
                source_sentence_zh: None,
                examples: vec![],
                review_hint: None,
            },
            QueryType::Sentence => ExplanationCard::Sentence {
                source_text: query_text.to_string(),
                learning_target_text: query_text.to_string(),
                translation: "这项工作仍在进行中。".to_string(),
                key_points: vec![KeyPointItem {
                    expression: "in progress".to_string(),
                    meaning: "正在进行中".to_string(),
                }],
                explanation: None,
                review_hint: None,
            },
            QueryType::Paragraph => ExplanationCard::Paragraph {
                source_text: query_text.to_string(),
                learning_target_text: query_text.to_string(),
                translation: "第一句说明状态，第二句说明下一步。".to_string(),
                key_points: vec![],
                summary: None,
            },
        }
    }

    fn save(
        store: &LearningRecordStore,
        query_text: &str,
        source_type: SourceType,
    ) -> LearningRecord {
        let input = input(query_text, source_type);
        let card = card_for(
            query_text,
            crate::explanation::classify_query_type(query_text).unwrap(),
        );
        store.save(&input, &card).unwrap()
    }

    fn migrate_fresh_database_through_v18(connection: &mut Connection) {
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        for &(version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version <= 18) {
            transaction.execute_batch(sql).unwrap();
            if version == 15 {
                repair_review_targets_v15(&transaction, 10_000).unwrap();
            }
            if version == 16 {
                repair_review_quality_feedback_v16(&transaction).unwrap();
            }
            if version == 17 {
                backfill_learning_record_targets_v17(&transaction).unwrap();
            }
            transaction
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                     VALUES (?1, ?2)",
                    params![version, 1_000 + version],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
    }

    fn insert_v18_record(
        connection: &Connection,
        id: i64,
        query_text: &str,
        learning_target_text: &str,
        query_type: QueryType,
        created_at_unix_ms: i64,
    ) {
        let card = card_for(learning_target_text, query_type);
        connection
            .execute(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type, source_app,
                   context_text, explanation_card_json, schema_version,
                   created_at_unix_ms, difficulty
                 ) VALUES (?1, ?2, ?3, ?4, 'manual', 'test.exe', ?5, ?6, 2, ?7, NULL)",
                params![
                    id,
                    query_text,
                    normalize_query_text(query_text),
                    query_type_to_storage(query_type),
                    format!("Context {id}: {query_text}"),
                    serde_json::to_string(&card).unwrap(),
                    created_at_unix_ms,
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO learning_record_targets (
                   learning_record_id, query_direction, learning_target_text,
                   normalized_target_text, created_at_unix_ms
                 ) VALUES (?1, 'en_to_zh', ?2, ?3, ?4)",
                params![
                    id,
                    learning_target_text,
                    canonicalize_learning_target_text(learning_target_text),
                    created_at_unix_ms
                ],
            )
            .unwrap();
    }

    #[test]
    fn first_migration_creates_database() {
        let (root, path) = test_database_path();
        let store = LearningRecordStore::open(&path).unwrap();
        let version: i64 = store
            .connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert!(path.exists());
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_eighteen_upgrade_aggregates_only_exact_canonical_targets() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let mut connection = Connection::open(&path).unwrap();
        migrate_fresh_database_through_v18(&mut connection);
        for (id, query, target, query_type, created_at) in [
            (1, "SQLite", "SQLite", QueryType::Word, 1_000),
            (2, " sqlite ", "  sqlite  ", QueryType::Word, 1_000),
            (3, "SQLite", "SQLite", QueryType::Phrase, 1_100),
            (4, "config", "config", QueryType::Word, 1_200),
            (5, "configuration", "configuration", QueryType::Word, 1_300),
            (6, "run", "run", QueryType::Word, 1_400),
            (7, "running", "running", QueryType::Word, 1_500),
            (8, "ran", "ran", QueryType::Word, 1_600),
        ] {
            insert_v18_record(&connection, id, query, target, query_type, created_at);
        }
        drop(connection);

        let store = LearningRecordStore::open(&path).unwrap();
        let page = store.list_targets(Some(1), Some(20), None, None).unwrap();
        assert_eq!(page.total, 7);
        let sqlite_word = page
            .targets
            .iter()
            .find(|target| {
                target.query_type == QueryType::Word && target.normalized_target_text == "sqlite"
            })
            .unwrap();
        assert_eq!(sqlite_word.query_count, 2);
        assert_eq!(sqlite_word.representative_record.id, 2);
        assert_eq!(sqlite_word.canonicalization_version, 1);
        assert_eq!(
            store
                .get_target(sqlite_word.id)
                .unwrap()
                .unwrap()
                .occurrences
                .len(),
            2
        );
        let phrase_count = page
            .targets
            .iter()
            .filter(|target| {
                target.query_type == QueryType::Phrase && target.normalized_target_text == "sqlite"
            })
            .count();
        assert_eq!(phrase_count, 1);

        for (table, column) in [
            ("learning_target_occurrences", "learning_target_id"),
            ("review_feed_items", "learning_target_id"),
            ("review_generated_cards", "learning_target_id"),
            ("review_feed_attempts", "learning_target_id"),
        ] {
            let not_null: i64 = store
                .connection
                .query_row(
                    "SELECT \"notnull\" FROM pragma_table_info(?1) WHERE name = ?2",
                    params![table, column],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(not_null, 1, "{table}.{column} 必须为 NOT NULL");
        }
        audit_learning_target_aggregation_v19(&store.connection.unchecked_transaction().unwrap())
            .unwrap();
        drop(store);

        let reopened = LearningRecordStore::open(&path).unwrap();
        assert_eq!(
            reopened
                .connection
                .query_row("SELECT COUNT(*) FROM learning_targets", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            7
        );
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_nineteen_audit_failure_rolls_back_the_whole_migration() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let mut connection = Connection::open(&path).unwrap();
        migrate_fresh_database_through_v18(&mut connection);
        insert_v18_record(&connection, 1, "SQLite", "SQLite", QueryType::Word, 1_000);
        connection
            .execute(
                "UPDATE learning_record_targets
                 SET normalized_target_text = 'corrupt-projection'
                 WHERE learning_record_id = 1",
                [],
            )
            .unwrap();
        drop(connection);

        let error = match LearningRecordStore::open(&path) {
            Ok(_) => panic!("损坏投影必须让 v19 迁移失败"),
            Err(error) => error,
        };
        assert!(error.contains("occurrence 与原始学习目标投影不一致"));
        let connection = Connection::open(&path).unwrap();
        let max_version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let target_table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'learning_targets')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let feed_target_column_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('review_feed_items')
                 WHERE name = 'learning_target_id')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(max_version, 18);
        assert!(!target_table_exists);
        assert!(!feed_target_column_exists);
        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_nineteen_replays_active_attempts_and_preserves_review_traceability() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let mut connection = Connection::open(&path).unwrap();
        migrate_fresh_database_through_v18(&mut connection);
        insert_v18_record(&connection, 1, "SQLite", "SQLite", QueryType::Word, 1_000);
        insert_v18_record(&connection, 2, " sqlite ", "sqlite", QueryType::Word, 1_100);
        connection
            .execute_batch(
                r#"
INSERT INTO review_targets (
  learning_record_id, revision, next_review_at_unix_ms, attempt_count,
  remembered_count, forgotten_count, success_streak,
  created_at_unix_ms, updated_at_unix_ms
) VALUES
  (1, 5, 9000, 1, 1, 0, 1, 1000, 9000),
  (2, 7, 9000, 1, 0, 1, 0, 1100, 9000);

INSERT INTO review_generated_cards (
  id, learning_record_id, variant_index, generation_request_key, content_json,
  model, created_at_unix_ms, expires_at_unix_ms, last_used_at_unix_ms, use_count
) VALUES (301, 1, 0, 'card-301', '{}', 'test-model', 1500, 999999, 1500, 1);

INSERT INTO review_feed_items (
  id, day_start_unix_ms, day_end_unix_ms, learning_record_id, cycle_index,
  ordinal, reason_code, generated_card_id, created_at_unix_ms
) VALUES
  (101, 10000, 20000, 1, 0, 0, 'new_record', 301, 1500),
  (102, 10000, 20000, 2, 0, 1, 'new_record', NULL, 1600),
  (103, 10000, 20000, 1, 1, 2, 'continued_practice', NULL, 1700);

INSERT INTO review_feed_attempts (
  id, feed_item_id, learning_record_id, request_key, expected_revision,
  target_revision, outcome, used_hint, next_review_at_unix_ms,
  previous_next_review_at_unix_ms, previous_attempt_count,
  previous_remembered_count, previous_forgotten_count, previous_success_streak,
  previous_last_reviewed_at_unix_ms, previous_last_outcome,
  previous_last_used_hint, previous_last_attempt_id, created_at_unix_ms,
  undone_at_unix_ms, undo_request_key, undo_expected_revision, undo_target_revision
) VALUES
  (201, 101, 1, 'attempt-later', 0, 1, 'remembered', 0, 259203000,
   1000, 0, 0, 0, 0, NULL, NULL, NULL, NULL, 3000,
   NULL, NULL, NULL, NULL),
  (202, 102, 2, 'attempt-earlier', 0, 1, 'forgotten', 0, 86402000,
   1100, 0, 0, 0, 0, NULL, NULL, NULL, NULL, 2000,
   NULL, NULL, NULL, NULL),
  (203, 103, 1, 'attempt-undone', 1, 2, 'remembered', 1, 172804000,
   259203000, 1, 1, 0, 1, 3000, 'remembered', 0, 201, 4000,
   5000, 'undo-203', 2, 3);

INSERT INTO review_quality_feedback (
  id, feed_item_id, learning_record_id, generated_card_id, card_context_key,
  revision, active, polarity, reason_codes_json, detail,
  created_at_unix_ms, updated_at_unix_ms
) VALUES (401, 101, 1, 301, 'generated:301', 2, 1, 'up', '["needed"]',
          '保留反馈', 3500, 3600);

INSERT INTO review_quality_mutations (
  request_key, feed_item_id, learning_record_id, operation,
  input_json, result_json, created_at_unix_ms
) VALUES ('quality-401', 101, 1, 'save', '{}', '{}', 3600);

INSERT INTO review_card_generation_failures (
  request_key, feed_item_id, learning_record_id, failure_count,
  retry_after_unix_ms, last_error, created_at_unix_ms, updated_at_unix_ms
) VALUES ('failure-102', 102, 2, 2, 9000, 'temporary', 3700, 3800);
"#,
            )
            .unwrap();
        drop(connection);

        let store = LearningRecordStore::open(&path).unwrap();
        let target_id: i64 = store
            .connection
            .query_row(
                "SELECT learning_target_id FROM learning_target_occurrences
                 WHERE learning_record_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let second_target_id: i64 = store
            .connection
            .query_row(
                "SELECT learning_target_id FROM learning_target_occurrences
                 WHERE learning_record_id = 2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(target_id, second_target_id);
        let state: (i64, i64, i64, i64, i64, i64, Option<i64>) = store
            .connection
            .query_row(
                "SELECT revision, next_review_at_unix_ms, attempt_count,
                        remembered_count, forgotten_count, success_streak, last_attempt_id
                 FROM learning_target_review_states WHERE learning_target_id = ?1",
                [target_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(state, (8, 259_203_000, 2, 1, 1, 1, Some(201)));

        for (table, expected) in [
            ("review_feed_items", 3_i64),
            ("review_feed_attempts", 3),
            ("review_generated_cards", 1),
            ("review_quality_feedback", 1),
            ("review_quality_mutations", 1),
            ("review_card_generation_failures", 1),
        ] {
            assert_eq!(
                store
                    .connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                expected,
                "{table} 行数必须保留"
            );
        }
        let feed_slots: Vec<(i64, i64)> = {
            let mut statement = store
                .connection
                .prepare(
                    "SELECT id, target_slot_active FROM review_feed_items
                     WHERE cycle_index = 0 ORDER BY ordinal",
                )
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(feed_slots, vec![(101, 1), (102, 0)]);
        let undone_at: Option<i64> = store
            .connection
            .query_row(
                "SELECT undone_at_unix_ms FROM review_feed_attempts WHERE id = 203",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(undone_at, Some(5_000));
        audit_learning_target_aggregation_v19(&store.connection.unchecked_transaction().unwrap())
            .unwrap();
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_nineteen_active_slot_prefers_latest_active_attempt_without_requiring_repeat() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let mut connection = Connection::open(&path).unwrap();
        migrate_fresh_database_through_v18(&mut connection);
        for (id, query, target) in [
            (1, "SQLite", "SQLite"),
            (2, " sqlite ", "sqlite"),
            (3, "config", "config"),
            (4, " CONFIG ", " config "),
        ] {
            insert_v18_record(&connection, id, query, target, QueryType::Word, 1_000 + id);
        }
        connection
            .execute_batch(
                r#"
INSERT INTO review_feed_items (
  id, day_start_unix_ms, day_end_unix_ms, learning_record_id, cycle_index,
  ordinal, reason_code, generated_card_id, created_at_unix_ms
) VALUES
  (101, 10000, 20000, 1, 0, 0, 'new_record', NULL, 1500),
  (102, 10000, 20000, 2, 0, 1, 'new_record', NULL, 1600),
  (103, 10000, 20000, 3, 0, 2, 'new_record', NULL, 1700),
  (104, 10000, 20000, 4, 0, 3, 'new_record', NULL, 1800);

INSERT INTO review_feed_attempts (
  id, feed_item_id, learning_record_id, request_key, expected_revision,
  target_revision, outcome, used_hint, next_review_at_unix_ms,
  previous_next_review_at_unix_ms, previous_attempt_count,
  previous_remembered_count, previous_forgotten_count, previous_success_streak,
  previous_last_reviewed_at_unix_ms, previous_last_outcome,
  previous_last_used_hint, previous_last_attempt_id, created_at_unix_ms,
  undone_at_unix_ms, undo_request_key, undo_expected_revision, undo_target_revision
) VALUES
  (201, 102, 2, 'late-slot-completed', 0, 1, 'remembered', 0,
   259202000, 1002, 0, 0, 0, 0, NULL, NULL, NULL, NULL, 2000,
   NULL, NULL, NULL, NULL),
  (202, 103, 3, 'first-completed-slot', 0, 1, 'remembered', 0,
   259203000, 1003, 0, 0, 0, 0, NULL, NULL, NULL, NULL, 3000,
   NULL, NULL, NULL, NULL),
  (203, 104, 4, 'latest-completed-slot', 0, 1, 'forgotten', 0,
   86403000, 1004, 0, 0, 0, 0, NULL, NULL, NULL, NULL, 3000,
   NULL, NULL, NULL, NULL);
"#,
            )
            .unwrap();
        drop(connection);

        let store = LearningRecordStore::open(&path).unwrap();
        let slots: Vec<(i64, i64)> = store
            .connection
            .prepare(
                "SELECT id, target_slot_active FROM review_feed_items
                 WHERE day_start_unix_ms = 10000 ORDER BY ordinal, id",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(slots, vec![(101, 0), (102, 1), (103, 0), (104, 1)]);

        let visible_stats: (i64, i64, i64) = store
            .connection
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN attempt.outcome = 'remembered' THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN attempt.outcome = 'forgotten' THEN 1 ELSE 0 END), 0)
                 FROM review_feed_attempts attempt
                 JOIN review_feed_items item ON item.id = attempt.feed_item_id
                 WHERE item.day_start_unix_ms = 10000
                   AND item.target_slot_active = 1
                   AND attempt.undone_at_unix_ms IS NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let visible_without_active_attempt: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM review_feed_items item
                 WHERE item.day_start_unix_ms = 10000
                   AND item.target_slot_active = 1
                   AND NOT EXISTS (
                     SELECT 1 FROM review_feed_attempts attempt
                     WHERE attempt.feed_item_id = item.id
                       AND attempt.undone_at_unix_ms IS NULL
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let preserved_counts: (i64, i64) = store
            .connection
            .query_row(
                "SELECT (SELECT COUNT(*) FROM review_feed_items),
                        (SELECT COUNT(*) FROM review_feed_attempts)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(visible_stats, (2, 1, 1));
        assert_eq!(visible_without_active_attempt, 0);
        assert_eq!(preserved_counts, (4, 3));
        audit_learning_target_aggregation_v19(&store.connection.unchecked_transaction().unwrap())
            .unwrap();
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_nineteen_preserves_cross_day_feed_without_english_projection_as_compatibility_history(
    ) {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let mut connection = Connection::open(&path).unwrap();
        migrate_fresh_database_through_v18(&mut connection);
        insert_v18_record(&connection, 1, "SQLite", "SQLite", QueryType::Word, 1_000);
        for (id, query, created_at) in [
            (38, "时练习，优先保证", 1_038),
            (60, "界面", 1_060),
            (62, "微调", 1_062),
        ] {
            let card = card_for(query, QueryType::Phrase);
            connection
                .execute(
                    "INSERT INTO learning_records (
                       id, query_text, normalized_text, query_type, source_type, source_app,
                       context_text, explanation_card_json, schema_version,
                       created_at_unix_ms, difficulty
                     ) VALUES (?1, ?2, ?3, 'phrase', 'manual', 'legacy.exe',
                               ?4, ?5, 1, ?6, NULL)",
                    params![
                        id,
                        query,
                        normalize_query_text(query),
                        format!("历史上下文 {id}"),
                        serde_json::to_string(&card).unwrap(),
                        created_at
                    ],
                )
                .unwrap();
        }
        connection
            .execute_batch(
                r#"
INSERT INTO review_targets (
  learning_record_id, revision, next_review_at_unix_ms, attempt_count,
  remembered_count, forgotten_count, success_streak,
  created_at_unix_ms, updated_at_unix_ms
) VALUES (1, 1, 259203000, 1, 1, 0, 1, 1000, 3000);

INSERT INTO review_generated_cards (
  id, learning_record_id, variant_index, generation_request_key, content_json,
  model, created_at_unix_ms, expires_at_unix_ms, last_used_at_unix_ms, use_count
) VALUES (301, 1, 0, 'compat-gap-card', '{}', 'test-model', 1500, 999999, 1500, 1);

INSERT INTO review_feed_items (
  id, day_start_unix_ms, day_end_unix_ms, learning_record_id, cycle_index,
  ordinal, reason_code, generated_card_id, created_at_unix_ms
) VALUES
  (101, 10000, 20000, 1, 0, 0, 'new_record', 301, 1500),
  (138, 10000, 20000, 38, 0, 1, 'new_record', NULL, 1501),
  (238, 20000, 30000, 38, 0, 0, 'continued_practice', NULL, 2501),
  (338, 30000, 40000, 38, 0, 0, 'continued_practice', NULL, 3501),
  (160, 10000, 20000, 60, 0, 2, 'new_record', NULL, 1502),
  (260, 20000, 30000, 60, 0, 1, 'continued_practice', NULL, 2502),
  (162, 10000, 20000, 62, 0, 3, 'new_record', NULL, 1503),
  (262, 20000, 30000, 62, 0, 2, 'continued_practice', NULL, 2503);

INSERT INTO review_feed_attempts (
  id, feed_item_id, learning_record_id, request_key, expected_revision,
  target_revision, outcome, used_hint, next_review_at_unix_ms,
  previous_next_review_at_unix_ms, previous_attempt_count,
  previous_remembered_count, previous_forgotten_count, previous_success_streak,
  previous_last_reviewed_at_unix_ms, previous_last_outcome,
  previous_last_used_hint, previous_last_attempt_id, created_at_unix_ms,
  undone_at_unix_ms, undo_request_key, undo_expected_revision, undo_target_revision
) VALUES (201, 101, 1, 'compat-gap-attempt', 0, 1, 'remembered', 0,
          259203000, 1000, 0, 0, 0, 0, NULL, NULL, NULL, NULL, 3000,
          NULL, NULL, NULL, NULL);

INSERT INTO review_quality_feedback (
  id, feed_item_id, learning_record_id, generated_card_id, card_context_key,
  revision, active, polarity, reason_codes_json, detail,
  created_at_unix_ms, updated_at_unix_ms
) VALUES (401, 101, 1, 301, 'generated:301', 1, 1, 'up', '[]',
          '保留反馈', 3100, 3100);

INSERT INTO review_quality_mutations (
  request_key, feed_item_id, learning_record_id, operation,
  input_json, result_json, created_at_unix_ms
) VALUES ('compat-gap-feedback', 101, 1, 'save', '{}', '{}', 3100);

INSERT INTO review_card_generation_failures (
  request_key, feed_item_id, learning_record_id, failure_count,
  retry_after_unix_ms, last_error, created_at_unix_ms, updated_at_unix_ms
) VALUES ('compat-gap-failure', 101, 1, 1, 5000, 'temporary', 3200, 3200);
"#,
            )
            .unwrap();
        drop(connection);

        let store = LearningRecordStore::open(&path).unwrap();
        for (table, expected) in [
            ("review_feed_items", 8_i64),
            ("review_generated_cards", 1),
            ("review_feed_attempts", 1),
            ("review_quality_feedback", 1),
            ("review_quality_mutations", 1),
            ("review_card_generation_failures", 1),
        ] {
            assert_eq!(
                store
                    .connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                expected,
                "{table} 数量必须完整保留"
            );
        }
        let compatibility_targets: Vec<(i64, String, i64, Option<String>, i64)> = store
            .connection
            .prepare(
                "SELECT target.representative_learning_record_id, target.stable_key,
                        target.canonicalization_version, target.normalized_target_text,
                        COUNT(occurrence.learning_record_id)
                 FROM learning_targets target
                 JOIN learning_target_occurrences occurrence
                   ON occurrence.learning_target_id = target.id
                 WHERE target.target_kind = 'legacy_compat'
                 GROUP BY target.id ORDER BY target.representative_learning_record_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            compatibility_targets,
            vec![
                (38, "legacy-compat:record:38".to_string(), 0, None, 1),
                (60, "legacy-compat:record:60".to_string(), 0, None, 1),
                (62, "legacy-compat:record:62".to_string(), 0, None, 1),
            ]
        );
        let compatibility_feed: Vec<(i64, i64, i64)> = store
            .connection
            .prepare(
                "SELECT item.id, item.learning_target_id, item.target_slot_active
                 FROM review_feed_items item
                 JOIN learning_targets target ON target.id = item.learning_target_id
                 WHERE target.target_kind = 'legacy_compat'
                 ORDER BY item.id",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(compatibility_feed.len(), 7);
        assert!(compatibility_feed
            .iter()
            .all(|(_, target_id, active)| *target_id > 0 && *active == 0));
        assert_eq!(
            store
                .list_targets(Some(1), Some(20), None, None)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            store
                .list_targets(Some(1), Some(20), Some("界面"), None)
                .unwrap()
                .total,
            0
        );
        let compatibility_target_id: i64 = store
            .connection
            .query_row(
                "SELECT id FROM learning_targets
                 WHERE stable_key = 'legacy-compat:record:60'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(store.get_target(compatibility_target_id).unwrap().is_none());
        assert!(get_learning_record_from_connection(&store.connection, 38)
            .unwrap()
            .is_some());
        let foreign_key_errors: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(foreign_key_errors, 0);
        audit_learning_target_aggregation_v19(&store.connection.unchecked_transaction().unwrap())
            .unwrap();
        drop(store);

        let reopened = LearningRecordStore::open(&path).unwrap();
        assert_eq!(
            reopened
                .connection
                .query_row("SELECT COUNT(*) FROM review_feed_items", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            8
        );
        assert_eq!(
            reopened
                .connection
                .query_row("SELECT COUNT(*) FROM learning_targets", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            4
        );
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn target_memory_search_pagination_history_and_delete_use_aggregate_identity() {
        let (root, path) = test_database_path();
        let mut store = LearningRecordStore::open(&path).unwrap();
        let first = store
            .save(
                &CaptureInput {
                    query_text: "SQLite".to_string(),
                    context_text: Some("The app keeps a local database.".to_string()),
                    source_type: SourceType::WindowsUia,
                    source_app: Some("Code.exe".to_string()),
                },
                &word_card("SQLite"),
            )
            .unwrap();
        let reverse_card = ExplanationCard::Word {
            source_text: "数据库".to_string(),
            learning_target_text: "SQLite".to_string(),
            headword: "SQLite".to_string(),
            part_of_speech: Some("noun".to_string()),
            phonetic: None,
            basic_meanings: vec!["嵌入式数据库".to_string()],
            context_meaning: Some("在配置页面中查询本地数据库".to_string()),
            source_sentence: None,
            source_sentence_zh: None,
            phrases: vec![],
            near_meanings: vec![],
            examples: vec![],
            review_hint: None,
        };
        let second = store
            .save(
                &CaptureInput {
                    query_text: "数据库".to_string(),
                    context_text: Some("在配置页面中再次看到这个数据库名称。".to_string()),
                    source_type: SourceType::Manual,
                    source_app: Some("Obsidian.exe".to_string()),
                },
                &reverse_card,
            )
            .unwrap();
        save(&store, "configuration", SourceType::Manual);

        let page = store.list_targets(Some(1), Some(1), None, None).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.targets.len(), 1);
        let sqlite_target = store
            .list_targets(Some(1), Some(20), Some("sqlite"), None)
            .unwrap()
            .targets
            .pop()
            .unwrap();
        assert_eq!(sqlite_target.query_count, 2);
        assert_eq!(sqlite_target.representative_record.id, second.id);
        for keyword in ["数据库", "配置页面", "嵌入式数据库"] {
            let result = store
                .list_targets(Some(1), Some(20), Some(keyword), None)
                .unwrap();
            assert_eq!(result.total, 1, "历史字段 {keyword} 必须能命中聚合目标");
            assert_eq!(result.targets[0].id, sqlite_target.id);
        }
        let detail = store.get_target(sqlite_target.id).unwrap().unwrap();
        assert_eq!(detail.occurrences.len(), 2);
        assert_eq!(detail.occurrences[0].id, second.id);
        assert_eq!(detail.occurrences[1].id, first.id);

        assert!(store.delete(second.id).unwrap());
        let after_delete = store.get_target(sqlite_target.id).unwrap().unwrap();
        assert_eq!(after_delete.target.query_count, 1);
        assert_eq!(after_delete.target.representative_record.id, first.id);
        assert_eq!(after_delete.occurrences.len(), 1);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn target_memory_search_prioritizes_exact_target_over_newer_context_match() {
        let (root, path) = test_database_path();
        let store = LearningRecordStore::open(&path).unwrap();
        save(&store, "config", SourceType::Manual);
        save(&store, "configuration", SourceType::Manual);
        store
            .save(
                &CaptureInput {
                    query_text: "toward".to_string(),
                    context_text: Some("SQLite then handoff then config then toward.".to_string()),
                    source_type: SourceType::Manual,
                    source_app: Some("ChatGPT.exe".to_string()),
                },
                &word_card("toward"),
            )
            .unwrap();

        let result = store
            .list_targets(Some(1), Some(20), Some("config"), None)
            .unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(
            result
                .targets
                .iter()
                .map(|target| target.normalized_target_text.as_str())
                .collect::<Vec<_>>(),
            vec!["config", "configuration", "toward"]
        );

        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn latest_migration_keeps_review_state_separate_from_learning_records() {
        let (root, path) = test_database_path();
        let store = LearningRecordStore::open(&path).unwrap();
        let review_tables = [
            "review_targets",
            "review_daily_items",
            "review_attempts",
            "review_quality_feedback",
            "review_quality_mutations",
            "review_generated_cards",
            "review_feed_items",
            "review_feed_attempts",
            "review_card_generation_failures",
        ];
        for table in review_tables {
            let exists: bool = store
                .connection
                .query_row(
                    "SELECT EXISTS(
                       SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
                     )",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "当前 schema 缺少 {table}");
        }
        let learning_columns: Vec<String> = {
            let mut statement = store
                .connection
                .prepare("PRAGMA table_info(learning_records)")
                .unwrap();
            statement
                .query_map([], |row| row.get(1))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert!(!learning_columns.iter().any(|column| {
            column.contains("review")
                || column.contains("attempt")
                || column.contains("remembered")
                || column.contains("forgotten")
        }));
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_ten_database_upgrades_to_latest_review_schema_and_preserves_learning_events() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for &(version, sql) in MIGRATIONS.iter().take(10) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                     VALUES (?1, ?2)",
                    params![version, version * 100],
                )
                .unwrap();
        }
        let card = word_card("market");
        connection
            .execute(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type, source_app,
                   context_text, explanation_card_json, schema_version,
                   created_at_unix_ms, difficulty
                 ) VALUES (41, 'market', 'market', 'word', 'manual', 'Obsidian',
                           'The company entered a new market.', ?1, 1, 1234, NULL)",
                [serde_json::to_string(&card).unwrap()],
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let record: (String, i64) = upgraded
            .query_row(
                "SELECT query_text, created_at_unix_ms FROM learning_records WHERE id = 41",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(record, ("market".to_string(), 1234));
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_eleven_upgrade_preserves_queue_attempt_usage_and_learning_event() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for &(version, sql) in MIGRATIONS.iter().take(11) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                     VALUES (?1, ?2)",
                    params![version, version * 100],
                )
                .unwrap();
        }
        let card = word_card("market");
        connection
            .execute(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type, source_app,
                   context_text, explanation_card_json, schema_version,
                   created_at_unix_ms, difficulty
                 ) VALUES (41, 'market', 'market', 'word', 'manual', 'Obsidian',
                           'The company entered a new market.', ?1, 1, 1234, NULL)",
                [serde_json::to_string(&card).unwrap()],
            )
            .unwrap();
        connection
            .execute_batch(
                "INSERT INTO review_targets (
                   learning_record_id, revision, next_review_at_unix_ms, attempt_count,
                   remembered_count, forgotten_count, success_streak,
                   last_reviewed_at_unix_ms, last_outcome, last_used_hint,
                   last_attempt_id, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (41, 1, 9000, 1, 1, 0, 1, 5000, 'remembered', 0, 81, 1234, 5000);
                 INSERT INTO review_daily_items (
                   id, day_start_unix_ms, day_end_unix_ms, learning_record_id,
                   ordinal, reason_code, created_at_unix_ms
                 ) VALUES (71, 0, 86400000, 41, 3, 'new_record', 4000);
                 INSERT INTO review_attempts (
                   id, daily_item_id, learning_record_id, request_key,
                   expected_revision, target_revision, outcome, used_hint,
                   next_review_at_unix_ms, previous_next_review_at_unix_ms,
                   previous_attempt_count, previous_remembered_count,
                   previous_forgotten_count, previous_success_streak,
                   previous_last_reviewed_at_unix_ms, previous_last_outcome,
                   previous_last_used_hint, previous_last_attempt_id,
                   created_at_unix_ms
                 ) VALUES (
                   81, 71, 41, 'legacy-attempt', 0, 1, 'remembered', 0,
                   9000, 1234, 0, 0, 0, 0, NULL, NULL, NULL, NULL, 5000
                 );
                 INSERT INTO model_usage_records (
                   id, category, prompt_tokens, completion_tokens, total_tokens,
                   created_at_unix_ms
                 ) VALUES (91, 'quick_ai', 10, 5, 15, 6000);",
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let feed_item: (i64, i64, i64, String) = upgraded
            .query_row(
                "SELECT id, cycle_index, ordinal, reason_code
                 FROM review_feed_items WHERE learning_record_id = 41",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let feed_attempt: (i64, i64, String) = upgraded
            .query_row(
                "SELECT id, feed_item_id, request_key
                 FROM review_feed_attempts WHERE learning_record_id = 41",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let learning_event: (String, i64) = upgraded
            .query_row(
                "SELECT query_text, created_at_unix_ms FROM learning_records WHERE id = 41",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let usage: (String, i64) = upgraded
            .query_row(
                "SELECT category, total_tokens FROM model_usage_records WHERE id = 91",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        upgraded
            .execute(
                "INSERT INTO model_usage_records (
                   category, prompt_tokens, completion_tokens, total_tokens,
                   created_at_unix_ms
                 ) VALUES ('review_card', 7, 3, 10, 7000)",
                [],
            )
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(feed_item, (71, 0, 3, "new_record".to_string()));
        assert_eq!(feed_attempt, (81, 71, "legacy-attempt".to_string()));
        assert_eq!(learning_event, ("market".to_string(), 1234));
        assert_eq!(usage, ("quick_ai".to_string(), 15));
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_twelve_upgrade_adds_bounded_card_pool_and_card_scoped_feedback() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for &(version, sql) in MIGRATIONS.iter().take(12) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                     VALUES (?1, ?2)",
                    params![version, version * 100],
                )
                .unwrap();
        }
        let card = word_card("market");
        connection
            .execute(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type, source_app,
                   context_text, explanation_card_json, schema_version,
                   created_at_unix_ms, difficulty
                 ) VALUES (41, 'market', 'market', 'word', 'manual', 'Obsidian',
                           'The company entered a new market.', ?1, 1, 1234, NULL)",
                [serde_json::to_string(&card).unwrap()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO review_generated_cards (
                   id, learning_record_id, variant_index, generation_request_key,
                   content_json, model, created_at_unix_ms
                 ) VALUES (51, 41, 0, 'legacy-generated-card', ?1, 'test-model', 3000)",
                [serde_json::json!({
                    "englishContext": "The company entered a new market.",
                    "englishContextZh": "这家公司进入了一个新市场。",
                    "hint": "想想表示市场的名词。"
                })
                .to_string()],
            )
            .unwrap();
        connection
            .execute_batch(
                "INSERT INTO review_feed_items (
                   id, day_start_unix_ms, day_end_unix_ms, learning_record_id,
                   cycle_index, ordinal, reason_code, generated_card_id, created_at_unix_ms
                 ) VALUES (61, 0, 86400000, 41, 0, 0, 'new_record', 51, 3000);
                 INSERT INTO review_quality_feedback (
                   id, learning_record_id, revision, active, polarity,
                   reason_codes_json, detail, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (71, 41, 2, 1, 'down', '[\"unclear_prompt\"]',
                           '旧卡提示不清楚', 3100, 3200);",
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let pool_state: (i64, i64, i64) = upgraded
            .query_row(
                "SELECT expires_at_unix_ms, last_used_at_unix_ms, use_count
                 FROM review_generated_cards WHERE id = 51",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let feedback_identity: (i64, i64, Option<i64>, String, String) = upgraded
            .query_row(
                "SELECT feed_item_id, learning_record_id, generated_card_id,
                        card_context_key, polarity
                 FROM review_quality_feedback WHERE id = 71",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        let learning_event: (String, i64) = upgraded
            .query_row(
                "SELECT query_text, created_at_unix_ms FROM learning_records WHERE id = 41",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(pool_state, (3000, 3000, 1));
        assert_eq!(
            feedback_identity,
            (
                61,
                41,
                Some(51),
                "generated:51".to_string(),
                "down".to_string()
            )
        );
        assert_eq!(learning_event, ("market".to_string(), 1234));
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_thirteen_upgrade_removes_later_cycles_after_an_incomplete_cycle() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for &(version, sql) in MIGRATIONS.iter().take(13) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                     VALUES (?1, ?2)",
                    params![version, version * 100],
                )
                .unwrap();
        }
        let card = serde_json::to_string(&word_card("market")).unwrap();
        connection
            .execute_batch(&format!(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type, source_app,
                   context_text, explanation_card_json, schema_version,
                   created_at_unix_ms, difficulty
                 ) VALUES
                   (41, 'market', 'market', 'word', 'manual', 'Obsidian',
                    'The company entered a new market.', '{card}', 1, 1234, NULL),
                   (42, 'signal', 'signal', 'word', 'manual', 'Obsidian',
                    'The device received a clear signal.', '{card}', 1, 1235, NULL);
                 INSERT INTO review_targets (
                   learning_record_id, revision, next_review_at_unix_ms,
                   attempt_count, remembered_count, forgotten_count, success_streak,
                   created_at_unix_ms, updated_at_unix_ms
                 ) VALUES
                   (41, 1, 9000, 1, 1, 0, 1, 1234, 5000),
                   (42, 0, 1235, 0, 0, 0, 0, 1235, 1235);
                 INSERT INTO review_feed_items (
                   id, day_start_unix_ms, day_end_unix_ms, learning_record_id,
                   cycle_index, ordinal, reason_code, created_at_unix_ms
                 ) VALUES
                   (61, 0, 86400000, 41, 0, 0, 'new_record', 4000),
                   (62, 0, 86400000, 42, 0, 1, 'new_record', 4000),
                   (63, 0, 86400000, 41, 1, 2, 'continued_practice', 4100),
                   (64, 0, 86400000, 42, 2, 3, 'continued_practice', 4200);
                 INSERT INTO review_feed_attempts (
                   id, feed_item_id, learning_record_id, request_key,
                   expected_revision, target_revision, outcome, used_hint,
                   next_review_at_unix_ms, previous_next_review_at_unix_ms,
                   previous_attempt_count, previous_remembered_count,
                   previous_forgotten_count, previous_success_streak,
                   created_at_unix_ms
                 ) VALUES (
                   71, 61, 41, 'completed-first-item', 0, 1, 'remembered', 0,
                   9000, 1234, 0, 0, 0, 0, 5000
                 );"
            ))
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let remaining_items: Vec<(i64, i64)> = upgraded
            .prepare(
                "SELECT id, cycle_index FROM review_feed_items
                 WHERE day_start_unix_ms = 0 ORDER BY ordinal",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let attempt_count: i64 = upgraded
            .query_row("SELECT COUNT(*) FROM review_feed_attempts", [], |row| {
                row.get(0)
            })
            .unwrap();
        let failure_table_exists: bool = upgraded
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM sqlite_master
                   WHERE type = 'table' AND name = 'review_card_generation_failures'
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(remaining_items, vec![(61, 0), (62, 0)]);
        assert_eq!(attempt_count, 1);
        assert!(failure_table_exists);
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_thirteen_upgrade_preserves_later_cycle_with_feedback_but_no_attempt() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for &(version, sql) in MIGRATIONS.iter().take(13) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                     VALUES (?1, ?2)",
                    params![version, version * 100],
                )
                .unwrap();
        }
        let card = serde_json::to_string(&word_card("market")).unwrap();
        connection
            .execute_batch(&format!(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type, source_app,
                   context_text, explanation_card_json, schema_version,
                   created_at_unix_ms, difficulty
                 ) VALUES
                   (41, 'market', 'market', 'word', 'manual', 'Obsidian',
                    'The company entered a new market.', '{card}', 1, 1000, NULL),
                   (42, 'signal', 'signal', 'word', 'manual', 'Obsidian',
                    'The device received a clear signal.', '{card}', 1, 1100, NULL);
                 INSERT INTO review_targets (
                   learning_record_id, revision, next_review_at_unix_ms,
                   attempt_count, remembered_count, forgotten_count, success_streak,
                   created_at_unix_ms, updated_at_unix_ms
                 ) VALUES
                   (41, 1, 9000, 1, 1, 0, 1, 1000, 5000),
                   (42, 0, 1100, 0, 0, 0, 0, 1100, 1100);
                 INSERT INTO review_feed_items (
                   id, day_start_unix_ms, day_end_unix_ms, learning_record_id,
                   cycle_index, ordinal, reason_code, created_at_unix_ms
                 ) VALUES
                   (61, 0, 86400000, 41, 0, 0, 'new_record', 4000),
                   (62, 0, 86400000, 42, 0, 1, 'new_record', 4100),
                   (63, 0, 86400000, 41, 1, 2, 'continued_practice', 4200),
                   (64, 0, 86400000, 42, 2, 3, 'continued_practice', 4300);
                 INSERT INTO review_feed_attempts (
                   id, feed_item_id, learning_record_id, request_key,
                   expected_revision, target_revision, outcome, used_hint,
                   next_review_at_unix_ms, previous_next_review_at_unix_ms,
                   previous_attempt_count, previous_remembered_count,
                   previous_forgotten_count, previous_success_streak,
                   created_at_unix_ms
                 ) VALUES (
                   71, 61, 41, 'completed-first-item', 0, 1, 'remembered', 0,
                   9000, 1000, 0, 0, 0, 0, 5000
                 );
                 INSERT INTO review_quality_feedback (
                   id, feed_item_id, learning_record_id, generated_card_id,
                   card_context_key, revision, active, polarity, reason_codes_json,
                   detail, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (81, 63, 41, NULL, 'recorded', 0, 1, 'down', '[]',
                           '后续轮次的卡片问题', 4200, 4200);"
            ))
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let remaining_items: Vec<(i64, i64)> = upgraded
            .prepare(
                "SELECT id, cycle_index FROM review_feed_items
                 WHERE day_start_unix_ms = 0 ORDER BY ordinal",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let feedback_exists: bool = upgraded
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM review_quality_feedback WHERE id = 81)",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(remaining_items, vec![(61, 0), (62, 0), (63, 1)]);
        assert!(feedback_exists, "持久质量反馈必须随 Feed 条目保留");
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_thirteen_upgrade_preserves_later_cycles_with_active_or_undone_attempts() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for &(version, sql) in MIGRATIONS.iter().take(13) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                     VALUES (?1, ?2)",
                    params![version, version * 100],
                )
                .unwrap();
        }
        let card = serde_json::to_string(&word_card("market")).unwrap();
        connection
            .execute(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type, source_app,
                   context_text, explanation_card_json, schema_version,
                   created_at_unix_ms, difficulty
                 ) VALUES (41, 'market', 'market', 'word', 'manual', 'Obsidian',
                           'The company entered a new market.', ?1, 1, 1000, NULL)",
                [card],
            )
            .unwrap();
        let next_review_at = 6000 + 3 * REVIEW_DAY_UNIX_MS;
        connection
            .execute_batch(&format!(
                "INSERT INTO review_targets (
                   learning_record_id, revision, next_review_at_unix_ms,
                   attempt_count, remembered_count, forgotten_count, success_streak,
                   last_reviewed_at_unix_ms, last_outcome, last_used_hint, last_attempt_id,
                   created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (41, 5, {next_review_at}, 1, 1, 0, 1,
                           6000, 'remembered', 0, 72, 1000, 7000);
                 INSERT INTO review_feed_items (
                   id, day_start_unix_ms, day_end_unix_ms, learning_record_id,
                   cycle_index, ordinal, reason_code, created_at_unix_ms
                 ) VALUES
                   (61, 0, 86400000, 41, 0, 0, 'new_record', 4000),
                   (62, 0, 86400000, 41, 1, 1, 'continued_practice', 5000),
                   (63, 0, 86400000, 41, 2, 2, 'continued_practice', 6000);
                 INSERT INTO review_feed_attempts (
                   id, feed_item_id, learning_record_id, request_key,
                   expected_revision, target_revision, outcome, used_hint,
                   next_review_at_unix_ms, previous_next_review_at_unix_ms,
                   previous_attempt_count, previous_remembered_count,
                   previous_forgotten_count, previous_success_streak,
                   created_at_unix_ms, undone_at_unix_ms, undo_request_key,
                   undo_expected_revision, undo_target_revision
                 ) VALUES
                   (71, 61, 41, 'cycle-zero', 0, 1, 'remembered', 0,
                    259205000, 1000, 0, 0, 0, 0, 5000, 7000,
                    'undo-cycle-zero', 4, 5),
                   (72, 62, 41, 'cycle-one', 1, 2, 'remembered', 0,
                    {next_review_at}, 259205000, 1, 1, 0, 1, 6000, NULL,
                    NULL, NULL, NULL),
                   (73, 63, 41, 'cycle-two', 2, 3, 'forgotten', 1,
                    86406500, {next_review_at}, 2, 2, 0, 2, 6500, 6600,
                    'undo-cycle-two', 3, 4);
                 INSERT INTO review_quality_feedback (
                   id, feed_item_id, learning_record_id, generated_card_id,
                   card_context_key, revision, active, polarity, reason_codes_json,
                   detail, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (81, 62, 41, NULL, 'recorded', 0, 1, 'down', '[]',
                           'cycle one feedback', 6100, 6100);"
            ))
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let items: Vec<(i64, i64)> = upgraded
            .prepare(
                "SELECT id, cycle_index FROM review_feed_items
                 WHERE learning_record_id = 41 ORDER BY cycle_index",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let attempts: Vec<(i64, Option<i64>)> = upgraded
            .prepare(
                "SELECT id, undone_at_unix_ms FROM review_feed_attempts
                 WHERE learning_record_id = 41 ORDER BY id",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let feedback_exists: bool = upgraded
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM review_quality_feedback WHERE id = 81)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let target: (i64, i64, i64, i64, Option<i64>) = upgraded
            .query_row(
                "SELECT revision, attempt_count, remembered_count, success_streak,
                        last_attempt_id
                 FROM review_targets WHERE learning_record_id = 41",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(items, vec![(61, 0), (62, 1), (63, 2)]);
        assert_eq!(
            attempts,
            vec![(71, Some(7000)), (72, None), (73, Some(6600))]
        );
        assert!(feedback_exists);
        assert_eq!(target, (5, 1, 1, 1, Some(72)));
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_fifteen_repairs_target_left_inconsistent_by_old_version_fourteen() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for &(version, sql) in MIGRATIONS.iter().take(13) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                     VALUES (?1, ?2)",
                    params![version, version * 100],
                )
                .unwrap();
        }
        let card = serde_json::to_string(&word_card("market")).unwrap();
        connection
            .execute(
                "INSERT INTO learning_records (
                   id, query_text, normalized_text, query_type, source_type,
                   context_text, explanation_card_json, schema_version,
                   created_at_unix_ms
                 ) VALUES (41, 'market', 'market', 'word', 'manual',
                           'The company entered a new market.', ?1, 1, 1000)",
                [card],
            )
            .unwrap();
        connection
            .execute_batch(&format!(
                "INSERT INTO review_targets (
                   learning_record_id, revision, next_review_at_unix_ms,
                   attempt_count, remembered_count, forgotten_count, success_streak,
                   last_reviewed_at_unix_ms, last_outcome, last_used_hint, last_attempt_id,
                   created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (41, 5, {}, 1, 1, 0, 1,
                           6000, 'remembered', 0, 72, 1000, 7000);
                 INSERT INTO review_feed_items (
                   id, day_start_unix_ms, day_end_unix_ms, learning_record_id,
                   cycle_index, ordinal, reason_code, created_at_unix_ms
                 ) VALUES
                   (61, 0, 86400000, 41, 0, 0, 'new_record', 4000),
                   (62, 0, 86400000, 41, 1, 1, 'continued_practice', 5000);
                 INSERT INTO review_feed_attempts (
                   id, feed_item_id, learning_record_id, request_key,
                   expected_revision, target_revision, outcome, used_hint,
                   next_review_at_unix_ms, previous_next_review_at_unix_ms,
                   previous_attempt_count, previous_remembered_count,
                   previous_forgotten_count, previous_success_streak,
                   created_at_unix_ms, undone_at_unix_ms, undo_request_key,
                   undo_expected_revision, undo_target_revision
                 ) VALUES
                   (71, 61, 41, 'cycle-zero', 0, 1, 'remembered', 0,
                    259205000, 1000, 0, 0, 0, 0, 5000, 7000,
                    'undo-cycle-zero', 4, 5),
                   (72, 62, 41, 'cycle-one', 1, 2, 'remembered', 0,
                    {}, 259205000, 1, 1, 0, 1, 6000, NULL,
                    NULL, NULL, NULL);",
                6000 + 3 * REVIEW_DAY_UNIX_MS,
                6000 + 3 * REVIEW_DAY_UNIX_MS,
            ))
            .unwrap();
        connection.execute_batch(MIGRATION_14).unwrap();
        connection
            .execute("DELETE FROM review_feed_items WHERE id = 62", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, applied_at_unix_ms) VALUES (14, 1400)",
                [],
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let target: (
            i64,
            i64,
            i64,
            i64,
            i64,
            Option<i64>,
            Option<String>,
            Option<i64>,
        ) = upgraded
            .query_row(
                "SELECT revision, next_review_at_unix_ms, attempt_count,
                        remembered_count, success_streak, last_reviewed_at_unix_ms,
                        last_outcome, last_attempt_id
                 FROM review_targets WHERE learning_record_id = 41",
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
                    ))
                },
            )
            .unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(target, (6, 1000, 0, 0, 0, None, None, None));
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_two_database_upgrade_removes_unclassified_test_conversations() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                 VALUES (1, 100), (2, 200)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO quick_ai_conversations (
                    id, title, model, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (44, '保留的会话', 'deepseek-v4-flash', 100, 100)",
                [],
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let legacy_conversation_count: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM quick_ai_conversations WHERE id = 44",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let writing_table_count: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'writing_documents'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(legacy_conversation_count, 0);
        assert_eq!(writing_table_count, 1);
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_nine_database_removes_legacy_conversations_and_cascades_messages() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute(
                "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                 VALUES
                   (1, 100), (2, 200), (3, 300), (4, 400), (5, 500),
                   (6, 600), (7, 700), (8, 800), (9, 900)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO quick_ai_conversations (
                    id, title, model, created_at_unix_ms, updated_at_unix_ms, origin
                 ) VALUES
                   (41, '旧测试会话', 'deepseek-v4-flash', 100, 100, 'legacy'),
                   (42, '主窗口会话', 'deepseek-v4-flash', 200, 200, 'main'),
                   (43, 'Quick AI 会话', 'deepseek-v4-flash', 300, 300, 'overlay')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO quick_ai_messages (
                    id, conversation_id, role, content, sequence, created_at_unix_ms
                 ) VALUES
                   (51, 41, 'user', '旧消息', 1, 100),
                   (52, 42, 'user', '主窗口消息', 1, 200),
                   (53, 43, 'user', 'Quick AI 消息', 1, 300)",
                [],
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let legacy_conversations: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM quick_ai_conversations WHERE origin = 'legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let legacy_messages: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM quick_ai_messages WHERE conversation_id = 41",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let preserved_origins: String = upgraded
            .query_row(
                "SELECT group_concat(origin, ',') FROM (
                   SELECT origin FROM quick_ai_conversations ORDER BY id
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let preserved_messages: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM quick_ai_messages WHERE conversation_id IN (42, 43)",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(legacy_conversations, 0);
        assert_eq!(legacy_messages, 0);
        assert_eq!(preserved_origins, "main,overlay");
        assert_eq!(preserved_messages, 2);
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_three_database_upgrades_writing_target_metadata_without_data_loss() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                 VALUES (1, 100), (2, 200), (3, 300)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO writing_documents (
                    id, revision, created_at_unix_ms, updated_at_unix_ms,
                    last_opened_at_unix_ms, draft_title, draft_paragraphs_json,
                    draft_updated_at_unix_ms, comparison_baseline_title,
                    comparison_baseline_paragraphs_json
                 ) VALUES (71, 5, 100, 200, NULL, '保留草稿', '[\"body\"]', 200, '', '[\"\"]')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO writing_versions (
                    id, document_id, ordinal, source_revision, title, paragraphs_json,
                    comparison_baseline_title, comparison_baseline_paragraphs_json,
                    analysis_json, completed_at_unix_ms
                 ) VALUES (
                    81, 71, 1, 4, '旧完成稿', '[\"version body\"]',
                    '旧基线', '[\"baseline body\"]',
                    '{\"issues\":[],\"patterns\":[]}', 190
                 )",
                [],
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let document_metadata_columns: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('writing_documents')
                 WHERE name = 'comparison_baseline_revision'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let answer_target_columns: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('writing_assistant_answers')
                 WHERE name = 'version_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let preserved: (String, i64) = upgraded
            .query_row(
                "SELECT draft_title, revision FROM writing_documents WHERE id = 71",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let unknown_legacy_baseline_revision: Option<i64> = upgraded
            .query_row(
                "SELECT comparison_baseline_revision
                 FROM writing_documents WHERE id = 71",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let preserved_legacy_version: (i64, String, Option<i64>) = upgraded
            .query_row(
                "SELECT source_revision, analysis_json, analysis_revision
                 FROM writing_versions WHERE id = 81",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(document_metadata_columns, 1);
        assert_eq!(answer_target_columns, 1);
        assert_eq!(preserved, ("保留草稿".to_string(), 5));
        assert_eq!(unknown_legacy_baseline_revision, None);
        assert_eq!(preserved_legacy_version.0, 4);
        assert_eq!(
            preserved_legacy_version.1,
            "{\"issues\":[],\"patterns\":[]}"
        );
        assert_eq!(preserved_legacy_version.2, None);
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_four_database_adds_usage_table_and_removes_unclassified_test_conversations() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection.execute_batch(MIGRATION_4).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                 VALUES (1, 100), (2, 200), (3, 300), (4, 400)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO quick_ai_conversations (
                    id, title, model, created_at_unix_ms, updated_at_unix_ms
                 ) VALUES (45, '升级后保留', 'deepseek-v4-flash', 500, 500)",
                [],
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let usage_table_count: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'model_usage_records'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let legacy_conversation_count: i64 = upgraded
            .query_row(
                "SELECT COUNT(*) FROM quick_ai_conversations WHERE id = 45",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let usage_count: i64 = upgraded
            .query_row("SELECT COUNT(*) FROM model_usage_records", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(usage_table_count, 1);
        assert_eq!(legacy_conversation_count, 0);
        assert_eq!(usage_count, 0);
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_five_database_adds_default_preferences_without_losing_usage() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection.execute_batch(MIGRATION_4).unwrap();
        connection.execute_batch(MIGRATION_5).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                 VALUES (1, 100), (2, 200), (3, 300), (4, 400), (5, 500)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO model_usage_records (
                    category, prompt_tokens, completion_tokens, total_tokens, created_at_unix_ms
                 ) VALUES ('quick_ai', 10, 5, 15, 600)",
                [],
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let usage_total: i64 = upgraded
            .query_row(
                "SELECT total_tokens FROM model_usage_records WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let defaults: (i64, String, i64, String, i64, String) = upgraded
            .query_row(
                "SELECT revision, ui_font, ui_font_size, learning_font,
                        learning_font_size, send_shortcut
                 FROM app_preferences WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert_eq!(usage_total, 15);
        assert_eq!(
            defaults,
            (
                0,
                "geist_source_han_sans".to_string(),
                14,
                "newsreader_source_han_serif".to_string(),
                17,
                "enter".to_string(),
            )
        );
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_six_preferences_upgrade_adds_lifecycle_defaults_without_data_loss() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute(
                "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                 VALUES (1, 100), (2, 200), (3, 300), (4, 400), (5, 500), (6, 600)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE app_preferences SET revision = 4, ui_font_size = 16,
                 learning_font_size = 20, send_shortcut = 'ctrl_enter' WHERE id = 1",
                [],
            )
            .unwrap();
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let values: (i64, i64, i64, String, String, String, String) = upgraded
            .query_row(
                "SELECT revision, ui_font_size, learning_font_size, send_shortcut,
                        close_behavior, quick_query_shortcut,
                        selection_explanation_shortcut
                 FROM app_preferences WHERE id = 1",
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
                    ))
                },
            )
            .unwrap();
        let theme_defaults: (i64, String, String) = upgraded
            .query_row(
                "SELECT revision, theme_id, mode FROM theme_preferences WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            values,
            (
                4,
                16,
                20,
                "ctrl_enter".to_string(),
                "hide_to_tray".to_string(),
                "Ctrl+Alt+R".to_string(),
                "Ctrl+Alt+U".to_string(),
            )
        );
        assert_eq!(
            theme_defaults,
            (0, "readray-default".to_string(), "light".to_string())
        );
        drop(upgraded);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn successful_record_saves_and_reads() {
        let (root, path) = test_database_path();
        let store = LearningRecordStore::open(&path).unwrap();
        let saved = save(&store, "market", SourceType::Manual);
        let read = store.get(saved.id).unwrap().unwrap();

        assert_eq!(read.query_text, "market");
        assert_eq!(read.normalized_text, "market");
        assert_eq!(read.source_type, SourceType::Manual);
        assert_eq!(read.source_app.as_deref(), Some("Obsidian.exe"));
        assert_eq!(read.schema_version, EXPLANATION_CARD_SCHEMA_VERSION);
        assert!(read.difficulty.is_none());
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_seventeen_backfills_only_reliable_english_targets() {
        let (directory, path) = test_database_path();
        fs::create_dir_all(&directory).unwrap();
        let store = LearningRecordStore::open(&path).unwrap();
        for (id, query_text) in [(41_i64, "legacy target"), (42_i64, "旧中文记录")] {
            let card = word_card(query_text);
            store
                .connection
                .execute(
                    "INSERT INTO learning_records (
                       id, query_text, normalized_text, query_type, source_type, source_app,
                       context_text, explanation_card_json, schema_version, created_at_unix_ms, difficulty
                     ) VALUES (?1, ?2, ?3, 'word', 'manual', NULL, NULL, ?4, 1, ?5, NULL)",
                    params![
                        id,
                        query_text,
                        normalize_query_text(query_text),
                        serde_json::to_string(&card).unwrap(),
                        1_000 + id,
                    ],
                )
                .unwrap();
        }
        store
            .connection
            .execute_batch(
                "DELETE FROM schema_migrations WHERE version = 17;
                 DROP TABLE learning_record_targets;",
            )
            .unwrap();
        drop(store);

        let upgraded = LearningRecordStore::open(&path).unwrap();
        let target_backfill = upgraded.connection.unchecked_transaction().unwrap();
        backfill_learning_targets_v19(&target_backfill).unwrap();
        rebuild_learning_target_review_states_v19(&target_backfill).unwrap();
        audit_learning_target_aggregation_v19(&target_backfill).unwrap();
        target_backfill.commit().unwrap();
        let raw_count: i64 = upgraded
            .connection
            .query_row("SELECT COUNT(*) FROM learning_records", [], |row| {
                row.get(0)
            })
            .unwrap();
        let target_count: i64 = upgraded
            .connection
            .query_row("SELECT COUNT(*) FROM learning_record_targets", [], |row| {
                row.get(0)
            })
            .unwrap();
        let page = upgraded.list(Some(1), Some(20), None, None).unwrap();
        assert_eq!(raw_count, 2);
        assert_eq!(target_count, 1);
        assert_eq!(page.total, 1);
        assert_eq!(page.records[0].learning_target_text, "legacy target");
        assert_eq!(page.records[0].query_direction, QueryDirection::EnToZh);
        assert!(upgraded.get(42).unwrap().is_none());
        drop(upgraded);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn version_seventeen_database_upgrades_to_explanation_cache_v18() {
        let (directory, path) = test_database_path();
        fs::create_dir_all(&directory).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                   version INTEGER PRIMARY KEY,
                   applied_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for &(version, sql) in MIGRATIONS.iter().take(17) {
            connection.execute_batch(sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, applied_at_unix_ms)
                     VALUES (?1, ?2)",
                    params![version, version * 100],
                )
                .unwrap();
        }
        drop(connection);

        let upgraded = open_database(&path).unwrap();
        let version: i64 = upgraded
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let cache_table_exists: bool = upgraded
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM sqlite_master
                   WHERE type = 'table' AND name = 'explanation_card_cache'
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let cache_columns: Vec<String> = {
            let mut statement = upgraded
                .prepare("PRAGMA table_info(explanation_card_cache)")
                .unwrap();
            statement
                .query_map([], |row| row.get(1))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(version, DATABASE_SCHEMA_VERSION);
        assert!(cache_table_exists);
        for required in [
            "cache_key",
            "normalized_source_text",
            "query_direction",
            "query_type",
            "minimal_context_fingerprint",
            "model_id",
            "model_revision",
            "prompt_version",
            "schema_version",
            "explanation_card_json",
            "created_at_unix_ms",
            "last_accessed_at_unix_ms",
        ] {
            assert!(cache_columns.iter().any(|column| column == required));
        }
        drop(upgraded);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn learning_event_and_target_projection_are_atomic() {
        let (directory, path) = test_database_path();
        fs::create_dir_all(&directory).unwrap();
        let store = LearningRecordStore::open(&path).unwrap();
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_learning_target
                 BEFORE INSERT ON learning_record_targets
                 BEGIN SELECT RAISE(ABORT, 'target rejected'); END;",
            )
            .unwrap();
        let error = store
            .save(&input("market", SourceType::Manual), &word_card("market"))
            .unwrap_err();
        let raw_count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM learning_records", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(error.contains("规范英文学习目标写入失败"));
        assert_eq!(raw_count, 0);
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn chinese_record_is_searchable_by_target_and_source_but_displays_english() {
        let (directory, path) = test_database_path();
        fs::create_dir_all(&directory).unwrap();
        let store = LearningRecordStore::open(&path).unwrap();
        let chinese_input = CaptureInput {
            query_text: "界面".to_string(),
            context_text: Some("这个界面支持本地学习记录。".to_string()),
            source_type: SourceType::WindowsUia,
            source_app: Some("Obsidian.exe".to_string()),
        };
        let card = ExplanationCard::Word {
            source_text: "界面".to_string(),
            learning_target_text: "interface".to_string(),
            headword: "interface".to_string(),
            part_of_speech: Some("noun".to_string()),
            phonetic: None,
            basic_meanings: vec!["界面".to_string()],
            context_meaning: Some("interface".to_string()),
            source_sentence: Some("这个界面支持本地学习记录。".to_string()),
            source_sentence_zh: None,
            phrases: vec![],
            near_meanings: vec![],
            examples: vec![],
            review_hint: None,
        };
        let saved = store.save(&chinese_input, &card).unwrap();
        assert_eq!(saved.query_text, "界面");
        assert_eq!(saved.learning_target_text, "interface");
        assert_eq!(saved.query_direction, QueryDirection::ZhToEn);
        for keyword in ["interface", "界面", "本地学习"] {
            let page = store.list(Some(1), Some(20), Some(keyword), None).unwrap();
            assert_eq!(page.total, 1, "keyword={keyword}");
            assert_eq!(page.records[0].learning_target_text, "interface");
        }
        let summary = store.summarize_range(0, i64::MAX).unwrap();
        assert_eq!(summary.record_count, 1);
        assert_eq!(
            summary.latest_record.unwrap().learning_target_text,
            "interface"
        );
        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn all_query_types_round_trip_through_json() {
        let (root, path) = test_database_path();
        let store = LearningRecordStore::open(&path).unwrap();
        let cases = [
            ("anchorRect", QueryType::Word),
            ("in progress", QueryType::Phrase),
            ("The work is still in progress.", QueryType::Sentence),
            (
                "The first sentence explains the state. The second describes the next action.",
                QueryType::Paragraph,
            ),
        ];

        for (query_text, query_type) in cases {
            let input = input(query_text, SourceType::WindowsUia);
            let card = card_for(query_text, query_type);
            let saved = store.save(&input, &card).unwrap();
            let read = store.get(saved.id).unwrap().unwrap();
            assert_eq!(read.query_type, query_type);
            assert_eq!(read.explanation_card, card);
        }

        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn search_filter_pagination_and_delete_work() {
        let (root, path) = test_database_path();
        let mut store = LearningRecordStore::open(&path).unwrap();
        let first = save(&store, "market", SourceType::Manual);
        let second = save(&store, "market share", SourceType::Manual);
        save(&store, "The market is growing.", SourceType::WindowsUia);

        let search = store.list(Some(1), Some(10), Some("MARKET"), None).unwrap();
        assert_eq!(search.total, 3);
        let word_only = store
            .list(Some(1), Some(10), Some("market"), Some(QueryType::Word))
            .unwrap();
        assert_eq!(word_only.total, 1);
        let second_page = store.list(Some(2), Some(1), None, None).unwrap();
        assert_eq!(second_page.records.len(), 1);
        assert!(store.delete(second.id).unwrap());
        assert!(store.get(second.id).unwrap().is_none());
        assert!(!store.delete(second.id).unwrap());
        assert!(store.get(first.id).unwrap().is_some());

        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_queries_remain_distinct_events() {
        let (root, path) = test_database_path();
        let store = LearningRecordStore::open(&path).unwrap();
        let first = save(&store, "market", SourceType::Manual);
        let second = save(&store, "market", SourceType::Manual);

        assert_ne!(first.id, second.id);
        assert_eq!(
            store
                .list(Some(1), Some(10), Some("market"), None)
                .unwrap()
                .total,
            2
        );
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn today_summary_counts_range_and_returns_latest_record() {
        let (root, path) = test_database_path();
        let store = LearningRecordStore::open(&path).unwrap();
        let first = save(&store, "market", SourceType::Manual);
        let second = save(&store, "market share", SourceType::WindowsUia);
        store
            .connection
            .execute(
                "UPDATE learning_records SET created_at_unix_ms = ?1 WHERE id = ?2",
                params![100_i64, first.id],
            )
            .unwrap();
        store
            .connection
            .execute(
                "UPDATE learning_records SET created_at_unix_ms = ?1 WHERE id = ?2",
                params![200_i64, second.id],
            )
            .unwrap();

        let summary = store.summarize_range(50, 250).unwrap();
        assert_eq!(summary.record_count, 2);
        assert_eq!(summary.latest_record.unwrap().id, second.id);
        let narrower = store.summarize_range(50, 150).unwrap();
        assert_eq!(narrower.record_count, 1);
        assert_eq!(narrower.latest_record.unwrap().id, first.id);
        assert!(store.summarize_range(100, 100).is_err());

        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_explanation_card_is_not_saved() {
        let (root, path) = test_database_path();
        let store = LearningRecordStore::open(&path).unwrap();
        let input = input("market", SourceType::Manual);
        let mut card = word_card("market");
        if let ExplanationCard::Word { basic_meanings, .. } = &mut card {
            basic_meanings.clear();
        }

        let error = store.save(&input, &card).unwrap_err();
        assert!(error.contains("校验失败"));
        assert_eq!(store.list(Some(1), Some(10), None, None).unwrap().total, 0);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn database_errors_are_clear_and_do_not_panic() {
        let (root, path) = test_database_path();
        fs::create_dir_all(&root).unwrap();
        let blocked_parent = root.join("blocked-parent");
        fs::write(&blocked_parent, "not a directory").unwrap();

        let error = match LearningRecordStore::open(&blocked_parent.join(path.file_name().unwrap()))
        {
            Ok(_) => panic!("数据库父路径是普通文件时不应打开成功"),
            Err(error) => error,
        };
        assert!(error.contains("数据库目录无法创建"));
        let _ = fs::remove_dir_all(root);
    }
}
