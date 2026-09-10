use std::path::{Path, PathBuf};

use age::secrecy::{ExposeSecret, SecretString};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const KEYCHAIN_SERVICE: &str = "dev.ferry.identity";
const KEYCHAIN_USER: &str = "identity";
const FINGERPRINT_BYTES: usize = 8;
const IDENTITY_FILE: &str = "identity.age";
const PASSPHRASE_ENV: &str = "FERRY_IDENTITY_PASSPHRASE";
const MIN_PASSPHRASE_LEN: usize = 8;

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
    #[error("identity keystore I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "the file-backed identity keystore needs a passphrase — set the {PASSPHRASE_ENV} \
         environment variable or point identity_passphrase_file at a file containing one"
    )]
    MissingPassphrase,
    #[error("the identity passphrase is too short — use at least {MIN_PASSPHRASE_LEN} characters")]
    WeakPassphrase,
    #[error("failed to seal the identity file: {0}")]
    Seal(String),
    #[error("failed to open the identity file — wrong passphrase or a corrupt file: {0}")]
    Unseal(String),
}

/// Where the machine identity's private key material lives.
#[derive(Debug)]
pub enum KeyBackend {
    /// The OS keychain (Keychain / DPAPI / Secret Service). The default.
    Keychain,
    /// An age-scrypt-encrypted `identity.age` in the data dir, for headless boxes with no
    /// working keychain. The key is never written unwrapped.
    File {
        data_dir: PathBuf,
        passphrase: SecretString,
    },
}

impl KeyBackend {
    /// Resolves a backend from config: `keychain` (default) or `file` with a passphrase taken
    /// from `$FERRY_IDENTITY_PASSPHRASE`, else from `passphrase_file`.
    pub fn resolve(
        file_mode: bool,
        data_dir: &Path,
        passphrase_file: Option<&Path>,
    ) -> Result<Self, IdentityError> {
        if !file_mode {
            return Ok(KeyBackend::Keychain);
        }
        let raw = match std::env::var(PASSPHRASE_ENV) {
            Ok(v) if !v.is_empty() => v,
            _ => {
                let path = passphrase_file.ok_or(IdentityError::MissingPassphrase)?;
                std::fs::read_to_string(path)
                    .map_err(|source| IdentityError::Io {
                        path: path.to_path_buf(),
                        source,
                    })?
                    .trim_end_matches(['\r', '\n'])
                    .to_string()
            }
        };
        if raw.chars().count() < MIN_PASSPHRASE_LEN {
            return Err(IdentityError::WeakPassphrase);
        }
        Ok(KeyBackend::File {
            data_dir: data_dir.to_path_buf(),
            passphrase: SecretString::from(raw),
        })
    }
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
        Self::load_or_generate_with(&KeyBackend::Keychain)
    }

    pub fn load_or_generate_with(backend: &KeyBackend) -> Result<Self, IdentityError> {
        match backend {
            KeyBackend::Keychain => Self::load_or_generate_keychain(),
            KeyBackend::File { data_dir, passphrase } => {
                Self::load_or_generate_file(data_dir, passphrase)
            }
        }
    }

    fn load_or_generate_keychain() -> Result<Self, IdentityError> {
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

    fn load_or_generate_file(data_dir: &Path, passphrase: &SecretString) -> Result<Self, IdentityError> {
        let path = data_dir.join(IDENTITY_FILE);
        match std::fs::read(&path) {
            Ok(ciphertext) => {
                let plaintext = age::decrypt(
                    &age::scrypt::Identity::new(passphrase.clone()),
                    &ciphertext,
                )
                .map_err(|e| IdentityError::Unseal(e.to_string()))?;
                let raw = String::from_utf8(plaintext)
                    .map_err(|e| IdentityError::Corrupt(format!("not valid utf-8: {e}")))?;
                Self::from_storage_string(&raw)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let identity = Self::generate();
                let recipient = age::scrypt::Recipient::new(passphrase.clone());
                let ciphertext = age::encrypt(&recipient, identity.to_storage_string().as_bytes())
                    .map_err(|e| IdentityError::Seal(e.to_string()))?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| IdentityError::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                write_owner_only(&path, &ciphertext)?;
                Ok(identity)
            }
            Err(source) => Err(IdentityError::Io {
                path: path.clone(),
                source,
            }),
        }
    }
}

fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<(), IdentityError> {
    let io_err = |source| IdentityError::Io {
        path: path.to_path_buf(),
        source,
    };
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(io_err)?;
        file.write_all(bytes).map_err(io_err)?;
        file.sync_all().map_err(io_err)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes).map_err(io_err)
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

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferry-keystore-test-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn uuid_like() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn file_backend_generates_then_reloads_the_same_identity() {
        let dir = temp_dir();
        let pass = SecretString::from("a-strong-enough-passphrase");
        let backend = KeyBackend::File {
            data_dir: dir.clone(),
            passphrase: pass.clone(),
        };

        let first = Identity::load_or_generate_with(&backend).unwrap();
        assert!(dir.join(IDENTITY_FILE).is_file(), "the file backend must persist identity.age");

        let second = Identity::load_or_generate_with(&backend).unwrap();
        assert_eq!(first.verifying_key().to_bytes(), second.verifying_key().to_bytes());
        assert_eq!(first.public().sealing_key, second.public().sealing_key);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_backend_stores_the_key_wrapped_never_plaintext() {
        let dir = temp_dir();
        let pass = SecretString::from("a-strong-enough-passphrase");
        let backend = KeyBackend::File { data_dir: dir.clone(), passphrase: pass };
        let identity = Identity::load_or_generate_with(&backend).unwrap();

        let on_disk = std::fs::read(dir.join(IDENTITY_FILE)).unwrap();
        let signing_hex = hex::encode(identity.signing_key.to_bytes());
        assert!(
            !String::from_utf8_lossy(&on_disk).contains(&signing_hex),
            "identity.age must never contain the raw signing key"
        );
        assert!(on_disk.starts_with(b"age-encryption.org/v1"), "identity.age must be an age file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_backend_rejects_the_wrong_passphrase() {
        let dir = temp_dir();
        Identity::load_or_generate_with(&KeyBackend::File {
            data_dir: dir.clone(),
            passphrase: SecretString::from("the-right-passphrase"),
        })
        .unwrap();

        let result = Identity::load_or_generate_with(&KeyBackend::File {
            data_dir: dir.clone(),
            passphrase: SecretString::from("the-wrong-passphrase"),
        });
        assert!(matches!(result, Err(IdentityError::Unseal(_))));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_keychain_when_not_in_file_mode() {
        let backend = KeyBackend::resolve(false, Path::new("/tmp/x"), None).unwrap();
        assert!(matches!(backend, KeyBackend::Keychain));
    }

    #[test]
    fn resolve_file_mode_reads_the_passphrase_file() {
        let dir = temp_dir();
        let pass_path = dir.join("pass");
        std::fs::write(&pass_path, "from-a-file-passphrase\n").unwrap();
        let backend = KeyBackend::resolve(true, &dir, Some(&pass_path)).unwrap();
        match backend {
            KeyBackend::File { passphrase, .. } => {
                assert_eq!(passphrase.expose_secret(), "from-a-file-passphrase")
            }
            KeyBackend::Keychain => panic!("expected file backend"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_file_mode_without_a_passphrase_is_an_error() {
        let err = KeyBackend::resolve(true, Path::new("/tmp/x"), None).unwrap_err();
        assert!(matches!(err, IdentityError::MissingPassphrase), "got {err:?}");
    }

    #[test]
    fn resolve_rejects_a_weak_passphrase() {
        let dir = temp_dir();
        let pass_path = dir.join("pass");
        std::fs::write(&pass_path, "short").unwrap();
        let err = KeyBackend::resolve(true, &dir, Some(&pass_path)).unwrap_err();
        assert!(matches!(err, IdentityError::WeakPassphrase), "got {err:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
