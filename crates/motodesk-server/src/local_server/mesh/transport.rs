//! What PCs of a shop say to one another on the network.
//!
//! Every message is encrypted and authenticated with AES-256-GCM under a key derived from the
//! network secret: customers, sales and account password hashes cross the shop's Wi-Fi, which
//! may not be the shop's alone. (Joining has its own sealing, under the SRP session key: the
//! new PC does not hold the secret yet. See `join`.)
//!
//! A message is `{"p": version, "box": base64(nonce || ciphertext)}`, and what is sealed is the
//! time it was sent and the fields. The direction is part of what is authenticated, so a reply
//! cannot be played back as a request. Replays are the receiver's business: [`open`] hands back
//! the message's nonce, which a server remembers.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use hkdf::Hkdf;
use rand::RngCore;
use serde_json::{json, Map, Value};
use sha2::Sha256;

use crate::local_server::error::{LocalError, LocalResult};
use crate::local_server::mesh_auth::timestamp_fresh;

/// The version of these exchanges, announced on the network. PCs of different versions cannot
/// talk and say so, instead of failing in silence.
pub const PROTOCOL: u32 = 3;

const NONCE_LEN: usize = 12;

/// Which way a message goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Request,
    Reply,
}

impl Kind {
    fn label(self) -> &'static [u8] {
        match self {
            Kind::Request => b"motodesk request",
            Kind::Reply => b"motodesk reply",
        }
    }
}

fn cipher(secret: &str) -> Aes256Gcm {
    let hk = Hkdf::<Sha256>::new(Some(b"motodesk-mesh-transport-v3"), secret.as_bytes());
    let mut key = [0u8; 32];
    hk.expand(b"aes-256-gcm", &mut key).expect("32 bytes is a valid HKDF length");
    Aes256Gcm::new_from_slice(&key).expect("32-byte key")
}

fn seal_at(secret: &str, kind: Kind, fields: Map<String, Value>, ts: i64) -> Value {
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let plain = json!({ "ts": ts, "f": fields }).to_string();
    let sealed = cipher(secret)
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain.as_bytes(), aad: kind.label() })
        .expect("encrypting in memory cannot fail");
    json!({ "p": PROTOCOL, "box": BASE64.encode([nonce.as_slice(), &sealed].concat()) })
}

pub fn seal(secret: &str, kind: Kind, fields: Map<String, Value>) -> Value {
    seal_at(secret, kind, fields, chrono::Utc::now().timestamp())
}

/// A message's fields and its nonce, if it was sealed with this secret, in this direction,
/// recently. Anything else is refused without saying why.
pub fn open(secret: &str, kind: Kind, message: &Value) -> LocalResult<(Map<String, Value>, String)> {
    let refused = || LocalError::Unauthorized;
    if message.get("p").and_then(Value::as_u64) != Some(u64::from(PROTOCOL)) {
        return Err(refused());
    }
    let raw = message
        .get("box")
        .and_then(Value::as_str)
        .and_then(|b| BASE64.decode(b).ok())
        .filter(|raw| raw.len() > NONCE_LEN)
        .ok_or_else(refused)?;
    let (nonce, sealed) = raw.split_at(NONCE_LEN);
    let plain = cipher(secret)
        .decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: kind.label() })
        .map_err(|_| refused())?;
    let mut value: Value = serde_json::from_slice(&plain).map_err(|_| refused())?;
    let ts = value.get("ts").and_then(Value::as_i64).ok_or_else(refused)?;
    if !timestamp_fresh(ts, chrono::Utc::now().timestamp()) {
        return Err(refused());
    }
    match value.get_mut("f").map(Value::take) {
        Some(Value::Object(fields)) => Ok((fields, hex::encode(nonce))),
        _ => Err(refused()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Map<String, Value> {
        json!({ "name": "Bernard", "hash": "$argon2id$secret" }).as_object().unwrap().clone()
    }

    #[test]
    fn a_message_opens_only_with_the_networks_secret_and_the_right_direction() {
        let sent = seal("network secret", Kind::Request, fields());
        assert_eq!(open("network secret", Kind::Request, &sent).unwrap().0, fields());
        assert!(open("another secret", Kind::Request, &sent).is_err());
        assert!(open("network secret", Kind::Reply, &sent).is_err());
    }

    #[test]
    fn nothing_readable_is_sent() {
        let wire = seal("network secret", Kind::Request, fields()).to_string();
        for word in ["Bernard", "argon2", "name", "hash", "network secret"] {
            assert!(!wire.contains(word), "{word} in {wire}");
        }
        assert_ne!(
            seal("s", Kind::Request, fields()),
            seal("s", Kind::Request, fields()),
            "a fresh nonce each time"
        );
    }

    #[test]
    fn altered_old_plain_and_other_version_messages_are_refused() {
        let sent = seal("s", Kind::Request, fields());
        let mut altered = sent.clone();
        let mut text = altered["box"].as_str().unwrap().to_string();
        let swap = if text.ends_with('A') { "B" } else { "A" };
        text.replace_range(text.len() - 5..text.len() - 4, swap);
        altered["box"] = text.into();
        let mut other = sent.clone();
        other["p"] = json!(2);
        let old = seal_at("s", Kind::Request, fields(), chrono::Utc::now().timestamp() - 3_600);
        for bad in [other, old, json!({ "ops": [] }), json!(null)] {
            assert!(open("s", Kind::Request, &bad).is_err(), "{bad}");
        }
        assert!(altered == sent || open("s", Kind::Request, &altered).is_err());
    }
}
