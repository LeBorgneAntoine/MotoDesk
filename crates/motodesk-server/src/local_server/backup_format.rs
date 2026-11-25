//! Encrypted backup container, format 2.
//!
//! ```text
//! "MGBAK2\0\0" | iterations u32 | salt 16 | nonce prefix 8 | chunk*
//! chunk = length u32 (top bit = last chunk) | AES-256-GCM ciphertext + tag
//! ```
//!
//! The *whole* archive is encrypted, not just the keys inside it: the database
//! holds every customer, sale and payment. The key comes from the password
//! through PBKDF2-HMAC-SHA256 with a random salt and a high iteration count (the
//! count is stored in the header so it can be raised later). Data is encrypted in
//! 1 MiB chunks, each with its own nonce and an "is this the last chunk" flag as
//! authenticated data, so a database of several gigabytes streams through in
//! constant memory and a truncated or reordered file is detected.
//!
//! Inside the encryption, a gzip stream carries a simple list of named entries
//! (see [`write_entry`] and [`next_entry`]).

use std::io::{self, Read, Write};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;

pub const MAGIC: &[u8; 8] = b"MGBAK2\0\0";
/// OWASP's current floor for PBKDF2-HMAC-SHA256.
pub const DEFAULT_ITERATIONS: u32 = 600_000;
const MAX_ITERATIONS: u32 = 20_000_000;
const CHUNK: usize = 1 << 20;
const TAG: usize = 16;
const LAST_FLAG: u32 = 0x8000_0000;

type HmacSha256 = Hmac<Sha256>;

/// PBKDF2-HMAC-SHA256, one 32-byte block.
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let base = <HmacSha256 as Mac>::new_from_slice(password).expect("HMAC accepts any key length");
    let mut first = base.clone();
    first.update(salt);
    first.update(&1u32.to_be_bytes());
    let mut u: [u8; 32] = first.finalize().into_bytes().into();
    let mut t = u;
    for _ in 1..iterations {
        let mut mac = base.clone();
        mac.update(&u);
        u = mac.finalize().into_bytes().into();
        for (a, b) in t.iter_mut().zip(u.iter()) {
            *a ^= b;
        }
    }
    t
}

fn cipher_for(password: &str, salt: &[u8], iterations: u32) -> Aes256Gcm {
    let key = pbkdf2_sha256(password.as_bytes(), salt, iterations);
    Aes256Gcm::new_from_slice(&key).expect("a 32-byte key")
}

fn nonce(prefix: &[u8; 8], counter: u32) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(prefix);
    n[8..].copy_from_slice(&counter.to_be_bytes());
    n
}

fn other(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

// ---------------------------------------------------------------- writing ---

pub struct EncryptWriter<W: Write> {
    inner: W,
    cipher: Aes256Gcm,
    prefix: [u8; 8],
    counter: u32,
    buf: Vec<u8>,
}

impl<W: Write> EncryptWriter<W> {
    pub fn new(mut inner: W, password: &str, iterations: u32) -> io::Result<Self> {
        let mut salt = [0u8; 16];
        let mut prefix = [0u8; 8];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        rand::rngs::OsRng.fill_bytes(&mut prefix);
        let cipher = cipher_for(password, &salt, iterations);
        inner.write_all(MAGIC)?;
        inner.write_all(&iterations.to_be_bytes())?;
        inner.write_all(&salt)?;
        inner.write_all(&prefix)?;
        Ok(Self { inner, cipher, prefix, counter: 0, buf: Vec::with_capacity(CHUNK + 4096) })
    }

    fn seal(&mut self, last: bool) -> io::Result<()> {
        let n = if last { self.buf.len() } else { CHUNK };
        let chunk: Vec<u8> = self.buf.drain(..n).collect();
        let nonce_bytes = nonce(&self.prefix, self.counter);
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), Payload { msg: &chunk, aad: &[u8::from(last)] })
            .map_err(|_| other("chiffrement impossible"))?;
        let len = u32::try_from(ct.len()).map_err(|_| other("bloc trop grand"))?
            | if last { LAST_FLAG } else { 0 };
        self.inner.write_all(&len.to_be_bytes())?;
        self.inner.write_all(&ct)?;
        self.counter = self.counter.checked_add(1).ok_or_else(|| other("archive trop volumineuse"))?;
        Ok(())
    }

    /// Write the final chunk. Without this the archive is truncated and will not open.
    pub fn finish(mut self) -> io::Result<W> {
        self.seal(true)?;
        self.inner.flush()?;
        Ok(self.inner)
    }
}

impl<W: Write> Write for EncryptWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        // Strictly more than one chunk: the last chunk is held back so `finish`
        // can mark it as the last.
        while self.buf.len() > CHUNK {
            self.seal(false)?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------- reading ---

#[derive(Debug)]
pub enum OpenError {
    /// Not one of our archives (wrong magic).
    NotABackup,
    /// The password does not open it.
    BadPassword,
    Corrupt(String),
    Io(io::Error),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NotABackup => write!(f, "ce fichier n'est pas une sauvegarde Motodesk"),
            OpenError::BadPassword => write!(f, "mot de passe de restauration incorrect, ou fichier de sauvegarde endommagé"),
            OpenError::Corrupt(m) => write!(f, "sauvegarde corrompue : {m}"),
            OpenError::Io(e) => write!(f, "lecture impossible : {e}"),
        }
    }
}

enum ChunkError {
    Auth,
    Truncated,
    Format(&'static str),
    Io(io::Error),
}

pub struct DecryptReader<R: Read> {
    inner: R,
    cipher: Aes256Gcm,
    prefix: [u8; 8],
    counter: u32,
    plain: Vec<u8>,
    pos: usize,
    last_seen: bool,
    trailing_checked: bool,
}

impl<R: Read> DecryptReader<R> {
    /// Read the header, derive the key and open the first chunk, so that a wrong
    /// password is reported here and not halfway through a restore.
    pub fn open(mut inner: R, password: &str) -> Result<Self, OpenError> {
        let mut magic = [0u8; 8];
        inner.read_exact(&mut magic).map_err(|_| OpenError::NotABackup)?;
        if &magic != MAGIC {
            return Err(OpenError::NotABackup);
        }
        let mut it = [0u8; 4];
        let mut salt = [0u8; 16];
        let mut prefix = [0u8; 8];
        for part in [&mut it[..], &mut salt[..], &mut prefix[..]] {
            inner.read_exact(part).map_err(|_| OpenError::Corrupt("en-tête incomplet".into()))?;
        }
        let iterations = u32::from_be_bytes(it);
        if iterations == 0 || iterations > MAX_ITERATIONS {
            return Err(OpenError::Corrupt("paramètres de chiffrement invalides".into()));
        }
        let cipher = cipher_for(password, &salt, iterations);
        let mut reader = Self {
            inner,
            cipher,
            prefix,
            counter: 0,
            plain: Vec::new(),
            pos: 0,
            last_seen: false,
            trailing_checked: false,
        };
        match reader.next_chunk() {
            Ok(()) => Ok(reader),
            Err(ChunkError::Auth) => Err(OpenError::BadPassword),
            Err(ChunkError::Truncated) => Err(OpenError::Corrupt("fichier tronqué".into())),
            Err(ChunkError::Format(m)) => Err(OpenError::Corrupt(m.into())),
            Err(ChunkError::Io(e)) => Err(OpenError::Io(e)),
        }
    }

    fn next_chunk(&mut self) -> Result<(), ChunkError> {
        let mut len_bytes = [0u8; 4];
        self.inner.read_exact(&mut len_bytes).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                ChunkError::Truncated
            } else {
                ChunkError::Io(e)
            }
        })?;
        let raw = u32::from_be_bytes(len_bytes);
        let last = raw & LAST_FLAG != 0;
        let len = (raw & !LAST_FLAG) as usize;
        if len < TAG || len > CHUNK + TAG {
            return Err(ChunkError::Format("taille de bloc invalide"));
        }
        let mut ct = vec![0u8; len];
        self.inner.read_exact(&mut ct).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                ChunkError::Truncated
            } else {
                ChunkError::Io(e)
            }
        })?;
        let nonce_bytes = nonce(&self.prefix, self.counter);
        self.plain = self
            .cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), Payload { msg: &ct, aad: &[u8::from(last)] })
            .map_err(|_| ChunkError::Auth)?;
        self.pos = 0;
        self.counter = self.counter.checked_add(1).ok_or(ChunkError::Format("trop de blocs"))?;
        self.last_seen = last;
        Ok(())
    }
}

impl<R: Read> Read for DecryptReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if self.pos < self.plain.len() {
                let n = out.len().min(self.plain.len() - self.pos);
                out[..n].copy_from_slice(&self.plain[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            if self.last_seen {
                if !self.trailing_checked {
                    self.trailing_checked = true;
                    let mut probe = [0u8; 1];
                    if self.inner.read(&mut probe)? != 0 {
                        return Err(other("données après la fin de la sauvegarde"));
                    }
                }
                return Ok(0);
            }
            self.next_chunk().map_err(|e| match e {
                ChunkError::Auth => other("sauvegarde corrompue ou modifiée"),
                ChunkError::Truncated => other("sauvegarde tronquée"),
                ChunkError::Format(m) => other(m),
                ChunkError::Io(e) => e,
            })?;
        }
    }
}

// --------------------------------------------------------------- container ---

/// One named entry: `name_len u16 | name | data_len u64 | data`.
pub fn write_entry<W: Write, R: Read>(w: &mut W, name: &str, len: u64, data: &mut R) -> io::Result<()> {
    let name_bytes = name.as_bytes();
    let name_len = u16::try_from(name_bytes.len()).map_err(|_| other("nom trop long"))?;
    if name_len == 0 {
        return Err(other("nom vide"));
    }
    w.write_all(&name_len.to_be_bytes())?;
    w.write_all(name_bytes)?;
    w.write_all(&len.to_be_bytes())?;
    let copied = io::copy(&mut data.take(len), w)?;
    if copied != len {
        return Err(other("le fichier a changé pendant la sauvegarde"));
    }
    Ok(())
}

pub fn write_bytes_entry<W: Write>(w: &mut W, name: &str, bytes: &[u8]) -> io::Result<()> {
    write_entry(w, name, bytes.len() as u64, &mut &bytes[..])
}

/// The marker after the last entry.
pub fn write_end<W: Write>(w: &mut W) -> io::Result<()> {
    w.write_all(&0u16.to_be_bytes())
}

#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub len: u64,
}

/// Read the next entry header; the caller then consumes exactly `len` bytes.
pub fn next_entry<R: Read>(r: &mut R) -> io::Result<Option<Entry>> {
    let mut nl = [0u8; 2];
    r.read_exact(&mut nl)?;
    let name_len = u16::from_be_bytes(nl) as usize;
    if name_len == 0 {
        return Ok(None);
    }
    if name_len > 512 {
        return Err(other("nom d'entrée invalide"));
    }
    let mut name = vec![0u8; name_len];
    r.read_exact(&mut name)?;
    let mut len = [0u8; 8];
    r.read_exact(&mut len)?;
    let name = String::from_utf8(name).map_err(|_| other("nom d'entrée invalide"))?;
    Ok(Some(Entry { name, len: u64::from_be_bytes(len) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Few iterations keep the tests fast; the format reads the count from the header.
    const FAST: u32 = 2;

    fn seal_bytes(password: &str, payload: &[u8]) -> Vec<u8> {
        let mut enc = EncryptWriter::new(Vec::new(), password, FAST).unwrap();
        enc.write_all(payload).unwrap();
        enc.finish().unwrap()
    }

    fn open_all(blob: &[u8], password: &str) -> Result<Vec<u8>, OpenError> {
        let mut r = DecryptReader::open(blob, password)?;
        let mut out = Vec::new();
        r.read_to_end(&mut out).map_err(OpenError::Io)?;
        Ok(out)
    }

    #[test]
    fn pbkdf2_matches_the_published_test_vectors() {
        let one = pbkdf2_sha256(b"password", b"salt", 1);
        assert_eq!(hex::encode(one), "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
        let two = pbkdf2_sha256(b"password", b"salt", 2);
        assert_eq!(hex::encode(two), "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43");
    }

    #[test]
    fn data_round_trips_across_chunk_boundaries() {
        for size in [0usize, 1, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK, 2 * CHUNK + 777] {
            let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let blob = seal_bytes("un mot de passe", &payload);
            assert_eq!(open_all(&blob, "un mot de passe").unwrap(), payload, "size {size}");
        }
    }

    #[test]
    fn nothing_readable_is_left_in_the_file() {
        let secret = b"Martin Jean 06 12 34 56 78 FA-2026-C1-00001";
        let blob = seal_bytes("un mot de passe", secret);
        assert!(!blob.windows(secret.len()).any(|w| w == secret));
        assert!(!blob.windows(6).any(|w| w == b"Martin"));
    }

    #[test]
    fn a_wrong_password_is_reported_as_such() {
        let blob = seal_bytes("le bon mot de passe", b"donnees");
        assert!(matches!(open_all(&blob, "un autre"), Err(OpenError::BadPassword)));
    }

    #[test]
    fn two_archives_of_the_same_data_differ() {
        assert_ne!(seal_bytes("pw pw pw pw", b"x"), seal_bytes("pw pw pw pw", b"x"));
    }

    #[test]
    fn a_tampered_byte_is_detected() {
        let payload = vec![7u8; 3 * CHUNK];
        let mut blob = seal_bytes("pw pw pw pw", &payload);
        let mid = blob.len() / 2;
        blob[mid] ^= 0x01;
        assert!(open_all(&blob, "pw pw pw pw").is_err());
    }

    #[test]
    fn a_truncated_archive_is_detected_even_on_a_chunk_boundary() {
        let payload = vec![9u8; 3 * CHUNK + 5];
        let blob = seal_bytes("pw pw pw pw", &payload);
        // Cut after the second chunk: every remaining chunk is individually valid.
        let header = 8 + 4 + 16 + 8;
        let chunk_on_disk = 4 + CHUNK + TAG;
        let cut = &blob[..header + 2 * chunk_on_disk];
        assert!(open_all(cut, "pw pw pw pw").is_err());
        assert!(open_all(&blob[..blob.len() - 3], "pw pw pw pw").is_err());
    }

    #[test]
    fn trailing_junk_is_refused() {
        let mut blob = seal_bytes("pw pw pw pw", b"data");
        blob.extend_from_slice(b"extra");
        assert!(open_all(&blob, "pw pw pw pw").is_err());
    }

    #[test]
    fn other_files_are_not_mistaken_for_backups() {
        assert!(matches!(open_all(b"PK\x03\x04 a zip", "x"), Err(OpenError::NotABackup)));
        assert!(matches!(open_all(b"", "x"), Err(OpenError::NotABackup)));
    }

    #[test]
    fn entries_round_trip_in_order() {
        let mut buf = Vec::new();
        write_bytes_entry(&mut buf, "manifest.json", b"{}").unwrap();
        write_bytes_entry(&mut buf, "local.db", &vec![5u8; 10_000]).unwrap();
        write_bytes_entry(&mut buf, "archives/2026.csv", b"a;b").unwrap();
        write_end(&mut buf).unwrap();

        let mut r = &buf[..];
        let mut names = Vec::new();
        while let Some(e) = next_entry(&mut r).unwrap() {
            let mut data = Vec::new();
            (&mut r).take(e.len).read_to_end(&mut data).unwrap();
            assert_eq!(data.len() as u64, e.len);
            names.push((e.name, data.len()));
        }
        assert_eq!(
            names,
            vec![("manifest.json".into(), 2), ("local.db".into(), 10_000), ("archives/2026.csv".into(), 3)]
        );
    }
}
