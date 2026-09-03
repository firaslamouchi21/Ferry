use std::str::FromStr;

use age::x25519::Recipient;
use ferry_proto::blob::BlobManifest;
use thiserror::Error;

use crate::identity::{parse_x25519_recipient_bytes, Identity};
use crate::seal;

const MAGIC: &[u8; 16] = b"FERRYSEALEDBLOB\x01";
const HEADER_BYTES: usize = MAGIC.len() + 32;

#[derive(Debug, Error)]
pub enum BlobError {
    #[error("not a Ferry sealed blob")]
    BadMagic,
    #[error("sealed blob is truncated")]
    Truncated,
    #[error("this sealed blob is addressed to a different machine")]
    WrongRecipient,
    #[error("invalid recipient key: {0}")]
    Recipient(String),
    #[error("seal error: {0}")]
    Seal(#[from] seal::SealError),
    #[error("manifest serialization failed: {0}")]
    Manifest(String),
}

pub fn seal_blob(recipient_sealing_key: &str, manifest: &BlobManifest) -> Result<Vec<u8>, BlobError> {
    let recipient = Recipient::from_str(recipient_sealing_key)
        .map_err(|e| BlobError::Recipient(e.to_string()))?;
    let recipient_bytes = parse_x25519_recipient_bytes(recipient_sealing_key)
        .map_err(|e| BlobError::Recipient(e.to_string()))?;

    let plaintext = bincode::serialize(manifest).map_err(|e| BlobError::Manifest(e.to_string()))?;
    let ciphertext = seal::seal(&recipient, &plaintext)?;

    let mut out = Vec::with_capacity(HEADER_BYTES + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&recipient_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn peek_recipient(blob: &[u8]) -> Result<[u8; 32], BlobError> {
    if blob.len() < HEADER_BYTES {
        return Err(BlobError::Truncated);
    }
    if &blob[..MAGIC.len()] != MAGIC {
        return Err(BlobError::BadMagic);
    }
    Ok(blob[MAGIC.len()..HEADER_BYTES].try_into().expect("slice is exactly 32 bytes"))
}

pub fn open_blob(identity: &Identity, blob: &[u8]) -> Result<BlobManifest, BlobError> {
    let recipient = peek_recipient(blob)?;
    let ours = identity
        .public()
        .sealing_key_x25519_bytes()
        .map_err(|e| BlobError::Recipient(e.to_string()))?;
    if recipient != ours {
        return Err(BlobError::WrongRecipient);
    }

    let plaintext = seal::open(identity.sealing_identity(), &blob[HEADER_BYTES..])?;
    bincode::deserialize(&plaintext).map_err(|e| BlobError::Manifest(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_proto::states::ItemKind;

    fn sample_manifest() -> BlobManifest {
        BlobManifest {
            item_id: "item-1".into(),
            origin_peer_id: "peer-a".into(),
            kind: ItemKind::File,
            name: "notes.txt".into(),
            size_bytes: 5,
            hash: "deadbeef".into(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            payload: b"hello".to_vec(),
        }
    }

    #[test]
    fn round_trips_a_manifest_to_the_intended_recipient() {
        let recipient_identity = Identity::generate();
        let recipient_key = recipient_identity.public().sealing_key;

        let blob = seal_blob(&recipient_key, &sample_manifest()).unwrap();
        let opened = open_blob(&recipient_identity, &blob).unwrap();

        assert_eq!(opened, sample_manifest());
    }

    #[test]
    fn a_blob_for_someone_else_is_rejected_before_any_decryption() {
        let recipient_identity = Identity::generate();
        let blob = seal_blob(&recipient_identity.public().sealing_key, &sample_manifest()).unwrap();

        let other = Identity::generate();
        assert!(matches!(open_blob(&other, &blob), Err(BlobError::WrongRecipient)));
    }

    #[test]
    fn garbage_is_rejected_as_bad_magic_or_truncated() {
        let recipient_identity = Identity::generate();
        assert!(matches!(open_blob(&recipient_identity, b"short"), Err(BlobError::Truncated)));
        assert!(matches!(
            open_blob(&recipient_identity, &[0u8; HEADER_BYTES + 4]),
            Err(BlobError::BadMagic)
        ));
    }

    #[test]
    fn the_recipient_header_is_readable_without_the_private_key() {
        let recipient_identity = Identity::generate();
        let blob = seal_blob(&recipient_identity.public().sealing_key, &sample_manifest()).unwrap();
        assert_eq!(
            peek_recipient(&blob).unwrap(),
            recipient_identity.public().sealing_key_x25519_bytes().unwrap()
        );
    }
}
