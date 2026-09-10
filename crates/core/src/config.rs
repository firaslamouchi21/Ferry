use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

const MIN_PORT: u16 = 1024;

pub const INLINE_STAGING_DIR: &str = "outbound-inline";

pub fn default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "ferry", "ferry")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".ferry"))
}

pub fn ipc_socket_path(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("ferry.sock")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeystoreMode {
    Keychain,
    File,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RawConfig {
    pub listen_port: u16,
    pub data_dir: Option<PathBuf>,
    pub auto_accept_from_roster: bool,
    pub identity_keystore: KeystoreMode,
    pub identity_passphrase_file: Option<PathBuf>,
    pub remote_features_enabled: bool,
    pub provider_token_file: Option<PathBuf>,
    pub provider_github_client_id: Option<String>,
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            listen_port: 47821,
            data_dir: None,
            auto_accept_from_roster: true,
            identity_keystore: KeystoreMode::Keychain,
            identity_passphrase_file: None,
            remote_features_enabled: false,
            provider_token_file: None,
            provider_github_client_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub listen_port: u16,
    pub data_dir: PathBuf,
    pub auto_accept_from_roster: bool,
    pub identity_keystore: KeystoreMode,
    pub identity_passphrase_file: Option<PathBuf>,
    pub remote_features_enabled: bool,
    pub provider_token_file: Option<PathBuf>,
    pub provider_github_client_id: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("listen_port {0} is below the minimum allowed port {MIN_PORT}")]
    PortTooLow(u16),
    #[error("data_dir must not be an empty path")]
    EmptyDataDir,
    #[error("identity_passphrase_file must not be an empty path")]
    EmptyPassphraseFile,
    #[error("provider_token_file must not be an empty path")]
    EmptyProviderTokenFile,
}

impl RawConfig {
    pub fn validate(self, default_data_dir: PathBuf) -> Result<Config, ConfigError> {
        if self.listen_port < MIN_PORT {
            return Err(ConfigError::PortTooLow(self.listen_port));
        }

        let data_dir = self.data_dir.unwrap_or(default_data_dir);
        if data_dir.as_os_str().is_empty() {
            return Err(ConfigError::EmptyDataDir);
        }

        if let Some(path) = &self.identity_passphrase_file {
            if path.as_os_str().is_empty() {
                return Err(ConfigError::EmptyPassphraseFile);
            }
        }

        if let Some(path) = &self.provider_token_file {
            if path.as_os_str().is_empty() {
                return Err(ConfigError::EmptyProviderTokenFile);
            }
        }

        Ok(Config {
            listen_port: self.listen_port,
            data_dir,
            auto_accept_from_roster: self.auto_accept_from_roster,
            identity_keystore: self.identity_keystore,
            identity_passphrase_file: self.identity_passphrase_file,
            remote_features_enabled: self.remote_features_enabled,
            provider_token_file: self.provider_token_file,
            provider_github_client_id: self
                .provider_github_client_id
                .filter(|s| !s.trim().is_empty()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate_cleanly() {
        let config = RawConfig::default()
            .validate(PathBuf::from("/tmp/ferry"))
            .unwrap();
        assert_eq!(config.listen_port, 47821);
        assert_eq!(config.data_dir, PathBuf::from("/tmp/ferry"));
    }

    #[test]
    fn rejects_port_below_minimum() {
        let raw = RawConfig {
            listen_port: 80,
            data_dir: None,
            auto_accept_from_roster: true,
            ..RawConfig::default()
        };
        assert_eq!(
            raw.validate(PathBuf::from("/tmp/ferry")),
            Err(ConfigError::PortTooLow(80))
        );
    }

    #[test]
    fn rejects_empty_data_dir() {
        let raw = RawConfig {
            listen_port: 47821,
            data_dir: Some(PathBuf::new()),
            auto_accept_from_roster: true,
            ..RawConfig::default()
        };
        assert_eq!(
            raw.validate(PathBuf::from("/tmp/ferry")),
            Err(ConfigError::EmptyDataDir)
        );
    }

    #[test]
    fn disabling_auto_accept_is_accepted_now_that_deferred_accept_exists() {
        let raw = RawConfig {
            listen_port: 47821,
            data_dir: None,
            auto_accept_from_roster: false,
            ..RawConfig::default()
        };
        let config = raw.validate(PathBuf::from("/tmp/ferry")).unwrap();
        assert!(!config.auto_accept_from_roster);
    }

    #[test]
    fn explicit_data_dir_overrides_default() {
        let raw = RawConfig {
            listen_port: 47821,
            data_dir: Some(PathBuf::from("/custom/path")),
            auto_accept_from_roster: true,
            ..RawConfig::default()
        };
        let config = raw.validate(PathBuf::from("/tmp/ferry")).unwrap();
        assert_eq!(config.data_dir, PathBuf::from("/custom/path"));
    }

    #[test]
    fn identity_keystore_defaults_to_keychain() {
        let config = RawConfig::default().validate(PathBuf::from("/tmp/ferry")).unwrap();
        assert_eq!(config.identity_keystore, KeystoreMode::Keychain);
        assert!(config.identity_passphrase_file.is_none());
    }

    #[test]
    fn remote_features_default_off_and_an_empty_token_file_is_rejected() {
        let config: RawConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.remote_features_enabled);
        assert!(config.provider_token_file.is_none());

        let bad = RawConfig {
            provider_token_file: Some(PathBuf::new()),
            ..Default::default()
        };
        assert_eq!(
            bad.validate(PathBuf::from("/data")).unwrap_err(),
            ConfigError::EmptyProviderTokenFile
        );
    }

    #[test]
    fn a_blank_github_client_id_normalises_to_none() {
        let raw: RawConfig =
            serde_json::from_str(r#"{"provider_github_client_id":"  "}"#).unwrap();
        let config = raw.validate(PathBuf::from("/data")).unwrap();
        assert!(config.provider_github_client_id.is_none());

        let raw: RawConfig =
            serde_json::from_str(r#"{"provider_github_client_id":"Iv1.abc123"}"#).unwrap();
        let config = raw.validate(PathBuf::from("/data")).unwrap();
        assert_eq!(config.provider_github_client_id.as_deref(), Some("Iv1.abc123"));
    }

    #[test]
    fn identity_keystore_file_mode_deserializes() {
        let raw: RawConfig = serde_json::from_str(
            r#"{"identity_keystore":"file","identity_passphrase_file":"/run/secrets/ferry-pass"}"#,
        )
        .unwrap();
        let config = raw.validate(PathBuf::from("/tmp/ferry")).unwrap();
        assert_eq!(config.identity_keystore, KeystoreMode::File);
        assert_eq!(
            config.identity_passphrase_file,
            Some(PathBuf::from("/run/secrets/ferry-pass"))
        );
    }

    #[test]
    fn rejects_empty_passphrase_file() {
        let raw = RawConfig {
            identity_passphrase_file: Some(PathBuf::new()),
            ..RawConfig::default()
        };
        assert_eq!(
            raw.validate(PathBuf::from("/tmp/ferry")),
            Err(ConfigError::EmptyPassphraseFile)
        );
    }
}
