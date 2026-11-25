//! What guards a PC against replayed or stale network messages (`mesh::transport` seals them).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

const MAX_SKEW_SECS: i64 = 300;
const NONCE_TTL: Duration = Duration::from_secs(600);

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
    fn constant_time_rejects_length_mismatch() {
        assert!(!constant_time_eq("aa", "a"));
        assert!(constant_time_eq("ab", "ab"));
        assert!(!constant_time_eq("ab", "ac"));
    }
}
