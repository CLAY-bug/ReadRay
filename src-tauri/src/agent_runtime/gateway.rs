//! ModelGateway：屏蔽 provider 协议差异的统一模型边界。
//!
//! 真实 DeepSeek gateway 会在后续任务实现；本任务只定义 trait 与假 provider。
//! 边界：ModelRequest 的输入校验（消息形状、工具与能力一致性）留待真实 provider
//! 任务实施，本任务由 coordinator 在组装侧保证。

use crate::agent_runtime::coordinator::Cancellation;
use crate::agent_runtime::protocol::{
    AgentError, ModelEvent, ProviderContinuationState, RunBudget, ToolCall, ToolResult,
};
use crate::agent_runtime::tool::ToolSchema;

/// 项目到 provider 的最小消息投影。内部 AgentMessage 与展示消息分离，外部
/// 内容由组装方显式标记。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ProviderMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: String,
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        result: ToolResult,
    },
    /// 长上下文兜底（任务 5，方案 A / 方案三）：被折叠的最旧一段对话的极简摘要。
    /// 它只存在投影/内存层——不写回用户可见 transcript、不落库、不进学习者记忆。
    /// 投影时作为一条**真实的 user 历史消息**（而非 system）提供给模型，措辞明确
    /// 标为"供回顾参考、以当前对话为准"，避免把过时内容抬成全局权威背景。
    CompactionSummary {
        content: String,
    },
}

/// 单次模型请求。每轮请求都携带完整 transcript 与活动工具；provider 按 stateless
/// 处理，不依赖调用方维护会话。
#[derive(Clone, Debug)]
pub(crate) struct ModelRequest {
    pub messages: Vec<ProviderMessage>,
    pub tools: Vec<ToolSchema>,
    /// 供真实 provider 实施重试/预算策略；当前 DeepSeek gateway 只读
    /// deadline 与 cancellation。
    #[allow(dead_code)]
    pub budget: RunBudget,
    pub deadline_unix_ms: u64,
    pub cancellation: Cancellation,
}

/// 一次模型 turn 的结果概要。文本、usage、tool call 等通过流式回调下发，
/// coordinator 边接收边发布事件。
#[derive(Clone, Copy, Debug)]
pub(crate) struct ModelTurnOutcome {
    pub aborted: bool,
}

pub(crate) trait ModelGateway {
    /// 流式请求模型。回调收到每个 `ModelEvent`；回调返回 Err 时 gateway 应立即
    /// 停止流式输出并把错误向上传播（用于同步内核的停止贯穿全链路）。
    fn stream_model(
        &mut self,
        request: ModelRequest,
        on_event: &mut dyn FnMut(ModelEvent) -> Result<(), AgentError>,
    ) -> Result<ModelTurnOutcome, AgentError>;

    /// 只留内存的 provider continuation 状态；私有 reasoning 永不进入 AgentEvent。
    /// 当前生产链路不读取（测试断言 reasoning 不泄漏时使用）。
    #[allow(dead_code)]
    fn continuation(&self) -> &ProviderContinuationState;
}
