use std::collections::HashMap;
use std::net::IpAddr;

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
    pub addresses: Vec<IpAddr>,
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
        )?
        .enable_addr_auto();

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
        addresses: resolved.addresses.iter().map(|scoped| scoped.to_ip_addr()).collect(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use mdns_sd::ServiceEvent;
    use std::time::Duration;

    #[test]
    #[ignore = "needs working mDNS multicast on the loopback interface"]
    fn an_advertised_service_resolves_with_a_reachable_address_and_its_fingerprint() {
        let advertiser = Discovery::new().unwrap();
        advertiser.advertise("test-node", "abc123fingerprint", 2, 47999).unwrap();

        let browser = Discovery::new().unwrap();
        let events = browser.browse().unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let resolved = loop {
            assert!(std::time::Instant::now() < deadline, "service never resolved");
            if let Ok(ServiceEvent::ServiceResolved(resolved)) = events.recv_timeout(Duration::from_secs(1)) {
                if resolved.fullname.starts_with("test-node.") {
                    break peer_from_resolved(&resolved);
                }
            }
        };

        assert_eq!(resolved.port, 47999);
        assert_eq!(resolved.fingerprint.as_deref(), Some("abc123fingerprint"));
        assert_eq!(resolved.protocol_version.as_deref(), Some("2"));
        assert!(
            !resolved.addresses.is_empty(),
            "the advertisement must carry at least one A/AAAA record or a peer cannot connect"
        );

        advertiser.shutdown().unwrap();
        browser.shutdown().unwrap();
    }
}
