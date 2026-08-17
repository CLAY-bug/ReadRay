//! ChatSurfaceAdapter：主应用完整对话与 Quick AI overlay 共享的会话 surface。
//!
//! 复用 `prepare_turn/complete_turn` 幂等边界（user 先落库、assistant 恰好补一
//! 条、pending 可重试），把 conversation 身份投影为 run 输入。恢复语义（§17）：
//! 最终 assistant 已落库时 prepare 返回 Completed 并对账 run 终态；只有 pending
//! user 时创建（retry_of 上一个未终态 run 的）新 run。任务 2 只提供 L0 可信本地
//! 只读工具，不开放 web_search/fetch（任务 3）。

use crate::agent_runtime::context::{ContextAssembler, RuntimeFacts};
use crate::agent_runtime::gateway::ProviderMessage;
use crate::agent_runtime::protocol::{AgentSurface, AuthorityRef, ToolProvenance, ToolResult};
use crate::agent_runtime::run_repository::{AgentRunRepository, AgentRunStatus};
use crate::agent_runtime::tool::{RiskLevel, ToolDefinition, ToolRegistry, ToolSchema};
use crate::conversations::{
    ConversationRole, ConversationSnapshot, ConversationStore, PreparedTurn,
};
use crate::learning_records::unix_time_ms;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

/// 与既有 Quick AI 上下文窗口一致（quick_ai.rs 私有常量）。
const MAX_CONTEXT_MESSAGES: usize = 40;

pub(crate) enum ChatPreparedTurn {
    Pending {
        snapshot: ConversationSnapshot,
        user_message_id: i64,
    },
    Completed {
        snapshot: ConversationSnapshot,
    },
}

pub(crate) struct ChatSurfaceAdapter {
    surface: AgentSurface,
    store: ConversationStore,
}

impl ChatSurfaceAdapter {
    pub(crate) fn open(surface: AgentSurface, store: ConversationStore) -> Result<Self, String> {
        match surface {
            AgentSurface::MainConversation | AgentSurface::QuickAiOverlay => {}
            AgentSurface::WritingCoach => {
                return Err(
                    "ChatSurfaceAdapter 只接受通用对话 surface；Writing Coach 属任务 6。"
                        .to_string(),
                )
            }
        }
        Ok(Self { surface, store })
    }

    pub(crate) fn authority_ref(
        &self,
        conversation_id: i64,
        expected_user_sequence: i64,
        user_message_id: i64,
    ) -> Result<AuthorityRef, String> {
        let authority = AuthorityRef::conversation(
            self.surface,
            conversation_id,
            expected_user_sequence,
            user_message_id,
        );
        authority.validate()?;
        Ok(authority)
    }

    /// 幂等准备轮次：user 先落库；已完成轮次直接返回权威快照并对账 run 终态。
    pub(crate) fn prepare(
        &mut self,
        conversation_id: i64,
        expected_user_sequence: i64,
        user_content: &str,
        repository: &mut AgentRunRepository,
    ) -> Result<ChatPreparedTurn, String> {
        match self
            .store
            .prepare_turn(conversation_id, expected_user_sequence, user_content)?
        {
            PreparedTurn::Completed { snapshot } => {
                self.reconcile_completed_run(repository, conversation_id, expected_user_sequence)?;
                Ok(ChatPreparedTurn::Completed { snapshot })
            }
            PreparedTurn::Pending {
                snapshot,
                user_message_id,
            } => Ok(ChatPreparedTurn::Pending {
                snapshot,
                user_message_id,
            }),
        }
    }

    /// 幂等完成轮次：只补一条 assistant，已完成轮次返回权威快照。
    pub(crate) fn complete(
        &mut self,
        conversation_id: i64,
        expected_user_sequence: i64,
        user_message_id: i64,
        assistant_content: &str,
    ) -> Result<ConversationSnapshot, String> {
        self.store.complete_turn(
            conversation_id,
            expected_user_sequence,
            user_message_id,
            assistant_content,
        )
    }

    /// 对账（§17 重启恢复）：assistant 已落库但 run 终态未写（模糊成功），
    /// 把该轮最近 run 同步为 completed；completed 保持不变。
    fn reconcile_completed_run(
        &self,
        repository: &mut AgentRunRepository,
        conversation_id: i64,
        expected_user_sequence: i64,
    ) -> Result<(), String> {
        let Some(latest) =
            repository.latest_run_for_turn(conversation_id, expected_user_sequence)?
        else {
            return Ok(());
        };
        if latest.status == AgentRunStatus::Completed {
            return Ok(());
        }
        if latest.status.is_terminal() {
            // stopped/failed/truncated 与已落库 assistant 不一致：保留原终态，
            // 由调用方直接返回权威快照，不伪造 run 生命周期。
            return Ok(());
        }
        repository.transition(
            &latest.run_id,
            AgentRunStatus::Completed,
            None,
            unix_time_ms()?,
        )
    }

    /// 会话快照 → provider 消息投影（system + 最近完整尾部，从 user 开始）。
    pub(crate) fn transcript(
        &self,
        snapshot: &ConversationSnapshot,
        facts: &RuntimeFacts,
        active_tools: &[ToolSchema],
    ) -> Vec<ProviderMessage> {
        let mut messages = vec![ProviderMessage::System {
            content: ContextAssembler::default().system_prompt(facts, active_tools),
        }];
        let history = &snapshot.messages;
        let mut start = history.len().saturating_sub(MAX_CONTEXT_MESSAGES);
        if matches!(
            history.get(start).map(|message| message.role),
            Some(ConversationRole::Assistant)
        ) {
            start += 1;
        }
        for message in &history[start..] {
            match message.role {
                ConversationRole::User => messages.push(ProviderMessage::User {
                    content: message.content.clone(),
                }),
                ConversationRole::Assistant => messages.push(ProviderMessage::Assistant {
                    content: message.content.clone(),
                    tool_calls: Vec::new(),
                }),
            }
        }
        messages
    }
}

/// 运行环境事实：UTC 日历时间（诚实标注，不做假时区）、应用版本。
pub(crate) fn runtime_facts(app_version: &str) -> RuntimeFacts {
    RuntimeFacts {
        local_datetime: format_local_datetime_utc(unix_time_ms().unwrap_or(0)),
        timezone: "UTC".to_string(),
        app_version: app_version.to_string(),
    }
}

/// 会话面的 L0 可信本地只读初始工具集。任务 2 不开放 web_search/fetch。
pub(crate) fn conversation_l0_tools(app_version: String) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry
        .register(
            ToolDefinition::new(
                "get_app_version",
                "返回 ReadRay 当前应用版本。",
                json!({}),
                RiskLevel::TrustedLocalReadOnly,
                move |call, started, _| {
                    Ok(ToolResult::success(
                        call,
                        app_version.clone(),
                        ToolProvenance::LocalFact,
                        started,
                        started + 1,
                    ))
                },
            )
            .expect("内置工具定义必须有效"),
        )
        .expect("内置工具注册必须成功");
    registry
        .register(
            ToolDefinition::new(
                "get_local_datetime",
                "返回当前 UTC 日期时间（Unix 毫秒时间戳对应的 UTC 日历时间）。",
                json!({}),
                RiskLevel::TrustedLocalReadOnly,
                |call, started, _| {
                    Ok(ToolResult::success(
                        call,
                        format_local_datetime_utc(i64::try_from(started).unwrap_or(0)),
                        ToolProvenance::LocalFact,
                        started,
                        started + 1,
                    ))
                },
            )
            .expect("内置工具定义必须有效"),
        )
        .expect("内置工具注册必须成功");
    registry
}

/// run_id 单调计数器（进程内；重启唯一性由 pid + unix 毫秒成分保证）。
static RUN_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 生成进程内唯一且跨重启不碰撞的 run_id（会话身份 + pid + unix 毫秒 + 单调
/// 计数器）。pid 与时间戳保证应用重启后计数器归零也不会与旧 run 冲突。
pub(crate) fn generate_run_id(conversation_id: i64, expected_user_sequence: i64) -> String {
    let counter = RUN_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let unix_ms = unix_time_ms().unwrap_or(0);
    format!(
        "run-{conversation_id}-{expected_user_sequence}-{}-{unix_ms}-{counter}",
        std::process::id()
    )
}

/// 测试专用：把计数器归零，模拟应用重启后重新开始计数。
#[cfg(test)]
pub(crate) fn reset_run_id_counter_for_test() {
    RUN_ID_COUNTER.store(0, Ordering::Relaxed);
}

/// Unix 毫秒 → UTC 日历时间（"YYYY-MM-DD HH:MM (UTC)"）。无 chrono 依赖。
pub(crate) fn format_local_datetime_utc(unix_ms: i64) -> String {
    let seconds = unix_ms.div_euclid(1000);
    let (days, secs_of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    let (hour, minute) = (secs_of_day / 3_600, (secs_of_day % 3_600) / 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} (UTC)")
}

/// Howard Hinnant 的 civil date 算法（days since 1970-01-01 → 年/月/日）。
fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_position = (5 * doy + 2) / 153;
    let day = doy - (153 * month_position + 2) / 5 + 1;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversations::ConversationMessage;
    use crate::learning_records::open_database;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_DATABASE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_database_path() -> (PathBuf, PathBuf) {
        let suffix = format!(
            "readray-chat-surface-{}-{}",
            std::process::id(),
            TEST_DATABASE_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(suffix);
        (root.clone(), root.join("readray.sqlite3"))
    }

    fn message(role: ConversationRole, content: &str, sequence: i64) -> ConversationMessage {
        ConversationMessage {
            id: sequence,
            conversation_id: 1,
            role,
            content: content.to_string(),
            sequence,
            created_at_unix_ms: 1,
        }
    }

    fn adapter(path: &Path) -> ChatSurfaceAdapter {
        let store = ConversationStore::open_path(path).unwrap();
        ChatSurfaceAdapter::open(AgentSurface::MainConversation, store).unwrap()
    }

    fn repository(path: &Path) -> AgentRunRepository {
        AgentRunRepository::open(path).unwrap()
    }

    fn facts() -> RuntimeFacts {
        runtime_facts("0.1.0-test")
    }

    #[test]
    fn rejects_writing_surface() {
        let (root, path) = test_database_path();
        let store = ConversationStore::open_path(&path).unwrap();
        let error = match ChatSurfaceAdapter::open(AgentSurface::WritingCoach, store) {
            Ok(_) => panic!("writing surface 必须被拒绝"),
            Err(error) => error,
        };
        assert!(error.contains("任务 6"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn transcript_projects_system_history_and_truncates_from_user() {
        let (root, path) = test_database_path();
        let mut history = Vec::new();
        for sequence in 1..=41_i64 {
            let role = if sequence % 2 == 1 {
                ConversationRole::User
            } else {
                ConversationRole::Assistant
            };
            history.push(message(role, &format!("message-{sequence}"), sequence));
        }
        let snapshot = ConversationSnapshot {
            id: 1,
            title: None,
            model: "deepseek-v4-flash".to_string(),
            origin: crate::conversations::ConversationOrigin::Main,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
            messages: history,
        };
        let transcript = adapter(&path).transcript(&snapshot, &facts(), &[]);

        assert!(matches!(
            &transcript[0],
            ProviderMessage::System { content }
                if content.starts_with("你是 ReadRay")
        ));
        assert!(matches!(
            &transcript[1],
            ProviderMessage::User { content } if content == "message-3"
        ));
        assert!(matches!(
            transcript.last(),
            Some(ProviderMessage::User { content }) if content == "message-41"
        ));
        assert!(transcript.len() <= MAX_CONTEXT_MESSAGES + 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_and_complete_keep_turn_idempotency() {
        let (root, path) = test_database_path();
        let mut surface = adapter(&path);
        let mut runs = repository(&path);
        let conversation_id = ConversationStore::open_path(&path)
            .unwrap()
            .create("deepseek-v4-flash")
            .unwrap()
            .id;

        let ChatPreparedTurn::Pending {
            user_message_id, ..
        } = surface
            .prepare(conversation_id, 1, "First question", &mut runs)
            .unwrap()
        else {
            panic!("首轮必须为 pending");
        };
        surface
            .complete(conversation_id, 1, user_message_id, "First answer")
            .unwrap();

        // 重试已完成轮次：直接返回权威快照，不重复 user/assistant。
        let ChatPreparedTurn::Completed { snapshot } = surface
            .prepare(conversation_id, 1, "First question", &mut runs)
            .unwrap()
        else {
            panic!("已完成轮次必须返回 Completed");
        };
        assert_eq!(snapshot.messages.len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reconcile_marks_fuzzy_success_run_as_completed() {
        let (root, path) = test_database_path();
        let mut surface = adapter(&path);
        let mut runs = repository(&path);
        let conversation_id = ConversationStore::open_path(&path)
            .unwrap()
            .create("deepseek-v4-flash")
            .unwrap()
            .id;

        // 模拟"complete_turn 已提交但 run 终态未写"的模糊成功。
        let ChatPreparedTurn::Pending {
            user_message_id, ..
        } = surface
            .prepare(conversation_id, 1, "Fuzzy success", &mut runs)
            .unwrap()
        else {
            panic!("首轮必须为 pending");
        };
        let run_id = generate_run_id(conversation_id, 1);
        runs.create_run(&crate::agent_runtime::run_repository::NewRun {
            run_id: run_id.clone(),
            surface: AgentSurface::MainConversation,
            conversation_id,
            expected_user_sequence: 1,
            user_message_id,
            retry_of_run_id: None,
            provider: "deepseek".to_string(),
            model: "deepseek-v4-flash".to_string(),
            started_at_unix_ms: 100,
        })
        .unwrap();
        runs.transition(&run_id, AgentRunStatus::ModelStreaming, None, 100)
            .unwrap();
        runs.transition(&run_id, AgentRunStatus::Synthesizing, None, 100)
            .unwrap();
        surface
            .complete(conversation_id, 1, user_message_id, "Committed answer")
            .unwrap();

        // 再次 prepare 命中 Completed → 对账把 run 标记 completed。
        let ChatPreparedTurn::Completed { .. } = surface
            .prepare(conversation_id, 1, "Fuzzy success", &mut runs)
            .unwrap()
        else {
            panic!("已完成轮次必须返回 Completed");
        };
        let stored = runs.get_run(&run_id).unwrap().unwrap();
        assert_eq!(stored.status, AgentRunStatus::Completed);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn l0_tool_registry_only_contains_trusted_local_read_only_tools() {
        let registry = conversation_l0_tools("0.1.0-test".to_string());
        let tools = registry.active_tools(&Default::default());
        assert_eq!(tools.len(), 2);
        for tool in &tools {
            assert!(matches!(
                tool.name.as_str(),
                "get_app_version" | "get_local_datetime"
            ));
        }
    }

    #[test]
    fn local_datetime_formats_utc_calendar_time_without_faking_timezone() {
        // 2026-01-01 00:00 UTC = 1_767_225_600_000 ms（56 年 + 14 闰日）。
        assert_eq!(format_local_datetime_utc(0), "1970-01-01 00:00 (UTC)");
        assert_eq!(
            format_local_datetime_utc(1_767_225_600_000),
            "2026-01-01 00:00 (UTC)"
        );
        assert_eq!(
            format_local_datetime_utc(1_767_225_600_000 + 86_400_000),
            "2026-01-02 00:00 (UTC)"
        );
        assert_eq!(format_local_datetime_utc(-1), "1969-12-31 23:59 (UTC)");
    }

    #[test]
    fn generate_run_ids_are_unique_within_the_process() {
        let first = generate_run_id(1, 3);
        let second = generate_run_id(1, 3);
        assert_ne!(first, second);
        assert!(first.starts_with("run-1-3-"));
    }

    #[test]
    fn generate_run_ids_are_unique_across_process_restart() {
        // 模拟应用重启：计数器归零后同一轮次再次生成，run_id 不得碰撞
        // （pid 相同，依靠 unix 毫秒成分区分；推进时间保证确定性）。
        let first = generate_run_id(1, 3);
        reset_run_id_counter_for_test();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = generate_run_id(1, 3);
        assert_ne!(first, second, "重启后计数器归零也不能碰撞");
        assert!(first.starts_with("run-1-3-"));
        assert!(second.starts_with("run-1-3-"));
        reset_run_id_counter_for_test();
    }

    #[test]
    fn database_paths_are_isolated() {
        let (root, path) = test_database_path();
        let connection = open_database(&path).unwrap();
        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(version >= 20);
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }
}
