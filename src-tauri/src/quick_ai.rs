use crate::conversations::{
    ConversationMessage, ConversationRole, ConversationSnapshot, ConversationStore,
};
use crate::deepseek_client::{configured_model, post_chat_completion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::AppHandle;

const QUICK_AI_MAX_USER_MESSAGE_LEN: usize = 8_000;
const QUICK_AI_MAX_CONTEXT_MESSAGES: usize = 40;
const QUICK_AI_MAX_TOKENS: u16 = 2_048;
const QUICK_AI_TEMPERATURE: f32 = 0.5;
const QUICK_AI_SYSTEM_PROMPT: &str = "You are Quick AI inside ReadRay. Answer the user's request directly and clearly. Match the user's language and use plain text without Markdown formatting. Do not claim to use tools, web search, local learning records, or long-term memory because none are available in this mode.";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct DeepSeekRequestMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct QuickAiChatResponse {
    choices: Vec<QuickAiChoice>,
}

#[derive(Debug, Deserialize)]
struct QuickAiChoice {
    finish_reason: Option<String>,
    message: QuickAiResponseMessage,
}

#[derive(Debug, Deserialize)]
struct QuickAiResponseMessage {
    content: Option<String>,
}

#[tauri::command]
pub fn create_quick_ai_conversation(app: AppHandle) -> Result<ConversationSnapshot, String> {
    ConversationStore::open_for_app(&app)?.create(&configured_model())
}

#[tauri::command]
pub fn get_quick_ai_conversation(
    app: AppHandle,
    conversation_id: i64,
) -> Result<Option<ConversationSnapshot>, String> {
    ConversationStore::open_for_app(&app)?.get(conversation_id)
}

#[tauri::command]
pub async fn send_quick_ai_message(
    app: AppHandle,
    conversation_id: Option<i64>,
    content: String,
) -> Result<ConversationSnapshot, String> {
    send_with_store_factory(
        || ConversationStore::open_for_app(&app),
        conversation_id,
        &content,
    )
    .await
}

async fn send_with_store_factory<F>(
    mut open_store: F,
    conversation_id: Option<i64>,
    content: &str,
) -> Result<ConversationSnapshot, String>
where
    F: FnMut() -> Result<ConversationStore, String>,
{
    validate_user_message(content)?;
    let existing = match conversation_id {
        Some(id) => Some(open_store()?.get_required(id)?),
        None => None,
    };
    let model = existing
        .as_ref()
        .map(|conversation| conversation.model.clone())
        .unwrap_or_else(configured_model);
    let history = existing
        .as_ref()
        .map(|conversation| conversation.messages.as_slice())
        .unwrap_or(&[]);
    let assistant_content = request_quick_ai_reply(&model, history, content).await?;
    let mut store = open_store()?;

    match conversation_id {
        Some(id) => store.append_exchange(id, content, &assistant_content),
        None => store.create_with_exchange(&model, content, &assistant_content),
    }
}

async fn request_quick_ai_reply(
    model: &str,
    history: &[ConversationMessage],
    content: &str,
) -> Result<String, String> {
    let messages = build_request_messages(history, content);
    let request_body = json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "max_tokens": QUICK_AI_MAX_TOKENS,
        "temperature": QUICK_AI_TEMPERATURE
    });
    let response: QuickAiChatResponse =
        post_chat_completion("DeepSeek Quick AI", &request_body).await?;

    extract_reply(response)
}

fn build_request_messages(
    history: &[ConversationMessage],
    content: &str,
) -> Vec<DeepSeekRequestMessage> {
    let mut messages = vec![DeepSeekRequestMessage {
        role: "system".to_string(),
        content: QUICK_AI_SYSTEM_PROMPT.to_string(),
    }];
    let start = history.len().saturating_sub(QUICK_AI_MAX_CONTEXT_MESSAGES);
    messages.extend(
        history[start..]
            .iter()
            .map(|message| DeepSeekRequestMessage {
                role: role_label(message.role).to_string(),
                content: message.content.clone(),
            }),
    );
    messages.push(DeepSeekRequestMessage {
        role: "user".to_string(),
        content: content.trim().to_string(),
    });
    messages
}

fn extract_reply(response: QuickAiChatResponse) -> Result<String, String> {
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| "DeepSeek Quick AI 响应缺少 choices[0]。".to_string())?;
    if let Some(finish_reason) = choice.finish_reason.as_deref() {
        if finish_reason != "stop" {
            return Err(format!(
                "DeepSeek Quick AI 生成未正常结束：finish_reason={finish_reason}。"
            ));
        }
    }
    let content = choice
        .message
        .content
        .unwrap_or_default()
        .trim()
        .to_string();
    if content.is_empty() {
        return Err("DeepSeek Quick AI 返回空消息。".to_string());
    }

    Ok(content)
}

fn validate_user_message(content: &str) -> Result<(), String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("Quick AI 消息不能为空。".to_string());
    }
    let length = content.chars().count();
    if length > QUICK_AI_MAX_USER_MESSAGE_LEN {
        return Err(format!(
            "Quick AI 消息长度不能超过 {QUICK_AI_MAX_USER_MESSAGE_LEN} 个字符，当前为 {length}。"
        ));
    }
    Ok(())
}

fn role_label(role: ConversationRole) -> &'static str {
    match role {
        ConversationRole::User => "user",
        ConversationRole::Assistant => "assistant",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversations::tests::test_database_path;
    use std::fs;
    use std::path::{Path, PathBuf};

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

    fn load_project_env_for_live_test() {
        let env_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|project_root| project_root.join(".env"));
        if let Some(env_path) = env_path {
            let _ = dotenvy::from_path_override(env_path);
        }
    }

    async fn send_at_path(
        path: &Path,
        conversation_id: Option<i64>,
        content: &str,
    ) -> Result<ConversationSnapshot, String> {
        send_with_store_factory(
            || ConversationStore::open_path(path),
            conversation_id,
            content,
        )
        .await
    }

    #[test]
    fn multi_round_request_contains_ordered_history() {
        let history = vec![
            message(ConversationRole::User, "Remember amber.", 1),
            message(ConversationRole::Assistant, "I will remember amber.", 2),
        ];
        let messages = build_request_messages(&history, "What was the word?");

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages[3].content, "What was the word?");
    }

    #[test]
    fn empty_message_is_rejected_before_request() {
        let error = validate_user_message("  ").unwrap_err();
        assert!(error.contains("不能为空"));
    }

    #[test]
    #[ignore = "requires DEEPSEEK_API_KEY and network access"]
    fn live_two_turn_quick_ai_conversation() {
        load_project_env_for_live_test();
        std::env::set_var("DEEPSEEK_MODEL", "deepseek-v4-flash");
        let (root, path) = test_database_path();

        let first = tauri::async_runtime::block_on(send_at_path(
            &path,
            None,
            "Remember the codeword amber for this conversation. Reply briefly.",
        ))
        .expect("first Quick AI turn should succeed");
        let second = tauri::async_runtime::block_on(send_at_path(
            &path,
            Some(first.id),
            "What codeword did I ask you to remember? Reply with only that word.",
        ))
        .expect("second Quick AI turn should succeed");

        assert_eq!(second.messages.len(), 4);
        assert!(second
            .messages
            .last()
            .unwrap()
            .content
            .to_lowercase()
            .contains("amber"));
        let _ = fs::remove_dir_all(root);
    }
}
