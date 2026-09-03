use std::collections::HashMap;

use mdns_sd::{Receiver, ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use thiserror::Error;

pub const SERVICE_TYPE: &str = "_ferry._tcp.local.";
const FINGERPRINT_TXT_KEY: &str = "fingerprint";
const PROTOCOL_VERSION_TXT_KEY: &str = "protocol_version";

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("mdns error: {0}")]
    Mdns(#[from] mdns_sd::Error),
}

pub struct Discovery {
    daemon: ServiceDaemon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub fullname: String,
    pub host: String,
    pub port: u16,
    pub fingerprint: Option<String>,
    pub protocol_version: Option<String>,
}

impl Discovery {
    pub fn new() -> Result<Self, DiscoveryError> {
        Ok(Self {
            daemon: ServiceDaemon::new()?,
        })
    }

    pub fn advertise(
        &self,
        display_name: &str,
        fingerprint: &str,
        protocol_version: u16,
        port: u16,
    ) -> Result<(), DiscoveryError> {
        let host_name = format!("{display_name}.local.");
        let properties = HashMap::from([
            (FINGERPRINT_TXT_KEY.to_string(), fingerprint.to_string()),
            (
                PROTOCOL_VERSION_TXT_KEY.to_string(),
                protocol_version.to_string(),
            ),
        ]);

        let info = ServiceInfo::new(
            SERVICE_TYPE,
            display_name,
            &host_name,
            "",
            port,
            properties,
        )?;

        self.daemon.register(info)?;
        Ok(())
    }

    pub fn browse(&self) -> Result<Receiver<ServiceEvent>, DiscoveryError> {
        Ok(self.daemon.browse(SERVICE_TYPE)?)
    }

    pub fn shutdown(&self) -> Result<(), DiscoveryError> {
        self.daemon.shutdown()?;
        Ok(())
    }
}

pub fn peer_from_resolved(resolved: &ResolvedService) -> DiscoveredPeer {
    DiscoveredPeer {
        fullname: resolved.fullname.clone(),
        host: resolved.host.clone(),
        port: resolved.port,
        fingerprint: resolved
            .txt_properties
            .get_property_val_str(FINGERPRINT_TXT_KEY)
            .map(String::from),
        protocol_version: resolved
            .txt_properties
            .get_property_val_str(PROTOCOL_VERSION_TXT_KEY)
            .map(String::from),
    }
}
