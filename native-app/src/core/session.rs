//! Atomic capture lifecycle gate shared by UI callbacks and worker threads.

use std::sync::atomic::{AtomicU8, Ordering};

const IDLE: u8 = 0;
const CAPTURING: u8 = 1;
const SELECTING: u8 = 2;
const ANNOTATING: u8 = 3;
const SAVING: u8 = 4;

#[derive(Debug, Default)]
pub struct CaptureSession {
    phase: AtomicU8,
}

impl CaptureSession {
    pub fn begin_capture(&self) -> bool {
        self.phase
            .compare_exchange(IDLE, CAPTURING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn capture_ready(&self) -> bool {
        self.phase
            .compare_exchange(CAPTURING, SELECTING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn begin_saving(&self) -> bool {
        self.phase
            .compare_exchange(ANNOTATING, SAVING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn begin_annotation(&self) -> bool {
        self.phase
            .compare_exchange(SELECTING, ANNOTATING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Reset the gate on success or failure, allowing the next capture.
    pub fn finish(&self) {
        self.phase.store(IDLE, Ordering::Release);
    }

    /// Cancel an active session and report whether there was one to cancel.
    pub fn cancel(&self) -> bool {
        self.phase.swap(IDLE, Ordering::AcqRel) != IDLE
    }

    pub fn is_active(&self) -> bool {
        self.phase.load(Ordering::Acquire) != IDLE
    }
}

#[cfg(test)]
mod tests {
    use super::CaptureSession;
    use std::sync::{Arc, Barrier};

    #[test]
    fn allows_only_one_concurrent_capture() {
        let session = Arc::new(CaptureSession::default());
        let barrier = Arc::new(Barrier::new(8));
        let workers = (0..8)
            .map(|_| {
                let session = Arc::clone(&session);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    session.begin_capture()
                })
            })
            .collect::<Vec<_>>();
        let started = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|started| *started)
            .count();

        assert_eq!(started, 1);
        assert!(session.is_active());
    }

    #[test]
    fn success_cancel_and_error_cleanup_allow_repeated_sessions() {
        let session = CaptureSession::default();

        assert!(session.begin_capture());
        assert!(session.capture_ready());
        assert!(session.begin_annotation());
        assert!(session.begin_saving());
        session.finish();
        assert!(!session.is_active());

        assert!(session.begin_capture());
        assert!(session.capture_ready());
        assert!(session.begin_annotation());
        assert!(session.cancel());
        assert!(!session.is_active());

        assert!(session.begin_capture());
        session.finish(); // Capture error cleanup.
        assert!(session.begin_capture());
        assert!(session.capture_ready());
        assert!(session.begin_annotation());
        assert!(session.begin_saving());
        session.finish();
        assert!(!session.is_active());
    }

    #[test]
    fn rejects_out_of_order_transitions() {
        let session = CaptureSession::default();
        assert!(!session.capture_ready());
        assert!(!session.begin_saving());
        assert!(!session.cancel());
        assert!(session.begin_capture());
        assert!(!session.begin_capture());
        assert!(!session.begin_annotation());
    }
}
