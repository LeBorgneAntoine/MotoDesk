//! Ed25519 signing of journal entries.
//!
//! Chaining alone proves internal consistency: it shows nobody edited a row
//! without rewriting everything after it. It does not prove the rows were
//! written by the software rather than reconstructed wholesale. § 100 of
//! BOI-TVA-DECLA-30-10-30 asks for low-level inalterability backed by "empreinte
//! numérique à clé privée, chaînage, etc.", so each register signs the digest of
//! every entry with a private key that never leaves the machine's keychain.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;

use crate::chain::FiscalEntry;
use crate::error::FiscalError;

/// A register's private signing key.
pub struct SecretKey(SigningKey);

/// A register's public key, safe to store in the database and ship in archives.
#[derive(Clone)]
pub struct PublicKey(VerifyingKey);

impl SecretKey {
    /// Create a fresh key. Called once, the first time a register is opened.
    pub fn generate() -> Self {
        Self(SigningKey::generate(&mut OsRng))
    }

    pub fn from_hex(hex_str: &str) -> Result<Self, FiscalError> {
        let raw = hex::decode(hex_str.trim()).map_err(|e| FiscalError::BadKey(e.to_string()))?;
        let bytes: [u8; 32] = raw
            .try_into()
            .map_err(|_| FiscalError::BadKey("expected 32 bytes".into()))?;
        Ok(Self(SigningKey::from_bytes(&bytes)))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(self.0.verifying_key())
    }

    /// Sign an entry's chain digest, returning the hex signature to store.
    pub fn sign_entry(&self, entry: &FiscalEntry) -> Result<String, FiscalError> {
        let digest = entry.chain_digest_bytes()?;
        Ok(hex::encode(self.0.sign(&digest).to_bytes()))
    }
}

impl PublicKey {
    pub fn from_hex(hex_str: &str) -> Result<Self, FiscalError> {
        let raw = hex::decode(hex_str.trim()).map_err(|e| FiscalError::BadKey(e.to_string()))?;
        let bytes: [u8; 32] = raw
            .try_into()
            .map_err(|_| FiscalError::BadKey("expected 32 bytes".into()))?;
        let key =
            VerifyingKey::from_bytes(&bytes).map_err(|e| FiscalError::BadKey(e.to_string()))?;
        Ok(Self(key))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    pub fn verify_entry(&self, entry: &FiscalEntry) -> Result<(), FiscalError> {
        let digest = entry.chain_digest_bytes()?;
        let raw = hex::decode(&entry.signature)
            .map_err(|_| FiscalError::BadSignature { seq: entry.seq })?;
        let bytes: [u8; 64] = raw
            .try_into()
            .map_err(|_| FiscalError::BadSignature { seq: entry.seq })?;
        self.0
            .verify(&digest, &Signature::from_bytes(&bytes))
            .map_err(|_| FiscalError::BadSignature { seq: entry.seq })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::{seal, verify_chain, ChainTip};
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    fn signed_chain(n: i64, key: &SecretKey) -> Vec<FiscalEntry> {
        let mut tip = ChainTip::genesis();
        let mut out = Vec::new();
        for i in 0..n {
            let mut e = seal(
                &tip,
                "C1",
                "sale",
                Utc.timestamp_opt(1_760_000_000 + i * 30, 0).unwrap(),
                json!({ "n": i }),
            )
            .unwrap();
            e.signature = key.sign_entry(&e).unwrap();
            tip = e.tip();
            out.push(e);
        }
        out
    }

    #[test]
    fn signs_and_verifies() {
        let key = SecretKey::generate();
        let entries = signed_chain(5, &key);
        let report = verify_chain(&entries, &ChainTip::genesis(), Some(&key.public())).unwrap();
        assert_eq!(report.signatures_checked, 5);
        assert_eq!(report.unsigned, 0);
    }

    #[test]
    fn another_key_cannot_impersonate_the_register() {
        let real = SecretKey::generate();
        let forger = SecretKey::generate();
        let entries = signed_chain(3, &forger);
        let err = verify_chain(&entries, &ChainTip::genesis(), Some(&real.public())).unwrap_err();
        assert_eq!(err.seq(), 1);
    }

    #[test]
    fn a_rebuilt_chain_without_the_key_fails_signature_check() {
        // Someone with database access rewrites history and recomputes every
        // hash correctly. Structure passes; signatures do not.
        let key = SecretKey::generate();
        let honest = signed_chain(3, &key);
        let mut forged = signed_chain(3, &SecretKey::generate());
        forged[2].payload = json!({ "n": 0 });

        assert!(verify_chain(&honest, &ChainTip::genesis(), Some(&key.public())).is_ok());
        assert!(verify_chain(&forged, &ChainTip::genesis(), Some(&key.public())).is_err());
    }

    #[test]
    fn keys_round_trip_through_hex() {
        let key = SecretKey::generate();
        let restored = SecretKey::from_hex(&key.to_hex()).unwrap();
        assert_eq!(key.public().to_hex(), restored.public().to_hex());

        let pubkey = PublicKey::from_hex(&key.public().to_hex()).unwrap();
        let entries = signed_chain(1, &key);
        assert!(pubkey.verify_entry(&entries[0]).is_ok());
    }

    #[test]
    fn rejects_malformed_keys() {
        assert!(SecretKey::from_hex("zz").is_err());
        assert!(SecretKey::from_hex("aabb").is_err());
        assert!(PublicKey::from_hex("").is_err());
    }
}
