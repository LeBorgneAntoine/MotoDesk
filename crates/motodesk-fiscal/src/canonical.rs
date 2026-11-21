//! Deterministic JSON serialisation for hashing.
//!
//! Two installations must produce byte-identical output for the same payload,
//! otherwise a replicated chain would appear broken. This follows RFC 8785
//! (JSON Canonicalization Scheme) with one deliberate restriction: floating
//! point numbers are rejected rather than serialised. Every monetary value in
//! the fiscal journal is an integer (cents, thousandths, basis points), so the
//! one genuinely fragile part of JCS -- ECMAScript number formatting -- is
//! avoided entirely instead of being approximated.

use serde_json::Value;

use crate::error::FiscalError;

/// Serialise `value` to its canonical form.
pub fn to_canonical_string(value: &Value) -> Result<String, FiscalError> {
    let mut out = String::new();
    write_value(value, &mut out)?;
    Ok(out)
}

/// Serialise `value` to canonical UTF-8 bytes, the input to every hash.
pub fn to_canonical_bytes(value: &Value) -> Result<Vec<u8>, FiscalError> {
    Ok(to_canonical_string(value)?.into_bytes())
}

fn write_value(value: &Value, out: &mut String) -> Result<(), FiscalError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else {
                return Err(FiscalError::NonIntegerNumber(n.to_string()));
            }
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            // JCS orders members by UTF-16 code unit, which differs from Rust's
            // UTF-8 byte ordering above the BMP.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by_key(|key| utf16_units(key));
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_value(&map[*key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn utf16_units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn orders_object_keys() {
        let v = json!({ "b": 1, "a": 2, "C": 3 });
        assert_eq!(to_canonical_string(&v).unwrap(), r#"{"C":3,"a":2,"b":1}"#);
    }

    #[test]
    fn ordering_is_independent_of_insertion_order() {
        let a: Value = serde_json::from_str(r#"{"z":1,"a":{"y":2,"b":3}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":{"b":3,"y":2},"z":1}"#).unwrap();
        assert_eq!(
            to_canonical_string(&a).unwrap(),
            to_canonical_string(&b).unwrap()
        );
    }

    #[test]
    fn emits_no_insignificant_whitespace() {
        let v = json!({ "lines": [ { "qty": 1 }, { "qty": 2 } ] });
        assert_eq!(
            to_canonical_string(&v).unwrap(),
            r#"{"lines":[{"qty":1},{"qty":2}]}"#
        );
    }

    #[test]
    fn escapes_control_characters() {
        let v = json!({ "label": "a\"b\\c\nd\te\u{01}f" });
        assert_eq!(
            to_canonical_string(&v).unwrap(),
            r#"{"label":"a\"b\\c\nd\te\u0001f"}"#
        );
    }

    #[test]
    fn keeps_accents_literal() {
        let v = json!({ "nom": "Vélo Éric" });
        assert_eq!(to_canonical_string(&v).unwrap(), "{\"nom\":\"Vélo Éric\"}");
    }

    #[test]
    fn rejects_floats() {
        let v = json!({ "amount": 12.5 });
        assert!(matches!(
            to_canonical_string(&v),
            Err(FiscalError::NonIntegerNumber(_))
        ));
    }

    #[test]
    fn accepts_negative_and_large_integers() {
        let v = json!({ "a": -4200, "b": i64::MAX });
        assert_eq!(
            to_canonical_string(&v).unwrap(),
            format!(r#"{{"a":-4200,"b":{}}}"#, i64::MAX)
        );
    }
}
