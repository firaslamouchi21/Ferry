use age::secrecy::ExposeSecret;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const KEYCHAIN_SERVICE: &str = "dev.ferry.identity";
const KEYCHAIN_USER: &str = "identity";
const FINGERPRINT_BYTES: usize = 8;

#[derive(Clone)]
pub struct Identity {
    signing_key: SigningKey,
    sealing_key: age::x25519::Identity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicIdentity {
    pub signing_key: [u8; 32],
    pub sealing_key: String,
}

impl PublicIdentity {
    pub fn sealing_key_x25519_bytes(&self) -> Result<[u8; 32], IdentityError> {
        parse_x25519_recipient_bytes(&self.sealing_key)
    }
}

pub fn parse_x25519_recipient_bytes(sealing_key: &str) -> Result<[u8; 32], IdentityError> {
    bech32_payload_32(sealing_key)
}

fn bech32_payload_32(s: &str) -> Result<[u8; 32], IdentityError> {
    let (_, data) =
        bech32::decode(s).map_err(|e| IdentityError::Corrupt(format!("not valid bech32: {e}")))?;
    data.try_into()
        .map_err(|_| IdentityError::Corrupt("expected a 32-byte payload".into()))
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("keychain error: {0}")]
    Keychain(#[from] keyring::Error),
    #[error("stored identity material is corrupt: {0}")]
    Corrupt(String),
}

impl Identity {
    pub fn generate() -> Self {
        Self {
            signing_key: SigningKey::generate(&mut OsRng),
            sealing_key: age::x25519::Identity::generate(),
        }
    }

    pub fn public(&self) -> PublicIdentity {
        PublicIdentity {
            signing_key: self.signing_key.verifying_key().to_bytes(),
            sealing_key: self.sealing_key.to_public().to_string(),
        }
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn sealing_identity(&self) -> &age::x25519::Identity {
        &self.sealing_key
    }

    pub fn sealing_key_x25519_bytes(&self) -> [u8; 32] {
        bech32_payload_32(self.sealing_key.to_string().expose_secret())
            .expect("a locally generated identity's own sealing key must decode")
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }

    pub fn fingerprint(&self) -> String {
        fingerprint_of(&self.verifying_key())
    }

    fn to_storage_string(&self) -> String {
        format!(
            "{}\n{}",
            hex::encode(self.signing_key.to_bytes()),
            self.sealing_key.to_string().expose_secret()
        )
    }

    fn from_storage_string(raw: &str) -> Result<Self, IdentityError> {
        let mut lines = raw.lines();
        let signing_hex = lines
            .next()
            .ok_or_else(|| IdentityError::Corrupt("missing signing key line".into()))?;
        let sealing_line = lines
            .next()
            .ok_or_else(|| IdentityError::Corrupt("missing sealing key line".into()))?;

        let signing_bytes: [u8; 32] = hex::decode(signing_hex)
            .map_err(|e| IdentityError::Corrupt(format!("signing key not valid hex: {e}")))?
            .try_into()
            .map_err(|_| IdentityError::Corrupt("signing key is not 32 bytes".into()))?;
        let signing_key = SigningKey::from_bytes(&signing_bytes);

        let sealing_key: age::x25519::Identity = sealing_line
            .parse()
            .map_err(|e: &str| IdentityError::Corrupt(format!("sealing key: {e}")))?;

        Ok(Self {
            signing_key,
            sealing_key,
        })
    }

    pub fn load_or_generate() -> Result<Self, IdentityError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)?;
        match entry.get_secret() {
            Ok(bytes) => {
                let raw = String::from_utf8(bytes)
                    .map_err(|e| IdentityError::Corrupt(format!("not valid utf-8: {e}")))?;
                Self::from_storage_string(&raw)
            }
            Err(keyring::Error::NoEntry) => {
                let identity = Self::generate();
                entry.set_secret(identity.to_storage_string().as_bytes())?;
                Ok(identity)
            }
            Err(e) => Err(IdentityError::Keychain(e)),
        }
    }
}

pub fn fingerprint_of(key: &VerifyingKey) -> String {
    let digest = Sha256::digest(key.as_bytes());
    hex::encode(&digest[..FINGERPRINT_BYTES])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    #[test]
    fn generated_identity_round_trips_through_storage_string() {
        let identity = Identity::generate();
        let raw = identity.to_storage_string();
        let restored = Identity::from_storage_string(&raw).unwrap();
        assert_eq!(
            identity.verifying_key().to_bytes(),
            restored.verifying_key().to_bytes()
        );
        assert_eq!(identity.public().sealing_key, restored.public().sealing_key);
    }

    #[test]
    fn signature_verifies_against_verifying_key() {
        let identity = Identity::generate();
        let message = b"ferry offer envelope";
        let signature = identity.sign(message);
        assert!(identity.verifying_key().verify(message, &signature).is_ok());
    }

    #[test]
    fn tampered_message_fails_verification() {
        let identity = Identity::generate();
        let signature = identity.sign(b"original");
        assert!(identity
            .verifying_key()
            .verify(b"tampered", &signature)
            .is_err());
    }

    #[test]
    fn sealing_key_x25519_bytes_are_deterministic_and_identity_dependent() {
        let a = Identity::generate();
        let b = Identity::generate();

        assert_eq!(a.sealing_key_x25519_bytes(), a.sealing_key_x25519_bytes());
        assert_ne!(a.sealing_key_x25519_bytes(), b.sealing_key_x25519_bytes());

        let a_public = a.public().sealing_key_x25519_bytes().unwrap();
        assert_ne!(
            a_public,
            a.sealing_key_x25519_bytes(),
            "public and private key bytes must differ"
        );
    }

    #[test]
    fn parse_x25519_recipient_bytes_rejects_garbage() {
        assert!(parse_x25519_recipient_bytes("not a bech32 string").is_err());
    }

    #[test]
    fn fingerprint_is_deterministic_and_key_dependent() {
        let a = Identity::generate();
        let b = Identity::generate();
        assert_eq!(a.fingerprint(), a.fingerprint());
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.fingerprint().len(), FINGERPRINT_BYTES * 2);
    }
}
