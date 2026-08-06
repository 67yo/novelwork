//! Local encryption for DeepSeek API key at rest (AES-256-GCM).
//! Key is machine-bound (hostname + user + app salt); stolen DB alone is not enough.
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};

const PREFIX: &str = "nw1:";
const APP_SALT: &[u8] = b"nove-work-deepseek-key-v1";

fn machine_key() -> [u8; 32] {
    let host = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "unknown-host".into());
    let user = whoami::username().unwrap_or_else(|_| "unknown-user".into());
    let mut hasher = Sha256::new();
    hasher.update(APP_SALT);
    hasher.update(host.as_bytes());
    hasher.update(b"|");
    hasher.update(user.as_bytes());
    hasher.finalize().into()
}

/// Encrypt plaintext API key for SQLite storage.
pub fn encrypt_secret(plain: &str) -> Result<String> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    if plain.starts_with(PREFIX) {
        return Ok(plain.to_string());
    }
    let key = machine_key();
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow!(e))?;
    let mut nonce_bytes = [0u8; 12];
    getrandom::fill(&mut nonce_bytes).map_err(|e| anyhow!(e))?;
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).map_err(|_| anyhow!("bad nonce"))?;
    let ct = cipher
        .encrypt(&nonce, plain.as_bytes())
        .map_err(|_| anyhow!("encrypt api key failed"))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(format!("{PREFIX}{}", hex::encode(out)))
}

/// Decrypt stored value; migrates legacy plaintext by returning as-is.
pub fn decrypt_secret(stored: &str) -> Result<String> {
    if stored.is_empty() {
        return Ok(String::new());
    }
    let Some(hex_body) = stored.strip_prefix(PREFIX) else {
        // legacy plaintext
        return Ok(stored.to_string());
    };
    let raw = hex::decode(hex_body).map_err(|e| anyhow!("corrupt key blob: {e}"))?;
    if raw.len() < 13 {
        return Err(anyhow!("corrupt key blob: too short"));
    }
    let (nonce_bytes, ct) = raw.split_at(12);
    let key = machine_key();
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow!(e))?;
    let nonce = Nonce::try_from(nonce_bytes).map_err(|_| anyhow!("bad nonce"))?;
    let pt = cipher
        .decrypt(&nonce, ct)
        .map_err(|_| anyhow!("decrypt api key failed (wrong machine?)"))?;
    String::from_utf8(pt).map_err(|e| anyhow!(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let e = encrypt_secret("sk-test-secret").unwrap();
        assert!(e.starts_with(PREFIX));
        assert_eq!(decrypt_secret(&e).unwrap(), "sk-test-secret");
    }

    #[test]
    fn legacy_plaintext() {
        assert_eq!(decrypt_secret("sk-plain").unwrap(), "sk-plain");
    }
}
