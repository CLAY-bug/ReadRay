//! Agent Runtime 的协议试验区与任务 1/2 内核。
//!
//! 承载协议、离线重放、provider spike、无持久化循环内核与任务 2 的会话接入
//! （ChatSurfaceAdapter / RunRepository / DeepSeek gateway）。Tauri command 与
//! 正式链路装配在 `quick_ai.rs`，这里不直接注册 command。

// 协议文件按任务 0 评审要求冻结；其校验函数与部分变体由测试和后续任务
// （任务 3 来源/任务 6 Writing）使用，生产接入后仍属预期未用项。
pub(crate) mod chat_surface;
pub(crate) mod context;
pub(crate) mod coordinator;
pub(crate) mod deepseek_gateway;
pub(crate) mod gateway;
#[allow(dead_code)]
pub(crate) mod network;
#[allow(dead_code)]
pub(crate) mod protocol;
pub(crate) mod run_repository;
pub(crate) mod tool;
pub(crate) mod tool_schema;

#[cfg(test)]
pub(crate) mod fake_gateway;
#[cfg(test)]
mod replay;
#[cfg(test)]
mod responses_spike;
