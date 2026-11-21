//! HMAC authentication for LAN mesh apply requests.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::Mutex;

type HmacSha256 = Hmac<Sha256>;

const MAX_SKEW_SECS: i64 = 300;
const NONCE_TTL: Duration = Duration::from_secs(600);

pub fn sign(secret: &str, nonce: &str, ts: i64, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC key length is valid for SHA-256");
    mac.update(nonce.as_bytes());
    mac.update(b":");
    mac.update(ts.to_string().as_bytes());
    mac.update(b":");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// A byte string for a batch of ops that does not depend on how either side
/// serialises it. `field_meta` is a `HashMap`, so two serialisations of the same
/// ops list its keys in different orders: signing the sender's bytes and
/// checking the receiver's re-serialisation never matched, and every batch was
/// refused. Keys are sorted here, recursively, on both sides.
pub fn canonical_ops_bytes(ops: &[motodesk_core::sync::ChangeLogEntry]) -> Vec<u8> {
    let value = serde_json::to_value(ops).unwrap_or(serde_json::Value::Null);
    canonical_value_bytes(&value)
}

/// The same canonical form for any JSON value: object keys sorted, recursively.
pub fn canonical_value_bytes(value: &serde_json::Value) -> Vec<u8> {
    fn write(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::Value::String(k.clone()).to_string());
                    out.push(':');
                    write(&map[k], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out.into_bytes()
}

/// Sign a JSON value with the mesh secret: a fresh nonce, the time, and the signature.
pub fn sign_value(secret: &str, value: &serde_json::Value) -> (String, i64, String) {
    let nonce = uuid::Uuid::new_v4().to_string();
    let ts = chrono::Utc::now().timestamp();
    let sig = sign(secret, &nonce, ts, &canonical_value_bytes(value));
    (nonce, ts, sig)
}

/// Whether a signature over a JSON value is genuine and recent. (Replay is the caller's:
/// a server also asks its `NonceCache`.)
pub fn value_signature_ok(secret: &str, nonce: &str, ts: i64, sig: &str, value: &serde_json::Value) -> bool {
    timestamp_fresh(ts, chrono::Utc::now().timestamp())
        && constant_time_eq(sig, &sign(secret, nonce, ts, &canonical_value_bytes(value)))
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.as_bytes().iter().zip(b.as_bytes().iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

pub struct NonceCache {
    seen: Mutex<HashMap<String, Instant>>,
}

impl NonceCache {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashMap::new()),
        }
    }

    pub async fn accept(&self, nonce: &str) -> bool {
        let mut map = self.seen.lock().await;
        let now = Instant::now();
        map.retain(|_, t| now.duration_since(*t) < NONCE_TTL);
        if map.contains_key(nonce) {
            return false;
        }
        map.insert(nonce.to_string(), now);
        true
    }
}

pub fn timestamp_fresh(ts: i64, now_unix: i64) -> bool {
    (now_unix - ts).abs() <= MAX_SKEW_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_match() {
        let sig = sign("secret", "n1", 1, b"ops");
        assert_eq!(sig, sign("secret", "n1", 1, b"ops"));
        assert_ne!(sig, sign("secret", "n2", 1, b"ops"));
    }

    #[test]
    fn constant_time_rejects_length_mismatch() {
        assert!(!constant_time_eq("aa", "a"));
        assert!(constant_time_eq("ab", "ab"));
        assert!(!constant_time_eq("ab", "ac"));
    }
}
