use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use ferry_proto::ids::PeerId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::identity::Identity;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEntry {
    pub peer_id: PeerId,
    pub display_name: String,
    pub signing_key: [u8; 32],
    pub sealing_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedRoster {
    pub entries: Vec<RosterEntry>,
    pub signer: PeerId,
    pub signer_signing_key: [u8; 32],
    pub signature: String,
}

#[derive(Debug, Error)]
pub enum RosterError {
    #[error("roster signature is invalid — file may be corrupt or tampered with")]
    InvalidSignature,
    #[error("roster signer key is malformed")]
    MalformedSignerKey,
    #[error("roster signature is not valid hex or is not 64 bytes")]
    MalformedSignature,
    #[error("roster contains the signer as one of its own entries")]
    SelfIncluded,
    #[error("failed to serialize roster for signing: {0}")]
    Serialize(String),
}

fn canonical_bytes(entries: &[RosterEntry]) -> Result<Vec<u8>, RosterError> {
    serde_json::to_vec(entries).map_err(|e| RosterError::Serialize(e.to_string()))
}

impl SignedRoster {
    pub fn sign(identity: &Identity, entries: Vec<RosterEntry>) -> Result<Self, RosterError> {
        let signer_key = identity.verifying_key();
        let signer_bytes = signer_key.to_bytes();

        if entries.iter().any(|e| e.signing_key == signer_bytes) {
            return Err(RosterError::SelfIncluded);
        }

        let bytes = canonical_bytes(&entries)?;
        let signature = identity.sign(&bytes);

        Ok(Self {
            entries,
            signer: PeerId(hex::encode(signer_bytes)),
            signer_signing_key: signer_bytes,
            signature: hex::encode(signature.to_bytes()),
        })
    }

    pub fn verify(&self) -> Result<&[RosterEntry], RosterError> {
        if self
            .entries
            .iter()
            .any(|e| e.signing_key == self.signer_signing_key)
        {
            return Err(RosterError::SelfIncluded);
        }

        let verifying_key = VerifyingKey::from_bytes(&self.signer_signing_key)
            .map_err(|_| RosterError::MalformedSignerKey)?;
        let signature_bytes: [u8; 64] = hex::decode(&self.signature)
            .map_err(|_| RosterError::MalformedSignature)?
            .try_into()
            .map_err(|_| RosterError::MalformedSignature)?;
        let signature = Signature::from_bytes(&signature_bytes);
        let bytes = canonical_bytes(&self.entries)?;

        verifying_key
            .verify(&bytes, &signature)
            .map_err(|_| RosterError::InvalidSignature)?;

        Ok(&self.entries)
    }

    pub fn peer_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(seed: u8) -> RosterEntry {
        RosterEntry {
            peer_id: PeerId(format!("peer-{seed}")),
            display_name: format!("laptop-{seed}"),
            signing_key: [seed; 32],
            sealing_key: format!("age1sealingkeystub{seed}"),
        }
    }

    #[test]
    fn signed_roster_verifies_cleanly() {
        let identity = Identity::generate();
        let entries = vec![sample_entry(1), sample_entry(2)];
        let signed = SignedRoster::sign(&identity, entries.clone()).unwrap();

        let verified = signed.verify().unwrap();
        assert_eq!(verified, entries.as_slice());
        assert_eq!(signed.peer_count(), 2);
    }

    #[test]
    fn tampered_entry_fails_verification() {
        let identity = Identity::generate();
        let mut signed = SignedRoster::sign(&identity, vec![sample_entry(1)]).unwrap();
        signed.entries[0].display_name = "attacker-renamed".into();

        assert!(matches!(
            signed.verify(),
            Err(RosterError::InvalidSignature)
        ));
    }

    #[test]
    fn wrong_signer_key_fails_verification() {
        let identity = Identity::generate();
        let mut signed = SignedRoster::sign(&identity, vec![sample_entry(1)]).unwrap();
        let attacker = Identity::generate();
        signed.signer_signing_key = attacker.verifying_key().to_bytes();

        assert!(matches!(
            signed.verify(),
            Err(RosterError::InvalidSignature)
        ));
    }

    #[test]
    fn signing_self_into_the_roster_is_rejected() {
        let identity = Identity::generate();
        let mut self_entry = sample_entry(9);
        self_entry.signing_key = identity.verifying_key().to_bytes();

        assert!(matches!(
            SignedRoster::sign(&identity, vec![self_entry]),
            Err(RosterError::SelfIncluded)
        ));
    }
}
