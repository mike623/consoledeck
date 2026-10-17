//! "password" actions store the password AES-256-GCM encrypted in deck.json, as base64 of
//! nonce + ciphertext + tag (the combined form CryptoKit uses, so the Swift app's passwords still work).
//! The key is a separate file next to deck.json, readable only by you. That keeps passwords
//! out of deck.json if it's shared or backed up, but anyone who can read both files can decrypt.

use crate::config::Config;
use aes_gcm::aead::{Aead, Generate, Key, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::path::PathBuf;

fn key_path() -> PathBuf {
    Config::path().with_file_name("secret.key")
}

/// Loads the key, or makes one the first time a password is saved.
pub fn password_key(create: bool) -> Option<Vec<u8>> {
    let path = key_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            // never replace a key that exists: every saved password would be lost
            let raw = B64.decode(s.trim()).ok().filter(|r| r.len() == 32);
            if raw.is_none() {
                log::error!("{} is not a valid key", path.display());
            }
            return raw;
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            log::error!("Cannot read {}: {e}", path.display());
            return None;
        }
        Err(_) if !create => return None,
        Err(_) => {}
    }
    let raw = Key::<Aes256Gcm>::generate().to_vec();
    let written = std::fs::create_dir_all(path.parent()?).and_then(|_| write_private(&path, B64.encode(&raw).as_bytes()));
    if let Err(e) = written {
        log::error!("Cannot write {}: {e}", path.display());
        return None;
    }
    Some(raw)
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?.write_all(data)
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, data) // the home folder is already private to the user
}

pub fn encrypt(password: &str, key: &[u8]) -> Option<String> {
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let nonce = Nonce::generate();
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(&nonce, password.as_bytes()).ok()?);
    Some(B64.encode(out))
}

pub fn decrypt(sealed: &str, key: &[u8]) -> Option<String> {
    let data = B64.decode(sealed).ok()?;
    if data.len() < 12 + 16 {
        return None;
    }
    let (nonce, ciphertext) = data.split_at(12);
    let nonce = Nonce::try_from(nonce).ok()?;
    let plain = Aes256Gcm::new_from_slice(key).ok()?.decrypt(&nonce, ciphertext).ok()?;
    String::from_utf8(plain).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_with_random_nonce() {
        let key = Key::<Aes256Gcm>::generate().to_vec();
        let sealed = encrypt("s3cret", &key).unwrap();
        assert!(!sealed.contains("s3cret"));
        assert_ne!(Some(&sealed), encrypt("s3cret", &key).as_ref());
        assert_eq!(decrypt(&sealed, &key).as_deref(), Some("s3cret"));
        assert_eq!(decrypt(&sealed, &Key::<Aes256Gcm>::generate()), None);
        assert_eq!(decrypt("not base64", &key), None);
        assert_eq!(decrypt("", &key), None);
    }

    #[test]
    fn decrypts_swift_cryptokit_output() {
        let key = B64.decode("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap();
        assert_eq!(decrypt("oTHlIr7/UsgauiaVDVmmW+V1DP/7Mlm1GQJqOlGz6lTazg==", &key).as_deref(), Some("s3cret"));
    }
}
