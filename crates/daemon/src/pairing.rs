use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferry_core::pairing::{generate_code, run_pairing_exchange, PairingOutcome};
use ferry_core::ports::NewRosterPeer;
use ferry_crypto::identity::Identity;
use ferry_proto::ipc::{PairBeginView, PairMode, PairPhase, PairStatusView};

const CONFIRM_TIMEOUT: Duration = Duration::from_secs(300);

struct Slot {
    phase: PairPhase,
    phrase: Option<String>,
    peer_fingerprint: Option<String>,
    peer_display_name: Option<String>,
    error: Option<String>,
    answer_tx: Option<SyncSender<bool>>,
    outcome: Option<PairingOutcome>,
    persisted: bool,
}

impl Slot {
    fn new() -> Self {
        Slot {
            phase: PairPhase::AwaitingPeer,
            phrase: None,
            peer_fingerprint: None,
            peer_display_name: None,
            error: None,
            answer_tx: None,
            outcome: None,
            persisted: false,
        }
    }

    fn view(&self) -> PairStatusView {
        PairStatusView {
            phase: self.phase,
            phrase: self.phrase.clone(),
            peer_fingerprint: self.peer_fingerprint.clone(),
            peer_display_name: self.peer_display_name.clone(),
            error: self.error.clone(),
        }
    }
}

pub struct PairingRegistry {
    identity: Identity,
    slots: Mutex<HashMap<String, Arc<Mutex<Slot>>>>,
}

impl PairingRegistry {
    pub fn new(identity: Identity) -> Self {
        Self {
            identity,
            slots: Mutex::new(HashMap::new()),
        }
    }

    pub fn begin(&self, mode: PairMode) -> Result<PairBeginView, String> {
        let pairing_id = uuid::Uuid::now_v7().to_string();
        let slot = Arc::new(Mutex::new(Slot::new()));
        self.slots.lock().unwrap().insert(pairing_id.clone(), slot.clone());

        let (listen_addr, code, listener) = match &mode {
            PairMode::Listen { .. } => {
                let l = TcpListener::bind("0.0.0.0:0")
                    .map_err(|e| format!("could not bind a pairing listener: {e}"))?;
                let port = l.local_addr().map_err(|e| e.to_string())?.port();
                (Some(format!("{}:{}", advertised_ip(), port)), generate_code(), Some(l))
            }
            PairMode::Connect { code, .. } => (None, code.clone(), None),
        };

        let identity = self.identity.clone();
        let slot_thread = slot.clone();
        let mode_thread = mode.clone();
        let code_thread = code.clone();
        std::thread::spawn(move || {
            let name = match &mode_thread {
                PairMode::Listen { display_name } | PairMode::Connect { display_name, .. } => display_name.clone(),
            };
            let stream: Result<TcpStream, String> = match (&mode_thread, listener) {
                (PairMode::Listen { .. }, Some(l)) => {
                    l.accept().map(|(s, _)| s).map_err(|e| format!("no peer connected: {e}"))
                }
                (PairMode::Connect { addr, .. }, _) => {
                    match ferry_net::addr::normalize_host_port(addr) {
                        Ok(target) => TcpStream::connect(&target)
                            .map_err(|e| format!("could not reach {target}: {e}")),
                        Err(e) => Err(e),
                    }
                }
                _ => Err("internal pairing setup error".to_string()),
            };
            let stream = match stream {
                Ok(s) => s,
                Err(e) => return fail(&slot_thread, e),
            };

            let confirm = |phrase: &str| -> bool {
                let (tx, rx) = sync_channel::<bool>(1);
                {
                    let mut s = slot_thread.lock().unwrap();
                    s.phase = PairPhase::AwaitingConfirmation;
                    s.phrase = Some(phrase.to_string());
                    s.answer_tx = Some(tx);
                }
                rx.recv_timeout(CONFIRM_TIMEOUT).unwrap_or(false)
            };

            match run_pairing_exchange(stream, &code_thread, &identity, &name, confirm) {
                Ok(outcome) => {
                    let mut s = slot_thread.lock().unwrap();
                    s.peer_fingerprint = Some(outcome.peer_id.clone());
                    s.peer_display_name = Some(outcome.display_name.clone());
                    s.outcome = Some(outcome);
                    s.phase = PairPhase::Done;
                }
                Err(e) => fail(&slot_thread, e),
            }
        });

        Ok(PairBeginView {
            pairing_id,
            listen_addr,
            code: match mode {
                PairMode::Listen { .. } => Some(code),
                PairMode::Connect { .. } => None,
            },
        })
    }

    pub fn status_view(&self, pairing_id: &str) -> Option<PairStatusView> {
        let slot = self.slots.lock().unwrap().get(pairing_id).cloned()?;
        let view = slot.lock().unwrap().view();
        Some(view)
    }

    pub fn take_pending_persist(&self, pairing_id: &str) -> Option<NewRosterPeer> {
        let slot = self.slots.lock().unwrap().get(pairing_id).cloned()?;
        let mut s = slot.lock().unwrap();
        if s.persisted {
            return None;
        }
        let outcome = s.outcome.take()?;
        s.persisted = true;
        Some(NewRosterPeer {
            peer_id: outcome.peer_id,
            display_name: outcome.display_name,
            signing_key_hex: outcome.signing_key_hex,
            sealing_key: outcome.sealing_key,
        })
    }

    pub fn confirm(&self, pairing_id: &str, accept: bool) -> Result<(), String> {
        let slot = self
            .slots
            .lock()
            .unwrap()
            .get(pairing_id)
            .cloned()
            .ok_or("no such pairing session")?;
        let tx = slot.lock().unwrap().answer_tx.take();
        match tx {
            Some(tx) => {
                let _ = tx.send(accept);
                Ok(())
            }
            None => Err("this pairing session is not waiting for a verification-phrase confirmation".to_string()),
        }
    }

    pub fn cancel(&self, pairing_id: &str) {
        if let Some(slot) = self.slots.lock().unwrap().remove(pairing_id) {
            let tx = slot.lock().unwrap().answer_tx.take();
            if let Some(tx) = tx {
                let _ = tx.send(false);
            }
        }
    }
}

fn advertised_ip() -> IpAddr {
    let probe = || -> Option<IpAddr> {
        let s = UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect("8.8.8.8:80").ok()?;
        Some(s.local_addr().ok()?.ip())
    };
    probe().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

fn fail(slot: &Arc<Mutex<Slot>>, msg: String) {
    let mut s = slot.lock().unwrap();
    s.phase = PairPhase::Failed;
    s.error = Some(msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listen_then_connect_pairing_completes_and_both_sides_persist_the_peer() {
        let reg_a = PairingRegistry::new(Identity::generate());
        let reg_b = PairingRegistry::new(Identity::generate());

        let begin_a = reg_a
            .begin(PairMode::Listen { display_name: "machine-a".into() })
            .unwrap();
        let addr = begin_a.listen_addr.clone().unwrap();
        let code = begin_a.code.clone().unwrap();

        let begin_b = reg_b
            .begin(PairMode::Connect {
                addr,
                code,
                display_name: "machine-b".into(),
            })
            .unwrap();

        let wait_for_confirmation = |reg: &PairingRegistry, id: &str| {
            for _ in 0..500 {
                let v = reg.status_view(id).unwrap();
                match v.phase {
                    PairPhase::AwaitingConfirmation => return v,
                    PairPhase::Failed => panic!("pairing failed: {:?}", v.error),
                    _ => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            panic!("pairing never reached the confirmation phase");
        };

        let view_a = wait_for_confirmation(&reg_a, &begin_a.pairing_id);
        let view_b = wait_for_confirmation(&reg_b, &begin_b.pairing_id);
        assert_eq!(view_a.phrase, view_b.phrase);
        assert!(view_a.phrase.is_some());

        reg_a.confirm(&begin_a.pairing_id, true).unwrap();
        reg_b.confirm(&begin_b.pairing_id, true).unwrap();

        let wait_for_done = |reg: &PairingRegistry, id: &str| {
            for _ in 0..500 {
                match reg.status_view(id).unwrap().phase {
                    PairPhase::Done => return,
                    PairPhase::Failed => panic!("pairing failed after confirmation"),
                    _ => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            panic!("pairing never completed");
        };
        wait_for_done(&reg_a, &begin_a.pairing_id);
        wait_for_done(&reg_b, &begin_b.pairing_id);

        let peer_a_sees = reg_a.take_pending_persist(&begin_a.pairing_id).unwrap();
        assert_eq!(peer_a_sees.display_name, "machine-b");
        assert!(reg_a.take_pending_persist(&begin_a.pairing_id).is_none());
        let peer_b_sees = reg_b.take_pending_persist(&begin_b.pairing_id).unwrap();
        assert_eq!(peer_b_sees.display_name, "machine-a");
    }

    #[test]
    fn declining_the_phrase_fails_the_pairing_on_both_sides() {
        let reg_a = PairingRegistry::new(Identity::generate());
        let reg_b = PairingRegistry::new(Identity::generate());

        let begin_a = reg_a.begin(PairMode::Listen { display_name: "a".into() }).unwrap();
        let begin_b = reg_b
            .begin(PairMode::Connect {
                addr: begin_a.listen_addr.clone().unwrap(),
                code: begin_a.code.clone().unwrap(),
                display_name: "b".into(),
            })
            .unwrap();

        let wait = |reg: &PairingRegistry, id: &str, want: PairPhase| {
            for _ in 0..500 {
                let phase = reg.status_view(id).unwrap().phase;
                if phase == want {
                    return;
                }
                assert_ne!(phase, PairPhase::Done);
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("pairing never reached {want:?}");
        };

        wait(&reg_a, &begin_a.pairing_id, PairPhase::AwaitingConfirmation);
        wait(&reg_b, &begin_b.pairing_id, PairPhase::AwaitingConfirmation);

        reg_a.confirm(&begin_a.pairing_id, false).unwrap();
        reg_b.confirm(&begin_b.pairing_id, true).unwrap();

        wait(&reg_a, &begin_a.pairing_id, PairPhase::Failed);
        wait(&reg_b, &begin_b.pairing_id, PairPhase::Failed);
        assert!(reg_a.take_pending_persist(&begin_a.pairing_id).is_none());
        assert!(reg_b.take_pending_persist(&begin_b.pairing_id).is_none());
    }

}
