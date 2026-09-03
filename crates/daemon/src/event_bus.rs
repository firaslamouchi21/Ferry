use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Mutex;

use ferry_proto::ipc::IpcEvent;

pub type EventSink<'a> = dyn Fn(IpcEvent) + 'a;

pub fn noop_sink(_: IpcEvent) {}

const QUEUE_BOUND: usize = 256;

#[derive(Default)]
pub struct EventBus {
    subscribers: Mutex<Vec<SyncSender<IpcEvent>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self) -> Receiver<IpcEvent> {
        let (tx, rx) = mpsc::sync_channel(QUEUE_BOUND);
        self.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(tx);
        rx
    }

    pub fn emit(&self, event: IpcEvent) {
        let is_progress = matches!(event, IpcEvent::Progress { .. });
        let mut subscribers = self.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        subscribers.retain(|tx| match tx.try_send(event.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => is_progress,
            Err(TrySendError::Disconnected(_)) => false,
        });
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_proto::ipc::IpcResource;

    fn changed(id: &str) -> IpcEvent {
        IpcEvent::Changed {
            resource: IpcResource::Transfer,
            id: Some(id.to_string()),
        }
    }

    fn progress(bytes: u64) -> IpcEvent {
        IpcEvent::Progress {
            item_id: "item-1".into(),
            bytes,
            total: 1000,
        }
    }

    #[test]
    fn an_emitted_event_reaches_every_live_subscriber() {
        let bus = EventBus::new();
        let a = bus.subscribe();
        let b = bus.subscribe();

        bus.emit(changed("item-1"));

        assert_eq!(a.recv().unwrap(), changed("item-1"));
        assert_eq!(b.recv().unwrap(), changed("item-1"));
    }

    #[test]
    fn a_dropped_subscriber_is_pruned_and_does_not_break_emission() {
        let bus = EventBus::new();
        let live = bus.subscribe();
        {
            let _dead = bus.subscribe();
        }
        assert_eq!(bus.subscriber_count(), 2);

        bus.emit(changed("item-1"));

        assert_eq!(bus.subscriber_count(), 1);
        assert_eq!(live.recv().unwrap(), changed("item-1"));
    }

    #[test]
    fn emitting_with_no_subscribers_is_a_no_op() {
        let bus = EventBus::new();
        bus.emit(changed("item-1"));
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[test]
    fn a_subscriber_whose_queue_overflows_with_progress_keeps_its_connection() {
        let bus = EventBus::new();
        let _rx = bus.subscribe();

        for n in 0..(QUEUE_BOUND as u64 + 50) {
            bus.emit(progress(n));
        }

        assert_eq!(
            bus.subscriber_count(),
            1,
            "overflowing progress events must be dropped, not the connection"
        );
    }

    #[test]
    fn a_subscriber_whose_queue_cannot_take_a_changed_event_is_dropped() {
        let bus = EventBus::new();
        let _rx = bus.subscribe();

        for _ in 0..QUEUE_BOUND {
            bus.emit(changed("filler"));
        }
        assert_eq!(bus.subscriber_count(), 1);

        bus.emit(changed("overflow"));
        assert_eq!(
            bus.subscriber_count(),
            0,
            "a connection that cannot receive a state-change event is dropped so it reconnects and re-reads"
        );
    }
}
