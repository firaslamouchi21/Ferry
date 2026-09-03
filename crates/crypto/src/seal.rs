use age::x25519::{Identity, Recipient};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SealError {
    #[error("failed to seal payload: {0}")]
    Encrypt(#[from] age::EncryptError),
    #[error("failed to open sealed payload: {0}")]
    Decrypt(#[from] age::DecryptError),
}

pub fn seal(recipient: &Recipient, plaintext: &[u8]) -> Result<Vec<u8>, SealError> {
    Ok(age::encrypt(recipient, plaintext)?)
}

pub fn open(identity: &Identity, ciphertext: &[u8]) -> Result<Vec<u8>, SealError> {
    Ok(age::decrypt(identity, ciphertext)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_arbitrary_payload() {
        let identity = Identity::generate();
        let recipient = identity.to_public();
        let plaintext = b"a secret worth carrying";

        let ciphertext = seal(&recipient, plaintext).unwrap();
        assert_ne!(ciphertext, plaintext);

        let opened = open(&identity, &ciphertext).unwrap();
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn wrong_identity_cannot_open() {
        let identity = Identity::generate();
        let recipient = identity.to_public();
        let ciphertext = seal(&recipient, b"payload").unwrap();

        let other_identity = Identity::generate();
        assert!(open(&other_identity, &ciphertext).is_err());
    }
}
