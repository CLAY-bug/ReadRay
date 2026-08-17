//! ToolRegistry / ToolDefinition / ToolPolicy 与 L0 能力边界。
//!
//! 能力注册与激活分离：`ToolRegistry` 持有全部已注册工具，`CapabilityPolicy`
//! 决定当前 run 的 active tool set，`ToolPolicy` 在最终执行边界重新校验工具的
//! 存在性、风险等级、显式启用状态和参数 schema。任务 1 只允许 L0 可信本地只读
//! 工具；web search / fetch / 任意文件 / Bash 等能力不属于本任务。

use crate::agent_runtime::protocol::{AgentError, AgentErrorKind, RunBudget, ToolCall, ToolResult};
use crate::agent_runtime::tool_schema;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// 工具风险分级，对应方案第 12 节。任务 1 只实现 L0 可信本地只读。
/// 声明顺序即等级顺序：TrustedLocalReadOnly < ExternalReadOnly < DomainWrite。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum RiskLevel {
    TrustedLocalReadOnly,
    ExternalReadOnly,
    DomainWrite,
}

/// 发送给 provider 的活动工具描述。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// 已注册工具的完整定义。执行器归 ToolDefinition 所有，避免在注册表内复制。
/// 返回完整 `ToolResult`（或标准 `AgentError`）；coordinator 在最终执行边界
/// 核对返回结果与本次调用身份是否一致。
pub(crate) struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub risk_level: RiskLevel,
    executor: Box<dyn Fn(&ToolCall, u64, &RunBudget) -> Result<ToolResult, AgentError> + 'static>,
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        risk_level: RiskLevel,
        executor: impl Fn(&ToolCall, u64, &RunBudget) -> Result<ToolResult, AgentError> + 'static,
    ) -> Result<Self, String> {
        let name = name.into();
        validate_name(&name)?;
        let description = description.into();
        if description.trim().is_empty() || description.len() > 512 {
            return Err("工具描述不能为空且不能超过 512 字节。".to_string());
        }
        tool_schema::validate_schema_supported(&input_schema)?;
        Ok(Self {
            name,
            description,
            input_schema,
            risk_level,
            executor: Box::new(executor),
        })
    }

    pub fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
        }
    }

    pub fn execute(
        &self,
        call: &ToolCall,
        started_at_unix_ms: u64,
        budget: &RunBudget,
    ) -> Result<ToolResult, AgentError> {
        (self.executor)(call, started_at_unix_ms, budget)
    }
}

/// 当前 run 的能力策略：风险上限 + 可选显式启用集合。
#[derive(Clone, Debug)]
pub(crate) struct CapabilityPolicy {
    pub allowed_risk: RiskLevel,
    /// None 表示启用风险上限内的全部已注册工具；Some 表示只启用集合内的名称
    /// （仍受风险上限约束）。
    pub enabled_tools: Option<BTreeSet<String>>,
}

impl Default for CapabilityPolicy {
    fn default() -> Self {
        Self {
            allowed_risk: RiskLevel::TrustedLocalReadOnly,
            enabled_tools: None,
        }
    }
}

pub(crate) struct ToolRegistry {
    tools: BTreeMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, definition: ToolDefinition) -> Result<(), String> {
        if self.tools.contains_key(&definition.name) {
            return Err(format!("工具 {} 已注册。", definition.name));
        }
        let name = definition.name.clone();
        self.tools.insert(name, definition);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    /// 依据能力策略生成 active tool set。enabled_tools 中未注册的名称忽略。
    pub fn active_tools(&self, capability: &CapabilityPolicy) -> Vec<ToolSchema> {
        self.tools
            .values()
            .filter(|definition| definition.risk_level <= capability.allowed_risk)
            .filter(|definition| {
                capability
                    .enabled_tools
                    .as_ref()
                    .map_or(true, |enabled| enabled.contains(&definition.name))
            })
            .map(ToolDefinition::schema)
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 最终执行边界策略：即使 active tool set 已经过滤过，工具真正执行前仍要重新
/// 校验存在性、风险、显式启用和参数 schema（方案第 6.4 条）。
pub(crate) struct ToolPolicy;

impl ToolPolicy {
    pub fn authorize<'a>(
        &self,
        call: &ToolCall,
        registry: &'a ToolRegistry,
        capability: &CapabilityPolicy,
    ) -> Result<&'a ToolDefinition, AgentError> {
        let definition = registry.get(&call.name).ok_or_else(|| {
            agent_error(
                AgentErrorKind::UnknownTool,
                format!("工具 {} 未注册或不在本 run 的能力范围内。", call.name),
            )
        })?;
        if definition.risk_level > capability.allowed_risk {
            return Err(agent_error(
                AgentErrorKind::ToolPolicyDenied,
                format!(
                    "工具 {} 的风险等级超出本 run 允许的最大风险。",
                    definition.name
                ),
            ));
        }
        if capability
            .enabled_tools
            .as_ref()
            .is_some_and(|enabled| !enabled.contains(&definition.name))
        {
            return Err(agent_error(
                AgentErrorKind::ToolPolicyDenied,
                format!("工具 {} 不在本 run 的 active tool set。", definition.name),
            ));
        }
        tool_schema::validate_instance(&definition.input_schema, &call.arguments)
            .map_err(|message| agent_error(AgentErrorKind::ToolSchemaInvalid, message))?;
        Ok(definition)
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > 64 {
        return Err("工具名称不能为空且不能超过 64 字节。".to_string());
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err("工具名称只能包含 ASCII 字母、数字、下划线或连字符。".to_string());
    }
    Ok(())
}

fn agent_error(kind: AgentErrorKind, message: impl Into<String>) -> AgentError {
    AgentError::new(kind, message).expect("工具策略的固定错误消息必须有效")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::protocol::{ToolProvenance, ToolResult};
    use serde_json::json;

    fn ok_tool(name: &str, description: &str) -> ToolDefinition {
        ToolDefinition::new(
            name,
            description,
            json!({}),
            RiskLevel::TrustedLocalReadOnly,
            |call, started, _| {
                Ok(ToolResult::success(
                    call,
                    "ok",
                    ToolProvenance::LocalFact,
                    started,
                    started + 1,
                ))
            },
        )
        .expect("测试工具定义必须有效")
    }

    #[test]
    fn registry_rejects_duplicate_names() {
        let mut registry = ToolRegistry::new();
        registry.register(ok_tool("get_date", "日期")).unwrap();
        assert!(registry.register(ok_tool("get_date", "日期")).is_err());
        assert!(registry.get("get_date").is_some());
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn active_tools_respect_risk_ceiling() {
        let mut registry = ToolRegistry::new();
        registry.register(ok_tool("local_a", "本地")).unwrap();
        registry
            .register(
                ToolDefinition::new(
                    "web_read",
                    "读取网页。",
                    json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}),
                    RiskLevel::ExternalReadOnly,
                    |call, started, _| {
                        Ok(ToolResult::success(
                            call,
                            "external",
                            ToolProvenance::ExternalPage,
                            started,
                            started + 1,
                        ))
                    },
                )
                .unwrap(),
            )
            .unwrap();
        registry
            .register(
                ToolDefinition::new(
                    "save_quote",
                    "保存引用。",
                    json!({}),
                    RiskLevel::DomainWrite,
                    |call, started, _| {
                        Ok(ToolResult::success(
                            call,
                            "saved",
                            ToolProvenance::LocalFact,
                            started,
                            started + 1,
                        ))
                    },
                )
                .unwrap(),
            )
            .unwrap();
        let active = registry.active_tools(&CapabilityPolicy::default());
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "local_a");
    }

    #[test]
    fn active_tools_respect_enabled_set() {
        let mut registry = ToolRegistry::new();
        registry.register(ok_tool("get_date", "日期")).unwrap();
        registry.register(ok_tool("get_version", "版本")).unwrap();
        let mut capability = CapabilityPolicy::default();
        capability.enabled_tools = Some(BTreeSet::from(["get_date".to_string()]));
        let active = registry.active_tools(&capability);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "get_date");
    }

    #[test]
    fn policy_authorize_rejects_unknown_denied_and_schema_invalid() {
        let mut registry = ToolRegistry::new();
        registry.register(ok_tool("get_date", "日期")).unwrap();
        registry
            .register(
                ToolDefinition::new(
                    "echo",
                    "原样返回文本。",
                    json!({
                        "type": "object",
                        "properties": { "text": { "type": "string", "minLength": 1 } },
                        "required": ["text"]
                    }),
                    RiskLevel::TrustedLocalReadOnly,
                    |call, started, _| {
                        Ok(ToolResult::success(
                            call,
                            "echo",
                            ToolProvenance::LocalFact,
                            started,
                            started + 1,
                        ))
                    },
                )
                .unwrap(),
            )
            .unwrap();
        registry
            .register(
                ToolDefinition::new(
                    "read_web",
                    "读取网页。",
                    json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}),
                    RiskLevel::ExternalReadOnly,
                    |_, _, _| Err(agent_error(AgentErrorKind::ToolExecutionFailed, "unused")),
                )
                .unwrap(),
            )
            .unwrap();
        let policy = ToolPolicy;
        let capability = CapabilityPolicy::default();

        let unknown = ToolCall {
            id: "c1".into(),
            name: "not_registered".into(),
            arguments: json!({}),
        };
        assert_eq!(
            policy
                .authorize(&unknown, &registry, &capability)
                .err()
                .unwrap()
                .kind,
            AgentErrorKind::UnknownTool
        );

        let denied = ToolCall {
            id: "c2".into(),
            name: "read_web".into(),
            arguments: json!({"url": "https://example.com"}),
        };
        assert_eq!(
            policy
                .authorize(&denied, &registry, &capability)
                .err()
                .unwrap()
                .kind,
            AgentErrorKind::ToolPolicyDenied
        );

        let schema_invalid = ToolCall {
            id: "c3".into(),
            name: "echo".into(),
            arguments: json!({}),
        };
        assert_eq!(
            policy
                .authorize(&schema_invalid, &registry, &capability)
                .err()
                .unwrap()
                .kind,
            AgentErrorKind::ToolSchemaInvalid
        );

        let allowed = ToolCall {
            id: "c4".into(),
            name: "get_date".into(),
            arguments: json!({}),
        };
        assert!(policy.authorize(&allowed, &registry, &capability).is_ok());
    }

    #[test]
    fn definition_rejects_unsupported_schema_keywords() {
        let definition = ToolDefinition::new(
            "bad_tool",
            "描述",
            json!({"pattern": "x"}),
            RiskLevel::TrustedLocalReadOnly,
            |_, _, _| Err(agent_error(AgentErrorKind::ToolExecutionFailed, "unused")),
        );
        assert!(definition.is_err());
    }
}
