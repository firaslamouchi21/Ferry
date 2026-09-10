use std::io::{Read, Write};

use ed25519_dalek::VerifyingKey;
use ferry_crypto::identity::{fingerprint_of, Identity};
use ferry_crypto::pair::PairingSession;
use ferry_crypto::phrase::verification_phrase;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct IdentityPayload {
    signing_key: [u8; 32],
    sealing_key: String,
    display_name: String,
}

pub struct PairingOutcome {
    pub peer_id: String,
    pub display_name: String,
    pub signing_key_hex: String,
    pub sealing_key: String,
}

pub fn generate_code() -> String {
    use rand::Rng;
    let n: u32 = rand::thread_rng().gen_range(0..1_000_000);
    format!("{n:06}")
}

pub fn run_pairing_exchange(
    mut stream: impl Read + Write,
    code: &str,
    local_identity: &Identity,
    local_display_name: &str,
    before_confirm: impl FnOnce(),
    confirm: impl FnOnce(&str) -> bool,
) -> Result<PairingOutcome, String> {
    let (session, my_spake_msg) = PairingSession::start(code.as_bytes());
    ferry_net::framing::write_frame(&mut stream, &my_spake_msg).map_err(|e| e.to_string())?;
    let peer_spake_msg = ferry_net::framing::read_frame(&mut stream).map_err(|e| e.to_string())?;

    let channel = session
        .finish(&peer_spake_msg)
        .map_err(|_| "pairing failed — the codes did not match".to_string())?;

    let local_public = local_identity.public();
    let payload = IdentityPayload {
        signing_key: local_public.signing_key,
        sealing_key: local_public.sealing_key.clone(),
        display_name: local_display_name.to_string(),
    };
    let payload_bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    let sealed_payload = channel.encrypt(&payload_bytes).map_err(|e| e.to_string())?;
    ferry_net::framing::write_frame(&mut stream, &sealed_payload).map_err(|e| e.to_string())?;

    let peer_sealed_payload = ferry_net::framing::read_frame(&mut stream).map_err(|e| e.to_string())?;
    let peer_payload_bytes = channel
        .decrypt(&peer_sealed_payload)
        .map_err(|_| "failed to decrypt the peer's identity — pairing channel is not trustworthy".to_string())?;
    let peer_payload: IdentityPayload =
        serde_json::from_slice(&peer_payload_bytes).map_err(|e| e.to_string())?;

    let remote_key = VerifyingKey::from_bytes(&peer_payload.signing_key)
        .map_err(|e| format!("peer sent a malformed signing key: {e}"))?;
    let phrase = verification_phrase(&local_identity.verifying_key(), &remote_key);

    before_confirm();
    let confirmed = confirm(&phrase);
    let my_ack: [u8; 1] = if confirmed { [1] } else { [0] };
    let sealed_ack = channel.encrypt(&my_ack).map_err(|e| e.to_string())?;
    ferry_net::framing::write_frame(&mut stream, &sealed_ack).map_err(|e| e.to_string())?;

    let peer_sealed_ack = ferry_net::framing::read_frame(&mut stream).map_err(|e| e.to_string())?;
    let peer_ack = channel.decrypt(&peer_sealed_ack).map_err(|e| e.to_string())?;

    if !confirmed {
        return Err("pairing declined locally — the peer was not added".to_string());
    }
    if peer_ack.first() != Some(&1) {
        return Err("the other device did not confirm the verification phrase — the peer was not added".to_string());
    }

    Ok(PairingOutcome {
        peer_id: fingerprint_of(&remote_key),
        display_name: peer_payload.display_name,
        signing_key_hex: hex::encode(peer_payload.signing_key),
        sealing_key: peer_payload.sealing_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    fn loopback_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client_thread = std::thread::spawn(move || TcpStream::connect(addr).unwrap());
        let (server_side, _) = listener.accept().unwrap();
        let client_side = client_thread.join().unwrap();
        (server_side, client_side)
    }

    #[test]
    fn matching_codes_and_mutual_confirmation_pairs_successfully() {
        let (stream_a, stream_b) = loopback_pair();
        let identity_a = Identity::generate();
        let identity_b = Identity::generate();
        let fingerprint_a = identity_a.fingerprint();
        let fingerprint_b = identity_b.fingerprint();

        let thread_b = std::thread::spawn(move || {
            run_pairing_exchange(stream_b, "482913", &identity_b, "phone-b", || {}, |_phrase| true)
        });

        let outcome_a = run_pairing_exchange(stream_a, "482913", &identity_a, "laptop-a", || {}, |_phrase| true).unwrap();
        let outcome_b = thread_b.join().unwrap().unwrap();

        assert_eq!(outcome_a.peer_id, fingerprint_b);
        assert_eq!(outcome_a.display_name, "phone-b");
        assert_eq!(outcome_b.peer_id, fingerprint_a);
        assert_eq!(outcome_b.display_name, "laptop-a");
    }

    #[test]
    fn both_sides_compute_the_same_verification_phrase() {
        let (stream_a, stream_b) = loopback_pair();
        let identity_a = Identity::generate();
        let identity_b = Identity::generate();

        let phrases_b = std::sync::Arc::new(std::sync::Mutex::new(None));
        let phrases_b_clone = phrases_b.clone();
        let thread_b = std::thread::spawn(move || {
            run_pairing_exchange(stream_b, "111111", &identity_b, "b", || {}, |phrase| {
                *phrases_b_clone.lock().unwrap() = Some(phrase.to_string());
                true
            })
        });

        let mut phrase_a = None;
        run_pairing_exchange(stream_a, "111111", &identity_a, "a", || {}, |phrase| {
            phrase_a = Some(phrase.to_string());
            true
        })
        .unwrap();
        thread_b.join().unwrap().unwrap();

        assert_eq!(phrase_a, *phrases_b.lock().unwrap());
        assert!(phrase_a.is_some());
    }

    #[test]
    fn mismatched_codes_fail_before_any_identity_is_exchanged() {
        let (stream_a, stream_b) = loopback_pair();
        let identity_a = Identity::generate();
        let identity_b = Identity::generate();

        let thread_b = std::thread::spawn(move || {
            run_pairing_exchange(stream_b, "000000", &identity_b, "b", || {}, |_| true)
        });

        let result_a = run_pairing_exchange(stream_a, "999999", &identity_a, "a", || {}, |_| true);
        assert!(result_a.is_err());
        let _ = thread_b.join().unwrap();
    }

    #[test]
    fn a_peer_that_connects_but_never_speaks_fails_with_an_error_rather_than_hanging() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let _silent = std::thread::spawn(move || {
            let (_s, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(3));
        });

        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_millis(400)))
            .unwrap();
        stream
            .set_write_timeout(Some(std::time::Duration::from_millis(400)))
            .unwrap();

        let started = std::time::Instant::now();
        let result = run_pairing_exchange(stream, "424242", &Identity::generate(), "a", || {}, |_| true);
        assert!(result.is_err(), "a silent peer must not pair");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the read timeout must bound the wait, not hang"
        );
    }

    #[test]
    fn declining_locally_never_adds_the_peer_and_tells_the_other_side() {
        let (stream_a, stream_b) = loopback_pair();
        let identity_a = Identity::generate();
        let identity_b = Identity::generate();

        let thread_b = std::thread::spawn(move || {
            run_pairing_exchange(stream_b, "555555", &identity_b, "b", || {}, |_| true)
        });

        let result_a = run_pairing_exchange(stream_a, "555555", &identity_a, "a", || {}, |_| false);
        assert!(result_a.is_err(), "declining locally must not yield a peer to add");

        let result_b = thread_b.join().unwrap();
        assert!(result_b.is_err(), "the other side must learn the decline and also not add the peer");
    }
}
