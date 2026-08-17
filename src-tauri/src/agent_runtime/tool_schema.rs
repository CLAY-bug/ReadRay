//! 面向 L0 只读工具的严格 JSON Schema 子集校验。
//!
//! 任务 1 只注册可信本地只读工具，因此这里只实现这些工具需要的 schema 关键字，
//! 不引入完整 JSON Schema 实现，也不新增依赖。注册工具时先校验 schema 只用
//! 白名单关键字（防止“静默忽略未知关键字”造成的弱校验），执行前再校验参数实例。
//! 默认拒绝 instance 中 schema 未声明的字段，满足“拒绝未知或越界字段”的协议要求。

use serde_json::Value;
use std::collections::BTreeSet;

const SUPPORTED_KEYWORDS: &[&str] = &[
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "enum",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "minItems",
    "maxItems",
];

const KNOWN_TYPES: &[&str] = &[
    "object", "string", "integer", "number", "boolean", "array", "null",
];

/// 注册期检查：拒绝任何白名单之外的关键字，避免弱校验关键字被静默忽略。
pub(crate) fn validate_schema_supported(schema: &Value) -> Result<(), String> {
    let Some(object) = schema.as_object() else {
        return Err("工具 schema 必须是 JSON object。".to_string());
    };
    for key in object.keys() {
        if !SUPPORTED_KEYWORDS.contains(&key.as_str()) {
            return Err(format!("工具 schema 使用不支持的关键字：{key}。"));
        }
    }
    if let Some(type_value) = object.get("type") {
        let types: Vec<&str> = match type_value {
            Value::String(name) => vec![name.as_str()],
            Value::Array(items) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .ok_or_else(|| "schema 的 type 数组元素必须是字符串。".to_string())
                })
                .collect::<Result<_, _>>()?,
            _ => return Err("schema 的 type 必须是字符串或字符串数组。".to_string()),
        };
        for name in types {
            if !KNOWN_TYPES.contains(&name) {
                return Err(format!("schema 使用未知类型：{name}。"));
            }
        }
    }
    if let Some(props) = object.get("properties") {
        let props = props
            .as_object()
            .ok_or_else(|| "schema 的 properties 必须是 object。".to_string())?;
        for sub_schema in props.values() {
            validate_schema_supported(sub_schema)?;
        }
    }
    if let Some(items) = object.get("items") {
        validate_schema_supported(items)?;
    }
    if let Some(required) = object.get("required") {
        let required = required
            .as_array()
            .ok_or_else(|| "schema 的 required 必须是数组。".to_string())?;
        for name in required {
            if name.as_str().is_none() {
                return Err("schema 的 required 元素必须是字符串。".to_string());
            }
        }
    }
    if let Some(enum_values) = object.get("enum") {
        if !enum_values.is_array() {
            return Err("schema 的 enum 必须是数组。".to_string());
        }
    }
    if let Some(flag) = object.get("additionalProperties") {
        if !flag.is_boolean() {
            return Err("schema 的 additionalProperties 必须是布尔值。".to_string());
        }
    }
    for key in [
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "minItems",
        "maxItems",
    ] {
        if let Some(value) = object.get(key) {
            if !value.is_number() {
                return Err(format!("schema 的 {key} 必须是数字。"));
            }
        }
    }
    Ok(())
}

/// 实例校验：instance 必须通过 schema 的全部约束。
pub(crate) fn validate_instance(schema: &Value, instance: &Value) -> Result<(), String> {
    let object = schema
        .as_object()
        .expect("调用方必须先通过 validate_schema_supported");
    if let Some(type_value) = object.get("type") {
        let types: Vec<&str> = match type_value {
            Value::String(name) => vec![name.as_str()],
            Value::Array(items) => items
                .iter()
                .map(|item| item.as_str().expect("对类型数组元素先做了校验"))
                .collect(),
            _ => unreachable!(),
        };
        if !types.iter().any(|name| matches_type(name, instance)) {
            return Err(format!(
                "值必须是 {:?} 之一，实际为 {}。",
                types,
                type_name(instance)
            ));
        }
    }
    if let Some(enum_values) = object.get("enum") {
        let enum_values = enum_values.as_array().expect("对 enum 先做了校验");
        if !enum_values.iter().any(|candidate| candidate == instance) {
            return Err("值不在 enum 允许范围内。".to_string());
        }
    }
    if let Some(value) = object.get("minLength") {
        let limit = value.as_u64().expect("对 minLength 先做了校验");
        if let Some(text) = instance.as_str() {
            if text.chars().count() < limit as usize {
                return Err(format!("字符串长度不能小于 {limit}。"));
            }
        }
    }
    if let Some(value) = object.get("maxLength") {
        let limit = value.as_u64().expect("对 maxLength 先做了校验");
        if let Some(text) = instance.as_str() {
            if text.chars().count() > limit as usize {
                return Err(format!("字符串长度不能大于 {limit}。"));
            }
        }
    }
    if let Some(value) = object.get("minimum") {
        let limit = value.as_f64().expect("对 minimum 先做了校验");
        if let Some(number) = instance.as_f64() {
            if number < limit {
                return Err(format!("数值不能小于 {limit}。"));
            }
        }
    }
    if let Some(value) = object.get("maximum") {
        let limit = value.as_f64().expect("对 maximum 先做了校验");
        if let Some(number) = instance.as_f64() {
            if number > limit {
                return Err(format!("数值不能大于 {limit}。"));
            }
        }
    }
    if let Some(value) = object.get("minItems") {
        let limit = value.as_u64().expect("对 minItems 先做了校验");
        if let Some(items) = instance.as_array() {
            if items.len() < limit as usize {
                return Err(format!("数组长度不能小于 {limit}。"));
            }
        }
    }
    if let Some(value) = object.get("maxItems") {
        let limit = value.as_u64().expect("对 maxItems 先做了校验");
        if let Some(items) = instance.as_array() {
            if items.len() > limit as usize {
                return Err(format!("数组长度不能大于 {limit}。"));
            }
        }
    }

    match instance {
        Value::Object(map) => {
            if let Some(required) = object.get("required") {
                for name in required.as_array().expect("对 required 先做了校验") {
                    let name = name.as_str().expect("对 required 元素先做了校验");
                    if !map.contains_key(name) {
                        return Err(format!("缺少必填字段：{name}。"));
                    }
                }
            }
            if let Some(props) = object.get("properties") {
                let props = props.as_object().expect("对 properties 先做了校验");
                for (key, sub_schema) in props {
                    if let Some(value) = map.get(key) {
                        validate_instance(sub_schema, value)?;
                    }
                }
            }
            let allow_extra = object
                .get("additionalProperties")
                .and_then(|flag| flag.as_bool())
                .unwrap_or(false);
            if !allow_extra {
                let declared: BTreeSet<&str> = object
                    .get("properties")
                    .and_then(|props| props.as_object())
                    .map(|props| props.keys().map(String::as_str).collect())
                    .unwrap_or_default();
                for key in map.keys() {
                    if !declared.contains(key.as_str()) {
                        return Err(format!("未知字段：{key}。"));
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(sub_schema) = object.get("items") {
                for item in items {
                    validate_instance(sub_schema, item)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn matches_type(name: &str, instance: &Value) -> bool {
    match name {
        "object" => instance.is_object(),
        "string" => instance.is_string(),
        "integer" => instance.is_i64(),
        "number" => instance.is_number(),
        "boolean" => instance.is_boolean(),
        "array" => instance.is_array(),
        "null" => instance.is_null(),
        _ => false,
    }
}

fn type_name(instance: &Value) -> &'static str {
    match instance {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_unknown_fields_by_default() {
        let schema = json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        });
        assert!(validate_instance(&schema, &json!({"text": "hi"})).is_ok());
        assert!(validate_instance(&schema, &json!({"text": "hi", "extra": 1})).is_err());
        assert!(validate_instance(&schema, &json!({})).is_err());
        assert!(validate_instance(&schema, &json!({"text": 5})).is_err());
    }

    #[test]
    fn additional_properties_true_allows_extra_fields() {
        let schema = json!({
            "type": "object",
            "additionalProperties": true,
            "properties": { "n": { "type": "integer", "minimum": 1, "maximum": 5 } }
        });
        assert!(validate_instance(&schema, &json!({"n": 3, "x": 1})).is_ok());
        assert!(validate_instance(&schema, &json!({"n": 6})).is_err());
        assert!(validate_instance(&schema, &json!({"n": "1"})).is_err());
    }

    #[test]
    fn validates_arrays_enum_and_string_length() {
        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 },
                    "maxItems": 2
                },
                "mode": { "type": "string", "enum": ["a", "b"] }
            }
        });
        assert!(validate_instance(&schema, &json!({"tags": ["x", "y"], "mode": "a"})).is_ok());
        assert!(validate_instance(&schema, &json!({"tags": ["x", "y", "z"]})).is_err());
        assert!(validate_instance(&schema, &json!({"mode": "c"})).is_err());
        assert!(validate_instance(&schema, &json!({"tags": [""]})).is_err());
    }

    #[test]
    fn schema_registration_rejects_unsupported_keywords() {
        assert!(validate_schema_supported(&json!({"type": "object", "properties": {}})).is_ok());
        assert!(validate_schema_supported(&json!({"pattern": "x"})).is_err());
        assert!(validate_schema_supported(&json!({"type": "unknown_type"})).is_err());
        assert!(validate_schema_supported(&json!([])).is_err());
    }
}
