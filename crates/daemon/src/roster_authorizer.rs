use ferry_crypto::identity::parse_x25519_recipient_bytes;
use rusqlite::Connection;

pub fn is_authorized_by_roster(conn: &Connection, remote_static: &[u8; 32]) -> bool {
    peer_id_for_static(conn, remote_static).is_some()
}

pub fn sealing_key_for_peer_id(conn: &Connection, peer_id: &str) -> Option<[u8; 32]> {
    let peer = ferry_store::roster::get_peer(conn, peer_id).ok()??;
    parse_x25519_recipient_bytes(&peer.sealing_key).ok()
}

pub fn peer_id_for_static(conn: &Connection, remote_static: &[u8; 32]) -> Option<String> {
    let peers = ferry_store::roster::list_peers(conn).ok()?;
    peers
        .into_iter()
        .find(|peer| {
            parse_x25519_recipient_bytes(&peer.sealing_key)
                .map(|bytes| &bytes == remote_static)
                .unwrap_or(false)
        })
        .map(|peer| peer.peer_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_crypto::identity::Identity;
    use ferry_net::transport::{accept_responder, connect_initiator, StaticKeypair};
    use std::os::unix::net::UnixStream;
    use std::thread;

    fn static_keypair_for(identity: &Identity) -> StaticKeypair {
        StaticKeypair {
            private: identity.sealing_key_x25519_bytes(),
            public: identity.public().sealing_key_x25519_bytes().unwrap(),
        }
    }

    #[test]
    fn rostered_identity_is_authorized_and_a_stranger_is_not() {
        let conn = ferry_store::connection::open_in_memory().unwrap();

        let rostered_identity = Identity::generate();
        let stranger_identity = Identity::generate();

        ferry_store::roster::insert_peer(
            &conn,
            &rostered_identity.fingerprint(),
            "laptop",
            &hex::encode(rostered_identity.verifying_key().to_bytes()),
            &rostered_identity.public().sealing_key,
        )
        .unwrap();

        let rostered_bytes = rostered_identity.public().sealing_key_x25519_bytes().unwrap();
        let stranger_bytes = stranger_identity.public().sealing_key_x25519_bytes().unwrap();

        assert!(is_authorized_by_roster(&conn, &rostered_bytes));
        assert!(!is_authorized_by_roster(&conn, &stranger_bytes));
    }

    #[test]
    fn peer_id_for_static_resolves_the_roster_fingerprint_and_is_none_for_a_stranger() {
        let conn = ferry_store::connection::open_in_memory().unwrap();

        let rostered_identity = Identity::generate();
        let stranger_identity = Identity::generate();

        ferry_store::roster::insert_peer(
            &conn,
            &rostered_identity.fingerprint(),
            "laptop",
            &hex::encode(rostered_identity.verifying_key().to_bytes()),
            &rostered_identity.public().sealing_key,
        )
        .unwrap();

        let rostered_bytes = rostered_identity.public().sealing_key_x25519_bytes().unwrap();
        let stranger_bytes = stranger_identity.public().sealing_key_x25519_bytes().unwrap();

        assert_eq!(
            peer_id_for_static(&conn, &rostered_bytes),
            Some(rostered_identity.fingerprint())
        );
        assert_eq!(peer_id_for_static(&conn, &stranger_bytes), None);
    }

    #[test]
    fn full_chain_a_real_noise_handshake_between_two_real_identities_is_authorized_by_the_real_roster() {
        let conn = ferry_store::connection::open_in_memory().unwrap();

        let responder_identity = Identity::generate();
        let initiator_identity = Identity::generate();

        ferry_store::roster::insert_peer(
            &conn,
            &initiator_identity.fingerprint(),
            "initiator-laptop",
            &hex::encode(initiator_identity.verifying_key().to_bytes()),
            &initiator_identity.public().sealing_key,
        )
        .unwrap();

        let responder_keys = static_keypair_for(&responder_identity);
        let initiator_keys = static_keypair_for(&initiator_identity);
        let responder_public = responder_keys.public;

        let (initiator_sock, responder_sock) = UnixStream::pair().unwrap();

        let responder_thread = thread::spawn(move || {
            let conn_for_thread = conn;
            accept_responder(responder_sock, &responder_keys, |remote_static| {
                is_authorized_by_roster(&conn_for_thread, remote_static)
            })
        });

        let initiator_result = connect_initiator(
            initiator_sock,
            &initiator_keys,
            &responder_public,
            |_| true,
        );

        assert!(initiator_result.is_ok());
        assert!(responder_thread.join().unwrap().is_ok());
    }

    #[test]
    fn full_chain_rejects_an_identity_that_was_never_added_to_the_roster() {
        let conn = ferry_store::connection::open_in_memory().unwrap();

        let responder_identity = Identity::generate();
        let stranger_identity = Identity::generate();

        let responder_keys = static_keypair_for(&responder_identity);
        let stranger_keys = static_keypair_for(&stranger_identity);
        let responder_public = responder_keys.public;

        let (initiator_sock, responder_sock) = UnixStream::pair().unwrap();

        let responder_thread = thread::spawn(move || {
            let conn_for_thread = conn;
            accept_responder(responder_sock, &responder_keys, |remote_static| {
                is_authorized_by_roster(&conn_for_thread, remote_static)
            })
        });

        let _ = connect_initiator(initiator_sock, &stranger_keys, &responder_public, |_| true);

        assert!(matches!(
            responder_thread.join().unwrap(),
            Err(ferry_net::transport::TransportError::Unauthorized)
        ));
    }
}
