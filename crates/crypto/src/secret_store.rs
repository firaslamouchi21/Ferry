use std::path::{Path, PathBuf};

use age::secrecy::SecretString;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("keychain error: {0}")]
    Keychain(#[from] keyring::Error),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not decrypt the stored secret: {0}")]
    Unseal(String),
    #[error("could not encrypt the secret: {0}")]
    Seal(String),
}

#[derive(Clone)]
enum Backend {
    Keychain { service: String, user: String },
    File { path: PathBuf, passphrase: SecretString },
}

#[derive(Clone)]
pub struct SecretStore {
    backend: Backend,
}

impl SecretStore {
    pub fn keychain(service: &str, user: &str) -> Self {
        Self {
            backend: Backend::Keychain {
                service: service.to_string(),
                user: user.to_string(),
            },
        }
    }

    pub fn file(path: PathBuf, passphrase: SecretString) -> Self {
        Self {
            backend: Backend::File { path, passphrase },
        }
    }

    pub fn file_with_passphrase_str(path: PathBuf, passphrase: &str) -> Self {
        Self::file(path, SecretString::from(passphrase.to_string()))
    }

    pub fn load(&self) -> Result<Option<Vec<u8>>, SecretStoreError> {
        match &self.backend {
            Backend::Keychain { service, user } => {
                let entry = keyring::Entry::new(service, user)?;
                match entry.get_secret() {
                    Ok(bytes) => Ok(Some(bytes)),
                    Err(keyring::Error::NoEntry) => Ok(None),
                    Err(e) => Err(SecretStoreError::Keychain(e)),
                }
            }
            Backend::File { path, passphrase } => match std::fs::read(path) {
                Ok(ciphertext) => {
                    let plaintext = age::decrypt(&age::scrypt::Identity::new(passphrase.clone()), &ciphertext)
                        .map_err(|e| SecretStoreError::Unseal(e.to_string()))?;
                    Ok(Some(plaintext))
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(source) => Err(SecretStoreError::Io {
                    path: path.clone(),
                    source,
                }),
            },
        }
    }

    pub fn store(&self, bytes: &[u8]) -> Result<(), SecretStoreError> {
        match &self.backend {
            Backend::Keychain { service, user } => {
                let entry = keyring::Entry::new(service, user)?;
                entry.set_secret(bytes)?;
                Ok(())
            }
            Backend::File { path, passphrase } => {
                let ciphertext = age::encrypt(&age::scrypt::Recipient::new(passphrase.clone()), bytes)
                    .map_err(|e| SecretStoreError::Seal(e.to_string()))?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| SecretStoreError::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                write_owner_only(path, &ciphertext)
            }
        }
    }

    pub fn clear(&self) -> Result<(), SecretStoreError> {
        match &self.backend {
            Backend::Keychain { service, user } => {
                let entry = keyring::Entry::new(service, user)?;
                match entry.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                    Err(e) => Err(SecretStoreError::Keychain(e)),
                }
            }
            Backend::File { path, .. } => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(SecretStoreError::Io {
                    path: path.clone(),
                    source,
                }),
            },
        }
    }
}

fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<(), SecretStoreError> {
    let io_err = |source| SecretStoreError::Io {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_backend_round_trips_and_clears() {
        let dir = std::env::temp_dir().join(format!(
            "ferry-secret-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let path = dir.join("token.age");
        let store = SecretStore::file(path.clone(), SecretString::from("a-strong-passphrase".to_string()));

        assert!(store.load().unwrap().is_none());
        store.store(b"ghp_exampletokenvalue").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some(b"ghp_exampletokenvalue".as_slice()));

        let on_disk = std::fs::read(&path).unwrap();
        assert!(!on_disk.windows(4).any(|w| w == b"ghp_"), "the token must not be stored in the clear");

        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_backend_with_the_wrong_passphrase_fails_to_unseal() {
        let dir = std::env::temp_dir().join(format!(
            "ferry-secret-store-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let path = dir.join("token.age");
        SecretStore::file(path.clone(), SecretString::from("right-passphrase".to_string()))
            .store(b"secret-bytes")
            .unwrap();

        let wrong = SecretStore::file(path, SecretString::from("wrong-passphrase".to_string()));
        assert!(matches!(wrong.load(), Err(SecretStoreError::Unseal(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore]
    fn keychain_backend_smoke_generate_read_clear() {
        let service = format!("dev.ferry.smoke-{}", std::process::id());
        let store = SecretStore::keychain(&service, "test");

        assert!(store.load().unwrap().is_none());
        store.store(b"smoke-value").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some(b"smoke-value".as_slice()));

        let reopened = SecretStore::keychain(&service, "test");
        assert_eq!(reopened.load().unwrap().as_deref(), Some(b"smoke-value".as_slice()));

        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }
}
