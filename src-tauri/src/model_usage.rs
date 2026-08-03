use crate::learning_records::{open_database_for_app, unix_time_ms};
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::AppHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelUsageCategory {
    ExplanationQuery,
    QuickAi,
    Writing,
}

impl ModelUsageCategory {
    fn as_storage(self) -> &'static str {
        match self {
            Self::ExplanationQuery => "explanation_query",
            Self::QuickAi => "quick_ai",
            Self::Writing => "writing",
        }
    }

    fn from_storage(value: &str) -> Result<Self, String> {
        match value {
            "explanation_query" => Ok(Self::ExplanationQuery),
            "quick_ai" => Ok(Self::QuickAi),
            "writing" => Ok(Self::Writing),
            _ => Err(format!("模型使用量包含未知业务分类：{value}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ModelTokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageCategorySummary {
    category: ModelUsageCategory,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    request_count: u64,
}

impl ModelUsageCategorySummary {
    fn empty(category: ModelUsageCategory) -> Self {
        Self {
            category,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            request_count: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageSummary {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    request_count: u64,
    statistics_start_unix_ms: Option<i64>,
    categories: Vec<ModelUsageCategorySummary>,
}

pub(crate) fn record_for_app(
    app: &AppHandle,
    category: ModelUsageCategory,
    usage: ModelTokenUsage,
) -> Result<(), String> {
    let connection = open_database_for_app(app)?;
    record_at(&connection, category, usage, unix_time_ms()?)
}

fn record_at(
    connection: &Connection,
    category: ModelUsageCategory,
    usage: ModelTokenUsage,
    created_at_unix_ms: i64,
) -> Result<(), String> {
    let prompt_tokens = token_count_to_i64(usage.prompt_tokens)?;
    let completion_tokens = token_count_to_i64(usage.completion_tokens)?;
    let total_tokens = token_count_to_i64(usage.total_tokens)?;
    if created_at_unix_ms <= 0 {
        return Err("模型使用量写入时间无效。".to_string());
    }
    connection
        .execute(
            "INSERT INTO model_usage_records (
                category, prompt_tokens, completion_tokens, total_tokens, created_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                category.as_storage(),
                prompt_tokens,
                completion_tokens,
                total_tokens,
                created_at_unix_ms
            ],
        )
        .map_err(|error| format!("模型使用量写入失败：{error}"))?;
    Ok(())
}

fn token_count_to_i64(value: u64) -> Result<i64, String> {
    i64::try_from(value).map_err(|_| "模型使用量超过本地数据库可存储范围。".to_string())
}

fn non_negative_to_u64(value: i64, label: &str) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| format!("模型使用量聚合返回了无效的{label}。"))
}

fn summarize(
    connection: &Connection,
    start_unix_ms: Option<i64>,
    end_unix_ms: Option<i64>,
) -> Result<ModelUsageSummary, String> {
    match (start_unix_ms, end_unix_ms) {
        (Some(start), Some(end)) if start >= 0 && end > start => {}
        (None, None) => {}
        _ => return Err("模型使用量时间范围无效。".to_string()),
    }

    let (prompt_tokens, completion_tokens, total_tokens, request_count, first_recorded_at): (
        i64,
        i64,
        i64,
        i64,
        Option<i64>,
    ) = connection
        .query_row(
            "SELECT
                COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(completion_tokens), 0),
                COALESCE(SUM(total_tokens), 0),
                COUNT(*),
                MIN(created_at_unix_ms)
             FROM model_usage_records
             WHERE (?1 IS NULL OR created_at_unix_ms >= ?1)
               AND (?2 IS NULL OR created_at_unix_ms < ?2)",
            params![start_unix_ms, end_unix_ms],
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
        .map_err(|error| format!("模型使用量总览读取失败：{error}"))?;

    let mut categories = vec![
        ModelUsageCategorySummary::empty(ModelUsageCategory::ExplanationQuery),
        ModelUsageCategorySummary::empty(ModelUsageCategory::QuickAi),
        ModelUsageCategorySummary::empty(ModelUsageCategory::Writing),
    ];
    let mut statement = connection
        .prepare(
            "SELECT category,
                    COALESCE(SUM(prompt_tokens), 0),
                    COALESCE(SUM(completion_tokens), 0),
                    COALESCE(SUM(total_tokens), 0),
                    COUNT(*)
             FROM model_usage_records
             WHERE (?1 IS NULL OR created_at_unix_ms >= ?1)
               AND (?2 IS NULL OR created_at_unix_ms < ?2)
             GROUP BY category",
        )
        .map_err(|error| format!("模型使用量分类语句无法准备：{error}"))?;
    let rows = statement
        .query_map(params![start_unix_ms, end_unix_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|error| format!("模型使用量分类读取失败：{error}"))?;
    for row in rows {
        let (category, prompt, completion, total, requests) =
            row.map_err(|error| format!("模型使用量分类行读取失败：{error}"))?;
        let category = ModelUsageCategory::from_storage(&category)?;
        let target = categories
            .iter_mut()
            .find(|item| item.category == category)
            .ok_or_else(|| "模型使用量分类映射失败。".to_string())?;
        *target = ModelUsageCategorySummary {
            category,
            prompt_tokens: non_negative_to_u64(prompt, "输入 Token")?,
            completion_tokens: non_negative_to_u64(completion, "输出 Token")?,
            total_tokens: non_negative_to_u64(total, "总 Token")?,
            request_count: non_negative_to_u64(requests, "请求次数")?,
        };
    }

    Ok(ModelUsageSummary {
        prompt_tokens: non_negative_to_u64(prompt_tokens, "输入 Token")?,
        completion_tokens: non_negative_to_u64(completion_tokens, "输出 Token")?,
        total_tokens: non_negative_to_u64(total_tokens, "总 Token")?,
        request_count: non_negative_to_u64(request_count, "请求次数")?,
        statistics_start_unix_ms: first_recorded_at,
        categories,
    })
}

#[tauri::command]
pub fn get_model_usage_summary(
    app: AppHandle,
    start_unix_ms: Option<i64>,
    end_unix_ms: Option<i64>,
) -> Result<ModelUsageSummary, String> {
    summarize(&open_database_for_app(&app)?, start_unix_ms, end_unix_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning_records::open_database;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_database() -> (PathBuf, PathBuf, Connection) {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "readray-model-usage-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("usage.sqlite3");
        let connection = open_database(&path).unwrap();
        (root, path, connection)
    }

    #[test]
    fn aggregates_three_categories_and_respects_half_open_time_range() {
        let (root, _path, connection) = test_database();
        record_at(
            &connection,
            ModelUsageCategory::ExplanationQuery,
            ModelTokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
            1_000,
        )
        .unwrap();
        record_at(
            &connection,
            ModelUsageCategory::QuickAi,
            ModelTokenUsage {
                prompt_tokens: 20,
                completion_tokens: 8,
                total_tokens: 28,
            },
            2_000,
        )
        .unwrap();
        record_at(
            &connection,
            ModelUsageCategory::Writing,
            ModelTokenUsage {
                prompt_tokens: 30,
                completion_tokens: 12,
                total_tokens: 42,
            },
            3_000,
        )
        .unwrap();

        let selected = summarize(&connection, Some(500), Some(3_000)).unwrap();
        assert_eq!(selected.prompt_tokens, 30);
        assert_eq!(selected.completion_tokens, 13);
        assert_eq!(selected.total_tokens, 43);
        assert_eq!(selected.request_count, 2);
        assert_eq!(selected.statistics_start_unix_ms, Some(1_000));
        assert_eq!(selected.categories[0].total_tokens, 15);
        assert_eq!(selected.categories[1].total_tokens, 28);
        assert_eq!(selected.categories[2].total_tokens, 0);

        let all = summarize(&connection, None, None).unwrap();
        assert_eq!(all.total_tokens, 85);
        assert_eq!(all.request_count, 3);
        assert_eq!(all.statistics_start_unix_ms, Some(1_000));
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_range_has_no_statistics_start_time() {
        let (root, _path, connection) = test_database();
        record_at(
            &connection,
            ModelUsageCategory::QuickAi,
            ModelTokenUsage {
                prompt_tokens: 4,
                completion_tokens: 1,
                total_tokens: 5,
            },
            2_000,
        )
        .unwrap();

        let empty = summarize(&connection, Some(3_000), Some(4_000)).unwrap();
        assert_eq!(empty.request_count, 0);
        assert_eq!(empty.statistics_start_unix_ms, None);
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_partial_or_reversed_ranges() {
        let (root, _path, connection) = test_database();
        assert!(summarize(&connection, Some(1_000), None).is_err());
        assert!(summarize(&connection, Some(2_000), Some(1_000)).is_err());
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }
}
