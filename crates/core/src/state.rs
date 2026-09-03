use ferry_proto::states::{MessageState, PeerState, TransferState};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("illegal transition from {from:?} to {to:?}")]
pub struct TransitionError<S: std::fmt::Debug> {
    pub from: S,
    pub to: S,
}

pub struct TransitionTable<S: 'static> {
    edges: &'static [(S, S)],
}

impl<S: Copy + PartialEq + std::fmt::Debug + 'static> TransitionTable<S> {
    pub const fn new(edges: &'static [(S, S)]) -> Self {
        Self { edges }
    }

    pub fn validate(&self, from: S, to: S) -> Result<(), TransitionError<S>> {
        if self.edges.iter().any(|(f, t)| *f == from && *t == to) {
            Ok(())
        } else {
            Err(TransitionError { from, to })
        }
    }

    pub fn is_terminal(&self, state: S) -> bool {
        !self.edges.iter().any(|(f, _)| *f == state)
    }
}

pub const TRANSFER_TRANSITIONS: TransitionTable<TransferState> = TransitionTable::new(&[
    (TransferState::Queued, TransferState::Offered),
    (TransferState::Queued, TransferState::Expired),
    (TransferState::Queued, TransferState::Failed),
    (TransferState::Offered, TransferState::Accepted),
    (TransferState::Offered, TransferState::Expired),
    (TransferState::Offered, TransferState::Failed),
    (TransferState::Accepted, TransferState::Transferring),
    (TransferState::Accepted, TransferState::Expired),
    (TransferState::Accepted, TransferState::Failed),
    (TransferState::Transferring, TransferState::Delivered),
    (TransferState::Transferring, TransferState::Expired),
    (TransferState::Transferring, TransferState::Failed),
    (TransferState::Delivered, TransferState::Opened),
    (TransferState::Delivered, TransferState::Expired),
]);

pub const MESSAGE_TRANSITIONS: TransitionTable<MessageState> = TransitionTable::new(&[
    (MessageState::Queued, MessageState::Sent),
    (MessageState::Queued, MessageState::Failed),
    (MessageState::Sent, MessageState::Delivered),
    (MessageState::Sent, MessageState::Failed),
]);

pub const PEER_TRANSITIONS: TransitionTable<PeerState> = TransitionTable::new(&[
    (PeerState::PendingVerification, PeerState::Paired),
    (PeerState::PendingVerification, PeerState::Removed),
    (PeerState::Paired, PeerState::Removed),
]);

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_TRANSFER_STATES: [TransferState; 8] = [
        TransferState::Queued,
        TransferState::Offered,
        TransferState::Accepted,
        TransferState::Transferring,
        TransferState::Delivered,
        TransferState::Opened,
        TransferState::Expired,
        TransferState::Failed,
    ];

    const ALL_MESSAGE_STATES: [MessageState; 4] = [
        MessageState::Queued,
        MessageState::Sent,
        MessageState::Delivered,
        MessageState::Failed,
    ];

    const ALL_PEER_STATES: [PeerState; 3] = [
        PeerState::PendingVerification,
        PeerState::Paired,
        PeerState::Removed,
    ];

    // Independently re-derived expectation (not a copy-paste of the
    // production table) — every pair not listed here must be illegal.
    fn expected_legal_transfer_edges() -> Vec<(TransferState, TransferState)> {
        use TransferState::*;
        vec![
            (Queued, Offered),
            (Queued, Expired),
            (Queued, Failed),
            (Offered, Accepted),
            (Offered, Expired),
            (Offered, Failed),
            (Accepted, Transferring),
            (Accepted, Expired),
            (Accepted, Failed),
            (Transferring, Delivered),
            (Transferring, Expired),
            (Transferring, Failed),
            (Delivered, Opened),
            (Delivered, Expired),
        ]
    }

    #[test]
    fn transfer_state_every_pair_matches_hand_derived_expectation() {
        let expected = expected_legal_transfer_edges();
        let mut checked = 0;
        for &from in &ALL_TRANSFER_STATES {
            for &to in &ALL_TRANSFER_STATES {
                let should_be_legal = expected.contains(&(from, to));
                let is_legal = TRANSFER_TRANSITIONS.validate(from, to).is_ok();
                assert_eq!(
                    is_legal, should_be_legal,
                    "transition {from:?} -> {to:?}: expected legal={should_be_legal}, got={is_legal}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, ALL_TRANSFER_STATES.len() * ALL_TRANSFER_STATES.len());
    }

    #[test]
    fn no_transfer_state_transitions_to_itself() {
        for &state in &ALL_TRANSFER_STATES {
            assert!(
                TRANSFER_TRANSITIONS.validate(state, state).is_err(),
                "{state:?} -> {state:?} should be illegal"
            );
        }
    }

    #[test]
    fn opened_expired_and_failed_are_terminal() {
        assert!(TRANSFER_TRANSITIONS.is_terminal(TransferState::Opened));
        assert!(TRANSFER_TRANSITIONS.is_terminal(TransferState::Expired));
        assert!(TRANSFER_TRANSITIONS.is_terminal(TransferState::Failed));
    }

    #[test]
    fn non_terminal_transfer_states_have_at_least_one_legal_edge() {
        for &state in &[
            TransferState::Queued,
            TransferState::Offered,
            TransferState::Accepted,
            TransferState::Transferring,
            TransferState::Delivered,
        ] {
            assert!(
                !TRANSFER_TRANSITIONS.is_terminal(state),
                "{state:?} should not be terminal"
            );
        }
    }

    #[test]
    fn cannot_skip_straight_from_queued_to_delivered() {
        let result = TRANSFER_TRANSITIONS.validate(TransferState::Queued, TransferState::Delivered);
        assert_eq!(
            result,
            Err(TransitionError {
                from: TransferState::Queued,
                to: TransferState::Delivered,
            })
        );
    }

    #[test]
    fn cannot_reopen_or_revive_a_terminal_transfer_state() {
        assert!(TRANSFER_TRANSITIONS
            .validate(TransferState::Opened, TransferState::Queued)
            .is_err());
        assert!(TRANSFER_TRANSITIONS
            .validate(TransferState::Failed, TransferState::Queued)
            .is_err());
        assert!(TRANSFER_TRANSITIONS
            .validate(TransferState::Expired, TransferState::Offered)
            .is_err());
    }

    #[test]
    fn message_state_legal_edges_and_terminals() {
        assert!(MESSAGE_TRANSITIONS
            .validate(MessageState::Queued, MessageState::Sent)
            .is_ok());
        assert!(MESSAGE_TRANSITIONS
            .validate(MessageState::Sent, MessageState::Delivered)
            .is_ok());
        assert!(MESSAGE_TRANSITIONS
            .validate(MessageState::Queued, MessageState::Delivered)
            .is_err());

        for &state in &ALL_MESSAGE_STATES {
            assert!(
                MESSAGE_TRANSITIONS.validate(state, state).is_err(),
                "{state:?} -> {state:?} should be illegal"
            );
        }
        assert!(MESSAGE_TRANSITIONS.is_terminal(MessageState::Delivered));
        assert!(MESSAGE_TRANSITIONS.is_terminal(MessageState::Failed));
    }

    #[test]
    fn peer_state_legal_edges_and_terminals() {
        assert!(PEER_TRANSITIONS
            .validate(PeerState::PendingVerification, PeerState::Paired)
            .is_ok());
        assert!(PEER_TRANSITIONS
            .validate(PeerState::Paired, PeerState::Removed)
            .is_ok());
        assert!(PEER_TRANSITIONS
            .validate(PeerState::Removed, PeerState::Paired)
            .is_err());

        for &state in &ALL_PEER_STATES {
            assert!(
                PEER_TRANSITIONS.validate(state, state).is_err(),
                "{state:?} -> {state:?} should be illegal"
            );
        }
        assert!(PEER_TRANSITIONS.is_terminal(PeerState::Removed));
    }
}
