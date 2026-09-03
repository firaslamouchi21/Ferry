use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use spake2::{Ed25519Group, Identity as PairingIdentity, Password, Spake2};
use thiserror::Error;

const PAIRING_IDENTITY_LABEL: &[u8] = b"ferry-pairing";
const CONFIRMATION_KEY_INFO: &[u8] = b"ferry-pairing-confirmation-v1";
const NONCE_LEN: usize = 24;

#[derive(Debug, Error)]
pub enum PairingError {
    #[error("pairing exchange failed — codes did not match or the message was corrupt")]
    ExchangeFailed,
    #[error("confirmation channel key derivation failed")]
    KeyDerivation,
    #[error("failed to encrypt confirmation payload")]
    Encrypt,
    #[error("failed to decrypt confirmation payload — tampered or wrong channel")]
    Decrypt,
}

pub struct PairingSession {
    spake: Spake2<Ed25519Group>,
}

pub struct ConfirmedChannel {
    key: Key,
}

impl PairingSession {
    pub fn start(short_code: &[u8]) -> (Self, Vec<u8>) {
        let password = Password::new(short_code);
        let identity = PairingIdentity::new(PAIRING_IDENTITY_LABEL);
        let (spake, outbound_message) = Spake2::<Ed25519Group>::start_symmetric(&password, &identity);
        (Self { spake }, outbound_message)
    }

    pub fn finish(self, inbound_message: &[u8]) -> Result<ConfirmedChannel, PairingError> {
        let shared_secret = self
            .spake
            .finish(inbound_message)
            .map_err(|_| PairingError::ExchangeFailed)?;

        let hk = Hkdf::<Sha256>::new(None, &shared_secret);
        let mut key_bytes = [0u8; 32];
        hk.expand(CONFIRMATION_KEY_INFO, &mut key_bytes)
            .map_err(|_| PairingError::KeyDerivation)?;

        Ok(ConfirmedChannel {
            key: Key::from(key_bytes),
        })
    }
}

impl ConfirmedChannel {
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, PairingError> {
        let cipher = XChaCha20Poly1305::new(&self.key);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);

        let mut sealed = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| PairingError::Encrypt)?;
        let mut out = nonce_bytes.to_vec();
        out.append(&mut sealed);
        Ok(out)
    }

    pub fn decrypt(&self, sealed: &[u8]) -> Result<Vec<u8>, PairingError> {
        if sealed.len() < NONCE_LEN {
            return Err(PairingError::Decrypt);
        }
        let (nonce_bytes, ciphertext) = sealed.split_at(NONCE_LEN);
        let cipher = XChaCha20Poly1305::new(&self.key);
        let nonce = XNonce::from_slice(nonce_bytes);
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| PairingError::Decrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_codes_derive_the_same_channel() {
        let code = b"482913";
        let (a, a_msg) = PairingSession::start(code);
        let (b, b_msg) = PairingSession::start(code);

        let a_channel = a.finish(&b_msg).unwrap();
        let b_channel = b.finish(&a_msg).unwrap();

        let sealed = a_channel.encrypt(b"identity pubkey bytes").unwrap();
        let opened = b_channel.decrypt(&sealed).unwrap();
        assert_eq!(opened, b"identity pubkey bytes");
    }

    #[test]
    fn mismatched_codes_derive_different_channels() {
        let (a, a_msg) = PairingSession::start(b"111111");
        let (b, b_msg) = PairingSession::start(b"222222");

        let a_channel = a.finish(&b_msg).unwrap();
        let b_channel = b.finish(&a_msg).unwrap();

        let sealed = a_channel.encrypt(b"payload").unwrap();
        assert!(b_channel.decrypt(&sealed).is_err());
    }
}
