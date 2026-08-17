//! ModelGateway：屏蔽 provider 协议差异的统一模型边界。
//!
//! 真实 DeepSeek gateway 会在后续任务实现；本任务只定义 trait 与假 provider。

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
}

/// 单次模型请求。每轮请求都携带完整 transcript 与活动工具；provider 按 stateless
/// 处理，不依赖调用方维护会话。
#[derive(Clone, Debug)]
pub(crate) struct ModelRequest {
    pub messages: Vec<ProviderMessage>,
    pub tools: Vec<ToolSchema>,
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
    fn continuation(&self) -> &ProviderContinuationState;
}
