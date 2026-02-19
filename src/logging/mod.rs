use std::fmt::{Debug, Formatter};

pub struct Redacted<'a, T: ?Sized> {
    value: &'a T,
}

impl<'a, T: ?Sized> Redacted<'a, T> {
    pub const fn new(value: &'a T) -> Self {
        Self { value }
    }
}

impl Debug for Redacted<'_, str> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED length={}]", self.value.len())
    }
}

impl Debug for Redacted<'_, String> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Redacted::new(self.value.as_str()).fmt(f)
    }
}

impl Debug for Redacted<'_, serde_json::Value> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value = self.value;
        match value {
            serde_json::Value::Object(map) => {
                let keys = map.keys().take(12).cloned().collect::<Vec<_>>().join(",");
                write!(
                    f,
                    "[REDACTED type=object keys=[{}] key_count={} length={}]",
                    keys,
                    map.len(),
                    value.to_string().len()
                )
            }
            serde_json::Value::Array(arr) => write!(
                f,
                "[REDACTED type=array items={} length={}]",
                arr.len(),
                value.to_string().len()
            ),
            serde_json::Value::String(s) => {
                write!(f, "[REDACTED type=string length={}]", s.len())
            }
            serde_json::Value::Number(_) => write!(f, "[REDACTED type=number]"),
            serde_json::Value::Bool(_) => write!(f, "[REDACTED type=bool]"),
            serde_json::Value::Null => write!(f, "[REDACTED type=null]"),
        }
    }
}
