//! AES-256-GCM for dataset secret values.
//!
//! This is not `crate::auth::secret`, which hashes passwords and API keys.
//! A secret value is reversible: an action handler reads it back.
//! The key never goes on disk. Each write draws a fresh 12-byte nonce, and
//! the ciphertext is bound to the dataset id and the declaration name, so a
//! row copied onto another name or dataset does not open.

use aes_gcm::aead::Aead;
use aes_gcm::aead::KeyInit;
use aes_gcm::aead::Payload;
use aes_gcm::Aes256Gcm;
use aes_gcm::Key;
use aes_gcm::Nonce;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// `LOCO_SECRET_KEY` as this process will use it.
///
/// `Ready` is not `Debug`-printed: the bytes are the key.
#[derive(Clone)]
pub enum KeyStatus {
    /// The variable is unset. Secret writes are 503; the process still boots
    /// so variable writes work.
    Missing,
    /// Set, but not standard base64 of 32 bytes. The string is the 503 body.
    Invalid(String),
    Ready([u8; 32]),
}

impl std::fmt::Debug for KeyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "Missing"),
            Self::Invalid(msg) => f.debug_tuple("Invalid").field(msg).finish(),
            Self::Ready(_) => write!(f, "Ready"),
        }
    }
}

impl KeyStatus {
    /// `None` is an unset `LOCO_SECRET_KEY`. A present value is parsed.
    /// A bad one stays [`Invalid`](Self::Invalid) so the process still boots.
    /// The environment is read by `crate::config`, not here.
    pub fn parse(raw: Option<&str>) -> Self {
        match raw {
            None => Self::Missing,
            Some(raw) => match parse_secret_key(raw) {
                Ok(key) => Self::Ready(key),
                Err(msg) => Self::Invalid(msg),
            },
        }
    }

    /// The key, or the 503 message. Both arms name `LOCO_SECRET_KEY`.
    pub fn require(&self) -> Result<&[u8; 32], String> {
        match self {
            Self::Ready(key) => Ok(key),
            Self::Missing => Err("LOCO_SECRET_KEY is not set".to_string()),
            Self::Invalid(msg) => Err(msg.clone()),
        }
    }
}

/// Standard base64 (RFC 4648, `+` `/` and `=` padding) of exactly 32 bytes.
/// `openssl rand -base64 32` prints that. Surrounding whitespace is ignored
/// so a trailing newline from a file does not fail the parse.
pub fn parse_secret_key(raw: &str) -> Result<[u8; 32], String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("LOCO_SECRET_KEY is empty; expected standard base64 of 32 bytes".to_string());
    }
    let bytes = STANDARD
        .decode(raw)
        .map_err(|_| "LOCO_SECRET_KEY is not standard base64 of 32 bytes".to_string())?;
    if bytes.len() != 32 {
        return Err(format!(
            "LOCO_SECRET_KEY must decode to 32 bytes (got {})",
            bytes.len()
        ));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

pub struct Sealed {
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

/// Wrong key, tampered bytes, or a body that is not a sealed secret.
/// One error for all of them: a caller must not be able to tell a wrong
/// key from garbage by the message. A missing row is `Ok(None)` from the
/// store, not an open failure.
#[derive(Debug)]
pub enum OpenError {
    Failed,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed => write!(f, "secret ciphertext failed to decrypt"),
        }
    }
}

impl std::error::Error for OpenError {}

/// Length-prefixed parts, so two different tuples cannot share an AAD.
/// `dataset_id` is `{account}/{project}/{dataset}`.
pub fn secret_aad(dataset_id: &str, name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for part in ["secret", dataset_id, name] {
        let bytes = part.as_bytes();
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(bytes);
    }
    out
}

pub fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Sealed, OpenError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    fill_nonce(&mut nonce_bytes)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| OpenError::Failed)?;
    Ok(Sealed {
        nonce: nonce_bytes,
        ciphertext,
    })
}

pub fn open(
    key: &[u8; 32],
    aad: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, OpenError> {
    if nonce.len() != 12 {
        return Err(OpenError::Failed);
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| OpenError::Failed)
}

fn fill_nonce(nonce: &mut [u8; 12]) -> Result<(), OpenError> {
    use aes_gcm::aead::rand_core::RngCore;
    aes_gcm::aead::OsRng
        .try_fill_bytes(nonce)
        .map_err(|_| OpenError::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_must_be_standard_base64_of_32_bytes() {
        let ok = STANDARD.encode([7u8; 32]);
        assert_eq!(parse_secret_key(&ok).unwrap(), [7u8; 32]);
        assert_eq!(parse_secret_key(&format!("\n{ok}\n")).unwrap(), [7u8; 32]);

        for bad in ["", "not base64!!!", &STANDARD.encode([0u8; 16]), "AAAA"] {
            let err = parse_secret_key(bad).unwrap_err();
            assert!(err.contains("LOCO_SECRET_KEY"), "{err}");
            assert!(err.contains("32"), "{err}");
        }
        // URL-safe alphabet is not the format we document. These bytes encode
        // with `+` `/` in standard and `-` `_` in URL-safe, so the two
        // strings differ. Bytes that need neither character would not.
        let url = base64::engine::general_purpose::URL_SAFE.encode([0xFBu8; 32]);
        assert_ne!(url, STANDARD.encode([0xFBu8; 32]));
        assert!(parse_secret_key(&url).is_err());
    }

    #[test]
    fn missing_key_names_the_variable() {
        let msg = KeyStatus::Missing.require().unwrap_err();
        assert!(msg.contains("LOCO_SECRET_KEY"), "{msg}");
    }

    #[test]
    fn wrong_key_and_moved_aad_fail_closed() {
        let key = [3u8; 32];
        let aad = secret_aad("alice/shop/dev", "alice/bricklink.consumer_key");
        let sealed = seal(&key, &aad, b"plain secret: do-not-store").unwrap();
        let opened = open(&key, &aad, &sealed.nonce, &sealed.ciphertext).unwrap();
        assert_eq!(opened, b"plain secret: do-not-store");

        let wrong = [4u8; 32];
        assert!(open(&wrong, &aad, &sealed.nonce, &sealed.ciphertext).is_err());

        let other = secret_aad("alice/shop/dev", "alice/bricklink.other");
        assert!(open(&key, &other, &sealed.nonce, &sealed.ciphertext).is_err());

        let mut flipped = sealed.ciphertext.clone();
        flipped[0] ^= 0xff;
        assert!(open(&key, &aad, &sealed.nonce, &flipped).is_err());
    }

    #[test]
    fn two_seals_use_different_nonces() {
        let key = [5u8; 32];
        let aad = secret_aad("a/b/dev", "k");
        let a = seal(&key, &aad, b"same").unwrap();
        let b = seal(&key, &aad, b"same").unwrap();
        assert_ne!(a.nonce, b.nonce);
    }
}
