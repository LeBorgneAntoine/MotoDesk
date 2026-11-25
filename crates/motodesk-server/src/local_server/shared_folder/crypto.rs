//! Encryption of the shared folder's files.
//!
//! Envelope scheme:
//!
//! - **Shop key** (KEK): derived with HKDF-SHA256 from the network secret every PC of the shop
//!   holds. It is never stored; the secret already lives in the OS keychain and in encrypted
//!   backups. Deriving it means PCs need no extra key exchange, and a future key change only has
//!   to re-wrap the small per-file keys, not re-encrypt the files.
//! - **File key** (DEK): 32 random bytes per stored content, wrapped by the shop key.
//! - **Chunks**: a file is cut into fixed-size chunks, each sealed with AES-256-GCM under the file
//!   key and a fresh nonce. The authenticated data names the content, the chunk's position, the
//!   chunk count and the file size, so a chunk cannot be moved, repeated or dropped without the
//!   decryption failing.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

/// Plaintext bytes per chunk (the last one may be shorter).
pub const CHUNK_BYTES: usize = 1024 * 1024;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
/// The version stored with each wrapped key; bump it when the derivation changes.
pub const KEY_VERSION: i64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("the shop key is unavailable")]
    NoKey,
    #[error("the file could not be decrypted (wrong key or damaged content)")]
    Decrypt,
    #[error("malformed stored data")]
    Malformed,
}

/// The key every file key is wrapped with.
#[derive(Clone)]
pub struct ShopKey([u8; 32]);

impl ShopKey {
    /// Derive the shop key from the network secret. Fails when there is no secret yet.
    pub fn from_network_secret(secret: &str) -> Result<Self, CryptoError> {
        if secret.is_empty() {
            return Err(CryptoError::NoKey);
        }
        let hk = Hkdf::<Sha256>::new(Some(b"motodesk-shared-folder-v1"), secret.as_bytes());
        let mut key = [0u8; 32];
        hk.expand(b"shop-key", &mut key).map_err(|_| CryptoError::NoKey)?;
        Ok(Self(key))
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(&self.0).expect("32-byte key")
    }
}

/// A fresh random file key.
pub fn new_file_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut key);
    key
}

fn random_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    nonce
}

/// Wrap a file key for storage: `nonce || ciphertext`, bound to the content it belongs to.
pub fn wrap_file_key(shop: &ShopKey, content_id: &str, file_key: &[u8; 32]) -> Vec<u8> {
    let nonce = random_nonce();
    let sealed = shop
        .cipher()
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: file_key, aad: content_id.as_bytes() })
        .expect("encrypting in memory cannot fail");
    [nonce.as_slice(), &sealed].concat()
}

pub fn unwrap_file_key(shop: &ShopKey, content_id: &str, wrapped: &[u8]) -> Result<[u8; 32], CryptoError> {
    if wrapped.len() < NONCE_LEN + TAG_LEN {
        return Err(CryptoError::Malformed);
    }
    let (nonce, sealed) = wrapped.split_at(NONCE_LEN);
    let plain = shop
        .cipher()
        .decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: content_id.as_bytes() })
        .map_err(|_| CryptoError::Decrypt)?;
    plain.try_into().map_err(|_| CryptoError::Malformed)
}

/// What a chunk is bound to.
#[derive(Clone, Copy)]
pub struct ChunkContext<'a> {
    pub content_id: &'a str,
    pub chunk_count: u32,
    pub total_size: u64,
}

fn chunk_aad(ctx: &ChunkContext<'_>, index: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(ctx.content_id.len() + 16);
    aad.extend_from_slice(ctx.content_id.as_bytes());
    aad.extend_from_slice(&index.to_be_bytes());
    aad.extend_from_slice(&ctx.chunk_count.to_be_bytes());
    aad.extend_from_slice(&ctx.total_size.to_be_bytes());
    aad
}

/// Seal one chunk: `nonce || ciphertext || tag`.
pub fn seal_chunk(file_key: &[u8; 32], ctx: &ChunkContext<'_>, index: u32, plain: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(file_key).expect("32-byte key");
    let nonce = random_nonce();
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: &chunk_aad(ctx, index) })
        .expect("encrypting in memory cannot fail");
    [nonce.as_slice(), &sealed].concat()
}

pub fn open_chunk(
    file_key: &[u8; 32],
    ctx: &ChunkContext<'_>,
    index: u32,
    stored: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if stored.len() < NONCE_LEN + TAG_LEN {
        return Err(CryptoError::Malformed);
    }
    let cipher = Aes256Gcm::new_from_slice(file_key).expect("32-byte key");
    let (nonce, sealed) = stored.split_at(NONCE_LEN);
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: &chunk_aad(ctx, index) })
        .map_err(|_| CryptoError::Decrypt)
}

/// Number of chunks a file of `size` bytes is cut into (an empty file has none).
pub fn chunks_for(size: u64) -> u32 {
    size.div_ceil(CHUNK_BYTES as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(count: u32, size: u64) -> ChunkContext<'static> {
        ChunkContext { content_id: "content-1", chunk_count: count, total_size: size }
    }

    #[test]
    fn a_chunk_round_trips_and_hides_its_content() {
        let key = new_file_key();
        let plain = b"Bilan 2025 - confidentiel".to_vec();
        let stored = seal_chunk(&key, &ctx(1, plain.len() as u64), 0, &plain);
        assert!(!stored.windows(plain.len()).any(|w| w == plain.as_slice()));
        assert_eq!(open_chunk(&key, &ctx(1, plain.len() as u64), 0, &stored).unwrap(), plain);
    }

    #[test]
    fn two_seals_of_the_same_chunk_differ() {
        let key = new_file_key();
        let a = seal_chunk(&key, &ctx(1, 3), 0, b"abc");
        let b = seal_chunk(&key, &ctx(1, 3), 0, b"abc");
        assert_ne!(a, b);
    }

    #[test]
    fn a_chunk_cannot_be_moved_or_replayed_elsewhere() {
        let key = new_file_key();
        let stored = seal_chunk(&key, &ctx(3, 3_000_000), 1, b"middle");
        assert!(open_chunk(&key, &ctx(3, 3_000_000), 2, &stored).is_err(), "another position");
        assert!(open_chunk(&key, &ctx(2, 3_000_000), 1, &stored).is_err(), "another chunk count");
        assert!(open_chunk(&key, &ctx(3, 2_999_999), 1, &stored).is_err(), "another size");
        let other = ChunkContext { content_id: "content-2", chunk_count: 3, total_size: 3_000_000 };
        assert!(open_chunk(&key, &other, 1, &stored).is_err(), "another content");
        assert!(open_chunk(&key, &ctx(3, 3_000_000), 1, &stored).is_ok());
    }

    #[test]
    fn a_tampered_chunk_is_detected() {
        let key = new_file_key();
        let mut stored = seal_chunk(&key, &ctx(1, 5), 0, b"hello");
        let last = stored.len() - 1;
        stored[last] ^= 1;
        assert!(matches!(open_chunk(&key, &ctx(1, 5), 0, &stored), Err(CryptoError::Decrypt)));
        assert!(matches!(open_chunk(&key, &ctx(1, 5), 0, &[1, 2, 3]), Err(CryptoError::Malformed)));
    }

    #[test]
    fn a_file_key_unwraps_only_with_the_shops_key_and_its_content() {
        let shop = ShopKey::from_network_secret("secret-of-the-shop").unwrap();
        let other = ShopKey::from_network_secret("another-shop").unwrap();
        let key = new_file_key();
        let wrapped = wrap_file_key(&shop, "content-1", &key);
        assert_eq!(unwrap_file_key(&shop, "content-1", &wrapped).unwrap(), key);
        assert!(unwrap_file_key(&other, "content-1", &wrapped).is_err(), "another shop");
        assert!(unwrap_file_key(&shop, "content-2", &wrapped).is_err(), "another content");
    }

    #[test]
    fn the_shop_key_is_stable_and_needs_a_secret() {
        let a = ShopKey::from_network_secret("same").unwrap();
        let b = ShopKey::from_network_secret("same").unwrap();
        let wrapped = wrap_file_key(&a, "c", &[7u8; 32]);
        assert_eq!(unwrap_file_key(&b, "c", &wrapped).unwrap(), [7u8; 32]);
        assert!(matches!(ShopKey::from_network_secret(""), Err(CryptoError::NoKey)));
    }

    #[test]
    fn chunk_counts_follow_the_size() {
        assert_eq!(chunks_for(0), 0);
        assert_eq!(chunks_for(1), 1);
        assert_eq!(chunks_for(CHUNK_BYTES as u64), 1);
        assert_eq!(chunks_for(CHUNK_BYTES as u64 + 1), 2);
    }
}
