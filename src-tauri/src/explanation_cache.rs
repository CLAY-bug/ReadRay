use crate::explanation::{
    classify_query_type, determine_query_direction, normalize_english_learning_target,
    normalize_model_english_learning_target, normalize_source_sentence_translation,
    validate_explanation_card, CaptureInput, ExplanationCard, QueryDirection, QueryType,
    MAX_CONTEXT_TEXT_LEN,
};
use crate::learning_records::{
    database_path_for_app, open_database, unix_time_ms, EXPLANATION_CARD_SCHEMA_VERSION,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

pub(crate) const EXPLANATION_CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub(crate) const EXPLANATION_CACHE_CAPACITY: i64 = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExplanationCacheIdentity {
    normalized_source_text: String,
    query_direction: QueryDirection,
    query_type: QueryType,
    minimal_context_fingerprint: String,
    model_id: String,
    model_revision: &'static str,
    prompt_version: &'static str,
    schema_version: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct ExplanationCacheSpec {
    pub(crate) identity: ExplanationCacheIdentity,
    pub(crate) cache_key: String,
    pub(crate) minimal_context_text: Option<String>,
}

impl ExplanationCacheSpec {
    pub(crate) fn new(
        input: &CaptureInput,
        minimal_context_text: Option<&str>,
        model_id: String,
        model_revision: &'static str,
        prompt_version: &'static str,
    ) -> Result<Self, String> {
        let normalized_source_text = normalize_source_text(&input.query_text);
        let query_type = classify_query_type(&input.query_text)?;
        let query_direction = determine_query_direction(&input.query_text)?;
        let minimal_context_text = normalize_minimal_context_text(minimal_context_text)?;
        let minimal_context_fingerprint =
            canonical_context_fingerprint(minimal_context_text.as_deref())?;
        let identity = ExplanationCacheIdentity {
            normalized_source_text: normalized_source_text.clone(),
            query_direction,
            query_type,
            minimal_context_fingerprint,
            model_id,
            model_revision,
            prompt_version,
            schema_version: EXPLANATION_CARD_SCHEMA_VERSION,
        };
        let cache_key = serde_json::to_string(&identity)
            .map_err(|error| format!("ExplanationCard cache identity 无法序列化：{error}"))?;

        Ok(Self {
            identity,
            cache_key,
            minimal_context_text,
        })
    }

    pub(crate) fn query_type(&self) -> QueryType {
        self.identity.query_type
    }

    pub(crate) fn query_direction(&self) -> QueryDirection {
        self.identity.query_direction
    }
}

fn normalize_source_text(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn normalize_minimal_context_text(value: Option<&str>) -> Result<Option<String>, String> {
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
    Ok(Some(value.to_string()))
}

fn canonical_context_fingerprint(value: Option<&str>) -> Result<String, String> {
    serde_json::to_string(&value)
        .map_err(|error| format!("minimalContext fingerprint 无法序列化：{error}"))
}

pub(crate) fn rebind_and_validate_card(
    input: &CaptureInput,
    spec: &ExplanationCacheSpec,
    mut card: ExplanationCard,
) -> Result<ExplanationCard, String> {
    let current_type = classify_query_type(&input.query_text)?;
    let current_direction = determine_query_direction(&input.query_text)?;
    if current_type != spec.query_type() || current_direction != spec.query_direction() {
        return Err("ExplanationCard cache identity 与当前请求方向或类型不一致。".to_string());
    }
    if card.query_type() != current_type {
        return Err("ExplanationCard cache 的 queryType 与当前请求不一致。".to_string());
    }

    card.set_source_text(input.query_text.trim().to_string());
    match current_direction {
        QueryDirection::EnToZh => {
            card.set_learning_target_text(normalize_english_learning_target(&input.query_text)?);
        }
        QueryDirection::ZhToEn => {
            card.set_learning_target_text(normalize_model_english_learning_target(
                card.learning_target_text(),
            ));
            card.align_primary_result_with_learning_target();
        }
    }
    normalize_source_sentence_translation(&mut card);
    validate_explanation_card(input, &card)
        .map_err(|errors| format!("ExplanationCard cache 重新校验失败：{}", errors.join("；")))?;
    Ok(card)
}

enum CacheReadOutcome {
    Hit {
        card: ExplanationCard,
        token: CacheRowToken,
    },
    Miss,
    Remove(CacheRowToken),
}

#[derive(Clone)]
struct CacheRowToken {
    created_at_unix_ms: i64,
    explanation_card_json: String,
}

struct StoredCacheEntry {
    normalized_source_text: String,
    query_direction: String,
    query_type: String,
    minimal_context_fingerprint: String,
    model_id: String,
    model_revision: String,
    prompt_version: String,
    schema_version: i64,
    explanation_card_json: String,
    created_at_unix_ms: i64,
}

struct ExplanationCacheStore {
    connection: Connection,
}

impl ExplanationCacheStore {
    fn open(path: &Path) -> Result<Self, String> {
        Ok(Self {
            connection: open_database(path)?,
        })
    }

    fn read(
        &self,
        spec: &ExplanationCacheSpec,
        input: &CaptureInput,
        now_unix_ms: i64,
    ) -> Result<CacheReadOutcome, String> {
        let stored = self
            .connection
            .query_row(
                "SELECT normalized_source_text, query_direction, query_type,
                        minimal_context_fingerprint, model_id, model_revision,
                        prompt_version, schema_version, explanation_card_json,
                        created_at_unix_ms
                 FROM explanation_card_cache WHERE cache_key = ?1",
                [&spec.cache_key],
                |row| {
                    Ok(StoredCacheEntry {
                        normalized_source_text: row.get(0)?,
                        query_direction: row.get(1)?,
                        query_type: row.get(2)?,
                        minimal_context_fingerprint: row.get(3)?,
                        model_id: row.get(4)?,
                        model_revision: row.get(5)?,
                        prompt_version: row.get(6)?,
                        schema_version: row.get(7)?,
                        explanation_card_json: row.get(8)?,
                        created_at_unix_ms: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("ExplanationCard cache 读取失败：{error}"))?;
        let Some(stored) = stored else {
            return Ok(CacheReadOutcome::Miss);
        };

        if !stored.matches(&spec.identity) || is_expired(stored.created_at_unix_ms, now_unix_ms) {
            return Ok(CacheReadOutcome::Remove(stored.token()));
        }
        let card: ExplanationCard = match serde_json::from_str(&stored.explanation_card_json) {
            Ok(card) => card,
            Err(_) => return Ok(CacheReadOutcome::Remove(stored.token())),
        };
        match rebind_and_validate_card(input, spec, card) {
            Ok(card) => Ok(CacheReadOutcome::Hit {
                card,
                token: stored.token(),
            }),
            Err(_) => Ok(CacheReadOutcome::Remove(stored.token())),
        }
    }

    fn upsert(
        &self,
        spec: &ExplanationCacheSpec,
        input: &CaptureInput,
        card: &ExplanationCard,
        now_unix_ms: i64,
    ) -> Result<(), String> {
        let validated_card = rebind_and_validate_card(input, spec, card.clone())?;
        let card_json = serde_json::to_string(&validated_card)
            .map_err(|error| format!("ExplanationCard cache 无法序列化：{error}"))?;
        self.connection
            .execute(
                "INSERT INTO explanation_card_cache (
                    cache_key, normalized_source_text, query_direction, query_type,
                    minimal_context_fingerprint, model_id, model_revision, prompt_version,
                    schema_version, explanation_card_json, created_at_unix_ms,
                    last_accessed_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
                 ON CONFLICT(cache_key) DO UPDATE SET
                    normalized_source_text = excluded.normalized_source_text,
                    query_direction = excluded.query_direction,
                    query_type = excluded.query_type,
                    minimal_context_fingerprint = excluded.minimal_context_fingerprint,
                    model_id = excluded.model_id,
                    model_revision = excluded.model_revision,
                    prompt_version = excluded.prompt_version,
                    schema_version = excluded.schema_version,
                    explanation_card_json = excluded.explanation_card_json,
                    created_at_unix_ms = excluded.created_at_unix_ms,
                    last_accessed_at_unix_ms = excluded.last_accessed_at_unix_ms",
                params![
                    spec.cache_key,
                    spec.identity.normalized_source_text,
                    query_direction_to_storage(spec.identity.query_direction),
                    query_type_to_storage(spec.identity.query_type),
                    spec.identity.minimal_context_fingerprint,
                    spec.identity.model_id,
                    spec.identity.model_revision,
                    spec.identity.prompt_version,
                    spec.identity.schema_version,
                    card_json,
                    now_unix_ms,
                ],
            )
            .map_err(|error| format!("ExplanationCard cache 写入失败：{error}"))?;
        Ok(())
    }

    fn touch(
        &self,
        cache_key: &str,
        token: &CacheRowToken,
        now_unix_ms: i64,
    ) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE explanation_card_cache
                 SET last_accessed_at_unix_ms = MAX(last_accessed_at_unix_ms, ?4)
                 WHERE cache_key = ?1 AND created_at_unix_ms = ?2
                   AND explanation_card_json = ?3",
                params![
                    cache_key,
                    token.created_at_unix_ms,
                    token.explanation_card_json,
                    now_unix_ms,
                ],
            )
            .map_err(|error| format!("ExplanationCard cache 访问时间维护失败：{error}"))?;
        Ok(())
    }

    fn remove(&self, cache_key: &str, token: &CacheRowToken) -> Result<(), String> {
        self.connection
            .execute(
                "DELETE FROM explanation_card_cache
                 WHERE cache_key = ?1 AND created_at_unix_ms = ?2
                   AND explanation_card_json = ?3",
                params![
                    cache_key,
                    token.created_at_unix_ms,
                    token.explanation_card_json,
                ],
            )
            .map_err(|error| format!("ExplanationCard cache 损坏项删除失败：{error}"))?;
        Ok(())
    }

    fn maintain(&self, now_unix_ms: i64) -> Result<(), String> {
        let cutoff = now_unix_ms.saturating_sub(cache_ttl_ms());
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| format!("ExplanationCard cache 维护事务无法开始：{error}"))?;
        transaction
            .execute(
                "DELETE FROM explanation_card_cache WHERE created_at_unix_ms <= ?1",
                [cutoff],
            )
            .map_err(|error| format!("ExplanationCard cache 过期清理失败：{error}"))?;
        transaction
            .execute(
                "DELETE FROM explanation_card_cache
                 WHERE cache_key IN (
                   SELECT cache_key FROM explanation_card_cache
                   ORDER BY last_accessed_at_unix_ms DESC,
                            created_at_unix_ms DESC,
                            cache_key DESC
                   LIMIT -1 OFFSET ?1
                 )",
                [EXPLANATION_CACHE_CAPACITY],
            )
            .map_err(|error| format!("ExplanationCard cache 容量清理失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("ExplanationCard cache 维护事务无法提交：{error}"))?;
        Ok(())
    }
}

impl StoredCacheEntry {
    fn token(&self) -> CacheRowToken {
        CacheRowToken {
            created_at_unix_ms: self.created_at_unix_ms,
            explanation_card_json: self.explanation_card_json.clone(),
        }
    }

    fn matches(&self, identity: &ExplanationCacheIdentity) -> bool {
        self.normalized_source_text == identity.normalized_source_text
            && self.query_direction == query_direction_to_storage(identity.query_direction)
            && self.query_type == query_type_to_storage(identity.query_type)
            && self.minimal_context_fingerprint == identity.minimal_context_fingerprint
            && self.model_id == identity.model_id
            && self.model_revision == identity.model_revision
            && self.prompt_version == identity.prompt_version
            && self.schema_version == identity.schema_version
    }
}

fn is_expired(created_at_unix_ms: i64, now_unix_ms: i64) -> bool {
    created_at_unix_ms <= now_unix_ms.saturating_sub(cache_ttl_ms())
}

fn cache_ttl_ms() -> i64 {
    i64::try_from(EXPLANATION_CACHE_TTL.as_millis()).expect("7 day TTL fits i64")
}

fn query_direction_to_storage(direction: QueryDirection) -> &'static str {
    match direction {
        QueryDirection::EnToZh => "en_to_zh",
        QueryDirection::ZhToEn => "zh_to_en",
    }
}

fn query_type_to_storage(query_type: QueryType) -> &'static str {
    match query_type {
        QueryType::Word => "word",
        QueryType::Phrase => "phrase",
        QueryType::Sentence => "sentence",
        QueryType::Paragraph => "paragraph",
    }
}

pub(crate) async fn lookup_for_app(
    app: &AppHandle,
    spec: &ExplanationCacheSpec,
    input: &CaptureInput,
) -> Option<ExplanationCard> {
    let path = match database_path_for_app(app) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("READRAY_EXPLANATION_CACHE_READ_FAILED: {error}");
            return None;
        }
    };
    let read_path = path.clone();
    let read_spec = spec.clone();
    let read_input = input.clone();
    let now_unix_ms = match unix_time_ms() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("READRAY_EXPLANATION_CACHE_READ_FAILED: {error}");
            return None;
        }
    };
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        ExplanationCacheStore::open(&read_path)?.read(&read_spec, &read_input, now_unix_ms)
    })
    .await;

    match outcome {
        Ok(Ok(CacheReadOutcome::Hit { card, token })) => {
            schedule_touch(path, spec.cache_key.clone(), token, now_unix_ms);
            Some(card)
        }
        Ok(Ok(CacheReadOutcome::Remove(token))) => {
            schedule_remove(path, spec.cache_key.clone(), token);
            None
        }
        Ok(Ok(CacheReadOutcome::Miss)) => None,
        Ok(Err(error)) => {
            eprintln!("READRAY_EXPLANATION_CACHE_READ_FAILED: {error}");
            None
        }
        Err(error) => {
            eprintln!("READRAY_EXPLANATION_CACHE_READ_TASK_FAILED: {error}");
            None
        }
    }
}

pub(crate) fn upsert_for_app_fail_open(
    app: &AppHandle,
    spec: &ExplanationCacheSpec,
    input: &CaptureInput,
    card: &ExplanationCard,
) {
    let result = database_path_for_app(app).and_then(|path| {
        let now_unix_ms = unix_time_ms()?;
        ExplanationCacheStore::open(&path)?.upsert(spec, input, card, now_unix_ms)
    });
    if let Err(error) = result {
        eprintln!("READRAY_EXPLANATION_CACHE_WRITE_FAILED: {error}");
    }
}

pub(crate) fn schedule_maintenance_for_app(app: &AppHandle) {
    let path = match database_path_for_app(app) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("READRAY_EXPLANATION_CACHE_MAINTENANCE_FAILED: {error}");
            return;
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        let result =
            unix_time_ms().and_then(|now| ExplanationCacheStore::open(&path)?.maintain(now));
        if let Err(error) = result {
            eprintln!("READRAY_EXPLANATION_CACHE_MAINTENANCE_FAILED: {error}");
        }
    });
}

fn schedule_touch(path: PathBuf, cache_key: String, token: CacheRowToken, now_unix_ms: i64) {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = ExplanationCacheStore::open(&path)
            .and_then(|store| store.touch(&cache_key, &token, now_unix_ms))
        {
            eprintln!("READRAY_EXPLANATION_CACHE_TOUCH_FAILED: {error}");
        }
    });
}

fn schedule_remove(path: PathBuf, cache_key: String, token: CacheRowToken) {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) =
            ExplanationCacheStore::open(&path).and_then(|store| store.remove(&cache_key, &token))
        {
            eprintln!("READRAY_EXPLANATION_CACHE_REMOVE_FAILED: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explanation::{ExampleItem, SourceType};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    const MODEL_REVISION: &str = "model-revision-test";
    const PROMPT_VERSION: &str = "prompt-version-test";
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "readray-explanation-cache-{}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        (root.clone(), root.join("readray.sqlite3"))
    }

    fn input(query_text: &str, context: Option<&str>) -> CaptureInput {
        CaptureInput {
            query_text: query_text.to_string(),
            context_text: context.map(str::to_string),
            source_type: SourceType::WindowsUia,
            source_app: Some("Obsidian.exe".to_string()),
        }
    }

    fn card(query_text: &str) -> ExplanationCard {
        ExplanationCard::Word {
            source_text: query_text.to_string(),
            learning_target_text: query_text.to_string(),
            headword: query_text.to_string(),
            part_of_speech: None,
            phonetic: None,
            basic_meanings: vec!["市场".to_string()],
            context_meaning: None,
            source_sentence: None,
            source_sentence_zh: None,
            phrases: Vec::new(),
            near_meanings: Vec::new(),
            examples: vec![ExampleItem {
                en: "The market is open.".to_string(),
                zh: "市场开放。".to_string(),
            }],
            review_hint: None,
        }
    }

    fn spec(input: &CaptureInput, context: Option<&str>) -> ExplanationCacheSpec {
        ExplanationCacheSpec::new(
            input,
            context,
            "deepseek-test".to_string(),
            MODEL_REVISION,
            PROMPT_VERSION,
        )
        .unwrap()
    }

    #[test]
    fn cache_miss_hit_and_restart_persistence() {
        let (root, path) = test_path();
        let input = input("market", None);
        let spec = spec(&input, None);
        let store = ExplanationCacheStore::open(&path).unwrap();
        assert!(matches!(
            store.read(&spec, &input, 1_000).unwrap(),
            CacheReadOutcome::Miss
        ));
        store.upsert(&spec, &input, &card("market"), 1_000).unwrap();
        assert!(matches!(
            store.read(&spec, &input, 1_001).unwrap(),
            CacheReadOutcome::Hit { .. }
        ));
        drop(store);

        let reopened = ExplanationCacheStore::open(&path).unwrap();
        assert!(matches!(
            reopened.read(&spec, &input, 1_002).unwrap(),
            CacheReadOutcome::Hit { .. }
        ));
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn context_and_version_fields_isolate_cache_identity() {
        let base_input = input("market", Some("The market remained open."));
        let base = spec(&base_input, Some("The market remained open."));
        let other_context = spec(&base_input, Some("They marketed the new product."));
        assert_ne!(base.cache_key, other_context.cache_key);

        for changed in [
            ExplanationCacheSpec::new(
                &base_input,
                Some("The market remained open."),
                "deepseek-other".to_string(),
                MODEL_REVISION,
                PROMPT_VERSION,
            )
            .unwrap(),
            ExplanationCacheSpec::new(
                &base_input,
                Some("The market remained open."),
                "deepseek-test".to_string(),
                "model-revision-next",
                PROMPT_VERSION,
            )
            .unwrap(),
            ExplanationCacheSpec::new(
                &base_input,
                Some("The market remained open."),
                "deepseek-test".to_string(),
                MODEL_REVISION,
                "prompt-version-next",
            )
            .unwrap(),
        ] {
            assert_ne!(base.cache_key, changed.cache_key);
        }
    }

    #[test]
    fn source_app_and_source_type_do_not_change_cache_identity() {
        let first = input("  Market  ", None);
        let mut second = input("Market", None);
        second.source_app = Some("Code.exe".to_string());
        second.source_type = SourceType::Manual;
        assert_eq!(spec(&first, None).cache_key, spec(&second, None).cache_key);
    }

    #[test]
    fn normalized_identity_preserves_newlines_without_changing_raw_query_type() {
        let paragraph = input("First line without punctuation\nSecond line", None);
        let paragraph_spec = spec(&paragraph, None);
        assert_eq!(paragraph_spec.query_type(), QueryType::Paragraph);
        assert!(paragraph_spec
            .identity
            .normalized_source_text
            .contains('\n'));

        let single_line = input("First line without punctuation Second line", None);
        let single_line_spec = spec(&single_line, None);
        assert_ne!(paragraph_spec.cache_key, single_line_spec.cache_key);
        assert_ne!(paragraph_spec.query_type(), single_line_spec.query_type());
    }

    #[test]
    fn expired_and_damaged_entries_are_reported_for_background_removal() {
        let (root, path) = test_path();
        let input = input("market", None);
        let spec = spec(&input, None);
        let store = ExplanationCacheStore::open(&path).unwrap();
        store.upsert(&spec, &input, &card("market"), 1_000).unwrap();
        assert!(matches!(
            store.read(&spec, &input, 1_000 + cache_ttl_ms()).unwrap(),
            CacheReadOutcome::Remove(_)
        ));
        store
            .connection
            .execute(
                "UPDATE explanation_card_cache SET explanation_card_json = '{broken'
                 WHERE cache_key = ?1",
                [&spec.cache_key],
            )
            .unwrap();
        assert!(matches!(
            store.read(&spec, &input, 1_001).unwrap(),
            CacheReadOutcome::Remove(_)
        ));
        let token = match store.read(&spec, &input, 1_001).unwrap() {
            CacheReadOutcome::Remove(token) => token,
            _ => panic!("damaged entry must request removal"),
        };
        store.remove(&spec.cache_key, &token).unwrap();
        assert!(matches!(
            store.read(&spec, &input, 1_002).unwrap(),
            CacheReadOutcome::Miss
        ));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_or_failed_provider_result_is_not_cached() {
        let (root, path) = test_path();
        let input = input("market", None);
        let spec = spec(&input, None);
        let store = ExplanationCacheStore::open(&path).unwrap();
        let mut invalid = card("market");
        if let ExplanationCard::Word { basic_meanings, .. } = &mut invalid {
            basic_meanings.clear();
        }
        assert!(store.upsert(&spec, &input, &invalid, 1_000).is_err());
        let count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM explanation_card_cache", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_remove_and_touch_cannot_mutate_a_newer_upsert() {
        let (root, path) = test_path();
        let input = input("market", None);
        let spec = spec(&input, None);
        let store = ExplanationCacheStore::open(&path).unwrap();
        store.upsert(&spec, &input, &card("market"), 1_000).unwrap();

        store
            .connection
            .execute(
                "UPDATE explanation_card_cache SET explanation_card_json = '{broken'
                 WHERE cache_key = ?1",
                [&spec.cache_key],
            )
            .unwrap();
        let stale_remove = match store.read(&spec, &input, 1_001).unwrap() {
            CacheReadOutcome::Remove(token) => token,
            _ => panic!("damaged entry must request removal"),
        };
        store.upsert(&spec, &input, &card("market"), 2_000).unwrap();
        store.remove(&spec.cache_key, &stale_remove).unwrap();
        assert!(matches!(
            store.read(&spec, &input, 2_001).unwrap(),
            CacheReadOutcome::Hit { .. }
        ));

        let stale_touch = match store.read(&spec, &input, 2_001).unwrap() {
            CacheReadOutcome::Hit { token, .. } => token,
            _ => panic!("healthy entry must hit"),
        };
        store.upsert(&spec, &input, &card("market"), 3_000).unwrap();
        store.touch(&spec.cache_key, &stale_touch, 2_500).unwrap();
        let last_accessed: i64 = store
            .connection
            .query_row(
                "SELECT last_accessed_at_unix_ms FROM explanation_card_cache
                 WHERE cache_key = ?1",
                [&spec.cache_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_accessed, 3_000);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn maintenance_converges_to_ttl_and_deterministic_capacity() {
        let (root, path) = test_path();
        let store = ExplanationCacheStore::open(&path).unwrap();
        let expired_input = input("expired", None);
        let expired_spec = spec(&expired_input, None);
        store
            .upsert(&expired_spec, &expired_input, &card("expired"), 1_000)
            .unwrap();

        let now = 1_000 + cache_ttl_ms() + 10;
        for index in 0..=EXPLANATION_CACHE_CAPACITY {
            let query = format!("term{index}");
            let input = input(&query, None);
            let spec = spec(&input, None);
            store
                .upsert(&spec, &input, &card(&query), now + index)
                .unwrap();
        }
        let before: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM explanation_card_cache", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(before > EXPLANATION_CACHE_CAPACITY);

        store
            .maintain(now + EXPLANATION_CACHE_CAPACITY + 1)
            .unwrap();
        let after: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM explanation_card_cache", [], |row| {
                row.get(0)
            })
            .unwrap();
        let expired_count: i64 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM explanation_card_cache WHERE cache_key = ?1",
                [&expired_spec.cache_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after, EXPLANATION_CACHE_CAPACITY);
        assert_eq!(expired_count, 0);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_hit_rebinds_current_source_and_learning_target_then_revalidates() {
        let cached_input = input("Market", None);
        let current_input = input("  Market  ", None);
        let spec = spec(&current_input, None);
        let rebound = rebind_and_validate_card(&current_input, &spec, card("Market")).unwrap();
        assert_eq!(rebound.source_text(), "Market");
        assert_eq!(rebound.learning_target_text(), "Market");
        validate_explanation_card(&current_input, &rebound).unwrap();
        let _ = cached_input;
    }
}
