use serde_json::Value;

const TOP_LEVEL_COMBINATORS: &[&str] = &["oneOf", "allOf", "anyOf"];

pub fn sanitize_top_level_schema(mut schema: Value, tool_name: &str) -> Value {
    let Some(obj) = schema.as_object_mut() else {
        tracing::warn!(
            tool = tool_name,
            schema_type = schema_type_name(&schema),
            "Tool parameters schema is not an object at top-level"
        );
        return schema;
    };

    let removed: Vec<&str> = TOP_LEVEL_COMBINATORS
        .iter()
        .copied()
        .filter(|k| obj.remove(*k).is_some())
        .collect();

    if !removed.is_empty() {
        tracing::warn!(
            tool = tool_name,
            removed_keys = removed.join(","),
            "Removed unsupported top-level schema combinators for provider compatibility"
        );
    }

    schema
}

fn schema_type_name(schema: &Value) -> &'static str {
    match schema {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
