use serde_json::Value;

pub fn text_fingerprint(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in text.as_bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn summarize_text(text: &str) -> String {
    format!("len={},fp={}", text.chars().count(), text_fingerprint(text))
}

pub fn summarize_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut fields = Vec::new();
            for (k, v) in map.iter().take(8) {
                fields.push(format!("{k}:{}", value_shape(v)));
            }
            format!(
                "object(keys={},fields=[{}],bytes={})",
                map.len(),
                fields.join(","),
                value.to_string().len()
            )
        }
        Value::Array(arr) => {
            let sample = arr
                .iter()
                .take(5)
                .map(value_shape)
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "array(len={},items=[{}],bytes={})",
                arr.len(),
                sample,
                value.to_string().len()
            )
        }
        _ => value_shape(value),
    }
}

fn value_shape(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(_) => "number".to_string(),
        Value::String(s) => format!("string({})", s.chars().count()),
        Value::Array(arr) => format!("array({})", arr.len()),
        Value::Object(map) => format!("object({})", map.len()),
    }
}
