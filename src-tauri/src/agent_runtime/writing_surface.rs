//! WritingSurfaceAdapter：Writing Coach 共享 Runtime 内核的 surface 适配层。
//!
//! 本模块只负责把写作身份与上下文投影为统一 run 输入，并给出写作专属的
//! **独立 active tool set**（不继承通用对话 L1 的全部工具，默认不自动联网）；
//! 不复制 Agent loop / ModelGateway / ToolRegistry / 网络工具。写作的校验与
//! 持久化权威仍归 `writing.rs`：coordinator 产出的最终 assistant 文本仍是
//! 结构化 JSON，由 `writing.rs` 负责解析/校验/按 expectedRevision 保存后进入
//! 正式状态。本模块不直接读写业务表。

use crate::agent_runtime::gateway::ProviderMessage;
use crate::agent_runtime::tool::{CapabilityPolicy, ToolRegistry};

/// 写作 surface：承载写作分析 / 问答的系统提示词，并据此组装 provider 上下文。
///
/// 系统提示词文本由 `writing.rs` 提供（写作专项语义），这里只负责把它与用户
/// 输入投影为 `[System, User]` 的 provider 消息序列。
pub(crate) struct WritingSurfaceAdapter {
    system_prompt: String,
}

impl WritingSurfaceAdapter {
    pub(crate) fn new(system_prompt: impl Into<String>) -> Self {
        Self {
            system_prompt: system_prompt.into(),
        }
    }

    /// 组装写作专项上下文：系统提示词 + 含文章/问题的用户消息。
    pub(crate) fn transcript(&self, user_content: &str) -> Vec<ProviderMessage> {
        vec![
            ProviderMessage::System {
                content: self.system_prompt.clone(),
            },
            ProviderMessage::User {
                content: user_content.to_string(),
            },
        ]
    }
}

/// 写作专属独立 active tool set：与写作任务相关的只读能力。
///
/// 边界：默认**不纳入** Web Search / 任意文件 / Bash——写作检查和问答是纯
/// 文章分析，无需外部信息，也不应继承通用对话 L1 的全部工具。这里返回空工具
/// 集（保持最小范围）；若未来确有"核实事实/资料"的写作需求，可在此注册受控
/// 只读工具并把它与建议绑定（本任务只留接口，不自动启用）。
pub(crate) fn writing_active_tools() -> ToolRegistry {
    ToolRegistry::new()
}

/// 写作能力策略：只允许可信本地只读（L0）。空 active tool set 下模型只能
/// 根据文章内容直接回答，不得假装联网/检索。
pub(crate) fn writing_capability() -> CapabilityPolicy {
    CapabilityPolicy::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_active_tools_are_empty_by_default() {
        // 写作检查/问答不自动联网、不继承对话 L1 工具：默认无外部或本地工具。
        let registry = writing_active_tools();
        let active = registry.active_tools(&writing_capability());
        assert!(active.is_empty(), "写作默认不应有任何 active 工具");
    }

    #[test]
    fn writing_capability_stays_l0_trusted_local() {
        assert_eq!(
            writing_capability().allowed_risk,
            crate::agent_runtime::tool::RiskLevel::TrustedLocalReadOnly
        );
    }

    #[test]
    fn transcript_is_system_then_user() {
        let adapter = WritingSurfaceAdapter::new("你是 ReadRay 的写作教练。");
        let messages = adapter.transcript("请检查这篇文章。");
        assert_eq!(messages.len(), 2);
        assert!(matches!(
            &messages[0],
            ProviderMessage::System { content } if content == "你是 ReadRay 的写作教练。"
        ));
        assert!(matches!(
            &messages[1],
            ProviderMessage::User { content } if content == "请检查这篇文章。"
        ));
    }
}
