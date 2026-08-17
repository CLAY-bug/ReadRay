//! Agent Runtime 的协议试验区。
//!
//! 本模块只承载任务 0 冻结前的纯协议、离线重放夹具和 provider live spike。
//! 这里没有 Tauri command、会话/写作装配、SQLite migration 或生产 Agent loop。

pub(crate) mod protocol;

#[cfg(test)]
mod replay;

#[cfg(test)]
mod responses_spike;
