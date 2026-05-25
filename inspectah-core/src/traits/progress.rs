use crate::types::progress::ProgressEvent;
use std::sync::Mutex;

/// Sink for progress events emitted during scan collection.
/// `Send + Sync` required because wave-2 inspectors run in parallel
/// via `std::thread::scope` and the sink is shared across scoped threads.
pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: ProgressEvent);
}

/// No-op progress sink for library consumers who don't need progress.
pub struct NullProgress;

impl ProgressSink for NullProgress {
    fn emit(&self, _event: ProgressEvent) {}
}

/// Test utility that collects events. Thread-safe via `Mutex`.
pub struct VecProgress {
    events: Mutex<Vec<ProgressEvent>>,
}

impl VecProgress {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    pub fn events(&self) -> Vec<ProgressEvent> {
        self.events
            .lock()
            .expect("VecProgress lock poisoned")
            .clone()
    }
}

impl Default for VecProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressSink for VecProgress {
    fn emit(&self, event: ProgressEvent) {
        self.events
            .lock()
            .expect("VecProgress lock poisoned")
            .push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::completeness::InspectorId;
    use crate::types::progress::{InspectorOutcome, ProgressEvent};

    #[test]
    fn null_progress_accepts_events() {
        let sink = NullProgress;
        sink.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
    }

    #[test]
    fn vec_progress_collects_events() {
        let sink = VecProgress::new();
        sink.emit(ProgressEvent::InspectorStarted(InspectorId::Rpm));
        sink.emit(ProgressEvent::InspectorFinished {
            id: InspectorId::Rpm,
            outcome: InspectorOutcome::Complete,
        });
        let events = sink.events();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            ProgressEvent::InspectorStarted(InspectorId::Rpm)
        ));
    }

    #[test]
    fn vec_progress_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VecProgress>();
    }
}
