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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RawConfig {
    pub listen_port: u16,
    pub data_dir: Option<PathBuf>,
    pub auto_accept_from_roster: bool,
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            listen_port: 47821,
            data_dir: None,
            auto_accept_from_roster: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub listen_port: u16,
    pub data_dir: PathBuf,
    pub auto_accept_from_roster: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("listen_port {0} is below the minimum allowed port {MIN_PORT}")]
    PortTooLow(u16),
    #[error("data_dir must not be an empty path")]
    EmptyDataDir,
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

        Ok(Config {
            listen_port: self.listen_port,
            data_dir,
            auto_accept_from_roster: self.auto_accept_from_roster,
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
        };
        let config = raw.validate(PathBuf::from("/tmp/ferry")).unwrap();
        assert_eq!(config.data_dir, PathBuf::from("/custom/path"));
    }
}
