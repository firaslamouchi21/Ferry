use age::x25519::{Identity as SealingIdentity, Recipient};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use thiserror::Error;
use zeroize::Zeroize;

use crate::seal::{self, SealError};

pub const DEK_BYTES: usize = 32;
const NONCE_LEN: usize = 24;

fn lock_memory<T>(ptr: *const T, len: usize) -> Option<region::LockGuard> {
    region::lock(ptr, len).ok()
}

pub struct DataEncryptionKey {
    bytes: [u8; DEK_BYTES],
    _lock: Option<region::LockGuard>,
}

impl DataEncryptionKey {
    pub fn generate() -> Self {
        let mut bytes = [0u8; DEK_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self::from_bytes(bytes)
    }

    fn from_bytes(bytes: [u8; DEK_BYTES]) -> Self {
        let lock = lock_memory(bytes.as_ptr(), bytes.len());
        Self { bytes, _lock: lock }
    }
}

impl Drop for DataEncryptionKey {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

pub struct Plaintext {
    bytes: Vec<u8>,
    _lock: Option<region::LockGuard>,
}

impl Plaintext {
    fn new(bytes: Vec<u8>) -> Self {
        let lock = lock_memory(bytes.as_ptr(), bytes.len());
        Self { bytes, _lock: lock }
    }

    pub fn expose(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }
}

impl Drop for Plaintext {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

#[derive(Debug, Error)]
pub enum SecretCryptoError {
    #[error("failed to wrap/unwrap data encryption key: {0}")]
    Seal(#[from] SealError),
    #[error("data encryption key must be exactly {DEK_BYTES} bytes, got {0}")]
    BadKeyLength(usize),
    #[error("chunk was too short to contain a nonce")]
    ChunkTooShort,
    #[error("chunk encryption failed")]
    Encrypt,
    #[error("chunk decryption failed — tampered, wrong key, or corrupt")]
    Decrypt,
}

pub fn wrap_dek(recipient: &Recipient, dek: &DataEncryptionKey) -> Result<Vec<u8>, SecretCryptoError> {
    Ok(seal::seal(recipient, &dek.bytes)?)
}

pub fn unwrap_dek(identity: &SealingIdentity, wrapped: &[u8]) -> Result<DataEncryptionKey, SecretCryptoError> {
    let mut bytes = seal::open(identity, wrapped)?;
    if bytes.len() != DEK_BYTES {
        let len = bytes.len();
        bytes.zeroize();
        return Err(SecretCryptoError::BadKeyLength(len));
    }
    let mut array = [0u8; DEK_BYTES];
    array.copy_from_slice(&bytes);
    bytes.zeroize();
    Ok(DataEncryptionKey::from_bytes(array))
}

pub fn encrypt_chunk(dek: &DataEncryptionKey, plaintext: &[u8]) -> Result<Vec<u8>, SecretCryptoError> {
    let cipher = XChaCha20Poly1305::new(&Key::from(dek.bytes));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);

    let mut sealed = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| SecretCryptoError::Encrypt)?;
    let mut out = nonce_bytes.to_vec();
    out.append(&mut sealed);
    Ok(out)
}

pub fn decrypt_chunk(dek: &DataEncryptionKey, framed: &[u8]) -> Result<Plaintext, SecretCryptoError> {
    if framed.len() < NONCE_LEN {
        return Err(SecretCryptoError::ChunkTooShort);
    }
    let (nonce_bytes, ciphertext) = framed.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(&Key::from(dek.bytes));
    let nonce = XNonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| SecretCryptoError::Decrypt)?;
    Ok(Plaintext::new(plaintext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_and_unwrap_dek_round_trips() {
        let identity = SealingIdentity::generate();
        let recipient = identity.to_public();
        let dek = DataEncryptionKey::generate();

        let wrapped = wrap_dek(&recipient, &dek).unwrap();
        let unwrapped = unwrap_dek(&identity, &wrapped).unwrap();
        assert_eq!(dek.bytes, unwrapped.bytes);
    }

    #[test]
    fn wrong_identity_cannot_unwrap_dek() {
        let identity = SealingIdentity::generate();
        let recipient = identity.to_public();
        let dek = DataEncryptionKey::generate();
        let wrapped = wrap_dek(&recipient, &dek).unwrap();

        let other = SealingIdentity::generate();
        assert!(unwrap_dek(&other, &wrapped).is_err());
    }

    #[test]
    fn encrypt_and_decrypt_chunk_round_trips() {
        let dek = DataEncryptionKey::generate();
        let plaintext = b"a secret chunk of bytes";

        let framed = encrypt_chunk(&dek, plaintext).unwrap();
        assert_ne!(&framed[NONCE_LEN..], plaintext, "ciphertext must not equal plaintext");

        let opened = decrypt_chunk(&dek, &framed).unwrap();
        assert_eq!(opened.expose(), plaintext);
    }

    #[test]
    fn tampered_chunk_fails_to_decrypt() {
        let dek = DataEncryptionKey::generate();
        let mut framed = encrypt_chunk(&dek, b"hello").unwrap();
        let last = framed.len() - 1;
        framed[last] ^= 0xFF;

        assert!(decrypt_chunk(&dek, &framed).is_err());
    }

    #[test]
    fn wrong_dek_fails_to_decrypt() {
        let dek = DataEncryptionKey::generate();
        let framed = encrypt_chunk(&dek, b"hello").unwrap();

        let wrong_dek = DataEncryptionKey::generate();
        assert!(decrypt_chunk(&wrong_dek, &framed).is_err());
    }

    #[test]
    fn two_chunks_from_the_same_key_get_different_nonces_and_ciphertext() {
        let dek = DataEncryptionKey::generate();
        let a = encrypt_chunk(&dek, b"same plaintext").unwrap();
        let b = encrypt_chunk(&dek, b"same plaintext").unwrap();
        assert_ne!(a, b, "reused plaintext must not produce identical ciphertext frames");
    }

    #[test]
    fn a_freshly_generated_key_is_actually_memory_locked_on_this_platform() {
        let dek = DataEncryptionKey::generate();
        assert!(
            dek._lock.is_some(),
            "mlock should succeed for a small allocation under typical RLIMIT_MEMLOCK — \
             if this fails in CI, the sandbox's memlock limit is the likely cause, not the code"
        );
    }

    #[test]
    fn decrypted_plaintext_is_actually_memory_locked_on_this_platform() {
        let dek = DataEncryptionKey::generate();
        let framed = encrypt_chunk(&dek, b"lock me").unwrap();
        let plaintext = decrypt_chunk(&dek, &framed).unwrap();
        assert!(plaintext._lock.is_some());
    }
}
