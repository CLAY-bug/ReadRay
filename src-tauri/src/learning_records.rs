use crate::explanation::{
    validate_explanation_card, CaptureInput, ExplanationCard, QueryType, SourceType,
};
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const DATABASE_FILE_NAME: &str = "readray.sqlite3";
const DATABASE_SCHEMA_VERSION: i64 = 1;
pub const EXPLANATION_CARD_SCHEMA_VERSION: i64 = 1;
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

const MIGRATIONS: &[(i64, &str)] = &[(DATABASE_SCHEMA_VERSION, MIGRATION_1)];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningRecord {
    pub id: i64,
    pub query_text: String,
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

struct StoredLearningRecord {
    id: i64,
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
}

struct LearningRecordStore {
    connection: Connection,
}

impl LearningRecordStore {
    fn open(path: &Path) -> Result<Self, String> {
        let parent = path
            .parent()
            .ok_or_else(|| format!("学习记录数据库路径缺少父目录：{}", path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("学习记录数据库目录无法创建：{error}"))?;

        let mut connection =
            Connection::open(path).map_err(|error| format!("学习记录数据库无法打开：{error}"))?;
        migrate(&mut connection)?;

        Ok(Self { connection })
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

        self.connection
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

        self.get(self.connection.last_insert_rowid())?
            .ok_or_else(|| "学习记录写入后无法读取新记录。".to_string())
    }

    fn get(&self, id: i64) -> Result<Option<LearningRecord>, String> {
        let mut statement = self
            .connection
            .prepare(&select_learning_record_sql("WHERE id = ?1"))
            .map_err(|error| format!("学习记录读取语句无法准备：{error}"))?;
        let stored = statement
            .query_row([id], read_stored_learning_record)
            .optional()
            .map_err(|error| format!("学习记录读取失败：{error}"))?;

        stored.map(decode_learning_record).transpose()
    }

    fn delete(&self, id: i64) -> Result<bool, String> {
        let affected = self
            .connection
            .execute("DELETE FROM learning_records WHERE id = ?1", [id])
            .map_err(|error| format!("学习记录删除失败：{error}"))?;

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
        let count_sql = format!("SELECT COUNT(*) FROM learning_records {where_clause}");
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
            "{} {where_clause} ORDER BY created_at_unix_ms DESC, id DESC LIMIT ? OFFSET ?",
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
}

pub fn initialize_for_app(app: &AppHandle) -> Result<(), String> {
    LearningRecordStore::open(&database_path_for_app(app)?).map(|_| ())
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
pub fn delete_learning_record(app: AppHandle, id: i64) -> Result<bool, String> {
    LearningRecordStore::open(&database_path_for_app(&app)?)?.delete(id)
}

fn database_path_for_app(app: &AppHandle) -> Result<PathBuf, String> {
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
        clauses.push("instr(normalized_text, ?) > 0".to_string());
        values.push(Value::Text(normalized_keyword));
    }
    if let Some(query_type) = query_type {
        clauses.push("query_type = ?".to_string());
        values.push(Value::Text(query_type_to_storage(query_type).to_string()));
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };

    Ok((where_clause, values))
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
        "SELECT id, query_text, normalized_text, query_type, source_type, source_app, context_text, \
         explanation_card_json, schema_version, created_at_unix_ms, difficulty \
         FROM learning_records {where_clause}"
    )
}

fn read_stored_learning_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredLearningRecord> {
    Ok(StoredLearningRecord {
        id: row.get(0)?,
        query_text: row.get(1)?,
        normalized_text: row.get(2)?,
        query_type: row.get(3)?,
        source_type: row.get(4)?,
        source_app: row.get(5)?,
        context_text: row.get(6)?,
        explanation_card_json: row.get(7)?,
        schema_version: row.get(8)?,
        created_at_unix_ms: row.get(9)?,
        difficulty: row.get(10)?,
    })
}

fn decode_learning_record(stored: StoredLearningRecord) -> Result<LearningRecord, String> {
    let query_type = query_type_from_storage(&stored.query_type)?;
    let source_type = source_type_from_storage(&stored.source_type)?;
    let explanation_card: ExplanationCard = serde_json::from_str(&stored.explanation_card_json)
        .map_err(|error| {
            format!(
                "学习记录 {} 的 ExplanationCard JSON 无法解析：{error}",
                stored.id
            )
        })?;

    if explanation_card.query_type() != query_type {
        return Err(format!(
            "学习记录 {} 的 queryType 与 ExplanationCard JSON 不一致。",
            stored.id
        ));
    }

    Ok(LearningRecord {
        id: stored.id,
        query_text: stored.query_text,
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

fn unix_time_ms() -> Result<i64, String> {
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
        let store = LearningRecordStore::open(&path).unwrap();
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
