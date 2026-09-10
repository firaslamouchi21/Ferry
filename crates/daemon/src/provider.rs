use std::sync::Mutex;

use ferry_core::ports::{RemoteLocator, Store};
use ferry_core::remote;
use ferry_crypto::secret_store::SecretStore;
use ferry_proto::ipc::{
    GistPublishedView, ProviderAuthView, ProviderStatusView, RosterFetchPreviewView,
    RosterPreviewEntryView,
};
use ferry_remote::github::{self, DeviceCode, GithubEndpoints};
use ferry_remote::http::HttpClient;

fn endpoints() -> GithubEndpoints {
    GithubEndpoints::default()
}

const PROVIDER: &str = "github";
const SCOPE: &str = "gist repo";

pub struct ProviderRegistry {
    enabled: bool,
    tokens: SecretStore,
    client_id: Option<String>,
    pending: Mutex<Option<DeviceCode>>,
    cached_login: Mutex<Option<String>>,
}

#[derive(Debug)]
pub enum ProviderError {
    Disabled,
    NotConnected,
    NoPendingAuth,
    NoClientId,
    Message(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Disabled => write!(
                f,
                "remote provider features are turned off — set remote_features_enabled = true in config.toml and restart"
            ),
            ProviderError::NotConnected => write!(f, "no GitHub connection — run: ferry provider connect"),
            ProviderError::NoPendingAuth => write!(f, "no device authorization is in progress"),
            ProviderError::NoClientId => write!(
                f,
                "GitHub device-flow login needs a registered OAuth App — set provider_github_client_id in config.toml (or $FERRY_GITHUB_CLIENT_ID), or connect with a personal access token instead"
            ),
            ProviderError::Message(m) => write!(f, "{m}"),
        }
    }
}

pub enum ConnectOutcome {
    Connected(ProviderStatusView),
    AwaitingDeviceAuth(ProviderAuthView),
}

impl ProviderRegistry {
    pub fn new(tokens: SecretStore, enabled: bool, client_id: Option<String>) -> Self {
        let client_id = std::env::var("FERRY_GITHUB_CLIENT_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or(client_id);
        Self {
            enabled,
            tokens,
            client_id,
            pending: Mutex::new(None),
            cached_login: Mutex::new(None),
        }
    }

    fn client_id(&self) -> Result<&str, ProviderError> {
        self.client_id.as_deref().ok_or(ProviderError::NoClientId)
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_login(&self, login: Option<String>) {
        *self.cached_login.lock().unwrap() = login;
    }

    fn token(&self) -> Result<Option<String>, ProviderError> {
        match self.tokens.load() {
            Ok(Some(bytes)) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| ProviderError::Message("stored token is not valid utf-8".into())),
            Ok(None) => Ok(None),
            Err(e) => Err(ProviderError::Message(e.to_string())),
        }
    }

    fn require_enabled(&self) -> Result<(), ProviderError> {
        if self.enabled {
            Ok(())
        } else {
            Err(ProviderError::Disabled)
        }
    }

    pub fn status(&self) -> ProviderStatusView {
        let has_token = self.enabled && matches!(self.token(), Ok(Some(_)));
        let login = if has_token {
            self.cached_login.lock().unwrap().clone()
        } else {
            None
        };
        ProviderStatusView {
            provider: PROVIDER.into(),
            enabled: self.enabled,
            connected: has_token,
            login,
        }
    }

    pub fn connect(&self, pat: Option<String>) -> Result<ConnectOutcome, ProviderError> {
        self.require_enabled()?;
        let http = HttpClient::new();

        if let Some(pat) = pat {
            let login = github::verify_token(&http, &endpoints(), &pat)
                .map_err(|e| ProviderError::Message(format!("that token was not accepted by GitHub: {e}")))?;
            self.tokens
                .store(pat.as_bytes())
                .map_err(|e| ProviderError::Message(e.to_string()))?;
            self.set_login(Some(login.login.clone()));
            return Ok(ConnectOutcome::Connected(ProviderStatusView {
                provider: PROVIDER.into(),
                enabled: true,
                connected: true,
                login: Some(login.login),
            }));
        }

        let client_id = self.client_id()?.to_string();
        let device = github::begin_device_flow(&http, &endpoints(), &client_id, SCOPE)
            .map_err(|e| ProviderError::Message(e.to_string()))?;
        let view = ProviderAuthView {
            user_code: device.user_code.clone(),
            verification_uri: device.verification_uri.clone(),
            interval_secs: device.interval as u32,
            expires_in_secs: device.expires_in as u32,
        };
        *self.pending.lock().unwrap() = Some(device);
        Ok(ConnectOutcome::AwaitingDeviceAuth(view))
    }

    pub fn connect_poll(&self) -> Result<Option<ProviderStatusView>, ProviderError> {
        self.require_enabled()?;
        let device_code = {
            let guard = self.pending.lock().unwrap();
            guard.as_ref().ok_or(ProviderError::NoPendingAuth)?.device_code.clone()
        };
        let client_id = self.client_id()?.to_string();
        match github::poll_device_flow(&HttpClient::new(), &endpoints(), &client_id, &device_code) {
            Ok(token) => {
                self.tokens
                    .store(token.as_bytes())
                    .map_err(|e| ProviderError::Message(e.to_string()))?;
                *self.pending.lock().unwrap() = None;
                let login = github::verify_token(&HttpClient::new(), &endpoints(), &token).ok().map(|l| l.login);
                self.set_login(login.clone());
                Ok(Some(ProviderStatusView {
                    provider: PROVIDER.into(),
                    enabled: true,
                    connected: true,
                    login,
                }))
            }
            Err(github::GithubError::AuthorizationPending) | Err(github::GithubError::SlowDown) => Ok(None),
            Err(e) => Err(ProviderError::Message(e.to_string())),
        }
    }

    pub fn disconnect(&self) -> Result<(), ProviderError> {
        *self.pending.lock().unwrap() = None;
        self.set_login(None);
        self.tokens
            .clear()
            .map_err(|e| ProviderError::Message(e.to_string()))
    }

    fn client(&self) -> Result<github::GithubClient, ProviderError> {
        self.require_enabled()?;
        let token = self.token()?;
        if token.is_none() {
            return Err(ProviderError::NotConnected);
        }
        Ok(github::GithubClient::new(token))
    }

    pub fn publish_gist(&self, store: &impl Store, item_id: &str) -> Result<GistPublishedView, ProviderError> {
        let client = self.client()?;
        let published = remote::publish_item_as_snippet(&client, store, item_id)
            .map_err(|e| ProviderError::Message(e.to_string()))?;
        Ok(GistPublishedView {
            url: published.url,
            id: published.id,
        })
    }

    pub fn fetch_roster_preview(
        &self,
        store: &impl Store,
        locator: &str,
    ) -> Result<RosterFetchPreviewView, ProviderError> {
        let client = self.client()?;
        let preview = remote::preview_roster(&client, store, &parse_locator(locator))
            .map_err(|e| ProviderError::Message(e.to_string()))?;
        Ok(RosterFetchPreviewView {
            signer_verifying_key_hex: preview.signer_verifying_key_hex,
            known_signer: preview.known_signer,
            adds: preview.adds,
            already_present: preview.already_present,
            entries: preview
                .entries
                .into_iter()
                .map(|e| RosterPreviewEntryView {
                    peer_id: e.peer_id,
                    display_name: e.display_name,
                    already_present: matches!(e.change, remote::RosterChange::AlreadyPresent),
                })
                .collect(),
        })
    }

    pub fn apply_roster(
        &self,
        store: &mut impl Store,
        locator: &str,
    ) -> Result<ferry_core::ports::RosterImportSummary, ProviderError> {
        let client = self.client()?;
        remote::apply_roster(&client, store, &parse_locator(locator))
            .map_err(|e| ProviderError::Message(e.to_string()))
    }
}

fn parse_locator(raw: &str) -> RemoteLocator {
    let (path, reference) = match raw.split_once('@') {
        Some((p, r)) => (p.to_string(), Some(r.to_string())),
        None => (raw.to_string(), None),
    };
    RemoteLocator {
        provider: PROVIDER.into(),
        host: None,
        path,
        reference,
    }
}
