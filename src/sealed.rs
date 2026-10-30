//! Authenticated local-file encryption with fixed, bounded Argon2id parameters.
use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
const AAD: &[u8] = b"Proxmark Studio encrypted file v1";
const LIMIT: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub schema: u8,
    pub salt: [u8; 16],
    pub nonce: [u8; 12],
    pub ciphertext: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Library {
    pub cards: Vec<crate::basic::SavedCard>,
    pub images: Vec<crate::emulator::Image>,
}
fn key(pass: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, String> {
    if pass.len() < 12 || pass.len() > 1024 {
        return Err("Use a passphrase of 12–1024 UTF-8 bytes".into());
    }
    let mut key = Zeroizing::new([0u8; 32]);
    let params = Params::new(19456, 2, 1, Some(32)).map_err(|e| e.to_string())?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(pass.as_bytes(), salt, &mut *key)
        .map_err(|e| e.to_string())?;
    Ok(key)
}
pub fn seal(bytes: &[u8], pass: &str) -> Result<Envelope, String> {
    if bytes.len() > LIMIT {
        return Err("Plaintext exceeds 8 MiB".into());
    }
    let mut salt = [0; 16];
    let mut nonce = [0; 12];
    OsRng
        .try_fill_bytes(&mut salt)
        .map_err(|_| "Random generation failed")?;
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| "Random generation failed")?;
    let key = key(pass, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&*key).map_err(|_| "Cipher setup failed")?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: bytes,
                aad: AAD,
            },
        )
        .map_err(|_| "Encryption failed")?;
    Ok(Envelope {
        schema: 1,
        salt,
        nonce,
        ciphertext: crate::command::encode(&ciphertext),
    })
}
pub fn open(e: &Envelope, pass: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    if e.schema != 1
        || e.ciphertext.len() > 2 * (LIMIT + 16)
        || e.ciphertext.len() < 32
        || !e.ciphertext.len().is_multiple_of(2)
    {
        return Err("Unsupported or oversized encrypted file".into());
    }
    let encrypted = e
        .ciphertext
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| {
            std::str::from_utf8(b)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
                .ok_or("Malformed ciphertext")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let key = key(pass, &e.salt)?;
    let cipher = Aes256Gcm::new_from_slice(&*key).map_err(|_| "Cipher setup failed")?;
    cipher
        .decrypt(
            Nonce::from_slice(&e.nonce),
            Payload {
                msg: &encrypted,
                aad: AAD,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| "Incorrect passphrase or altered file; nothing was loaded".into())
}
pub fn save(path: &std::path::Path, bytes: &[u8], pass: &str) -> Result<(), String> {
    crate::storage::write_private(
        path,
        &serde_json::to_vec_pretty(&seal(bytes, pass)?).map_err(|e| e.to_string())?,
    )
}
pub fn load(path: &std::path::Path, pass: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let bytes = crate::storage::read_bounded(path, (LIMIT * 2 + 4096) as u64)?;
    let e: Envelope = serde_json::from_slice(&bytes).map_err(|_| "Not a Studio encrypted file")?;
    open(&e, pass)
}
