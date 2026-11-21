use serde_json::Value;
use uuid::Uuid;

/// Shared JSON payload extractors used by cloud and desktop upsert paths.
pub fn as_str(p: &Value, key: &str) -> Option<String> {
    match p.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        v => Some(v.to_string()),
    }
}

pub fn as_i32(p: &Value, key: &str) -> Option<i32> {
    let v = p.get(key)?;
    if let Some(n) = v.as_i64() {
        return Some(n as i32);
    }
    v.as_str().and_then(|s| s.parse().ok())
}

pub fn as_i64(p: &Value, key: &str) -> Option<i64> {
    let v = p.get(key)?;
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    v.as_str().and_then(|s| s.parse().ok())
}

pub fn as_bool(p: &Value, key: &str, default: bool) -> bool {
    let Some(v) = p.get(key) else { return default };
    if let Some(b) = v.as_bool() {
        return b;
    }
    if let Some(n) = v.as_i64() {
        return n != 0;
    }
    if let Some(s) = v.as_str() {
        return matches!(s, "true" | "1" | "yes" | "on");
    }
    default
}

pub fn as_uuid(p: &Value, key: &str) -> Option<Uuid> {
    as_str(p, key).and_then(|s| Uuid::parse_str(&s).ok())
}

/// Canonical product vehicle columns replicated by both REST and sync upserts.
pub const PRODUCT_VEHICLE_COLUMNS: &[&str] = &[
    "vehicle_type",
    "year",
    "cylinder_capacity",
    "vin_or_serial",
    "plate_number",
    "odometer_value",
    "odometer_unit",
    "notes",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_bool_and_uuid() {
        let p = json!({ "is_active": 1, "category_id": "00000000-0000-0000-0000-000000000001" });
        assert!(as_bool(&p, "is_active", false));
        assert!(as_uuid(&p, "category_id").is_some());
        assert!(as_uuid(&p, "missing").is_none());
    }
}
