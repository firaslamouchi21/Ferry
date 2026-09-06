use std::io::{Read, Write};

use snow::{Builder, HandshakeState, TransportState};
use thiserror::Error;

use crate::framing::{self, FramingError, MAX_FRAME_BYTES};

const NOISE_PATTERN: &str = "Noise_XK_25519_ChaChaPoly_BLAKE2s";
const AEAD_TAG_BYTES: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct StaticKeypair {
    pub private: [u8; 32],
    pub public: [u8; 32],
}

pub fn generate_keypair() -> StaticKeypair {
    let params = NOISE_PATTERN.parse().expect("hardcoded noise pattern is valid");
    let keypair = Builder::new(params)
        .generate_keypair()
        .expect("keypair generation must succeed");
    StaticKeypair {
        private: keypair.private[..32].try_into().expect("x25519 private key is 32 bytes"),
        public: keypair.public[..32].try_into().expect("x25519 public key is 32 bytes"),
    }
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("noise protocol error: {0}")]
    Noise(#[from] snow::Error),
    #[error("framing error: {0}")]
    Framing(#[from] FramingError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("remote peer is not authorized — its static key is not in the roster")]
    Unauthorized,
    #[error("handshake completed without a usable remote static key")]
    MissingRemoteStatic,
    #[error("payload of {0} bytes is too large for a single transport message")]
    PayloadTooLarge(usize),
}

pub struct AuthenticatedChannel<S> {
    stream: S,
    transport: TransportState,
    pub remote_static: [u8; 32],
}

impl<S: Read + Write> AuthenticatedChannel<S> {
    pub fn send(&mut self, plaintext: &[u8]) -> Result<(), TransportError> {
        if plaintext.len() + AEAD_TAG_BYTES > MAX_FRAME_BYTES {
            return Err(TransportError::PayloadTooLarge(plaintext.len()));
        }
        let mut buf = vec![0u8; plaintext.len() + AEAD_TAG_BYTES];
        let len = self.transport.write_message(plaintext, &mut buf)?;
        framing::write_frame(&mut self.stream, &buf[..len])?;
        Ok(())
    }

    pub fn recv(&mut self) -> Result<Vec<u8>, TransportError> {
        let frame = framing::read_frame(&mut self.stream)?;
        let mut buf = vec![0u8; frame.len()];
        let len = self.transport.read_message(&frame, &mut buf)?;
        buf.truncate(len);
        Ok(buf)
    }
}

pub fn connect_initiator<S: Read + Write>(
    mut stream: S,
    local: &StaticKeypair,
    remote_static_public: &[u8; 32],
    authorizer: impl Fn(&[u8; 32]) -> bool,
) -> Result<AuthenticatedChannel<S>, TransportError> {
    let params = NOISE_PATTERN.parse().expect("hardcoded noise pattern is valid");
    let mut handshake = Builder::new(params)
        .local_private_key(&local.private)?
        .remote_public_key(remote_static_public)?
        .build_initiator()?;

    let mut buf = vec![0u8; MAX_FRAME_BYTES];

    let len = handshake.write_message(&[], &mut buf)?;
    framing::write_frame(&mut stream, &buf[..len])?;

    let msg = framing::read_frame(&mut stream)?;
    handshake.read_message(&msg, &mut buf)?;

    let len = handshake.write_message(&[], &mut buf)?;
    framing::write_frame(&mut stream, &buf[..len])?;

    finish_handshake(stream, handshake, authorizer)
}

pub fn accept_responder<S: Read + Write>(
    mut stream: S,
    local: &StaticKeypair,
    authorizer: impl Fn(&[u8; 32]) -> bool,
) -> Result<AuthenticatedChannel<S>, TransportError> {
    let params = NOISE_PATTERN.parse().expect("hardcoded noise pattern is valid");
    let mut handshake = Builder::new(params)
        .local_private_key(&local.private)?
        .build_responder()?;

    let mut buf = vec![0u8; MAX_FRAME_BYTES];

    let msg = framing::read_frame(&mut stream)?;
    handshake.read_message(&msg, &mut buf)?;

    let len = handshake.write_message(&[], &mut buf)?;
    framing::write_frame(&mut stream, &buf[..len])?;

    let msg = framing::read_frame(&mut stream)?;
    handshake.read_message(&msg, &mut buf)?;

    finish_handshake(stream, handshake, authorizer)
}

fn finish_handshake<S: Read + Write>(
    stream: S,
    handshake: HandshakeState,
    authorizer: impl Fn(&[u8; 32]) -> bool,
) -> Result<AuthenticatedChannel<S>, TransportError> {
    let remote_static: [u8; 32] = handshake
        .get_remote_static()
        .ok_or(TransportError::MissingRemoteStatic)?
        .try_into()
        .map_err(|_| TransportError::MissingRemoteStatic)?;

    if !authorizer(&remote_static) {
        return Err(TransportError::Unauthorized);
    }

    let transport = handshake.into_transport_mode()?;
    Ok(AuthenticatedChannel {
        stream,
        transport,
        remote_static,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn stream_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let a = TcpStream::connect(addr).unwrap();
        let (b, _) = listener.accept().unwrap();
        (a, b)
    }

    #[test]
    fn authorized_peers_complete_handshake_and_exchange_messages() {
        let (initiator_sock, responder_sock) = stream_pair();
        let initiator_keys = generate_keypair();
        let responder_keys = generate_keypair();
        let responder_public = responder_keys.public;
        let initiator_public = initiator_keys.public;

        let responder_thread = thread::spawn(move || {
            let mut channel =
                accept_responder(responder_sock, &responder_keys, move |remote| {
                    *remote == initiator_public
                })
                .unwrap();
            let received = channel.recv().unwrap();
            channel.send(b"pong").unwrap();
            received
        });

        let mut channel = connect_initiator(initiator_sock, &initiator_keys, &responder_public, |_| true)
            .unwrap();
        channel.send(b"ping").unwrap();
        let reply = channel.recv().unwrap();

        assert_eq!(reply, b"pong");
        assert_eq!(responder_thread.join().unwrap(), b"ping");
        assert_eq!(channel.remote_static, responder_public);
    }

    #[test]
    fn responder_rejects_a_peer_whose_static_key_is_not_authorized() {
        let (initiator_sock, responder_sock) = stream_pair();
        let initiator_keys = generate_keypair();
        let responder_keys = generate_keypair();
        let responder_public = responder_keys.public;

        let responder_thread = thread::spawn(move || {
            accept_responder(responder_sock, &responder_keys, |_remote| false)
        });

        let initiator_result =
            connect_initiator(initiator_sock, &initiator_keys, &responder_public, |_| true);

        let responder_result = responder_thread.join().unwrap();
        assert!(matches!(
            responder_result,
            Err(TransportError::Unauthorized)
        ));
        // The initiator's final handshake write can succeed even though the
        // responder then rejects post-handshake — either the initiator sees
        // the dropped connection on its own next read, or it never gets to
        // send application data at all. What matters is that the responder
        // never entered transport mode.
        let _ = initiator_result;
    }

    #[test]
    fn oversize_payload_is_rejected_before_sending() {
        let (initiator_sock, responder_sock) = stream_pair();
        let initiator_keys = generate_keypair();
        let responder_keys = generate_keypair();
        let responder_public = responder_keys.public;

        let responder_thread =
            thread::spawn(move || accept_responder(responder_sock, &responder_keys, |_| true));

        let mut channel =
            connect_initiator(initiator_sock, &initiator_keys, &responder_public, |_| true).unwrap();
        responder_thread.join().unwrap().unwrap();

        let oversized = vec![0u8; MAX_FRAME_BYTES];
        let result = channel.send(&oversized);
        assert!(matches!(result, Err(TransportError::PayloadTooLarge(_))));
    }

    #[test]
    fn responder_errors_on_an_oversize_declared_frame_length_without_reading_the_body() {
        use std::io::Write;
        use std::time::{Duration, Instant};

        let (initiator_sock, responder_sock) = stream_pair();
        let initiator_keys = generate_keypair();
        let responder_keys = generate_keypair();
        let responder_public = responder_keys.public;
        let initiator_public = initiator_keys.public;

        let mut raw_wire = initiator_sock.try_clone().unwrap();

        let responder_thread = thread::spawn(move || {
            let mut channel =
                accept_responder(responder_sock, &responder_keys, move |remote| *remote == initiator_public)
                    .unwrap();
            channel.recv()
        });

        let _initiator = connect_initiator(initiator_sock, &initiator_keys, &responder_public, |_| true).unwrap();

        raw_wire
            .write_all(&((MAX_FRAME_BYTES as u32) + 1).to_be_bytes())
            .unwrap();

        let started = Instant::now();
        let result = responder_thread.join().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "an oversize declared length must fail fast, never block waiting for a body that will not come"
        );
        assert!(matches!(
            result,
            Err(TransportError::Framing(FramingError::OversizeFrame(..)))
        ));
    }
}
