//! Agent Runtime 的协议试验区与任务 1 最小 Agent Kernel。
//!
//! 本模块只承载协议、离线重放、provider spike 与无持久化的最小循环内核。
//! 这里没有 Tauri command、会话/写作装配、SQLite migration 或正式 UI 接线。

pub(crate) mod context;
pub(crate) mod coordinator;
pub(crate) mod gateway;
pub(crate) mod protocol;
pub(crate) mod tool;
pub(crate) mod tool_schema;

#[cfg(test)]
mod fake_gateway;
#[cfg(test)]
mod replay;
#[cfg(test)]
mod responses_spike;
